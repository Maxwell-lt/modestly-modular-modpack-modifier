use std::{collections::HashMap, thread::{spawn, JoinHandle}};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::channel;
use tracing::{event, span, Level};
use tracing_unwrap::ResultExt;

use crate::di::container::{DiContainer, InputType, OutputType};

use super::{config::{ChannelId, NodeConfig, NodeInitError}, utils::{get_input, get_output}};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ManifestFilter;

impl NodeConfig for ManifestFilter {
    fn validate_and_spawn(
        &self,
        node_id: String,
        input_ids: &HashMap<String, ChannelId>,
        ctx: &DiContainer,
    ) -> Result<JoinHandle<()>, NodeInitError> {
        let mut manifest_channel = get_input!("manifest", Text, ctx, input_ids)?;
        let mut excluded_project_ids_channel = get_input!("excluded_project_ids", List, ctx, input_ids)?;
        let out_channel = get_output!(ChannelId(node_id.clone(), "default".into()), Text, ctx)?;
        let mut waker = ctx.get_waker();
        
        Ok(spawn(move || {
            let _span = span!(Level::INFO, "ManifestFilter", nodeid = node_id).entered();
            if !waker.blocking_recv().unwrap_or_log() {
                panic!()
            }

            let manifest = manifest_channel.blocking_recv().expect_or_log("Failed to receive on manifest input");
            let excluded_project_ids = excluded_project_ids_channel.blocking_recv().expect_or_log("Failed to receive on excluded_project_ids input");
            
            // Parse excluded project IDs as u32
            let excluded_ids: Vec<u32> = excluded_project_ids.iter()
                .filter_map(|id_str| id_str.parse::<u32>().ok())
                .collect();
            
            event!(Level::INFO, "Filtering manifest, excluding {} project IDs: {:?}", excluded_ids.len(), excluded_ids);

            let filtered_manifest = filter_manifest(&manifest, &excluded_ids)
                .expect_or_log("Failed to filter manifest");

            if out_channel.send(filtered_manifest).is_err() {
                event!(Level::DEBUG, "Channel 'default' has no subscribers");
            }
        }))
    }

    fn generate_channels(&self, node_id: &str) -> HashMap<ChannelId, InputType> {
        HashMap::from([
            (ChannelId(node_id.to_owned(), "default".into()), InputType::Text(channel(1).0)),
        ])
    }
}

#[derive(Serialize, Deserialize)]
struct CurseManifest {
    files: Vec<CurseManifestFile>,
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct CurseManifestFile {
    #[serde(rename = "projectID")]
    project_id: u32,
    #[serde(rename = "fileID")]
    file_id: u32,
    required: bool,
}

fn filter_manifest(manifest_json: &str, excluded_project_ids: &[u32]) -> Result<String, Box<dyn std::error::Error>> {
    // Parse the manifest
    let mut manifest: CurseManifest = serde_json::from_str(manifest_json)?;
    
    let original_count = manifest.files.len();
    
    // Filter out excluded project IDs
    manifest.files.retain(|file| !excluded_project_ids.contains(&file.project_id));
    
    let filtered_count = manifest.files.len();
    let removed_count = original_count - filtered_count;
    
    if removed_count > 0 {
        event!(Level::INFO, "Removed {} problematic mods from manifest ({} -> {} mods)", 
               removed_count, original_count, filtered_count);
    }
    
    // Serialize back to JSON
    let filtered_json = serde_json::to_string(&manifest)?;
    Ok(filtered_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_filtering() {
        let manifest = r#"{
            "minecraft": {
                "version": "1.18.2",
                "modLoaders": [{"id": "fabric-0.16.3", "primary": true}]
            },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Test Pack",
            "version": "1.0.0",
            "files": [
                {"projectID": 123456, "fileID": 789, "required": true},
                {"projectID": 550579, "fileID": 4492100, "required": true},
                {"projectID": 654321, "fileID": 987, "required": true}
            ]
        }"#;

        let excluded_ids = vec![550579];
        let result = filter_manifest(manifest, &excluded_ids).unwrap();
        
        let parsed: CurseManifest = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.files.len(), 2);
        assert!(!parsed.files.iter().any(|f| f.project_id == 550579));
        assert!(parsed.files.iter().any(|f| f.project_id == 123456));
        assert!(parsed.files.iter().any(|f| f.project_id == 654321));
    }
}