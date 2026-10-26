# Graph Health Signals — Design Spec

**Date**: 2026-04-06
**Status**: Revised
**Scope**: Phase 1 of post-competitor-research roadmap

## Overview

Add three graph-derived health signals to RepoLens-RS: cycle detection, orphan file detection, and test coverage heuristics. Expose cycle detection as a new `find_cycles` MCP tool. Enhance `analyze_health` with new graph-aware signals.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Cycle granularity | File-level SCCs | Matches existing `FileGraph` model; symbol-level deferred to v2 |
| Edge scope for cycles | Structural only (`imports`, `extends`, `implements`, `type_ref`, `declares`) | `calls` edges are projected from method names to files — too noisy for dependency-cycle signals |
| Dead code approach | `orphan-files` (zero inbound structural edges) | Conservative; avoids false "dead code" claims. Entry-point-rooted reachability deferred to v2 |
| Test coverage metric | Global test-to-source ratio | Already computed in summary pipeline; per-module deferred until module boundaries improve |
| Test ratio reuse | Reuse existing summary computation via shared helper | Avoid duplicate logic between summary building and `analyze_health` |
| Architecture | New `graph_analysis` module in `repolens-app` | Graph analysis next to graph building; avoids bloating `repolens-profile` with petgraph |
| Cycle output shape | All internal structural edges per SCC | More useful than a single ring; avoids inventing a canonical cycle path |
| Self-loops | Excluded in v1 | Keep the signal focused on multi-file dependency cycles |

## 1. Core Graph Analysis Module

**New file**: `crates/repolens-app/src/graph_analysis.rs`
**New dependency**: `petgraph` added to `crates/repolens-app/Cargo.toml`

### Public API

```rust
pub struct GraphAnalyzer;

pub struct CycleReport {
    pub cycles: Vec<Scc>,
    pub total_cycles: usize,
}

pub struct Scc {
    pub files: Vec<String>,       // sorted file paths in this SCC
    pub edges: Vec<GraphEdge>,    // all structural edges internal to the SCC, sorted deterministically
    pub size: usize,
}

pub struct OrphanReport {
    pub orphan_files: Vec<String>,
    pub total_orphans: usize,
    pub total_files: usize,
}

const STRUCTURAL_EDGE_TYPES: &[&str] = &[
    "imports", "extends", "implements", "type_ref", "declares"
];

impl GraphAnalyzer {
    pub fn find_cycles(graph: &FileGraph) -> CycleReport;
    pub fn find_orphans(
        graph: &FileGraph,
        entry_point_files: &HashSet<String>,
    ) -> OrphanReport;
}
```

### Algorithms

**`find_cycles`**:
1. Filter `graph.edges` to structural types only.
2. Build `petgraph::DiGraph` — nodes are file path strings, edges are filtered structural edges.
3. Run `petgraph::algo::tarjan_scc()`.
4. Filter to SCCs with size >= 2 (exclude trivial single-node components).
5. For each SCC, collect all structural edges where both source and target are in the SCC.
6. Sort SCCs by size descending, then by first file path alphabetically for stable ties.
7. Sort edges within each SCC by `(source, target, edge_type, detail)` for deterministic tool output and tests.

**`find_orphans`**:
1. Build inbound-edge set from structural edges only.
2. For each node in `graph.nodes`, if zero inbound structural edges and not in `entry_point_files`, it's an orphan.
3. Sort alphabetically.

## 2. Health Signal Integration

**Modified file**: `crates/repolens-profile/src/health.rs`

### Data Bridge

New struct in `crates/repolens-core/src/model/health.rs` to pass pre-computed results without introducing a dependency from `repolens-profile` to `repolens-app`:

```rust
pub struct GraphHealthInput {
    pub cycle_count: usize,
    pub largest_cycle_size: usize,
    pub orphan_count: usize,
    pub orphan_files: Vec<String>,   // capped at ~10 for readability
    pub test_to_source_ratio: f64,
}
```

### Updated Signature

```rust
pub fn analyze(
    index: &PackIndex,
    profile: &ProjectProfile,
    graph: &FileGraph,
    graph_health: Option<&GraphHealthInput>,
) -> HealthReport;
```

`None` preserves backward compatibility for callers that don't have graph analysis results.

### New Signals

| Signal ID | Severity | Trigger | Detail |
|-----------|----------|---------|--------|
| `dependency-cycles` | high | `cycle_count > 0` | "Found {n} dependency cycle(s); largest involves {m} files. Circular dependencies make code harder to reason about, test, and refactor." |
| `orphan-files` | low | `orphan_count > 0` AND orphan ratio > 5% of total files | "Found {n} file(s) not referenced by other scanned files: {list}. These may be entry points, unused code, or integration seams." |
| `low-test-ratio` | medium if ratio < 0.05, low if < 0.1 | `test_to_source_ratio < 0.1` | "Test-to-source file ratio is {ratio:.0%}. Consider increasing test coverage." |

Notes:
- `orphan-files` 5% threshold avoids noise on small repos.
- Orphan ratio is computed against `graph.nodes.len()` because orphan detection is defined over the persisted file graph.
- `low-test-ratio` reuses the existing summary computation, surfaced as a health signal rather than inventing a second metric.
- `dependency-cycles` is high severity — cycles are structural problems worth flagging prominently.
- The existing summary recommendation `low-test-ratio` remains unchanged; this work mirrors that signal into the health surface as well.

## 3. MCP Tool — `find_cycles`

**Location**: New handler in `crates/repolens-mcp/src/tools/graph.rs`

### Parameters

