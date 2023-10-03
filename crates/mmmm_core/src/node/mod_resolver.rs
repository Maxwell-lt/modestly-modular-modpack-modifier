use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use api_client::{
    common::{download_file, ApiError, DownloadError},
    curse::{model::HashAlgo, CurseClient},
    modrinth::ModrinthClient,
};
use digest::Digest;
use md5::Md5;
use serde::Deserialize;
use sha2::Sha256;
use thiserror::Error;
use tokio::sync::broadcast::channel;

use crate::di::{
    container::{DiContainer, InputType, OutputType},
    logger::LogLevel::Panic,
};

use super::{
    config::{ChannelId, ModDefinition, ModDefinitionFields, NodeConfig, NodeInitError, ResolvedMod},
    utils::{get_input, get_output, log_err, log_send_err},
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

        let logger = ctx.get_logger();
        let mut waker = ctx.get_waker();

        let minecraft_version = ctx
            .get_config("minecraft_version")
            .ok_or_else(|| NodeInitError::MissingConfig("minecraft_version".into()))?;
        let modloader = ctx
            .get_config("modloader")
            .ok_or_else(|| NodeInitError::MissingConfig("modloader".into()))?;

        let curse_client_option = ctx.get_curse_client();
        let modrinth_client = ctx.get_modrinth_client();
        Ok(spawn(move || {
            let should_run = log_err(waker.blocking_recv(), &logger, &node_id);
            if !should_run {
                panic!()
            }

            let mods = log_err(mod_channel.blocking_recv(), &logger, &node_id);

            // Check if the Curse API is needed, but the client wasn't configured. Logs an error
            // message then terminates the thread, so if execution continues past this block we know it is safe to
            // call .unwrap() on the Curse client Option.
            if mods.iter().any(|m| matches!(m, ModDefinition::Curse { .. })) && curse_client_option.is_none() {
                logger.log(
                    node_id,
                    Panic,
                    "Curse client not set up! Cannot resolve mods.".to_owned(),
                    Some(
                        mods.into_iter()
                            .filter_map(|m| match m {
                                ModDefinition::Curse { fields, .. } => Some(fields.name),
                                ModDefinition::Modrinth { .. } => None,
                                ModDefinition::Url { .. } => None,
                            })
                            .collect(),
                    ),
                );
                panic!();
            }

            let resolved = mods
                .into_iter()
                .map(|mod_def| match mod_def {
                    ModDefinition::Modrinth { id, file_id, fields } => log_err(
                        resolve_modrinth(&modrinth_client, id, file_id, fields, &minecraft_version, &modloader),
                        &logger,
                        &node_id,
                    ),
                    ModDefinition::Curse { id, file_id, fields } => log_err(
                        resolve_curse(curse_client_option.as_ref().unwrap(), id, file_id, fields, &minecraft_version, &modloader),
                        &logger,
                        &node_id,
                    ),
                    ModDefinition::Url { location, filename, fields } => log_err(resolve_url(location, filename, fields), &logger, &node_id),
                })
                .map(|r| r.to_string())
                .collect::<Vec<String>>();

            let raw_nix_file = format!(
                r#"{{
                version = "{version}";
                imports = [];
                mods = {{
                    {mods}
                }};
            }}"#,
                version = minecraft_version,
                mods = resolved.join("\n")
            );
            let nix_file = nixpkgs_fmt::reformat_string(&raw_nix_file);
            log_send_err(out_channel.send(nix_file), &logger, &node_id, "default");
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([(ChannelId(node_id.to_owned(), "default".into()), InputType::Text(channel(1).0))])
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
}

fn resolve_curse(
    client: &CurseClient,
    mod_id: Option<u32>,
    file_id: Option<u32>,
    meta: ModDefinitionFields,
    mcversion: &str,
    loader: &str,
) -> Result<ResolvedMod, ResolveError> {
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
    Ok(ResolvedMod {
        default: meta.default.unwrap_or(true),
        filename: file_response.file_name.clone(),
        src: file_response.download_url,
        md5: md5hash,
        side: meta.side,
        title: mod_response.name,
        name: mod_response.slug,
        size: file_data.len() as u64,
        sha256: sha256hash,
        encoded: file_response.file_name,
        required: meta.required.unwrap_or(true),
    })
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
) -> Result<ResolvedMod, ResolveError> {
    let (mod_response, file_response) = if let Some(id) = file_id {
        let file_response = client.get_version(&id)?;
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
    Ok(ResolvedMod {
        name: mod_response.slug,
        title: mod_response.title,
        side: meta.side,
        required: meta.required.unwrap_or(true),
        default: meta.default.unwrap_or(true),
        filename: primary_file.filename.clone(),
        encoded: primary_file.filename.clone(),
        src: primary_file.url.clone(),
        size: primary_file.size,
        md5: md5hash,
        sha256: sha256hash,
    })
}

fn resolve_url(location: String, filename: Option<String>, meta: ModDefinitionFields) -> Result<ResolvedMod, ResolveError> {
    let file_data = download_file(&location)?;
    let resolved_filename = match filename {
        Some(value) => value,
        None => get_filename(&location)?,
    };
    let md5hash = md5hash(&file_data);
    let sha256hash = sha256hash(&file_data);
    Ok(ResolvedMod {
        name: meta.name.clone(),
        title: meta.name,
        side: meta.side,
        required: meta.required.unwrap_or(true),
        default: meta.default.unwrap_or(true),
        filename: resolved_filename.clone(),
        encoded: resolved_filename,
        src: location,
        size: file_data.len() as u64,
        md5: md5hash,
        sha256: sha256hash,
    })
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
    use std::{str::FromStr, time::Duration};

    use tokio::sync::broadcast;

    use crate::{
        di::container::DiContainerBuilder,
        node::{
            config::{ModDefinition, NodeConfigTypes},
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
        }
        let mut ctx = ctx_builder
            .channel_from_node(node.generate_channels(node_id))
            .channel(ChannelId::from_str("mod-source").unwrap(), InputType::Mods(mod_channel.clone()))
            .set_config("minecraft_version", "1.12.2")
            .set_config("modloader", "forge")
            .build();

        let mut out_channel = get_output_test!(ChannelId::from_str("resolver").unwrap(), Text, ctx);

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
        // If no mmmm.toml with an API key is present, skip this test.
        if curse_config.is_err() {
            return;
        }

        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        ctx.run().unwrap();
        mod_channel.send(mod_config).unwrap();

        handle.join().unwrap();

        let timeout = Duration::from_secs(30);
        let output: String = read_channel(&mut out_channel, timeout).unwrap();

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
        assert_eq!(expected, output);
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
}
