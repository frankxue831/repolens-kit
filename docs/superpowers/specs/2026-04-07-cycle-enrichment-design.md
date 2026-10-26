# Cycle Enrichment Design

## Context

`find_cycles` landed in PR #4 and both summary generation and the MCP `analyze_health` tool already run cycle detection. The current integration is still shallow:

- The `dependency-cycles` health signal is always severity `high`, regardless of cycle count or blast radius.
- Recommendations do not react to cycles, so `get_summary` can surface a serious health signal without a matching action item.
- `FitnessScorer` ignores cycles, so the overall quality score understates cyclic coupling.
- Cycle-derived metrics are assembled independently in `SummaryService` and `analyze_health`, which risks drift.
- There is no structured cycle metric in `HealthReport`, so downstream consumers only get cycle information through prose.

This design enriches cycle data across health signals, recommendations, and fitness scoring while keeping the public contract changes narrow and single-sourced.

## Review Corrections

The first draft had three design problems that this revision fixes:

1. It duplicated `files_in_cycles_count` computation in `repolens-app` and `repolens-mcp` instead of introducing one shared graph-health builder.
2. It claimed to make cycle metrics available to downstream consumers, but only added them to free-form detail strings.
3. It did not spell out how `SummaryService` would compute graph-health once and reuse it across `HealthAnalyzer`, `RecommendationBuilder`, and `FitnessScorer`.

## Design

### 1. Expand `GraphHealthInput`

**File:** `crates/repolens-core/src/model/health.rs`

Add one field:

```rust
pub struct GraphHealthInput {
    pub cycle_count: usize,
    pub largest_cycle_size: usize,
    pub files_in_cycles_count: usize,
    pub orphan_count: usize,
    pub orphan_files: Vec<String>,
    pub test_to_source_ratio: f64,
}
```

`files_in_cycles_count` is the number of distinct repo-relative file paths that appear in any strongly connected component with size >= 2.

### 2. Expose graph health structurally in `HealthReport`

**File:** `crates/repolens-core/src/model/health.rs`

Add an optional field:

```rust
pub struct HealthReport {
    pub structure: Structure,
    pub dependency_risk: DependencyRisk,
    pub signals: Vec<Signal>,
    pub graph_health: Option<GraphHealthInput>,
}
```

Rationale:

- This makes `cycleCount`, `largestCycleSize`, and `filesInCyclesCount` available as stable JSON fields instead of forcing MCP consumers to parse prose.
- It avoids adding a new top-level struct to `RepoSummary`; `health` already exists there and is the right place for graph-derived health metrics.
- The field remains `None` when graph analysis data is unavailable.

### 3. Single-source graph-health assembly

**New file:** `crates/repolens-app/src/graph_health.rs`

Introduce a shared helper:

```rust
pub fn build_graph_health_input(
    index: &PackIndex,
    profile: &ProjectProfile,
    graph: &FileGraph,
) -> GraphHealthInput
```

Responsibilities:

- run `GraphAnalyzer::find_cycles(graph)`
- compute `files_in_cycles_count` by collecting all cycle member files into a `HashSet`
- derive `largest_cycle_size` from the first sorted cycle, defaulting to `0`
- run `GraphAnalyzer::find_orphans(graph, &entry_point_files)`
- compute `test_to_source_ratio` via the existing shared logic
- return a fully-populated `GraphHealthInput`

Rationale:

- `SummaryService` and MCP `analyze_health` already both depend on `repolens-app`, so this is the narrowest place to centralize the metric assembly.
- This keeps `GraphHealthInput` construction single-sourced instead of repeating the same cycle/orphan/test plumbing in multiple call sites.

### 4. Tier `dependency-cycles` severity

**File:** `crates/repolens-profile/src/health.rs`

Add a shared helper:

```rust
pub fn cycle_severity(cycle_count: usize, largest_cycle_size: usize) -> &'static str
```

Rules, evaluated top-down:

| Condition | Severity |
|-----------|----------|
| `cycle_count >= 6` OR `largest_cycle_size > 6` | `high` |
| `cycle_count >= 3` OR `largest_cycle_size > 3` | `medium` |
| otherwise (`cycle_count > 0`) | `low` |

Update the `dependency-cycles` signal to use that helper and expand the detail string:

> `Found {cycle_count} dependency cycle(s) involving {files_in_cycles_count} files; largest cycle has {largest_cycle_size} files. Circular dependencies make code harder to reason about, test, and refactor.`

### 5. Carry `graph_health` through `HealthAnalyzer`

