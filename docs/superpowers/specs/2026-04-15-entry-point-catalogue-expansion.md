# Entry Point Catalogue Expansion

**Date:** 2026-04-15
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-13-observation-invariant-rollout-design.md](2026-04-13-observation-invariant-rollout-design.md), [2026-04-14-java-entry-point-catalogue.md](2026-04-14-java-entry-point-catalogue.md)

## Goal

Extend the entry-point detection family beyond the already-catalogued Rust and Java slices by naming the Python, Go, and JS-family branches currently shipped in `ProfileAnalyzer::detect_entry_points`.

## Matching Contract

- Python: a file named `__main__.py`, or any Python file whose content contains `if __name__`
- Go: any Go file whose content contains `func main()`
- JS family: `index.js`, `index.ts`, `index.jsx`, `index.tsx`, `main.js`, `main.ts`, `app.js`, and `app.ts` only, with path depth `<= 1`

This catalogue intentionally does not claim support for `main.jsx`, `main.tsx`, `app.jsx`, or `app.tsx`, because the shipped code does not detect those filenames.

## Invariant Catalogue

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-PROFILE-PYTHON-DUNDER-MAIN-ENTRY-POINT` | Python `__main__.py` files are emitted as entry points | `crates/repolens-profile/src/profile.rs::detects_python_dunder_main_entry_point` | `confirmOnly` |
| `I-PROFILE-PYTHON-IF-NAME-ENTRY-POINT` | Python files containing `if __name__` are emitted as entry points | `crates/repolens-profile/src/profile.rs::detects_python_if_name_entry_point` | `confirmOnly` |
| `I-PROFILE-GO-MAIN-ENTRY-POINT` | Go files containing `func main()` are emitted as entry points | `crates/repolens-profile/src/profile.rs::detects_go_main_function_entry_point` | `confirmOnly` |
| `I-PROFILE-JS-FAMILY-ENTRY-POINTS` | The shipped JS/TS/JSX/TSX filename matrix at depth `<= 1` is emitted as entry points | `crates/repolens-profile/src/profile.rs::detects_js_family_entry_points_for_supported_filenames` | `confirmOnly` |
| `I-PROFILE-JS-FAMILY-ENTRY-POINT-EXCLUSIONS` | Deeper paths and unsupported `main.jsx` / `app.tsx` style filenames are not emitted as entry points | `crates/repolens-profile/src/profile.rs::rejects_deep_or_unsupported_js_family_entry_point_candidates` | `confirmOnly` |

## Relationship To Existing Catalogues

Rust remains covered by `I-PROFILE-RUST-BIN-TARGETS`.
Java remains covered by `I-PROFILE-JAVA-MAIN-ENTRY-POINT`.
This file only adds the missing language slices.
