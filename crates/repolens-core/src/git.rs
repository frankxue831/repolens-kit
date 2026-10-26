use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;
use tracing::{debug, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSnapshot {
    pub head_commit: String,
    /// Repo-relative dirty-path entries captured from worktree status, not raw porcelain lines.
    pub porcelain: Vec<String>,
}

/// Files changed between a base commit and the current state.
#[derive(Debug, Clone, Default)]
pub struct ChangeSet {
    /// Relative paths of files added, modified, or renamed (new name).
    pub changed: Vec<String>,
    /// Relative paths of files deleted or renamed (old name).
    pub deleted: Vec<String>,
}

fn repo_path(path: impl AsRef<[u8]>) -> String {
    String::from_utf8_lossy(path.as_ref()).into_owned()
}

fn push_path_if_file(
    paths: &mut BTreeSet<String>,
    entry_mode: gix::object::tree::EntryMode,
    path: impl AsRef<[u8]>,
) {
    if entry_mode.is_blob_or_symlink() {
        paths.insert(repo_path(path));
    }
}

fn push_index_path_if_file(
    paths: &mut BTreeSet<String>,
    entry_mode: gix::index::entry::Mode,
    path: impl AsRef<[u8]>,
) {
    if !entry_mode.is_submodule() && !entry_mode.is_sparse() {
        paths.insert(repo_path(path));
    }
}

/// Uncommitted changes relative to HEAD: staged changes (HEAD tree vs index,
/// e.g. `git mv`, `git rm`, `git add`) plus unstaged and untracked changes
/// (index vs working tree).
fn collect_worktree_changes(repo: &gix::Repository) -> Result<ChangeSet> {
    let mut changed = BTreeSet::new();
    let mut deleted = BTreeSet::new();

    let mut iter = repo
        .status(gix::progress::Discard)
        .context("Failed to prepare gix status")?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_rewrites(Some(Default::default()))
        .into_iter(Vec::<gix::bstr::BString>::new())
        .context("Failed to iterate gix status")?;

    for item in &mut iter {
        let item = item.context("Failed to read gix status entry")?;
        match item {
            gix::status::Item::IndexWorktree(item) => match item {
                gix::status::index_worktree::Item::Rewrite { ref source, .. } => {
                    changed.insert(repo_path(item.rela_path()));
                    deleted.insert(repo_path(source.rela_path()));
                }
                _ => match item.summary() {
                    Some(gix::status::index_worktree::iter::Summary::Removed) => {
                        deleted.insert(repo_path(item.rela_path()));
                    }
                    Some(_) => {
                        changed.insert(repo_path(item.rela_path()));
                    }
                    None => {}
                },
            },
            gix::status::Item::TreeIndex(change) => match change {
                gix::diff::index::ChangeRef::Addition {
                    location,
                    entry_mode,
                    ..
                }
                | gix::diff::index::ChangeRef::Modification {
                    location,
                    entry_mode,
                    ..
                } => push_index_path_if_file(&mut changed, entry_mode, location.as_ref()),
                gix::diff::index::ChangeRef::Deletion {
                    location,
                    entry_mode,
                    ..
                } => push_index_path_if_file(&mut deleted, entry_mode, location.as_ref()),
                gix::diff::index::ChangeRef::Rewrite {
                    source_location,
                    source_entry_mode,
                    location,
                    entry_mode,
                    copy,
                    ..
                } => {
                    push_index_path_if_file(&mut changed, entry_mode, location.as_ref());
                    if !copy {
                        push_index_path_if_file(
                            &mut deleted,
                            source_entry_mode,
                            source_location.as_ref(),
                        );
                    }
                }
            },
        }
    }

    // A path can show up on both sides, e.g. `git rm --cached f` (staged
    // deletion, file still on disk as untracked) or `git add f && rm f`.
    // What is on disk now decides which side wins.
    let both: Vec<String> = changed.intersection(&deleted).cloned().collect();
    if !both.is_empty() {
        let workdir = repo.workdir();
        for path in both {
            let on_disk = workdir.is_some_and(|dir| dir.join(&path).symlink_metadata().is_ok());
            if on_disk {
                deleted.remove(&path);
            } else {
                changed.remove(&path);
            }
        }
    }

    Ok(ChangeSet {
        changed: changed.into_iter().collect(),
        deleted: deleted.into_iter().collect(),
    })
}

fn collect_committed_changes(repo: &gix::Repository, base_commit: &str) -> Result<ChangeSet> {
    let base_id = gix::hash::ObjectId::from_hex(base_commit.as_bytes())
        .with_context(|| format!("Invalid base commit hex: {base_commit}"))?;
    let base_tree = repo
        .find_commit(base_id)
        .context("Failed to resolve base commit")?
        .tree()
        .context("Failed to resolve base commit tree")?;
    let head_tree = repo
        .head_commit()
        .context("Failed to resolve HEAD commit")?
        .tree()
        .context("Failed to resolve HEAD tree")?;

    let mut changed = BTreeSet::new();
    let mut deleted = BTreeSet::new();
    for change in repo
        .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)
        .context("Failed to diff base tree to HEAD tree")?
    {
        match change {
            gix::object::tree::diff::ChangeDetached::Addition {
                location,
                entry_mode,
                ..
            } => {
                push_path_if_file(&mut changed, entry_mode, location);
            }
            gix::object::tree::diff::ChangeDetached::Modification {
                location,
                entry_mode,
                ..
            } => {
                push_path_if_file(&mut changed, entry_mode, location);
            }
            gix::object::tree::diff::ChangeDetached::Deletion {
                location,
                entry_mode,
                ..
            } => {
                push_path_if_file(&mut deleted, entry_mode, location);
            }
            gix::object::tree::diff::ChangeDetached::Rewrite {
                source_location,
                source_entry_mode,
                location,
                entry_mode,
                ..
            } => {
                push_path_if_file(&mut changed, entry_mode, location);
                push_path_if_file(&mut deleted, source_entry_mode, source_location);
            }
        }
    }

    Ok(ChangeSet {
        changed: changed.into_iter().collect(),
        deleted: deleted.into_iter().collect(),
    })
}

