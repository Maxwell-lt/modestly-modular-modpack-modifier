use anyhow::Result;
use serde::{de, Deserialize};
use std::{collections::HashMap, str::FromStr};
use tokio::sync::broadcast;

use crate::file::{filestore::FileStore, filetree::FileTree};

use super::logger::Logger;

pub(crate) struct DiContainer {
    // Global config values (e.g. API keys)
    configs: HashMap<String, String>,
    // Broadcast channel handles
    channels: HashMap<ChannelId, InputType>,
    // Global FileStore object
    filestore: FileStore,
    // Triggers all nodes to begin waiting for inputs
    // On node init, not all [`broadcast::Receiver`] instances may exist yet,
    // so messages sent from the paired [`broadcast::Sender`]
    // by nodes that take no inputs would not be sent to nodes yet to
    // be initialized if it began processing post-init.
    waker: broadcast::Sender<()>,
    // Log messages
    logs: Logger,
}

#[derive(Debug, Clone)]
pub(crate) enum InputType {
    Text(broadcast::Sender<String>),
    Files(broadcast::Sender<FileTree>),
    List(broadcast::Sender<Vec<String>>),
}

impl InputType {
    fn subscribe(&self) -> OutputType {
        match self {
            InputType::Text(c) => OutputType::Text(c.subscribe()),
            InputType::Files(c) => OutputType::Files(c.subscribe()),
            InputType::List(c) => OutputType::List(c.subscribe()),
        }
    }
}

#[derive(Debug)]
pub(crate) enum OutputType {
    Text(broadcast::Receiver<String>),
    Files(broadcast::Receiver<FileTree>),
    List(broadcast::Receiver<Vec<String>>),
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

impl DiContainer {
    pub(crate) fn new(configs: HashMap<String, String>, channels: HashMap<ChannelId, InputType>) -> Self {
        Self {
            configs,
            channels,
            filestore: FileStore::new(),
            waker: broadcast::channel::<()>(1).0,
            logs: Logger::new(),
        }
    }

    pub(crate) fn get_filestore(&self) -> FileStore {
        self.filestore.clone()
    }

    pub(crate) fn get_config(&self, config: &str) -> Option<String> {
        self.configs.get(config).cloned()
    }

    pub(crate) fn get_sender(&self, id: &ChannelId) -> Option<InputType> {
        self.channels.get(id).cloned()
    }

    pub(crate) fn get_receiver(&self, id: &ChannelId) -> Option<OutputType> {
        self.channels.get(id).map(|c| c.subscribe())
    }

    pub(crate) fn get_waker(&self) -> broadcast::Receiver<()> {
        self.waker.subscribe()
    }

    pub(crate) fn run(&self) -> Result<usize> {
        Ok(self.waker.send(())?)
    }

    pub(crate) fn get_logger(&self) -> Logger {
        self.logs.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filestore_is_cloned() {
        let c = DiContainer::new(HashMap::new(), HashMap::new());
        let fs1 = c.get_filestore();
        let fs2 = c.get_filestore();
        let hash = fs1.write_file("Hello World!".into());

        // Test that both filestores point to the same underlying data
        assert!(fs2.get_file(hash).is_some());
    }

    #[test]
    fn get_config() {
        let c = DiContainer::new(HashMap::from([("curse.api-key".into(), "12345678".into())]), HashMap::new());

        assert_eq!(c.get_config("curse.api-key").unwrap(), "12345678");
    }

    #[test]
    fn get_sender() {
        let (tx, mut rx) = broadcast::channel::<String>(1);
        let channel_id = ChannelId("node1".into(), "outputA".into());
        let c = DiContainer::new(HashMap::new(), HashMap::from([(channel_id.clone(), InputType::Text(tx))]));

        let container_tx = match c.get_sender(&channel_id).unwrap() {
            InputType::Text(channel) => channel,
            _ => unreachable!(),
        };
        container_tx.send("Test".into()).unwrap();

        assert_eq!(rx.try_recv().unwrap(), "Test");
    }

    #[test]
    fn get_receiver() {
        let tx = broadcast::channel::<String>(1).0;
        let channel_id = ChannelId("node1".into(), "outputA".into());
        let c = DiContainer::new(HashMap::new(), HashMap::from([(channel_id.clone(), InputType::Text(tx.clone()))]));

        let mut container_rx = match c.get_receiver(&channel_id).unwrap() {
            OutputType::Text(channel) => channel,
            _ => unreachable!(),
        };
        tx.send("Test".into()).unwrap();

        assert_eq!(container_rx.try_recv().unwrap(), "Test");
    }

    #[test]
    fn get_waker_channel() {
        let c = DiContainer::new(HashMap::new(), HashMap::new());

        let mut waker_rx = c.get_waker();
        let waker_tx = c.waker.clone();
        waker_tx.send(()).unwrap();

        assert!(waker_rx.try_recv().is_ok());
    }

    #[test]
    fn parse_channel_id() {
        assert_eq!(
            ChannelId::from_str("channel:name").unwrap(),
            ChannelId("channel:name".into(), "default".into())
        );
        assert_eq!(ChannelId::from_str("node::port").unwrap(), ChannelId("node".into(), "port".into()));
        assert!(ChannelId::from_str("").is_err());
        assert!(ChannelId::from_str("node::port::extra").is_err());
    }
}
