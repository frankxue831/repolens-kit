# Graph Declares, Cycles, and Orphans Invariant Catalogue

**Date:** 2026-04-14
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-14-graph-import-edges-invariant-catalogue.md](2026-04-14-graph-import-edges-invariant-catalogue.md)

## Goal

Close the remaining three `confirmOnly` graph families in doc 06 §5 — *graph declares edges*, *graph cycles*, *orphan detection* — by binding them to named invariant IDs. Unlike the prior catalogues, this one adds **no new tests**. The underlying coverage already exists as well-structured unit tests in `crates/repolens-app/src/graph_analysis.rs` and `crates/repolens-extract/tests/rust_extract_test.rs`. The gap was that doc 06 §5 described the coverage as "Unit + fixture tests for graph_analysis.rs" — informal and ungreppable. This spec renames the existing tests to invariant-ID form and catalogues them, so a future `grep -n "I-GRAPH-CYCLES-EXCLUDE-DECLARES"` finds both the catalogue and the test body.

This is a paperwork commit. The invariants are the same invariants the code has been carrying since `68845c5`. What changes: they now have names, and doc 06 §5's rubric requirement 5 is satisfied for three more families.

## Scope

In scope:

- Rename 8 existing `#[test]` functions in `graph_analysis.rs` and 1 in `rust_extract_test.rs` to match invariant-ID snake_case convention, with an invariant-ID doc comment above each.
- Cite those renamed tests in a catalogue spec (this file).
- Update doc 06 §5 rows for *graph declares edges*, *graph cycles*, and *orphan detection* to cite the invariant IDs.

Out of scope:

- Any new test logic or fixture data.
- Any change to `graph_analysis.rs` production code.
- Language classification (separate family, separate future commit).
- Project profile name (separate family, separate future commit).
- A Rust end-to-end scan test of `fixtures/inputs/rust-small`. That would close a different gap — extractor-to-graph parity under a live scan — and deserves its own commit.

## Why Paperwork Is Legitimate Rubric Coverage

Doc 06 §6 rubric requirement 3 explicitly accepts "targeted unit test" as a valid evidence form alongside property-based tests and golden fixtures. Requirement 2 requires each invariant to map to exactly one observation family. Requirement 5 requires at least one invariant per family at `confirmOnly` or higher. Requirements 1 and 6 — stable ID, lockstep maintenance — are the ones the existing tests currently fail, because they are named by the phenomenon they test (`test_declares_edges_excluded_from_cycles`), not by a catalogue ID. Renaming closes those two gaps without touching coverage.

The rename is cheap (mechanical), reversible (git-obvious), and unambiguous (one new name per test, one-to-one mapping). History cost is minimal because no test callers exist — test functions are called by the harness only.

## Invariant Catalogue

All ten invariants are green on `2026-04-14`. Each maps to one or more of three observation families.

### Graph Declares Edges Family

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-RUST-MOD-DECLARES-EMITTED` | `RustRelationshipExtractor` emits `declares` from `mod X;` file-mode statements, and *only* from file-mode (inline `mod X { ... }` produces no declares edge) | `crates/repolens-extract/tests/rust_extract_test.rs::i_rust_mod_declares_emitted` | `confirmOnly` |
| `I-GRAPH-CYCLES-EXCLUDE-DECLARES` | `GraphAnalyzer::find_cycles` filters declares edges out of the SCC input. A `lib.rs declares foo.rs; foo.rs imports lib.rs` shape does not produce a cycle | `crates/repolens-app/src/graph_analysis.rs::i_graph_cycles_exclude_declares` | `confirmOnly` |
| `I-GRAPH-ORPHANS-RESPECT-DECLARES` | A file referenced only by an incoming declares edge is NOT flagged as an orphan | `crates/repolens-app/src/graph_analysis.rs::i_graph_orphans_respect_declares` | `confirmOnly` |

### Graph Cycles Family

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-GRAPH-CYCLES-EXCLUDE-DECLARES` (cross-family) | See above | Same | `confirmOnly` |
| `I-GRAPH-CYCLES-EXCLUDE-CALLS` | `calls` edges are not counted in cycle detection (they project from method names to files and are too noisy to be a dependency signal) | `crates/repolens-app/src/graph_analysis.rs::i_graph_cycles_exclude_calls` | `confirmOnly` |
| `I-GRAPH-CYCLES-EXCLUDE-SELF-LOOP` | An edge with `source == target` does not produce a 1-node cycle in the SCC output | `crates/repolens-app/src/graph_analysis.rs::i_graph_cycles_exclude_self_loop` | `confirmOnly` |
| `I-GRAPH-CYCLE-REPORT-SHAPE` | `GraphAnalyzer::find_cycles` over `fixtures/test-data/cyclic-graph/graph.json` produces exactly two cycles: `{a.rs, b.rs, c.rs}` (3-file) and `{d.rs, e.rs}` (2-file). Edge counts and node membership are asserted | `crates/repolens-app/src/graph_analysis.rs::i_graph_cycle_and_orphan_report_shape_from_fixture` (cycle portion) | `confirmOnly` |

