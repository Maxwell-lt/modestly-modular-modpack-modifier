use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use serde::Deserialize;
use tokio::sync::broadcast::{channel, Receiver};

use crate::di::container::{ChannelId, DiContainer, InputType, OutputType};

use super::{
    config::{NodeConfig, NodeInitError},
    utils::{get_input, get_output, log_err},
};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DirectoryMerger;

impl NodeConfig for DirectoryMerger {
    fn validate_and_spawn(&self, node_id: String, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        // Nothing is stopping us from accepting literally any input names
        // pro: simpler to implement this way
        // con: how do I document to users "just connect these channels to literally any name, just
        // for the DirectoryMerger node"
        let mut input_channels = input_ids
            .keys()
            .cloned()
            .map(|id| get_input!(&id, Files, ctx, input_ids))
            .collect::<Result<Vec<Receiver<_>>, _>>()?;
        let output_channel = get_output!(ChannelId(node_id.clone(), "default".into()), Files, ctx)?;
        let logger = ctx.get_logger();
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            log_err(waker.blocking_recv(), &logger, &node_id);
            let output_dir = log_err(
                input_channels
                    .iter_mut()
                    .map(|channel| log_err(channel.blocking_recv(), &logger, &node_id))
                    .reduce(|mut res, cur| {
                        res.add_all(cur);
                        res
                    })
                    .ok_or(String::from("No inputs passed to DirectoryMerger node!")),
                &logger,
                &node_id,
            );
            log_err(output_channel.send(output_dir), &logger, &node_id);
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([(ChannelId(node_id.to_owned(), "default".into()), InputType::Files(channel(1).0))])
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use crate::{
        file::{filepath::FilePath, filestore::FileStore, filetree::FileTree},
        node::{config::NodeConfigTypes, utils::read_channel},
    };

    use super::*;

    #[test]
    fn merge_directories() {
        let node_id = "merge";
        let store = FileStore::new();
        let alt_store = FileStore::new();
        let mut tree1 = FileTree::new(store.clone());
        let mut tree2 = FileTree::new(store);
        let mut tree3 = FileTree::new(alt_store);
        tree1.add_file(FilePath::from_str("dir/file.txt").unwrap(), "abc".into());
        tree2.add_file(FilePath::from_str("file.json").unwrap(), "def".into());
        tree3.add_file(FilePath::from_str("readme.md").unwrap(), "ghi".into());

        let node = NodeConfigTypes::DirectoryMerger(DirectoryMerger);
        let mut channels = node.generate_channels(node_id);
        let c1 = tokio::sync::broadcast::channel::<FileTree>(1).0;
        let c2 = tokio::sync::broadcast::channel::<FileTree>(1).0;
        let c3 = tokio::sync::broadcast::channel::<FileTree>(1).0;
        //let out = match channels.get(&ChannelId::from_str(node_id).unwrap()).unwrap() { InputType::Files(c) => c.clone(), _ => panic!()};
        channels.insert(ChannelId::from_str("tree1").unwrap(), InputType::Files(c1.clone()));
        channels.insert(ChannelId::from_str("tree2").unwrap(), InputType::Files(c2.clone()));
        channels.insert(ChannelId::from_str("tree3").unwrap(), InputType::Files(c3.clone()));

        let input_ids = HashMap::from([
            ("1".into(), ChannelId::from_str("tree1").unwrap()),
            ("2".into(), ChannelId::from_str("tree2").unwrap()),
            ("3".into(), ChannelId::from_str("tree3").unwrap()),
        ]);

        let ctx = DiContainer::new(HashMap::new(), channels);
        let mut out = match ctx.get_receiver(&ChannelId::from_str(node_id).unwrap()).unwrap() { OutputType::Files(c) => c, _ => panic!() };

        node.validate_and_spawn(node_id.into(), input_ids, &ctx).unwrap();

        c1.send(tree1).unwrap();
        c2.send(tree2).unwrap();
        c3.send(tree3).unwrap();
        ctx.run().unwrap();

        let result = read_channel(&mut out, Duration::from_secs(30)).unwrap();

        assert_eq!(std::str::from_utf8(&result.get_file(&FilePath::from_str("dir/file.txt").unwrap()).unwrap()).unwrap(), "abc");
        assert_eq!(std::str::from_utf8(&result.get_file(&FilePath::from_str("file.json").unwrap()).unwrap()).unwrap(), "def");
        assert_eq!(std::str::from_utf8(&result.get_file(&FilePath::from_str("readme.md").unwrap()).unwrap()).unwrap(), "ghi");
        assert_eq!(result.list_files().len(), 3);
    }
}
