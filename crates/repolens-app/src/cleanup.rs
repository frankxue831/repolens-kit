use std::fs;
use std::path::Path;

use anyhow::Result;
use tracing::{info, warn};

use repolens_core::model::manifest::Manifest;
use repolens_core::session::SessionStore;
use repolens_core::storage::RepoStoragePaths;

/// Default number of packs to retain.
pub const DEFAULT_KEEP_COUNT: usize = 3;

/// Summary of a cleanup run.
pub struct CleanupResult {
    /// Pack directory names that were successfully deleted.
    pub deleted: Vec<String>,
    /// Pack directory names that failed to delete.
    pub failed: Vec<String>,
    /// Total packs remaining after cleanup.
    pub kept: usize,
}

/// Remove old pack directories beyond the retention limit.
///
/// Keeps the newest `keep` packs plus the active session pack and the newest
/// clean pack (the incremental base), if either falls outside the newest
/// `keep`. Deletion failures are logged and recorded
/// in `CleanupResult::failed` but do not abort the cleanup.
pub fn cleanup_old_packs(repo_root: &Path, keep: usize) -> Result<CleanupResult> {
    let storage_dir = RepoStoragePaths::storage_dir(repo_root);
    if !storage_dir.exists() {
        return Ok(CleanupResult {
            deleted: vec![],
            failed: vec![],
            kept: 0,
        });
    }

    // 1. List pack-* directories, sorted newest-first
    let mut pack_dirs: Vec<String> = fs::read_dir(&storage_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                && e.file_name().to_string_lossy().starts_with("pack-")
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    pack_dirs.sort();
    pack_dirs.reverse(); // newest first

    if pack_dirs.len() <= keep {
        return Ok(CleanupResult {
            deleted: vec![],
            failed: vec![],
            kept: pack_dirs.len(),
        });
    }

    // 2. Determine protected active pack
    let active_pack_name = resolve_active_pack_name(repo_root, &storage_dir);

    // 3. Build retained set: newest N + protected
    let mut retained: std::collections::HashSet<&str> =
        pack_dirs.iter().take(keep).map(|s| s.as_str()).collect();

    if let Some(ref name) = active_pack_name {
        retained.insert(name.as_str());
    }

    // Incremental scans only use a clean pack as their base, so keep the
    // newest one even when dirty scans have pushed it out of the newest N.
    // Without it every later scan is a full scan (#37).
    if let Some(name) = pack_dirs
        .iter()
        .find(|name| is_clean_base_candidate(&storage_dir.join(name)))
    {
        retained.insert(name.as_str());
    }

    // 4. Delete non-retained packs
    let mut deleted = Vec::new();
    let mut failed = Vec::new();

    for name in &pack_dirs {
        if retained.contains(name.as_str()) {
            continue;
        }
        let path = storage_dir.join(name);
        match fs::remove_dir_all(&path) {
            Ok(()) => {
                info!(pack = %name, "Deleted old pack");
                deleted.push(name.clone());
            }
            Err(e) => {
                warn!(pack = %name, error = %e, "Failed to delete old pack");
                failed.push(name.clone());
            }
        }
    }

    let kept = pack_dirs.len() - deleted.len();

    Ok(CleanupResult {
        deleted,
        failed,
        kept,
    })
}

/// Whether the pack's manifest says it was a complete scan of a clean
/// worktree: the precondition for being an incremental base pack.
fn is_clean_base_candidate(pack_dir: &Path) -> bool {
    let Ok(json) = fs::read_to_string(pack_dir.join("manifest.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<Manifest>(&json) else {
        return false;
    };
    manifest.worktree_dirty == Some(false) && manifest.scan_scope.as_deref() == Some("complete")
}

/// Extract the active pack directory name from session.json.
///
/// Returns `None` if session is missing, corrupt, or the active pack path
/// doesn't resolve to a valid pack directory under this repo's `.repolens/`.
fn resolve_active_pack_name(repo_root: &Path, storage_dir: &Path) -> Option<String> {
    let session = SessionStore::load(repo_root).ok()??;

    let active_path = std::path::PathBuf::from(&session.active_pack_path);
    if !active_path.is_dir() {
        return None;
    }

    // Verify it lives under this repo's .repolens/
    let parent = active_path.parent()?;
    if parent != storage_dir {
        let canonical_parent = parent.canonicalize().ok()?;
        let canonical_storage = storage_dir.canonicalize().ok()?;
        if canonical_parent != canonical_storage {
            return None;
        }
    }

    let name = active_path.file_name()?.to_string_lossy().to_string();
    if name.starts_with("pack-") {
        Some(name)
    } else {
        None
    }
}
