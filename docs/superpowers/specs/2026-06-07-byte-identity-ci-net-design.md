# Byte-identity CI safety net — multi-language pack golden

- **Date:** 2026-06-07
- **Status:** Design — approach + scope approved; revised after a Codex adversarial spec review (2026-06-07, read-only); pending the user's spec review, then `writing-plans`.
- **Source:** `BACKLOG.md` → Icebox → "Byte-identity safety net in CI (review F2)"; raised by the 2026-06-06 adversarial review follow-up.
- **Decision (approved):** Add a hermetic in-suite snapshot golden over `fixtures/inputs/{mixed-small, rust-small, java-small}` that runs inside the existing `cargo test --workspace`. Keep `scripts/golden-pack-diff.sh` unchanged as the local, on-demand "vs-`main`" tool.

## Problem

The scan pipeline's byte-level output (file index, symbols, dependency graph, profile) can drift silently behind an unchanged JSON *shape*. Today that drift is caught two ways, and neither covers multi-language live-scan output automatically:

1. **`crates/repolens-core/tests/*_contract_test.rs`** operate on `fixtures/scenarios/` — *captured Java RepoLens output*. They prove the Rust **models** deserialize/re-serialize the Java contract faithfully (a serde round-trip), and the only **insta snapshots** there are `java_small_*` (e.g. `snapshot_java_small_manifest_reserialization`, `snapshot_java_small_first_chunk_sample` — the latter only a 3-entry *sample*). These do **not** run the Rust scanner.
2. **`scripts/golden-pack-diff.sh`** runs the *real* Rust scanner over `fixtures/inputs/{java-small, rust-small, mixed-small}`, builds a second baseline binary from a git ref, and diffs the two packs (content byte-for-byte, excluding `manifest.json`). This is the only thing that exercises **live multi-language scan output** — but it is **manual** (never invoked by `cargo test` or CI).

So a regression in Go / Python / JS / TS extraction or in graph resolution that changes serialized bytes — without changing JSON shape — passes `cargo test` and CI today. It is caught only if a developer remembers to run the script.

Note F1 (thread-count invariance of the parallel graph build) already landed 2026-06-06 (`crates/repolens-app/src/graph.rs`). This spec is F2, the remaining CI-coverage half.

## Goal / success criteria

- Multi-language pack-byte drift is caught **automatically** by `cargo test --workspace`, therefore by CI, with **no `.github/workflows` change**.
- The net is **hermetic and deterministic**: same fixture in → same committed golden, on any machine, run-to-run.
- Legitimate output changes surface as a **reviewed diff** (re-bless via `cargo insta review`), not a silent pass.
- **Zero new runtime dependencies**; reuse existing patterns (insta snapshots, `tempfile`, `ScanService`).
- `scripts/golden-pack-diff.sh` keeps working unchanged for local on-demand proofs.

## Decision & rejected alternatives

**Chosen — in-suite snapshot golden (cheap auto CI net).** A new test scans each fixture with the real `ScanService`, canonicalizes environment-specific tokens, and asserts an insta snapshot of the pack content. It rides `cargo test --workspace`; CI's existing `paths:` filter already matches new `.rs`, `.snap`, and `fixtures/**` files, so no YAML changes.

**Rejected — wire `golden-pack-diff.sh` into CI.** True A/B vs a ref with no committed golden, but: builds a *second* baseline binary (heavy on the self-hosted macOS runner), needs a fragile git-worktree dance inside CI, and has awkward semantics on push-*to*-`main` (diffing `main` against itself). Higher cost and complexity for the same regression-detection outcome.

**Rejected — `mixed-small` only.** The BACKLOG's minimal scope. `mixed-small` spans Go/JS/TS/Python/JSON but **not** Rust or Java live-scan output; `rust-small` has *zero* byte-identity coverage today. Since the test is parameterized over a fixture list, covering all three costs ~milliseconds and one extra snapshot each, so we cover all three.

## Design

### Location

- New test binary: `crates/repolens-app/tests/pack_byte_identity_test.rs`.
  - `repolens-app` owns `ScanService` (`crates/repolens-app/src/scan.rs`), and the live-scan-in-test pattern already exists there (`crates/repolens-app/tests/evidence_findings_e2e_test.rs`).
