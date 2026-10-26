# Grounded Memory (Path B Foundation)

**Date:** 2026-04-05
**Status:** Revised draft
**Scope:** Memory-model preparation + grounding-on-recall (opinion pool steps 1-2)

## Context

The opinion pool (`docs/opinion_pool/`) reached consensus on Path B: validate memory against observations to prevent sticky falsehoods. This spec covers the foundation only: prepare the memory model for grounding edges and add anchor-based grounding to `recall_longterm`.

Out of scope for this phase: high-stakes on-scan grounding, configurable decay for contradicted nodes, absence-as-signal heuristics, truth-layer surfacing across all tools, and typed/structured anchor formats. Those can layer on once the basic recall-time grounding loop proves useful.

## Decision Summary

| Question | Decision | Rationale |
|----------|----------|-----------|
| Scope | Foundation only | Prove the mechanism before layering policy and heuristics |
| Grounding evidence source | `PackIndex` only in this phase | File entries already provide file-path and symbol observations; no extra `graph.json` I/O is needed |
| Groundable anchor forms | Exact file paths and exact qualified symbols only | Deterministic and low-risk; avoids false positives from simple-name collisions |
| Contradiction detection | Anchor-based only | Keeps the signal inspectable and trustworthy |
| Architecture | Grounding logic in `repolens-app`, orchestration in MCP | `repolens-app` owns pack-aware logic; MCP owns tool wiring and response shaping |
| Response serialization | Typed serde structs, not `format!("{:?}")` | Preserves `SCREAMING_SNAKE_CASE` enum wire values such as `TRADE_OFF` and `CONFIRMED` |

## Schema / Model Changes

### New edge types in `MemoryRelation`

File: `crates/repolens-core/src/model/memory.rs`

Add two variants to the existing enum:

```rust
pub enum MemoryRelation {
    Supersedes,
    RelatesTo,
    Contradicts,
    GroundedBy,      // memory node -> evidence that confirms it
    ContradictedBy,  // memory node -> evidence that refutes it
}
```

These variants are added now for model completeness, but this phase does not persist grounding edges. Recall-time grounding is computed on demand only.

### New `GroundingStatus` enum

File: `crates/repolens-core/src/model/memory.rs`

```rust
pub enum GroundingStatus {
    Confirmed,
    Contradicted,
    Unverifiable,
    Unchecked,
}
```

Serialized as `SCREAMING_SNAKE_CASE` strings (`"CONFIRMED"`, `"CONTRADICTED"`, `"UNVERIFIABLE"`, `"UNCHECKED"`), matching the existing enum convention in this crate.

Semantics for this phase:

- `Confirmed`: every groundable anchor matched the active pack
- `Contradicted`: at least one groundable anchor failed to match the active pack
- `Unverifiable`: the node has groundable anchors, but no active/latest pack could be resolved
- `Unchecked`: the node has no anchors, or no anchors in a form this phase can ground

That `Unchecked` definition is intentionally broader than "no anchors". It prevents free-form or legacy anchor strings from being mislabeled as contradictions before typed anchors exist.

## Grounding Logic

### New module: `crates/repolens-app/src/grounding.rs`

#### `GroundingAssessment` struct

```rust
pub struct GroundingAssessment {
    pub grounding_status: GroundingStatus,
    pub missing_anchors: Vec<String>,
    pub found_anchors: Vec<String>,
}
```

This stays separate from MCP response structs. The app layer computes grounding; the MCP layer decides how to present a recalled memory plus its score and grounding metadata.

#### `assess_grounding` function

```rust
pub fn assess_grounding(
    node: &MemoryNode,
    pack: Option<&PackIndex>,
) -> GroundingAssessment
```

Algorithm per node:

1. Partition `node.anchors` into:
   - groundable file-path anchors: exact match against `FileEntry.path`
   - groundable qualified-symbol anchors: exact match against `FileEntry.symbols[].qualified_name`
   - unsupported anchors: everything else
2. If there are no groundable anchors, return `Unchecked`
3. If `pack` is `None`, return `Unverifiable`
4. For each groundable anchor:
   - record it in `found_anchors` on an exact match
   - otherwise record it in `missing_anchors`
5. Derive status:
   - all groundable anchors found -> `Confirmed`
   - any groundable anchor missing -> `Contradicted`

Properties:

- Pure function: no disk I/O, no graph mutation, no session mutation
- Exact-match only in foundation
- No simple-symbol grounding in this phase
- No `graph.json` dependency in this phase

Rationale for skipping simple-symbol grounding: matching `save`, `Config`, or `Handler` by bare name across a repo is not trustworthy. If we want that later, it should come with typed anchors or explicit ambiguity handling.

## Integration into `recall_longterm`

### Real integration point

The current handler lives in `crates/repolens-mcp/src/tools/memory.rs`, not in `crates/repolens-mcp/src/lib.rs`. The tool method in `lib.rs` is just the routing layer.

This change should update both files:

- `RepoLensMcpServer::recall_longterm` in `crates/repolens-mcp/src/lib.rs` passes `&self.resolver` into the handler
- `handle_recall_longterm` in `crates/repolens-mcp/src/tools/memory.rs` resolves the active/latest pack from `params.root` and applies grounding to returned nodes

Suggested handler signature:

