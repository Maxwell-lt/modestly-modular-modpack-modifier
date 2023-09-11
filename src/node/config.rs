use std::collections::HashMap;

use serde::Deserialize;
use enum_dispatch::enum_dispatch;
use serde_yaml::Number;
use thiserror::Error;
use super::archive_downloader::ArchiveDownloaderNode;
use crate::di::container::DiContainer;

#[enum_dispatch]
pub(crate) trait NodeConfig {
    fn validate_and_spawn(&self, input_ids: HashMap<String, String>, ctx: DiContainer) -> Result<(), NodeInitError>;
}

#[derive(Debug, Error)]
pub enum NodeInitError {
    #[error("Channel provided for input {input} is of the incorrect type! Channel name is {channel}.")]
    InvalidInputType {
        input: String,
        channel: String,
    },
}

#[enum_dispatch(NodeConfig)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum NodeConfigTypes {
    ArchiveDownloaderNode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "untagged")]
enum SourceValue {
    Text(String),
    Number(Number),
}

#[derive(Debug, Clone, Deserialize)]
struct NodeDefinition {
    kind: NodeConfigTypes,
    id: String,
    input: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceDefinition {
    id: String,
    value: SourceValue,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum NodeConfigEntry {
    Node(NodeDefinition),
    Source(SourceDefinition),
}