- Committed goldens: `crates/repolens-app/tests/snapshots/` (insta default), one `.snap` per fixture.

### Mechanism (data flow), per fixture

1. **Isolate into a *named* child.** Create a `tempfile::tempdir()`, then recursively copy `fixtures/inputs/<fixture>/` into a deterministic child `"<temp>/<fixture>"` (e.g. `<temp>/rust-small`), skipping any `.repolens/`. **Scan that child, not the random temp root.** This is load-bearing: the project profile derives its `name` from the scanned root's *basename* whenever the build system carries no explicit project name — workspace-only Cargo (`rust-small`) and Maven (`java-small`) both fall through to `workspace_dir_name_from_pack_dir` → `repo_root_from_pack_dir(..).file_name()` (`crates/repolens-profile/src/profile.rs:88-92,122-126,529-533`). Scanning the random `tempdir()` basename directly would bake a random `"name"` into `project-profile.json` that path-substitution canonicalization cannot fix (it is a bare value, not a path). A fixed child name yields `"name": "<fixture>"` deterministically. (`golden-pack-diff.sh` is immune because its same-dir A/B cancels the basename; a single committed golden is not.)
2. **Scan.** `ScanService::new().run(&child, /* force_full */ true, /* keep */ 1)` → `ScanResult { pack_dir, .. }` (signature: `crates/repolens-app/src/scan.rs:122`; `pack_dir: PathBuf` at `:98`). A fresh child dir has no base pack, so the scan is full regardless; `force_full = true` makes that explicit and bypasses the incremental path (`scan.rs:126`).
3. **Collect content artifacts** from `pack_dir`, excluding `manifest.json`: every `files-*.json` (sorted by filename), `graph.json`, `graph-metrics.json`, `project-profile.json`. Read each as text (not re-serialized) so the snapshot reflects the bytes the pack writer actually emits.
4. **Canonicalize** environment-specific tokens in each file's text (see below).
5. **Assemble** one deterministic document: files sorted by name, each under a `===== <filename> =====` header, concatenated. `insta::assert_snapshot!("<fixture>_pack_bytes", doc)`.

### Fixtures covered

Parameterized over a `const FIXTURES: &[&str] = &["mixed-small", "rust-small", "java-small"]`. Adding/removing a fixture is one list entry plus its committed snapshot.

### Canonicalization rules

Applied as plain string substitution on each file's text **before** snapshotting:

- **Scan-root absolute path (`<temp>/<fixture>`) → `__REPO_ROOT__`.** Replace *both* the raw path and its `std::fs::canonicalize` form. (On macOS `tempfile::tempdir()` yields `/var/folders/…` while canonicalization yields `/private/var/folders/…`; the scanner may embed the canonical form.)
- **Pack-id `pack-YYYYMMDD-HHMMSS` → `pack-CANONICAL`.** Derive the exact token from `pack_dir.file_name()`; the id is embedded in content (e.g. evidence-scope `packId` in `project-profile.json`). Mirrors the substitution in `golden-pack-diff.sh:81`.
- **Project `name` basename** — handled *structurally*, not by substitution: scanning `<temp>/<fixture>` (not the random temp root) makes the profile `name` deterministic (`<fixture>`). See Mechanism step 1.

After substitution, the pack directory path appears deterministically as `__REPO_ROOT__/.repolens/pack-CANONICAL`, so no separate pack-dir rule is needed.

### Determinism guards (both cheap on ≤9-file fixtures)

