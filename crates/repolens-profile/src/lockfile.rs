//! Cargo.lock state detector. Returns one of four states defined in
//! `repolens-core::model::evidence::CargoLockState`. A `Cargo.lock` with an
//! entry in the git index is committed-and-tracked (#25). Otherwise presence
//! plus a conservative `.gitignore` pattern check distinguishes
//! present-but-ignored from present-with-unknown-tracking (not a git repo,
//! or present but not yet added). Findings that consume this state attach
//! the `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation because the
//! `.gitignore` check stays partial.
//!
//! Spec: `docs/superpowers/specs/2026-05-11-evidence-aware-findings.md` §H.

use std::fs;
use std::path::Path;

use repolens_core::model::evidence::CargoLockState;

const LOCKFILE_NAME: &str = "Cargo.lock";

pub struct CargoLockStateDetector;

impl CargoLockStateDetector {
    /// Detect the lockfile state for the given repo root. The `repo_root`
    /// must be an absolute path to a Cargo project root.
    pub fn detect(repo_root: &Path) -> CargoLockState {
        let lockfile = repo_root.join(LOCKFILE_NAME);
        if !lockfile.exists() {
            return CargoLockState::AbsentFromRepo;
        }

        // Git ignores `.gitignore` for files it already tracks, so the index
        // wins over any ignore pattern.
        if repolens_core::git::is_path_tracked(repo_root, LOCKFILE_NAME) {
            return CargoLockState::CommittedAndTracked {
                path: LOCKFILE_NAME.into(),
            };
        }

        if is_path_in_gitignore(repo_root, LOCKFILE_NAME) {
            return CargoLockState::PresentButGitIgnored {
                path: LOCKFILE_NAME.into(),
            };
        }

        // Present, not in the git index (or no git repo) and not gitignored:
        // tracking cannot be confirmed.
        // This is honest about the limit of the check; consumers attach
        // `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` as a finding limitation.
        CargoLockState::PresentTrackingUnknown {
            path: LOCKFILE_NAME.into(),
        }
    }
}

