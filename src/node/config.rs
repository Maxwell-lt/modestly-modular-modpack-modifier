use std::{collections::HashMap, thread::JoinHandle};

use serde::Deserialize;
use enum_dispatch::enum_dispatch;
use serde_yaml::Number;
use thiserror::Error;
use super::archive_downloader::ArchiveDownloaderNode;
use crate::di::container::{DiContainer, ChannelId};

#[enum_dispatch]
pub(crate) trait NodeConfig {
    fn validate_and_spawn(&self, node_id: &str, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError>;
}

#[derive(Debug, Error)]
pub enum NodeInitError {
    #[error("Channel provided for input {input} is of the incorrect type! Channel name is {channel:?}.")]
    InvalidInputType {
        input: String,
        channel: ChannelId,
    },
    #[error("Channel provided for output is of the incorrect type! Channel name is {0:?}.")]
    InvalidOutputType (ChannelId),
    #[error("No identifier found for required input {0}!")]
    MissingInputId (String),
    #[error("Could not find channel in context for id {0:?}!")]
    MissingChannel (ChannelId),
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
    List(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
struct NodeDefinition {
    kind: NodeConfigTypes,
    id: String,
    input: HashMap<String, ChannelId>,
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
