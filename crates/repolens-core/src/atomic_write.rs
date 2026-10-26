//! Durable file writes via a temp file + atomic rename.
//!
//! [`write_atomic`] writes bytes to a sibling `<path>.tmp` file and then renames it
//! over `path`. Because `rename(2)` is atomic on a single filesystem, a concurrent
//! reader never observes a partially written file — it sees either the previous
//! contents or the complete new contents, never a torn middle state. On any failure
//! the temp file is best-effort removed so a crash mid-write does not leave an
//! orphaned `.tmp` behind.
//!
//! Callers must ensure the parent directory exists (every current caller already does
//! a `create_dir_all` first); [`write_atomic`] does not create it. Callers that need a
//! bespoke temp name — e.g. a UUID suffix for concurrent writers, as the memory store
//! uses — should keep their own staging scheme rather than this deterministic sibling.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Atomically write `contents` to `path` via a `<path>.tmp` sibling + rename.
///
/// On success `path` contains exactly `contents`. On failure the temp file is
/// best-effort removed and the original `path` (if any) is left untouched.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let tmp = tmp_sibling(path);
    if let Err(e) = fs::write(&tmp, contents) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// `<path>.tmp` — the temp sibling used to stage the atomic write.
fn tmp_sibling(path: &Path) -> PathBuf {
    let mut os = OsString::from(path.as_os_str());
    os.push(".tmp");
    PathBuf::from(os)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_new_file_with_exact_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        write_atomic(&path, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        fs::write(&path, b"stale-old-contents").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn leaves_no_tmp_file_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        write_atomic(&path, b"data").unwrap();
        assert!(
            !tmp_sibling(&path).exists(),
            "tmp sibling must be renamed away on success"
        );
        let mut names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, vec![OsString::from("out.json")]);
    }

    #[test]
    fn tmp_sibling_appends_suffix() {
        assert_eq!(
            tmp_sibling(Path::new("/a/b/c.json")),
            PathBuf::from("/a/b/c.json.tmp")
        );
    }

    #[test]
    fn errors_and_cleans_up_when_parent_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-subdir").join("out.json");
        let err = write_atomic(&path, b"data").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(!path.exists());
        assert!(!tmp_sibling(&path).exists());
    }
}
