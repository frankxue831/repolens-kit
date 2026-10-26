# Language Classification and Project Profile Name Invariant Catalogue

**Date:** 2026-04-14
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-14-graph-declares-cycles-orphans-catalogue.md](2026-04-14-graph-declares-cycles-orphans-catalogue.md)

## Goal

Bind the `language classification` and `project profile name` families in doc 06 §5 to named invariant IDs. After this spec and the later Task-4 paperwork additions, every family in doc 06 §5 at `confirmOnly` or higher has a named catalogue, and rubric §6 requirement 5 is fully satisfied across the whole table.

The existing coverage for these two families is already substantial: language classification is backed by the `lang_test.rs` unit suite plus fixture parity, and project-profile naming now has direct runtime coverage for the shipped `package.json`, root `Cargo.toml`, Cargo-workspace, and generic unknown-fallback branches in `repolens-profile`. The gap was only that doc 06 §5 described the coverage in prose rather than cite named invariants.

This catalogue uses the same mixed evidence style as the surrounding invariant paperwork: stable test-name citations are primary, and the evidence cells cite the shipped tests by `path::test_name`. Grouping existing tests under family-level invariant IDs keeps the paperwork proportional: six invariants for language, six for profile, no test renames.

## Scope

In scope:

- Catalogue spec listing 6 language-classification invariants and 6 project-profile-name invariants, citing the existing language tests plus the direct name-resolution tests in `repolens-profile`.
- Doc 06 §5 update: refresh the `Language classification` and `Project profile name` rows to cite the new invariant IDs.

Out of scope:

- Any rename of existing tests.
- Any runtime behavior change. This spec binds already-shipped coverage to named invariants; it does not change the detector logic.
- The `confirmOnly` → `contradictionEligible` promotion path. Neither family is on the resolver's critical path and neither participates in grounding today.

## Invariant Catalogue

All twelve invariants are green on `2026-04-17`.

### Language Classification Family

The matching contract: `repolens_extract::lang::detect(path)` is deterministic, case-insensitive, and maps 60+ file extensions to a stable language string. `FileEntry.language` in a pack must equal `detect(FileEntry.path)` for every included entry.

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-LANG-CORE-EXTENSIONS` | `detect()` returns the correct language for canonical source-code extensions across Java, JavaScript (`.js/.jsx/.mjs/.cjs`), TypeScript (`.ts/.tsx/.mts`), Python, Rust, Go, C/C++ (`.c/.h/.cpp/.cc/.cxx/.hpp`), Shell (`.sh/.bash/.zsh`), and Kotlin (`.kt/.kts`) | `crates/repolens-extract/tests/lang_test.rs::detects_java`, `::detects_javascript_variants`, `::detects_typescript_variants`, `::detects_python`, `::detects_rust`, `::detects_go`, `::detects_c_and_cpp`, `::detects_shell_variants`, `::detects_kotlin_variants` | `confirmOnly` |
| `I-LANG-MARKUP-AND-CONFIG` | `detect()` classifies common markup, config, and web-framework file types (`.md`, `.html`, `.json`, `.yaml`, `.xml`, `.toml`, `.vue`, `.svelte`, `.css`, `.scss`, `.less`) | `crates/repolens-extract/tests/lang_test.rs::detects_markup_and_config`, `::detects_web_frameworks`, `::detects_css_variants` | `confirmOnly` |
| `I-LANG-MISC-SUPPORT` | `detect()` also handles the long tail: Scala, Dart, Zig, R, Lua, PHP, Java `.properties`, Groovy, Ruby, Swift | `crates/repolens-extract/tests/lang_test.rs::detects_misc_languages` | `confirmOnly` |
| `I-LANG-UNKNOWN-FALLBACK` | `detect()` returns `"Unknown"` for unrecognized extensions and for files with no extension at all, rather than panicking or defaulting to a wrong language | `crates/repolens-extract/tests/lang_test.rs::unknown_extension`, `::no_extension` | `confirmOnly` |
| `I-LANG-CASE-INSENSITIVE` | `detect()` is case-insensitive over file extensions (`.JAVA` and `.Py` resolve identically to `.java` and `.py`) | `crates/repolens-extract/tests/lang_test.rs::case_insensitive` | `confirmOnly` |
| `I-LANG-FIXTURE-PARITY` | For every captured contract scenario, `detect(entry.path)` equals the `entry.language` stored in the pack. This is the end-to-end invariant: extension mapping is stable across scanner runs and does not drift between writer and reader | `crates/repolens-extract/tests/fixture_parity_test.rs::language_detection_matches_golden_for_all_scenarios` | `confirmOnly` |

### Project Profile Name Family

The matching contract: `ProfileAnalyzer::detect_name` resolves `project-profile.json:name` from `package.json`'s `name` field when present, otherwise from Cargo `[package]` metadata (including root-package and workspace-member selection), and otherwise falls back to `"unknown"`. This catalogue documents the directly tested Rust detector slices named below: npm name resolution plus the audited Cargo package paths.

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-PROFILE-NAME-NPM` | `package.json` with a `name` field resolves `project-profile.json:name` from that field | `crates/repolens-profile/src/profile.rs::detects_npm_build_system` | `confirmOnly` |
| `I-PROFILE-NAME-CARGO-ROOT-PACKAGE` | A root `Cargo.toml` with `[package] name = "..."` resolves `project-profile.json:name` from the root package section | `crates/repolens-profile/src/profile.rs::detects_cargo_root_package_name` | `confirmOnly` |
| `I-PROFILE-NAME-CARGO-WORKSPACE` | A Cargo workspace root without `[package]` resolves `name` from a member crate's `[package]` section | `crates/repolens-profile/src/profile.rs::detects_cargo_workspace_name_from_member_package` | `confirmOnly` |
| `I-PROFILE-NAME-CARGO-WORKSPACE-BINARY-PREFERENCE` | When multiple member crates exist, a member with `src/main.rs` wins over an alphabetical library-only crate | `crates/repolens-profile/src/profile.rs::cargo_workspace_prefers_binary_crate_over_alphabetical_library` | `confirmOnly` |
| `I-PROFILE-NAME-CARGO-WORKSPACE-FALLBACK` | A workspace root with no usable member package name falls back to `"unknown"` rather than inventing a name | `crates/repolens-profile/src/profile.rs::cargo_workspace_without_member_falls_back_to_unknown` | `confirmOnly` |
| `I-PROFILE-NAME-GENERIC-UNKNOWN-FALLBACK` | A repo with neither `package.json` nor usable Cargo `[package]` metadata falls back to `"unknown"` rather than inventing a name | `crates/repolens-profile/src/profile.rs::detects_unknown_name_when_no_package_json_or_cargo_package_exists` | `confirmOnly` |