/// Returns `true` if `commit` is an ancestor of HEAD (i.e., reachable).
pub fn is_commit_reachable(repo_root: &Path, commit: &str) -> bool {
    let repo = match gix::open(repo_root) {
        Ok(repo) => repo,
        Err(e) => {
            warn!(root = %repo_root.display(), err = %e, "gix open failed for reachability check");
            return false;
        }
    };

    let target_id = match gix::hash::ObjectId::from_hex(commit.as_bytes()) {
        Ok(id) => id,
        Err(e) => {
            debug!(root = %repo_root.display(), commit, err = %e, "Commit id is invalid");
            return false;
        }
    };

    if repo.find_commit(target_id).is_err() {
        debug!(root = %repo_root.display(), commit, "Commit does not resolve");
        return false;
    }

    let head = match repo.head_commit() {
        Ok(commit) => commit,
        Err(e) => {
            debug!(root = %repo_root.display(), err = %e, "Cannot resolve HEAD for reachability");
            return false;
        }
    };

    let reachable = if head.id() == target_id {
        true
    } else {
        match head.ancestors().all() {
            Ok(ancestors) => ancestors
                .filter_map(|info| info.ok())
                .any(|info| info.id == target_id),
            Err(e) => {
                warn!(
                    root = %repo_root.display(),
                    err = %e,
                    commit,
                    "Failed to walk ancestors for reachability"
                );
                false
            }
        }
    };
    debug!(root = %repo_root.display(), commit, reachable, "Commit reachability check");
    reachable
}

