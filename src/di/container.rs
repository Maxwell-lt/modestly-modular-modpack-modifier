use std::collections::HashMap;
use anyhow::Result;
use regex::Regex;
use tokio::sync::broadcast;
use serde::Deserialize;

use crate::filetree::{filepath::FilePath, filetree::FileTree, filestore::FileStore};

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
}

#[derive(Debug, Clone)]
pub(crate) enum InputType {
    Text(broadcast::Sender<String>),
    Path(broadcast::Sender<FilePath>),
    Files(broadcast::Sender<FileTree>),
    Regex(broadcast::Sender<Regex>),
    List(broadcast::Sender<Vec<String>>),
}

impl InputType {
    fn subscribe(&self) -> OutputType {
        match self {
            InputType::Text(c) => OutputType::Text(c.subscribe()),
            InputType::Path(c) => OutputType::Path(c.subscribe()),
            InputType::Files(c) => OutputType::Files(c.subscribe()),
            InputType::Regex(c) => OutputType::Regex(c.subscribe()),
            InputType::List(c) => OutputType::List(c.subscribe()),
        }
    }
}

#[derive(Debug)]
pub(crate) enum OutputType {
    Text(broadcast::Receiver<String>),
    Path(broadcast::Receiver<FilePath>),
    Files(broadcast::Receiver<FileTree>),
    Regex(broadcast::Receiver<Regex>),
    List(broadcast::Receiver<Vec<String>>),
}

/// Stores a channel ID by a tuple of (output node name, output name)
#[derive(Debug, PartialEq, Eq, Hash, Clone, Deserialize)]
pub struct ChannelId(pub String, pub String);

impl DiContainer {
    pub(crate) fn new(configs: HashMap<String, String>, channels: HashMap<ChannelId, InputType>) -> Self {
        Self { configs, channels, filestore: FileStore::new(), waker: broadcast::channel::<()>(1).0 }
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
        let c = DiContainer::new(HashMap::new(), HashMap::from([
                                                               (channel_id.clone(), InputType::Text(tx))
        ]));

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
        let c = DiContainer::new(HashMap::new(), HashMap::from([
                                                               (channel_id.clone(), InputType::Text(tx.clone()))
        ]));

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
}
