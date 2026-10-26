use crate::model::session::RepoSession;
use crate::storage::RepoStoragePaths;
use std::fs;
use std::io;
use std::path::Path;
use tracing::{debug, warn};

/// Manages atomic load/save of `.repolens/session.json`.
pub struct SessionStore;

impl SessionStore {
    pub fn load(repo_root: &Path) -> io::Result<Option<RepoSession>> {
        let path = RepoStoragePaths::session_path(repo_root);
        if !path.exists() {
            return Ok(None);
        }

        let json = fs::read_to_string(&path)?;
        match serde_json::from_str::<RepoSession>(&json) {
            Ok(session) => {
                debug!(session_id = %session.session_id, "Loaded session");
                Ok(Some(session))
            }
            Err(e) => {
                warn!(path = %path.display(), err = %e, "Corrupt session.json, ignoring");
                Ok(None)
            }
        }
    }

    pub fn save(repo_root: &Path, session: &RepoSession) -> io::Result<()> {
        let storage_dir = RepoStoragePaths::storage_dir(repo_root);
        fs::create_dir_all(&storage_dir)?;
        RepoStoragePaths::ensure_storage_ignored(&storage_dir);

        let path = RepoStoragePaths::session_path(repo_root);

        let json = serde_json::to_string_pretty(session).map_err(io::Error::other)?;
        crate::atomic_write::write_atomic(&path, json.as_bytes())?;

        debug!(session_id = %session.session_id, "Saved session");
        Ok(())
    }
}
