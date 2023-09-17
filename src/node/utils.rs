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
