use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use crate::di::container::{ChannelId, DiContainer, InputType, OutputType};
use serde::Deserialize;

use super::config::{NodeConfig, NodeInitError};
use super::utils;
use super::utils::log_err;

#[derive(Debug, Clone, Deserialize)]
struct FileFilterNode;

const FILES: &str = "files";
const PATTERN: &str = "pattern";

impl NodeConfig for FileFilterNode {
    fn validate_and_spawn(&self, node_id: String, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = utils::get_output!(ChannelId(node_id.clone(), "default".into()), Files, ctx);
        let mut file_input_channel = utils::get_input!(FILES, Files, ctx, input_ids);
        let mut pattern_input_channel = utils::get_input!(PATTERN, List, ctx, input_ids);
        let mut waker = ctx.get_waker();
        let logger = ctx.get_logger();
        Ok(spawn(move || {
            log_err(waker.blocking_recv(), &logger, &node_id);
            let source_filetree = log_err(file_input_channel.blocking_recv(), &logger, &node_id);
            let pattern = log_err(pattern_input_channel.blocking_recv(), &logger, &node_id);

            let output_filetree = source_filetree.filter_files(&pattern);
            log_err(out_channel.send(output_filetree), &logger, &node_id);
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        str::FromStr,
        thread::sleep,
        time::{Duration, Instant},
    };

    use crate::{
        di::{container::InputType, logger::LogLevel},
        file::{filepath::FilePath, filetree::FileTree},
    };

    use super::*;
    use tokio::sync::broadcast::channel;

    #[test]
    fn test_file_filter() {
        let file_in_channel = channel::<FileTree>(1).0;
        let filter_in_channel = channel::<Vec<String>>(1).0;
        let (out_channel, mut rx) = channel::<FileTree>(1);

        let ctx = DiContainer::new(
            HashMap::new(),
            HashMap::from([
                (
                    ChannelId("source_filetree".into(), "default".into()),
                    InputType::Files(file_in_channel.clone()),
                ),
                (ChannelId("globs".into(), "default".into()), InputType::List(filter_in_channel.clone())),
                (ChannelId("filter_node".into(), "default".into()), InputType::Files(out_channel)),
            ]),
        );

        let channel_ids = HashMap::from([
            ("files".into(), ChannelId("source_filetree".into(), "default".into())),
            ("pattern".into(), ChannelId("globs".into(), "default".into())),
        ]);

        let mut source_tree = FileTree::new(ctx.get_filestore());
        source_tree.add_file(&FilePath::from_str("modrinth.index.json").unwrap(), "{}".into());
        source_tree.add_file(
            &FilePath::from_str("overrides/config/mymod.cfg").unwrap(),
            "B:MyConfigValue = false".into(),
        );

        let filters: Vec<String> = vec!["overrides/**".into()];

        let handle = FileFilterNode {}.validate_and_spawn("filter_node".into(), channel_ids, &ctx).unwrap();

        file_in_channel.send(source_tree).unwrap();
        filter_in_channel.send(filters).unwrap();
        ctx.run().unwrap();

        let start = Instant::now();
        let timeout = Duration::from_secs(30);
        let interval = Duration::from_millis(250);
        let result: FileTree = loop {
            sleep(interval);
            if Instant::now() - start >= timeout {
                panic!("Timed out waiting for node to complete!");
            }

            match rx.try_recv() {
                Ok(res) => break res,
                Err(..) => continue,
            }
        };

        handle.join().unwrap();
        assert_eq!(result.list_files().len(), 1);
        assert_eq!(
            std::str::from_utf8(&result.get_file(&FilePath::from_str("overrides/config/mymod.cfg").unwrap()).unwrap()).unwrap(),
            "B:MyConfigValue = false"
        );
        assert!(!ctx.get_logger().get_logs().any(|log| log.level == LogLevel::Panic));
    }
}
