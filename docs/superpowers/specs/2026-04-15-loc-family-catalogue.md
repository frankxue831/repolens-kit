# LOC Family Invariant Catalogue

**Date:** 2026-04-15
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-15-profile-signal-catalogue.md](2026-04-15-profile-signal-catalogue.md)

## Goal

Catalogue RepoLens line-count (`loc`) as a named observation family so tools that surface raw `FileEntry.loc` can be graded above `unaudited`. This catalogue does not use captured `ProjectProfile` fixture shapes as evidence for the live Rust `languageDistribution` or `modules` semantics.

## Matching Contract

`repolens_core::metadata::count_lines(path)` counts newline-terminated segments plus a final non-empty line without a trailing newline. `FileEntry.loc` stores that count during scan materialization, and `read_file_content` outline mode surfaces the stored value unchanged from the pack index. LOC-backed `ProjectProfile` aggregations are intentionally out of scope for this raw-LOC family because captured fixture payloads do not currently align with the live Rust producer semantics.

## Invariant Catalogue

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-LOC-COUNTS-BLANK-LINES` | Blank lines are included in the line count | `crates/repolens-core/tests/metadata_test.rs::loc_counts_all_lines_including_blank` | `confirmOnly` |
| `I-LOC-COUNTS-FINAL-LINE-WITHOUT-TRAILING-NEWLINE` | A final line without `\n` is still counted | `crates/repolens-core/tests/metadata_test.rs::loc_counts_final_line_without_trailing_newline` | `confirmOnly` |
| `I-LOC-FIXTURE-PARITY` | Known fixture files produce stable exact LOC values across scan storage and outline surfacing | `crates/repolens-core/tests/metadata_test.rs::loc_of_java_file_matches_fixture`; `::loc_of_helper_matches_fixture`; `crates/repolens-mcp/tests/integration_test.rs::read_file_content_outline_returns_symbols` | `confirmOnly` |

## Maintenance Rule

If any cited LOC test is removed or weakened, any tool whose promoted payload relies on raw `FileEntry.loc` must be demoted in the same change unless equivalent coverage is added.