/// Returns the current HEAD commit SHA, or `None` if not a git repo or HEAD is unresolvable.
pub fn head_commit(repo_root: &Path) -> Option<String> {
    let repo = match gix::open(repo_root) {
        Ok(r) => r,
        Err(e) => {
            debug!(root = %repo_root.display(), err = %e, "Not a git repo or cannot open");
            return None;
        }
    };
    // Binding needed: `Commit` borrows `repo`, so the match result must outlive
    // the temporary's destructor that runs at end of block.
    let result = match repo.head_commit() {
        Ok(commit) => {
            let sha = commit.id().to_string();
            debug!(root = %repo_root.display(), sha = %sha, "Resolved HEAD commit");
            Some(sha)
        }
        Err(e) => {
            debug!(root = %repo_root.display(), err = %e, "Cannot resolve HEAD commit");
            None
        }
    };
    result
}

/// Returns the short branch name HEAD points at (e.g. `main`), or `None` when
/// HEAD is detached or the path is not a git repository. Note: a freshly
/// initialized repo with no commits still reports its unborn branch name.
pub fn head_branch(repo_root: &Path) -> Option<String> {
    let repo = gix::open(repo_root).ok()?;
    let head = repo.head().ok()?;
    head.referent_name().map(|name| name.shorten().to_string())
}

/// Returns `true` when `path` (relative to `repo_root`, `/`-separated) has
/// an entry in the git index, i.e. git tracks it. Returns `false` when the
/// path is not tracked, the repo has no index yet, or `repo_root` is not a
/// git repository.
pub fn is_path_tracked(repo_root: &Path, path: &str) -> bool {
    let Ok(repo) = gix::open(repo_root) else {
        return false;
    };
    let tracked = match repo.index_or_empty() {
        Ok(index) => index.entry_by_path(path.into()).is_some(),
        Err(e) => {
            debug!(root = %repo_root.display(), err = %e, "Cannot read git index");
            false
        }
    };
    debug!(root = %repo_root.display(), path, tracked, "Git index tracking check");
    tracked
}

/// Returns `true` when the worktree has uncommitted changes (tracked or untracked).
/// Returns `false` if the path is not a git repo.
pub fn is_worktree_dirty(repo_root: &Path) -> bool {
    let dirty = match list_worktree_changes(repo_root) {
        Ok(changes) => !changes.changed.is_empty() || !changes.deleted.is_empty(),
        Err(e) => {
            debug!(root = %repo_root.display(), err = %e, "Cannot collect worktree changes, assuming clean");
            return false;
        }
    };
    debug!(root = %repo_root.display(), dirty, "Worktree dirty check");
    dirty
}

pub fn capture_clean_worktree_snapshot(repo_root: &Path) -> Result<WorktreeSnapshot> {
    let changes = list_worktree_changes(repo_root)?;
    let mut porcelain = changes.changed;
    porcelain.extend(changes.deleted);
    if !porcelain.is_empty() {
        anyhow::bail!("worktree must be clean before bootstrap start");
    }
    let head = head_commit(repo_root).ok_or_else(|| anyhow::anyhow!("HEAD is not resolvable"))?;
    Ok(WorktreeSnapshot {
        head_commit: head,
        porcelain,
    })
}

/// Path of the repository's `info/exclude` as git resolves it: in the common
/// git dir, so linked worktrees and submodules (where `.git` is a file) work.
pub fn info_exclude_path(repo_root: &Path) -> Option<std::path::PathBuf> {
    let repo = gix::open(repo_root).ok()?;
    Some(repo.common_dir().join("info").join("exclude"))
}

pub fn list_worktree_changes(repo_root: &Path) -> Result<ChangeSet> {
    let repo = gix::open(repo_root).context("Failed to open git repository")?;
    collect_worktree_changes(&repo)
}

/// Detects file changes between a base commit and the current working tree state.
pub struct GitChangeDetector<'a> {
    repo_root: &'a Path,
    base_commit: &'a str,
}

impl<'a> GitChangeDetector<'a> {
    pub fn new(repo_root: &'a Path, base_commit: &'a str) -> Self {
        Self {
            repo_root,
            base_commit,
        }
    }

