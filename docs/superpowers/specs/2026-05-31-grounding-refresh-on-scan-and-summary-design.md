# Grounding refresh on scan + get_summary — widen the snapshot write surface

**Date:** 2026-05-31
**Status:** design approved (brainstormed + user-reviewed)

## Problem

PR #70 (2026-05-29) made the decay engine consume a persisted recall-time
grounding snapshot (`MemoryNode.last_grounding`): `Confirmed` memories get an
importance floor (slower decay), `Contradicted` a multiplier (faster
decay). But the **only** code path that writes that snapshot is
`recall_longterm`. doc 07 row 26 / Known Mismatch #3 record the consequence: a
memory that has **not been recalled since the code drifted carries a stale
grounding** until its next recall. The self-correcting loop the product implies
— *memory that quietly distrusts itself when the code it describes drifts away* —
fires only when an agent happens to call `recall_longterm`.

Two higher-traffic / more-principled moments already have everything needed to
refresh the snapshot but don't:

- **Scan** (`repolens scan`) is the moment code drift becomes observable — it
  produces a fresh pack reflecting current code — yet it never touches
  `memory.json`.
- **`get_summary`** is the first context surface most agents call. It already
  loads the memory graph, runs lazy decay, **saves**, and computes
  `assess_grounding` per surfaced insight — but never persists that assessment.

## Goal

Widen the grounding-snapshot **write** surface to two more triggers, reusing
the PR #70 mechanism (no new fields, no decay change):

1. **Scan-time (all memories, unbiased):** after `repolens scan` builds a pack,
   ground **every** memory against it and persist the snapshot.
2. **get_summary (selected insights):** stamp the grounding snapshot for the
   tier-selected insights `get_summary` surfaces.

This closes the "stale-until-recall" caveat: a memory is re-grounded whenever
the repo is rescanned (the drift moment) or summarized, not only on explicit
recall.

## Non-goals

- **No decay at scan time.** Scan writes the grounding snapshot **only**; it does
  **not** run a decay pass. Decay stays lazy / read-triggered exactly as today
  (`recall_longterm` + `get_summary`). The fresh snapshot influences the *next*
  decay run. Rationale: decay compounds per run, so making scan a decay trigger
  would change the established cadence and make scan-frequency affect memory
  longevity — out of scope. (User decision, 2026-05-31.)
- **`ScanService` stays a pure scan→pack service.** The scan-time refresh lives
  in the CLI `run_scan`, **not** inside `ScanService::run`. `ScanService::run`
  has several internal callers (`bootstrap/planner.rs`, `bootstrap/service.rs`
  for `init`/`brief`, and graph tests); only the user-facing `repolens scan`
  command should re-ground memory. (User decision, 2026-05-31.)
- **get_summary stamps selected-only, not all loaded memories.**
  `select_memory_insights` already *assesses* every loaded memory (to compute the
  resolving-anchor filter), then sorts and truncates to the tier cap (compact 3 /
  standard 5 / full 20). This feature stamps **only the surfaced (truncated)
  insights** — assessment cost is unchanged; only the persisted-snapshot set is
  capped. Scan-time handles the unbiased full refresh, so get_summary need not
  stamp every loaded node on a read path (avoids extra write volume).
