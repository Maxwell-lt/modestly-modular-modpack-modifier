macro_rules! get_output {
    ($channel:expr, $variant:ident, $context:expr) => {
        match $context
            .get_sender(&$channel)
            .ok_or_else(|| NodeInitError::MissingChannel($channel.clone()))?
        {
            InputType::$variant(c) => Ok(c),
            _ => Err(NodeInitError::InvalidOutputType($channel)),
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
            OutputType::$variant(c) => Ok(c),
            _ => Err(NodeInitError::InvalidInputType {
                input: $input_name.to_owned(),
                channel: channel_id.clone(),
            }),
        }
    }};
}

pub(super) use get_input;
pub(super) use get_output;

#[cfg(test)]
pub mod test_only {
    use std::io::Read;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use serde::Deserialize;
    use thiserror::Error;
    use tokio::sync::broadcast::Receiver;
    pub fn read_channel<T: Clone>(channel: &mut Receiver<T>, timeout: Duration) -> Result<T, &str> {
        let start = Instant::now();
        let interval = Duration::from_millis(50);
        loop {
            if let Ok(res) = channel.try_recv() {
                break Ok(res);
            }

            sleep(interval);
            if Instant::now() - start >= timeout {
                break Err("Timed out waiting for node to complete!");
            }
        }
    }

    macro_rules! _get_output_test {
        ($channel:expr, $variant:ident, $context:expr) => {
            match $context.get_receiver(&$channel).unwrap() {
                OutputType::$variant(c) => c,
                _ => panic!(),
            }
        };
    }
    // Hack to put the macro in a non-root path
    pub(crate) use _get_output_test as get_output_test;

    #[derive(Deserialize)]
    pub struct Config {
        pub curse_api_key: String,
    }

    #[derive(Debug, Error)]
    pub enum CurseConfigError {
        #[error("Could not open file! Error: {0}")]
        Io(#[from] std::io::Error),
        #[error("Could not parse TOML! Error: {0}")]
        Toml(#[from] toml::de::Error),
    }

    pub fn get_curse_config() -> Result<Config, CurseConfigError> {
        // Tests are run in crate root, go two directories up to find config.
        let mut file = std::fs::File::open("../../mmmm.toml")?;
        let mut data = String::new();
        file.read_to_string(&mut data)?;
        Ok(toml::from_str::<Config>(&data)?)
    }
}

#[cfg(test)]
pub use test_only::*;
