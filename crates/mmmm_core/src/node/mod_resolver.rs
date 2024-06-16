use std::{
    collections::HashMap,
    sync::Arc,
    thread::{spawn, JoinHandle},
};

use api_client::{
    common::{download_file, ApiError, DownloadError},
    curse::{model::HashAlgo, CurseClient},
    modrinth::ModrinthClient,
};
use digest::Digest;
use md5::Md5;
use rayon::prelude::*;
use serde::Deserialize;
use sha2::Sha256;
use thiserror::Error;
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::ResultExt;
use urlencoding::encode;

use crate::{
    di::container::{DiContainer, InputType, OutputType},
    Cache, CacheError,
};

use super::{
    config::{ChannelId, ModDefinition, ModDefinitionFields, NodeConfig, NodeInitError, ResolvedMod},
    utils::{get_input, get_output},
};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModResolver;

impl NodeConfig for ModResolver {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let mut mod_channel = get_input!("mods", Mods, ctx, input_ids)?;
        let out_channel = get_output!(ChannelId(node_id.clone(), "default".into()), Text, ctx)?;
        let json_out = get_output!(ChannelId(node_id.clone(), "json".into()), Text, ctx)?;

        let mut waker = ctx.get_waker();

        let minecraft_version = ctx
            .get_config("minecraft_version")
            .ok_or_else(|| NodeInitError::MissingConfig("minecraft_version".into()))?;
        let modloader = ctx
            .get_config("modloader")
            .ok_or_else(|| NodeInitError::MissingConfig("modloader".into()))?;

        let curse_client_option = ctx.get_curse_client();
        let modrinth_client = ctx.get_modrinth_client();
        let cache = ctx.get_cache();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "ModResolver", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let mods = mod_channel.blocking_recv().expect_or_log("Failed to receive on mods input");
            event!(Level::INFO, "Got {} mods to resolve", mods.len());

            // Check if the Curse API is needed, but the client wasn't configured. Logs an error
            // message then terminates the thread, so if execution continues past this block we know it is safe to
            // call .unwrap() on the Curse client Option.
            if mods.iter().any(|m| matches!(m, ModDefinition::Curse { .. })) && curse_client_option.is_none() {
                let curse_mods: Vec<String> = mods
                    .into_iter()
                    .filter_map(|m| match m {
                        ModDefinition::Curse { fields, .. } => Some(fields.name),
                        ModDefinition::Modrinth { .. } => None,
                        ModDefinition::Url { .. } => None,
                    })
                    .collect();
                let modlist = curse_mods.join(", ");
                event!(Level::ERROR, "Curse client not set up! Cannot resolve mods: {modlist}");
                panic!();
            }

            let resolved: Vec<ResolvedMod> = mods
                .into_par_iter()
                .map(|mod_def| match mod_def {
                    ModDefinition::Modrinth { id, file_id, fields } => {
                        resolve_modrinth(&modrinth_client, id, file_id, fields, &minecraft_version, &modloader, &cache)
                            .expect_or_log("Failed to resolve Modrinth mod")
                    },
                    ModDefinition::Curse { id, file_id, fields } => resolve_curse(
                        curse_client_option.as_ref().unwrap(),
                        id,
                        file_id,
                        fields,
                        &minecraft_version,
                        &modloader,
                        &cache,
                    )
                    .expect_or_log("Failed to resolve Curse mod"),
                    ModDefinition::Url { location, filename, fields } => {
                        resolve_url(location, filename, fields, &cache).expect_or_log("Failed to resolve URL mod")
                    },
                })
                .collect();

            let raw_nix_file = format!(
                r#"{{
                version = "{version}";
                imports = [];
                mods = {{
                    {mods}
                }};
            }}"#,
                version = minecraft_version,
                mods = resolved.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n")
            );
            let nix_file = nixpkgs_fmt::reformat_string(&raw_nix_file);

            let json_file = serde_json::to_string_pretty(&resolved).expect_or_log("Serialization of resolved mods to JSON failed");

            if out_channel.send(nix_file).is_err() {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
            if json_out.send(json_file).is_err() {
                event!(Level::DEBUG, "Channel 'json' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([
            (ChannelId(node_id.to_owned(), "default".into()), InputType::Text(channel(1).0)),
            (ChannelId(node_id.to_owned(), "json".into()), InputType::Text(channel(1).0)),
        ])
    }
}

