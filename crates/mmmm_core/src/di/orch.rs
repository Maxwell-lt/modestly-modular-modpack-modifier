use std::collections::HashMap;

use serde::Deserialize;
use thiserror::Error;

use crate::node::{
    config::{NodeConfig, NodeConfigEntry, NodeInitError, PackDefinition},
    source::Source,
};

use super::container::{DiContainer, DiContainerBuilder, OutputType, WakeError};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MMMMConfig {
    pub curse_api_key: Option<String>,
    pub curse_proxy_url: Option<String>,
}

#[derive(Debug, Error)]
pub enum BuildGraphError {
    #[error("One or more nodes failed to initialize! Errors: {0:?}")]
    NodeConstruction(Vec<NodeInitError>),
    #[error("Source failed to initialize! Error: {0}")]
    SourceConstruction(#[from] NodeInitError),
    #[error("Failed to obtain a channel for an output! Check your output definitions!")]
    OutputChannel,
    #[error("Failed to send signal to waker channel! Error: {0}")]
    WakeError(#[from] WakeError),
}

pub struct Graph {
    pub context: DiContainer,
    pub outputs: HashMap<String, OutputType>,
}

pub fn build_graph(pack_definition: &str, global_config: MMMMConfig) -> Result<Graph, BuildGraphError> {
    let pack = serde_yaml::from_str::<PackDefinition>(pack_definition).unwrap();

    // Separate out node types
    let intermediate_nodes = pack
        .nodes
        .iter()
        .filter_map(|n| match n {
            NodeConfigEntry::Node(node) => Some(node),
            _ => None,
        })
        .collect::<Vec<_>>();
    let source_nodes = pack
        .nodes
        .iter()
        .filter_map(|n| match n {
            NodeConfigEntry::Source(source) => Some(source),
            _ => None,
        })
        .collect::<Vec<_>>();
    let output_nodes = pack
        .nodes
        .iter()
        .filter_map(|n| match n {
            NodeConfigEntry::Output(output) => Some(output),
            _ => None,
        })
        .collect::<Vec<_>>();

    // Create Source node builder
    let source_builder = Source::new(&source_nodes);

    // Build DiContainer
    let mut ctx_builder = DiContainerBuilder::default();
    // Set pack config
    ctx_builder = pack.config.iter().fold(ctx_builder, |cb, (k, v)| cb.set_config(k, v));
    // Create and store output channels
    ctx_builder = intermediate_nodes
        .iter()
        .fold(ctx_builder, |cb, n| cb.channel_from_node(n.kind.generate_channels(&n.id)));
    ctx_builder = ctx_builder.channel_from_node(source_builder.generate_channels());

    // Setup Curse API client if global config specifies the required parameters
    ctx_builder = if let Some(key) = global_config.curse_api_key {
        ctx_builder.curse_client_key(&key)
    } else if let Some(proxy) = global_config.curse_proxy_url {
        ctx_builder.curse_client_proxy(&proxy)
    } else {
        ctx_builder
    };

    // Build DiContainer
    let mut ctx = ctx_builder.build();

    // Get output channels
    let outputs: HashMap<String, OutputType> = output_nodes
        .into_iter()
        .map(|n| ctx.get_receiver(&n.source).map(|r| (n.filename.clone(), r)))
        .collect::<Option<Vec<_>>>()
        .ok_or(BuildGraphError::OutputChannel)?
        .into_iter()
        .collect();

    // Spawn nodes. Drop all returned JoinHandles for now to detach threads.

    // Early return if source fails to spawn thread.
    source_builder.spawn(&ctx)?;

    // Can't early return anymore, since we want to terminate all threads if any fail to start
    let errors = intermediate_nodes
        .iter()
        .map(|node| node.kind.validate_and_spawn(node.id.clone(), &node.input, &ctx))
        .filter_map(|result| result.err())
        .collect::<Vec<_>>();
    // Check if any nodes errored, and if so, cancel and return an error.
    if !errors.is_empty() {
        ctx.cancel()?;
        return Err(BuildGraphError::NodeConstruction(errors));
    }
    Ok(Graph { context: ctx, outputs })
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use crate::{file::filepath::FilePath, node::utils::read_channel};

    use super::*;

    #[test]
    fn test_orchestrator() {
        let mod_config = r#"---
config:
  minecraft_version: '1.20.2'
  modloader: 'fabric'
nodes:
  - id: modlist
    value:
    - name: fabric-api
      source: modrinth
      file_id: Hi8quJUM
    - name: sodium
      source: modrinth
      side: client
      required: false
      default: true
      file_id: bbP1qBMr
    - name: no-chat-reports
      source: modrinth
      id: qQyHxfxd
      file_id: xQyq2W5g
    - name: worldedit
      source: curse
      id: 225608
      file_id: 4773938
    - name: modmenu
      source: url
      location: https://github.com/TerraformersMC/ModMenu/releases/download/v8.0.0/modmenu-8.0.0.jar
      filename: modmenu-8.0.0.jar
  - id: resolver
    kind: ModResolver
    input:
      mods: 'modlist'
  - filename: manifest.zip
    source: 'resolver::default'
..."#;
        let global_config = MMMMConfig {
            curse_proxy_url: Some("https://api.curse.tools/v1/cf".into()),
            curse_api_key: None,
        };
        let mut graph = build_graph(mod_config, global_config).unwrap();
        graph.context.run().unwrap();

        let manifest_channel = if let OutputType::Files(channel) = graph.outputs.get_mut("manifest.zip").unwrap() {
            channel
        } else {
            panic!()
        };
        let timeout = Duration::from_secs(10);
        let manifest_tree = read_channel(manifest_channel, timeout).unwrap();
        let manifest_file = std::str::from_utf8(&manifest_tree.get_file(&FilePath::from_str("manifest.nix").unwrap()).unwrap())
            .unwrap()
            .to_string();
        println!("{}", manifest_file);
        let expected = r#"{
  version = "1.20.2";
  imports = [ ];
  mods = {
    "fabric-api" = {
      title = "Fabric API";
      name = "fabric-api";
      side = "both";
      required = "true";
      default = "true";
      filename = "fabric-api-0.89.3+1.20.2.jar";
      encoded = "fabric-api-0.89.3+1.20.2.jar";
      src = "https://cdn.modrinth.com/data/P7dR8mSH/versions/Hi8quJUM/fabric-api-0.89.3%2B1.20.2.jar";
      size = "2084853";
      md5 = "e41df274506b22b3de52a1cebb9e16cb";
      sha256 = "098250241cc5365e8578d0beb8ca38967c6293f0a485c2019a0dc67c5764b98d";
    };
    "sodium" = {
      title = "Sodium";
      name = "sodium";
      side = "client";
      required = "false";
      default = "true";
      filename = "sodium-fabric-mc1.20.2-0.5.3.jar";
      encoded = "sodium-fabric-mc1.20.2-0.5.3.jar";
      src = "https://cdn.modrinth.com/data/AANobbMI/versions/bbP1qBMr/sodium-fabric-mc1.20.2-0.5.3.jar";
      size = "854938";
      md5 = "1aa421662c19886665425ca9d202fead";
      sha256 = "d42be779e517117adba7af427eb4d196e667278fbe8a8237a4d8929f65815af7";
    };
    "no-chat-reports" = {
      title = "No Chat Reports";
      name = "no-chat-reports";
      side = "both";
      required = "true";
      default = "true";
      filename = "NoChatReports-FABRIC-1.20.2-v2.3.1.jar";
      encoded = "NoChatReports-FABRIC-1.20.2-v2.3.1.jar";
      src = "https://cdn.modrinth.com/data/qQyHxfxd/versions/xQyq2W5g/NoChatReports-FABRIC-1.20.2-v2.3.1.jar";
      size = "719016";
      md5 = "df554e782381e82500739dfcf22357af";
      sha256 = "8524827fe6d3ccc830a4d8a9b248eee6334b13e011705805d60ba9df2b3d484d";
    };
    "worldedit" = {
      title = "WorldEdit";
      name = "worldedit";
      side = "both";
      required = "true";
      default = "true";
      filename = "worldedit-mod-7.2.16.jar";
      encoded = "worldedit-mod-7.2.16.jar";
      src = "https://edge.forgecdn.net/files/4773/938/worldedit-mod-7.2.16.jar";
      size = "5952031";
      md5 = "740d6dac5136ddb4d58ce003a70bd941";
      sha256 = "253f36bd6d9a62c3fccf6a85f0e657ee3617b97434957dd62829ee9741278b76";
    };
    "modmenu" = {
      title = "modmenu";
      name = "modmenu";
      side = "both";
      required = "true";
      default = "true";
      filename = "modmenu-8.0.0.jar";
      encoded = "modmenu-8.0.0.jar";
      src = "https://github.com/TerraformersMC/ModMenu/releases/download/v8.0.0/modmenu-8.0.0.jar";
      size = "727276";
      md5 = "1dfdf78dda5cad51256b53a9fe6ebbf5";
      sha256 = "586500e765dac915510805031145e634fd9d0aa5e1254c1a50425e05d3efb2c9";
    };
  };
}
"#;
        assert_eq!(manifest_file, expected);
    }
}