    /// Returns a `ChangeSet` combining committed changes since `base_commit`
    /// and any current working tree modifications.
    pub fn detect(&self) -> Result<ChangeSet> {
        debug!(
            root = %self.repo_root.display(),
            base = self.base_commit,
            "Detecting git changes"
        );

        let mut changed: BTreeSet<String> = BTreeSet::new();
        let mut deleted: BTreeSet<String> = BTreeSet::new();
        let repo = match gix::open(self.repo_root) {
            Ok(repo) => repo,
            Err(e) => {
                warn!(
                    root = %self.repo_root.display(),
                    err = %e,
                    "Failed to open git repository, skipping change detection"
                );
                return Ok(ChangeSet::default());
            }
        };

        // Committed changes: base_commit..HEAD
        match collect_committed_changes(&repo, self.base_commit) {
            Ok(changeset) => {
                changed.extend(changeset.changed);
                deleted.extend(changeset.deleted);
            }
            Err(err) => {
                let err = format!("{err:#}");
                warn!(
                    root = %self.repo_root.display(),
                    base = self.base_commit,
                    err = %err,
                    "gix tree diff failed, skipping committed changes"
                );
            }
        }

        // Working tree changes: uncommitted modifications and untracked files
        match collect_worktree_changes(&repo) {
            Ok(changeset) => {
                changed.extend(changeset.changed);
                deleted.extend(changeset.deleted);
            }
            Err(err) => {
                let err = format!("{err:#}");
                warn!(
                    root = %self.repo_root.display(),
                    err = %err,
                    "gix worktree status failed, skipping working tree changes"
                );
            }
        }

        let result = ChangeSet {
            changed: changed.into_iter().collect(),
            deleted: deleted.into_iter().collect(),
        };

        debug!(
            root = %self.repo_root.display(),
            changed_count = result.changed.len(),
            deleted_count = result.deleted.len(),
            "Git change detection complete"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    fn init_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .expect("git config name");
    }

    fn git_add_and_commit(root: &Path, path: &str, message: &str) {
        Command::new("git")
            .args(["add", path])
            .current_dir(root)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(root)
            .output()
            .expect("git commit");
    }

    fn run_gitless_child(test_filter: &str, envs: &[(&str, &str)]) {
        let mut command = Command::new(std::env::current_exe().expect("current test binary"));
        command
            .arg(test_filter)
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env("PATH", "/definitely/not-installed");
        for (key, value) in envs {
            command.env(key, value);
        }

        let output = command.output().expect("spawn child test process");
        assert!(
            output.status.success(),
            "child test failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn head_commit_returns_none_for_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(head_commit(dir.path()), None);
    }

    #[test]
    fn head_commit_returns_sha_after_first_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::write(root.join("hello.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();
        let sha = head_commit(root);
        assert!(sha.is_some(), "should resolve HEAD after commit");
        assert_eq!(sha.unwrap().len(), 40, "SHA should be 40 hex chars");
    }

    #[test]
    fn head_branch_returns_none_for_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(head_branch(dir.path()), None);
    }

    #[test]
    fn head_branch_returns_current_branch_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);
        Command::new("git")
            .args(["checkout", "-b", "feature-x"])
            .current_dir(root)
            .output()
            .expect("git checkout -b");
        fs::write(root.join("a.txt"), "x").unwrap();
        git_add_and_commit(root, "a.txt", "init");
        assert_eq!(head_branch(root), Some("feature-x".to_string()));
    }

