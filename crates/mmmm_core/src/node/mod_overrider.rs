use std::{collections::HashMap, thread::{spawn, JoinHandle}};

use serde::Deserialize;
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::ResultExt;

use crate::di::container::{DiContainer, InputType, OutputType};

use super::{config::{ChannelId, NodeConfig, NodeInitError}, utils::{get_input, get_output}};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModOverrider;

impl NodeConfig for ModOverrider {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let mut mods_channel = get_input!("mods", ResolvedMods, ctx, input_ids)?;
        let mut overrides_channel = get_input!("overrides", Mods, ctx, input_ids)?;
        let out_channel = get_output!(ChannelId(node_id.clone(), "default".into()), ResolvedMods, ctx)?;
        let mut waker = ctx.get_waker();
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "ModOverrider", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let mut mods = mods_channel.blocking_recv().expect_or_log("Failed to receive on mods input");
            let overrides = overrides_channel.blocking_recv().expect_or_log("Failed to receive on overrides input");

            let overrides_map = overrides.into_iter()
                .map(|m| (m.get_fields().name.clone(), m))
                .collect::<HashMap<_, _>>();

            mods.iter_mut().for_each(|m| {
                let o = overrides_map.get(&m.name);
                if let Some(o) = o {
                    m.side = o.get_fields().side;
                    if let Some(required) = o.get_fields().required {
                        m.required = required;
                    }
                    if let Some(default) = o.get_fields().default {
                        m.default = default;
                    }
                }
            });

            if out_channel.send(mods).is_err() {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([
            (ChannelId(node_id.to_owned(), "default".into()), InputType::ResolvedMods(channel(1).0)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use tokio::sync::broadcast;

    use crate::{di::container::DiContainerBuilder, node::{config::{ModDefinition, ModDefinitionFields, NodeConfigTypes, ResolvedMod, Side}, utils::{get_output_test, read_channel}}};

    use super::*;

    #[test]
    fn test_mod_overrider() {
        let node_id = "overrider";
        let mods_channel = broadcast::channel(1).0;
        let overrides_channel = broadcast::channel(1).0;
        let input_ids = HashMap::from([
            ("mods".into(), ChannelId::from_str("mod-source").unwrap()),
            ("overrides".into(), ChannelId::from_str("overrides-source").unwrap()),
        ]);
        let node = NodeConfigTypes::ModOverrider(ModOverrider);

        let mut ctx = DiContainerBuilder::default()
            .channel_from_node(node.generate_channels(&node_id))
            .channel_from_node(HashMap::from([
                    (ChannelId::from_str("mod-source").unwrap(), InputType::ResolvedMods(mods_channel.clone())),
                    (ChannelId::from_str("overrides-source").unwrap(), InputType::Mods(overrides_channel.clone())),
            ]))
            .build();

        let mut out_channel = get_output_test!(ChannelId::from_str("overrider").unwrap(), ResolvedMods, ctx);

        let mut resolved_mods = vec![
            ResolvedMod {
                title: "AppleSkin".to_owned(),
                name: "appleskin".to_owned(),
                side: Side::Both,
                required: true,
                default: true,
                filename: "AppleSkin-mc1.12-1.0.14.jar".to_owned(),
                encoded: "AppleSkin-mc1.12-1.0.14.jar".to_owned(),
                src: "https://cdn.modrinth.com/data/EsAfCjCV/versions/Tsz4BT2X/AppleSkin-mc1.12-1.0.14.jar".to_owned(),
                size: 33683,
                md5: "b435860d5cfa23bc53d3b8e120be91d4".to_owned(),
                sha256: "4bbd37edecff0b420ab0eea166b5d7b4b41a9870bfb8647bf243140dc57f101e".to_owned(),
            },
            ResolvedMod {
                title: "Mouse Tweaks".to_owned(),
                name: "mouse-tweaks".to_owned(),
                side: Side::Client,
                required: true,
                default: true,
                filename: "MouseTweaks-2.10.1-mc1.12.2.jar".to_owned(),
                encoded: "MouseTweaks-2.10.1-mc1.12.2.jar".to_owned(),
                src: "https://edge.forgecdn.net/files/3359/843/MouseTweaks-2.10.1-mc1.12.2.jar".to_owned(),
                size: 80528,
                md5: "a6034d3ff57091c78405e46f1f926282".to_owned(),
                sha256: "5e13315f4e0d0c96b1f9b800a42fecb89f519aca81d556c91df617c8751aa575".to_owned(),
            },
            ResolvedMod {
                title: "title-changer".to_owned(),
                name: "title-changer".to_owned(),
                side: Side::Client,
                required: false,
                default: true,
                filename: "titlechanger-1.1.3.jar".to_owned(),
                encoded: "titlechanger-1.1.3.jar".to_owned(),
                src: "https://github.com/Maxwell-lt/TitleChanger/releases/download/1.1.3/titlechanger-1.1.3.jar".to_owned(),
                size: 5923,
                md5: "8fda92da93d78919cff1139e847d3e1c".to_owned(),
                sha256: "78bbe270f2f2ca443a4e794ee1f0c5920ef933ce1030bae0dcff45cb16689eb7".to_owned(),
            },
        ];

        let filters = vec![
            ModDefinition::Curse { id: None, file_id: None, fields: ModDefinitionFields { name: "appleskin".into(), side: Side::Client, required: Some(false), default: None } },
            ModDefinition::Curse { id: None, file_id: None, fields: ModDefinitionFields { name: "mouse-tweaks".into(), side: Side::Client, required: None, default: Some(false) } },
        ];

        let handle = node.validate_and_spawn(node_id.into(), &input_ids, &ctx).unwrap();

        ctx.run().unwrap();
        mods_channel.send(resolved_mods.clone()).unwrap();
        overrides_channel.send(filters).unwrap();
        handle.join().unwrap();

        let timeout = Duration::from_secs(30);
        let output: Vec<ResolvedMod> = read_channel(&mut out_channel, timeout).unwrap();

        // Perform expected modifications to mod list
        resolved_mods[0].side = Side::Client;
        resolved_mods[0].required = false;
        resolved_mods[1].default = false;
        let expected = resolved_mods;

        assert_eq!(output, expected);
    }
}