### Orphan Detection Family

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-GRAPH-ORPHANS-RESPECT-DECLARES` (cross-family) | See above | Same | `confirmOnly` |
| `I-GRAPH-ORPHANS-BASELINE` | A file with zero structural inbound edges is reported as an orphan; `total_files` and `orphan_files` are consistent | `crates/repolens-app/src/graph_analysis.rs::i_graph_orphans_baseline` | `confirmOnly` |
| `I-GRAPH-ORPHANS-EXCLUDE-ENTRY-POINTS` | Files in the entry-point set (e.g., `main.rs`) are not flagged as orphans even when they have zero inbound edges | `crates/repolens-app/src/graph_analysis.rs::i_graph_orphans_exclude_entry_points` | `confirmOnly` |
| `I-GRAPH-ORPHANS-IGNORE-CALLS` | `calls` edges do not count as structural inbound edges for orphan purposes — symmetric with `I-GRAPH-CYCLES-EXCLUDE-CALLS` | `crates/repolens-app/src/graph_analysis.rs::i_graph_orphans_ignore_calls` | `confirmOnly` |
| `I-GRAPH-ORPHAN-REPORT-SHAPE` | `GraphAnalyzer::find_orphans` over the cyclic-graph fixture flags exactly `src/orphan.rs` | `crates/repolens-app/src/graph_analysis.rs::i_graph_cycle_and_orphan_report_shape_from_fixture` (orphan portion) | `confirmOnly` |

Ten distinct invariants; two cross-cutting (I-GRAPH-CYCLES-EXCLUDE-DECLARES and I-GRAPH-ORPHANS-RESPECT-DECLARES each serve two families). Every family has at least three invariants, well above the rubric §6 requirement 5 minimum.

## Maintenance Rule

If any of the ten invariants is deleted or weakened, the affected family or families drop a tier (`confirmOnly` → `excluded`) in doc 06 §5. Because five of the ten test the *negative* space of cycle/orphan detection (what must NOT be flagged), weakening them is indistinguishable from a silent over-reporting regression — exactly the class of bug `68845c5` was. Reviewers of any PR touching `graph_analysis.rs::find_cycles`, `find_orphans`, `STRUCTURAL_EDGE_TYPES`, or `is_dependency` must verify that all ten tests remain green and have not been renamed or `#[ignore]`d.

## Rubric-Requirement-5 Progress

After this spec:

| Family | Tier | Named invariants |
|---|---|---|
| Included file presence | `contradictionEligible` | 4 ✅ |
| Exact qualified symbols | `contradictionEligible` | 4 ✅ |
| File content extraction | `contradictionEligible` | 5 ✅ |
| Graph import edges | `confirmOnly` | 5 ✅ |
| **Graph declares edges** | `confirmOnly` | **3 ✅** (this spec) |
| **Graph cycles** | `confirmOnly` | **4 ✅** (this spec) |
| **Orphan detection** | `confirmOnly` | **5 ✅** (this spec) |
| Entry point detection | `confirmOnly` | 1 ✅ |
| Incremental manifest metadata | `confirmOnly` | 1 ✅ |
| Language classification | `confirmOnly` | 0 ❌ |
| Project profile name | `confirmOnly` | 0 ❌ |

Eight of eleven families now have named catalogues. Only *language classification* and *project profile name* remain. Both are `confirmOnly` and neither involves the graph resolver, so neither endangers the memory layer under Path B. They are the natural targets for a final rubric-completion commit.

## File Plan

| File | Change |
|---|---|
| `crates/repolens-app/src/graph_analysis.rs` | Rename 8 `#[test]` functions to `i_graph_*` form with invariant-ID doc comments. |
| `crates/repolens-extract/tests/rust_extract_test.rs` | Rename `extracts_mod_declarations` to `i_rust_mod_declares_emitted` with invariant-ID doc comment. |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Update §5 rows for declares, cycles, orphans to cite the ten invariant IDs; add this spec to `Depends on`. |
| `docs/superpowers/specs/2026-04-14-graph-declares-cycles-orphans-catalogue.md` | This spec. |

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-app graph_analysis
cargo test -p repolens-extract i_rust_mod_declares_emitted
cargo clippy -p repolens-app --tests -- -D warnings
cargo clippy -p repolens-extract --tests -- -D warnings
```

All eleven `graph_analysis::tests` must be green alongside the invariant-renamed extractor test. No regressions elsewhere.