#[derive(Error, Debug)]
enum ResolveError {
    #[error("API request failed! Error: {0}")]
    Api(#[from] ApiError),
    #[error("File download failed! Error: {0}")]
    Download(#[from] DownloadError),
    #[error("Missing data when: '{0}'!")]
    EmptyOption(String),
    #[error("Cache interaction failed! Error: {0}")]
    Cache(#[from] CacheError),
    #[error("Failed to deserialize cached data! Error: {0}")]
    CacheDeserialize(#[from] serde_json::Error),
}

struct CacheKey<'a> {
    name: &'a str,
    id: &'a str,
    version: Option<(&'a str, &'a str)>,
}

impl ToString for CacheKey<'_> {
    fn to_string(&self) -> String {
        match self.version {
            Some(version) => format!("{}::{}::{}+{}", self.name, self.id, version.0, version.1),
            None => format!("{}::{}", self.name, self.id),
        }
    }
}

fn get_from_cache(
    cache: &Option<Arc<dyn Cache>>,
    namespace: &str,
    key: &CacheKey,
    merge_meta: &ModDefinitionFields,
) -> Result<Option<ResolvedMod>, ResolveError> {
    match cache {
        Some(cache) => {
            let cache_data = cache.get(namespace, &key.to_string())?;
            match cache_data {
                Some(cache_data) => {
                    let mut resolved: Option<ResolvedMod> = serde_json::from_str(&cache_data)?;
                    if let Some(ref mut resolved) = resolved {
                        resolved.side = merge_meta.side;
                        resolved.default = merge_meta.default.unwrap_or(true);
                        resolved.required = merge_meta.required.unwrap_or(true);
                    }
                    Ok(resolved)
                },
                None => Ok(None),
            }
        },
        None => Ok(None),
    }
}

fn store_in_cache(cache: &Option<Arc<dyn Cache>>, namespace: &str, key: &CacheKey, value: &ResolvedMod) -> Result<(), ResolveError> {
    match cache {
        Some(cache) => {
            let serialized = serde_json::to_string(value)?;
            cache.put(namespace, &key.to_string(), &serialized)?;
            Ok(())
        },
        None => Ok(()),
    }
}

const CURSE_CACHE_NAMESPACE: &str = "ModResolver::Curse";
const MODRINTH_CACHE_NAMESPACE: &str = "ModResolver::Modrinth";
const URL_CACHE_NAMESPACE: &str = "ModResolver::URL";

fn resolve_curse(
    client: &CurseClient,
    mod_id: Option<u32>,
    file_id: Option<u32>,
    meta: ModDefinitionFields,
    mcversion: &str,
    loader: &str,
    cache: &Option<Arc<dyn Cache>>,
) -> Result<ResolvedMod, ResolveError> {
    let name = meta.name.clone();
    let _span = span!(Level::INFO, "Curse", mod_name = name).entered();
    let cache_key = CacheKey {
        name: &name,
        id: &file_id.unwrap_or_default().to_string(),
        version: Some((&mcversion, &loader)),
    };
    if let Some(cached) = get_from_cache(cache, CURSE_CACHE_NAMESPACE, &cache_key, &meta)? {
        return Ok(cached);
    }
    let (mod_response, file_response, file_data) = if let Some(id) = file_id {
        let file_response = client
            .get_files(&[id])?
            .pop()
            .ok_or_else(|| ResolveError::EmptyOption("popping single file from Curse files by IDs response".to_owned()))?;
        let file_data = download_file(&file_response.download_url)?;
        let mod_response = client.find_mod_by_id(file_response.mod_id)?;
        (mod_response, file_response, file_data)
    } else {
        let mod_response = match mod_id {
            Some(id) => client.find_mod_by_id(id),
            None => client.find_mod_by_slug(&meta.name),
        }?;

        let mut filtered_files_response = client
            .get_mod_files(mod_response.id)?
            .into_iter()
            .filter(|f| cf_matches_version(f, mcversion, loader))
            .collect::<Vec<_>>();
        filtered_files_response.sort_unstable_by_key(|f| f.file_date.clone());
        let file_response = filtered_files_response
            .pop()
            .ok_or_else(|| ResolveError::EmptyOption("popping latest file from Curse files by mod response".to_owned()))?;
        let file_data = download_file(&file_response.download_url)?;
        (mod_response, file_response, file_data)
    };

    let sha256hash = sha256hash(&file_data);
    let md5hash = {
        match file_response.hashes.into_iter().find(|h| h.algo == HashAlgo::Md5) {
            Some(hash) => hash.value,
            None => md5hash(&file_data),
        }
    };
    let resolved = ResolvedMod {
        default: meta.default.unwrap_or(true),
        encoded: encode(&file_response.file_name).into_owned(),
        filename: file_response.file_name,
        src: encode_spaces(&file_response.download_url),
        md5: md5hash,
        side: meta.side,
        title: mod_response.name,
        name: mod_response.slug,
        size: file_data.len() as u64,
        sha256: sha256hash,
        required: meta.required.unwrap_or(true),
    };
    store_in_cache(cache, CURSE_CACHE_NAMESPACE, &cache_key, &resolved)?;
    Ok(resolved)
}

fn encode_spaces(url: &str) -> String {
    url.replace(" ", "%20")
}

// CF has an awesome API where modloader type is a first-class field. Oh wait, that's Modrinth...
// We can't check for existence of the modloader in the gameVersions field, as it is optional.
// Instead, we do a best-effort removal of any files that explicitly support the opposite
// modloader. Will this break on anyone crazy enough to ship multi-loader JARs? Probably!
fn cf_matches_version(file: &api_client::curse::model::File, mcversion: &str, loader: &str) -> bool {
    file.game_versions.iter().any(|v| v == mcversion)
        && match loader.to_lowercase().as_str() {
            "forge" => file.game_versions.iter().any(|v| v == "Forge") || !file.game_versions.iter().any(|v| v == "Fabric"),
            "fabric" => file.game_versions.iter().any(|v| v == "Fabric") || !file.game_versions.iter().any(|v| v == "Forge"),
            _ => true,
        }
}

fn resolve_modrinth(
    client: &ModrinthClient,
    mod_id: Option<String>,
    file_id: Option<String>,
    meta: ModDefinitionFields,
    mcversion: &str,
    loader: &str,
    cache: &Option<Arc<dyn Cache>>,
) -> Result<ResolvedMod, ResolveError> {
    let name = meta.name.clone();
    let _span = span!(Level::INFO, "Modrinth", mod_name = name).entered();
    let cache_key = CacheKey {
        name: &name,
        id: &file_id.clone().unwrap_or_default(),
        version: Some((&mcversion, &loader)),
    };
    if let Some(cached) = get_from_cache(cache, MODRINTH_CACHE_NAMESPACE, &cache_key, &meta)? {
        return Ok(cached);
    }
    let (mod_response, file_response) = if let Some(ref id) = file_id {
        let file_response = client.get_version(id)?;
        let mod_response = client.get_mod_info(&file_response.project_id)?;
        (mod_response, file_response)
    } else {
        let mod_response = client.get_mod_info(match mod_id {
            Some(ref id) => id,
            None => &meta.name,
        })?;

        let mut filtered_files_response = client.get_mod_versions(&mod_response.id, Some(&[loader]), Some(&[mcversion]))?;
        filtered_files_response.sort_unstable_by_key(|f| f.date_published.clone());
        let file_response = filtered_files_response
            .pop()
            .ok_or_else(|| ResolveError::EmptyOption("popping latest file from Modrinth versions by mod response".to_owned()))?;
        (mod_response, file_response)
    };
    let primary_file = file_response
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| file_response.files.first())
        .ok_or_else(|| ResolveError::EmptyOption("getting primary or first file from Modrinth version by ID response".to_owned()))?;
    let file_data = download_file(&primary_file.url)?;
    let sha256hash = sha256hash(&file_data);
    let md5hash = md5hash(&file_data);
    let resolved = ResolvedMod {
        name: mod_response.slug,
        title: mod_response.title,
        side: meta.side,
        required: meta.required.unwrap_or(true),
        default: meta.default.unwrap_or(true),
        encoded: encode(&primary_file.filename).into_owned(),
        filename: primary_file.filename.clone(),
        src: encode_spaces(&primary_file.url),
        size: primary_file.size,
        md5: md5hash,
        sha256: sha256hash,
    };
    store_in_cache(cache, MODRINTH_CACHE_NAMESPACE, &cache_key, &resolved)?;
    Ok(resolved)
}

fn resolve_url(
    location: String,
    filename: Option<String>,
    meta: ModDefinitionFields,
    cache: &Option<Arc<dyn Cache>>,
) -> Result<ResolvedMod, ResolveError> {
    let name = meta.name.clone();
    let _span = span!(Level::INFO, "URL", mod_name = name).entered();
    let cache_key = CacheKey {
        name: &name,
        id: &location,
        version: None,
    };
    if let Some(cached) = get_from_cache(cache, URL_CACHE_NAMESPACE, &cache_key, &meta)? {
        return Ok(cached);
    }
    let file_data = download_file(&location)?;
    let resolved_filename = match filename {
        Some(value) => value,
        None => get_filename(&location)?,
    };
    let md5hash = md5hash(&file_data);
    let sha256hash = sha256hash(&file_data);
    let resolved = ResolvedMod {
        name: meta.name.clone(),
        title: meta.name,
        side: meta.side,
        required: meta.required.unwrap_or(true),
        default: meta.default.unwrap_or(true),
        encoded: encode(&resolved_filename).into_owned(),
        filename: resolved_filename,
        src: location.clone(),
        size: file_data.len() as u64,
        md5: md5hash,
        sha256: sha256hash,
    };
    store_in_cache(cache, URL_CACHE_NAMESPACE, &cache_key, &resolved)?;
    Ok(resolved)
}

fn sha256hash<T>(data: T) -> String
where
    T: AsRef<[u8]>,
{
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
fn md5hash<T>(data: T) -> String
where
    T: AsRef<[u8]>,
{
    let mut hasher = Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn get_filename(url: &str) -> Result<String, ResolveError> {
    url.split('/')
        .last()
        .ok_or_else(|| {
            ResolveError::EmptyOption(format!(
                "getting last part of URL after splitting on / when resolving filename. URL: {url}"
            ))
        })?
        .split('?')
        .next()
        .map(|s| s.to_string())
        .ok_or_else(|| ResolveError::EmptyOption(format!("trimming query params off URL if present to resolve filename. URL: {url}")))
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Mutex, time::Duration};

    use tokio::sync::broadcast;

    use crate::{
        di::container::DiContainerBuilder,
        node::{
            config::{ModDefinition, NodeConfigTypes, Side},
            utils::{get_curse_config, get_output_test, read_channel},
        },
    };

    use super::*;

    #[test]
    fn test_mod_resolver() {
        let node_id = "resolver";
        let mod_channel = broadcast::channel(1).0;
        let input_ids = HashMap::from([("mods".into(), ChannelId::from_str("mod-source").unwrap())]);
        let node = NodeConfigTypes::ModResolver(ModResolver);

        let mut ctx_builder = DiContainerBuilder::default();
        let curse_config = get_curse_config();
        if let Ok(ref c) = curse_config {
            ctx_builder = ctx_builder.curse_client_key(&c.curse_api_key);
        } else {
            ctx_builder = ctx_builder.curse_client_proxy("https://api.curse.tools/v1/cf")
        }
        let mut ctx = ctx_builder
            .channel_from_node(node.generate_channels(node_id))
            .channel_from_node(HashMap::from([(
                ChannelId::from_str("mod-source").unwrap(),
                InputType::Mods(mod_channel.clone()),
            )]))
            .set_config("minecraft_version", "1.12.2")
            .set_config("modloader", "forge")
            .build();

        let mut out_channel = get_output_test!(ChannelId::from_str("resolver").unwrap(), Text, ctx);
        let mut json_out_channel = get_output_test!(ChannelId::from_str("resolver::json").unwrap(), Text, ctx);

        let mod_config: Vec<ModDefinition> = serde_yaml::from_str(
            r#"---
- name: appleskin
  source: modrinth
- name: mouse-tweaks
  source: curse
  side: client
- name: title-changer
  source: url
  location: https://github.com/Maxwell-lt/TitleChanger/releases/download/1.1.3/titlechanger-1.1.3.jar
  side: client
  required: false
        "#,
        )
        .unwrap();

        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        ctx.run().unwrap();
        mod_channel.send(mod_config).unwrap();

        handle.join().unwrap();

        let timeout = Duration::from_secs(30);
        let output: String = read_channel(&mut out_channel, timeout).unwrap();
        let json_output: String = read_channel(&mut json_out_channel, timeout).unwrap();

        let expected = r#"{
  version = "1.12.2";
  imports = [ ];
  mods = {
    "appleskin" = {
      title = "AppleSkin";
      name = "appleskin";
      side = "both";
      required = "true";
      default = "true";
      filename = "AppleSkin-mc1.12-1.0.14.jar";
      encoded = "AppleSkin-mc1.12-1.0.14.jar";
      src = "https://cdn.modrinth.com/data/EsAfCjCV/versions/Tsz4BT2X/AppleSkin-mc1.12-1.0.14.jar";
      size = "33683";
      md5 = "b435860d5cfa23bc53d3b8e120be91d4";
      sha256 = "4bbd37edecff0b420ab0eea166b5d7b4b41a9870bfb8647bf243140dc57f101e";
    };
    "mouse-tweaks" = {
      title = "Mouse Tweaks";
      name = "mouse-tweaks";
      side = "client";
      required = "true";
      default = "true";
      filename = "MouseTweaks-2.10.1-mc1.12.2.jar";
      encoded = "MouseTweaks-2.10.1-mc1.12.2.jar";
      src = "https://edge.forgecdn.net/files/3359/843/MouseTweaks-2.10.1-mc1.12.2.jar";
      size = "80528";
      md5 = "a6034d3ff57091c78405e46f1f926282";
      sha256 = "5e13315f4e0d0c96b1f9b800a42fecb89f519aca81d556c91df617c8751aa575";
    };
    "title-changer" = {
      title = "title-changer";
      name = "title-changer";
      side = "client";
      required = "false";
      default = "true";
      filename = "titlechanger-1.1.3.jar";
      encoded = "titlechanger-1.1.3.jar";
      src = "https://github.com/Maxwell-lt/TitleChanger/releases/download/1.1.3/titlechanger-1.1.3.jar";
      size = "5923";
      md5 = "8fda92da93d78919cff1139e847d3e1c";
      sha256 = "78bbe270f2f2ca443a4e794ee1f0c5920ef933ce1030bae0dcff45cb16689eb7";
    };
  };
}
"#;

        let json_expected = r#"[
  {
    "name": "appleskin",
    "title": "AppleSkin",
    "side": "both",
    "required": true,
    "default": true,
    "filename": "AppleSkin-mc1.12-1.0.14.jar",
    "encoded": "AppleSkin-mc1.12-1.0.14.jar",
    "src": "https://cdn.modrinth.com/data/EsAfCjCV/versions/Tsz4BT2X/AppleSkin-mc1.12-1.0.14.jar",
    "size": 33683,
    "md5": "b435860d5cfa23bc53d3b8e120be91d4",
    "sha256": "4bbd37edecff0b420ab0eea166b5d7b4b41a9870bfb8647bf243140dc57f101e"
  },
  {
    "name": "mouse-tweaks",
    "title": "Mouse Tweaks",
    "side": "client",
    "required": true,
    "default": true,
    "filename": "MouseTweaks-2.10.1-mc1.12.2.jar",
    "encoded": "MouseTweaks-2.10.1-mc1.12.2.jar",
    "src": "https://edge.forgecdn.net/files/3359/843/MouseTweaks-2.10.1-mc1.12.2.jar",
    "size": 80528,
    "md5": "a6034d3ff57091c78405e46f1f926282",
    "sha256": "5e13315f4e0d0c96b1f9b800a42fecb89f519aca81d556c91df617c8751aa575"
  },
  {
    "name": "title-changer",
    "title": "title-changer",
    "side": "client",
    "required": false,
    "default": true,
    "filename": "titlechanger-1.1.3.jar",
    "encoded": "titlechanger-1.1.3.jar",
    "src": "https://github.com/Maxwell-lt/TitleChanger/releases/download/1.1.3/titlechanger-1.1.3.jar",
    "size": 5923,
    "md5": "8fda92da93d78919cff1139e847d3e1c",
    "sha256": "78bbe270f2f2ca443a4e794ee1f0c5920ef933ce1030bae0dcff45cb16689eb7"
  }
]"#;

        assert_eq!(output, expected);
        assert_eq!(json_output, json_expected);
    }

