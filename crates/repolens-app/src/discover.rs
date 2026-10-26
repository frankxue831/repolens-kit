use anyhow::{anyhow, bail, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

use repolens_core::pack_reader::{PackIndex, PackReader};
use repolens_core::session::SessionStore;
use repolens_core::storage::RepoStoragePaths;

/// Pack-id grammar: `pack-YYYYMMDD-HHMMSS` or the bare `YYYYMMDD-HHMMSS` form.
static PACK_ID_GRAMMAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(pack-)?\d{8}-\d{6}$").unwrap());

/// Accepts `pack-YYYYMMDD-HHMMSS` or the bare `YYYYMMDD-HHMMSS` form and
/// returns the canonical `pack-*` directory name. Rejects anything else —
/// in particular, absolute paths, path separators, `..` segments, and
/// arbitrary names that merely start with `pack-`.
///
/// Lifted from `repolens-cli/src/compare.rs` in the 2026-05-13 discover_packs
/// expansion so MCP handlers can resolve `packId` aliases against `root`
/// without duplicating the grammar check.
pub fn validate_pack_id(raw: &str) -> Result<String> {
    if raw.is_empty() || raw.contains('/') || raw.contains('\\') || raw.contains("..") {
        bail!("Invalid pack id: {raw}. Expected 'pack-YYYYMMDD-HHMMSS' or 'YYYYMMDD-HHMMSS'.");
    }
    if !PACK_ID_GRAMMAR.is_match(raw) {
        bail!("Invalid pack id: {raw}. Expected 'pack-YYYYMMDD-HHMMSS' or 'YYYYMMDD-HHMMSS'.");
    }
    if let Some(stripped) = raw.strip_prefix("pack-") {
        Ok(format!("pack-{stripped}"))
    } else {
        Ok(format!("pack-{raw}"))
    }
}

/// Cheap presence check: does `raw` look like a pack-id rather than a path?
/// Used by MCP handlers to decide whether to resolve `raw` against a root
/// (`packId` alias path) or treat it as an absolute pack directory path.
pub fn looks_like_pack_id(raw: &str) -> bool {
    !raw.contains('/') && !raw.contains('\\') && PACK_ID_GRAMMAR.is_match(raw)
}

/// Resolve a pack reference — either an absolute path or a pack-id — against
/// the storage directory under `root`. Returns the canonical pack directory
/// path. Errors when the pack doesn't exist on disk.
pub fn resolve_pack_ref(raw: &str, root: &Path) -> Result<PathBuf> {
    if looks_like_pack_id(raw) {
        let canonical = validate_pack_id(raw)?;
        let candidate = RepoStoragePaths::storage_dir(root).join(&canonical);
        if !candidate.join("manifest.json").exists() {
            return Err(anyhow!(
                "Pack not found under {}: {canonical}",
                RepoStoragePaths::storage_dir(root).display()
            ));
        }
        Ok(candidate)
    } else {
        Ok(PathBuf::from(raw))
    }
}

/// Result of discovering packs for a repository.
///
/// The 2026-05-13 dogfood loop found that returning only `latest_pack`
/// forced agents to shell-list `.repolens/` to enumerate other packs for
/// `compare_summaries`. `available_packs` carries the full sorted list so
/// the discover → compare workflow stays inside MCP.
#[derive(Debug)]
pub struct DiscoverResult {
    pub latest_pack: PathBuf,
    pub available_packs: Vec<PathBuf>,
    pub session_id: Option<String>,
}

/// Resolves a pack directory from explicit path or repository root,
/// with a single-entry cache to avoid re-reading on repeated calls.
pub struct PackResolver {
    cached_pack: Arc<Mutex<Option<(PathBuf, PackIndex)>>>,
}

#[derive(Debug)]
struct NoUsablePackError {
    root: PathBuf,
}

impl std::fmt::Display for NoUsablePackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No packs found for {}", self.root.display())
    }
}

impl std::error::Error for NoUsablePackError {}

pub fn no_usable_pack_error(root: &Path) -> anyhow::Error {
    NoUsablePackError {
        root: root.to_path_buf(),
    }
    .into()
}

pub fn is_no_usable_pack_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<NoUsablePackError>().is_some()
}

/// List every `pack-*` directory in `<root>/.repolens/`, sorted ascending by
/// directory name. The filter is prefix-only — any directory named `pack-*`
/// passes through, regardless of whether the suffix parses as a timestamp.
/// Callers that rely on lexical-sort-equals-chronological-sort depend on the
/// `pack-YYYYMMDD-HHMMSS` naming convention established by `PackWriter`; if
/// that convention ever changes, update both this helper and every caller that
/// sorts by name.
///
/// Returns an empty vector (not an error) when the storage directory does not
/// exist; the caller distinguishes "no packs" from "one pack" based on length.
pub fn list_pack_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let storage_dir = RepoStoragePaths::storage_dir(root);
    if !storage_dir.exists() {
        return Ok(Vec::new());
    }

    let mut pack_dirs: Vec<PathBuf> = std::fs::read_dir(&storage_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("pack-"))
                    .unwrap_or(false)
        })
        .collect();

    pack_dirs.sort();
    Ok(pack_dirs)
}

