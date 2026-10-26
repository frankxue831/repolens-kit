use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// Path utilities for `.repolens/` storage layout within a repository.
pub struct RepoStoragePaths;

/// Contents of the self-ignoring `.repolens/.gitignore`.
pub const STORAGE_GITIGNORE: &str = "# Created by RepoLens: ignore this whole folder.\n*\n";

impl RepoStoragePaths {
    /// Best-effort: if `path` is (or lies inside) a `.repolens` directory that
    /// exists, make sure it contains a `.gitignore` with `*` so git ignores the
    /// whole folder without editing any of the user's files. Never fails;
    /// an existing `.repolens/.gitignore` is left untouched.
    pub fn ensure_storage_ignored(path: &Path) {
        let Some(storage) = path
            .ancestors()
            .find(|d| d.file_name().is_some_and(|n| n == ".repolens"))
        else {
            return;
        };
        if !storage.is_dir() {
            return;
        }
        let gi = storage.join(".gitignore");
        if gi.exists() {
            return;
        }
        let _ = std::fs::write(&gi, STORAGE_GITIGNORE);
    }

    pub fn storage_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(".repolens")
    }

    pub fn bootstrap_dir(repo_root: &Path) -> PathBuf {
        Self::storage_dir(repo_root).join("bootstrap")
    }

    pub fn config_path(repo_root: &Path) -> PathBuf {
        Self::storage_dir(repo_root).join("config.json")
    }

    pub fn bootstrap_active_path(repo_root: &Path) -> PathBuf {
        Self::bootstrap_dir(repo_root).join("ACTIVE")
    }

    pub fn bootstrap_current_path(repo_root: &Path) -> PathBuf {
        Self::bootstrap_dir(repo_root).join("CURRENT")
    }

    pub fn session_path(repo_root: &Path) -> PathBuf {
        Self::storage_dir(repo_root).join("session.json")
    }

    /// Returns the run directory for a validated runtime-generated bootstrap run id.
    pub fn bootstrap_run_dir(repo_root: &Path, run_id: &str) -> PathBuf {
        Self::bootstrap_dir(repo_root).join(run_id)
    }

    /// Recover the repo root from a pack directory produced by `pack_dir`.
    /// Returns `None` if `pack_dir` doesn't follow the `<repo>/.repolens/pack-*`
    /// layout (e.g. a bare temp dir handed in by a test).
    pub fn repo_root_from_pack_dir(pack_dir: &Path) -> Option<&Path> {
        let storage_dir = pack_dir.parent()?;
        if storage_dir.file_name()? != ".repolens" {
            return None;
        }
        storage_dir.parent()
    }

    pub fn pack_dir(repo_root: &Path, now: OffsetDateTime) -> PathBuf {
        let utc = now.to_offset(time::UtcOffset::UTC);
        let name = format!(
            "pack-{:04}{:02}{:02}-{:02}{:02}{:02}",
            utc.year(),
            utc.month() as u8,
            utc.day(),
            utc.hour(),
            utc.minute(),
            utc.second(),
        );
        Self::storage_dir(repo_root).join(name)
    }

    /// Create a new, empty pack directory named after `now`. Pack names have
    /// one-second resolution, so when the name is taken (another scan in the
    /// same second) the next free second is used: a scan never writes into an
    /// existing pack. The name is also kept after the newest existing pack, so
    /// name order stays creation order even when a burst of scans pushed names
    /// ahead of the clock. `create_dir` claims the name atomically, so
    /// concurrent scans cannot both get it.
    pub fn create_pack_dir(repo_root: &Path, now: OffsetDateTime) -> std::io::Result<PathBuf> {
        let storage_dir = Self::storage_dir(repo_root);
        std::fs::create_dir_all(&storage_dir)?;
        let mut at = now;
        let newest = std::fs::read_dir(&storage_dir)?
            .flatten()
            .filter_map(|e| pack_name_time(&e.file_name().to_string_lossy()))
            .max();
        if let Some(newest) = newest {
            if newest >= now.replace_nanosecond(0).unwrap_or(now) {
                at = newest + time::Duration::SECOND;
            }
        }
        loop {
            let dir = Self::pack_dir(repo_root, at);
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    at += time::Duration::SECOND;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn is_git_repo(repo_root: &Path) -> bool {
        repo_root.join(".git").exists()
    }

    /// One fingerprint per top-level entry of `.repolens/` (except
    /// `bootstrap/`), keyed by `.repolens/<name>`. The fingerprint hashes every
    /// file's relative path, size and modification time below the entry, so
    /// adding, removing, resizing or re-saving a file changes it. File contents
    /// are not hashed: an edit that keeps the size and restores the old mtime
    /// (e.g. `touch -r`) is not detected. This is meant to catch accidental
    /// writes outside the boundary, not deliberate tampering. Empty when there
    /// is no `.repolens/`.
    pub fn storage_fingerprints(repo_root: &Path) -> std::collections::BTreeMap<String, String> {
        use sha2::{Digest, Sha256};
        fn walk(path: &Path, rel: &str, hasher: &mut Sha256) {
            let Ok(meta) = std::fs::symlink_metadata(path) else {
                return;
            };
            if meta.is_dir() {
                let Ok(rd) = std::fs::read_dir(path) else {
                    return;
                };
                let mut names: Vec<_> = rd.flatten().map(|e| e.file_name()).collect();
                names.sort();
                for n in names {
                    let child_rel = format!("{rel}/{}", n.to_string_lossy());
                    walk(&path.join(&n), &child_rel, hasher);
                }
                return;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            hasher.update(format!("{rel}\0{}\0{mtime}\n", meta.len()).as_bytes());
        }
        let mut out = std::collections::BTreeMap::new();
        let Ok(rd) = std::fs::read_dir(Self::storage_dir(repo_root)) else {
            return out;
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "bootstrap" {
                continue;
            }
            let mut hasher = Sha256::new();
            walk(&entry.path(), &name, &mut hasher);
            out.insert(
                format!(".repolens/{name}"),
                format!("{:x}", hasher.finalize()),
            );
        }
        out
    }
}

/// The UTC time encoded in a `pack-YYYYMMDD-HHMMSS` directory name, or
/// `None` for any other name.
fn pack_name_time(name: &str) -> Option<OffsetDateTime> {
    let (date, clock) = name.strip_prefix("pack-")?.split_once('-')?;
    if date.len() != 8
        || clock.len() != 6
        || !date
            .bytes()
            .chain(clock.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let num = |s: &str| -> Option<u8> { s.parse().ok() };
    let year: i32 = date[..4].parse().ok()?;
    let month = time::Month::try_from(num(&date[4..6])?).ok()?;
    let date = time::Date::from_calendar_date(year, month, num(&date[6..])?).ok()?;
    let clock =
        time::Time::from_hms(num(&clock[..2])?, num(&clock[2..4])?, num(&clock[4..])?).ok()?;
    Some(time::PrimitiveDateTime::new(date, clock).assume_utc())
}

#[cfg(test)]
mod storage_ignore_tests {
    use super::*;

    #[test]
    fn ensure_storage_ignored_writes_star_once() {
        let tmp = tempfile::tempdir().unwrap();
        let pack = tmp.path().join(".repolens/pack-1");
        std::fs::create_dir_all(&pack).unwrap();
        RepoStoragePaths::ensure_storage_ignored(&pack);
        let gi = tmp.path().join(".repolens/.gitignore");
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), STORAGE_GITIGNORE);
        std::fs::write(&gi, "custom\n").unwrap();
        RepoStoragePaths::ensure_storage_ignored(&pack);
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), "custom\n");
    }

    #[test]
    fn create_pack_dir_never_reuses_an_existing_pack_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // 2026-03-29 12:00:00 UTC
        let now = OffsetDateTime::from_unix_timestamp(1_774_785_600).unwrap();

        let first = RepoStoragePaths::create_pack_dir(tmp.path(), now).unwrap();
        let second = RepoStoragePaths::create_pack_dir(tmp.path(), now).unwrap();

        assert_eq!(first, tmp.path().join(".repolens/pack-20260329-120000"));
        assert_eq!(second, tmp.path().join(".repolens/pack-20260329-120001"));
        assert!(first.is_dir() && second.is_dir());
    }

    /// A burst of scans pushes names ahead of the clock; once cleanup frees
    /// an earlier slot, a new pack must still sort after the newest one.
    #[test]
    fn create_pack_dir_names_new_pack_after_the_newest_existing_one() {
        let tmp = tempfile::tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_774_785_600).unwrap();
        std::fs::create_dir_all(tmp.path().join(".repolens/pack-20260329-120001")).unwrap();

        let dir = RepoStoragePaths::create_pack_dir(tmp.path(), now).unwrap();

        assert_eq!(dir, tmp.path().join(".repolens/pack-20260329-120002"));
    }

    #[test]
    fn create_pack_dir_ignores_pack_dirs_without_a_timestamp_name() {
        let tmp = tempfile::tempdir().unwrap();
        let now = OffsetDateTime::from_unix_timestamp(1_774_785_600).unwrap();
        std::fs::create_dir_all(tmp.path().join(".repolens/pack-not-a-timestamp")).unwrap();
        // Eight bytes, but `é` straddles the year/month boundary.
        std::fs::create_dir_all(tmp.path().join(".repolens/pack-202é234-123456")).unwrap();

        let dir = RepoStoragePaths::create_pack_dir(tmp.path(), now).unwrap();

        assert_eq!(dir, tmp.path().join(".repolens/pack-20260329-120000"));
    }

    #[test]
    fn ensure_storage_ignored_ignores_paths_outside_storage() {
        let tmp = tempfile::tempdir().unwrap();
        RepoStoragePaths::ensure_storage_ignored(tmp.path());
        assert!(!tmp.path().join(".gitignore").exists());
    }
}
