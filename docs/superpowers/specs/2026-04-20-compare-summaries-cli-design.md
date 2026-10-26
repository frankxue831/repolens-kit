# `compare-summaries` CLI Route Design

**Date:** 2026-04-20
**Status:** Accepted
**Depends on:** [2026-04-20-workflow-closure-phase-1-design.md](./2026-04-20-workflow-closure-phase-1-design.md), [2026-04-20-workflow-closure-phase-2-dogfood-design.md](./2026-04-20-workflow-closure-phase-2-dogfood-design.md), [../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md](../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md), ../../opinion_pool/opinion_pool_006.md, ../../opinion_pool/opinion_pool_007.md

## Goal

Expose `compare_summaries` as a CLI subcommand so the dogfood-snapshot's eval question 5 ("what changed between the latest two packs?") can be answered without an MCP client.

Phase 1 established the eval questions and PR snapshot rule. Phase 2 shipped the mechanical-snapshot helper. This slice closes the question-5 gap that both earlier phases explicitly deferred — it is a product surface change in `repolens-cli`, not more dogfood-routine scaffolding.

## Problem Statement

The workflow-closure eval questions asked at the PR snapshot stage include:

> **Q5.** What changed between the latest two packs that matters to an agent?

Per the first dogfood note's friction observation **F4**, this question has no CLI route today:

> `compare_summaries` is MCP-only. If we want PR snapshots to actually answer "what changed between the latest two packs?", either the helper has to drive an MCP client (heavy), or `repolens compare-summaries` needs to exist as a CLI subcommand (small, but a product surface change).

The phase-2 dogfood helper deliberately deferred this — it leaves Q5 as a manual `Answer: TODO` line. After phase 2 shipped, the eval question remains unanswerable without spinning up a full MCP client, which none of the dogfood snapshots to date have done.

Adding a CLI route is small in isolation but it *is* a product surface change, and deferring it into a separate spec (this document) rather than folding it into phase 2 was the opinion_pool_007 §7 discipline — "project hygiene, not product surface" was the phase-2 boundary.

## Non-Goals

This slice deliberately does not do the following, each deferred with a named trigger for revisiting:

- **No fresh scan as a side effect of `compare-summaries`.** Trigger to revisit: if contributors consistently report forgetting to scan before comparing, fold scan-then-compare into the dogfood helper (not the CLI).
- **No memory-insight diffing.** Trigger: after the memory graph's semantics stabilize enough that "added/removed/superseded insight" has a contract the comparison layer can depend on.
- **No `--since <commit>` or `--since HEAD~1` shortcuts.** Trigger: a concrete dogfood snapshot that tries to use it and has to fall back to pack IDs.
- **No `--format yaml|tsv|plain`.** Trigger: a second consumer of the CLI output that actually needs one of those shapes.
- **No changes to the `SummaryComparison` struct.** Trigger: a field the markdown renderer cannot express from the current shape.
- **No epistemic envelope in JSON output.** Trigger: an agent wants to consume the CLI output through a grounded-reasoning pipeline instead of driving MCP directly — at which point it should drive MCP directly, not wrap CLI JSON.

## Options Considered

### Option A — Explicit-only (always `--before` and `--after` required)

Mirror the MCP tool's strictness: no defaults, every invocation names both pack IDs.

**Rejected** because the F4 use case — the dogfood PR snapshot — almost always wants "compare the two most recent packs." Forcing explicit IDs means every snapshot contributor has to `ls .repolens/pack-*` first, defeating the low-friction goal.

### Option B — Latest-two default, flag override

Default (no flags): compare the two most recent packs in `.repolens/pack-*/` by lexical order. Override with `--before <pack-id>` and/or `--after <pack-id>` for pinned baselines.

**Recommendation.**

### Option C — Subcommand nested under a `repolens summary` group

Introduce `repolens summary { get | compare }` so a future `get_summary` CLI and this slice share a namespace.

**Rejected as YAGNI** — no concrete plan or spec exists yet for a `get_summary` CLI, and adding a command-tree level now forces every future caller through that namespace without evidence it earns its keep. `repolens compare-summaries` (flat) mirrors the MCP tool verb exactly and can be grouped later if the second command materializes.

### Option D — Auto-scan then compare

Run a fresh scan first so there is always a "latest" pack, then compare the two most recent.

**Rejected** because it collides with the phase-2 dogfood helper (which already does fresh-scan + snapshot-emit) and muddies the read-only contract. A caller who wants both should compose `repolens scan && repolens compare-summaries`, or use the dogfood helper.