impl PackResolver {
    pub fn new() -> Self {
        Self {
            cached_pack: Arc::new(Mutex::new(None)),
        }
    }

    /// Discover the latest pack for a repository root, plus the full list of
    /// available packs (sorted ascending by directory name, i.e.
    /// chronologically under the `pack-YYYYMMDD-HHMMSS` convention).
    ///
    /// `available_packs` is always populated by enumerating
    /// `<root>/.repolens/pack-*` — even when the active session points at a
    /// pack, the discover loop still surfaces every other pack so callers can
    /// build pack-pair workflows (`compare_summaries`) without shelling out.
    pub fn discover(&self, root: &Path) -> Result<DiscoverResult> {
        info!(root = %root.display(), "Discovering packs");

        let available_packs = list_pack_dirs(root)?;

        // Try session first — but only honour it when the referenced pack is
        // still on disk. A stale session is the older "session existed,
        // pack deleted" failure mode the directory-scan fallback was added
        // for; preserved here.
        if let Some(session) = SessionStore::load(root)? {
            let pack_path = PathBuf::from(&session.active_pack_path);
            if pack_path.join("manifest.json").exists() {
                info!(
                    pack = %pack_path.display(),
                    session_id = %session.session_id,
                    available = available_packs.len(),
                    "Found pack via session"
                );
                return Ok(DiscoverResult {
                    latest_pack: pack_path,
                    available_packs,
                    session_id: Some(session.session_id),
                });
            }
            warn!(pack = %pack_path.display(), "Session pack missing, falling back to directory scan");
        }

        // Fallback: latest pack from the directory scan.
        if available_packs.is_empty() {
            return Err(no_usable_pack_error(root));
        }

        let latest = available_packs.last().unwrap().clone();
        info!(
            pack = %latest.display(),
            available = available_packs.len(),
            "Found pack via directory scan (no session)"
        );
        Ok(DiscoverResult {
            latest_pack: latest,
            available_packs,
            session_id: None,
        })
    }

    /// Resolve a pack path and return its index.
    /// Uses `pack` if provided, otherwise discovers from `root`.
    /// Caches the result for repeated calls to the same pack.
    pub fn resolve(&self, pack: Option<&str>, root: Option<&str>) -> Result<(PathBuf, PackIndex)> {
        let pack_path = match (pack, root) {
            (Some(p), _) => PathBuf::from(p),
            (None, Some(r)) => {
                let result = self.discover(Path::new(r))?;
                result.latest_pack
            }
            (None, None) => {
                let r = default_repo_root();
                let result = self.discover(Path::new(&r))?;
                result.latest_pack
            }
        };

        // Check cache
        {
            let cache = self.cached_pack.lock().unwrap();
            if let Some((cached_path, cached_index)) = cache.as_ref() {
                if *cached_path == pack_path {
                    info!(pack = %pack_path.display(), "Using cached pack index");
                    return Ok((cached_path.clone(), cached_index.clone()));
                }
            }
        }

        // Read from disk
        info!(pack = %pack_path.display(), "Reading pack index");
        let index = PackReader::read(&pack_path).map_err(|e| {
            anyhow::anyhow!("Failed to read pack at {}: {}", pack_path.display(), e)
        })?;

        // Update cache
        {
            let mut cache = self.cached_pack.lock().unwrap();
            *cache = Some((pack_path.clone(), index.clone()));
        }

        Ok((pack_path, index))
    }
}

impl Default for PackResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Default repository root used when an MCP caller omits `root`.
///
/// Walks up from the server's current working directory to the nearest
/// directory containing `.git`; falls back to the cwd itself. This lets
/// agents launched from inside a repo call tools like `get_summary` with no
/// arguments.
pub fn default_repo_root() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    default_repo_root_from(&cwd).display().to_string()
}

