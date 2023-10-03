use std::{
    collections::HashMap,
    thread::{spawn, JoinHandle},
};

use thiserror::Error;
use tokio::sync::broadcast::{channel, error::SendError};

use crate::di::{
    container::{DiContainer, InputType},
    logger::LogLevel,
};

use super::{
    config::{ChannelId, ModDefinition, NodeInitError, SourceDefinition, SourceValue},
    utils::log_err,
};

pub struct Source<'a> {
    sources: &'a [&'a SourceDefinition],
}

impl<'a> Source<'a> {
    pub fn new(sources: &'a [&'a SourceDefinition]) -> Self {
        Source { sources }
    }

    pub fn generate_channels(&self) -> HashMap<ChannelId, InputType> {
        self.sources
            .into_iter()
            .map(|source| {
                (
                    ChannelId(source.id.clone(), "default".into()),
                    match source.value {
                        SourceValue::Text(_) => InputType::Text(channel(1).0),
                        SourceValue::List(_) => InputType::List(channel(1).0),
                        SourceValue::Mods(_) => InputType::Mods(channel(1).0),
                    },
                )
            })
            .collect()
    }

    pub fn spawn(self, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        let resolved_channels = self
            .sources
            .into_iter()
            .map(|s| {
                let channel_id = ChannelId(s.id.clone(), "default".into());
                ctx.get_sender(&channel_id)
                    .ok_or_else(|| NodeInitError::MissingChannel(channel_id.clone()))
                    .map(|r| (s, r, channel_id))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(
                |(source, sender, channel_id)| -> Result<Box<dyn Fn() -> Result<usize, GenericSendError> + Send>, NodeInitError> {
                    match sender {
                        InputType::Text(channel) => {
                            if let SourceValue::Text(ref value) = source.value {
                                let cloned_val = value.clone();
                                Ok(Box::new(move || Ok(channel.send(cloned_val.to_owned())?)))
                            } else {
                                Err(NodeInitError::InvalidInputType {
                                    input: source.id.clone(),
                                    channel: channel_id,
                                })
                            }
                        },
                        InputType::List(channel) => {
                            if let SourceValue::List(ref value) = source.value {
                                let cloned_val = value.clone();
                                Ok(Box::new(move || Ok(channel.send(cloned_val.to_owned())?)))
                            } else {
                                Err(NodeInitError::InvalidInputType {
                                    input: source.id.clone(),
                                    channel: channel_id,
                                })
                            }
                        },
                        InputType::Mods(channel) => {
                            if let SourceValue::Mods(ref value) = source.value {
                                let cloned_val = value.clone();
                                Ok(Box::new(move || Ok(channel.send(cloned_val.to_owned())?)))
                            } else {
                                Err(NodeInitError::InvalidInputType {
                                    input: source.id.clone(),
                                    channel: channel_id,
                                })
                            }
                        },
                        _ => Err(NodeInitError::InvalidInputType {
                            input: source.id.clone(),
                            channel: channel_id,
                        }),
                    }
                },
            )
            .collect::<Result<Vec<_>, NodeInitError>>()?;
        let mut waker = ctx.get_waker();
        let logger = ctx.get_logger();
        Ok(spawn(move || {
            let should_run = log_err(waker.blocking_recv(), &logger, "Sources Node");
            if !should_run {
                panic!()
            }
            for error in resolved_channels.into_iter().map(|f| f()).filter_map(|r| r.err()) {
                logger.log(
                    "Sources Node".into(),
                    LogLevel::Warning,
                    "Failed to send data to channel, missing receiver?".into(),
                    Some(vec![error.to_string()]),
                );
            }
        }))
    }
}

#[derive(Debug, Error)]
pub enum GenericSendError {
    #[error("Send Error: {0}")]
    Text(#[from] SendError<String>),
    #[error("Send Error: {0}")]
    List(#[from] SendError<Vec<String>>),
    #[error("Send Error: {0}")]
    Mods(#[from] SendError<Vec<ModDefinition>>),
}
