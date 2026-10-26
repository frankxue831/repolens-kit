# Epistemic Envelope: Observation / Heuristic / Interpretation Distinction in Tool Responses

**Date:** 2026-04-11
**Status:** Draft (amended 2026-04-14 to add `evidenceGrade` per [06-observation-invariants-and-grounding-eligibility.md §7](../../philosophy/06-observation-invariants-and-grounding-eligibility.md))

## Goal

Make RepoLens's three-layer truth model explicit in every MCP tool response. Today, agents consuming RepoLens data cannot distinguish a mechanical fact (file has 12 imports) from a codified opinion (file has "high coupling") from an agent-authored memory (this module handles auth). This causes two problems:

1. **Trust calibration** — agents treat all data as equally trustworthy, making decisions without accounting for confidence differences between layers.
2. **Provenance traceability** — agents cannot trace claims back to their source (pack extraction, heuristic rule, or memory node), breaking the "Evidence Over Confidence" principle.

This feature wraps every tool response in a uniform envelope that declares its knowledge layer, confidence level, and structured provenance.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Surface format | Top-level envelope wrapping existing `data` | Clean contract, uniform across tools, existing payload untouched |
| Confidence model | Categorical (mechanical/codified/variable) | No false precision; maps 1:1 to layers; agents don't over-index on numbers |
| Provenance format | Structured typed objects | Machine-parseable for automated trust decisions; agents can follow provenance programmatically |
| Provenance scope | Response-level, not per-field exhaustive | Keeps the contract compact; provenance points to the artifacts/stores that produced the response |
| Classification rule | Based on the actual payload emitted for this invocation | Some tools change layer composition by detail tier or by pack availability |
| Rollout | Phased — heuristic/interpretive/mixed tools first, pure observation tools second | Ships value faster; the non-mechanical tools benefit most from disambiguation |
| Mixed-layer handling | `knowledgeLayer` as array, `confidence` takes lowest | Honest about composite responses; worst-case confidence prevents over-trust |

## Envelope Schema

Every **successful** MCP tool response becomes:

```json
{
  "knowledgeLayer": "observation",
  "confidence": "mechanical",
  "evidenceGrade": "verified",
  "provenance": {
    "type": "packExtraction",
    "packId": "pack-20260411-120000",
    "source": "graph.json"
  },
  "data": { }
}
```

### Fields

| Field | Type | Values | Description |
|-------|------|--------|-------------|
| `knowledgeLayer` | `string \| string[]` | `"observation"`, `"heuristic"`, `"interpretation"` | Single string for pure-layer tools; array for mixed-layer tools |
| `confidence` | `string` | `"mechanical"`, `"codified"`, `"variable"` | Maps to layer: observation→mechanical, heuristic→codified, interpretation→variable. For mixed tools, the lowest confidence present. |
| `evidenceGrade` | `string` | `"verified"`, `"derived"`, `"unaudited"` | Claim made by the invariant suite about the producer's track record, orthogonal to `knowledgeLayer` and `confidence`. See "Evidence Grade" below. Defaults to `"unaudited"` until the tool's observation family cites a rubric-conforming catalogue from [doc 06 §5](../../philosophy/06-observation-invariants-and-grounding-eligibility.md). |
| `provenance` | `object \| object[]` | See below | Structured provenance tracing the claim to its source. Use a single object when one source is sufficient; use an array for mixed-layer or multi-source responses. |
| `data` | `object` | Tool-specific | The existing response body, unchanged from today |

**Error responses are not wrapped.** Errors returned via `err_json` stay flat as `{"error": "<message>"}` and carry no epistemic metadata — see "What Stays the Same" below. Consumers must therefore branch on the presence of `error` vs. `knowledgeLayer` / `evidenceGrade` at the top level. This is an explicit carve-out from "every response is an envelope": errors have no producer-track-record claim to make, so `evidenceGrade` would be meaningless on them. A future revision may wrap errors for uniformity, but this spec does not.

### Confidence Ordering

`mechanical` > `codified` > `variable`

For mixed-layer tools, `confidence` is set to the lowest (least trustworthy) layer present. A response containing both observation and heuristic data gets `confidence: "codified"`.

### Evidence Grade

