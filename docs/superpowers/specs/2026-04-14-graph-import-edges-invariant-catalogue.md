# Graph Import Edges Invariant Catalogue

**Date:** 2026-04-14
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-13-observation-invariant-rollout-design.md](2026-04-13-observation-invariant-rollout-design.md)

## Goal

Extend observation-invariant coverage from the `contradictionEligible` families (now fully covered by the two April 13 catalogues) down into the highest-decay-risk `confirmOnly` family: **graph import edges**. Doc 06 §5 calls graph-derived observations "the weakest link" and names the resolver as the source of most historical self-scan bugs (Rust `self::`/`super::` resolution in `883565b`, `declares`-in-SCC in `68845c5`). This catalogue formalizes five invariants over the captured `graph.json` contract — four new ones plus the pre-existing `I-GRAPH-EDGE-CLOSURE` from the first rollout — to guard against the class of resolver drift those bugs belonged to.

This is a docs-and-tests change only. No runtime behaviour changes.

## Scope

In scope:

- Four new invariant tests in `crates/repolens-core/tests/graph_contract_test.rs` that bind the `graph import edges` family to rubric-conforming coverage.
- A minor update to doc 06 §5: refresh the `Graph import edges` row's "Current invariant coverage" column to cite the five IDs.

Explicitly deferred:

- **Graph `declares` edges** (doc 06 §5, `confirmOnly`). Captured Java contract fixtures do not exercise declares edges at all (see `VALID_EDGE_TYPES` in `graph_contract_test.rs`). Invariants here need Rust fixtures — deferred to a future spec.
- **Graph cycles** (`CycleReport`, `confirmOnly`). The 15-file phantom cycle fixed in `68845c5` was exactly this family; its invariant needs a Rust fixture with declares edges and a live run of `graph_analysis.rs`. Deferred.
- **Orphan detection** (`confirmOnly`). Needs a fixture with a file that has neither incoming nor outgoing edges. Deferred.

Those three deferred families remain at `confirmOnly` with informal coverage. They do not block any Path B phase — rubric §6 requirement 5 is a coverage target, not a promotion gate.

## The Matching Contract

From the existing graph model (`crates/repolens-core/src/model/graph.rs`):

- A `FileGraph` has `nodes: Vec<GraphNode>` and `edges: Vec<GraphEdge>`.
- `GraphNode.id` is a file path relative to the repo root — the same shape as `FileEntry.path`.
- `GraphEdge.source` / `GraphEdge.target` are both `GraphNode.id` values.
- `GraphEdge.edge_type` is one of `imports`, `extends`, `implements`, `calls`, `type_ref` (see `VALID_EDGE_TYPES`).
- `GraphEdge.detail` is `Option<String>` and is serialized only when `Some`.

The `graph import edges` family covers edges of type `imports`. The matching contract, stated end-to-end, is:

> For every edge the resolver emits, both endpoints are real files the scanner captured, the edge does not loop on itself, and every `imports` edge carries the resolver input that produced it.

The five invariants below together encode that contract.

## Invariant Catalogue

All five invariants are green on `2026-04-14` against the captured contract scenarios at `fixtures/scenarios/{java-small, mixed-small, excluded-small}`.

| ID | Property | Evidence form | Tier held | Status |
|---|---|---|---|---|
| `I-GRAPH-EDGE-CLOSURE` | Every `edge.source` and `edge.target` appears in `graph.nodes[].id` | Scenario loop, set-membership assertion | `confirmOnly` | Pre-existing (rollout 1) |
| `I-GRAPH-NODE-ID-UNIQUE` | Within a single graph, `nodes[].id` values are unique | Scenario loop, `HashSet` count | `confirmOnly` | New |
| `I-GRAPH-NODE-MATCHES-PACK-PATH` | Every `nodes[].id` appears as an included `FileEntry.path` in the same pack | Cross-file scenario assertion against `files-*.json` | `confirmOnly` | New |
| `I-GRAPH-NO-SELF-LOOP` | No edge has `source == target` | Scenario loop, per-edge assertion | `confirmOnly` | New |
| `I-GRAPH-IMPORT-EDGE-DETAIL` | Every `imports`-type edge has non-empty `Some(detail)` | Scenario loop, per-`imports`-edge assertion | `confirmOnly` | New |

