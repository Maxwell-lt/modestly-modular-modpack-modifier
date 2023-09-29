use super::config::{NodeConfig, NodeInitError};
use super::utils;
use super::utils::log_err;
use crate::{
    di::container::{ChannelId, DiContainer, InputType, OutputType},
    file::{filepath::FilePath, filetree::FileTree},
};
use api_client::common::download_archive;
use serde::Deserialize;
use std::io::Cursor;
use std::{
    collections::HashMap,
    io::Read,
    thread::{spawn, JoinHandle},
};
use tokio::sync::broadcast::channel;
use zip::read::ZipArchive;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ArchiveDownloaderNode;

const URL: &str = "url";

impl NodeConfig for ArchiveDownloaderNode {
    fn validate_and_spawn(&self, node_id: String, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = utils::get_output!(ChannelId(node_id.clone(), "default".into()), Files, ctx)?;
        let mut in_channel = utils::get_input!(URL, Text, ctx, input_ids)?;
        let fs = ctx.get_filestore();
        let mut waker = ctx.get_waker();
        let logger = ctx.get_logger();
        Ok(spawn(move || {
            log_err(waker.blocking_recv(), &logger, &node_id);
            let url = log_err(in_channel.blocking_recv(), &logger, &node_id);

            let archive = log_err(download_archive(&url), &logger, &node_id);

            let mut zip_archive = log_err(ZipArchive::new(Cursor::new(archive)), &logger, &node_id);
            let mut filetree = FileTree::new(fs);
            for index in 0..zip_archive.len() {
                let mut file = log_err(zip_archive.by_index(index), &logger, &node_id);
                if file.is_file() {
                    let mut contents: Vec<u8> = Vec::with_capacity(file.size() as usize);
                    file.read_to_end(&mut contents).unwrap();
                    // As in FilePath, we don't care about properly handling "interesting" paths.
                    let filename = log_err(FilePath::try_from(file.mangled_name().as_ref()), &logger, &node_id);

                    filetree.add_file(filename, contents);
                }
            }

            log_err(out_channel.send(filetree), &logger, &node_id);
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([(ChannelId(node_id.to_owned(), "default".into()), InputType::Files(channel(1).0))])
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        di::{
            container::{ChannelId, InputType},
            logger::LogLevel,
        },
        file::{filepath::FilePath, filetree::FileTree},
        node::{config::NodeConfigTypes, utils::read_channel},
    };
    use std::{str::FromStr, time::Duration};

    use super::*;

    #[test]
    fn test_archive_downloader() {
        // Setup context and spawn node thread
        let url = "https://cdn.modrinth.com/data/p87Jiw2q/versions/tW5eAKWB/LostEra_Modpack_1.6.1.mrpack";
        let url_channel = tokio::sync::broadcast::channel::<String>(1).0;
        let node_id = "archive_downloader_test";
        let input_ids = HashMap::from([("url".to_string(), ChannelId("test_node".to_string(), "test_output".to_string()))]);
        let mut container_channels = HashMap::from([(
            ChannelId("test_node".to_string(), "test_output".to_string()),
            InputType::Text(url_channel.clone()),
        )]);
        let node = NodeConfigTypes::ArchiveDownloaderNode(ArchiveDownloaderNode);
        container_channels.extend(node.generate_channels(node_id).into_iter());
        let ctx = DiContainer::new(HashMap::new(), container_channels);
        let mut output_rx = match ctx.get_receiver(&ChannelId(node_id.into(), "default".into())).unwrap() {
            OutputType::Files(c) => c,
            _ => panic!(),
        };
        let handle = node.validate_and_spawn(node_id.into(), input_ids, &ctx).unwrap();

        // Wake nodes and simulate dependency node(s)
        url_channel.send(url.to_string()).unwrap();
        ctx.run().unwrap();

        // Get results from node
        let timeout = Duration::from_secs(30);
        let output: FileTree = read_channel(&mut output_rx, timeout).unwrap();
        handle.join().unwrap();
        assert!(output.get_file(&FilePath::from_str("modrinth.index.json").unwrap()).is_some());
        assert!(!ctx.get_logger().get_logs().any(|log| log.level == LogLevel::Panic));
    }
}