    #[test]
    fn parse_filename_from_url() {
        assert_eq!(
            get_filename("https://github.com/Maxwell-lt/TitleChanger/releases/download/1.1.3/titlechanger-1.1.3.jar").unwrap(),
            "titlechanger-1.1.3.jar"
        );
        assert_eq!(
            get_filename("https://github.com/Maxwell-lt/TitleChanger/releases/download/1.1.3/titlechanger-1.1.3.jar?query=param").unwrap(),
            "titlechanger-1.1.3.jar"
        );
    }

    struct TestCache {
        data: Arc<Mutex<HashMap<(String, String), String>>>,
    }

    impl Cache for TestCache {
        fn put(&self, namespace: &str, key: &str, data: &str) -> Result<(), CacheError> {
            self.data.lock().unwrap().insert((namespace.to_owned(), key.to_owned()), data.to_owned());
            Ok(())
        }

        fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError> {
            Ok(self.data.lock().unwrap().get(&(namespace.to_owned(), key.to_owned())).map(|v| v.clone()))
        }
    }

    #[test]
    fn test_cache() {
        let node_id = "resolver";
        let mod_channel = broadcast::channel(1).0;
        let input_ids = HashMap::from([("mods".into(), ChannelId::from_str("mod-source").unwrap())]);
        let node = NodeConfigTypes::ModResolver(ModResolver);

        let curse_mod = ResolvedMod {
            name: "fake-mod".to_owned(),
            title: "".to_owned(),
            side: Side::Client,
            required: false,
            default: true,
            filename: "".to_owned(),
            encoded: "".to_owned(),
            src: "".to_owned(),
            size: 12345,
            md5: "".to_owned(),
            sha256: "".to_owned(),
        };

        let modrinth_mod = ResolvedMod {
            name: "fake-mod-2".to_owned(),
            title: "".to_owned(),
            side: Side::Server,
            required: false,
            default: false,
            filename: "".to_owned(),
            encoded: "".to_owned(),
            src: "".to_owned(),
            size: 12345,
            md5: "".to_owned(),
            sha256: "".to_owned(),
        };

        let mods: Vec<ModDefinition> = vec![
            ModDefinition::Curse {
                id: None,
                file_id: Some(12345),
                fields: ModDefinitionFields {
                    name: "fake-mod".to_owned(),
                    side: Side::Both,
                    required: Some(true),
                    default: Some(true),
                },
            },
            ModDefinition::Modrinth {
                id: None,
                file_id: Some("abcde".to_owned()),
                fields: ModDefinitionFields {
                    name: "fake-mod-2".to_owned(),
                    side: Side::Server,
                    required: Some(false),
                    default: Some(true),
                },
            },
        ];

        let expected_resolved = {
            let mut expected_curse = curse_mod.clone();
            let mut expected_modrinth = modrinth_mod.clone();
            expected_curse.side = Side::Both;
            expected_curse.required = true;
            expected_curse.default = true;

            expected_modrinth.side = Side::Server;
            expected_modrinth.required = false;
            expected_modrinth.default = true;
            vec![expected_curse, expected_modrinth]
        };

        let cache = TestCache {
            data: Arc::new(Mutex::new(HashMap::from([
                (
                    ("ModResolver::Curse".to_owned(), "fake-mod::12345::1.12.2+forge".to_owned()),
                    serde_json::to_string(&curse_mod).unwrap(),
                ),
                (
                    ("ModResolver::Modrinth".to_owned(), "fake-mod-2::abcde::1.12.2+forge".to_owned()),
                    serde_json::to_string(&modrinth_mod).unwrap(),
                ),
            ]))),
        };

        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(node.generate_channels(node_id))
            .channel_from_node(HashMap::from([(
                ChannelId::from_str("mod-source").unwrap(),
                InputType::Mods(mod_channel.clone()),
            )]))
            .set_config("minecraft_version", "1.12.2")
            .set_config("modloader", "forge")
            .set_cache(Box::new(cache))
            .curse_client_proxy("www.example.com/v1")
            .build();

        let mut json_out_channel = get_output_test!(ChannelId::from_str("resolver::json").unwrap(), Text, ctx);
        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        ctx.run().unwrap();
        mod_channel.send(mods).unwrap();

        handle.join().unwrap();
        let timeout = Duration::from_secs(30);
        let json_output: String = read_channel(&mut json_out_channel, timeout).unwrap();

        let actual_resolved: Vec<ResolvedMod> = serde_json::from_str(&json_output).unwrap();

        assert_eq!(actual_resolved, expected_resolved);
    }
}
