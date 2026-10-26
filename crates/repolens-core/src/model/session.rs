use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::serde_util::iso8601;

/// Wire model for `.repolens/session.json`.
///
/// Mirrors the Java `RepoSession` record.  All timestamp fields use the
/// shared `iso8601` serde helpers for round-trip fidelity with the Java
/// serializer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSession {
    /// Format version, e.g. `"1.0"`.
    pub schema_version: String,

    /// Stable UUID v4 assigned on first scan.
    pub session_id: String,

    /// Absolute path to the repo root (string in wire format).
    pub repo_root: String,

    /// Absolute path to the active pack directory.
    pub active_pack_path: String,

    /// Absolute path to the active report file, if any.
    pub active_report_path: Option<String>,

    /// When the session was last written.
    #[serde(with = "iso8601::required")]
    pub updated_at: OffsetDateTime,

    /// When the session was last accessed.
    #[serde(with = "iso8601::required")]
    pub last_accessed: OffsetDateTime,

    /// Scan mode of the last scan: `"full"` or `"incremental"`.
    pub last_scan_mode: String,

    /// Number of files reprocessed in the last scan.
    pub reprocessed_files: Option<i32>,

    /// Number of files deleted since the base pack.
    pub deleted_files: Option<i32>,

    /// Number of files unchanged since the previous scan; with
    /// `reprocessed_files` this adds up to the file total (#40).
    pub carried_forward_files: Option<i32>,
}
