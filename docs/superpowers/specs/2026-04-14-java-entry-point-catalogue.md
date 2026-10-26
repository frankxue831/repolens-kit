# Java Entry Point Catalogue

**Date:** 2026-04-14
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [2026-04-13-observation-invariant-rollout-design.md](2026-04-13-observation-invariant-rollout-design.md)

## Goal

Extend the entry-point detection family's named-invariant coverage from Rust (`I-PROFILE-RUST-BIN-TARGETS`, shipped in the first rollout) to an initial Java slice. This closes one slice of the non-Rust entry-point gap surfaced by the derived-batch review in `docs/superpowers/plans/2026-04-14-derived-tool-promotions.md` without over-promising: Python, Go, and the JS family remain uncovered and are tracked as open gaps below.

This is a paperwork commit. No new test logic. The underlying Java detector has been in place since the profile crate first shipped; this catalogue binds the direct `profile.rs` unit test to a named invariant. Additional Java end-to-end assertions already exist in `crates/repolens-core/tests/profile_contract_test.rs` and `crates/repolens-app/src/scan.rs` — those broader checks are untouched.

## Scope

In scope:

- A single named invariant `I-PROFILE-JAVA-MAIN-ENTRY-POINT` citing the direct Java unit test at `crates/repolens-profile/src/profile.rs::i_profile_java_main_entry_point`.
- Doc 06 §5 "Entry point detection" row updated to cite both `I-PROFILE-RUST-BIN-TARGETS` and the new ID.
- Module-level doc comment on `profile.rs` tests pointing at this spec so the lockstep maintenance rule is grep-discoverable.

Out of scope:

- Python, Go, JavaScript, TypeScript, JSX, TSX, or any other language. No direct unit tests exist for those branches in `profile.rs` today. Adding them requires writing new unit tests first, which is a separate effort.
- Tool promotions. `analyze_health` and `find_dead_code` remain at `evidenceGrade: "unaudited"` because their payloads still contain fields outside this catalogue (non-Java entry points for mixed repos, severity thresholds, bare symbols, `loc`).
- Any change to `ProfileAnalyzer::detect_entry_points` production code.
- Any tightening of Java detection beyond the current substring match. If we want exact signature matching, that is a separate implementation + negative-test plan.

## Matching Contract

The Java entry-point detection contract, as implemented by `ProfileAnalyzer::detect_entry_points` and directly tested:

> For any included `FileEntry` with `language == "Java"` whose content contains the substring `public static void main`, the file appears in `ProjectProfile.entry_points` with its repo-relative path.

This is intentionally a lexical contract, not an AST-level or signature-exact claim. The catalogue must not claim `String[] args` specifically, because the current implementation does not parse or validate the full Java `main` signature. That limitation is acceptable for a `confirmOnly` family but must stay explicit in the spec text. If future work adds support for `MANIFEST.MF`-declared `Main-Class` entries, Spring Boot `@SpringBootApplication` classes, or exact-signature parsing, those are separate invariants and should be catalogued alongside.

## Invariant

| ID | Property | Evidence | Tier |
|---|---|---|---|
| `I-PROFILE-JAVA-MAIN-ENTRY-POINT` | For a Java file containing the substring `public static void main`, `ProfileAnalyzer::analyze` reports the file as an entry point with its repo-relative path | `crates/repolens-profile/src/profile.rs::i_profile_java_main_entry_point` | `confirmOnly` |

### Maintenance Rule

If `i_profile_java_main_entry_point` is deleted, renamed, or `#[ignore]`d, the "Entry point detection" family in doc 06 §5 MUST drop its Java citation. Reviewers of any PR touching `ProfileAnalyzer::detect_entry_points` or the Java detection logic must verify the test remains green.

## Known Gaps

1. **Python, Go, and the JS family (`JavaScript`, `TypeScript`, `JSX`, `TSX`) are uncovered.** `ProfileAnalyzer::detect_entry_points` has code paths for each of these languages but no direct unit tests. Until tests are written, these branches have no named invariants, and any tool whose response depends on their entry-point detection remains at `evidenceGrade: "unaudited"`. Each language slice deserves its own follow-up catalogue with tests designed against realistic fixtures.
2. **Java coverage is lexical only.** The direct unit test proves the current substring matcher, not exact Java `main(String[] args)` semantics. No negative test distinguishes a true main-method signature from a broader false-positive occurrence of `public static void main`. If stronger semantics are desired, implementation and negative tests come first, then the catalogue text can be tightened.
3. **Java-specific alternative entry points are not catalogued.** Spring Boot `@SpringBootApplication`, JAR `MANIFEST.MF` `Main-Class`, and multi-module Maven `<mainClass>` entries are not tested here and therefore not catalogued. If production code already supports any of these, they should be added in a follow-up; if not, implementation comes first.

## File Plan

| File | Change |
|---|---|
| `crates/repolens-profile/src/profile.rs` | Rename the existing Java entry-point test to `i_profile_java_main_entry_point`; add doc comment above the test with the invariant ID; add a module-level doc comment at the top of the `#[cfg(test)]` module pointing at this spec. |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Update the §5 "Entry point detection" row's coverage column to cite both `I-PROFILE-RUST-BIN-TARGETS` and `I-PROFILE-JAVA-MAIN-ENTRY-POINT`; add this spec to `Depends on`. |
| `docs/superpowers/specs/2026-04-14-java-entry-point-catalogue.md` | This spec. |

## Verification

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p repolens-profile i_profile_java_main_entry_point -- --exact
cargo test -p repolens-profile
cargo clippy -p repolens-profile --tests -- -D warnings
```

The renamed test must be green. No other tests should regress.

Manual verification: `rg -n "I-PROFILE-JAVA-MAIN-ENTRY-POINT" docs/philosophy/ crates/repolens-profile/` — both roots should return at least one match so the invariant ID is discoverable from both the contract doc and the code.
