# Lockfile-State Invariant Catalogue

Authority: Supporting
Lifecycle: Living

**Date:** 2026-05-11
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-05-11-evidence-aware-findings.md](2026-05-11-evidence-aware-findings.md)

## Goal

Catalogue RepoLens's `CargoLockState` detector as a named observation family so the F-CARGO-LOCK-STATE evidence-aware finding can be graded above `unaudited`. The detector is the single source for the `profile.lockfileState` metric.

## Matching Contract

`repolens_profile::lockfile::CargoLockStateDetector::detect(repo_root: &Path) -> CargoLockState` returns one of four states by inspecting only the filesystem at the given repo root:

- `AbsentFromRepo` — no `Cargo.lock` exists.
- `PresentButGitIgnored { path }` — `Cargo.lock` exists AND a literal `Cargo.lock` pattern (with optional leading `/`) appears as a non-comment line in the root `.gitignore`. Per the [gitignore spec](https://git-scm.com/docs/gitignore#_pattern_format), a pattern ending in `/` matches directories only and does **not** match the file `Cargo.lock`; the detector respects that.
- `PresentTrackingUnknown { path }` — `Cargo.lock` exists AND no matching `.gitignore` entry was found.
- `CommittedAndTracked { path }` — reserved variant. **Phase 1 of the evidence-aware findings work intentionally does not inspect the git index, so this variant is never emitted by `detect()`.** Findings that surface this state attach the `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation to document the partial check.

The detector does not parse `.gitignore` exhaustively: it recognises the file-matching forms (`Cargo.lock`, `/Cargo.lock`), rejects directory-only forms (`Cargo.lock/`, `/Cargo.lock/`) per Git's slash semantics, rejects comment lines and blank lines, and honours `!`-prefixed negation entries under [Git's last-match-wins rule](https://git-scm.com/docs/gitignore#_pattern_format) (a later `!Cargo.lock` undoes an earlier `Cargo.lock`, and vice-versa). Wildcard patterns and nested `.gitignore` files are out of scope.

## Invariant Catalogue

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-LOCKFILE-STATE-ABSENT` | When `Cargo.lock` does not exist, `detect()` returns `AbsentFromRepo` | `crates/repolens-profile/src/lockfile.rs::tests::absent_when_lockfile_missing` | `confirmOnly` |
| `I-LOCKFILE-STATE-PRESENT-TRACKING-UNKNOWN` | When `Cargo.lock` exists and no matching `.gitignore` entry exists, `detect()` returns `PresentTrackingUnknown` | `crates/repolens-profile/src/lockfile.rs::tests::present_tracking_unknown_when_no_gitignore`; `::present_tracking_unknown_when_gitignore_does_not_match` | `confirmOnly` |
| `I-LOCKFILE-STATE-GITIGNORED-UNANCHORED` | Unanchored `Cargo.lock` entry in `.gitignore` produces `PresentButGitIgnored` | `crates/repolens-profile/src/lockfile.rs::tests::present_but_gitignored_when_gitignore_matches_unanchored` | `confirmOnly` |
| `I-LOCKFILE-STATE-GITIGNORED-ANCHORED` | Leading-slash `/Cargo.lock` entry in `.gitignore` produces `PresentButGitIgnored` | `crates/repolens-profile/src/lockfile.rs::tests::present_but_gitignored_when_gitignore_matches_anchored` | `confirmOnly` |
| `I-LOCKFILE-STATE-TRAILING-SLASH-IS-DIRECTORY-ONLY` | Per Git's gitignore spec, a pattern ending in `/` matches directories only. A `.gitignore` line of `Cargo.lock/` or `/Cargo.lock/` does **not** match the file `Cargo.lock` — those entries describe a directory of that name, not the lockfile. The detector returns `PresentTrackingUnknown` in this case, matching Git's `check-ignore` behaviour | `crates/repolens-profile/src/lockfile.rs::tests::i_lockfile_state_trailing_slash_is_directory_only` | `confirmOnly` |
| `I-LOCKFILE-STATE-COMMENT-NOT-MATCH` | Comment lines and blank lines in `.gitignore` do not produce a match, even if a comment literally contains the word `Cargo.lock` | `crates/repolens-profile/src/lockfile.rs::tests::gitignore_comments_and_blank_lines_ignored` | `confirmOnly` |
| `I-LOCKFILE-STATE-RESPECTS-NEGATION` | `!`-prefixed entries are honoured under last-match-wins: `Cargo.lock` followed by `!Cargo.lock` produces `PresentTrackingUnknown` (file is not ignored), and the reverse order produces `PresentButGitIgnored` (last match wins) | `crates/repolens-profile/src/lockfile.rs::tests::i_lockfile_state_respects_negation` | `confirmOnly` |
| `I-LOCKFILE-STATE-NO-COMMITTED-AND-TRACKED-EMISSION` | `detect()` never returns `CargoLockState::CommittedAndTracked` in Phase 1 (the git index is not inspected). Consuming findings attach the `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation to make the partial check explicit | `crates/repolens-profile/src/lockfile.rs::tests::i_lockfile_state_no_committed_and_tracked_emission` | `confirmOnly` |

## Out of Scope

- **Git index inspection.** Phase 1 of the evidence-aware findings work does not run `git ls-files` or read `.git/index`, so a file that is present, not gitignored, but explicitly untracked (e.g. via `git rm --cached`) is reported as `PresentTrackingUnknown`, not as an "untracked" variant. The `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation captures this on every F-CARGO-LOCK-STATE finding.
- **Wildcard `.gitignore` patterns.** `*.lock`, `**/Cargo.lock`, and similar glob patterns are not honoured. A Cargo workspace whose root `.gitignore` uses a glob pattern to ignore `Cargo.lock` will report `PresentTrackingUnknown` rather than `PresentButGitIgnored`. This is a known false-negative; the limitation above keeps the finding honest about it.
- **Nested `.gitignore` files.** Only the root `.gitignore` is consulted. Crates in a workspace that have their own `.gitignore` ignoring `Cargo.lock` are not detected.
- **Non-Cargo lockfiles.** `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `Pipfile.lock`, `poetry.lock`, `go.sum`, etc. are out of scope for this family. A future catalogue may extend the detector to other ecosystems.

## Maintenance Rule

If any cited detector test is removed or weakened, any tool whose promoted payload relies on `profile.lockfileState` (currently F-CARGO-LOCK-STATE) must drop a tier in the same change unless equivalent coverage is added. If RepoLens later inspects the git index and starts emitting `CargoLockState::CommittedAndTracked`, `I-LOCKFILE-STATE-NO-COMMITTED-AND-TRACKED-EMISSION` must be retired in lockstep.