## Decision

Ship two artifacts under one spec:

1. A new `Commands::CompareSummaries(CompareSummariesArgs)` subcommand in `crates/repolens-cli/src/main.rs`.
2. A markdown renderer, pack-listing helper, and small CLI-local normalization layer (pack-ID validation + stable recommendation ordering), scoped to make the CLI work without mutating the existing `SummaryComparison` / `ComparisonService` / `SummaryService` surface.

The CLI is strictly read-only on `.repolens/pack-*/`.

## Command Surface

```
repolens compare-summaries [--root <path>]
                           [--before <pack-id>]
                           [--after <pack-id>]
                           [--format markdown|json]
```

### Flags

| Flag       | Type            | Default                                | Purpose                                                     |
| ---------- | --------------- | -------------------------------------- | ----------------------------------------------------------- |
| `--root`   | `PathBuf`       | Enclosing git worktree (PR #26 rule)   | Locate `.repolens/pack-*/`                                  |
| `--before` | `String` (ID)   | Second-most-recent pack (lexical sort) | Pin the earlier baseline                                    |
| `--after`  | `String` (ID)   | Most-recent pack (lexical sort)        | Pin the later baseline                                      |
| `--format` | `markdown\|json` | `markdown`                             | Output format (no auto-detect, no environment sniffing)     |

### Pack ID grammar

`--before` / `--after` accept either form:

- Full directory name: `pack-20260420-112702`
- Bare timestamp: `20260420-112702` (the CLI auto-prepends `pack-`)

The accepted grammar is therefore:

- `^pack-\d{8}-\d{6}$`
- `^\d{8}-\d{6}$`

Everything else is rejected up front. In particular, the CLI rejects:

- absolute paths
- path separators (`/` or `\`)
- parent-directory segments (`..`)
- arbitrary directory names that merely start with `pack-`

Absolute paths are not accepted from the CLI — they would bypass the `.repolens/` scope and create inconsistent error paths between CLI-vs-MCP callers. The MCP tool keeps its path-based interface; the CLI always resolves against `.repolens/`.

### Exit codes

| Exit | Meaning                                                            |
| ---- | ------------------------------------------------------------------ |
| `0`  | Comparison computed and emitted successfully.                      |
| `1`  | Any failure — printed to stderr, no partial stdout (matches `scripts/dogfood-snapshot.sh`). |

## Pack Resolution

### Default path (no `--before` / `--after`)

1. List directory entries under `<root>/.repolens/` whose names start with `pack-`.
2. Sort by directory name. The `pack-YYYYMMDD-HHMMSS` format makes lexical order equal chronological order — no filesystem `mtime` is read.
3. If fewer than two survive, fail with the appropriate error (see below).
4. The last two in sorted order become `before` (older) and `after` (newer).

### Override path (one or both flags present)

1. Parse each flag value. If the value begins with `pack-`, use as-is; otherwise auto-prepend `pack-`.
2. Reject the value unless it matches the pack-ID grammar above. This validation happens before any path join.
3. Verify `<root>/.repolens/<resolved-id>/` exists and is a directory.
4. If only `--after` is supplied, resolve `before` as the newest pack whose directory name sorts strictly before the chosen `after`.
5. If only `--before` is supplied, resolve `after` as the oldest pack whose directory name sorts strictly after the chosen `before`.
6. If the missing side has no chronological neighbor, fail.
7. If both flags are supplied, require `before < after` under the same lexical ordering the default path uses; equal or inverted pairs fail.

Neighbor semantics are intentional: `--after X` means "compare X against its immediate predecessor," not "compare X against some global default such as the second-most-recent pack." An earlier draft of this spec used the global-default fallback; it was replaced because a pinned `--after` almost always implies interest in that specific pack's transition, which only the neighbor reading preserves.

### Sorting note

This design relies on the pack-directory naming convention. If a future change introduces pack directories whose names do not embed a sortable timestamp, this slice breaks in a specific, loud way: the default "latest two" call and the one-sided override neighbor lookup may pick the wrong pair. A comment on the pack-listing helper documents this coupling so a future refactor of the pack naming scheme updates both places together.

## Output Shapes

### Markdown (default)

The CLI emits exactly one markdown block to stdout. Empty sections render as `_(none)_` rather than being dropped, so the block's shape is stable across PRs (important for dogfood snapshot readability).

```
## Summary Comparison

Before: `pack-20260420-112702` | After: `pack-20260421-091533` | Δ fitness: `+0.13`

### Fitness Scores

| Dimension       | Before | After | Delta |
| --------------- | -----: | ----: | ----: |
| Overall         |  0.72  |  0.85 | +0.13 |
| Resolution rate |  0.80  |  0.90 | +0.10 |
| Coupling        |  0.65  |  0.78 | +0.13 |
| Noise reduction |  0.70  |  0.88 | +0.18 |

### Metrics

| Metric         | Before | After | Trend |
| -------------- | -----: | ----: | :---: |
| File count     |    306 |   312 |   ↑   |
| Included count |    303 |   309 |   ↑   |
| Total LoC      |  28144 | 28867 |   ↑   |
| Graph nodes    |    412 |   418 |   ↑   |
| Graph edges    |   1238 |  1267 |   ↑   |

### Recommendations

**Added (2):**
- `rec-id-1` — "Title" (`path/to/file`)
- `rec-id-2` — "Title"

**Removed (1):**
- `rec-id-3` — "Title" (`path/to/file`)
```

Notes on rendering:

- `MetricDelta.before` and `after` are already `String` (see `crates/repolens-core/src/model/summary.rs:153`) — the renderer inserts them as-is without numeric parsing.
- `MetricDelta.trend` is already a string like `"improved"` / `"degraded"` / `"unchanged"` — the renderer maps to arrow glyphs (`↑ ↓ →`) purely for display; the string values are the authoritative source.
- `RecommendationDelta.file` is `Option<String>` — rendered in parens when present, omitted when absent.
- Fitness score deltas (per-dimension) are computed in the renderer (`after - before`) because `SummaryComparison` today only carries the `overall` delta (`delta_fitness`). Adding per-dimension deltas to the struct is listed in Non-Goals above.
- Recommendation deltas are canonicalized in the CLI before output so markdown snapshots stay stable across runs. Sort rule: `action`, then `id`, then `file` (with `None` sorted before `Some`).
- `FitnessScore.breakdown` (`Option<serde_json::Value>` on the struct) is passed through unchanged in `--format json` but omitted from markdown rendering. Its shape is not stable enough to render usefully, and the markdown path exists for dogfood snapshots, not deep debugging.

### JSON (`--format json`)

`serde_json::to_string_pretty(&SummaryComparison)` with trailing newline, after the same CLI-local recommendation-delta canonicalization used by the markdown renderer. No epistemic envelope, no wrapping object, no metadata — the CLI's JSON is the same schema and field layout the MCP tool serializes inside its envelope. Note the byte-level divergence: the MCP tool emits `recommendations[]` in the order `ComparisonService`'s `HashMap` iteration happens to produce (unstable across runs), whereas the CLI sorts that array deterministically. Callers needing envelope/provenance drive the MCP tool, not the CLI.

## Error Cases

Each failure prints a single-line message to stderr and exits `1`. No partial markdown/JSON is written to stdout on any failure path.

| Condition                                                  | Message                                                                                  |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `.repolens/` missing under resolved root                    | `No pack directory found at <path>. Run 'repolens scan' first.`                          |
| Fewer than 2 packs exist and no flags override              | `Only 1 pack in .repolens/. Need at least 2 to compare.`                                 |
| `--before` or `--after` fails the pack-ID grammar          | `Invalid pack id: <raw-value>. Expected 'pack-YYYYMMDD-HHMMSS' or 'YYYYMMDD-HHMMSS'.`    |
| `--before` or `--after` refers to a missing pack            | `Pack not found: <resolved-id>`                                                          |
| One-sided override has no chronological neighbor            | `Cannot resolve comparison pair from <resolved-id>: no older/newer pack exists.`         |
| Both flags resolve to the same pack                         | `Before and after resolve to the same pack: <resolved-id>`                               |
| `--before` sorts after `--after`                            | `Before pack must be older than after pack: <before-id> >= <after-id>`                   |
| Either pack lacks fitness scores                            | Passthrough of the existing MCP-tool error — both slices share `ComparisonService::compare`'s `None` return, same message. |
| Summarize failure (`SummaryService::summarize` returns err) | `Failed to build summary for <pack-id>: <underlying error>`                              |
| Pack directory exists but is not readable                   | `Could not read pack: <pack-id>: <underlying error>`                                     |

## Implementation Shape

### Crate placement

- **Subcommand enum + handler:** `crates/repolens-cli/src/main.rs`, mirroring `Commands::Scan` / `Commands::Clean`. A dedicated `commands/compare.rs` module is optional — use it only if the handler grows beyond roughly 150 lines.
- **Pack listing:** new helper `fn list_pack_dirs(repo_root: &Path) -> Result<Vec<PathBuf>>` in `crates/repolens-app/src/discover.rs`. Lives alongside `PackResolver` because pack discovery is an `app`-layer concern; multiple future callers (CLI, MCP, dogfood tooling) can reuse it.
- **CLI-local normalization + renderer:** pack-ID validation / neighbor resolution / stable recommendation ordering plus `fn render_comparison_markdown(cmp: &SummaryComparison, before_id: &str, after_id: &str) -> String` in the CLI crate. Kept out of `repolens-app` until a second caller materializes.
- **`--format` validation:** implemented as `clap::ValueEnum` on a `OutputFormat { Markdown, Json }` enum so invalid values fail at parse time with clap's standard error, matching `--keep`'s `value_parser` pattern in the same file.

### Reuse of existing surface

- `SummaryService::summarize(pack_dir, None, SummaryMemoryTier::Full)` — unchanged, called twice (one per pack).
- `ComparisonService::compare(&before, &after) -> Option<SummaryComparison>` — unchanged.
- `SummaryComparison` / `MetricDelta` / `RecommendationDelta` / `FitnessScore` — unchanged.

No edits are planned in `repolens-app/src/summary.rs`, `repolens-core/src/model/summary.rs`, or any MCP tool file. If the implementation discovers it needs to, that is a signal the scope is wider than this spec and a short design addendum should precede the change.

## Verification Plan

1. Unit test the markdown renderer against a hand-built `SummaryComparison` fixture (filled recommendations, empty recommendations, partial-fitness edge cases). `insta` snapshot if the existing profile/summary tests already use it, plain `assert_eq!` otherwise.
2. Unit test `list_pack_dirs` against a tempdir populated with `pack-<timestamp>` directories plus noise (a non-`pack-*` file, an empty dir). Assert sort order and lexical-sort correctness.
3. CLI parse-level integration tests in `crates/repolens-cli/src/main.rs` alongside `scan_defaults_root_to_current_directory`:
   - `compare-summaries` with no args parses to `Commands::CompareSummaries` with all flags `None`.
   - `--before pack-A --after pack-B --format json` parses all fields.
   - Auto-prepend: `--before 20260420-112702` parses to the bare timestamp (prepending happens in the handler, not the parser, so tests here assert the raw `--before` value passes through).
4. Handler-level tests for pack-ID validation and pair resolution:
   - reject absolute paths, separators, `..`, and malformed `pack-*` names;
   - resolve zero-flag call to the latest two packs;
   - resolve one-sided overrides to the nearest chronological neighbor;
   - fail when one-sided override has no neighbor;
   - fail when explicit `before >= after`.
5. End-to-end integration test that builds two minimal packs in a tempdir, runs the CLI, and asserts markdown-block shape plus deterministic recommendation ordering.

## Acceptance Criteria

- `repolens compare-summaries --help` documents all four flags and the exit-code contract.
- Zero-flag invocation in a repo with ≥ 2 packs emits the markdown block on stdout, exit 0.
- `--format json` emits the same `SummaryComparison` schema and field layout as the MCP tool's inner data (minus envelope), validated by round-tripping through `serde_json::from_str::<SummaryComparison>`. `recommendations[]` order is canonicalized for determinism (the MCP tool's `HashMap`-sourced order is not).
- Pack-ID inputs outside the accepted grammar are rejected before any filesystem access.
- One-sided overrides preserve chronology by selecting the nearest valid neighbor, not an unrelated global default.
- All failure cases in the Error Cases table return exit 1, stderr-only output, no partial stdout.
- Markdown and JSON outputs are deterministic for the same pair of packs.
- No changes to `repolens-core`, `repolens-mcp`, or `repolens-app::summary` module internals.
- `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` clean.

## Follow-on Decision Rule

After 2-3 real PRs use `repolens compare-summaries` in their dogfood snapshot to answer Q5, re-evaluate:

- whether the five `SummaryComparison` metrics are the right set (current choices: fileCount, includedCount, totalLoc, graphNodes, graphEdges);
- whether the markdown shape needs trimming or expanding;
- whether memory-insight diffing (explicitly deferred above) has a clear contract to move on;
- whether a flat `repolens summary compare` would be more discoverable as part of a `repolens summary …` group.

The evidence trigger matches the phase-2 dogfood design's follow-on rule: evidence from repeated use, not speculation from a single data point.