### Rationale for Each New Invariant

**`I-GRAPH-NODE-ID-UNIQUE`.** The existing `I-GRAPH-EDGE-CLOSURE` test could pass vacuously if the same file appeared twice in `nodes[]` — an edge endpoint would still be "present in the node set" even under a resolver bug that emits duplicate nodes. Making uniqueness its own invariant prevents closure from rubber-stamping.

**`I-GRAPH-NODE-MATCHES-PACK-PATH`.** The strongest structural tie between graph and pack: every node is a real captured file. If the resolver ever emits a ghost node (an import target that does not exist on disk), this invariant catches it. This is the graph-family analogue of `I-FILE-CONTENT-INDEX-COMPLETE` — it ties two independent pack artifacts together and fails loudly if they drift.

**`I-GRAPH-NO-SELF-LOOP`.** A file does not import itself. A self-loop is the exact shape a "resolver fell back to source file when target resolution failed" bug takes — and historically the Rust `self::`/`super::` bug resolved paths incorrectly enough to produce this class of error in adjacent code. Cheap to check, high signal.

**`I-GRAPH-IMPORT-EDGE-DETAIL`.** `imports` edges are special because the resolver uses `detail` as both input and output — the import spec (`com.example.Helper`, `crate::foo::Bar`) is what resolves to the target. An imports edge without detail is unauditable: a downstream consumer cannot tell which import statement produced it, so a memory anchor claiming "this module imports X" cannot be grounded against the edge. Other edge types (`calls`, `type_ref`, `extends`, `implements`) are permitted to carry `None` because their `detail` is not on the resolver's critical path.

## Maintenance Rule

If any of the five invariants is deleted or weakened, the `graph import edges` family drops a tier (`confirmOnly` → `excluded`) in doc 06 §5, AND `crates/repolens-app/src/grounding.rs` MUST be audited to ensure it does not trust anchors for this family. Reviewers of any PR touching `graph_contract_test.rs`, the graph model, or the resolver must verify this rule.

## Rubric-Requirement-5 Progress

After this spec:

| Family | Tier | Named invariants |
|---|---|---|
| Included file presence | `contradictionEligible` | 4 ✅ |
| Exact qualified symbols | `contradictionEligible` | 4 ✅ |
| File content extraction | `contradictionEligible` | 5 ✅ |
| Graph import edges | `confirmOnly` | **5 ✅** (this spec) |
| Entry point detection | `confirmOnly` | 1 ✅ |
| Incremental manifest metadata | `confirmOnly` | 1 ✅ |
| Language classification | `confirmOnly` | 0 ❌ |
| Graph declares edges | `confirmOnly` | 0 ❌ |
| Graph cycles | `confirmOnly` | 0 ❌ |
| Orphan detection | `confirmOnly` | 0 ❌ |
| Project profile name | `confirmOnly` | 0 ❌ |

Five of eleven `confirmOnly`-or-higher families now have named catalogues. Four of the six remaining (declares, cycles, orphans, plus language classification) fail primarily because captured Java fixtures do not exercise them. A single Rust fixture would close three of those four in one commit.

## File Plan

| File | Change |
|---|---|
| `crates/repolens-core/tests/graph_contract_test.rs` | Add `load_entries` helper; add 4 new `#[test]` functions named by invariant ID. Existing tests untouched. |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Refresh the `Graph import edges` row in §5 to cite the 5 invariant IDs; add this spec to `Depends on`. |
| `docs/superpowers/specs/2026-04-14-graph-import-edges-invariant-catalogue.md` | This spec. |

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-core --test graph_contract_test
cargo clippy -p repolens-core --tests -- -D warnings
```

All 21 tests in `graph_contract_test.rs` must be green (17 pre-existing + 4 new). `clippy` must remain warning-free.
