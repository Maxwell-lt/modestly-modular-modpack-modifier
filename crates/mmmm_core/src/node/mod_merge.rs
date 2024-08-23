use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use serde::Deserialize;
use tokio::sync::broadcast::{channel, Receiver};
use tracing::{event, span, Level};
use tracing_unwrap::{OptionExt, ResultExt};

use crate::di::container::{DiContainer, InputType, OutputType};

use super::{
    config::{ChannelId, NodeConfig, NodeInitError, ResolvedMod},
    utils::{get_input, get_output},
};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModMerger;

impl NodeConfig for ModMerger {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let mut input_channels = {
            let mut keys = input_ids.keys().cloned().collect::<Vec<_>>();
            // Ordering matters when input modlists have mods with matching names. Sort by input
            // names to ensure deterministic behavior.
            // Sorting is reversed; earlier input names take priority with overlapping mods.
            keys.sort_unstable_by(|a, b| b.cmp(a));
            keys.into_iter()
                .map(|id| get_input!(&id, ResolvedMods, ctx, input_ids).map(|channel| (id, channel)))
                .collect::<Result<Vec<(String, Receiver<_>)>, _>>()
        }?;
        let output_channel = get_output!(ChannelId(node_id.clone(), "default".into()), ResolvedMods, ctx)?;
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "ModMerger", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let output_list: Vec<ResolvedMod> = input_channels
                .iter_mut()
                .map(|(id, channel)| channel.blocking_recv().expect_or_log(&format!("Failed to receive on {id} input")))
                .map(|list| {
                    list.into_iter()
                        .map(|mod_def| (mod_def.name.to_owned(), mod_def))
                        .collect::<HashMap<_, _>>()
                })
                .reduce(|mut res, cur| {
                    res.extend(cur.into_iter());
                    res
                })
                .expect_or_log(&format!("No inputs passed to ModMerger node with id {node_id}"))
                .into_values()
                .collect();
            if output_channel.send(output_list).is_err() {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([(ChannelId(node_id.to_owned(), "default".into()), InputType::ResolvedMods(channel(1).0))])
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use crate::{
        di::container::DiContainerBuilder,
        node::{
            config::{NodeConfigTypes, Side},
            utils::{get_output_test, read_channel},
        },
    };

    use super::*;

    #[test]
    fn merge_mods() {
        let node_id = "merge";
        let list1: Vec<ResolvedMod> = vec![ResolvedMod {
            name: "thaumcraft7".to_owned(),
            title: String::new(),
            side: Side::Both,
            required: true,
            default: true,
            filename: String::new(),
            encoded: String::new(),
            src: String::new(),
            size: 0,
            md5: String::new(),
            sha256: String::new(),
        }];

        let list2: Vec<ResolvedMod> = vec![
            ResolvedMod {
                name: "redpower3".to_owned(),
                title: String::new(),
                side: Side::Both,
                required: true,
                default: true,
                filename: String::new(),
                encoded: String::new(),
                src: String::new(),
                size: 0,
                md5: String::new(),
                sha256: String::new(),
            },
            ResolvedMod {
                name: "thaumcraft7".to_owned(),
                title: String::new(),
                side: Side::Server,
                required: true,
                default: true,
                filename: String::new(),
                encoded: String::new(),
                src: String::new(),
                size: 0,
                md5: String::new(),
                sha256: String::new(),
            },
        ];

        let node = NodeConfigTypes::ModMerger(ModMerger);
        let c1 = channel::<Vec<ResolvedMod>>(1).0;
        let c2 = channel::<Vec<ResolvedMod>>(1).0;
        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(node.generate_channels(node_id))
            .channel_from_node(HashMap::from([
                (ChannelId::from_str("list1").unwrap(), InputType::ResolvedMods(c1.clone())),
                (ChannelId::from_str("list2").unwrap(), InputType::ResolvedMods(c2.clone())),
            ]))
            .build();

        let input_ids = HashMap::from([
            ("1".into(), ChannelId::from_str("list1").unwrap()),
            ("2".into(), ChannelId::from_str("list2").unwrap()),
        ]);

        let mut out = get_output_test!(&ChannelId::from_str(node_id).unwrap(), ResolvedMods, ctx);

        node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        c1.send(list1.clone()).unwrap();
        c2.send(list2.clone()).unwrap();
        ctx.run().unwrap();

        let mut result = read_channel(&mut out, Duration::from_secs(30)).unwrap();
        assert_eq!(Vec::from([list1[0].clone(), list2[0].clone()]).sort(), result.sort());
    }
}
