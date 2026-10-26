# Pack Retention and Cleanup Design Spec

## Overview

Add bounded pack retention to keep `.repolens/pack-*` growth under control:

1. Automatic cleanup after a successful scan
2. A manual `clean` CLI command for on-demand cleanup

Retention is count-based, not age-based. By default, the system keeps the newest 3 packs.

## Why This Needs to Exist

Each scan writes a fresh `pack-YYYYMMDD-HHMMSS` directory. Those packs are several megabytes each and currently accumulate forever. Incremental scans reduce scan cost, but they do not reduce disk growth. Cleanup should be cheap, predictable, and safe enough to run after every successful scan.

## Key Design Constraints

- Follow existing storage conventions in `repolens_core::storage::RepoStoragePaths`
- Preserve the current scan success path: cleanup must never turn a successful scan into a failed scan
- Keep retention semantics simple and deterministic: newest-first by pack directory name
- Match the actual pack format used today: incremental packs are self-contained after write, because carried-forward content is materialized into the new pack
- Reuse the same root canonicalization behavior as the existing `scan` command

## User-Facing Behavior

### `scan`

Extend the existing command:

```bash
cargo run -- scan --root <path> [--full] [--keep N]
```

- `--keep N` is optional
- default is `3`
- minimum is `1`
- after a successful scan writes the pack and updates `session.json`, cleanup runs best-effort

### `clean`

Add a new top-level command:

```bash
cargo run -- clean --root <path> [--keep N]
```

- `--root` is required and canonicalized exactly like `scan`
- `--keep N` is optional, default `3`, minimum `1`
- command deletes eligible old packs and prints a summary to stdout

## Retention Rules

### Baseline Rule

Keep the newest `N` `pack-*` directories under `.repolens/`, where `N = keep`.

Newest/oldest is determined by reverse lexicographic sort of the directory name, matching the existing `pack-YYYYMMDD-HHMMSS` naming scheme already used for incremental pack discovery.

### Protected Pack

One pack is always protected from deletion:

- the pack referenced by `.repolens/session.json -> activePackPath`, if that path exists and still points at a pack directory under this repo's `.repolens`

If the active pack is already inside the newest `N`, protection changes nothing. If it is older, retention expands to keep it as well.

### Important Non-Rule: Do Not Retain Base Packs Transitively

We should not keep `basePackId` ancestors just because a retained pack references them in `manifest.json`.

Reason:

- `MergedPackWriter` materializes carried-forward file content into the new pack
- `PackReader` reads only the pack's own `manifest.json` and chunk files
- current readers do not require the base pack to remain on disk

Keeping transitive base packs would grow retention beyond the requested count without providing real safety for the current format.

## Shared Cleanup API

Add a shared cleanup module in `crates/repolens-app/src/cleanup.rs`:

```rust
pub const DEFAULT_KEEP_COUNT: usize = 3;

pub struct CleanupResult {
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
    pub kept: usize,
}

pub fn cleanup_old_packs(repo_root: &Path, keep: usize) -> anyhow::Result<CleanupResult>
```

### `CleanupResult` semantics

- `deleted`: pack directory names successfully removed
- `failed`: pack directory names that were selected for deletion but could not be removed
- `kept`: number of pack directories remaining after cleanup finishes, including undeletable packs

The `failed` field is needed so the manual CLI command can report partial success honestly. Without it, the command can only print misleading summaries.

## Cleanup Algorithm

1. Resolve `.repolens/` via `RepoStoragePaths::storage_dir(repo_root)`
2. If `.repolens/` does not exist, return `CleanupResult { deleted: vec![], failed: vec![], kept: 0 }`
3. Enumerate direct child directories whose names start with `pack-`
4. Sort them newest-first by path/name
5. Load `session.json` via `SessionStore::load(repo_root)`
6. Extract the protected active pack only if all of the following are true:
   - session exists
   - `active_pack_path` is non-empty
   - it resolves to a directory
   - it lives directly under this repo's `.repolens`
   - its directory name starts with `pack-`
