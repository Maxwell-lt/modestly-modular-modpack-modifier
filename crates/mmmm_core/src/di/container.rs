use api_client::{curse::CurseClient, modrinth::ModrinthClient};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::sync::broadcast::{self, error::SendError};

use crate::{
    file::{filestore::FileStore, filetree::FileTree},
    node::config::{ChannelId, ModDefinition, ResolvedMod},
    Cache,
};

/// Holds references to shared resources required by nodes, and coordinates nodes.
///
/// When a node is initialized, it obtains required objects from this container, such as
/// input/output channels, a start signal, and API clients with shared rate limits and
/// API keys. This achieves a pseudo-IoC structure, where the required dependencies can be loosely
/// coupled with the code required to spawn a node. Implementors of the [`NodeConfig`] trait can
/// obtain all of their variable dependencies from the container, returning runtime errors if any
/// are missing.
/// After all nodes have been initialized, the owner of a [`DiContainer`] can trigger all nodes to
/// start.
pub struct DiContainer {
    // Global config values (e.g. Minecraft version
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
    // Passing false through the waker channel will abort all nodes, passing true will start them.
    waker: broadcast::Sender<bool>,
    waker_called: bool,
    // Curse API client
    // Curse API requires an API key, so may not exist if no key is found.
    // To prevent Modrinth/URL only packs from building in this case, nodes should only panic when
    // the Curse API is actually required to resolve a mod download.
    curse_client: Option<CurseClient>,
    // Modrinth API client
    // The Modrinth API does not require an API key, so one can always be created.
    modrinth_client: ModrinthClient,
    // Cache
    cache: Option<Arc<dyn Cache>>,
}

#[derive(Debug, Clone)]
pub enum InputType {
    Text(broadcast::Sender<String>),
    Files(broadcast::Sender<FileTree>),
    List(broadcast::Sender<Vec<String>>),
    Mods(broadcast::Sender<Vec<ModDefinition>>),
    ResolvedMods(broadcast::Sender<Vec<ResolvedMod>>),
}

// TODO: replace with a macro (proc macro required?)
impl InputType {
    fn subscribe(&self) -> OutputType {
        match self {
            InputType::Text(c) => OutputType::Text(c.subscribe()),
            InputType::Files(c) => OutputType::Files(c.subscribe()),
            InputType::List(c) => OutputType::List(c.subscribe()),
            InputType::Mods(c) => OutputType::Mods(c.subscribe()),
            InputType::ResolvedMods(c) => OutputType::ResolvedMods(c.subscribe()),
        }
    }
}

#[derive(Debug)]
pub enum OutputType {
    Text(broadcast::Receiver<String>),
    Files(broadcast::Receiver<FileTree>),
    List(broadcast::Receiver<Vec<String>>),
    Mods(broadcast::Receiver<Vec<ModDefinition>>),
    ResolvedMods(broadcast::Receiver<Vec<ResolvedMod>>),
}

#[derive(Error, Debug)]
pub enum WakeError {
    #[error("Node start failed, have any nodes been spawned? Error: {0}")]
    Send(SendError<bool>),
    #[error("Nodes have already been started or cancelled.")]
    AlreadyAwake,
}

impl DiContainer {
    /// Get a [`FileStore`] linked to all others produced from this [`DiContainer`].
    pub fn get_filestore(&self) -> FileStore {
        self.filestore.clone()
    }

    /// Get a send channel by ID.
    pub fn get_sender(&self, id: &ChannelId) -> Option<InputType> {
        self.channels.get(id).cloned()
    }

    /// Get a receive channel by ID. Will only get messages sent after this method was called.
    pub fn get_receiver(&self, id: &ChannelId) -> Option<OutputType> {
        self.channels.get(id).map(|c| c.subscribe())
    }

    /// Get the waker channel. This channel is intended to be used to synchronize nodes starting.
    /// Each node must wait for a message to be sent to this channel before attempting to receive
    /// inputs from data channels.
    pub fn get_waker(&self) -> broadcast::Receiver<bool> {
        self.waker.subscribe()
    }

    /// Triggers all nodes to start running, can only be called once.
    ///
    /// Will return [`WakeError::Send`] if the broadcast message is not sent.
    /// If called again after nodes have been triggered, will return [`WakeError::AlreadyAwake`].
    pub fn run(&mut self) -> Result<(), WakeError> {
        if self.waker_called {
            Err(WakeError::AlreadyAwake)
        } else {
            match self.waker.send(true) {
                Ok(_) => {
                    self.waker_called = true;
                    // Drop the Sender channels held by the context so that node panics will
                    // propagate through graph.
                    self.channels.clear();
                    Ok(())
                },
                Err(e) => Err(WakeError::Send(e)),
            }
        }
    }

