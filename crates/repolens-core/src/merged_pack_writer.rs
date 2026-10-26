use crate::model::file_entry::FileEntry;
use crate::pack_writer::{PackWriteOptions, PackWriter};
use crate::path_safety::{
    resolve_existing_prefixed_subpath, resolve_existing_subpath,
    resolve_prefixed_subpath_for_create,
};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

const DEFAULT_CHUNK_SIZE: usize = 500;

/// Writes merged (incremental) packs by combining reprocessed files with
/// carried-forward entries from a base pack.
///
/// Content materialization uses hardlinks for carried-forward files (falling
/// back to copy when hardlinks are not supported), and direct copies for
/// reprocessed files sourced from the repo working tree.
pub struct MergedPackWriter {
    pack_dir: PathBuf,
    chunk_size: usize,
    link: fn(&Path, &Path) -> io::Result<()>,
}

impl MergedPackWriter {
    pub fn new(pack_dir: &Path) -> Self {
        Self {
            pack_dir: pack_dir.to_path_buf(),
            chunk_size: DEFAULT_CHUNK_SIZE,
            link: |source, target| fs::hard_link(source, target),
        }
    }

    /// Override the number of entries per chunk file (default: 500).
    pub fn chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }

    /// Write a merged pack to `self.pack_dir`.
    ///
    /// For each included entry that has a `content_path`:
    /// - If the entry's path is in `carried_paths`, the content is hardlinked
    ///   from `base_pack_dir`; a copy is used as fallback.
    /// - Otherwise the content is copied from `repo_root/<entry.path>`.
    ///
    /// Chunk files (`files-0001.json`, …) and `manifest.json` are written in
    /// the same format as `PackWriter`.
    pub fn write(
        &self,
        entries: &[FileEntry],
        base_pack_dir: &Path,
        repo_root: &Path,
        carried_paths: &HashSet<String>,
        options: &PackWriteOptions<'_>,
    ) -> io::Result<()> {
        info!(
            pack_dir = %self.pack_dir.display(),
            entries = entries.len(),
            carried = carried_paths.len(),
            "Writing merged pack"
        );

        // Carried-forward files are hardlinked from the base pack; written
        // into the base itself, the copy fallback would truncate them.
        if let (Ok(target), Ok(base)) = (
            fs::canonicalize(&self.pack_dir),
            fs::canonicalize(base_pack_dir),
        ) {
            if target == base {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "refusing to write a merged pack into its own base pack: {}",
                        base.display()
                    ),
                ));
            }
        }

        fs::create_dir_all(&self.pack_dir)?;

        let _copied_fallbacks =
            self.materialize_content(entries, base_pack_dir, repo_root, carried_paths)?;
        let chunk_names = PackWriter::write_chunks(&self.pack_dir, entries, self.chunk_size)?;
        PackWriter::new(&self.pack_dir, repo_root)
            .write_manifest_with_chunks(&chunk_names, options)?;

        info!(chunks = chunk_names.len(), "Merged pack write complete");
        Ok(())
    }

    fn materialize_content(
        &self,
        entries: &[FileEntry],
        base_pack_dir: &Path,
        repo_root: &Path,
        carried_paths: &HashSet<String>,
    ) -> io::Result<usize> {
        // Copies made because a hardlink failed. Falling back to a copy is
        // expected on some filesystems and the pack is complete, so it is
        // logged at debug level only, never once per file on stderr (#59).
        let mut copied_fallbacks = 0usize;
        for entry in entries.iter().filter(|e| e.included) {
            let Some(content_path) = &entry.content_path else {
                continue;
            };

            let target =
                resolve_prefixed_subpath_for_create(&self.pack_dir, content_path, "files")?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }

            if carried_paths.contains(&entry.path) {
                // Carried-forward: prefer hardlink from base pack, fall back to copy.
                let source =
                    resolve_existing_prefixed_subpath(base_pack_dir, content_path, "files")
                        .map_err(|e| {
                            io::Error::new(
                                e.kind(),
                                format!(
                                    "failed to resolve carried-forward content for {}: {e}",
                                    entry.path
                                ),
                            )
                        })?;
                match (self.link)(&source, &target) {
                    Ok(()) => debug!(path = %entry.path, "Hardlinked carried-forward content"),
                    // The target can be a hardlink of the source; copying onto
                    // it would truncate the base pack's file.
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        return Err(io::Error::new(
                            e.kind(),
                            format!(
                                "carried-forward content for {} already exists in the new pack: {}",
                                entry.path,
                                target.display()
                            ),
                        ));
                    }
                    Err(e) => {
                        debug!(
                            path = %entry.path,
                            error = %e,
                            "Hardlink failed for carried-forward entry, falling back to copy"
                        );
                        copied_fallbacks += 1;
                        fs::copy(&source, &target).map_err(|e| {
                            io::Error::new(
                                e.kind(),
                                format!(
                                    "failed to copy carried-forward content for {}: {e}",
                                    entry.path
                                ),
                            )
                        })?;
                    }
                }
            } else {
                // Reprocessed: copy from the working tree.
                let source = resolve_existing_subpath(repo_root, &entry.path).map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!(
                            "failed to resolve reprocessed content for {}: {e}",
                            entry.path
                        ),
                    )
                })?;
                fs::copy(&source, &target).map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!("failed to copy reprocessed content for {}: {e}", entry.path),
                    )
                })?;
                debug!(path = %entry.path, "Copied reprocessed content");
            }
        }
        if copied_fallbacks > 0 {
            debug!(
                count = copied_fallbacks,
                "Copied carried-forward entries because hardlinks failed"
            );
        }
        Ok(copied_fallbacks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::file_entry::FileEntry;
    use crate::pack_writer::PackWriteOptions;
    use std::collections::HashSet;
    use tempfile::TempDir;

    fn make_entry(path: &str, content_path: &str, included: bool) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            sha256: "abc123".to_string(),
            language: "Rust".to_string(),
            loc: 10,
            included,
            reason: if included {
                "included".to_string()
            } else {
                "excluded:gitignore".to_string()
            },
            content_path: if included {
                Some(content_path.to_string())
            } else {
                None
            },
            symbols: vec![],
        }
    }

    fn make_options<'a>(base_pack_id: &'a str) -> PackWriteOptions<'a> {
        PackWriteOptions {
            scan_mode: "incremental",
            head_commit: "abc123def456",
            worktree_dirty: false,
            scan_config_hash: "hash1",
            base_pack_id: Some(base_pack_id),
            base_head_commit: Some("base_commit"),
            reprocessed_files: 1,
            deleted_files: Some(0),
            carried_forward_files: Some(1),
        }
    }

    #[test]
    fn refuses_to_write_into_its_own_base_pack() {
        let tmp = TempDir::new().unwrap();
        let pack_dir = tmp.path().join("pack");
        fs::create_dir_all(pack_dir.join("files")).unwrap();
        fs::write(pack_dir.join("files/file.txt"), b"carried content").unwrap();
        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let entry = make_entry("src/file.txt", "files/file.txt", true);
        let carried: HashSet<String> = HashSet::from(["src/file.txt".to_string()]);

        let result = MergedPackWriter::new(&pack_dir).write(
            &[entry],
            &pack_dir,
            &repo_root,
            &carried,
            &make_options("pack"),
        );

        assert!(result.is_err(), "a merged pack must not overwrite its base");
        assert_eq!(
            fs::read(pack_dir.join("files/file.txt")).unwrap(),
            b"carried content"
        );
    }

    /// If the target already exists (two entries sharing a content path), the
    /// copy fallback would write onto the base's file and could truncate it.
    #[test]
    fn errors_instead_of_copying_onto_an_existing_carried_forward_target() {
        let tmp = TempDir::new().unwrap();
        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(base_pack_dir.join("files")).unwrap();
        fs::write(base_pack_dir.join("files/file.txt"), b"carried content").unwrap();
        let new_pack_dir = tmp.path().join("new-pack");
        fs::create_dir_all(new_pack_dir.join("files")).unwrap();
        fs::hard_link(
            base_pack_dir.join("files/file.txt"),
            new_pack_dir.join("files/file.txt"),
        )
        .unwrap();
        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let entry = make_entry("src/file.txt", "files/file.txt", true);
        let carried: HashSet<String> = HashSet::from(["src/file.txt".to_string()]);

        let result = MergedPackWriter::new(&new_pack_dir).write(
            &[entry],
            &base_pack_dir,
            &repo_root,
            &carried,
            &make_options("base-pack"),
        );

        assert!(
            result.is_err(),
            "an existing target must not be copied onto"
        );
        assert_eq!(
            fs::read(base_pack_dir.join("files/file.txt")).unwrap(),
            b"carried content"
        );
    }

    /// #59 item 1: when hardlinks fail, every carried-forward file is
    /// copied and the pack is complete; no per-file warning is emitted (the
    /// fallback logs at debug level only).
    #[test]
    fn copies_every_carried_forward_entry_when_hardlinks_fail() {
        let tmp = TempDir::new().unwrap();
        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(base_pack_dir.join("files")).unwrap();
        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();
        let mut entries = Vec::new();
        let mut carried = HashSet::new();
        for i in 0..5 {
            let content = format!("files/f{i}.txt");
            fs::write(base_pack_dir.join(&content), format!("content {i}")).unwrap();
            entries.push(make_entry(&format!("src/f{i}.txt"), &content, true));
            carried.insert(format!("src/f{i}.txt"));
        }
        let new_pack_dir = tmp.path().join("new-pack");
        let mut writer = MergedPackWriter::new(&new_pack_dir);
        writer.link = |_, _| Err(io::Error::new(io::ErrorKind::Unsupported, "no links"));

        fs::create_dir_all(&new_pack_dir).unwrap();
        let copied = writer
            .materialize_content(&entries, &base_pack_dir, &repo_root, &carried)
            .unwrap();

        assert_eq!(copied, 5);
        for i in 0..5 {
            assert_eq!(
                fs::read_to_string(new_pack_dir.join(format!("files/f{i}.txt"))).unwrap(),
                format!("content {i}")
            );
        }
        let source = include_str!("merged_pack_writer.rs");
        let warn_macro = concat!("warn", "!(");
        assert!(
            !source.contains(warn_macro),
            "the hardlink fallback must not warn per file"
        );
    }

    #[test]
    fn writes_carried_forward_via_hardlink() {
        let tmp = TempDir::new().unwrap();

        // Set up base pack with a content file.
        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(base_pack_dir.join("files")).unwrap();
        let base_content = base_pack_dir.join("files/file.txt");
        fs::write(&base_content, b"carried content").unwrap();

        // New pack directory.
        let new_pack_dir = tmp.path().join("new-pack");
        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let entry = make_entry("src/file.txt", "files/file.txt", true);
        let mut carried: HashSet<String> = HashSet::new();
        carried.insert("src/file.txt".to_string());

        let writer = MergedPackWriter::new(&new_pack_dir);
        writer
            .write(
                &[entry],
                &base_pack_dir,
                &repo_root,
                &carried,
                &make_options("base-pack-001"),
            )
            .unwrap();

        let target = new_pack_dir.join("files/file.txt");
        assert!(target.exists(), "content file should exist in new pack");
        let content = fs::read_to_string(&target).unwrap();
        assert_eq!(content, "carried content");

        // On Unix verify same inode (hardlink).
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let base_ino = fs::metadata(&base_content).unwrap().ino();
            let new_ino = fs::metadata(&target).unwrap().ino();
            assert_eq!(base_ino, new_ino, "hardlink should share the same inode");
        }
    }

    #[test]
    fn copies_reprocessed_content_from_repo() {
        let tmp = TempDir::new().unwrap();

        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(&base_pack_dir).unwrap();

        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(repo_root.join("src")).unwrap();
        fs::write(repo_root.join("src/new.rs"), b"reprocessed content").unwrap();

        let new_pack_dir = tmp.path().join("new-pack");

        let entry = make_entry("src/new.rs", "files/new.rs", true);
        let carried: HashSet<String> = HashSet::new(); // not carried

        let writer = MergedPackWriter::new(&new_pack_dir);
        writer
            .write(
                &[entry],
                &base_pack_dir,
                &repo_root,
                &carried,
                &make_options("base-pack-001"),
            )
            .unwrap();

        let target = new_pack_dir.join("files/new.rs");
        assert!(target.exists(), "reprocessed content file should exist");
        let content = fs::read_to_string(&target).unwrap();
        assert_eq!(content, "reprocessed content");
    }

    #[test]
    fn writes_chunks_and_manifest() {
        let tmp = TempDir::new().unwrap();

        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(&base_pack_dir).unwrap();

        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let new_pack_dir = tmp.path().join("new-pack");

        // Excluded entry — no content to materialize.
        let entry = FileEntry {
            path: "src/ignored.rs".to_string(),
            sha256: String::new(),
            language: String::new(),
            loc: 0,
            included: false,
            reason: "excluded:gitignore".to_string(),
            content_path: None,
            symbols: vec![],
        };

        let carried: HashSet<String> = HashSet::new();

        let writer = MergedPackWriter::new(&new_pack_dir);
        writer
            .write(
                &[entry],
                &base_pack_dir,
                &repo_root,
                &carried,
                &make_options("base-pack-001"),
            )
            .unwrap();

        assert!(
            new_pack_dir.join("files-0001.json").exists(),
            "files-0001.json should exist"
        );
        assert!(
            new_pack_dir.join("manifest.json").exists(),
            "manifest.json should exist"
        );

        // Verify manifest contains incremental scan_mode.
        let manifest_json = fs::read_to_string(new_pack_dir.join("manifest.json")).unwrap();
        assert!(manifest_json.contains("\"incremental\""));
        assert!(manifest_json.contains("\"base-pack-001\""));
    }

    #[test]
    fn rejects_carried_forward_content_that_escapes_base_pack() {
        let tmp = TempDir::new().unwrap();

        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(&base_pack_dir).unwrap();

        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let new_pack_dir = tmp.path().join("new-pack");

        let entry = make_entry("src/file.txt", "../outside.txt", true);
        let mut carried: HashSet<String> = HashSet::new();
        carried.insert("src/file.txt".to_string());

        let writer = MergedPackWriter::new(&new_pack_dir);
        let err = writer
            .write(
                &[entry],
                &base_pack_dir,
                &repo_root,
                &carried,
                &make_options("base-pack-001"),
            )
            .unwrap_err();

        assert!(err.to_string().contains("path"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_carried_forward_content_without_files_prefix() {
        // Even a relative path that stays inside the pack (no `..`) must be
        // rejected if it doesn't live under `files/` — otherwise a crafted
        // base pack could overwrite `manifest.json` or a chunk file.
        let tmp = TempDir::new().unwrap();

        let base_pack_dir = tmp.path().join("base-pack");
        fs::create_dir_all(&base_pack_dir).unwrap();

        let repo_root = tmp.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let new_pack_dir = tmp.path().join("new-pack");

        let entry = make_entry("src/file.txt", "manifest.json", true);
        let mut carried: HashSet<String> = HashSet::new();
        carried.insert("src/file.txt".to_string());

        let writer = MergedPackWriter::new(&new_pack_dir);
        let err = writer
            .write(
                &[entry],
                &base_pack_dir,
                &repo_root,
                &carried,
                &make_options("base-pack-001"),
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("files/"),
            "error should call out files/ prefix requirement: {err}"
        );
    }
}