    #[test]
    fn dirty_returns_false_for_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_worktree_dirty(dir.path()));
    }

    #[test]
    fn dirty_returns_false_for_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::write(root.join("hello.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(!is_worktree_dirty(root));
    }

    #[test]
    fn dirty_returns_true_after_modification() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::write(root.join("hello.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();
        fs::write(root.join("hello.txt"), "changed").unwrap();
        assert!(is_worktree_dirty(root));
    }

    #[test]
    fn capture_clean_snapshot_rejects_dirty_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        std::fs::write(dir.path().join("README.md"), "dirty\n").unwrap();

        let err = capture_clean_worktree_snapshot(dir.path()).expect_err("snapshot should fail");
        assert!(err.to_string().contains("worktree must be clean"));
    }

    #[test]
    fn list_worktree_changes_reports_modified_and_untracked_files() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "one\n").unwrap();
        git_add_and_commit(dir.path(), "tracked.txt", "seed");

        std::fs::write(dir.path().join("tracked.txt"), "two\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "new\n").unwrap();

        let changes = list_worktree_changes(dir.path()).unwrap();
        assert!(changes.changed.contains(&"new.txt".to_string()));
        assert!(changes.changed.contains(&"tracked.txt".to_string()));
    }

    #[test]
    fn capture_clean_snapshot_rejects_deleted_tracked_files() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "one\n").unwrap();
        git_add_and_commit(dir.path(), "tracked.txt", "seed");
        std::fs::remove_file(dir.path().join("tracked.txt")).unwrap();

        let changes = list_worktree_changes(dir.path()).unwrap();
        assert!(changes.deleted.contains(&"tracked.txt".to_string()));

        let err = capture_clean_worktree_snapshot(dir.path()).expect_err("snapshot should fail");
        assert!(err.to_string().contains("worktree must be clean"));
    }

    #[test]
    fn worktree_helpers_do_not_require_git_binary_for_rename_and_untracked_status() {
        if std::env::var_os("REPOLENS_GITLESS_WORKTREE_ROOT").is_some() {
            let root = PathBuf::from(
                std::env::var("REPOLENS_GITLESS_WORKTREE_ROOT").expect("worktree root env"),
            );
            assert!(is_worktree_dirty(&root));

            let changes =
                list_worktree_changes(&root).expect("status should work without git on PATH");
            assert!(changes.changed.contains(&"renamed.txt".to_string()));
            assert!(changes.changed.contains(&"untracked.txt".to_string()));
            assert!(changes.deleted.contains(&"tracked.txt".to_string()));
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        std::fs::write(dir.path().join("tracked.txt"), "one\n").unwrap();
        git_add_and_commit(dir.path(), "tracked.txt", "seed");

        std::fs::rename(
            dir.path().join("tracked.txt"),
            dir.path().join("renamed.txt"),
        )
        .unwrap();
        std::fs::write(dir.path().join("untracked.txt"), "new\n").unwrap();

        run_gitless_child(
            "worktree_helpers_do_not_require_git_binary_for_rename_and_untracked_status",
            &[(
                "REPOLENS_GITLESS_WORKTREE_ROOT",
                dir.path().to_str().expect("utf-8 tempdir path"),
            )],
        );
    }

    // --- New tests ---

    #[test]
    fn is_commit_reachable_returns_true_for_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::write(root.join("a.txt"), "first").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "first commit"])
            .current_dir(root)
            .output()
            .unwrap();

        let first_sha = head_commit(root).expect("should have HEAD after first commit");

        fs::write(root.join("b.txt"), "second").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "second commit"])
            .current_dir(root)
            .output()
            .unwrap();

        assert!(
            is_commit_reachable(root, &first_sha),
            "first commit should be reachable from HEAD"
        );
    }

    #[test]
    fn is_commit_reachable_returns_false_for_invalid_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::write(root.join("a.txt"), "first").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let fake_sha = "0000000000000000000000000000000000000000";
        assert!(
            !is_commit_reachable(root, fake_sha),
            "fake SHA should not be reachable"
        );
    }

    #[test]
    fn git_change_detector_and_reachability_do_not_require_git_binary() {
        if std::env::var_os("REPOLENS_GITLESS_DETECT_ROOT").is_some() {
            let root = PathBuf::from(
                std::env::var("REPOLENS_GITLESS_DETECT_ROOT").expect("detect root env"),
            );
            let base_sha = std::env::var("REPOLENS_GITLESS_BASE_SHA").expect("base sha env");

            assert!(
                is_commit_reachable(&root, &base_sha),
                "base commit should remain reachable without git on PATH"
            );

            let changes = GitChangeDetector::new(&root, &base_sha)
                .detect()
                .expect("detect should work without git on PATH");
            assert!(changes.changed.contains(&"committed-added.txt".to_string()));
            assert!(changes.changed.contains(&"renamed.txt".to_string()));
            assert!(changes
                .changed
                .contains(&"worktree-untracked.txt".to_string()));
            assert!(changes.deleted.contains(&"tracked.txt".to_string()));
            assert!(!changes.changed.contains(&"keep.txt".to_string()));
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::write(root.join("tracked.txt"), "one").unwrap();
        fs::write(root.join("keep.txt"), "stable").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let base_sha = head_commit(root).expect("base commit sha");

        fs::rename(root.join("tracked.txt"), root.join("renamed.txt")).unwrap();
        fs::write(root.join("committed-added.txt"), "added").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "rename and add"])
            .current_dir(root)
            .output()
            .unwrap();

        fs::write(root.join("worktree-untracked.txt"), "later").unwrap();

        run_gitless_child(
            "git_change_detector_and_reachability_do_not_require_git_binary",
            &[
                (
                    "REPOLENS_GITLESS_DETECT_ROOT",
                    root.to_str().expect("utf-8 tempdir path"),
                ),
                ("REPOLENS_GITLESS_BASE_SHA", base_sha.as_str()),
            ],
        );
    }

    #[test]
    fn detect_added_modified_deleted_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        // Initial commit with 3 files
        fs::write(root.join("keep.txt"), "keep").unwrap();
        fs::write(root.join("modify.txt"), "original").unwrap();
        fs::write(root.join("delete.txt"), "gone").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let base_sha = head_commit(root).expect("should have HEAD after init commit");

        // Modify, delete, and add a file; then commit
        fs::write(root.join("modify.txt"), "changed content").unwrap();
        fs::remove_file(root.join("delete.txt")).unwrap();
        fs::write(root.join("added.txt"), "new file").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "changes"])
            .current_dir(root)
            .output()
            .unwrap();

        let detector = GitChangeDetector::new(root, &base_sha);
        let changeset = detector.detect().expect("detect should succeed");

        assert!(
            changeset.changed.contains(&"modify.txt".to_string()),
            "modify.txt should be in changed"
        );
        assert!(
            changeset.changed.contains(&"added.txt".to_string()),
            "added.txt should be in changed"
        );
        assert!(
            changeset.deleted.contains(&"delete.txt".to_string()),
            "delete.txt should be in deleted"
        );
        assert!(
            !changeset.changed.contains(&"keep.txt".to_string()),
            "keep.txt should not be in changed"
        );
    }

    #[test]
    fn detect_subtree_changes_returns_only_file_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/keep.rs"), "fn keep() {}\n").unwrap();
        fs::write(root.join("src/modify.rs"), "fn original() {}\n").unwrap();
        fs::write(root.join("src/delete.rs"), "fn delete_me() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let base_sha = head_commit(root).expect("should have HEAD after init commit");

        fs::write(root.join("src/modify.rs"), "fn modified() {}\n").unwrap();
        fs::remove_file(root.join("src/delete.rs")).unwrap();
        fs::write(root.join("src/added.rs"), "fn added() {}\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "changes"])
            .current_dir(root)
            .output()
            .unwrap();

        let detector = GitChangeDetector::new(root, &base_sha);
        let changeset = detector.detect().expect("detect should succeed");

        assert_eq!(
            changeset.changed,
            vec!["src/added.rs".to_string(), "src/modify.rs".to_string()],
            "subtree change detection should report only changed files, got {:?}",
            changeset.changed
        );
        assert_eq!(
            changeset.deleted,
            vec!["src/delete.rs".to_string()],
            "subtree change detection should report only deleted files, got {:?}",
            changeset.deleted
        );
    }

    #[test]
    fn file_path_filters_reject_gitlink_modes() {
        let mut changed = BTreeSet::new();
        let mut deleted = BTreeSet::new();
        let gitlink_mode = gix::object::tree::EntryKind::Commit.into();

        push_path_if_file(&mut changed, gitlink_mode, "submodule");
        push_path_if_file(&mut deleted, gitlink_mode, "submodule");

        assert!(
            changed.is_empty(),
            "gitlink entries should not be treated as changed file paths"
        );
        assert!(
            deleted.is_empty(),
            "gitlink entries should not be treated as deleted file paths"
        );
    }

    #[test]
    fn detect_working_tree_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        // Initial commit
        fs::write(root.join("tracked.txt"), "original").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let base_sha = head_commit(root).expect("should have HEAD");

        // Uncommitted modification + new untracked file (no new commit)
        fs::write(root.join("tracked.txt"), "modified but not staged").unwrap();
        fs::write(root.join("untracked.txt"), "new untracked file").unwrap();

        let detector = GitChangeDetector::new(root, &base_sha);
        let changeset = detector.detect().expect("detect should succeed");

        assert!(
            changeset.changed.contains(&"tracked.txt".to_string()),
            "modified tracked file should appear in changed"
        );
        assert!(
            changeset.changed.contains(&"untracked.txt".to_string()),
            "untracked file should appear in changed"
        );
    }

    #[test]
    fn detect_paths_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_git_repo(root);

        // Initial commit
        fs::write(root.join("normal.txt"), "normal").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();

        let base_sha = head_commit(root).expect("should have HEAD");

        // Add a file with spaces in its name
        fs::write(root.join("file with spaces.txt"), "content with spaces").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add file with spaces"])
            .current_dir(root)
            .output()
            .unwrap();

        let detector = GitChangeDetector::new(root, &base_sha);
        let changeset = detector.detect().expect("detect should succeed");

        assert!(
            changeset
                .changed
                .contains(&"file with spaces.txt".to_string()),
            "file with spaces should be correctly parsed from NUL-delimited output"
        );
    }

    // --- #30: staged changes (HEAD vs index) ---

    fn run_git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn committed_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        for (path, content) in files {
            fs::write(dir.path().join(path), content).unwrap();
        }
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "-q", "-m", "seed"]);
        let head = head_commit(dir.path()).expect("HEAD");
        (dir, head)
    }

    #[test]
    fn detect_sees_staged_git_mv() {
        let (dir, base) = committed_repo(&[("a.rs", "fn a() {}\n"), ("keep.rs", "fn k() {}\n")]);
        run_git(dir.path(), &["mv", "a.rs", "b.rs"]);

        let changes = GitChangeDetector::new(dir.path(), &base).detect().unwrap();
        assert_eq!(changes.changed, vec!["b.rs".to_string()]);
        assert_eq!(changes.deleted, vec!["a.rs".to_string()]);
        assert!(is_worktree_dirty(dir.path()));
    }

    #[test]
    fn detect_sees_staged_git_rm() {
        let (dir, base) = committed_repo(&[("a.rs", "fn a() {}\n"), ("keep.rs", "fn k() {}\n")]);
        run_git(dir.path(), &["rm", "-q", "a.rs"]);

        let changes = GitChangeDetector::new(dir.path(), &base).detect().unwrap();
        assert!(changes.changed.is_empty(), "got {:?}", changes.changed);
        assert_eq!(changes.deleted, vec!["a.rs".to_string()]);
    }

    #[test]
    fn dirty_returns_true_when_changes_are_only_staged() {
        let (dir, _) = committed_repo(&[("a.rs", "fn a() {}\n")]);
        fs::write(dir.path().join("a.rs"), "fn b() {}\n").unwrap();
        run_git(dir.path(), &["add", "a.rs"]);

        assert!(is_worktree_dirty(dir.path()));
        assert_eq!(
            list_worktree_changes(dir.path()).unwrap().changed,
            vec!["a.rs".to_string()]
        );
        assert!(capture_clean_worktree_snapshot(dir.path()).is_err());
    }

    #[test]
    fn staged_removal_of_file_still_on_disk_counts_as_changed_not_deleted() {
        let (dir, _) = committed_repo(&[("a.rs", "fn a() {}\n")]);
        run_git(dir.path(), &["rm", "-q", "--cached", "a.rs"]);

        let changes = list_worktree_changes(dir.path()).unwrap();
        assert_eq!(changes.changed, vec!["a.rs".to_string()]);
        assert!(changes.deleted.is_empty(), "got {:?}", changes.deleted);
    }
}