```rust
pub struct FindCyclesParams {
    pub pack: Option<String>,
    pub root: Option<String>,
    pub file: Option<String>,  // repo-relative path; filters SCCs post-computation
}
```

### Response Shape

```json
{
  "cycles": [
    {
      "files": ["crates/a/src/lib.rs", "crates/b/src/lib.rs", "crates/c/src/lib.rs"],
      "edges": [
        {"source": "crates/a/src/lib.rs", "target": "crates/b/src/lib.rs", "type": "imports", "detail": "crate_b::Foo"},
        {"source": "crates/b/src/lib.rs", "target": "crates/c/src/lib.rs", "type": "extends", "detail": null},
        {"source": "crates/c/src/lib.rs", "target": "crates/a/src/lib.rs", "type": "type_ref", "detail": "crate_a::Bar"}
      ],
      "size": 3
    }
  ],
  "totalCycles": 1
}
```

### Semantics

- Always runs cycle detection on the full structural graph.
- If `file` is provided, returns only SCCs containing that file. If the file is not in any cycle, returns `cycles: []`, `totalCycles: 0`.
- `totalCycles` reports the number of SCCs returned after optional filtering.
- Self-loops are not returned in v1, even if a structural self-edge exists.
- No `depth` parameter — not meaningful for cycle detection.

### Tool Description

> "Detect dependency cycles (strongly connected components) in the file graph. Returns cycles with all internal structural edges. Optionally filter to cycles involving a specific file."

### Registration

Add `#[tool(name = "find_cycles")]` to the router in `lib.rs`.

## 4. Integration Points

Health analysis is computed **on-demand**, not during the scan pipeline. There are two health-analysis call sites, plus one separate cycle-inspection tool:

No pack format, manifest schema, or persisted summary schema changes are required for this phase.

### 4a. MCP `analyze_health` tool (`crates/repolens-mcp/src/tools/profile.rs`)

Currently loads pack index, runs `ProfileAnalyzer::analyze`, reads `graph.json`, then calls `HealthAnalyzer::analyze(&index, &profile, &graph)`.

**Updated flow**:
1. Load pack index, profile, graph (unchanged)
2. **New**: `GraphAnalyzer::find_cycles(&graph)` + `GraphAnalyzer::find_orphans(&graph, &entry_points)`
3. **New**: Reuse the existing scan-summary test ratio computation via a shared helper instead of re-implementing it locally
4. Build `GraphHealthInput` from results
5. `HealthAnalyzer::analyze(&index, &profile, &graph, Some(&graph_health))`

Entry-point files come from `profile.entry_points` — extract the `path` field from each `EntryPoint` into a `HashSet<String>`.

### 4b. Summary builder (`crates/repolens-app/src/summary.rs`)

Currently calls `HealthAnalyzer::analyze(&index, &profile, g)` and already computes the test-to-source ratio in the scan summary path.

**Updated flow**:
1. After loading graph (unchanged), run `GraphAnalyzer::find_cycles` + `find_orphans`
2. Build `GraphHealthInput` using the already-computed test ratio from the shared scan-summary helper
3. Pass to updated `HealthAnalyzer::analyze` call

### 4c. MCP `find_cycles` tool (new)

Calls `GraphAnalyzer::find_cycles` directly. Does **not** go through `HealthAnalyzer`. See Section 3.

### Performance

Tarjan's is O(V+E), orphan detection is O(V+E). Both negligible relative to pack I/O. On-demand computation means no scan-time cost unless the tools are actually called.

### Shared Helper Reuse

To keep the test-ratio metric single-sourced, extract the existing scan-summary computation into a shared helper that both `SummaryService` and MCP `analyze_health` can call. The helper may remain in `repolens-app::summary`; it does not need a new crate or a new persisted artifact.

## 5. Testing Strategy

### Unit Tests (`crates/repolens-app/src/graph_analysis.rs`)

- `test_no_cycles_in_dag` — acyclic graph returns empty `CycleReport`
- `test_simple_two_node_cycle` — A<->B returns one SCC of size 2 with correct edges
- `test_multiple_sccs` — graph with two independent cycles returns both, sorted by size desc
- `test_calls_edges_excluded` — graph with cycle only via `calls` edges returns no cycles
- `test_self_loop_excluded` — single node with self-edge not reported (size < 2)
- `test_orphan_detection` — nodes with no inbound structural edges reported
- `test_orphans_exclude_entry_points` — entry-point files not flagged as orphans
- `test_orphans_ignore_calls_edges` — file with only inbound `calls` edge is still orphan

### Integration Tests (`crates/repolens-profile/`)

- `test_health_with_cycles_signal` — verify `dependency-cycles` signal emitted when cycles present
- `test_health_without_graph_input` — `graph_health: None` produces no graph signals (backward compat)
- `test_health_orphan_signal_threshold` — orphan signal only fires when orphan ratio > 5%
- `test_health_low_test_ratio_signal` — verify signal at different ratio thresholds

### MCP Tool Tests (`crates/repolens-mcp/`)

- `test_find_cycles_full_graph` — end-to-end: load fixture pack, call tool, verify response shape
- `test_find_cycles_file_filter` — filter to specific file, verify only matching SCCs returned
- `test_find_cycles_no_cycles` — DAG returns `totalCycles: 0`

### Fixture

Add `fixtures/scenarios/cyclic-graph/` with a hand-crafted `graph.json` containing known cycles for deterministic testing.

## Out of Scope (v2+)

- Symbol-level cycle detection (requires a real symbol/call graph)
- Entry-point-rooted reachability analysis (depends on stronger root detection)
- Per-module test coverage (depends on improved module boundary detection)
- `calls` edge inclusion in structural analysis
- Cycle output capping / pagination for very large graphs