`evidenceGrade` was added in the 2026-04-14 amendment to separate *claims about the production process* (`confidence`) from *claims about the producer's track record* (`evidenceGrade`). The two axes are orthogonal: a `confidence: mechanical` value may be `evidenceGrade: verified` (the observation family has a named invariant catalogue) or `evidenceGrade: unaudited` (the family has not yet been catalogued). Collapsing them into a single label would force a single claim that is true about production but misleading about verification.

| Value | Meaning | When to emit |
|---|---|---|
| `verified` | The observation family backing this response has a passing invariant suite cited in [doc 06 §5](../../philosophy/06-observation-invariants-and-grounding-eligibility.md) at `confirmOnly` or `contradictionEligible`. | Tool handler explicitly opts in via `ok_epistemic_graded(..., EvidenceGrade::Verified)`, naming the families in a code comment. |
| `derived` | The response is computed from verified inputs via a derivation step that does not itself have invariant coverage. | Heuristic and interpretation tools whose upstream observations are verified but whose derivation is not. |
| `unaudited` | No invariant suite has been cited for this tool's observation family, or the tool has not been promoted yet. | Default. `ok_epistemic()` (the 4-param helper) returns this value. |

**Default posture.** Per doc 06 §3, new observation families are excluded until invariant coverage exists. The envelope implements this by defaulting `evidenceGrade` to `"unaudited"` at every call site that has not opted in. The 4-argument `ok_epistemic()` helper preserves the default; the 5-argument `ok_epistemic_graded()` helper accepts an explicit `EvidenceGrade` and is the only way to emit `"verified"` or `"derived"`. Promoting a tool is therefore a visible, greppable code change — not a silent side effect.

**Mixed responses.** For tools whose data spans multiple observation families with different grades, use the lowest grade present (`unaudited` < `derived` < `verified`). This mirrors the lowest-confidence rule and keeps the envelope honest about the weakest link.

### Provenance Types

**Observation (pack-backed)** — traced to pack extraction:
```json
{ "type": "packExtraction", "packId": "pack-20260411-120000", "source": "graph.json" }
```

The `source` field names the primary pack file the data came from. For tools that aggregate multiple pack files (e.g., `get_summary` reads manifest + graph + profile), use the most specific source or `"multiple"` if no single file dominates.

**Observation (live repository read)** — traced to repo files or directories outside a specific pack:
```json
{ "type": "repositoryRead", "source": ".claude/rules/*.md" }
```

Use this for tools that inspect repository state directly rather than reading a resolved pack. Examples:
- `discover_packs` → `repositoryRead { source: ".repolens/" }`
- `get_conventions` → `repositoryRead` entries for `CLAUDE.md` and `.claude/rules/*.md`

**Heuristic** — traced to rule and threshold:
```json
{ "type": "heuristicRule", "rule": "dominantModule", "threshold": ">80%", "packId": "pack-20260411-120000" }
```

Use this when the tool emits an analyzer judgment rather than a raw extracted fact. The `rule` field should name the computation unit (`healthAnalysis`, `projectProfile`, `deadCodeDetection`, `repoSummary`, etc.), and `threshold` may be `"multiple"` when several heuristics contribute.

**Interpretation (memory, targeted)** — traced to a specific memory node:
```json
{ "type": "memoryNode", "nodeId": "abc-123", "decayImportance": 0.72, "groundingStatus": "unverifiable" }
```

For `memorize_insight`, `nodeId` is the newly created node ID. For `forget_insight`, it is the deleted node ID. `decayImportance` and `groundingStatus` are optional and omitted when unavailable.

**Interpretation (memory, collection)** — traced to the memory store, with optional returned node IDs:
```json
{ "type": "memoryStore", "source": "memory.json", "nodeIds": ["abc-123", "def-456"] }
```

Use this for collection responses like `recall_longterm`, where the response is produced from the memory store rather than centered on a single node.

**Interpretation (wiki, targeted)** — traced to a single wiki article:
```json
{ "type": "wikiArticle", "articleId": "def-456", "staleness": "fresh" }
```

**Interpretation (wiki, collection)** — traced to the wiki index, with optional returned article IDs:
```json
{ "type": "wikiIndex", "source": "wiki-index.json", "articleIds": ["def-456", "ghi-789"] }
```

Use this for collection responses like `wiki_search` and `wiki_index`.

### Provenance Rules

