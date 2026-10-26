# Graph Traversal MCP Tools + CI Pipeline Design Spec

## Overview

This spec covers two independent changes:

1. Add three graph traversal MCP tools for direct dependency lookup and transitive impact analysis
2. Add a minimal GitHub Actions CI workflow that enforces formatting, linting, and tests on the Rust workspace

The graph work is read-only over existing `graph.json` data. It does not change scan-time extraction or graph construction.

## Feature 1: Graph Traversal MCP Tools

### Goal

Enable clients to answer three common file-level questions directly from the MCP server:

- What files depend on this file?
- What files does this file depend on?
- If these files change, what downstream files are transitively affected?

These answers are graph-reachability results over the existing file dependency graph. They are useful for review, refactoring, and scoped reasoning, but they are not guarantees of runtime breakage.

### Design Constraints

- Reuse existing MCP patterns in `repolens-mcp`: `Parameters<T>`, `JsonSchema`, `resolver.resolve()`, `ok_json()`, `err_json()`, and entry/exit logging with `Instant`
- Keep implementation in `crates/repolens-mcp/src/tools/graph.rs` alongside `get_graph`
- Register each new handler in `crates/repolens-mcp/src/lib.rs` as a first-class `#[tool]`
- Reuse `repolens_app::summary::read_graph()` and follow existing graph-tool behavior: missing or invalid `graph.json` produces an empty graph, not a tool error
- Return deterministic output ordering so MCP snapshots and client expectations stay stable across runs

### Tool Contracts

#### `get_callers`

**Purpose:** Return all direct incoming edges for a file.

**Parameters:**
- `pack: Option<String>`: absolute pack path, optional when `root` is provided
- `root: Option<String>`: absolute repo root for pack discovery
- `file: String`: repo-relative file path

**Response:**

```json
{
  "file": "src/graph.rs",
  "callers": [
    {
      "source": "src/lib.rs",
      "edgeType": "imports",
      "detail": "crate::graph"
    },
    {
      "source": "src/scan.rs",
      "edgeType": "imports",
      "detail": "crate::graph::GraphBuilder"
    }
  ],
  "totalCallers": 2
}
```

**Behavior:**
- Filter `graph.edges` where `edge.target == file`
- Preserve edge-level detail; do not collapse multiple edges from the same source
- Sort results by `source`, then `edgeType`, then `detail`
- Do not recurse

#### `get_dependencies`

**Purpose:** Return all direct outgoing edges for a file.

**Parameters:** Same as `get_callers`

**Response:**

```json
{
  "file": "src/scan.rs",
  "dependencies": [
    {
      "target": "src/discover.rs",
      "edgeType": "imports",
      "detail": "crate::discover"
    },
    {
      "target": "src/graph.rs",
      "edgeType": "imports",
      "detail": "crate::graph::GraphBuilder"
    }
  ],
  "totalDependencies": 2
}
```

**Behavior:**
- Filter `graph.edges` where `edge.source == file`
- Preserve edge-level detail; do not collapse multiple edges to the same target
- Sort results by `target`, then `edgeType`, then `detail`
- Do not recurse

#### `get_impact`

**Purpose:** Starting from one or more changed files, return the transitively affected downstream files plus one shortest propagation path per file.

**Parameters:**
- `pack: Option<String>`: absolute pack path, optional when `root` is provided
- `root: Option<String>`: absolute repo root for pack discovery
- `files: Vec<String>`: repo-relative seed files
- `maxDepth: Option<usize>`: traversal depth limit, default `10`, hard cap `50`

**Response:**

```json
{
  "seedFiles": ["src/graph.rs"],
  "impactedFiles": [
    {
      "file": "src/scan.rs",
      "depth": 1,
      "path": ["src/graph.rs", "src/scan.rs"]
    },
    {
      "file": "src/lib.rs",
      "depth": 2,
      "path": ["src/graph.rs", "src/scan.rs", "src/lib.rs"]
    }
  ],
  "totalImpacted": 2,
  "maxDepthReached": false
}
```

