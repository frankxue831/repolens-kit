# Test-File Detection Invariant Catalogue

Authority: Supporting
Lifecycle: Living

**Date:** 2026-05-11
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-05-11-evidence-aware-findings.md](2026-05-11-evidence-aware-findings.md)

## Goal

Catalogue RepoLens's test-file classifier as a named observation family so the F-TEST-INVENTORY-RATIO and F-DELTA-TEST-FILE-RATIO evidence-aware findings can be graded above `unaudited`. The classifier is the single source for `scan.testFileCount`, `scan.sourceFileCount`, and `scan.testToSourceFileRatio` in the pack-level scan summary.

## Matching Contract

`repolens_app::summary::is_test_file(path: &str) -> bool` classifies a file path as a test file using language-aware filename and path-segment patterns. The classifier is intentionally conservative: it operates on the path string alone, not on file contents, and applies separator-bounded matching so substrings like `contest`, `manifest`, or `testing-utils` do not false-positive.

Inline-Rust tests (`#[cfg(test)] mod tests { ... }` inside a source file) are **not** counted as test files by this family — Phase 1 of the evidence-aware findings work documents this explicitly via the `INLINE_RUST_TESTS_NOT_COUNTED` limitation code, attached to every F-TEST-INVENTORY-RATIO and F-DELTA-TEST-FILE-RATIO emission. The catalogue is for filename-based detection only.

## Invariant Catalogue

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-TEST-DETECT-RUST` | `*_test.rs` and paths under `tests/**/*.rs` are classified as test | `crates/repolens-app/src/summary.rs::tests::i_test_detect_rust` | `confirmOnly` |
| `I-TEST-DETECT-JAVA` | JVM-style `*Test.java` / `*Spec.java` classnames and paths under `/test/`, `/spec/` are classified as test | `crates/repolens-app/src/summary.rs::tests::i_test_detect_java` | `confirmOnly` |
| `I-TEST-DETECT-PYTHON` | `test_*.py` and `*_test.py` are classified as test | `crates/repolens-app/src/summary.rs::tests::i_test_detect_python` | `confirmOnly` |
| `I-TEST-DETECT-GO` | `*_test.go` is classified as test | `crates/repolens-app/src/summary.rs::tests::i_test_detect_go` | `confirmOnly` |
| `I-TEST-DETECT-JSTS` | `*.test.{js,ts,jsx,tsx}`, `*.spec.*`, and paths under `__tests__/` are classified as test | `crates/repolens-app/src/summary.rs::tests::i_test_detect_jsts` | `confirmOnly` |
| `I-TEST-DETECT-PATH-PREFIX` | Leading `test/`, `tests/`, `spec/` directory segments are classified as test (covers test-only repos and project-style layouts) | `crates/repolens-app/src/summary.rs::tests::i_test_detect_path_prefix` | `confirmOnly` |
| `I-TEST-DETECT-NO-FALSE-POSITIVE-CONTEST` | Substring matches alone do not trigger classification: `contest.rs`, `manifest.rs`, `crates/testing-utils/src/lib.rs` are NOT classified as test | `crates/repolens-app/src/summary.rs::tests::i_test_detect_no_false_positive_contest` | `confirmOnly` |
| `I-TEST-DETECT-FIXTURE-PARITY` | Known fixture-style file paths produce stable classifications across versions of the classifier; covered both by the path-shape spot-check and by the public `compute_test_to_source_ratio` integration test which depends on the classifier | `crates/repolens-app/src/summary.rs::tests::i_test_detect_fixture_parity`; `::test_compute_test_to_source_ratio_public` | `confirmOnly` |

## Out of Scope

- **Inline-language tests.** Inline-Rust `#[cfg(test)]` modules, Python `if __name__ == "__main__": ...` doctest-style suites, and Java `@Test`-annotated methods inside source files are not counted. The `INLINE_RUST_TESTS_NOT_COUNTED` limitation captures the Rust case explicitly on consuming findings.
- **AST-driven detection.** The catalogue covers the path-string classifier, not any future content-based detector. If RepoLens later inspects file content to detect tests, a new family + catalogue is required.
- **Generated test scaffolding.** Test runners that generate `.spec` / `.test` files at build time may or may not be counted depending on whether RepoLens scans the build output. The classifier itself is correct on whatever it sees; what the scan includes is governed by `.gitignore` and the rules engine, not by this family.

## Maintenance Rule

If any cited classifier test is removed or weakened, any tool whose promoted payload relies on `scan.testFileCount` / `scan.sourceFileCount` / `scan.testToSourceFileRatio` (currently F-TEST-INVENTORY-RATIO and F-DELTA-TEST-FILE-RATIO) must drop a tier in the same change unless equivalent coverage is added.