**File:** `crates/repolens-profile/src/health.rs`

When `HealthAnalyzer::analyze(..., graph_health)` is called:

- continue using `graph_health` to emit graph-derived signals
- set `HealthReport.graph_health = graph_health.cloned()`

This keeps signal generation and the structured metrics sourced from the same input.

### 6. Add a cycle-aware recommendation

**File:** `crates/repolens-app/src/summary.rs`

Change:

```rust
RecommendationBuilder::build(
    scan,
    profile,
    degree_map,
    index,
    graph_metrics,
    graph_health,
)
```

Add a new rule after existing Rule 6:

```text
Rule 7: dependency-cycles
Condition: graph_health.cycle_count > 0
id: "dependency-cycles"
severity: cycle_severity(cycle_count, largest_cycle_size)
category: "architecture"
title: "Dependency cycles detected"
detail: "Found {cycle_count} cycle(s) involving {files_in_cycles_count} files; largest has {largest_cycle_size} files."
suggested_action: "Use the find_cycles tool for full details. Break cycles by extracting shared interfaces or inverting dependencies."
file: None
confidence: None
```

This aligns `get_summary` recommendations with the `analyze_health` signal instead of leaving cycle findings stranded in health output.

### 7. Factor cycles into fitness scoring

**File:** `crates/repolens-app/src/summary.rs`

Change:

```rust
FitnessScorer::score_with_graph(metrics, scan, degree_map, graph_health)
```

Apply a bounded cycle penalty to `coupling_quality`:

```rust
let cycle_penalty = if node_count > 0 {
    (files_in_cycles_count as f64 / node_count as f64).min(0.3)
} else {
    0.0
};

let coupling_quality = (base_coupling_quality - cycle_penalty).clamp(0.0, 1.0);
```

Notes:

- `node_count` remains `degree_map.len()`, which already mirrors graph node count because `compute_degree_map()` seeds every node.
- The `0.3` cap keeps cycles meaningful without letting them dominate the full score. At the current 0.3 coupling weight, the max overall score impact is `0.09`.

### 8. Summary and MCP plumbing

**Files:**

- `crates/repolens-app/src/summary.rs`
- `crates/repolens-app/src/lib.rs`
- `crates/repolens-mcp/src/tools/profile.rs`

Changes:

- export the new `graph_health` module from `repolens-app`
- in `SummaryService::summarize()`, compute `graph_health` once from the graph, then reuse it for:
  - `HealthAnalyzer::analyze()`
  - `RecommendationBuilder::build()`
  - `FitnessScorer::score_with_graph()`
- in MCP `handle_analyze_health()`, replace the inline cycle/orphan/test assembly with `build_graph_health_input()`

This removes the existing drift risk between summary generation and the MCP health tool.

## Files to Modify

| File | Change |
|------|--------|
| `crates/repolens-core/src/model/health.rs` | Add `files_in_cycles_count` to `GraphHealthInput`; add optional `graph_health` to `HealthReport` |
| `crates/repolens-profile/src/health.rs` | Add `cycle_severity()`, tier the cycle signal, clone `graph_health` into `HealthReport` |
| `crates/repolens-app/src/graph_health.rs` | New shared builder for `GraphHealthInput` |
| `crates/repolens-app/src/lib.rs` | Export `graph_health` module |
| `crates/repolens-app/src/summary.rs` | Reuse `graph_health` in health, recommendations, and fitness scoring |
| `crates/repolens-mcp/src/tools/profile.rs` | Replace inline assembly with shared `build_graph_health_input()` helper |

## Testing

1. `cargo test -p repolens-core`
   - `GraphHealthInput` serde round-trip covers `files_in_cycles_count`
   - `HealthReport` serde round-trip covers optional `graph_health`
2. `cargo test -p repolens-profile`
   - cycle severity unit tests for `low`, `medium`, and `high`
   - `dependency-cycles` signal detail includes `files_in_cycles_count`
   - `HealthReport.graph_health` is populated when input is present
3. `cargo test -p repolens-app`
   - `build_graph_health_input()` computes distinct cycle file counts correctly
   - `RecommendationBuilder` emits `dependency-cycles`
   - `FitnessScorer` applies the bounded cycle penalty
4. `cargo test -p repolens-mcp`
   - `analyze_health` response includes structured `graphHealth`
   - cycle counts and `filesInCyclesCount` match the cycle fixture
5. `cargo fmt --all -- --check`
6. `cargo clippy --workspace --all-targets -- -D warnings`
7. `cargo test`
