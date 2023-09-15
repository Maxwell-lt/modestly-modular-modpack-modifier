use std::{collections::HashMap, thread::JoinHandle};

use serde::Deserialize;
use crate::di::container::{ChannelId, DiContainer};

use super::config::{NodeConfig, NodeInitError};

#[derive(Debug, Clone, Deserialize)]
struct FileFilterNode;

impl NodeConfig for FileFilterNode {
    fn validate_and_spawn(&self, node_id: &str, input_ids: HashMap<String, ChannelId>, ctx: &DiContainer) -> Result<JoinHandle<()>, NodeInitError> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_file_filter() {
        todo!();
    }
}
