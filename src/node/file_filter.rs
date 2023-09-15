use std::{collections::HashMap, thread::{JoinHandle, spawn}};

use serde::Deserialize;
use crate::di::container::{ChannelId, DiContainer, InputType, OutputType};

use super::config::{NodeConfig, NodeInitError};

#[derive(Debug, Clone, Deserialize)]
struct FileFilterNode;

impl NodeConfig for FileFilterNode {
    fn validate_and_spawn(&self, node_id: &str, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        let out_channel = {
            let out_id = ChannelId(node_id.into(), "default".into());
            match ctx.get_sender(&out_id).ok_or(NodeInitError::MissingChannel(out_id.clone()))? {
                InputType::Files(channel) => channel,
                _ => return Err(NodeInitError::InvalidOutputType(out_id)),
            }
        };
        let mut file_input_channel = {
            let in_id = input_ids.get("files").ok_or(NodeInitError::MissingInputId("files".into()))?;
            match ctx.get_receiver(&in_id).ok_or_else(|| NodeInitError::MissingChannel(in_id.clone()))? {
                OutputType::Files(channel) => channel,
                _ => return Err(NodeInitError::InvalidInputType { input: "files".into(), channel: in_id.clone() }),
            }
        };
        let mut pattern_input_channel = {
            let in_id = input_ids.get("pattern").ok_or(NodeInitError::MissingInputId("pattern".into()))?;
            match ctx.get_receiver(&in_id).ok_or_else(|| NodeInitError::MissingChannel(in_id.clone()))? {
                OutputType::List(channel) => channel,
                _ => return Err(NodeInitError::InvalidInputType { input: "pattern".into(), channel: in_id.clone() }),
            }
        };
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            waker.blocking_recv().unwrap();
            let source_filetree = file_input_channel.blocking_recv().unwrap();
            let pattern = pattern_input_channel.blocking_recv().unwrap();

            let output_filetree = source_filetree.filter_files(&pattern);
            out_channel.send(output_filetree).unwrap();
        }))
    }
}

#[cfg(test)]
mod test {
    use std::{str::FromStr, time::{Instant, Duration}, thread::sleep};

    use crate::{filetree::{filetree::FileTree, filepath::FilePath}, di::container::InputType};

    use super::*;
    use tokio::sync::broadcast::channel;

    #[test]
    fn test_file_filter() {
        let file_in_channel = channel::<FileTree>(1).0;
        let filter_in_channel = channel::<Vec<String>>(1).0;
        let (out_channel, mut rx) = channel::<FileTree>(1);

        let ctx = DiContainer::new(HashMap::new(), HashMap::from([
                                                                 (ChannelId("source_filetree".into(), "default".into()), InputType::Files(file_in_channel.clone())),
                                                                 (ChannelId("globs".into(), "default".into()), InputType::List(filter_in_channel.clone())),
                                                                 (ChannelId("filter_node".into(), "default".into()), InputType::Files(out_channel)),
        ]));

        let channel_ids = HashMap::from([
                                        ("files".into(), ChannelId("source_filetree".into(), "default".into())),
                                        ("pattern".into(), ChannelId("globs".into(), "default".into())),
        ]);

        let mut source_tree = FileTree::new(ctx.get_filestore());
        source_tree.add_file(&FilePath::from_str("modrinth.index.json").unwrap(), "{}".into());
        source_tree.add_file(&FilePath::from_str("overrides/config/mymod.cfg").unwrap(), "B:MyConfigValue = false".into());

        let filters: Vec<String> = vec!["overrides/**".into()];

        let handle = FileFilterNode{}.validate_and_spawn("filter_node", channel_ids, &ctx).unwrap();

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
        assert_eq!(std::str::from_utf8(&result.get_file(&FilePath::from_str("overrides/config/mymod.cfg").unwrap()).unwrap()).unwrap(), "B:MyConfigValue = false");
    }
}