7. Build the retained set:
   - the first `keep` packs from the sorted list
   - plus the protected active pack, if present
8. Attempt to delete every non-retained pack with `std::fs::remove_dir_all`
9. Record successful deletions in `deleted`
10. Record failed deletions in `failed`, log a warning, and continue
11. Return the final summary

## Integration Points

### App integration

Update `crates/repolens-app/src/lib.rs` to export the new module.

Update scan flow in both successful scan paths:

- `ScanService::run(...)` accepts `keep: usize`
- `run_full(...)` and incremental execution both call cleanup only after:
  - pack write succeeds
  - graph write succeeds
  - session update succeeds
- cleanup errors are caught at the scan boundary, logged as warnings, and do not fail the scan

This keeps cleanup out of the critical correctness path. If scan output exists on disk and session points at it, the scan is considered successful even if old-pack deletion had problems.

### CLI integration

Update `crates/repolens-cli/src/main.rs`:

- add `keep: usize` to `ScanArgs`
- add a `Clean(CleanArgs)` subcommand
- validate `keep >= 1` with clap rather than manual runtime checks
- pass `keep` through to `ScanService`
- add `run_clean(args)` that invokes `cleanup_old_packs`

Recommended CLI surface:

```rust
#[arg(long, default_value_t = repolens_app::cleanup::DEFAULT_KEEP_COUNT, value_parser = clap::value_parser!(usize).range(1..))]
keep: usize,
```

## Output and Logging

### `clean` command stdout

If packs were deleted and none failed:

```text
Cleaned 4 packs, kept 3
```

If some deletions failed:

```text
Cleaned 3 packs, failed to delete 1, kept 4
```

If nothing was eligible:

```text
Nothing to clean (3 packs kept)
```

We do not need to print every deleted pack name in the first version. Counts are enough for the CLI surface, and detailed paths can go to logs if needed later.

### `scan` command stdout

Do not change the existing human-facing scan stdout in this design.

Cleanup should be logged with `tracing` at `info!` or `warn!`, but it should not add a new stdout line unless we later decide that scan output needs to expose retention details explicitly. That keeps this feature smaller and avoids adding cleanup-only fields to `ScanResult` unless they become necessary.

## Error Handling

### Non-fatal conditions

These should not fail cleanup:

- missing `.repolens/`
- missing `session.json`
- corrupt `session.json` because `SessionStore::load()` already degrades to `Ok(None)`
- active pack path that points outside the repo storage directory
- failed deletion of one specific pack

### Fatal cleanup errors

These should return `Err(...)` from `cleanup_old_packs`:

- inability to enumerate `.repolens/`
- unexpected filesystem errors before the algorithm can determine the candidate set

Scan catches and logs those errors. Manual `clean` returns them to the user as command failure.

## Testing

Add focused tests in `crates/repolens-app` for the cleanup module:

1. No storage directory returns an empty result
2. Fewer packs than `keep` deletes nothing
3. More packs than `keep` deletes the oldest packs and keeps the newest `N`
4. Active pack from `session.json` is preserved even when older than the newest `N`
5. Corrupt or missing `session.json` falls back to pure newest-`N` retention
6. Non-pack directories inside `.repolens/` are ignored
7. Best-effort delete continues after one deletion failure and reports that failure

Add CLI integration tests for:

8. `clean --keep 1` leaves exactly one pack (superseded by #37: it also keeps the newest clean pack if that is older, so up to two)
9. `scan --keep 1` triggers automatic cleanup after repeated scans

The delete-failure test should be deterministic. If the platform makes filesystem permission failures unreliable, factor deletion behind a small helper so the failure path can be tested without depending on OS-specific behavior.

## Non-Goals

- No age-based expiration window
- No size-based retention policy
- No persistent config file for retention settings
- No MCP cleanup tool
- No attempt to compact or rewrite existing packs
- No retention of historical incremental ancestry beyond the actual retained pack set