```rust
pub fn handle_recall_longterm(
    resolver: &PackResolver,
    params: RecallLongtermParams,
) -> Result<CallToolResult, McpError>
```

### Modified flow

Current flow in `handle_recall_longterm`:

1. Load memory graph
2. Lazy decay (if needed)
3. Filter + score nodes
4. Sort, limit, touch returned nodes
5. Return results

Revised flow:

1. Load memory graph
2. Lazy decay (if needed)
3. Filter + score nodes
4. Sort and take top `limit`
5. Resolve the active/latest pack via `resolver.resolve(None, Some(&params.root))`
6. For each returned node, call `assess_grounding`
7. Touch returned nodes
8. Return results

Grounding should run only on the nodes that will actually be returned, not on every scored candidate.

### Response shape

Per-result additions:

```json
{
  "id": "...",
  "content": "...",
  "type": "DECISION",
  "importance": 0.7,
  "score": 0.85,
  "tags": ["auth"],
  "anchors": ["src/auth.rs"],
  "groundingStatus": "CONFIRMED",
  "missingAnchors": [],
  "foundAnchors": ["src/auth.rs"]
}
```

Top-level response remains minimal:

```json
{
  "results": [...],
  "total": 5,
  "decayRan": true
}
```

Do not add a `groundingRan` flag in this phase. If grounding is wired into `recall_longterm`, that flag will almost always be `true` and adds little value. The meaningful signal is already carried per result.

### Serialization guidance

Avoid building enum strings with `format!("{:?}")`. That is already fragile in the current recall response because `MemoryType::TradeOff` would become `"TRADEOFF"` instead of `"TRADE_OFF"`.

Use typed response structs with serde:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallResult {
    id: String,
    content: String,
    #[serde(rename = "type")]
    memory_type: MemoryType,
    importance: f32,
    score: f64,
    tags: Vec<String>,
    anchors: Vec<String>,
    grounding_status: GroundingStatus,
    missing_anchors: Vec<String>,
    found_anchors: Vec<String>,
}
```

That keeps `type`, `groundingStatus`, and future enums aligned with the crate's existing serde rules.

### Error handling

- No active/latest pack found -> grounding runs with `pack: None` -> nodes with groundable anchors become `Unverifiable`
- Memory nodes with only unsupported anchors -> `Unchecked`
- Missing or corrupt memory file behavior is unchanged: `MemoryStore::load` already returns an empty graph

This phase should not fail `recall_longterm` solely because grounding could not run.

## Testing

### Unit tests: `crates/repolens-app/src/grounding.rs`

| Test case | Input | Expected |
|-----------|-------|----------|
| File path anchor found | Node anchored to existing file path | `Confirmed` |
| Qualified symbol anchor found | Node anchored to existing qualified symbol | `Confirmed` |
| Missing file path anchor | Node anchored to nonexistent file | `Contradicted` |
| Missing qualified symbol anchor | Node anchored to nonexistent qualified symbol | `Contradicted` |
| No anchors | Empty anchors vec | `Unchecked` |
| Unsupported anchor format | Anchor like `"AuthService"` only | `Unchecked` |
| No pack | Groundable anchor present, `pack: None` | `Unverifiable` |
| Mixed found + missing groundable anchors | One hit, one miss | `Contradicted` |

All unit tests should use in-memory `PackIndex` values. No disk I/O and no fixtures are required.

### Integration tests: `crates/repolens-mcp/tests/memory_test.rs`

Extend the existing memory-tool tests:

| Test case | Setup | Assert |
|-----------|-------|--------|
| Recall with grounded file anchor | Memory anchored to file path present in active/latest pack | `groundingStatus == "CONFIRMED"` |
| Recall with stale file anchor | Memory anchored to missing file path | `groundingStatus == "CONTRADICTED"` and `missingAnchors` populated |
| Recall without pack | Call recall in repo with memory but no pack | groundable anchors return `"UNVERIFIABLE"` |
| Recall with unsupported anchor format | Memory anchored only by bare symbol name | `groundingStatus == "UNCHECKED"` |
| Enum wire values remain correct | Use `TRADE_OFF` memory and grounded result | response keeps `"TRADE_OFF"` and `"CONFIRMED"` |

No snapshot tests are needed. This is deterministic structured JSON; `assert_eq!` is clearer.

## Files Changed

| File | Change |
|------|--------|
| `crates/repolens-core/src/model/memory.rs` | Add `GroundedBy` / `ContradictedBy` to `MemoryRelation`; add `GroundingStatus` |
| `crates/repolens-app/src/grounding.rs` | New pure grounding module |
| `crates/repolens-app/src/lib.rs` | Export `grounding` module |
| `crates/repolens-mcp/src/tools/memory.rs` | Resolve pack, call app grounding, switch recall response to typed serde structs |
| `crates/repolens-mcp/src/lib.rs` | Pass resolver into `handle_recall_longterm` |

## What This Does NOT Do

- Does not persist grounding edges yet
- Does not change decay rates based on grounding status
- Does not perform keyword or heuristic contradiction detection
- Does not use `graph.json` as grounding evidence in this phase
- Does not interpret bare symbol names as trustworthy anchors
- Does not add high-stakes on-scan grounding
- Does not change `memorize_insight` or `forget_insight`

Those are future-phase concerns that can build on this foundation once the recall-time grounding loop is in place and trustworthy.
