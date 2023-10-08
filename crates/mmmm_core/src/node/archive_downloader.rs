use super::{
    config::{ChannelId, NodeConfig, NodeInitError},
    utils::{get_input, get_output},
};
use crate::{
    di::container::{DiContainer, InputType, OutputType},
    file::{filepath::FilePath, filetree::FileTree},
};
use api_client::common::download_file;
use serde::Deserialize;
use std::io::Cursor;
use std::{
    collections::HashMap,
    io::Read,
    thread::{spawn, JoinHandle},
};
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::ResultExt;
use zip::read::ZipArchive;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ArchiveDownloader;

const URL: &str = "url";

impl NodeConfig for ArchiveDownloader {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = get_output!(ChannelId(node_id.clone(), "default".into()), Files, ctx)?;
        let mut in_channel = get_input!(URL, Text, ctx, input_ids)?;
        let fs = ctx.get_filestore();
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "ArchiveDownloader", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let url = in_channel.blocking_recv().expect_or_log("Failed to receive on url input");
            event!(Level::INFO, "Downloading archive from {}", url);

            let archive = download_file(&url).expect_or_log(&format!("Failed to download archive from URL {url}"));

            let mut zip_archive = ZipArchive::new(Cursor::new(archive)).expect_or_log("Failed to read archive as ZIP");
            let mut filetree = FileTree::new(fs);
            for index in 0..zip_archive.len() {
                let mut file = zip_archive.by_index(index).expect_or_log("Failed to read file from archive");
                if file.is_file() {
                    let mut contents: Vec<u8> = Vec::with_capacity(file.size() as usize);
                    file.read_to_end(&mut contents).unwrap();
                    // As in FilePath, we don't care about properly handling "interesting" paths.
                    let filename = FilePath::try_from(file.mangled_name().as_ref())
                        .expect_or_log(&format!("Filename from archive invalid: {}", file.mangled_name().to_string_lossy()));

                    filetree.add_file(filename, contents);
                }
            }

            if out_channel.send(filetree).is_err() {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([(ChannelId(node_id.to_owned(), "default".into()), InputType::Files(channel(1).0))])
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        di::container::{DiContainerBuilder, InputType},
        file::{filepath::FilePath, filetree::FileTree},
        node::{
            config::{ChannelId, NodeConfigTypes},
            utils::{get_output_test, read_channel},
        },
    };
    use std::{str::FromStr, time::Duration};

    use super::*;

    #[test]
    fn test_archive_downloader() {
        // Setup context and spawn node thread
        let url = "https://cdn.modrinth.com/data/p87Jiw2q/versions/tW5eAKWB/LostEra_Modpack_1.6.1.mrpack";
        let url_channel = tokio::sync::broadcast::channel::<String>(1).0;
        let node_id = "archive_downloader_test";
        let input_ids = HashMap::from([("url".to_string(), ChannelId::from_str("test_node::test_output").unwrap())]);
        let node = NodeConfigTypes::ArchiveDownloader(ArchiveDownloader);
        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(HashMap::from([(
                input_ids.get("url").unwrap().clone(),
                InputType::Text(url_channel.clone()),
            )]))
            .channel_from_node(node.generate_channels(node_id))
            .build();
        let mut output_rx = get_output_test!(ChannelId::from_str(node_id).unwrap(), Files, ctx);
        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        // Wake nodes and simulate dependency node(s)
        url_channel.send(url.to_string()).unwrap();
        ctx.run().unwrap();

        // Get results from node
        let timeout = Duration::from_secs(30);
        let output: FileTree = read_channel(&mut output_rx, timeout).unwrap();
        handle.join().unwrap();
        assert!(output.get_file(&FilePath::from_str("modrinth.index.json").unwrap()).is_some());
    }
}