- **Scan-twice equality.** Scan each fixture into two independent temp dirs, canonicalize both, and `assert_eq!` the documents **before** snapshotting. This is the real safety net for latent run-to-run nondeterminism. Graph **build/merge** order is already stabilized (`BTreeMap` + sorted merge, `crates/repolens-app/src/graph.rs:167,249`; `perLanguage` is a `BTreeMap`, `crates/repolens-core/src/model/graph.rs:51`), but several **resolution** paths still iterate hashed collections — Rust workspace-crate detection over a `HashSet` (`graph.rs:319`), Java wildcard-import expansion over `HashMap` keys (`graph.rs:340`), Go module-path first-match from a `HashSet` (`graph.rs:399`). These are expected to be stable for the three named fixtures, but the spec does **not** claim they are eliminated — the scan-twice guard is what makes relying on them safe. **Contingency:** if scan-twice fails for a fixture (most plausibly `rust-small` via `graph.rs:319`), the fix is to make that one iteration deterministic (collect→sort) — a small, isolated `graph.rs` change folded into this work.
- **No-leak assertion.** Assert the canonicalized document contains neither the raw nor the canonical scan-root path. This catches only *full-path* leakage; a bare-basename leak (the issue fixed structurally in Mechanism step 1) would not be caught here, but is prevented by the fixed child-dir name and would also trip scan-twice.

### What is excluded, and why

- **`manifest.json`** — carries the generation timestamp and `headCommit`/`worktreeDirty`. The timestamp always varies; provenance is collected via `gix::open(root)` with **no upward repo discovery** (`crates/repolens-core/src/git.rs:188,223`; `.git` is checked only as a direct child, `storage.rs:62`), so a non-git scan root records `headCommit: None` / `worktreeDirty: false` and never captures the enclosing `repolens-rs` HEAD. Excluding `manifest.json` sidesteps the timestamp regardless. Its *shape* is already covered by the scenario contract tests. (Mirrors `golden-pack-diff.sh`, which skips `manifest.json`.)
- **Raw `files/` content blobs** — verbatim copies of the fixture source; uninformative about extraction/serialization and noisy in a snapshot. The `files-*.json` index (with `sha256` + `contentPath` + symbols) **is** in the golden, so captured-content drift surfaces as a changed `sha256`. Note `I-FILE-CONTENT-SHA256-PARITY` runs only over captured *scenario* packs (`crates/repolens-core/tests/pack_contract_test.rs:567`), **not** this freshly-scanned pack — so to keep index↔blob integrity covered without snapshotting blobs, the new test adds a direct parity assertion: for each included entry, hash `pack_dir/<contentPath>` and assert it equals `entry.sha256`.

### CI integration

No workflow edits. `.github/workflows/ci.yml` already triggers on:
- `**/*.rs` → the new test file,
- `**/*.snap` → the new goldens,
- `**/fixtures/**` → the scanned fixtures,
- `**/Cargo.toml` → the `repolens-app` dev-dep addition (`.github/workflows/ci.yml:40`).

The test runs in the existing **Build & Test** matrix (hosted `ubuntu-latest` + self-hosted `macOS`) via `cargo test --workspace`. On a legitimate output change, re-bless with `cargo insta review`; the `.snap` diff is part of the PR for human review.

### Dependencies

- Add `insta = { workspace = true }` to `crates/repolens-app/Cargo.toml` `[dev-dependencies]` (workspace-pinned at root `Cargo.toml:15`, `version = "1", features = ["json"]`; already used by `repolens-core`/`repolens-mcp`). **Not a new dependency** to the workspace.
- `tempfile` is already a `repolens-app` dev-dependency.

## Component boundaries (test-internal helpers)

Small, single-purpose helpers within the test binary (no production-code change beyond the `Cargo.toml` dev-dep):

- `fixture_inputs_dir() -> PathBuf` — resolve `fixtures/inputs/` from `CARGO_MANIFEST_DIR` (pop `crates/repolens-app`, push `fixtures/inputs`), mirroring `fixture_helpers.rs`.
- `copy_fixture_into_named_child(fixture: &str) -> (TempDir, PathBuf)` — make a `TempDir`, recursively copy `fixtures/inputs/<fixture>/` into `<temp>/<fixture>` (skipping `.repolens/`), and return the temp guard plus the child path to scan.
- `assert_sha256_parity(pack_dir: &Path)` — for each included entry, hash `pack_dir/<contentPath>` and assert it equals `entry.sha256`.
- `scan_and_render(child: &Path) -> String` — run the scan, assert sha256 parity, then return the canonicalized, assembled document.
- `canonicalize(text, scan_root, pack_id) -> String` — apply the substitution rules.