- Provenance is response-level. It should point to the artifact/store that produced the response, not attempt exhaustive per-field lineage.
- `provenance` may contain more than one entry for the same layer if that is the clearest representation of the response. Example: `wiki_lint` may need `wikiIndex` + `memoryStore` + `packExtraction`.
- Tools whose payload changes by parameters or runtime context must compute `knowledgeLayer`, `confidence`, and `provenance` from the payload actually emitted, not from a static tool-level constant.

## Tool Classification

### Phase 1 — Heuristic / Interpretive / Mixed Tools (14 tools)

| Tool | Layers | Confidence | Provenance Types | Notes |
|------|--------|------------|------------------|-------|
| `analyze_health` | observation + heuristic | codified | packExtraction + heuristicRule | Health report combines pack facts with analyzer judgments |
| `get_profile` | observation + heuristic | codified | packExtraction + heuristicRule | Framework, entry-point, and project-type detection are inferred |
| `get_summary` | `compact`: observation + heuristic; `standard`/`full`: observation + heuristic + interpretation | `compact`: codified; `standard`/`full`: variable | packExtraction + heuristicRule + memoryStore (when `memoryInsights` are included) | Envelope must follow the chosen detail tier |
| `compare_summaries` | observation + heuristic | codified | packExtraction + heuristicRule | Compares derived summary metrics, not raw pack facts |
| `find_dead_code` | observation + heuristic | codified | packExtraction + heuristicRule | Depends on inferred entry points plus graph analysis |
| `recall_longterm` | interpretation; add observation when grounding is assessed against a pack | variable | memoryStore + packExtraction (when pack grounding is available) | `groundingStatus` / anchor checks are observational support metadata |
| `memorize_insight` | interpretation | variable | memoryNode |
| `forget_insight` | interpretation | variable | memoryNode |
| `wiki_compile` | interpretation | variable | wikiArticle |
| `wiki_read` | interpretation | variable | wikiArticle |
| `wiki_search` | interpretation | variable | wikiIndex |
| `wiki_index` | interpretation | variable | wikiIndex |
| `wiki_lint` | observation + interpretation | variable | wikiIndex + memoryStore + packExtraction (when pack is available) | Broken memory refs come from memory-store existence checks; stale detection uses the active/latest pack when present |
| `wiki_delete` | interpretation | variable | wikiArticle |

### Phase 2 — Pure Observation Tools (10 tools)

| Tool | Layer | Confidence | Provenance Type |
|------|-------|------------|-----------------|
| `discover_packs` | observation | mechanical | repositoryRead |
| `read_file_content` | observation | mechanical | packExtraction |
| `search_code` | observation | mechanical | packExtraction |
| `get_graph` | observation | mechanical | packExtraction |
| `get_callers` | observation | mechanical | packExtraction |
| `get_dependencies` | observation | mechanical | packExtraction |
| `get_impact` | observation | mechanical | packExtraction |
| `find_cycles` | observation | mechanical | packExtraction |
| `get_architecture` | observation | mechanical | packExtraction |
| `get_conventions` | observation | mechanical | repositoryRead |

## Rust Implementation

### New Module: `crates/repolens-mcp/src/epistemic.rs`

```rust
use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeLayer {
    Observation,
    Heuristic,
    Interpretation,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Mechanical,
    Codified,
    Variable,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceGrade {
    Verified,
    Derived,
    #[default]
    Unaudited,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Provenance {
    PackExtraction {
        pack_id: String,
        source: String,
    },
    RepositoryRead {
        source: String,
    },
    HeuristicRule {
        rule: String,
        threshold: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pack_id: Option<String>,
    },
    MemoryNode {
        node_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        decay_importance: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        grounding_status: Option<String>,
    },
    MemoryStore {
        source: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        node_ids: Vec<String>,
    },
    WikiArticle {
        article_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        staleness: Option<String>,
    },
    WikiIndex {
        source: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        article_ids: Vec<String>,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum KnowledgeLayerField {
    Single(KnowledgeLayer),
    Multiple(Vec<KnowledgeLayer>),
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ProvenanceField {
    Single(Provenance),
    Multiple(Vec<Provenance>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResponse {
    pub knowledge_layer: KnowledgeLayerField,
    pub confidence: Confidence,
    pub evidence_grade: EvidenceGrade,
    pub provenance: ProvenanceField,
    pub data: serde_json::Value,
}
```

