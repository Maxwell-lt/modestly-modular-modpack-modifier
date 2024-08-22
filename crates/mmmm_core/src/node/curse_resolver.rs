use std::{
    collections::HashMap,
    sync::Arc,
    thread::{spawn, JoinHandle},
};

use api_client::{
    common::{download_file, ApiError, DownloadError},
    curse::{model::HashAlgo, CurseClient},
};
use digest::Digest;
use md5::Md5;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
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
    config::{ChannelId, NodeConfig, NodeInitError, ResolvedMod, Side},
    utils::{get_input, get_output},
};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CurseResolver;

impl NodeConfig for CurseResolver {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let mut manifest_channel = get_input!("manifest", Text, ctx, input_ids)?;
        let out_channel = get_output!(ChannelId(node_id.clone(), "default".into()), Text, ctx)?;
        let json_out = get_output!(ChannelId(node_id.clone(), "json".into()), Text, ctx)?;

        let mut waker = ctx.get_waker();

        let minecraft_version = ctx
            .get_config("minecraft_version")
            .ok_or_else(|| NodeInitError::MissingConfig("minecraft_version".into()))?;

        let curse_client = ctx.get_curse_client().ok_or_else(|| NodeInitError::CurseClientRequired)?;
        let cache = ctx.get_cache();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "CurseResolver", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let manifest = manifest_channel.blocking_recv().expect_or_log("Failed to receive on manifest input");
            event!(Level::INFO, "Got {} mods to resolve", manifest.len());

            let manifest_mods = serde_json::from_str::<CurseManifest>(&manifest).expect_or_log("Failed to deserialize Curse manifest!").files;
            let resolved: Vec<ResolvedMod> = manifest_mods.par_iter()
                .map(|manifest_mod| resolve_curse(&curse_client, manifest_mod.project_id, manifest_mod.file_id, &cache)
                .expect_or_log("Failed to resolve Curse mod"))
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

#[derive(Serialize, Deserialize)]
struct CurseManifest {
    files: Vec<CurseManifestFile>,
}

#[derive(Serialize, Deserialize)]
struct CurseManifestFile {
    #[serde(rename = "projectID")]
    project_id: u32,
    #[serde(rename = "fileID")]
    file_id: u32,
    required: bool,
}

struct CacheKey<'a> {
    id: &'a str,
    file_id: &'a str,
}

impl ToString for CacheKey<'_> {
    fn to_string(&self) -> String {
        format!("{}::{}", self.id, self.file_id)
    }
}

fn get_from_cache(
    cache: &Option<Arc<dyn Cache>>,
    namespace: &str,
    key: &CacheKey,
) -> Result<Option<ResolvedMod>, ResolveError> {
    match cache {
        Some(cache) => {
            let cache_data = cache.get(namespace, &key.to_string())?;
            match cache_data {
                Some(cache_data) => {
                    let resolved: Option<ResolvedMod> = serde_json::from_str(&cache_data)?;
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

const CURSE_CACHE_NAMESPACE: &str = "CurseResolver";

fn resolve_curse(
    client: &CurseClient,
    mod_id: u32,
    file_id: u32,
    cache: &Option<Arc<dyn Cache>>,
) -> Result<ResolvedMod, ResolveError> {
    let _span = span!(Level::INFO, "Curse", mod_id = mod_id, file_id = file_id).entered();
    let cache_key = CacheKey {
        id: &mod_id.to_string(),
        file_id: &file_id.to_string(),
    };
    if let Some(cached) = get_from_cache(cache, CURSE_CACHE_NAMESPACE, &cache_key)? {
        return Ok(cached);
    }
    let mod_response = client.find_mod_by_id(mod_id)?;
    let file_response = client.get_files(&[file_id])?
        .pop()
        .ok_or_else(|| ResolveError::EmptyOption("popping single file from Curse files by IDs response".to_owned()))?;
    let file_data = download_file(&file_response.download_url)?;

    let sha256hash = sha256hash(&file_data);
    let md5hash = {
        match file_response.hashes.into_iter().find(|h| h.algo == HashAlgo::Md5) {
            Some(hash) => hash.value,
            None => md5hash(&file_data),
        }
    };
    let resolved = ResolvedMod {
        default: true,
        encoded: encode(&file_response.file_name).into_owned(),
        filename: file_response.file_name,
        src: encode_spaces(&file_response.download_url),
        md5: md5hash,
        side: Side::Both,
        title: mod_response.name,
        name: mod_response.slug,
        size: file_data.len() as u64,
        sha256: sha256hash,
        required: true,
    };
    store_in_cache(cache, CURSE_CACHE_NAMESPACE, &cache_key, &resolved)?;
    Ok(resolved)
}

fn encode_spaces(url: &str) -> String {
    url.replace(" ", "%20")
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

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use tokio::sync::broadcast;

    use crate::{
        di::container::DiContainerBuilder,
        node::{
            config::NodeConfigTypes,
            utils::{get_curse_config, get_output_test, read_channel},
        },
    };

    use super::*;

    #[test]
    fn test_mod_resolver() {
        let node_id = "resolver";
        let mod_channel = broadcast::channel(1).0;
        let input_ids = HashMap::from([("manifest".into(), ChannelId::from_str("mod-source").unwrap())]);
        let node = NodeConfigTypes::CurseResolver(CurseResolver);

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
                InputType::Text(mod_channel.clone()),
            )]))
            .set_config("minecraft_version", "1.12.2")
            .set_config("modloader", "forge")
            .build();

        let mut out_channel = get_output_test!(ChannelId::from_str("resolver").unwrap(), Text, ctx);
        let mut json_out_channel = get_output_test!(ChannelId::from_str("resolver::json").unwrap(), Text, ctx);

        let manifest = r#"{"files":[{"projectID":357178,"fileID":3437402,"required":true}]}"#;

        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        ctx.run().unwrap();
        mod_channel.send(manifest.to_string()).unwrap();

        handle.join().unwrap();

        let timeout = Duration::from_secs(30);
        let output: String = read_channel(&mut out_channel, timeout).unwrap();
        let json_output: String = read_channel(&mut json_out_channel, timeout).unwrap();

        let expected = r#"{
  version = "1.12.2";
  imports = [ ];
  mods = {
    "mixinbootstrap" = {
      title = "MixinBootstrap";
      name = "mixinbootstrap";
      side = "both";
      required = "true";
      default = "true";
      filename = "_MixinBootstrap-1.1.0.jar";
      encoded = "_MixinBootstrap-1.1.0.jar";
      src = "https://edge.forgecdn.net/files/3437/402/_MixinBootstrap-1.1.0.jar";
      size = "1119478";
      md5 = "9df0dc628ebcd787270f487fbbf8157a";
      sha256 = "17c589aad9907d4ba56d578d502afa80aac1ba2fa8677e8b4d06c019c41d7731";
    };
  };
}
"#;

        let json_expected = r#"[
  {
    "name": "mixinbootstrap",
    "title": "MixinBootstrap",
    "side": "both",
    "required": true,
    "default": true,
    "filename": "_MixinBootstrap-1.1.0.jar",
    "encoded": "_MixinBootstrap-1.1.0.jar",
    "src": "https://edge.forgecdn.net/files/3437/402/_MixinBootstrap-1.1.0.jar",
    "size": 1119478,
    "md5": "9df0dc628ebcd787270f487fbbf8157a",
    "sha256": "17c589aad9907d4ba56d578d502afa80aac1ba2fa8677e8b4d06c019c41d7731"
  }
]"#;

        assert_eq!(output, expected);
        assert_eq!(json_output, json_expected);
    }
}
