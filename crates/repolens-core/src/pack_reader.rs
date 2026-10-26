use crate::model::file_entry::FileEntry;
use crate::model::manifest::Manifest;
use crate::path_safety::{validate_prefixed_subpath, validate_relative_subpath};
use std::fs;
use std::io;
use std::path::Path;
use tracing::debug;

/// The result of reading a pack: the manifest plus all file entries from chunks.
#[derive(Debug, Clone)]
pub struct PackIndex {
    pub manifest: Manifest,
    pub entries: Vec<FileEntry>,
}

impl PackIndex {
    /// Build a `PackIndex` directly from in-memory entries, without reading from disk.
    ///
    /// Uses a minimal placeholder manifest. Suitable when the entries are already
    /// in memory (e.g. right after a scan writes the pack) and only `entries` will
    /// be accessed (profile analysis, health analysis).
    pub fn from_entries(entries: Vec<FileEntry>) -> Self {
        let manifest: Manifest = serde_json::from_str(
            r#"{"schemaVersion":"1.0","schemaUrl":"","timestamp":0,"chunks":[]}"#,
        )
        .expect("static manifest JSON is valid");
        Self { manifest, entries }
    }
}

/// Reads an existing pack directory (manifest + chunk files).
pub struct PackReader;

impl PackReader {
    pub fn read(pack_dir: &Path) -> io::Result<PackIndex> {
        let manifest_path = pack_dir.join("manifest.json");
        let manifest_json = fs::read_to_string(&manifest_path)?;
        let manifest: Manifest = serde_json::from_str(&manifest_json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        debug!(pack_dir = %pack_dir.display(), chunks = manifest.chunks.len(), "Reading pack");

        let mut entries = Vec::new();
        for chunk_name in &manifest.chunks {
            validate_relative_subpath(chunk_name).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid chunk name in manifest: {chunk_name}: {e}"),
                )
            })?;
            let chunk_path = pack_dir.join(chunk_name);
            let chunk_json = fs::read_to_string(&chunk_path)?;
            let chunk_entries: Vec<FileEntry> = serde_json::from_str(&chunk_json)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            for entry in &chunk_entries {
                if let Some(content_path) = entry.content_path.as_deref() {
                    validate_prefixed_subpath(content_path, "files").map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("invalid content path for {}: {e}", entry.path),
                        )
                    })?;
                }
            }
            entries.extend(chunk_entries);
        }

        debug!(total_entries = entries.len(), "Pack read complete");
        Ok(PackIndex { manifest, entries })
    }
}