### New Helper Functions

Two helpers cover the default-unaudited and explicit-grade cases. `ok_epistemic` is the existing 4-argument entry point and silently attaches `EvidenceGrade::Unaudited`; `ok_epistemic_graded` takes an explicit 5th argument and is the only way to emit `verified` or `derived`. Adopting the 5-argument helper in a tool handler is the observable code change that promotes a tool out of the unaudited default.

```rust
pub fn ok_epistemic(
    layer: KnowledgeLayerField,
    confidence: Confidence,
    provenance: ProvenanceField,
    data: serde_json::Value,
) -> Result<CallToolResult, McpError> {
    ok_epistemic_graded(layer, confidence, EvidenceGrade::default(), provenance, data)
}

pub fn ok_epistemic_graded(
    layer: KnowledgeLayerField,
    confidence: Confidence,
    evidence_grade: EvidenceGrade,
    provenance: ProvenanceField,
    data: serde_json::Value,
) -> Result<CallToolResult, McpError> {
    let response = ToolResponse {
        knowledge_layer: layer,
        confidence,
        evidence_grade,
        provenance,
        data,
    };
    let text = serde_json::to_string_pretty(&response).unwrap_or_default();
    Ok(CallToolResult::success(vec![Content::text(text)]))
}
```

### Migration Pattern

Each tool handler changes from:
```rust
ok_json(&value)
```
to:
```rust
ok_epistemic(
    KnowledgeLayerField::Single(KnowledgeLayer::Observation),
    Confidence::Mechanical,
    ProvenanceField::Single(Provenance::RepositoryRead {
        source: ".repolens/".to_string(),
    }),
    &value,
)
```

For mixed-layer tools like `get_profile`:
```rust
ok_epistemic(
    KnowledgeLayerField::Multiple(vec![
        KnowledgeLayer::Observation,
        KnowledgeLayer::Heuristic,
    ]),
    Confidence::Codified,
    ProvenanceField::Multiple(vec![
        Provenance::PackExtraction {
            pack_id: pack_id.clone(),
            source: "multiple".into(),
        },
        Provenance::HeuristicRule {
            rule: "projectProfile".into(),
            threshold: "multiple".into(),
            pack_id: Some(pack_id.clone()),
        },
    ]),
    &value,
)
```

For parameter-sensitive tools like `get_summary`, choose the envelope from the emitted tier:

```rust
let (layers, confidence, provenance) = if detail == "full" || detail == "standard" {
    (
        KnowledgeLayerField::Multiple(vec![
            KnowledgeLayer::Observation,
            KnowledgeLayer::Heuristic,
            KnowledgeLayer::Interpretation,
        ]),
        Confidence::Variable,
        ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: pack_id.clone(),
                source: "multiple".into(),
            },
            Provenance::HeuristicRule {
                rule: "repoSummary".into(),
                threshold: "multiple".into(),
                pack_id: Some(pack_id.clone()),
            },
            Provenance::MemoryStore {
                source: "memory.json".into(),
                node_ids: collect_memory_ids(&response),
            },
        ]),
    )
} else {
    (
        KnowledgeLayerField::Multiple(vec![
            KnowledgeLayer::Observation,
            KnowledgeLayer::Heuristic,
        ]),
        Confidence::Codified,
        ProvenanceField::Multiple(vec![
            Provenance::PackExtraction {
                pack_id: pack_id.clone(),
                source: "multiple".into(),
            },
            Provenance::HeuristicRule {
                rule: "repoSummary".into(),
                threshold: "multiple".into(),
                pack_id: Some(pack_id.clone()),
            },
        ]),
    )
};

ok_epistemic(layers, confidence, provenance, &value)
```

### Files Touched

