use std::{collections::HashMap, thread::{spawn, JoinHandle}};

use serde::Deserialize;
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::{OptionExt, ResultExt};

use crate::{di::container::{DiContainer, InputType, OutputType}, file::filepath::FilePath};

use super::{config::{ChannelId, NodeConfig, NodeInitError}, utils};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct FilePicker;

const FILES: &str = "files";
const PATH: &str = "path";

impl NodeConfig for FilePicker {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = utils::get_output!(ChannelId(node_id.clone(), "default".into()), Text, ctx)?;
        let mut file_input_channel = utils::get_input!(FILES, Files, ctx, input_ids)?;
        let mut path_input_channel = utils::get_input!(PATH, Text, ctx, input_ids)?;
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "FilePicker", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let files = file_input_channel.blocking_recv().expect_or_log("Failed to receive on files input");
            let path = path_input_channel.blocking_recv().expect_or_log("Failed to receive on path input");

            let file = files.get_file(&FilePath::try_from(path.as_ref()).expect_or_log(&format!("Failed to parse provided path: \"{}\"", path))).expect_or_log(&format!("File at path \"{}\" does not exist", path));

            let file_content = String::from_utf8(file.to_vec()).expect_or_log("Failed to read file contents as a UTF-8 encoded string");

            if out_channel.send(file_content).is_err() {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([(ChannelId(node_id.to_owned(), "default".into()), InputType::Text(channel(1).0))])
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use utils::{get_output_test, read_channel};

    use crate::{di::container::DiContainerBuilder, file::{filestore::FileStore, filetree::FileTree}, node::config::NodeConfigTypes};

    use super::*;

    #[test]
    fn test_file_picker() {
        let node_id = "picker_node";

        let file_in_channel = channel(1).0;
        let path_in_channel = channel(1).0;

        let channel_ids = HashMap::from([
            ("files".into(), ChannelId("filetree".into(), "default".into())),
            ("path".into(), ChannelId("path".into(), "default".into())),
        ]);
        let node = NodeConfigTypes::FilePicker(FilePicker);
        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(node.generate_channels(node_id))
            .channel_from_node(HashMap::from([
                    (channel_ids.get("files").unwrap().clone(), InputType::Files(file_in_channel.clone())),
                    (channel_ids.get("path").unwrap().clone(), InputType::Text(path_in_channel.clone())),
            ]))
            .build();

        let mut rx = get_output_test!(&ChannelId::from_str(node_id).unwrap(), Text, ctx);

        let mut files = FileTree::new(FileStore::new());
        files.add_file(FilePath::from_str("manifest.json").unwrap(), "Hello\nWorld!".as_bytes().to_vec());

        let handle = node.validate_and_spawn(node_id.into(), &channel_ids, &ctx).unwrap();

        file_in_channel.send(files).unwrap();
        path_in_channel.send("manifest.json".to_owned()).unwrap();
        ctx.run().unwrap();

        let timeout = Duration::from_secs(30);
        let output: String = read_channel(&mut rx, timeout).unwrap();
        handle.join().unwrap();
        assert_eq!(output, "Hello\nWorld!");
    }
}
