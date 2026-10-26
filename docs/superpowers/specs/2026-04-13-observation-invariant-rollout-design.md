# Observation Invariant Rollout

**Date:** 2026-04-13
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md)

## Goal

Turn the new philosophy contract into an initial engineering slice: define the first observation families to cover, add the first invariant tests, and keep runtime grounding behavior unchanged except where an uncovered observation bug is found and fixed as part of the red-green cycle.

## Scope

This rollout covers three invariant families only:

1. **Graph endpoint closure** — every emitted graph edge must point at graph nodes that exist in the same graph payload.
2. **Rust entry-point completeness (initial Cargo slice)** — profile analysis must detect Rust binary entry points not only at `src/main.rs`, but also in the most common Cargo bin target conventions supported by the current codebase.
3. **Incremental manifest core metadata parity** — incremental scans must retain the same core manifest metadata fields that full scans emit today, while also populating incremental-only fields.

This rollout does **not**:

- change grounding eligibility tiers at runtime
- add `evidenceGrade` to the epistemic envelope
- attempt full observation-family coverage
- add contradiction decay or high-stakes scan-time grounding

## Decisions

| Question | Decision | Rationale |
|---|---|---|
| Where does the first registry live? | In docs first, tests second | Keeps the contract explicit before any runtime policy change |
| Which manifests are compared? | Only fields RepoLens actually models today | Avoids writing tests against schema fields that do not exist in `Manifest` |
| How much Rust Cargo support is in scope? | `src/main.rs`, direct `src/bin/*.rs`, and explicit `[[bin]] path = ...` targets | Covers the common cases the current code misses without introducing a TOML parser dependency |
| Should runtime grounding change in this slice? | No | The aim is to tighten observation guarantees before widening contradiction rights |

## Initial Registry

| Invariant ID | Observation Family | Tier Impact | Target files |
|---|---|---|---|
| `I-GRAPH-EDGE-CLOSURE` | Graph import / structural edges | Supports staying at least `confirmOnly` | `crates/repolens-core/tests/graph_contract_test.rs` |
| `I-PROFILE-RUST-BIN-TARGETS` | Project profile entry-point detection | Supports keeping entry points at `confirmOnly` until broader Cargo coverage lands | `crates/repolens-profile/src/profile.rs` |
| `I-SCAN-INCR-MANIFEST-CORE` | Incremental manifest metadata | Supports lifting incremental manifest metadata out of `excluded` once the broader family is specified | `crates/repolens-app/src/scan.rs` |

## Test Design

### 1. `I-GRAPH-EDGE-CLOSURE`

Assert across all contract fixtures that:

- every `edge.source` appears in `graph.nodes[].id`
- every `edge.target` appears in `graph.nodes[].id`

This is a pure contract test. It should not require production code changes.

### 2. `I-PROFILE-RUST-BIN-TARGETS`

Add profile tests covering:

- direct bin discovery from `src/bin/*.rs`
- explicit bin discovery from Cargo manifests containing `[[bin]]` sections with `path = "..."`

Implementation should stay lightweight:

- reuse `read_content` on `Cargo.toml` entries already present in the pack
- parse only enough TOML structure to detect `[[bin]]` sections and `path` keys
- resolve `path` relative to the manifest directory
- avoid duplicate entry points when a file matches more than one convention

### 3. `I-SCAN-INCR-MANIFEST-CORE`

Run a full scan and a follow-up incremental scan over the same temporary repo, then assert:

- both manifests populate the common core fields RepoLens models today:
  `schemaVersion`, `schemaUrl`, `timestamp`, `chunks`, `scanMode`, `scanScope`,
  `headCommit`, `worktreeDirty`, `scanConfigHash`, `reprocessedFiles`
- the incremental manifest additionally populates:
  `basePackId`, `baseHeadCommit`, `deletedFiles`, `carriedForwardFiles`

This is an invariant over the current manifest schema, not over aspirational fields such as `repoRoot` or `fileCount`.

## File Plan

| File | Change |
|---|---|
| `docs/philosophy/00-status.md` | Refresh status and open questions to reflect the new contract |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Philosophy contract already written; no further scope change in this rollout |
| `docs/superpowers/specs/2026-04-13-observation-invariant-rollout-design.md` | This spec |
| `crates/repolens-core/tests/graph_contract_test.rs` | Add graph endpoint-closure invariant test |
| `crates/repolens-profile/src/profile.rs` | Add Rust bin-target tests and minimal implementation |
| `crates/repolens-app/src/scan.rs` | Add incremental manifest core-metadata parity test |

## Verification

Targeted commands for this rollout:

```sh
cargo test -p repolens-core --test graph_contract_test
cargo test -p repolens-profile --lib
cargo test -p repolens-app incremental_manifest_retains_core_metadata_fields
```

If the Rust entry-point test fails first, fix only the minimal logic needed for the new Cargo bin cases and rerun the same targeted test before running the broader crate test.
