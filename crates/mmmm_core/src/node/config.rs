use std::{collections::HashMap, thread::JoinHandle};

use super::{archive_downloader::ArchiveDownloaderNode, dir_merge::DirectoryMerger, file_filter::FileFilterNode};
use crate::di::container::{ChannelId, DiContainer, InputType};
use enum_dispatch::enum_dispatch;
use serde::Deserialize;
use thiserror::Error;

#[enum_dispatch]
pub(crate) trait NodeConfig {
    fn validate_and_spawn(&self, node_id: String, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError>;
    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType>;
}

#[derive(Debug, Error)]
pub enum NodeInitError {
    #[error("Channel provided for input {input} is of the incorrect type! Channel name is {channel:?}.")]
    InvalidInputType { input: String, channel: ChannelId },
    #[error("Channel provided for output is of the incorrect type! Channel name is {0:?}.")]
    InvalidOutputType(ChannelId),
    #[error("No identifier found for required input {0}!")]
    MissingInputId(String),
    #[error("Could not find channel in context for id {0:?}!")]
    MissingChannel(ChannelId),
}

#[enum_dispatch(NodeConfig)]
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum NodeConfigTypes {
    ArchiveDownloaderNode,
    FileFilterNode,
    DirectoryMerger,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SourceValue {
    Text(String),
    Number(i64),
    List(Vec<String>),
    Mods(Vec<ModDefinition>),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NodeDefinition {
    #[serde(flatten)]
    kind: NodeConfigTypes,
    id: String,
    input: HashMap<String, ChannelId>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SourceDefinition {
    id: String,
    value: SourceValue,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OutputDefinition {
    filename: String,
    source: ChannelId,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NodeConfigEntry {
    Node(NodeDefinition),
    Source(SourceDefinition),
    Output(OutputDefinition),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "source")]
#[serde(rename_all = "lowercase")]
pub enum ModDefinition {
    Modrinth {
        id: Option<String>,
        file_id: Option<String>,
        #[serde(flatten)]
        fields: ModDefinitionFields,
    },
    Curse {
        id: Option<u32>,
        file_id: Option<u32>,
        #[serde(flatten)]
        fields: ModDefinitionFields,
    },
    Url {
        location: String,
        filename: Option<String>,
        #[serde(flatten)]
        fields: ModDefinitionFields,
    },
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModDefinitionFields {
    name: String,
    #[serde(default)]
    side: Side,
    required: Option<bool>,
    default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Client,
    Server,
    #[default]
    Both,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn deserialize() {
        let yaml = r#"---
- id: pack-url
  value: https://example.com/my-pack.zip
- id: filter-pattern
  value:
  - overrides/**
  - modrinth.index.json
  - resource-packs/**
- id: number
  value: 65536
- id: mods
  value:
  - name: appleskin
    source: modrinth
    id: EsAfCjCV
    file_id: pyRMqaEV
    side: both
    required: true
    default: true
  - name: mouse-tweaks
    source: curse
    id: 60089
    file_id: 4581240
    side: client
    required: false
    default: true
  - name: waystones
    source: curse
  - name: patchouli
    source: url
    location: 'https://github.com/VazkiiMods/Patchouli/releases/download/release-1.20.1-81/Patchouli-1.20.1-81-FORGE.jar'
- id: download
  kind: ArchiveDownloaderNode
  input:
    url: pack-url
- id: filter
  kind: FileFilterNode
  input:
    files: download
    pattern: filter-pattern::default
- filename: my-pack
  source: filter"#;
        let nodes: Vec<NodeConfigEntry> = serde_yaml::from_str(yaml).unwrap();
        let expected = [
            NodeConfigEntry::Source(SourceDefinition {
                id: "pack-url".into(),
                value: SourceValue::Text("https://example.com/my-pack.zip".into()),
            }),
            NodeConfigEntry::Source(SourceDefinition {
                id: "filter-pattern".into(),
                value: SourceValue::List(vec!["overrides/**".into(), "modrinth.index.json".into(), "resource-packs/**".into()]),
            }),
            NodeConfigEntry::Source(SourceDefinition {
                id: "number".into(),
                value: SourceValue::Number((65536).into()),
            }),
            NodeConfigEntry::Source(SourceDefinition {
                id: "mods".into(),
                value: SourceValue::Mods(vec![
                    ModDefinition::Modrinth {
                        id: Some("EsAfCjCV".into()),
                        file_id: Some("pyRMqaEV".into()),
                        fields: ModDefinitionFields {
                            name: "appleskin".into(),
                            side: Side::Both,
                            required: Some(true),
                            default: Some(true),
                        },
                    },
                    ModDefinition::Curse {
                        id: Some(60089),
                        file_id: Some(4581240),
                        fields: ModDefinitionFields {
                            name: "mouse-tweaks".into(),
                            side: Side::Client,
                            required: Some(false),
                            default: Some(true),
                        },
                    },
                    ModDefinition::Curse {
                        id: None,
                        file_id: None,
                        fields: ModDefinitionFields {
                            name: "waystones".into(),
                            side: Side::Both,
                            required: None,
                            default: None,
                        },
                    },
                    ModDefinition::Url {
                        location: "https://github.com/VazkiiMods/Patchouli/releases/download/release-1.20.1-81/Patchouli-1.20.1-81-FORGE.jar".into(),
                        filename: None,
                        fields: ModDefinitionFields {
                            name: "patchouli".into(),
                            side: Side::Both,
                            required: None,
                            default: None,
                        },
                    },
                ]),
            }),
            NodeConfigEntry::Node(NodeDefinition {
                kind: NodeConfigTypes::ArchiveDownloaderNode(ArchiveDownloaderNode),
                id: "download".into(),
                input: HashMap::from([("url".into(), ChannelId::from_str("pack-url").unwrap())]),
            }),
            NodeConfigEntry::Node(NodeDefinition {
                kind: NodeConfigTypes::FileFilterNode(FileFilterNode),
                id: "filter".into(),
                input: HashMap::from([
                    ("files".into(), ChannelId::from_str("download").unwrap()),
                    ("pattern".into(), ChannelId::from_str("filter-pattern::default").unwrap()),
                ]),
            }),
            NodeConfigEntry::Output(OutputDefinition {
                filename: "my-pack".into(),
                source: ChannelId::from_str("filter").unwrap(),
            }),
        ];
        println!("{:?}", nodes);
        assert_eq!(nodes.len(), expected.len());
        for (a, e) in nodes.iter().zip(expected.iter()) {
            assert_eq!(a, e);
        }
    }
}