/// Pure helper behind [`default_repo_root`]: nearest ancestor of `start`
/// (inclusive) that contains `.git`, or `start` if none does.
pub fn default_repo_root_from(start: &Path) -> PathBuf {
    start
        .ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(start)
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_repo_root_walks_up_to_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let sub = tmp.path().join("a/b");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(default_repo_root_from(&sub), tmp.path());
    }

    #[test]
    fn default_repo_root_falls_back_to_start_without_git() {
        let tmp = tempfile::tempdir().unwrap();
        // tempdir may live under a git checkout on some machines; only assert
        // when no ancestor has .git.
        if tmp.path().ancestors().all(|d| !d.join(".git").exists()) {
            assert_eq!(default_repo_root_from(tmp.path()), tmp.path());
        }
    }
    use repolens_core::model::session::RepoSession;
    use std::fs;
    use time::OffsetDateTime;

    #[test]
    fn discover_from_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let pack_dir = root.join(".repolens/pack-20260329-120000");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(
            pack_dir.join("manifest.json"),
            r#"{"schemaVersion":"1.0","schemaUrl":"","timestamp":1774753241.321919000,"chunks":["files-0001.json"]}"#,
        ).unwrap();
        fs::write(pack_dir.join("files-0001.json"), "[]").unwrap();

        let session = RepoSession {
            schema_version: "1.0".to_string(),
            session_id: "test-uuid-1234".to_string(),
            repo_root: root.to_string_lossy().to_string(),
            active_pack_path: pack_dir.to_string_lossy().to_string(),
            active_report_path: None,
            updated_at: OffsetDateTime::now_utc(),
            last_accessed: OffsetDateTime::now_utc(),
            last_scan_mode: "full".to_string(),
            reprocessed_files: Some(0),
            deleted_files: None,
            carried_forward_files: None,
        };
        SessionStore::save(root, &session).unwrap();

        let resolver = PackResolver::new();
        let result = resolver.discover(root).unwrap();

        assert_eq!(result.latest_pack, pack_dir);
        assert_eq!(result.session_id, Some("test-uuid-1234".to_string()));
        // Even when the session points at a pack, `available_packs` must
        // surface the full directory listing so consumers can build pack-
        // pair workflows without shelling out.
        assert_eq!(result.available_packs, vec![pack_dir]);
    }

    #[test]
    fn discover_fallback_without_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let pack1 = root.join(".repolens/pack-20260329-100000");
        let pack2 = root.join(".repolens/pack-20260329-120000");
        for p in [&pack1, &pack2] {
            fs::create_dir_all(p).unwrap();
            fs::write(
                p.join("manifest.json"),
                r#"{"schemaVersion":"1.0","schemaUrl":"","timestamp":1774753241.321919000,"chunks":["files-0001.json"]}"#,
            ).unwrap();
            fs::write(p.join("files-0001.json"), "[]").unwrap();
        }

        let resolver = PackResolver::new();
        let result = resolver.discover(root).unwrap();

        assert_eq!(result.latest_pack, pack2, "should pick latest by name sort");
        assert_eq!(result.session_id, None, "no session means null sessionId");
        assert_eq!(
            result.available_packs,
            vec![pack1, pack2],
            "available_packs must enumerate every pack-* dir sorted ascending"
        );
    }

    #[test]
    fn discover_no_packs_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".repolens")).unwrap();

        let resolver = PackResolver::new();
        let err = resolver.discover(root).unwrap_err();
        assert!(err.to_string().contains("No packs found"));
        assert!(is_no_usable_pack_error(&err));
    }

    #[test]
    fn resolve_with_explicit_pack() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path().join("my-pack");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(
            pack_dir.join("manifest.json"),
            r#"{"schemaVersion":"1.0","schemaUrl":"","timestamp":1774753241.321919000,"chunks":["files-0001.json"]}"#,
        ).unwrap();
        fs::write(pack_dir.join("files-0001.json"), "[]").unwrap();

        let resolver = PackResolver::new();
        let (path, index) = resolver
            .resolve(Some(pack_dir.to_str().unwrap()), None)
            .unwrap();

        assert_eq!(path, pack_dir);
        assert!(index.entries.is_empty());
    }

    #[test]
    fn resolve_caches_repeated_reads() {
        let dir = tempfile::tempdir().unwrap();
        let pack_dir = dir.path().join("my-pack");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(
            pack_dir.join("manifest.json"),
            r#"{"schemaVersion":"1.0","schemaUrl":"","timestamp":1774753241.321919000,"chunks":["files-0001.json"]}"#,
        ).unwrap();
        fs::write(pack_dir.join("files-0001.json"), "[]").unwrap();

        let resolver = PackResolver::new();
        let pack_str = pack_dir.to_str().unwrap();

        let (p1, _) = resolver.resolve(Some(pack_str), None).unwrap();
        assert_eq!(p1, pack_dir);

        fs::remove_file(pack_dir.join("manifest.json")).unwrap();
        fs::remove_file(pack_dir.join("files-0001.json")).unwrap();

        let (p2, _) = resolver.resolve(Some(pack_str), None).unwrap();
        assert_eq!(p2, pack_dir);
    }

    #[test]
    fn resolve_neither_pack_nor_root_returns_error() {
        let resolver = PackResolver::new();
        let err = resolver.resolve(None, None).unwrap_err();
        assert!(err.to_string().contains("pack") || err.to_string().contains("root"));
    }

    #[test]
    fn list_pack_dirs_returns_sorted_ascending_pack_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let storage = root.join(".repolens");
        fs::create_dir_all(&storage).unwrap();

        fs::create_dir_all(storage.join("pack-20260329-120000")).unwrap();
        fs::create_dir_all(storage.join("pack-20260329-100000")).unwrap();
        fs::create_dir_all(storage.join("pack-20260328-235959")).unwrap();
        // Entries that must be filtered out (no pack- prefix):
        fs::write(storage.join("session.json"), "{}").unwrap();
        fs::create_dir_all(storage.join("wiki")).unwrap();

        // Malformed pack name that must pass through (proves prefix-only filter):
        fs::create_dir_all(storage.join("pack-not-a-timestamp")).unwrap();

        let packs = list_pack_dirs(root).unwrap();

        let names: Vec<String> = packs
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(
            names,
            vec![
                "pack-20260328-235959".to_string(),
                "pack-20260329-100000".to_string(),
                "pack-20260329-120000".to_string(),
                "pack-not-a-timestamp".to_string(),
            ],
            "list_pack_dirs must return every pack-* directory, sorted ascending"
        );
    }

    #[test]
    fn list_pack_dirs_returns_empty_when_storage_missing() {
        let dir = tempfile::tempdir().unwrap();
        let packs = list_pack_dirs(dir.path()).unwrap();
        assert!(packs.is_empty());
    }

    // ---------------------------------------------------------------------
    // validate_pack_id / looks_like_pack_id / resolve_pack_ref
    // (lifted from `repolens-cli/src/compare.rs` in 2026-05-13).
    // ---------------------------------------------------------------------

    #[test]
    fn validate_pack_id_accepts_full_form() {
        assert_eq!(
            validate_pack_id("pack-20260420-112702").unwrap(),
            "pack-20260420-112702"
        );
    }

    #[test]
    fn validate_pack_id_accepts_bare_timestamp_and_prepends_prefix() {
        assert_eq!(
            validate_pack_id("20260420-112702").unwrap(),
            "pack-20260420-112702"
        );
    }

    #[test]
    fn validate_pack_id_rejects_path_separators_and_traversal() {
        for bad in [
            "/tmp/pack-20260420-112702",
            "subdir/pack-20260420-112702",
            "pack-20260420-112702/files-0001.json",
            "../pack-20260420-112702",
            "",
        ] {
            assert!(
                validate_pack_id(bad).is_err(),
                "{bad} must be rejected by validate_pack_id"
            );
        }
    }

    #[test]
    fn validate_pack_id_rejects_malformed_timestamps() {
        for bad in [
            "pack-2026-04-20-112702",
            "pack-20260420",
            "pack-foo",
            "pack-",
        ] {
            assert!(
                validate_pack_id(bad).is_err(),
                "{bad} must be rejected by validate_pack_id"
            );
        }
    }

    #[test]
    fn looks_like_pack_id_distinguishes_path_from_id() {
        assert!(looks_like_pack_id("pack-20260420-112702"));
        assert!(looks_like_pack_id("20260420-112702"));
        // Anything containing a separator is treated as a path.
        assert!(!looks_like_pack_id("/abs/path/pack-20260420-112702"));
        assert!(!looks_like_pack_id("rel/path"));
        assert!(!looks_like_pack_id("pack-foo"));
        assert!(!looks_like_pack_id(""));
    }

    #[test]
    fn resolve_pack_ref_resolves_pack_id_against_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let storage = root.join(".repolens");
        let pack_dir = storage.join("pack-20260420-112702");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(pack_dir.join("manifest.json"), "{}").unwrap();

        let resolved = resolve_pack_ref("pack-20260420-112702", root).unwrap();
        assert_eq!(resolved, pack_dir);

        // Bare form also resolves.
        let resolved = resolve_pack_ref("20260420-112702", root).unwrap();
        assert_eq!(resolved, pack_dir);
    }

    #[test]
    fn resolve_pack_ref_errors_when_pack_id_missing_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".repolens")).unwrap();

        let err = resolve_pack_ref("pack-99999999-999999", root).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Pack not found"), "got: {msg}");
    }

    #[test]
    fn resolve_pack_ref_passes_paths_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let abs = "/tmp/some-absolute-path-that-need-not-exist";
        let resolved = resolve_pack_ref(abs, root).unwrap();
        assert_eq!(resolved, PathBuf::from(abs));
    }
}