    pub fn cancel(&mut self) -> Result<(), WakeError> {
        if self.waker_called {
            Err(WakeError::AlreadyAwake)
        } else {
            match self.waker.send(false) {
                Ok(_) => {
                    self.waker_called = true;
                    Ok(())
                },
                Err(e) => Err(WakeError::Send(e)),
            }
        }
    }

    /// Get a CurseForge API client, if one is available.
    pub fn get_curse_client(&self) -> Option<CurseClient> {
        self.curse_client.clone()
    }

    /// Get a Modrinth API client. All instances of a [`ModrinthClient`] returned from the same
    /// [`DiContainer`] share the same rate limiter.
    pub fn get_modrinth_client(&self) -> ModrinthClient {
        self.modrinth_client.clone()
    }

    pub fn get_config(&self, key: &str) -> Option<String> {
        self.configs.get(key).cloned()
    }

    pub fn get_cache(&self) -> Option<Arc<dyn Cache>> {
        self.cache.clone()
    }
}

/// Builder for the [`DiContainer`], allowing for channels and API configuration to be set
/// dynamically.
#[derive(Default)]
pub struct DiContainerBuilder {
    channels: HashMap<ChannelId, InputType>,
    curse_client: Option<CurseClient>,
    configs: HashMap<String, String>,
    cache: Option<Box<dyn Cache>>,
}

impl DiContainerBuilder {
    /// Create a Curse API client that points to a proxy service with no API key requirement.
    pub fn curse_client_proxy(mut self, proxy_url: &str) -> Self {
        self.curse_client = Some(CurseClient::from_proxy(proxy_url.to_owned()));
        self
    }

    /// Create a Curse API client that points to the official API, given an API key.
    pub fn curse_client_key(mut self, key: &str) -> Self {
        self.curse_client = Some(CurseClient::from_key(key.to_owned()));
        self
    }

    /// Adds multiple channels sourced from a node.
    pub fn channel_from_node(mut self, channels: HashMap<ChannelId, InputType>) -> Self {
        self.channels.extend(channels);
        self
    }

    pub fn set_config(mut self, key: &str, value: &str) -> Self {
        self.configs.insert(key.to_owned(), value.to_owned());
        self
    }

    pub fn set_cache(mut self, cache: Box<dyn Cache>) -> Self {
        let _ = self.cache.insert(cache);
        self
    }

    /// Construct the [`DiContainer`].
    pub fn build(self) -> DiContainer {
        DiContainer {
            channels: self.channels,
            filestore: FileStore::new(),
            waker: broadcast::channel(1).0,
            waker_called: false,
            curse_client: self.curse_client,
            modrinth_client: ModrinthClient::new(),
            configs: self.configs,
            cache: self.cache.map(Arc::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn filestore_is_cloned() {
        let c = DiContainerBuilder::default().build();
        let fs1 = c.get_filestore();
        let fs2 = c.get_filestore();
        let hash = fs1.write_file("Hello World!".into());

        // Test that both filestores point to the same underlying data
        assert!(fs2.get_file(hash).is_some());
    }

    #[test]
    fn get_sender() {
        let (tx, mut rx) = broadcast::channel::<String>(1);
        let channel_id = ChannelId("node1".into(), "outputA".into());
        let c = DiContainerBuilder::default()
            .channel_from_node(HashMap::from([(channel_id.clone(), InputType::Text(tx))]))
            .build();

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
        let c = DiContainerBuilder::default()
            .channel_from_node(HashMap::from([(channel_id.clone(), InputType::Text(tx.clone()))]))
            .build();

        let mut container_rx = match c.get_receiver(&channel_id).unwrap() {
            OutputType::Text(channel) => channel,
            _ => unreachable!(),
        };
        tx.send("Test".into()).unwrap();

        assert_eq!(container_rx.try_recv().unwrap(), "Test");
    }

    #[test]
    fn get_config() {
        let c = DiContainerBuilder::default().set_config("curse-api-key", "12345678").build();

        assert_eq!(c.get_config("curse-api-key").unwrap(), "12345678");
    }

    #[test]
    fn get_waker_channel() {
        let c = DiContainerBuilder::default().build();

        let mut waker_rx = c.get_waker();
        let waker_tx = c.waker.clone();
        waker_tx.send(true).unwrap();

        assert!(waker_rx.try_recv().is_ok());
    }

    #[test]
    fn curse_client_setup() {
        let key = DiContainerBuilder::default().curse_client_key("12345678").build();
        let proxy = DiContainerBuilder::default().curse_client_proxy("https://example.com/cfapi").build();
        let none = DiContainerBuilder::default().build();

        assert!(key.get_curse_client().is_some());
        assert!(proxy.get_curse_client().is_some());
        assert!(none.get_curse_client().is_none());
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
