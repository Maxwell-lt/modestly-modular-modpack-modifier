use std::{collections::HashMap, fmt::Display, str::FromStr, thread::JoinHandle};

use super::{archive_downloader::ArchiveDownloader, dir_merge::DirectoryMerger, file_filter::FileFilter, mod_resolver::ModResolver};
use crate::di::container::{DiContainer, InputType};
use enum_dispatch::enum_dispatch;
use serde::{
    de::{self},
    Deserialize, Serialize,
};
use thiserror::Error;

#[enum_dispatch]
pub trait NodeConfig {
    fn validate_and_spawn(&self, node_id: String, input_ids: &HashMap<String, ChannelId>, ctx: &DiContainer)
        -> Result<JoinHandle<()>, NodeInitError>;
    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType>;
}

pub trait Cache: Send + Sync {
    fn put(&self, namespace: &str, key: &str, data: &str) -> Result<(), CacheError>;
    fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError>;
}

#[derive(Debug, Error)]
#[error("Error accessing the cache: {msg}")]
pub struct CacheError {
    pub msg: String,
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
    #[error("Could not find config value named {0}!")]
    MissingConfig(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PackDefinition {
    pub config: HashMap<String, String>,
    pub nodes: Vec<NodeConfigEntry>,
}

#[enum_dispatch(NodeConfig)]
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum NodeConfigTypes {
    ArchiveDownloader,
    FileFilter,
    DirectoryMerger,
    ModResolver,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SourceValue {
    Text(String),
    List(Vec<String>),
    Mods(Vec<ModDefinition>),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NodeDefinition {
    #[serde(flatten)]
    pub kind: NodeConfigTypes,
    pub id: String,
    pub input: HashMap<String, ChannelId>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SourceDefinition {
    pub id: String,
    pub value: SourceValue,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OutputDefinition {
    pub filename: String,
    pub source: ChannelId,
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
    pub name: String,
    #[serde(default)]
    pub side: Side,
    pub required: Option<bool>,
    pub default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default, Hash, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Client,
    Server,
    #[default]
    Both,
}

impl Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Side::Client => "client",
                Side::Server => "server",
                Side::Both => "both",
            }
        )
    }
}

/// Stores a channel ID by a tuple of (output node name, output name)
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct ChannelId(pub String, pub String);

impl FromStr for ChannelId {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let parts = s.split("::").filter(|p| !p.is_empty()).collect::<Vec<_>>();
        match parts.len() {
            2 => Ok(ChannelId(parts[0].to_string(), parts[1].to_string())),
            1 => Ok(ChannelId(parts[0].to_string(), "default".into())),
            _ => Err(format!("Tried to parse ChannelId from invalid string: '{}'", s)),
        }
    }
}

// From https://github.com/serde-rs/serde/issues/908
impl<'de> Deserialize<'de> for ChannelId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

/// Representation of Nix output format for mods. Several fields have been removed compared to past
/// implementations of cursetool, as they are not used by the builder.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct ResolvedMod {
    pub name: String,
    pub title: String,
    pub side: Side,
    pub required: bool,
    pub default: bool,
    pub filename: String,
    pub encoded: String,
    pub src: String,
    pub size: u64,
    pub md5: String,
    pub sha256: String,
}

impl Display for ResolvedMod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#""{name}" = {{
                title = "{title}";
                name = "{name}";
                side = "{side}";
                required = "{required}";
                default = "{default}";
                filename = "{filename}";
                encoded = "{encoded}";
                src = "{src}";
                size = "{size}";
                md5 = "{md5}";
                sha256 = "{sha256}";
            }};"#,
            title = self.title,
            name = self.name,
            side = self.side,
            required = self.required,
            default = self.default,
            filename = self.filename,
            encoded = self.encoded,
            src = self.src,
            size = self.size,
            md5 = self.md5,
            sha256 = self.sha256
        )
    }
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
  kind: ArchiveDownloader
  input:
    url: pack-url
- id: filter
  kind: FileFilter
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
                kind: NodeConfigTypes::ArchiveDownloader(ArchiveDownloader),
                id: "download".into(),
                input: HashMap::from([("url".into(), ChannelId::from_str("pack-url").unwrap())]),
            }),
            NodeConfigEntry::Node(NodeDefinition {
                kind: NodeConfigTypes::FileFilter(FileFilter),
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