Each is independently understandable and testable; the public surface is the single `#[test]`.

## Error handling

Test code: `unwrap()`/`expect()` with messages naming the fixture and step (consistent with `evidence_findings_e2e_test.rs` and `fixture_helpers.rs`). A scan or copy failure should panic with a message identifying which fixture failed.

## Testing strategy / "does it bite"

- The test *is* the regression net: `cargo test -p repolens-app --test pack_byte_identity_test`.
- **Bite check (manual, during implementation, then reverted):** perturb an extractor (e.g. change a symbol field) and confirm the relevant snapshot diffs and the test fails. Documented in the plan; not committed.
- Full gate: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

## Non-goals

- Not wiring `golden-pack-diff.sh` into CI, and not removing or changing it.
- Not snapshotting `manifest.json` or raw `files/` blobs.
- Not adding new CI jobs/steps or new runtime dependencies.
- Not covering `go-small`, `js-small`, or `excluded-small` (Go/JS/TS already exercised by `mixed-small`; `excluded-small` is about exclusion semantics, covered by existing contract tests). The fixture list stays trivially extensible if that changes.
- Not asserting cross-language *correctness* of extraction — only that serialized output does not drift unreviewed.

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| **Random temp basename leaks into `project-profile.json` `name`** (workspace-only Cargo / Maven fall through to the root basename) | **Scan a fixed child `<temp>/<fixture>`** so `name` is deterministic; structural naming + scan-twice both defend it. |
| Latent nondeterminism (hashed-collection iteration in resolution) makes the golden flaky | Scan-twice equality guard fails loudly *before* snapshotting; build/merge order already stable; contingency is a small collect→sort fix in `graph.rs` (see Determinism guards). |
| macOS `/var`→`/private/var` symlink leaks a machine path | Canonicalize both raw and `fs::canonicalize` forms of the scan-root child; no-leak assertion proves completeness. |
| Snapshot churn on legitimate output evolution | Expected and desired — `cargo insta review` turns drift into a reviewed diff. |
| Scan writes `.repolens/` into the tree it scans | Scan the temp child copy, never the real `fixtures/inputs/` tree. |
| Test slows the suite | Each fixture is ≤9 files; a full scan is milliseconds × 3 fixtures × 2 (determinism) — negligible. |

## Acceptance criteria

1. `crates/repolens-app/tests/pack_byte_identity_test.rs` scans each fixture from a **named child dir** `<temp>/<fixture>`; canonicalizes (path + pack-id); asserts scan-twice determinism, no path leak, and index↔blob `sha256` parity for every included entry; snapshots each.
2. `cargo test -p repolens-app --test pack_byte_identity_test` is green with the three `.snap` goldens committed; scan-twice is stable for all three (if any fixture is unstable, the offending `graph.rs` iteration is made deterministic in the same change).
3. Full gate green (`fmt`, `clippy -D warnings`, `test --workspace`).
4. No change under `.github/workflows/`; the new files are confirmed to match the existing `paths:` triggers.
5. Bite check demonstrated and reverted (a deliberate extractor perturbation makes a snapshot fail).
6. `scripts/golden-pack-diff.sh` unchanged; `BACKLOG.md` not edited (maintainer-owned).

## References

- `BACKLOG.md` — Icebox: "Byte-identity safety net in CI (review F2)".
- `scripts/golden-pack-diff.sh` — local vs-ref tool; canonicalization precedent (`:81`).
- `crates/repolens-app/tests/evidence_findings_e2e_test.rs` — live-scan-in-test pattern (`tempdir` + `ScanService::new().run(...)`).
- `crates/repolens-app/src/scan.rs:98,122` — `ScanResult.pack_dir`, `ScanService::run` signature.
- `crates/repolens-core/tests/fixture_helpers.rs`, `pack_contract_test.rs` — fixture resolution, placeholder + sha256 precedents.
- `.github/workflows/ci.yml:40-65` — `paths:` triggers (`**/*.rs`, `**/*.snap`, `**/fixtures/**`).
