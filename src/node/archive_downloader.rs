use super::config::{NodeConfig, NodeInitError};
use serde::Deserialize;
use crate::di::container::DiContainer;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveDownloaderNode;

impl NodeConfig for ArchiveDownloaderNode {
    fn validate_and_spawn(&self, input_ids: HashMap<String,String>, ctx: DiContainer) -> Result<(), NodeInitError> {
       todo!()
    }
}