/// Conservative check for a literal lockfile pattern in `.gitignore`.
/// Recognises the file-matching forms:
/// - `Cargo.lock`
/// - `/Cargo.lock`
///
/// Per the gitignore spec, a trailing slash on a pattern makes it match
/// only directories. Since `Cargo.lock` is a file, `Cargo.lock/` and
/// `/Cargo.lock/` are *not* treated as matches.
///
/// Honours `!`-prefixed negation under last-match-wins semantics:
/// `Cargo.lock` followed by `!Cargo.lock` means the file is NOT ignored
/// even though the first line literally matches it. The detector walks
/// every matching line and tracks the final ignored-state instead of
/// returning on first match.
///
/// Does not handle wildcard patterns or nested .gitignores. The
/// `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation captures this
/// scope explicitly.
fn is_path_in_gitignore(repo_root: &Path, file: &str) -> bool {
    let gi = repo_root.join(".gitignore");
    let Ok(content) = fs::read_to_string(&gi) else {
        return false;
    };
    let mut ignored = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negated, body) = if let Some(rest) = line.strip_prefix('!') {
            (true, rest.trim_start())
        } else {
            (false, line)
        };
        // Trailing slash = directory-only pattern (gitignore spec). Don't
        // treat it as a file match.
        let after_leading_slash = body.trim_start_matches('/');
        if after_leading_slash.ends_with('/') {
            continue;
        }
        if after_leading_slash == file {
            ignored = !negated;
        }
    }
    ignored
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn absent_when_lockfile_missing() {
        let dir = tempdir().unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert_eq!(st, CargoLockState::AbsentFromRepo);
    }

    #[test]
    fn present_tracking_unknown_when_no_gitignore() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert_eq!(
            st,
            CargoLockState::PresentTrackingUnknown {
                path: "Cargo.lock".into(),
            }
        );
    }

    #[test]
    fn present_tracking_unknown_when_gitignore_does_not_match() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\nnode_modules/\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(matches!(st, CargoLockState::PresentTrackingUnknown { .. }));
    }

    #[test]
    fn present_but_gitignored_when_gitignore_matches_unanchored() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        fs::write(dir.path().join(".gitignore"), "Cargo.lock\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert_eq!(
            st,
            CargoLockState::PresentButGitIgnored {
                path: "Cargo.lock".into(),
            }
        );
    }

    #[test]
    fn present_but_gitignored_when_gitignore_matches_anchored() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        fs::write(dir.path().join(".gitignore"), "/Cargo.lock\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(matches!(st, CargoLockState::PresentButGitIgnored { .. }));
    }

    #[test]
    fn gitignore_comments_and_blank_lines_ignored() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        fs::write(
            dir.path().join(".gitignore"),
            "# Cargo.lock\n\n   \n# the comment lists Cargo.lock but does not match\n",
        )
        .unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(matches!(st, CargoLockState::PresentTrackingUnknown { .. }));
    }

    // ------------------------------------------------------------------
    // Lockfile-state invariants (catalogue: 2026-05-11-lockfile-state-invariant-catalogue.md)
    // ------------------------------------------------------------------

    /// I-LOCKFILE-STATE-TRAILING-SLASH-IS-DIRECTORY-ONLY: per the gitignore
    /// spec, a pattern ending in `/` matches only directories. Since
    /// `Cargo.lock` is a file, a `.gitignore` line of `Cargo.lock/` does
    /// NOT match it. Regression for the 2026-05-11 Codex pass on
    /// PR #51 (Phase 4 Part A) which caught that the prior detector
    /// stripped trailing slashes and would have falsely reported
    /// `PresentButGitIgnored` for a file that Git still tracks.
    #[test]
    fn i_lockfile_state_trailing_slash_is_directory_only() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        fs::write(dir.path().join(".gitignore"), "Cargo.lock/\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(
            matches!(st, CargoLockState::PresentTrackingUnknown { .. }),
            "Cargo.lock/ is a directory-only pattern; file should NOT be considered gitignored"
        );

        // Same for the leading + trailing form `/Cargo.lock/`.
        fs::write(dir.path().join(".gitignore"), "/Cargo.lock/\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(
            matches!(st, CargoLockState::PresentTrackingUnknown { .. }),
            "/Cargo.lock/ is a directory-only pattern at root; file should NOT match"
        );
    }

    /// I-LOCKFILE-STATE-RESPECTS-NEGATION: a `.gitignore` line of the form
    /// `!Cargo.lock` un-ignores the file even when a prior literal line
    /// matched it, per Git's last-match-wins semantics. The detector
    /// returns `PresentTrackingUnknown` in this case (not
    /// `PresentButGitIgnored`). Regression for the 2026-05-11 second Codex
    /// pass on PR #51.
    #[test]
    fn i_lockfile_state_respects_negation() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        // Simple negation that follows the ignore line — last match wins.
        fs::write(dir.path().join(".gitignore"), "Cargo.lock\n!Cargo.lock\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(
            matches!(st, CargoLockState::PresentTrackingUnknown { .. }),
            "Cargo.lock + !Cargo.lock = file is NOT ignored under last-match-wins; got {st:?}"
        );

        // Negation before the ignore line should NOT un-ignore (re-ignored last).
        fs::write(dir.path().join(".gitignore"), "!Cargo.lock\nCargo.lock\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(
            matches!(st, CargoLockState::PresentButGitIgnored { .. }),
            "!Cargo.lock then Cargo.lock = file IS ignored (last match wins); got {st:?}"
        );

        // Lone negation with no prior ignore is a no-op — file is just tracked.
        fs::write(dir.path().join(".gitignore"), "!Cargo.lock\n").unwrap();
        let st = CargoLockStateDetector::detect(dir.path());
        assert!(matches!(st, CargoLockState::PresentTrackingUnknown { .. }));
    }

    /// I-LOCKFILE-STATE-COMMITTED-ONLY-FROM-INDEX: `CommittedAndTracked` is
    /// emitted only when the git index has a `Cargo.lock` entry. Outside a
    /// git repository the detector never claims tracking.
    #[test]
    fn i_lockfile_state_committed_only_from_index() {
        let dir = tempdir().unwrap();
        let absent = CargoLockStateDetector::detect(dir.path());
        assert!(!matches!(
            absent,
            CargoLockState::CommittedAndTracked { .. }
        ));

        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        let present = CargoLockStateDetector::detect(dir.path());
        assert!(!matches!(
            present,
            CargoLockState::CommittedAndTracked { .. }
        ));

        fs::write(dir.path().join(".gitignore"), "Cargo.lock\n").unwrap();
        let gitignored = CargoLockStateDetector::detect(dir.path());
        assert!(!matches!(
            gitignored,
            CargoLockState::CommittedAndTracked { .. }
        ));
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    /// #25: a Cargo.lock that git tracks is reported as tracked, even when
    /// a later `.gitignore` line matches it (git keeps tracking it).
    #[test]
    fn committed_and_tracked_when_in_git_index() {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        git(dir.path(), &["add", "Cargo.lock"]);
        git(dir.path(), &["commit", "-qm", "lock"]);
        let tracked = CargoLockState::CommittedAndTracked {
            path: "Cargo.lock".into(),
        };
        assert_eq!(CargoLockStateDetector::detect(dir.path()), tracked);

        fs::write(dir.path().join(".gitignore"), "Cargo.lock\n").unwrap();
        assert_eq!(CargoLockStateDetector::detect(dir.path()), tracked);
    }

    /// Present but never added: not tracked, so not reported as tracked.
    #[test]
    fn untracked_lockfile_in_git_repo_is_not_reported_as_tracked() {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        fs::write(dir.path().join("Cargo.lock"), "# stub").unwrap();
        assert!(matches!(
            CargoLockStateDetector::detect(dir.path()),
            CargoLockState::PresentTrackingUnknown { .. }
        ));
    }
}