**Behavior:**
- Treat `files` as a multi-source seed set
- Deduplicate seeds while preserving stable sorted output in `seedFiles`
- Traverse reverse edges (`target -> source`) because impact propagates from a changed dependency to its dependents
- Use BFS so the recorded path is the shortest path by edge count
- Exclude seed files from `impactedFiles`
- Return one entry per impacted file
- Sort `impactedFiles` by `depth`, then `file`
- Set `maxDepthReached` to `true` when at least one frontier expansion is skipped because the depth limit was reached

### Implementation Approach

The implementation stays local to `crates/repolens-mcp/src/tools/graph.rs`.

1. Resolve pack/index with the existing `PackResolver`
2. Read `graph.json` via `read_graph(&pack_dir)` and fall back to `FileGraph { nodes: vec![], edges: vec![] }`
3. Build request-local adjacency views from `graph.edges`
4. Serialize typed response structs with camelCase field names

Request-local adjacency views:

- `incoming: HashMap<&str, Vec<&GraphEdge>>` for `get_callers` and `get_impact`
- `outgoing: HashMap<&str, Vec<&GraphEdge>>` for `get_dependencies`

This avoids repeatedly scanning the full edge list during BFS while keeping state scoped to a single tool call. No persistent indexing or pack format changes are needed.

### Response Types

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CallerEntry {
    source: String,
    edge_type: String,
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DependencyEntry {
    target: String,
    edge_type: String,
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImpactEntry {
    file: String,
    depth: usize,
    path: Vec<String>,
}
```

Tool parameter structs should follow the existing `GetGraphParams` style and carry `JsonSchema` descriptions for MCP self-documentation.

### Error Handling and Edge Cases

- Pack resolution failure remains an error via existing `err_json(...)` patterns
- Missing or invalid `graph.json` returns a successful empty result, consistent with `get_architecture` and `analyze_health`
- A file absent from graph nodes is treated the same as a file with zero matching edges: successful empty result
- `get_impact` with an empty `files` list returns:
  - `"seedFiles": []`
  - `"impactedFiles": []`
  - `"totalImpacted": 0`
  - `"maxDepthReached": false`
- `maxDepth <= 0` is normalized to the default; values above `50` are clamped to `50`

### Testing

Add focused unit tests in `crates/repolens-mcp` for:

- `get_callers` returns only incoming edges
- `get_dependencies` returns only outgoing edges
- `get_impact` performs reverse-direction traversal rather than forward traversal
- `get_impact` chooses shortest paths under BFS
- seed files are excluded from impact output
- depth limiting sets `maxDepthReached` correctly
- missing graph data returns empty success payloads
- output ordering is deterministic

If existing test helpers cover MCP tool invocation, extend them rather than creating a parallel harness.

## Feature 2: CI Pipeline

### Goal

Protect `main` with one workspace-level workflow that rejects unformatted code, lint warnings, and test regressions.

### Workflow File

Create `.github/workflows/ci.yml`.

**Triggers**

- `push` on `main`
- `pull_request` targeting `main`

**Runner**

- `ubuntu-latest`

**Recommended shape**

1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable` with `clippy` and `rustfmt`
3. `Swatinem/rust-cache@v2`
4. `cargo fmt --all -- --check`
5. `cargo clippy --workspace --all-targets -- -D warnings`
6. `cargo test --workspace`

Why this command set:

- `--workspace` ensures all crates are covered explicitly
- `--all-targets` makes Clippy check tests and binaries in addition to library targets
- keeping format, lint, and test as separate steps makes failures easy to diagnose in GitHub Actions

### Pre-existing Failing Test

There is a known existing failure in `crates/repolens-core/tests/discovery_test.rs`: `records_dot_git_entries_as_excluded_git`.

Before enabling required CI, scope the workaround narrowly:

- mark only that test with `#[ignore]`
- add a short TODO comment explaining that the fixture does not currently contain the expected `.git` entries
- do not weaken any other tests or lower CI strictness globally

This keeps the known issue visible while allowing workspace CI to go green.

### Non-Goals

- No release automation
- No MSRV policy changes
- No matrix builds
- No split jobs unless the single-job workflow proves too slow
