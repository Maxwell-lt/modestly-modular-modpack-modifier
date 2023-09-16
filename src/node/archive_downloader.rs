use super::config::{NodeConfig, NodeInitError};
use crate::{
    di::container::{ChannelId, DiContainer, InputType, OutputType},
    filetree::{filepath::FilePath, filetree::FileTree},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Read,
    thread::{spawn, JoinHandle},
};
use zip::read::ZipArchive;

#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveDownloaderNode;

impl NodeConfig for ArchiveDownloaderNode {
    fn validate_and_spawn(&self, node_id: &str, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = {
            let out_id = ChannelId(node_id.into(), "default".into());
            match ctx.get_sender(&out_id).ok_or(NodeInitError::MissingChannel(out_id.clone()))? {
                InputType::Files(channel) => channel,
                _ => return Err(NodeInitError::InvalidOutputType(out_id)),
            }
        };
        let mut in_channel = {
            let in_id = input_ids.get("url").ok_or(NodeInitError::MissingInputId("url".into()))?;
            match ctx.get_receiver(&in_id).ok_or_else(|| NodeInitError::MissingChannel(in_id.clone()))? {
                OutputType::Text(channel) => channel,
                _ => {
                    return Err(NodeInitError::InvalidInputType {
                        input: "url".into(),
                        channel: in_id.clone(),
                    })
                },
            }
        };
        let fs = ctx.get_filestore();
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            waker.blocking_recv().unwrap();
            let url = in_channel.blocking_recv().unwrap();

            let response = reqwest::blocking::get(url).unwrap();
            let archive = response.bytes().unwrap();

            let mut zip_archive = ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
            let mut filetree = FileTree::new(fs);
            for index in 0..zip_archive.len() {
                let mut file = zip_archive.by_index(index).unwrap();
                if file.is_file() {
                    let mut contents: Vec<u8> = Vec::with_capacity(file.size().try_into().unwrap());
                    file.read_to_end(&mut contents).unwrap();
                    let filename = FilePath::try_from(file.enclosed_name().unwrap()).unwrap();

                    filetree.add_file(&filename, contents);
                }
            }

            out_channel.send(filetree).unwrap();
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        di::container::{ChannelId, InputType},
        filetree::{filepath::FilePath, filetree::FileTree},
    };
    use std::thread::sleep;
    use std::{
        str::FromStr,
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn test_archive_downloader() {
        // Setup context and spawn node thread
        let url = "https://cdn.modrinth.com/data/p87Jiw2q/versions/tW5eAKWB/LostEra_Modpack_1.6.1.mrpack";
        let url_channel = tokio::sync::broadcast::channel::<String>(1).0;
        let (output_channel, mut output_rx) = tokio::sync::broadcast::channel::<FileTree>(1);
        let node_id = "archive_downloader_test";
        let input_ids = HashMap::from([("url".to_string(), ChannelId("test_node".to_string(), "test_output".to_string()))]);
        let container_channels = HashMap::from([
            (
                ChannelId("test_node".to_string(), "test_output".to_string()),
                InputType::Text(url_channel.clone()),
            ),
            (
                ChannelId("archive_downloader_test".to_string(), "default".to_string()),
                InputType::Files(output_channel.clone()),
            ),
        ]);
        let ctx = DiContainer::new(HashMap::new(), container_channels);
        let handle = ArchiveDownloaderNode {}.validate_and_spawn(node_id, input_ids, &ctx).unwrap();

        // Wake nodes and simulate dependency node(s)
        url_channel.send(url.to_string()).unwrap();
        ctx.run().unwrap();

        // Get results from node
        let start = Instant::now();
        let timeout = Duration::from_secs(30);
        let interval = Duration::from_millis(250);
        let output: FileTree = loop {
            sleep(interval);
            if Instant::now() - start >= timeout {
                panic!("Timed out waiting for node to complete!");
            }

            match output_rx.try_recv() {
                Ok(res) => break res,
                Err(..) => continue,
            }
        };
        handle.join().unwrap();
        assert!(output.get_file(&FilePath::from_str("modrinth.index.json").unwrap()).is_some());
    }
}
