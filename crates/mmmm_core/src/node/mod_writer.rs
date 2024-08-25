use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use serde::Deserialize;
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::ResultExt;

use crate::di::container::{DiContainer, InputType, OutputType};

use super::{
    config::{ChannelId, NodeConfig, NodeInitError},
    utils::{get_input, get_output},
};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModWriter;

impl NodeConfig for ModWriter {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let mut resolved_mods_channel = get_input!("resolved", ResolvedMods, ctx, input_ids)?;
        let out_channel = get_output!(ChannelId(node_id.clone(), "default".into()), Text, ctx)?;
        let json_out = get_output!(ChannelId(node_id.clone(), "json".into()), Text, ctx)?;

        let mut waker = ctx.get_waker();

        let minecraft_version = ctx
            .get_config("minecraft_version")
            .ok_or_else(|| NodeInitError::MissingConfig("minecraft_version".into()))?;

        Ok(spawn(move || {
            let _span = span!(Level::INFO, "ModWriter", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let mut resolved = resolved_mods_channel.blocking_recv().expect_or_log("Failed to receive on resolved input");
            resolved.sort_by_key(|r| r.name.clone());

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

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use tokio::sync::broadcast;

    use crate::{
        di::container::DiContainerBuilder,
        node::{
            config::{NodeConfigTypes, ResolvedMod, Side},
            utils::{get_output_test, read_channel},
        },
    };

    use super::*;

    #[test]
    fn test_mod_writer() {
        let node_id = "writer";
        let resolved_mods_channel = broadcast::channel(1).0;
        let input_ids = HashMap::from([("resolved".into(), ChannelId::from_str("mod-source").unwrap())]);
        let node = NodeConfigTypes::ModWriter(ModWriter);

        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(node.generate_channels(node_id))
            .channel_from_node(HashMap::from([(
                ChannelId::from_str("mod-source").unwrap(),
                InputType::ResolvedMods(resolved_mods_channel.clone()),
            )]))
            .set_config("minecraft_version", "1.12.2")
            .set_config("modloader", "forge")
            .build();

        let mut out_channel = get_output_test!(ChannelId::from_str("writer").unwrap(), Text, ctx);
        let mut json_out_channel = get_output_test!(ChannelId::from_str("writer::json").unwrap(), Text, ctx);


        let resolved_mods = vec![ResolvedMod {
            name: "mixinbootstrap".to_owned(),
            title: "MixinBootstrap".to_owned(),
            side: Side::Both,
            required: true,
            default: true,
            filename: "_MixinBootstrap-1.1.0.jar".to_owned(),
            encoded: "_MixinBootstrap-1.1.0.jar".to_owned(),
            src: "https://edge.forgecdn.net/files/3437/402/_MixinBootstrap-1.1.0.jar".to_owned(),
            size: 1119478,
            md5: "9df0dc628ebcd787270f487fbbf8157a".to_owned(),
            sha256: "17c589aad9907d4ba56d578d502afa80aac1ba2fa8677e8b4d06c019c41d7731".to_owned(),
        }];

        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        ctx.run().unwrap();
        resolved_mods_channel.send(resolved_mods).unwrap();

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