## Known Gaps

1. **No round-trip test for either family.** `detect()` is deterministic; `ProfileAnalyzer::detect_name` is deterministic-on-inputs. Neither is tested against serde drift (would a serialize → deserialize → re-detect cycle preserve the value?). Given both values are plain strings, the risk is low. Acknowledged.

2. **Language classification is not tested against ambiguous double extensions.** `.tar.gz` resolves to `"Unknown"` today by virtue of `.gz` not being in the map, but that is implicit rather than asserted. Low priority — double-extension files are rare in source trees.

These two gaps do not block the rubric. The family requirement is "at least one named invariant", and both families are well above that threshold.

## Maintenance Rule

If any cited test above is deleted, renamed, `#[ignore]`d, or weakened, the affected family drops a tier (`confirmOnly` → `excluded`) in doc 06 §5 and the responsible PR must either add replacement coverage in the same commit or demote the row.

## Rubric-Requirement-5 Final Tally

After this spec:

| Family | Tier | Named invariants |
|---|---|---|
| Included file presence | `contradictionEligible` | 4 ✅ |
| Exact qualified symbols | `contradictionEligible` | 4 ✅ |
| File content extraction | `contradictionEligible` | 5 ✅ |
| Graph import edges | `confirmOnly` | 5 ✅ |
| Graph declares edges | `confirmOnly` | 3 ✅ |
| Graph cycles | `confirmOnly` | 4 ✅ |
| Orphan detection | `confirmOnly` | 5 ✅ |
| Build system detection | `confirmOnly` | 7 ✅ |
| Framework detection | `confirmOnly` | 6 ✅ |
| Profile language distribution | `confirmOnly` | 1 ✅ |
| Entry point detection | `confirmOnly` | 7 ✅ |
| Incremental manifest metadata | `confirmOnly` | 1 ✅ |
| Module mapping | `confirmOnly` | 2 ✅ |
| **Language classification** | `confirmOnly` | **6 ✅** (this spec) |
| **Project profile name** | `confirmOnly` | **6 ✅** (this spec) |

**15 of 15.** Every family in doc 06 §5 at `confirmOnly` or higher now has a named invariant catalogue. Rubric §6 requirement 5 is fully satisfied. The remaining family in §5 (`health signals`, the sole `excluded`-with-purpose row) is explicitly exempt by the rubric.

## File Plan

| File | Change |
|---|---|
| `crates/repolens-profile/src/profile.rs` | Maintain the module-level citation block linking the direct Cargo name-resolution tests back to this catalogue. |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Update §5 rows for `Language classification` and `Project profile name` to cite the new invariant IDs; keep dependencies aligned with shipped specs only. |
| `docs/superpowers/specs/2026-04-14-language-and-profile-catalogue.md` | This spec. |

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-extract --test lang_test
cargo test -p repolens-extract --test fixture_parity_test language_detection_matches
cargo test -p repolens-profile
```

All cited tests must remain green.