- **No schema change / migration.** `last_grounding` + `last_grounded_at`
  already exist (PR #70). This adds only new write call sites.
- **No edge-based representation, no looser/bare-name anchor grounding, no
  per-field envelope grades, no memory-schema closure.** Parked as separate
  follow-ons. Wider/looser anchor grounding (so bare names like `AuthService`
  stop returning `Unchecked`) is the natural next step and would make these
  snapshots informative for more memories, but is its own arc.

## Design

### Shared helper — `ground_all_memories` (pure, `repolens-app/src/grounding.rs`)

```rust
/// Ground every memory node in `graph` against `pack` and persist each node's
/// grounding snapshot (`last_grounding` + `last_grounded_at`). Returns the
/// number of nodes stamped (= all nodes; every node gets an honest snapshot,
/// `Confirmed` / `Contradicted` / `Unchecked` — never `Unverifiable`, because a
/// pack is always supplied). Idempotent — overwrites, never accumulates.
/// Pure: mutates `graph`, no I/O.
pub fn ground_all_memories(
    graph: &mut MemoryGraph,
    pack: &PackIndex,
    now: OffsetDateTime,
) -> usize {
    // Direct iter_mut, O(n): assess_grounding(node, ..) takes a shared reborrow
    // of `node` that ends when the owned assessment returns, so the subsequent
    // field mutation on the same `node` is fine. Do NOT loop calling
    // `record_grounding` here — it re-scans nodes by id on every call, making
    // a full refresh O(n^2). `record_grounding` stays for single-node callers
    // (recall, get_summary).
    for node in graph.nodes.iter_mut() {
        let status = assess_grounding(node, Some(pack)).grounding_status;
        node.last_grounding = Some(status);
        node.last_grounded_at = Some(now);
    }
    graph.nodes.len()
}
```

No filesystem access — load/save stays at the call sites. This keeps the helper
trivially unit-testable and reusable by both triggers. The call sites set
`graph.updated_at = now` before persisting (the helper mutates per-node
`last_*` fields only; bumping the graph-level timestamp keeps "the graph changed
at T" honest, matching the decay path — `decay.rs:168`).

**Inherited grounding semantics (unchanged).** Grounding uses `assess_grounding`
exactly as today. Two inherited behaviors worth stating because scan-time now
runs it over *all* memories: (a) it matches a `FileEntry` path regardless of
`entry.included` (`grounding.rs:51`), so an anchor to an *excluded* (e.g.
gitignored) file still grounds `Confirmed`; (b) a memory with anchors that are
not groundable (bare names, no `/` or `.`) or that has no anchors grounds
`Unchecked`. This feature does **not** change either — altering them would also
change `recall_longterm`. Whether excluded files should count is a parked
question, not in scope here.

### Trigger A — scan-time (`repolens-cli` `run_scan`)

After `ScanService::run` returns a `ScanResult`:

1. Load the just-written pack via `PackReader::read(&result.pack_dir)` (returns
   `io::Result`). **Best-effort: do NOT `?`** — on error, warn and skip the
   refresh. The scan already succeeded and must still report success.
2. `MemoryStore::load(root)`. If the graph has no nodes, skip silently (info
   log) — nothing to ground. (`load` returns `Ok(empty_graph)` for a
   missing/unparseable `memory.json` — `store.rs:53` — so the common "no memory
   yet" case is the 0-node skip, not an `Err`; a hard FS error is also a
   non-fatal skip.)
3. `let now = OffsetDateTime::now_utc(); let n = grounding::ground_all_memories(&mut graph, &pack, now);`
4. Set `graph.updated_at = now`, then `MemoryStore::save(root, &graph)`. **Only
   on success**, print one CLI line: `Refreshed grounding for N memories against
   the new pack.` On save error, warn and print nothing (the snapshot was not
   persisted). Every failure in steps 1–4 is **non-fatal** — the scan result is
   never lost.

No decay is run here. The CLI already depends on `repolens-memory`
(`MemoryStore` is used at `main.rs:533`) and on `repolens-app`
(`grounding::ground_all_memories`), so no new crate dependency.

Scope note: this fires for **both** full and incremental scans (both produce a
current pack via `ScanService::run`). It fires **only** for the `repolens scan`
command — internal `ScanService::run` callers (bootstrap, graph) are unaffected
because the refresh lives in `run_scan`, not in `ScanService`.

### Trigger B — get_summary (`repolens-app` `SummaryService::load_and_select_memory`)

Today this fn: loads the graph → (if due) runs lazy decay and saves →
`select_memory_insights(&graph, pack, tier)` returns insights carrying
`grounding_status`.

Add, after selection and before returning:

```rust
let insights = select_memory_insights(&graph, pack, tier);
// Write-back: persist the grounding snapshot for the surfaced insights so the
// next decay run reflects it (selected-only; scan handles the full refresh).
if !insights.is_empty() {
    for ins in &insights {
        grounding::record_grounding(&mut graph, &ins.id, ins.grounding_status.clone(), now);
    }
    if let Err(e) = MemoryStore::save(repo_root, &graph) {
        warn!(error = %e, "SummaryService: failed to save grounding snapshot");
    }
}
insights
```

`now` is the same timestamp already used for the lazy-decay step. Set
`graph.updated_at = now` before this save. Reusing `record_grounding` here is
fine — the selected set is tier-capped (≤ 20), so there is no O(n²) concern. The
`Skip`/no-memory tier never reaches `load_and_select_memory` (the `summarize`
match falls to `_ => Vec::new()`), so `compare_summaries` (which uses
`SummaryMemoryTier::Skip`) stays side-effect-free.

**Two saves when decay also runs (acceptable).** When lazy decay fires on this
call *and* `insights` is non-empty, the function performs **two sequential
saves**: the existing post-decay save, then this stamp save. This matches the
`recall_longterm` pattern. They are not atomic *across* each other (the stamp
save could fail after the decay save already persisted), but both operate on the
same in-memory graph with atomic temp-file+rename, so there is no in-process
lost update; a failed stamp save is warned and non-fatal. An optional
optimization — when stamping will occur, skip the separate decay save and save
once after stamping — is left to the implementation plan.

**Tier-filter semantic (intended, not a bug).** Compact/standard retain only
memories with **≥1 resolving anchor** (`resolved = !found_anchors.is_empty()`,
`summary.rs:235`/`249`). That set includes `Confirmed` memories **and
mixed-anchor `Contradicted`** ones (≥1 anchor found, ≥1 missing — `Contradicted`
because any missing anchor wins, `grounding.rs:70`). Only a **fully-drifted**
memory (every anchor missing → `Contradicted` with no found anchor) or an
`Unchecked`/`Unverifiable` one is filtered out at compact/standard, and is
surfaced/stamped only at the **full** tier. Fully-drifted memories are otherwise
refreshed by **scan-time** (the unbiased full pass) or by `recall`. So
get_summary write-back is a *supplement* to scan-time, not the primary path for
distrusting fully-drifted memories.

### Decay interaction & ordering

`decay.rs` is unchanged. Ordering is consistent with `recall_longterm`:

- **get_summary:** lazy decay runs first using the **prior** snapshot, then the
  **fresh** snapshot is stamped for the *next* decay run. (Same as recall.)
- **scan:** no decay; only the fresh snapshot is written, consumed by whatever
  read path next triggers lazy decay.

So the floor/multiplier semantics from PR #70 apply unchanged; this feature only
makes the snapshot fresher, more often, for more memories.

### Backward compatibility

Zero schema change. `last_grounding` / `last_grounded_at` are the PR #70
`#[serde(default)]` fields. Existing `memory.json` files are unaffected; this is
purely additive write behavior.

## Testing

**Helper unit tests** (`grounding.rs`):
- `ground_all_memories_stamps_every_node` — graph with a resolving anchor, a
  missing anchor, a bare-name anchor, **and a no-anchor node** + a pack → nodes
  get `Confirmed` / `Contradicted` / `Unchecked` / `Unchecked` respectively;
  returns count = node count. (No node can be `Unverifiable` — a pack is always
  supplied.)
- `ground_all_memories_empty_graph_is_zero` — no nodes → returns 0, no panic.
- `ground_all_memories_overwrites_prior_snapshot` — a node with a stale
  `last_grounding` gets re-stamped (idempotent overwrite).

**get_summary integration** (`repolens-mcp` `memory_test.rs` or app summary
tests):
- `get_summary_persists_confirmed_for_resolving_insight` — memorize an
  anchored-to-present memory, call `get_summary` (compact), reload
  `memory.json` → surfaced insight carries `last_grounding == Confirmed` +
  `last_grounded_at`.
- `get_summary_full_tier_persists_contradicted` — a `Contradicted`
  (anchored-to-missing) memory is surfaced only at the **full** tier
  (compact/standard filter it out per `summary.rs:249`), so test the
  contradicted-stamp path at full tier; reload → `last_grounding == Contradicted`.
- `get_summary_full_tier_stamps_only_selected_20` — > 20 memories present →
  full tier stamps exactly the 20 surfaced insights, leaving the rest's
  `last_grounding` untouched (proves selected-only, not load-all).
- `get_summary_skip_tier_writes_no_grounding` — `Skip` tier → graph snapshot
  unchanged on disk.

**Scan-time:**
- Unit coverage is carried by the pure `ground_all_memories` helper tests.
- A CLI-path test if a `repolens-cli` integration harness exists; otherwise the
  scan-time wiring is verified by the dogfood below (the wiring is a thin
  best-effort `PackReader::read` → `MemoryStore::load` → `ground_all_memories`
  → `save`).
- **Incremental-deletion grounding** (dogfood + a test if a harness exists):
  memory anchored to a file, full scan → `Confirmed`; delete the file,
  incremental scan → the scan-time refresh re-grounds it to `Contradicted`.
  This relies on the incremental pack omitting `FileEntry` rows for deleted
  paths (`incremental.rs:307`/`355` remove deleted paths from the merged
  entries) — call that dependency out so the test isn't mistaken for testing
  grounding alone.

## Dogfood

1. `repolens scan --root <tmp>` a small repo; memorize an insight anchored to a
   **non-existent** path (e.g. `src/deleted.rs`) via `memorize_insight`.
2. `repolens scan --root <tmp>` again **without any recall** → reload
   `memory.json` → confirm the node now shows `lastGrounding: CONTRADICTED` +
   `lastGroundedAt` (proving scan-time refresh works without a recall).
3. Then trigger a decay (recall or aged `lastDecayRun`) → confirm importance
   drops (the Contradicted multiplier), as in the PR #70 dogfood.
4. Separately: `get_summary` on a repo with a contradicted memory → reload →
   confirm the surfaced insight's snapshot is persisted.

## Closeout

- **doc 07** (`docs/philosophy/07-current-state-vs-direction.md`, doc-canon
  Living-doc content edit; run `scripts/verify-doc-canon.sh`):
  - Row 25 (grounding on recall) / row 26 (memory decay): note that
    `record_grounding` is now also invoked **on scan** (all memories, CLI
    `repolens scan`) and **on get_summary** (selected insights), not only on
    recall. Reword the "stale-until-recall" caveat to "refreshes on scan,
    get_summary, or recall; a memory neither scanned-against nor read still
    carries `None`/stale until one of those occurs."
  - Known Mismatch #3: update the on-recall-only caveat accordingly.
- **CLAUDE.md** (optional, not doc-canon): one line that `repolens scan`
  refreshes memory grounding snapshots.
- Pre-push gate: `cargo fmt --all && cargo clippy --workspace --all-targets --
  -D warnings && cargo test --workspace`.

## Open questions

None. Resolved by brainstorming (2026-05-31): scan writes grounding snapshot
only (no decay); scan refresh lives in the CLI `run_scan` (ScanService stays
pure); get_summary stamps selected insights only; shared pure helper in
`grounding.rs`.
