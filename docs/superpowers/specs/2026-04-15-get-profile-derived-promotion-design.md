# get_profile Derived Promotion Design

**Date:** 2026-04-15
**Status:** Draft
**Depends on:** [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [../../philosophy/07-current-state-vs-direction.md](../../philosophy/07-current-state-vs-direction.md), [../../philosophy/notes/2026-04-14-arc-review-and-next-steps.md](../../philosophy/notes/2026-04-14-arc-review-and-next-steps.md), [2026-04-14-language-and-profile-catalogue.md](2026-04-14-language-and-profile-catalogue.md), [2026-04-14-java-entry-point-catalogue.md](2026-04-14-java-entry-point-catalogue.md)

## Goal

Promote `get_profile` from `evidenceGrade: "unaudited"` to `evidenceGrade: "derived"` without shrinking the current `ProjectProfile` surface.

The point of this slice is not to make profile analysis more ambitious. The point is to make the existing shipped payload trustworthy enough to grade honestly: every field that `get_profile` relies on should either be backed by named tests and cited contract rows, be a mechanical consequence of already-backed inputs, or stay explicitly outside the promotion.

## Why This Slice

The current roadmap orders the remaining work this way:

1. close the profile-family catalogue gaps
2. close the missing non-Rust/non-Java entry-point tests
3. then promote `get_profile`

That ordering is recorded explicitly in [2026-04-14-arc-review-and-next-steps.md](../../philosophy/notes/2026-04-14-arc-review-and-next-steps.md). The project has already finished the self-bootstrap runtime arc, so the next highest-value step is to make the first mixed-layer profile tool calibrated instead of leaving it in the default unaudited bucket.

## Current State

`handle_get_profile` still returns the default unaudited envelope through `ok_epistemic(...)` in `crates/repolens-mcp/src/tools/profile.rs`.

`ProjectProfile` currently contains these fields:

- `name`
- `buildSystem`
- `frameworks`
- `projectType`
- `entryPoints`
- `description`
- `languageDistribution`
- `modules`

The blocking trust gaps are not uniform across that payload:

- `name` already has a named family in doc 06 via the project-profile-name catalogue.
- `entryPoints` has partial catalogue coverage today: Rust and Java are covered, Python/Go/JS-family are not.
- `buildSystem` has only partial direct branch coverage today: Maven, npm, and the no-build-system case are tested, while Gradle, Cargo, Go, and Python manifest detection are still uncited.
- `frameworks` has only partial direct branch coverage today: React is tested, while Express, Spring Boot, Spring, Django, and Flask are still uncited.
- `modules` and `languageDistribution` both depend on `FileEntry.loc`, and `modules` also emits LOC directly through `ModuleInfo.loc`. Doc 07 still treats `loc` as an uncatalogued observation family, so neither field can be treated as harmless background data in a tool-level promotion.
- `projectType` is computed from `frameworks` and `entryPoints` in `ProfileAnalyzer::classify_project_type`, so it can ride on those families instead of needing its own independent family in this slice.
- `description` is currently hardcoded to `None` by `ProfileAnalyzer::analyze`; it is not a live detection surface in this code path.

## Scope

In scope:

- Add direct `repolens-profile` unit tests for the uncovered Python, Go, and JS-family entry-point branches.
- Add direct `repolens-profile` unit tests for every emitted build-system branch: Maven, Gradle, npm, Cargo, Go, Python, and the no-build-system case.
- Add direct `repolens-profile` unit tests for every emitted framework branch: React, Express, Spring Boot, Spring, Django, and Flask.
- Add named catalogue coverage for:
  - build system detection
  - framework detection
  - module mapping
  - Python entry-point detection
  - Go entry-point detection
  - JS-family entry-point detection
- Resolve the LOC blocker for `get_profile` by either:
  - adding a named LOC family strong enough to justify `languageDistribution` and `ModuleInfo.loc`, or
  - trimming those LOC-backed fields from the graded surface before the promotion flip
- Expand doc 06 so those families are visible in the trust table instead of living only as uncited tests.
- Flip `get_profile` to `EvidenceGrade::Derived`.
- Add MCP-level regression coverage that asserts the envelope grade for `get_profile`.
- Update shipped-vs-direction narration to reflect the new shipped state.

Out of scope:

- Promoting `analyze_health`, `get_summary`, `compare_summaries`, or `find_dead_code`.
- Introducing per-field `evidenceGrade`.
- Tightening the production heuristics beyond what the code already does today.
- Reworking the `ProjectProfile` wire format.
- Solving the `loc` family question globally beyond what `get_profile` needs.

## Recommended Approach

Keep the current broad `entryPoints` API and close the missing evidence gaps rather than narrowing the contract to Rust+Java only.

That is the better path for three reasons:

1. it preserves the shipped surface instead of creating a smaller graded contract than the live tool already returns
2. the missing language slices and detector branches are cheap to cover because the current behavior is already implemented in `ProfileAnalyzer`
3. it avoids another narration mismatch where the tool still emits broad profile data but the trust docs only half-endorse it

The design therefore assumes the honest promotion path is:

1. close every emitted detector branch for `buildSystem`, `frameworks`, and `entryPoints`
2. resolve the LOC-backed payload blocker for `languageDistribution` and `modules`
3. only then flip `get_profile` to `derived`

## Architecture

This arc stays split across three layers.

### 1. Behavioral truth lives in `repolens-profile`

`ProfileAnalyzer` remains the single source of behavioral truth for:

- build-system detection
- framework detection
- module mapping
- entry-point detection
- project-type classification

The first work in this arc is therefore test-first work in `crates/repolens-profile/src/profile.rs`. The missing gaps are direct unit-test gaps, not MCP integration gaps.

The production detector semantics should stay as they are today:

- Python: `__main__.py` or content containing `if __name__`
- Go: content containing `func main()`
- JS-family: `index.js`, `index.ts`, `index.jsx`, `index.tsx`, plus `main.js`, `main.ts`, `app.js`, and `app.ts` at depth `<= 1`

This slice should test those rules directly, including at least one negative JS-family depth case. It should not silently tighten the semantics to "real entry points" without an explicit separate design.

### 2. Trust contract lives in `docs/philosophy` and `docs/superpowers/specs`

The promotion should be justified by explicit contract rows, not by "we have some tests somewhere."

This requires three paperwork additions:

- a new profile-signals catalogue covering `buildSystem`, `frameworks`, and `modules`
- a new entry-point expansion catalogue covering the missing Python, Go, and JS-family slices while leaving the existing Rust and Java catalogues in place
- a LOC-family decision and corresponding trust paperwork for the `get_profile` payload, because `languageDistribution` and `ModuleInfo.loc` cannot ride along as if LOC were already catalogued

Doc 06 then needs trust-table expansion:

- add rows for build-system detection, framework detection, and module mapping
- update the entry-point row so it no longer lists Python, Go, and JS-family as uncovered gaps
- either add a LOC row that justifies the profile payload’s LOC-backed fields, or explicitly record that the grade flip is blocked until such a row exists

The catalogue text should stay honest about the nature of each detector. These are lexical or path-shape contracts, not semantic parsers.

### 3. MCP owns the final grade flip and regression guard

Only after the tests and docs land should `crates/repolens-mcp/src/tools/profile.rs` switch from:

- `ok_epistemic(...)`

to:

- `ok_epistemic_graded(..., EvidenceGrade::Derived, ...)`

This should land with an envelope-level regression test, not only a comment. The MCP test suite already has envelope helpers for other promoted tools; `get_profile` should get the same treatment so future edits cannot silently demote or over-promote it.

## Payload Boundary

This design intentionally treats `ProjectProfile` fields in three groups.

### Explicit blockers in this slice

- `buildSystem`
- `frameworks`
- `modules`
- `entryPoints`
- `languageDistribution`
- the underlying LOC family needed to justify `modules.loc` and `languageDistribution`

These are the fields this design treats as blockers for an honest tool-level promotion.

### Fields that ride on existing or newly-backed inputs

- `name`
- `projectType`

`name` is already covered by the existing project-profile-name family. `projectType` is a derived classification over `frameworks` and `entryPoints`, so this slice accepts it as downstream of those families rather than opening a separate catalogue family.

### Fields deliberately not reopened here

- `description`

`description` is statically `None` in the live analyzer path today, so it is not a live detector surface for this promotion.

## Components and Likely Files

Expected files in the implementation plan:

| File | Responsibility |
|---|---|
| `crates/repolens-profile/src/profile.rs` | Add direct tests for Python, Go, and JS-family entry-point detection; add discoverable spec comments if needed |
| `crates/repolens-mcp/src/tools/profile.rs` | Flip `get_profile` to `ok_epistemic_graded(..., EvidenceGrade::Derived, ...)` |
| `crates/repolens-mcp/src/lib.rs` | Add `call_get_profile_envelope` helper if the MCP tests need one |
| `crates/repolens-mcp/tests/integration_test.rs` | Add regression test asserting `get_profile` emits `derived` |
| `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` | Add trust-table rows for build system / frameworks / modules; update entry-point row |
| `docs/philosophy/07-current-state-vs-direction.md` | Record that `get_profile` is now a mixed-layer tool that emits `derived` |
| `docs/philosophy/notes/2026-04-14-arc-review-and-next-steps.md` | Mark the `get_profile` blocker slice as landed or partially landed |
| `docs/superpowers/specs/2026-04-15-profile-signal-catalogue.md` | New catalogue for build system / frameworks / modules |
| `docs/superpowers/specs/2026-04-15-entry-point-catalogue-expansion.md` | New catalogue for Python / Go / JS-family entry points |
| `docs/superpowers/specs/2026-04-15-loc-family-catalogue.md` | New or revised LOC-family paperwork if the promotion keeps LOC-backed fields in `get_profile` |

The exact catalogue filenames can change during planning, but the design assumes at least three focused specs rather than cramming all profile fields into the older language-and-profile document.

## Data Flow

The promoted flow should remain simple:

1. `PackResolver` resolves the pack.
2. `ProfileAnalyzer::analyze` builds `ProjectProfile`.
3. MCP serializes that payload unchanged.
4. The handler emits `knowledgeLayer = [observation, heuristic]`, `confidence = codified`, and `evidenceGrade = derived`.

No new runtime pipeline is needed. The change is about proving the current pipeline is catalogue-backed, not replacing it.

## Error Handling

This slice should not change `get_profile` error behavior.

Resolution failures and serialization failures should keep returning the existing flat error JSON. The trust work here is on successful responses only. Any attempt to bundle envelope-error work into this arc should be rejected as scope creep.

## Testing Strategy

The implementation plan should include all of these:

### Direct profile tests

Add direct tests for:

- Python `__main__.py`
- Python file containing `if __name__`
- Go file containing `func main()`
- JS-family positive cases that match the shipped filenames:
  - `index.js`
  - `index.ts`
  - `index.jsx`
  - `index.tsx`
  - `main.js`
  - `main.ts`
  - `app.js`
  - `app.ts`
- JS-family negative case rejected when the path is too deep

These tests should exercise the current lexical and path-shape rules, not a stronger semantic contract than production code actually implements.

### Existing profile-family regression runs

Keep the already-relevant tests green for:

- every emitted build-system variant
- every emitted framework branch
- module mapping
- language distribution
- project-type classification

### LOC-backed payload verification

Before the MCP grade flip, the implementation plan must prove one of these two states:

- LOC is now catalogued strongly enough for `languageDistribution` and `ModuleInfo.loc` to participate in a `derived` tool response, or
- the payload has been narrowed so those LOC-backed fields are no longer part of what `get_profile` emits under the promoted envelope

If neither state is true, the promotion step must not happen.

### MCP envelope regression test

Add an integration-level assertion that `get_profile` emits:

- `evidenceGrade == "derived"`

and still returns a profile-shaped `data` payload.

### Final verification

At minimum, the implementation plan should finish with:

- targeted `cargo test` runs for the new `repolens-profile` cases
- `cargo test -p repolens-profile`
- relevant `repolens-mcp` integration tests
- `cargo fmt --all`
- `cargo clippy` on touched crates with warnings denied

## Risks and Guardrails

1. **Overstating semantics.** The detectors are mostly lexical. The specs must not claim AST-precise or framework-semantic understanding that the code does not implement.
2. **LOC-backed payload blocker.** `languageDistribution` and `ModuleInfo.loc` are not safely promotable until the LOC family question is resolved for this payload. If the LOC work does not land, the grade flip must stop.
3. **Promotion without regression guard.** The grade flip must land with an MCP-level envelope test.
4. **Doc drift after promotion.** `07-current-state-vs-direction.md` must be updated in the same arc so shipped narration matches the new behavior immediately.

## Success Criteria

This design is complete when all of the following are true:

- `get_profile` keeps its current response shape
- Python, Go, and JS-family entry-point branches have direct tests
- every emitted `buildSystem` and `frameworks` branch has direct coverage
- build system, framework detection, and module mapping are cited as named trust-table families
- the LOC-backed fields in `get_profile` are either honestly catalogued or explicitly removed from the promoted surface
- doc 06 no longer lists non-Rust/non-Java entry-point gaps for `get_profile`
- `get_profile` emits `evidenceGrade: "derived"` in MCP tests
- shipped-vs-direction docs describe that state accurately
