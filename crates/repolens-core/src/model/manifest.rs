use serde::{Deserialize, Serialize};

use crate::serde_util::java_epoch_seconds::JavaEpochSeconds;

/// Wire-model for the `manifest.json` file emitted by the Java pack writer.
///
/// All camelCase JSON keys are mapped via `#[serde(rename_all = "camelCase")]`.
/// Optional fields that the Java writer omits (`null`) use `Option<T>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: String,
    pub schema_url: String,
    pub timestamp: JavaEpochSeconds,
    pub chunks: Vec<String>,
    pub scan_mode: Option<String>,
    pub scan_scope: Option<String>,
    pub base_pack_id: Option<String>,
    pub base_head_commit: Option<String>,
    pub head_commit: Option<String>,
    pub worktree_dirty: Option<bool>,
    pub scan_config_hash: Option<String>,
    pub reprocessed_files: Option<i32>,
    pub deleted_files: Option<i32>,
    pub carried_forward_files: Option<i32>,
}
