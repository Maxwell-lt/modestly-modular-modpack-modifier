use std::collections::HashMap;
use regex::Regex;
use tokio::sync::broadcast;

use crate::filetree::{filepath::FilePath, filetree::FileTree, filestore::FileStore};

pub(crate) struct DiContainer {
    // Global config values (e.g. API keys)
    configs: HashMap<String, String>,
    // Broadcast channel handles
    channels: HashMap<String, OutputType>,
    // Global FileStore object
    filestore: FileStore,
    // Triggers all nodes to begin waiting for inputs
    // On node init, not all [`broadcast::Receiver`] instances may exist yet,
    // so messages sent from the paired [`broadcast::Sender`]
    // by nodes that take no inputs would not be sent to nodes yet to 
    // be initialized if it began processing post-init.
    waker: broadcast::Sender<()>,
}

pub(crate) enum OutputType {
    Text(broadcast::Sender<String>),
    Path(broadcast::Sender<FilePath>),
    Files(broadcast::Sender<FileTree>),
    Regex(broadcast::Sender<Regex>),
}

impl DiContainer {
    pub(crate) fn new(configs: HashMap<String, String>, channels: HashMap<String, OutputType>) -> Self {
        Self { configs, channels, filestore: FileStore::new(), waker: broadcast::channel::<()>(1).0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
