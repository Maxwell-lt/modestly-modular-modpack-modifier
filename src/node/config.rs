use std::{collections::HashMap, thread::JoinHandle};

use super::{archive_downloader::ArchiveDownloaderNode, file_filter::FileFilterNode};
use crate::di::container::{ChannelId, DiContainer};
use enum_dispatch::enum_dispatch;
use serde::Deserialize;
use thiserror::Error;

#[enum_dispatch]
pub(crate) trait NodeConfig {
    fn validate_and_spawn(&self, node_id: String, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError>;
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
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SourceValue {
    Text(String),
    Number(i64),
    List(Vec<String>),
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

#[cfg(test)]
mod tests {
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
- id: download
  kind: ArchiveDownloaderNode
  input:
    url: [pack-url, default]
- id: filter
  kind: FileFilterNode
  input:
    files: [download, default]
    pattern: [filter-pattern, default]
- filename: my-pack
  source: [filter, default]"#;
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
            NodeConfigEntry::Node(NodeDefinition {
                kind: NodeConfigTypes::ArchiveDownloaderNode(ArchiveDownloaderNode),
                id: "download".into(),
                input: HashMap::from([("url".into(), ChannelId("pack-url".into(), "default".into()))]),
            }),
            NodeConfigEntry::Node(NodeDefinition {
                kind: NodeConfigTypes::FileFilterNode(FileFilterNode),
                id: "filter".into(),
                input: HashMap::from([
                    ("files".into(), ChannelId("download".into(), "default".into())),
                    ("pattern".into(), ChannelId("filter-pattern".into(), "default".into())),
                ]),
            }),
            NodeConfigEntry::Output(OutputDefinition {
                filename: "my-pack".into(),
                source: ChannelId("filter".into(), "default".into()),
            }),
        ];
        println!("{:?}", nodes);
        assert_eq!(nodes.len(), expected.len());
        for (a, e) in nodes.iter().zip(expected.iter()) {
            assert_eq!(a, e);
        }
    }
}
