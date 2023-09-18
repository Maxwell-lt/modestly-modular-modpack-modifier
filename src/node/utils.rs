macro_rules! get_output {
    ($channel:expr, $variant:ident, $context:expr) => {
        match $context
            .get_sender(&$channel)
            .ok_or_else(|| NodeInitError::MissingChannel($channel.clone()))?
        {
            InputType::$variant(c) => c,
            _ => return Err(NodeInitError::InvalidOutputType($channel)),
        }
    };
}

macro_rules! get_input {
    ($input_name:expr, $variant:ident, $context:expr, $id_mappings:expr) => {{
        let channel_id = $id_mappings
            .get($input_name)
            .ok_or_else(|| NodeInitError::MissingInputId($input_name.into()))?;
        match $context
            .get_receiver(&channel_id)
            .ok_or_else(|| NodeInitError::MissingChannel(channel_id.to_owned()))?
        {
            OutputType::$variant(c) => c,
            _ => {
                return Err(NodeInitError::InvalidInputType {
                    input: $input_name.to_owned(),
                    channel: channel_id.clone(),
                })
            },
        }
    }};
}

pub(super) use get_input;
pub(super) use get_output;

use crate::di::logger::{LogLevel, Logger};

/// Unwrap a [`Result`], but pass a message to a provided [`Logger`] before panicking on [`Err`].
///
/// Current implementation does not allow caller to attach a custom error message.
pub(super) fn log_err<T, E>(result: Result<T, E>, logger: &Logger, id: &str) -> T
where
    E: std::fmt::Display,
{
    match result {
        Ok(val) => val,
        Err(e) => {
            logger.log(id.into(), LogLevel::Panic, "Something went wrong!".to_string(), Some(vec![e.to_string()]));
            panic!();
        },
    }
}
