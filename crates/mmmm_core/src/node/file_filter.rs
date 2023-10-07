use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use crate::di::container::{DiContainer, InputType, OutputType};
use serde::Deserialize;
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::ResultExt;

use super::config::{NodeConfig, NodeInitError};
use super::{config::ChannelId, utils};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct FileFilter;

const FILES: &str = "files";
const PATTERN: &str = "pattern";

impl NodeConfig for FileFilter {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = utils::get_output!(ChannelId(node_id.clone(), "default".into()), Files, ctx)?;
        let inverse_channel = utils::get_output!(ChannelId(node_id.clone(), "inverse".into()), Files, ctx)?;
        let mut file_input_channel = utils::get_input!(FILES, Files, ctx, input_ids)?;
        let mut pattern_input_channel = utils::get_input!(PATTERN, List, ctx, input_ids)?;
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "FileFilter", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let source_filetree = file_input_channel.blocking_recv().expect_or_log("Failed to receive on files input");
            let pattern = pattern_input_channel.blocking_recv().expect_or_log("Failed to receive on pattern input");

            let (output_filetree, inverse_filetree) = source_filetree.filter_files(&pattern);

            if let Err(_) = out_channel.send(output_filetree) {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
            if let Err(_) = inverse_channel.send(inverse_filetree) {
                event!(Level::DEBUG, "Channel 'inverse' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([
            (ChannelId(node_id.to_owned(), "default".into()), InputType::Files(channel(1).0)),
            (ChannelId(node_id.to_owned(), "inverse".into()), InputType::Files(channel(1).0)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use crate::{
        di::container::{DiContainerBuilder, InputType},
        file::{filepath::FilePath, filetree::FileTree},
        node::{
            config::NodeConfigTypes,
            utils::{get_output_test, read_channel},
        },
    };

    use super::*;
    use tokio::sync::broadcast::channel;

    #[test]
    fn test_file_filter() {
        let node_id = "filter_node";
        let file_in_channel = channel::<FileTree>(1).0;
        let filter_in_channel = channel::<Vec<String>>(1).0;

        let channel_ids = HashMap::from([
            ("files".into(), ChannelId("source_filetree".into(), "default".into())),
            ("pattern".into(), ChannelId("globs".into(), "default".into())),
        ]);
        let node = NodeConfigTypes::FileFilter(FileFilter);
        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(node.generate_channels(node_id))
            .channel_from_node(HashMap::from([
                (channel_ids.get("files").unwrap().clone(), InputType::Files(file_in_channel.clone())),
                (channel_ids.get("pattern").unwrap().clone(), InputType::List(filter_in_channel.clone())),
            ]))
            .build();

        let mut rx = get_output_test!(&ChannelId::from_str(node_id).unwrap(), Files, ctx);
        let mut inverse_rx = get_output_test!(&ChannelId(node_id.into(), "inverse".into()), Files, ctx);

        let mut source_tree = FileTree::new(ctx.get_filestore());
        source_tree.add_file(FilePath::from_str("modrinth.index.json").unwrap(), "{}".into());
        source_tree.add_file(
            FilePath::from_str("overrides/config/mymod.cfg").unwrap(),
            "B:MyConfigValue = false".into(),
        );

        let filters: Vec<String> = vec!["overrides/**".into()];

        let handle = node.validate_and_spawn(node_id.into(), &channel_ids, &ctx).unwrap();

        file_in_channel.send(source_tree).unwrap();
        filter_in_channel.send(filters).unwrap();
        ctx.run().unwrap();

        let timeout = Duration::from_secs(30);
        let output: FileTree = read_channel(&mut rx, timeout).unwrap();
        let inverse: FileTree = read_channel(&mut inverse_rx, timeout).unwrap();
        handle.join().unwrap();
        assert_eq!(output.list_files().len(), 1);
        assert_eq!(
            std::str::from_utf8(&output.get_file(&FilePath::from_str("overrides/config/mymod.cfg").unwrap()).unwrap()).unwrap(),
            "B:MyConfigValue = false"
        );
        assert_eq!(
            std::str::from_utf8(&inverse.get_file(&FilePath::from_str("modrinth.index.json").unwrap()).unwrap()).unwrap(),
            "{}"
        );
    }
}
