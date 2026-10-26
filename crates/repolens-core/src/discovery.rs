use crate::path_safety::canonicalize_within_base;
use crate::rules::RulesEngine;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// A discovered file with its include/exclude decision.
#[derive(Debug, Clone)]
pub struct FileRecord {
    pub absolute_path: PathBuf,
    pub relative_path: String,
    pub included: bool,
    pub reason: String,
}

/// Walks a repository's filesystem and evaluates include/exclude rules.
pub struct FileDiscovery {
    root: PathBuf,
    follow_symlinks: bool,
    include_binaries: bool,
    reposnapshot_files: Vec<PathBuf>,
}

impl FileDiscovery {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            follow_symlinks: false,
            include_binaries: false,
            reposnapshot_files: Vec::new(),
        }
    }

    pub fn follow_symlinks(mut self, follow: bool) -> Self {
        self.follow_symlinks = follow;
        self
    }

    pub fn include_binaries(mut self, include: bool) -> Self {
        self.include_binaries = include;
        self
    }

    pub fn reposnapshot_files(mut self, files: Vec<PathBuf>) -> Self {
        self.reposnapshot_files = files;
        self
    }

    pub fn discover(&self) -> std::io::Result<Vec<FileRecord>> {
        let engine = RulesEngine::load(&self.root, &self.reposnapshot_files)?;
        let prune_engine = engine.clone();
        let root = self.root.clone();
        let canonical_root = self.root.canonicalize()?;
        let mut records = Vec::new();

        debug!(root = %self.root.display(), "Starting file discovery");

        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(false)
            .git_ignore(false)
            .git_exclude(false)
            .git_global(false)
            .follow_links(self.follow_symlinks)
            .filter_entry(move |entry| {
                let name = entry.file_name();
                if name == ".git" || name == ".repolens" {
                    return false;
                }
                let Some(file_type) = entry.file_type() else {
                    return true;
                };
                if !file_type.is_dir() {
                    return true;
                }
                if entry.path_is_symlink()
                    && canonicalize_within_base(&canonical_root, entry.path()).is_err()
                {
                    return false;
                }
                should_descend_dir(entry.path(), &root, &prune_engine)
            });

        for entry in builder.build() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!(err = %e, "Skipping inaccessible path during discovery");
                    continue;
                }
            };

            let Some(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                continue;
            }

            let abs_path = entry.path().to_path_buf();
            let rel = match abs_path.strip_prefix(&self.root) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let relative_path = rel.to_string_lossy().replace('\\', "/");

            if relative_path.starts_with(".git/") || relative_path == ".git" {
                records.push(FileRecord {
                    absolute_path: abs_path,
                    relative_path,
                    included: false,
                    reason: "excluded:git".to_string(),
                });
                continue;
            }

            if entry.path_is_symlink()
                && (!self.follow_symlinks
                    || canonicalize_within_base(&self.root, &abs_path).is_err())
            {
                records.push(FileRecord {
                    absolute_path: abs_path,
                    relative_path,
                    included: false,
                    reason: "excluded:symlink".to_string(),
                });
                continue;
            }

            let decision = engine.evaluate(&abs_path, false);
            if !decision.included {
                records.push(FileRecord {
                    absolute_path: abs_path,
                    relative_path,
                    included: false,
                    reason: decision.reason,
                });
                continue;
            }

            if !self.include_binaries {
                let is_binary = crate::metadata::is_binary(&abs_path).unwrap_or_else(|e| {
                    warn!(path = %abs_path.display(), err = %e, "Binary check failed, assuming binary");
                    true
                });
                if is_binary {
                    records.push(FileRecord {
                        absolute_path: abs_path,
                        relative_path,
                        included: false,
                        reason: "excluded:binary".to_string(),
                    });
                    continue;
                }
            }

            records.push(FileRecord {
                absolute_path: abs_path,
                relative_path,
                included: true,
                reason: "included".to_string(),
            });
        }

        records.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        debug!(
            total = records.len(),
            included = records.iter().filter(|r| r.included).count(),
            "File discovery complete"
        );
        Ok(records)
    }
}