- **Create:** `crates/repolens-mcp/src/epistemic.rs` — types + helper
- **Modify:** `crates/repolens-mcp/src/lib.rs` — add `mod epistemic; pub use epistemic::*;`; migrate `discover_packs`, `read_file_content`, and `search_code`
- **Modify (Phase 1):** `crates/repolens-mcp/src/tools/profile.rs` — `get_profile`, `analyze_health`
- **Modify (Phase 1):** `crates/repolens-mcp/src/tools/summary.rs` — `get_summary`, `compare_summaries`
- **Modify (Phase 1):** `crates/repolens-mcp/src/tools/memory.rs` — `memorize_insight`, `recall_longterm`, `forget_insight`
- **Modify (Phase 1):** `crates/repolens-mcp/src/tools/wiki.rs` — all 6 wiki tools
- **Modify (Phase 1):** `crates/repolens-mcp/src/tools/graph.rs` — `find_dead_code`
- **Modify (Phase 2):** `crates/repolens-mcp/src/tools/graph.rs` — `get_graph`, `get_callers`, `get_dependencies`, `get_impact`, `find_cycles`
- **Modify (Phase 2):** `crates/repolens-mcp/src/tools/architecture.rs` — `get_architecture`
- **Modify (Phase 2):** `crates/repolens-mcp/src/tools/conventions.rs` — `get_conventions`

### What Stays the Same

- `err_json()` — errors don't need epistemic metadata
- `CallToolResult` / `Content::text()` — rmcp framework untouched
- Tool parameter structs — no input changes
- Existing `_meta` / `_hints` fields within `data` — preserved inside the envelope
- Test helper `extract_text()` — unchanged; tests will parse the new envelope instead of the old root payload

## Testing Strategy

### Unit Tests (`epistemic.rs`)

- Serialize each `KnowledgeLayer` variant → verify camelCase output
- Serialize each `Confidence` variant → verify camelCase output
- Serialize each `Provenance` variant → verify tagged union with `type` field
- Verify optional fields on `MemoryNode` / `WikiArticle` are omitted when absent
- Verify `MemoryStore` / `WikiIndex` omit empty ID arrays
- Serialize `KnowledgeLayerField::Single` → string, `Multiple` → array
- Serialize `ProvenanceField::Single` → object, `Multiple` → array
- Serialize full `ToolResponse` → verify complete envelope shape

### Integration Tests (Per Migrated Tool)

For each migrated tool:
- Call the handler with valid params
- Deserialize the response text as `serde_json::Value`
- Assert `knowledgeLayer` field exists with correct value
- Assert `confidence` field exists with correct value
- Assert `provenance` field exists with correct `type`
- Assert `data` field contains the expected tool-specific payload

Add explicit coverage for parameter-sensitive and context-sensitive tools:
- `get_summary(detail=compact)` → observation + heuristic, `confidence == "codified"`
- `get_summary(detail=full)` → observation + heuristic + interpretation, `confidence == "variable"`
- `recall_longterm` with active pack → includes pack-backed provenance for grounding
- `recall_longterm` without pack → no pack-backed provenance, still valid envelope
- `wiki_lint` with pack → includes `packExtraction` provenance
- `wiki_lint` without pack → omits `packExtraction` provenance but still reports `wikiIndex` / `memoryStore`

Existing MCP test suites will need updates to unwrap `.data`:
- `crates/repolens-mcp/tests/integration_test.rs`
- `crates/repolens-mcp/tests/memory_test.rs`
- `crates/repolens-mcp/tests/wiki_test.rs`
- `crates/repolens-mcp/tests/wiki_e2e_test.rs`
- Any wiki populate or helper tests that currently assert against the old root JSON shape

### Manual Verification

- `cargo run -- mcp` → call tools via MCP client
- Inspect JSON responses have the envelope at the top level
- Verify mixed-layer tools return arrays for `knowledgeLayer` and `provenance`
- Verify `discover_packs` / `get_conventions` use `repositoryRead`, not fake pack provenance
- Verify `get_summary` changes layer/confidence by detail tier
- Verify `recall_longterm` and `wiki_lint` vary provenance correctly when a pack is or is not available
- Verify error responses (`err_json`) are unchanged

## Out of Scope

| Item | Reason |
|------|--------|
| Per-field annotations | Envelope-level metadata is sufficient; per-field would require restructuring all response types |
| Numeric confidence scores | Categorical labels avoid false precision; revisit if agents need finer granularity |
| Automatic provenance from call stack | Would require plumbing pack context through all layers; manual construction is sufficient |
| Tool description updates | Tool descriptions could mention epistemic metadata, but that's a documentation task, not a code change |
| Breaking change migration guide | Agents consuming RepoLens will need to unwrap `.data`; this is intentional and documented in release notes |