fn should_descend_dir(path: &Path, root: &Path, engine: &RulesEngine) -> bool {
    if path == root {
        return true;
    }
    if engine.evaluate(path, true).included {
        return true;
    }

    // An ignored directory is pruned unless a `.reposnapshot.rules` whitelist may
    // re-include something inside it. A `.gitignore` *inside* an ignored directory
    // does NOT trigger descent — git-aligned: "it is not possible to re-include a
    // file if a parent directory of that file is excluded". Use `.reposnapshot.rules`
    // to re-include under an ignored subtree.
    let rel = path
        .strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or_default()
        .replace('\\', "/");

    engine.reposnapshot_whitelist_may_touch_subtree(&rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn prunes_gitignored_subtree_without_reinclude_paths() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();

        fs::create_dir_all(root.join("target/debug/deps")).unwrap();
        fs::write(root.join("target/debug/deps/libfoo.rmeta"), "noise").unwrap();
        fs::write(root.join("target/debug/build.log"), "noise").unwrap();

        let records = FileDiscovery::new(root).discover().unwrap();

        assert!(records.iter().any(|r| r.relative_path == "src/main.rs"));
        assert!(
            records
                .iter()
                .all(|r| !r.relative_path.starts_with("target/")),
            "target/ should be pruned entirely, got {:?}",
            records
                .iter()
                .map(|r| r.relative_path.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn root_reposnapshot_whitelist_blocks_pruning_of_ignored_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        let rules_path = root.join(".reposnapshot.rules");
        std::fs::write(&rules_path, "!build/keep.txt\n").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/keep.txt"), "important").unwrap();

        let records = FileDiscovery::new(root)
            .reposnapshot_files(vec![rules_path])
            .discover()
            .unwrap();

        let keep = records
            .iter()
            .find(|r| r.relative_path == "build/keep.txt")
            .expect("build/keep.txt should still be reachable");
        assert!(keep.included);
    }

    #[test]
    fn root_reposnapshot_wildcard_whitelist_blocks_pruning_of_ignored_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        let rules_path = root.join(".reposnapshot.rules");
        std::fs::write(&rules_path, "!**/keep.txt\n").unwrap();
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/keep.txt"), "important").unwrap();

        let records = FileDiscovery::new(root)
            .reposnapshot_files(vec![rules_path])
            .discover()
            .unwrap();

        let keep = records
            .iter()
            .find(|r| r.relative_path == "build/keep.txt")
            .expect("build/keep.txt should still be reachable");
        assert!(keep.included);
    }

    #[test]
    fn prunes_ignored_dir_even_with_a_nested_gitignore() {
        // git-aligned: a `.gitignore` *inside* an ignored directory does NOT
        // re-include files ("it is not possible to re-include a file if a parent
        // directory of that file is excluded"). The whole `build/` subtree is
        // pruned — no `.reposnapshot.rules` whitelist touches it — so neither the
        // re-include attempt (`build/keep.txt`) nor the junk (`build/output.bin`)
        // is recorded at all. (To re-include under an ignored dir, use
        // `.reposnapshot.rules`; see the whitelist tests above.)
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/.gitignore"), "!keep.txt\n").unwrap();
        fs::write(root.join("build/keep.txt"), "important").unwrap();
        fs::write(root.join("build/output.bin"), "junk").unwrap();

        fs::write(root.join(".gitignore"), "build/\n").unwrap();

        let records = FileDiscovery::new(root).discover().unwrap();

        assert!(
            records
                .iter()
                .all(|r| !r.relative_path.starts_with("build/")),
            "ignored build/ must be pruned entirely (a nested .gitignore does not \
             re-include); got {:?}",
            records
                .iter()
                .map(|r| r.relative_path.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            records.iter().any(|r| r.relative_path == "src/main.rs"),
            "src/main.rs must still be discovered"
        );
    }

    #[test]
    fn still_records_gitignored_files() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(root.join("app.rs"), "fn main() {}").unwrap();
        fs::write(root.join("debug.log"), "noise").unwrap();
        fs::write(root.join(".gitignore"), "*.log\n").unwrap();

        let records = FileDiscovery::new(root).discover().unwrap();

        let log_record = records
            .iter()
            .find(|r| r.relative_path == "debug.log")
            .expect("debug.log should still be recorded as an excluded file");
        assert!(!log_record.included);
        assert_eq!(log_record.reason, "excluded:gitignore");
    }

    #[test]
    fn excludes_repolens_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create src/main.rs
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

        // Create .repolens artifacts
        let pack_dir = root.join(".repolens/pack-20260329-120000/files/src");
        fs::create_dir_all(&pack_dir).unwrap();
        fs::write(
            root.join(".repolens/pack-20260329-120000/manifest.json"),
            "{}",
        )
        .unwrap();
        fs::write(pack_dir.join("main.rs"), "fn main() {}").unwrap();
        fs::write(root.join(".repolens/session.json"), "{}").unwrap();

        let records = FileDiscovery::new(root).discover().unwrap();

        // src/main.rs must be included
        let src_record = records
            .iter()
            .find(|r| r.relative_path == "src/main.rs")
            .expect("src/main.rs should be discovered");
        assert!(src_record.included, "src/main.rs should be included");

        // .repolens/ directory is pruned at walk level — no entries should appear
        let repolens_records: Vec<_> = records
            .iter()
            .filter(|r| r.relative_path.starts_with(".repolens/"))
            .collect();

        assert!(
            repolens_records.is_empty(),
            "Expected .repolens/ to be pruned at walk level, but found {} entries",
            repolens_records.len()
        );
    }
}
