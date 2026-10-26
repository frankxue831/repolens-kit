# Evidence-Aware Findings (Phase 1)

Authority: Supporting
Lifecycle: Living

**Date:** 2026-05-11
**Status:** Draft (Phase 1 implemented)
**Scope:** Trust calibration of the reporting layer — `repolens-profile` health, `repolens-app` summary/recommendations, and the MCP tools that surface them.

## Context

The current reporting layer can make narrow structural observations sound broader than the evidence supports. Import-graph signals, file-layout metrics, and basic repo statistics may be presented with language that implies overall repository "health," "fitness," "quality," "risk," or "readiness." That is dangerous, especially for security-sensitive, cryptographic, embedded, compiler, database, or infrastructure repositories.

This spec introduces a parallel evidence-aware finding model that consumers should prefer over legacy `signals`/`recommendations`/`fitnessScore`/`dependencyRisk`. The legacy fields remain for backwards compatibility but lack per-finding calibration; the response payload now carries a `calibration` block telling consumers exactly that.

## Product north star

Transform RepoLens into a precise, repo-aware structural triage assistant — not a generic repo-quality grader. Core principles:

- **Observe, don't judge.**
- **Evidence over confidence.**
- **Never outrun the evidence.**
- Every finding must communicate: (1) what was observed, (2) what can be responsibly inferred, (3) what is not covered, (4) what should happen next.

## Out of scope (Phase 1)

- Cryptographic correctness or constant-time analysis.
- Fuzzing campaigns, MSRV checking, or no-std target validation.
- Release-readiness, security-audit, or test-pass/fail claims.
- LLM-driven interpretation.
- External command execution (cargo, npm, pip, etc.).
- Rewrites of scan/profile/extract/memory pipelines.
- Breaking pack, MCP, or JSON compatibility.

## Architecture

```
ProjectProfile  ──┐
FileGraph        ─┤
GraphHealthInput ─┼──▶ EvidenceFindingsBuilder ──▶ Vec<EvidenceFinding>
ScanSummary      ─┤    (in repolens-app)
RepoContext      ─┤
CargoLockState   ─┘

Helpers:
  RepoContextDetector       — repolens-profile/src/repo_context.rs
  CargoLockStateDetector    — repolens-profile/src/lockfile.rs
  triage::augment_data      — repolens-mcp/src/triage.rs
```

Findings are populated on `HealthReport.evidenceFindings` and `RepoSummary.evidenceFindings` (additive fields, omitted when empty). MCP responses for `analyze_health` and `get_summary` add three top-level keys to `data`: `triageHeader`, `notChecked`, `calibration`.

## Data model

The four-part calibration block is structural, not stylistic:

```rust
pub struct EvidenceFinding {
    pub id: FindingId,
    pub title: String,
    pub classification: FindingClassification,
    pub severity: FindingSeverity,
    pub knowledge_layer: KnowledgeLayer,
    pub evidence_grade: EvidenceGrade,
    pub confidence: EpistemicConfidence,

    // Four-part calibration:
    pub evidence: Vec<EvidenceItem>,            // (1) what was observed
    pub inference: ResponsibleInference,        // (2) what can be responsibly inferred
    pub limitations: Vec<Limitation>,           // (3) what the evidence does not cover
    pub action: RecommendedAction,              // (4) what should happen next

    pub scope: EvidenceScope,
    pub not_checked: Vec<NotCheckedArea>,
    pub repo_context: Vec<RepoContextTag>,
    pub false_positive_notes: Vec<String>,
    pub provenance: Option<Provenance>,
}
```

`ResponsibleInference` is intentionally separate from `RecommendedAction`: a finding with `action: NoAction` must still emit a non-empty `inference.statement` and at least one `does_not_imply` entry. The four-part shape enforces that observations, inferences, limitations, and actions are reviewable independently.

`EvidenceItem::Observation` is structured (metric name, observed value, source paths, source type) rather than free-form prose. All metric names are drawn from the `metric_names::ALL` constant table; CI tests assert membership.

`RepoContextTag` distinguishes proven project-shape tags (`RustLibrary`, `JavaService`, …) from suggestive `*Indicators` tags (`CryptoIndicators`, `NoStdIndicators`, …). Indicator tags must always be accompanied by a `false_positive_notes` entry naming the literal evidence that matched (dependency name, path hint, etc.).

`CargoLockState` has four explicit reportable variants: `CommittedAndTracked`, `AbsentFromRepo`, `PresentButGitIgnored`, `PresentTrackingUnknown`. RepoLens never runs `cargo`; the `LOCKFILE_GENERATION_OUT_OF_SCOPE` always-shown `NotCheckedArea` reflects that.

## Phase 1 findings

| ID | Trigger | Notes |
|---|---|---|
| `F-NO-ENTRY-POINTS` | `entry_point_count == 0` | `Expected` for libraries; `Informational` for binaries with no detected entry. |
| `F-ORPHAN-FILES` | `orphan_count > 0` after filtering root configs/policy/docs | Above 5% ratio → `Actionable`. |
| `F-DOMINANT-MODULE` | `dominantModuleShare > 0.8` | `Expected` for single-crate libraries and workspace anchors; `Informational` otherwise. |
| `F-HIGH-FAN-IN-FILE` | `inboundEdges > 2.0 × average` | `Expected` for `lib.rs`/`mod.rs`/`prelude.rs`/`*-core/*-util` paths or compiler/infra indicator repos. |
| `F-CARGO-LOCK-STATE` | Rust repo, any state | Dispatch table on `(state, repo shape)`. |
| `F-TEST-INVENTORY-RATIO` | `testToSourceFileRatio < 0.1` AND `sourceFileCount > 5` | Title and detail use "test-file ratio", never "coverage". Inline-Rust caveat attached. |

## "What this scan cannot prove"

The `notChecked` array is split into:

- **General (always shown):** `RUNTIME_BEHAVIOR`, `TEST_PASS_FAIL`, `RELEASE_READINESS`, `SECURITY_AUDIT_GENERIC`, `LICENSE_COMPATIBILITY`, `EXTERNAL_DEPENDENCY_VERSIONS`, `LOCKFILE_FRESHNESS`, `LOCKFILE_GENERATION_OUT_OF_SCOPE`, `CHANGE_SAFETY`.
- **Indicator-conditional:** `CRYPTO_CORRECTNESS`, `CONSTANT_TIME`, `TIMING_LEAKAGE`, `FUZZING_RESULTS`, `MSRV`, `NO_STD_TARGET_VALIDATION`, `MIGRATION_SAFETY`, `ABI_STABILITY`. Each fires only when the relevant `RepoContextTag` is present, so the agent does not see crypto warnings on a CSV parser.

## Compatibility

- All envelope JSON keys are unchanged: `knowledgeLayer`, `confidence`, `evidenceGrade`, `provenance`, `data`.
- Existing pack JSON keys (`structure`, `dependencyRisk`, `signals`, `graphHealth`, `recommendations`, `fitnessScore`, …) are unchanged.
- New JSON keys are camelCase (`evidenceFindings`, `triageHeader`, `notChecked`, `calibration`).
- The `Confidence` envelope enum is renamed to `EpistemicConfidence` in `repolens-core::model::epistemic`. Wire format and the `confidence` field name on `ToolResponse` are unchanged.
- Legacy strings have been softened (e.g. "Low test coverage ratio" → "Low detected test-file ratio"), but the field shapes and identifiers are preserved.

## What this spec does not change

- `analyze_health`, `get_summary`, `compare_summaries`, and `find_dead_code` continue to emit `evidenceGrade: unaudited` at the response envelope level. Per-finding grading inside `data` is the finer signal; promoting the response envelope is a future-phase decision after the per-finding grades are reviewed.

## Phase 2 amendments

Phase 2 extends the evidence-aware path to two more MCP tools and adds the corresponding delta-finding family. All changes are additive:

- **`find_dead_code`**: applies the `is_root_config_or_policy` filter (extracted to `repolens-core::path_classification`) before populating the legacy `deadCodeFiles` / `totalDeadCodeFiles` keys. Adds the triage envelope (`triageHeader`, `notChecked`, `calibration` block listing `deadCodeFiles`/`totalDeadCodeFiles`/`percentDead` as legacy fields) and surfaces `evidenceFindings: [F-ORPHAN-FILES]` alongside.
- **`compare_summaries`**: adds an `evidence_findings: Vec<EvidenceFinding>` field on `SummaryComparison` (additive, defaults to empty), populated by a new `repolens-app::compare_findings::DeltaFindingsBuilder`. Six delta findings emitted when their threshold fires:
  - `F-DELTA-TEST-FILE-RATIO` — change in `scan.testToSourceFileRatio` ≥ 0.05; carries the inline-Rust caveat.
  - `F-DELTA-DOMINANT-MODULE-SHARE` — change in `structure.dominantModuleShare` ≥ 0.05.
  - `F-DELTA-ORPHAN-COUNT` — absolute change in `graph.orphanCount` ≥ 5.
  - `F-DELTA-CYCLE-COUNT` — any non-zero change in `graph.cycleCount`; observation-layer.
  - `F-DELTA-TOTAL-LOC` — relative change in `scan.totalLoc` ≥ 5%.
  - `F-DELTA-FILE-COUNT` — relative change in `scan.fileCount` ≥ 10%.
  Every delta finding carries the `DELTA_REFLECTS_PACK_BOUNDARIES_NOT_COMMIT_DELTAS` and `DELTA_THRESHOLD_HEURISTIC` limitations. The compare_summaries response also gains the triage envelope, with `deltaFitness`/`beforeFitness`/`afterFitness`/`metrics` named in the calibration block as legacy fields.
- New `repolens-core::path_classification` module shared by orphan-file, dead-code, and hub-module detection so the same root/policy filter governs all three.
- New end-to-end integration test (`crates/repolens-app/tests/evidence_findings_e2e_test.rs`) that runs a real `ScanService` against a synthetic Rust library fixture and asserts the four-part shape, forbidden-token absence, and metric-name membership in the constant table.

After Phase 2, the per-finding evidence-aware path covers all four mixed-grade tools (`analyze_health`, `get_summary`, `find_dead_code`, `compare_summaries`). `find_dead_code` and `compare_summaries` keep their `evidenceGrade: unaudited` response envelope grade for the same reason as Phase 1: per-finding grades are the finer signal during Phase-3 review.

## Phase 3 amendments

Phase 3 promotes `find_dead_code`'s response envelope grade from `unaudited` to `derived`. It is the smallest viable Phase-3 increment: every metric in the tool's substantive payload is backed by a doc-06 §5 catalogue, and the tool does not emit the two findings that currently lack catalogue coverage.

### Catalogue-coverage analysis (per-finding metrics → doc-06 §5)

| Metric (from `metric_names::ALL`) | doc-06 §5 family | Catalogue | Today's tier |
|---|---|---|---|
| `graph.orphanCount` / `graph.orphanFiles` / `graph.orphanRatio` | Orphan detection | I-GRAPH-ORPHANS-* (see [orphans catalogue](2026-04-14-graph-declares-cycles-orphans-catalogue.md)) | `confirmOnly` |
| `graph.inboundEdges` / `graph.averageInboundEdges` | Graph import edges | I-GRAPH-EDGE-CLOSURE, I-GRAPH-NODE-ID-UNIQUE, I-GRAPH-NODE-MATCHES-PACK-PATH (see [graph import catalogue](2026-04-14-graph-import-edges-invariant-catalogue.md)) | `confirmOnly` |
| `graph.cycleCount` / `graph.largestCycleSize` | Graph cycles | I-GRAPH-CYCLES-* (see [cycles catalogue](2026-04-14-graph-declares-cycles-orphans-catalogue.md)) | `confirmOnly` |
| `structure.dominantModule` / `structure.dominantModuleShare` / `structure.moduleCount` | Module mapping | I-PROFILE-MODULES-AGGREGATE-TOP-LEVEL-LOC, I-PROFILE-MODULES-SKIP-DOTFOLDERS (see [profile signal catalogue](2026-04-15-profile-signal-catalogue.md)) | `confirmOnly` |
| `profile.entryPointCount` | Entry point detection | I-PROFILE-*-ENTRY-POINT (see [entry point catalogue expansion](2026-04-15-entry-point-catalogue-expansion.md)) | `confirmOnly` |
| `profile.buildSystem` | Build system detection | I-PROFILE-BUILD-SYSTEM-* (see [profile signal catalogue](2026-04-15-profile-signal-catalogue.md)) | `confirmOnly` |
| `delta.scan.totalLoc` | LOC | I-LOC-COUNTS-* (see [LOC family catalogue](2026-04-15-loc-family-catalogue.md)) | `confirmOnly` |
| `delta.scan.fileCount` / `delta.scan.includedCount` | Included file presence | I-FILE-PATH-* (see [grandfathered families catalogue](2026-04-13-grandfathered-families-invariant-catalogue.md)) | `contradictionEligible` |
| `scan.testFileCount` / `scan.sourceFileCount` / `scan.testToSourceFileRatio` | **No catalogue yet** — test-file detection is a new derivation introduced by Phase 1 | — | uncatalogued |
| `profile.lockfileState` | **No catalogue yet** — Cargo.lock presence + .gitignore parse is a new derivation introduced by Phase 1 | — | uncatalogued |

Two families are uncatalogued. Both are introduced by Phase 1 findings and gate the promotion of the tools that emit those findings.

### Per-tool promotion (Phase 3 scope: find_dead_code only)

| Tool | Findings emitted | Legacy fields | Uncatalogued metrics? | Phase 3 status |
|---|---|---|---|---|
| `find_dead_code` | F-ORPHAN-FILES only | `deadCodeFiles` / `totalDeadCodeFiles` / `percentDead` (derived from orphan detection) | none | **promotes to `derived`** |
| `analyze_health` | F-NO-ENTRY-POINTS, F-ORPHAN-FILES, F-DOMINANT-MODULE, F-HIGH-FAN-IN-FILE, F-CARGO-LOCK-STATE, F-TEST-INVENTORY-RATIO | `signals`, `dependencyRisk`, `graphHealth.*` | lockfileState, testFileDetection — **and** `signals` is doc-06 §5 `excluded` | blocked |
| `get_summary` | same as analyze_health (when health tier surfaces) | `recommendations`, `fitnessScore`, `stats.testToSourceRatio`, `health.*` | lockfileState, testFileDetection, plus `excluded`-tier legacy fields | blocked |
| `compare_summaries` | 6 delta findings | `deltaFitness`, `beforeFitness`, `afterFitness`, `metrics`, `recommendations` | F-DELTA-TEST-FILE-RATIO depends on testFileDetection | blocked (one finding) |

`find_dead_code` is the only tool where every observation family in the substantive payload has a passing catalogue per doc 06 §5. The legacy `deadCodeFiles`/`totalDeadCodeFiles`/`percentDead` keys are direct derivations of the catalogued orphan-detection family, so they don't introduce new uncatalogued claims.

### Promotion mechanics

Phase 3 promotes evidence grades at two levels:

**Per-finding `evidence_grade`** on every catalogue-backed finding emitted from `EvidenceFindingsBuilder` and `DeltaFindingsBuilder` flips from `Unaudited` to `Derived`:

| Finding | Metric family | Catalogue (doc-06 §5) |
|---|---|---|
| F-NO-ENTRY-POINTS | Entry-point detection | I-PROFILE-*-ENTRY-POINT |
| F-ORPHAN-FILES | Orphan detection | I-GRAPH-ORPHANS-* |
| F-DOMINANT-MODULE | Module mapping | I-PROFILE-MODULES-* |
| F-HIGH-FAN-IN-FILE | Graph import edges | I-GRAPH-EDGE-CLOSURE etc. |
| F-DELTA-DOMINANT-MODULE-SHARE | Module mapping | I-PROFILE-MODULES-* |
| F-DELTA-ORPHAN-COUNT | Orphan detection | I-GRAPH-ORPHANS-* |
| F-DELTA-CYCLE-COUNT | Graph cycles | I-GRAPH-CYCLES-* |
| F-DELTA-TOTAL-LOC | LOC | I-LOC-COUNTS-* |
| F-DELTA-FILE-COUNT | File presence | I-FILE-PATH-* |

The remaining three findings stay at `Unaudited` until Phase 4 lands their catalogues: F-CARGO-LOCK-STATE (lockfile state), F-TEST-INVENTORY-RATIO (test-file detection), F-DELTA-TEST-FILE-RATIO (same).

**Envelope `evidenceGrade`** on `find_dead_code` flips from `unaudited` to `derived` (one-line swap: `ok_epistemic` → `ok_epistemic_graded(..., EvidenceGrade::Derived, ...)` in `crates/repolens-mcp/src/tools/graph.rs`). The `knowledgeLayer: [observation, heuristic]` mix, the `confidence: codified` value, and the `provenance` array all stay unchanged.

The per-finding promotion is independent of the envelope promotion: in `analyze_health`/`get_summary`/`compare_summaries`, the catalogued findings now also emit `derived` per-finding while the envelope stays `unaudited` — making the finer signal visible inside the response even when the response-level grade is pinned by other constraints. This consistency between nested and top-level grades was the missing piece that the 2026-05-11 first Codex pass on the envelope-only change flagged.

Regression tests on both the envelope (`evidenceGrade == "derived"` for find_dead_code) and on the per-finding grades (`phase3_per_finding_evidence_grades`, `phase3_cargo_lock_stays_unaudited` in `crates/repolens-app/src/evidence_findings/tests.rs`) lock the change in.

### Out of scope for Phase 3 (deferred to Phase 4)

1. **Two new invariant catalogues**, needed before `analyze_health`/`get_summary`/`compare_summaries` can promote:
   - Test-file detection family: invariants over `*_test.rs` / `*Test.java` / `__tests__/*.py` filename patterns per language, fixture parity tests.
   - Lockfile state family: invariants for `Cargo.lock` presence, gitignore parse correctness, the four `CargoLockState` variants.
2. **Per-field grades vs response-level grade**: the three blocked tools all carry doc-06 §5 `excluded`-tier legacy fields (`signals`, `recommendations`, `fitnessScore`). Even after the two new catalogues land, the envelope grade will be pinned to `unaudited` unless either (a) the legacy fields are deprecated/removed, or (b) the envelope adopts per-field grading (doc 06 §"Open Research Questions"). This is a deliberate architectural decision and not a Phase-3 mechanic.

## Phase 4 amendments (Part A — catalogues land)

Phase 4 lands the two missing invariant catalogues identified in Phase 3 §"Out of scope" and uses them to promote the last three per-finding evidence grades from `Unaudited` to `Derived`.

### New catalogues

- **[Test-file detection catalogue](2026-05-11-test-file-detection-invariant-catalogue.md)** — 8 invariants (`I-TEST-DETECT-RUST`, `I-TEST-DETECT-JAVA`, `I-TEST-DETECT-PYTHON`, `I-TEST-DETECT-GO`, `I-TEST-DETECT-JSTS`, `I-TEST-DETECT-PATH-PREFIX`, `I-TEST-DETECT-NO-FALSE-POSITIVE-CONTEST`, `I-TEST-DETECT-FIXTURE-PARITY`) covering the path-string classifier `is_test_file` at `repolens-app/src/summary.rs`. Inline-language tests remain out of scope and surface via the `INLINE_RUST_TESTS_NOT_COUNTED` limitation on consuming findings.
- **[Lockfile-state catalogue](2026-05-11-lockfile-state-invariant-catalogue.md)** — 7 invariants over the four `CargoLockState` variants and the `.gitignore` matcher (`I-LOCKFILE-STATE-ABSENT`, `…-PRESENT-TRACKING-UNKNOWN`, `…-GITIGNORED-UNANCHORED`, `…-GITIGNORED-ANCHORED`, `…-GITIGNORED-TRAILING-SLASH`, `…-COMMENT-NOT-MATCH`, `…-NO-COMMITTED-AND-TRACKED-EMISSION`). The git-index inspection limitation remains explicit via `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL`.

Both catalogues are registered in `docs/doc-registry.yaml` and added as rows in [doc 06 §5](../../philosophy/06-observation-invariants-and-grounding-eligibility.md). All cited invariant tests are green on `main` at amendment time.

### Per-finding promotions (last three)

| Finding | Metric family | New per-finding `evidence_grade` |
|---|---|---|
| F-CARGO-LOCK-STATE | Lockfile state (`profile.lockfileState`) | `Derived` |
| F-TEST-INVENTORY-RATIO | Test-file detection (`scan.testFileCount`, `scan.sourceFileCount`, `scan.testToSourceFileRatio`) | `Derived` |
| F-DELTA-TEST-FILE-RATIO | Test-file detection (delta over `scan.testToSourceFileRatio`) | `Derived` |

After Phase 4 Part A, every Phase-1/Phase-2 evidence finding emits `evidence_grade: Derived`. The per-finding `Unaudited` state is no longer used by any catalogued finding.

### Out of scope for Phase 4 Part A (deferred to Phase 4 Part B)

- **Envelope promotion for `analyze_health` / `get_summary` / `compare_summaries`.** All three tools still carry doc-06 §5 `excluded`-tier legacy fields (`signals`, `recommendations`, `fitnessScore`). The envelope grade stays `unaudited` until either (a) the legacy fields are deprecated, or (b) the envelope adopts per-field grading. This is the architectural decision deferred from Phase 3; it is not in scope here.

### Test summary (Phase 4 Part A)

- 8 new invariant unit tests on `is_test_file` (`crates/repolens-app/src/summary.rs`).
- 2 new invariant unit tests on `CargoLockStateDetector::detect` (`crates/repolens-profile/src/lockfile.rs`) in addition to the 6 existing tests cited in the catalogue.
- 2 new regression tests pinning the per-finding promotion (`phase4_cargo_lock_evidence_grade_derived`, `phase4_test_inventory_evidence_grade_derived` in `crates/repolens-app/src/evidence_findings/tests.rs`).
- 1 assertion added to the existing `test_file_ratio_delta_finding` in `crates/repolens-app/src/compare_findings.rs` pinning F-DELTA-TEST-FILE-RATIO at `Derived`.
- The `phase3_cargo_lock_stays_unaudited` test is removed (it pinned the old state that this amendment changes).

### Test summary

The new test (`find_dead_code_envelope_grade_promoted_to_derived` in `crates/repolens-mcp/tests/integration_test.rs`) asserts `envelope["evidenceGrade"] == "derived"` for a real `find_dead_code` call against a scanned repo. Existing envelope-shape tests for `find_dead_code` are unchanged; the promotion is additive.

## Phase 4 amendments (Part B Step 1 — retire fitnessScore)

Phase 4 Part A landed the two missing invariant catalogues and promoted the last three per-finding evidence grades to `Derived`. The remaining blocker for envelope promotion on `analyze_health` / `get_summary` / `compare_summaries` is that those responses still carry doc-06 §5 `excluded`-tier legacy fields (`signals`, `recommendations`, `fitnessScore`). Phase 4 Part B starts retiring those fields incrementally; **Step 1 retires `fitnessScore`**.

### Why fitnessScore first

`fitnessScore` is the most egregious overclaim in the response surface:

- It is a single composite number (or four-number breakdown) that purports to summarise an entire repo's "fitness." That collapse from a multi-dimensional structural snapshot to a single score is exactly the "outrun the evidence" pattern the trust-calibration spec exists to prevent.
- It has no doc-06 §5 catalogue and no plausible path to one — the "fitness" weighting (0.5 × resolutionRate + 0.3 × couplingQuality + 0.2 × noiseReduction) is an arbitrary heuristic with no operational meaning.
- It is the simplest field to retire: nothing downstream depends on it beyond `compare_summaries`'s `deltaFitness` derivative (which is retired in the same change).
- Removing it aligns with RepoLens's agent-focused product north star — composite "quality scores" are a human-facing summary pattern, not an agent decision input.

### What this step removes

From the wire format:
- `get_summary` (compact/standard/full tiers): the top-level `fitnessScore` field.
- `compare_summaries`: the top-level `beforeFitness`, `afterFitness`, and `deltaFitness` fields.
- The calibration block on both tools no longer lists those names as legacy fields (because they are no longer emitted).
- The `repolens compare-summaries` CLI markdown renderer no longer includes the "Fitness Scores" section or the "Δ fitness" line.

From the code:
- `crates/repolens-core/src/model/summary.rs::FitnessScore` struct — retired entirely.
- `RepoSummary.fitness_score` field — removed.
- `SummaryComparison.before_fitness` / `after_fitness` / `delta_fitness` fields — removed.
- `crates/repolens-app/src/summary.rs::FitnessScorer` — retired entirely along with `score_with_graph` and the three scorer unit tests.
- `ComparisonService::compare_with_pack_ids` and `ComparisonService::compare` change their return type from `Option<SummaryComparison>` to `SummaryComparison` — they were Option-returning only to signal a missing fitness score, which no longer applies. `metrics` and `evidence_findings` are computed unconditionally now.
- The `repolens compare-summaries` CLI no longer errors out with "one or both packs lack fitness scores."

### What this step does NOT do

- Does not promote any envelope grade. `analyze_health` / `get_summary` / `compare_summaries` all still emit `unaudited` envelopes because `signals`, `recommendations`, and `health.*` legacy content is still in the response.
- Does not touch `signals` or `recommendations`. Those are larger surfaces (`signals` is the Java-parity health output; `recommendations` is a richer object) and warrant separate Phase 4 Part B steps.

### Test summary (Phase 4B Step 1)

- 4 retired tests (`fitness_score_round_trip`, `fitness_scorer_basic`, `fitness_scorer_cycle_penalty`, `fitness_scorer_cycle_penalty_capped`) — the behaviour they pinned is no longer emitted.
- `compare_summaries_detects_delta` updated to assert that `beforeFitness`/`afterFitness`/`deltaFitness` are now ABSENT and `metrics` is still present.
- `compare_summaries_emits_triage_envelope_and_calibration` updated to assert the new calibration legacy list (`metrics`, `recommendations`) and the absence of `deltaFitness`.
- `get_summary_calibration_calls_out_stats_and_graph_health` updated to assert the absence of `fitnessScore` from the calibration legacy list.
- `render_markdown_produces_stable_block_for_fixture` (CLI) updated to assert no "Fitness Scores" / "Δ fitness" appear in the markdown output.
- `compare_summaries_emits_stable_markdown_for_two_hand_built_packs` (CLI) updated the same way.

## Phase 4 amendments (Part B Step 2 — retire signals + add F-DEPENDENCY-CYCLES, F-LOCKFILE-MISSING-NON-CARGO)

Step 1 retired `fitnessScore`. **Step 2 retires the legacy `signals` field** from `analyze_health` and `get_summary`'s `health.signals` block. The pre-retirement code emitted up to seven signals: three from `graph_health` (`dependency-cycles`, `orphan-files`, `low-test-ratio`) and four from `compute_signals` (`no-lockfile`, `dominant-module`, `no-entry-points`, `high-framework-count`). Mapping each to a calibrated replacement:

| Signal | Replacement | Status before this step |
|---|---|---|
| `dependency-cycles` | **F-DEPENDENCY-CYCLES** | new in this step |
| `orphan-files` | F-ORPHAN-FILES | existed |
| `low-test-ratio` | F-TEST-INVENTORY-RATIO | existed |
| `no-lockfile` (Cargo) | F-CARGO-LOCK-STATE | existed |
| `no-lockfile` (non-Cargo: npm, poetry, etc.) | **F-LOCKFILE-MISSING-NON-CARGO** | new in this step |
| `dominant-module` | F-DOMINANT-MODULE | existed |
| `no-entry-points` | F-NO-ENTRY-POINTS | existed |
| `high-framework-count` | (intentionally dropped — see below) | — |

### Why signals second

- Each signal is a composite severity-tagged judgment (`severity: "low"/"medium"/"high"`) with no per-finding inference / limitations / does-not-imply discipline. Agents reading them cannot tell what a "medium" cycle signal means or whether the threshold reflects compile-time risk, refactoring cost, or something else.
- The information each signal carried is already in the catalogue (after this step adds F-DEPENDENCY-CYCLES): structural cycles → F-DEPENDENCY-CYCLES, orphan files → F-ORPHAN-FILES, low test ratio → F-TEST-INVENTORY-RATIO.
- The `low-test-ratio` signal duplicates the same number F-TEST-INVENTORY-RATIO calibrates with a `does_not_imply` block that explicitly disclaims coverage.
- Retiring `signals` lets the next step (retire `recommendations`) and the final step (promote envelope grades) proceed without `signals` muddying the response.

### Why `high-framework-count` is intentionally dropped

The retired signal fired at `framework_count > 5` with a "may increase maintenance burden and security surface area" rationale. The threshold (`> 5`) has no doc-06 §5 catalogue and no operational meaning — five frameworks may be conservative for a polyglot service or excessive for a microlib. The composite judgment is exactly the excluded-tier overclaim pattern Phase 4B exists to retire. The underlying observation is preserved as `dependencyRisk.frameworkCount` so agents can apply their own threshold if useful.

### What F-DEPENDENCY-CYCLES looks like

- `id`: `F-DEPENDENCY-CYCLES`
- `fires when`: `graph_health.cycle_count > 0`
- `severity`: derived from the existing `cycle_severity(cycle_count, largest_cycle_size)` heuristic (≥ 6 cycles or > 6 files → high; ≥ 3 cycles or > 3 files → medium; otherwise low) → mapped to `FindingSeverity::{High, Medium, Low}`.
- `classification`: `Actionable`
- `knowledge_layer`: `Heuristic` (severity tier uses thresholds)
- `evidence_grade`: `Derived` (heuristic severity over verified inputs)
- `evidence`: structural observation of `graph.cycleCount` carrying `invariant_id: I-GRAPH-CYCLES-EXCLUDE-DECLARES` (the same invariant F-DELTA-CYCLE-COUNT references), plus a heuristic-judgment item recording the severity rule.
- `inference.does_not_imply`: `the code fails to compile`, `cycles cause runtime problems`, `cycles must be removed` — calibration agents need to distinguish structural import cycles from compile or runtime failure.
- `limitations`: `IMPORT_GRAPH_STRUCTURAL_ONLY` (newly added) — cycles in build-time-only, type-level, or reflection-mediated relationships are out of scope.
- `action`: `ReviewIfMaintainerPain`

### What F-LOCKFILE-MISSING-NON-CARGO looks like

- `id`: `F-LOCKFILE-MISSING-NON-CARGO`
- `fires when`: `dependency_risk.build_system.is_some()` AND `build_system != "cargo"` (case-insensitive) AND `dependency_risk.lockfile_present == false`.
- `severity`: `Low` (composite "build system without lockfile" suggestion, not a verified-tier finding).
- `classification`: `Actionable`
- `knowledge_layer`: `Heuristic`
- `evidence_grade`: `Unaudited` — the cross-language lockfile-state catalogue does not yet exist in doc-06 §5. F-CARGO-LOCK-STATE is backed by the Cargo-specific lockfile-state invariant catalogue; a future PR can generalise and promote this finding once the catalogue extends to npm / poetry / go.sum / etc.
- `evidence`: `profile.buildSystem` observation + heuristic-judgment item recording the "build system implies lockfile expectation" rule.
- `does_not_imply`: `the project is broken`, `the project is misconfigured`, `a lockfile would resolve all reproducibility concerns`.
- `limitations`: `LIVE_ROOT_VS_PACK_TIME_DIVERGENCE_POSSIBLE` (lockfile presence is read from the pack's file inventory; a lockfile added/removed in the working tree after the scan is not reflected).
- `action`: `ReviewIfMaintainerPain`

### What this step removes

From the wire format:
- `analyze_health`: the `signals` field (and its `Signal[]` payload).
- `get_summary` at `detail: full`: `health.signals`.
- The calibration block on both tools no longer lists `signals` / `health.signals` as legacy fields.

From the code:
- `crates/repolens-core/src/model/health.rs::Signal` struct — retired entirely.
- `HealthReport.signals` field — removed.
- `crates/repolens-profile/src/health.rs::HealthAnalyzer::analyze` no longer takes a `&FileGraph` parameter (graph-derived observations now flow through `EvidenceFindingsBuilder`). The `compute_signals` helper and the post-analyze `signals` push blocks are removed.
- The `cycle_severity` function stays — it is now consumed by `F-DEPENDENCY-CYCLES` (via `repolens_app::evidence_findings`) instead of the retired signal builder. This keeps the severity rule in one place and preserves the existing unit-test coverage.

### What this step does NOT do

- Does not promote any envelope grade. `analyze_health` / `get_summary` / `compare_summaries` still emit `unaudited` envelopes because `recommendations`, `dependencyRisk`, and `graphHealth.*` legacy content remains.
- Does not touch `recommendations`. Step 3 will retire that.
- Does not retire `dependencyRisk`. The composite `riskLevel: "low"/"medium"/"high"` field is structurally similar overclaim, but its observation-tier siblings (`buildSystem`, `lockfilePresent`, `frameworkCount`, `missingVersionCount`) are useful raw observations that an agent may legitimately consume. A follow-up step can wrap or trim it separately.

### Test summary (Phase 4B Step 2)

- 5 retired tests in `repolens-profile/src/health.rs` (`dominant_module_signal_fires_above_80_percent`, `test_health_with_cycles_signal`, `test_health_without_graph_input`, `test_health_orphan_signal_threshold`, `test_health_low_test_ratio_signal`) — the behaviours they pinned are no longer emitted; the calibrated versions are tested in `evidence_findings/tests.rs`.
- 1 new replacement test in `health.rs` (`graph_health_carried_through_to_report`) — verifies `graph_health` round-trips through the report so the evidence-findings builder can still consume it.
- 4 new unit tests in `evidence_findings/tests.rs` for `F-DEPENDENCY-CYCLES`: skipped when cycle_count is zero, fires with `Low` severity for small counts, escalates to `High` for ≥ 6 cycles, and references `I-GRAPH-CYCLES-EXCLUDE-DECLARES`.
- 5 new unit tests for `F-LOCKFILE-MISSING-NON-CARGO`: fires for npm without lockfile, skipped for Cargo (F-CARGO-LOCK-STATE covers it), skipped when lockfile is present, skipped when no build system is detected, skipped when `dependency_risk` is not threaded through (no panic from old call sites).
- `analyze_health_returns_structure_and_risk` updated to assert that `signals` is ABSENT and `evidenceFindings` is present.
- `analyze_health_calibration_calls_out_graph_health_metrics` updated to assert `signals` is not in the calibration legacy list.
- `get_summary_calibration_calls_out_stats_and_graph_health` updated to assert `health.signals` is not in the calibration legacy list.

### Phase 4B Step 2 follow-up — F-HIGH-FAN-IN-FILE aggregation (2026-05-13)

The 2026-05-12 dogfood against the sibling `gm-crypto-rs` repo surfaced that `F-HIGH-FAN-IN-FILE` was emitting one finding per high-fan-in file (10 separate findings on gm-crypto-rs, all `severity=none classification=expected`). That shape diverges from the sibling per-file finding `F-ORPHAN-FILES`, which aggregates orphans into one finding with a `sample` array of paths. The duplication is pure noise in the agent's context — every per-path finding has identical inference and limitations, differing only by which path is named.

This follow-up brings `F-HIGH-FAN-IN-FILE` in line with the `F-ORPHAN-FILES` shape:

- One finding when ≥1 path exceeds the threshold (post-cap), else none.
- `scope.paths` lists every high-fan-in path (sorted by inbound desc; capped at `HIGH_FAN_IN_MAX_FINDINGS=10`).
- The `graph.inboundEdges` observation's `observedValue` is `{sample: [{path, inboundEdges}, ...], sampleComplete: bool}` — the same shape used by `F-ORPHAN-FILES`'s `graph.orphanFiles` observation.
- Worst-case classification across the kept set: any non-indicator/non-core path makes the finding `actionable/medium/reviewIfMaintainerPain`. Otherwise `expected/none/noAction`. The HeuristicJudgment's `observedValue` reports the largest inbound count currently in the kept set.
- `inference.dependsOn` records `match:coreModulePath` when any indicator path is present and `match:repoIndicatorTag` when the repo is compiler/infra-tagged; the threshold dep is always recorded.
- The `FAN_IN_FINDINGS_CAPPED` limitation fires once on the aggregate (not per-path) when total above-threshold > cap, and discloses the omitted count.

Test summary for the follow-up:
- `high_fan_in_caps_findings_and_discloses_omission` updated to assert one finding with `scope.paths.len() == 10` and a single `FAN_IN_FINDINGS_CAPPED` limitation (was: 10 findings each carrying the limitation).
- New: `high_fan_in_aggregates_multiple_files_into_one_finding` — 5 non-indicator hubs above threshold → exactly 1 finding with `scope.paths.len() == 5`, no cap limitation.
- New: `high_fan_in_mixed_indicator_classified_actionable_with_all_paths` — 1 indicator hub + 1 non-indicator hub → 1 actionable finding containing both paths, `dependsOn` records `match:coreModulePath`.

## Phase 4 amendments (Part B Step 3 — retire recommendations)

Step 1 retired `fitnessScore`; Step 2 retired `signals`. **Step 3 retires the legacy `recommendations` field** from `RepoSummary` and the legacy `recommendations: Vec<RecommendationDelta>` field from `SummaryComparison`. The pre-retirement code emitted seven recommendation rules; each maps to a calibrated replacement or is dropped outright:

| Recommendation rule | Replacement evidence finding | Disposition |
|---|---|---|
| `high-coupling-file` | F-HIGH-FAN-IN-FILE | covered |
| `low-test-ratio` | F-TEST-INVENTORY-RATIO | covered |
| `no-lockfile` (Cargo) | F-CARGO-LOCK-STATE | covered |
| `no-lockfile` (non-Cargo) | F-LOCKFILE-MISSING-NON-CARGO | covered |
| `dominant-module` | F-DOMINANT-MODULE | covered |
| `dependency-cycles` | F-DEPENDENCY-CYCLES | covered |
| `large-file` (LOC > 500) | — | **dropped (no replacement)** |
| `low-resolution-rate` | — | **dropped (no replacement)** |

### Why `large-file` is intentionally dropped

The retired recommendation's own suggested-action text said "Review only if the file is hard to navigate or change. Large file size is not a defect by itself." That sentence is the spec's own admission that the rule is non-actionable. An agent reading a file with `read_file_content` already sees its LOC; surfacing the same number as a finding adds nothing actionable and crowds the response. If a future use case surfaces — e.g., a delta-only "this file grew by 30%" signal during compare — it can be added as a calibrated finding (`F-LARGE-FILE-DELTA`) in a follow-up PR rather than carried forward as a per-pack noise source.

### Why `low-resolution-rate` is intentionally dropped

This rule reported RepoLens's own extractor coverage ("only X% of import relationships were resolved"), not a fact about the repository. It is tooling transparency, not a repo finding — the correct home for it is `calibration.notChecked`, not a fingerprinted recommendation in the response body. The not-checked surface already names per-language limitations; if a quantitative coverage callout is needed later it can be a `NOT_CHECKED` code or a dedicated finding `F-EXTRACTOR-COVERAGE-LOW`, but that decision is out of scope for Step 3.

### What this step removes

From the core model (`crates/repolens-core/src/model/summary.rs`):
- The `Recommendation` struct and `RecommendationDelta` struct — retired entirely.
- `RepoSummary.recommendations: Vec<Recommendation>` — removed.
- `SummaryComparison.recommendations: Vec<RecommendationDelta>` — removed.
- The `recommendation_optional_fields` unit test (pinned the `Recommendation` serialization shape, no longer relevant).

From the orchestration layer (`crates/repolens-app/src/summary.rs`):
- The `RecommendationBuilder` struct and its `build` method (the seven rule blocks).
- The recommendation-delta computation inside `ComparisonService::compare_with_pack_ids`.
- The `read_graph_metrics` helper (its only consumer was the `low-resolution-rate` rule).
- Four per-rule unit tests (`recommendation_high_coupling`, `recommendation_low_test_ratio`, `recommendation_dependency_cycles`, `recommendation_no_cycles_when_zero`) — their behaviours are pinned by the evidence-finding equivalents in `evidence_findings/tests.rs`.

From the MCP layer (`crates/repolens-mcp/src/tools/summary.rs`):
- `top3_recs` / `top5_recs` / `top10_recs` in the `build_compact` / `build_standard` / `build_full` response shapes.
- `"totalRecommendations"` and the `"truncated": { "recommendations": ... }` block in the `_meta` envelope.
- `"recommendations"` from the calibration `legacyFields` list at both `get_summary` and `compare_summaries`.

From the CLI layer (`crates/repolens-cli/`):
- `canonicalize_recommendations` helper and `RecommendationDelta` import in `compare.rs`.
- The markdown "Recommendations" section in `render_comparison_markdown`.
- Two unit tests pinning the markdown render of that section.
- The integration assertion that `### Recommendations` appears in the CLI compare output (replaced with an inverted assertion that it must NOT appear).

### What this step does NOT do

- Does not promote any envelope grade. `analyze_health` / `get_summary` / `compare_summaries` still emit `unaudited` envelopes because `dependencyRisk` and `graphHealth.*` legacy fields remain.
- Does not touch `dependencyRisk` or the `graphHealth` block. Those are covered by F-LOCKFILE-MISSING-NON-CARGO / F-CARGO-LOCK-STATE and F-ORPHAN-FILES / F-DEPENDENCY-CYCLES respectively, but the raw legacy keys remain in the response for now; a follow-up step can wrap or trim them once the envelope promotion criteria are agreed.
- Does not add `F-LARGE-FILE` or `F-EXTRACTOR-COVERAGE-LOW`. Either can land as a follow-up PR if dogfood surfaces real need.

### Test summary (Phase 4B Step 3)

- 1 retired test in `repolens-core` (`recommendation_optional_fields`).
- 4 retired tests in `repolens-app/src/summary.rs` (the per-rule recommendation tests listed above).
- 2 retired tests in `repolens-cli/src/compare.rs` (`canonicalize_sorts_by_action_then_id_then_file_none_first`, `render_markdown_empty_recommendation_sections_show_none_placeholder`).
- 1 updated test in `repolens-cli/tests/compare_summaries_cli_test.rs` (`compare_summaries_emits_stable_markdown_for_two_hand_built_packs`) — assertion inverted: `### Recommendations` must NOT appear.
- 4 updated calibration assertions across `repolens-mcp/tests/integration_test.rs` and `repolens-mcp/tests/triage_envelope_test.rs` — `recommendations` is now asserted to be absent from `calibration.legacyFields` at every detail tier on `get_summary` and on `compare_summaries`.
- `triage.rs` unit tests rewritten to use `metrics` / `health.dependencyRisk` as the present-field exemplars instead of the retired `recommendations` / `signals`.

## Phase 4 amendments (Part B Step 4 — retire dependencyRisk/graphHealth/metrics.trend + promote envelopes)

Phase 4B Step 3 retired the last user-facing interpretive field (`recommendations`) but stopped short of envelope grade promotion because the response payload still carried two raw-counter blocks (`dependencyRisk`, `graphHealth.*`) and one direction-blind label field (`metrics.trend` on `compare_summaries`). Phase 4B Step 4 (2026-05-13) finishes the retirement sequence and lifts the envelope grade on the last three mixed-grade tools from `unaudited` to `derived`.

### What this step does

**1. Retire `health.dependencyRisk` from the `analyze_health` and `get_summary` response shape.**

The block carried five fields, four of which were already catalogued elsewhere and one of which was the excluded-tier heuristic that doc 06 §5 calls out by example:

| Field | Replacement |
|-------|-------------|
| `buildSystem` | `profile.buildSystem.name` (`I-PROFILE-BUILD-SYSTEM-*` catalogue, confirmOnly) |
| `frameworkCount` | `profile.frameworks.length` (`I-PROFILE-FRAMEWORK-*` catalogue, confirmOnly) |
| `missingVersionCount` | Stubbed to `0`, never populated — dead field. |
| `lockfilePresent` | `lockfileState` block + F-CARGO-LOCK-STATE / F-LOCKFILE-MISSING-NON-CARGO (`I-LOCKFILE-STATE-*` catalogue, confirmOnly) |
| `riskLevel` | **Retired entirely.** The three-bucket `low`/`medium`/`high` classifier was the exact excluded-tier "threshold without calibration" pattern doc 06 §5 cites as `Health signals` — the thresholds (`framework_count > 2`, `> 3`) had no operational meaning. |

`HealthReport.dependency_risk` stays on the in-memory struct because `EvidenceFindingsBuilder` consumes it for F-LOCKFILE-MISSING-NON-CARGO; what is retired is the response-level serialization. The on-disk pack format is untouched.

**2. Retire `health.graphHealth` from the `analyze_health` and `get_summary` response shape.**

Every field under `graphHealth` was duplicated by a catalogued evidence finding that already carries the same number plus per-finding calibration:

| Raw field | Calibrated equivalent |
|-----------|----------------------|
| `cycleCount` / `largestCycleSize` / `filesInCyclesCount` | F-DEPENDENCY-CYCLES |
| `orphanCount` / `orphanFiles` | F-ORPHAN-FILES |
| `testToSourceRatio` | F-TEST-INVENTORY-RATIO |

`HealthReport.graph_health` stays on the in-memory struct (consumed by `EvidenceFindingsBuilder`). What is retired is the response-level serialization.

**3. Retire `MetricDelta.trend` on `compare_summaries`.**

The `trend` string carried `"improved"` / `"degraded"` / `"unchanged"` labels computed mechanically from `after > before` / `after < before`. These labels are direction-blind: a LOC reduction was labelled `degraded`, which inverts the intent of most refactor diffs (the 2026-05-12 dogfood D3 finding). The replacement is implicit — agents that need a directional read-out derive it locally from `before` / `after`, and the calibrated F-DELTA-* findings carry thresholds.

`MetricDelta` survives as `{name, before, after}` — pure verified-observation. The CLI `compare-summaries` markdown render drops its `Trend` column accordingly.

**4. Promote envelope grade on `analyze_health` / `get_summary` / `compare_summaries` to `derived` — conditional on payload contents.**

With the excluded-tier `dependencyRisk.riskLevel` and the direction-blind `metrics.trend` gone, and with the remaining `graphHealth` raw-counter duplication retired in favour of catalogued findings, the *substantive* payload in all three tools is doc 06 §5 catalogue-backed (confirmOnly or higher). The candidate envelope grade is therefore `Derived`. But the envelope contract is that mixed responses use the lowest grade represented in the payload — so the envelope falls back to `Unaudited` when either:

* any emitted per-finding `evidence_grade` is `Unaudited` — currently this is `F-LOCKFILE-MISSING-NON-CARGO` for non-Cargo build systems (npm/Go/Python lacking lockfiles), which the heuristic-grade comment in `crates/repolens-app/src/evidence_findings/lockfile_missing.rs` pins to `Unaudited` until cross-language lockfile-state invariants land;
* `memoryInsights` is non-empty on `get_summary` — memory store content has no doc 06 §5 grade, so the surrounding `Interpretation` layer cannot implicitly inherit `Derived`.

The pattern mirrors `find_dead_code`'s conditional grading at `crates/repolens-mcp/src/tools/graph.rs:801-805` (which falls back to `Unaudited` when `graph.json` is missing). The shared helper `crate::epistemic::min_envelope_grade(candidate, &findings, has_ungraded_interpretation)` lives in `crates/repolens-mcp/src/epistemic.rs`; tools call it before passing the result to `ok_epistemic_graded`. Adoption of the helper is the greppable change that captures the contract.

The envelope's mixed `[Observation, Heuristic]` (or `+ Interpretation` when memory is present) `knowledgeLayer` and `Codified` `confidence` are unchanged.

### What this step removes

From the MCP layer (`crates/repolens-mcp/src/`):
- `tools/profile.rs::handle_analyze_health`: the `serde_json::to_value(&report)` path is replaced with a tailored shape (`{structure, evidenceFindings}`). `dependencyRisk` and `graphHealth` no longer appear in the response. The `legacyFields` argument to `augment_data` becomes `&[]`.
- `tools/summary.rs::handle_get_summary`: `build_standard`'s `health_partial` drops the `dependencyRisk` key. `build_full`'s `health_full` switches from full `serde_json::to_value(h)` to `{structure, evidenceFindings}`. The `legacyFields` argument becomes `&[]`.
- `tools/summary.rs::handle_compare_summaries`: `legacyFields` argument becomes `&[]`.
- All three handlers: `ok_epistemic` → `ok_epistemic_graded(..., EvidenceGrade::Derived, ...)`.

From the core/orchestration layer:
- `MetricDelta.trend: String` field is removed.
- `ComparisonService::make_delta` no longer computes a trend label.

From the CLI layer (`crates/repolens-cli/`):
- `render_comparison_markdown`'s `trend_glyph` helper and the `Trend` table column.

### What this step does NOT do

- Does not touch the on-disk pack format. `project-profile.json` still carries `dependency_risk` and `graph_health` because `HealthAnalyzer` still computes them; only the MCP response shape changes.
- Does not retire `MetricDelta` itself. The `{name, before, after}` payload remains a verified-observation tabular view of pack metric deltas.
- Does not promote any envelope to `Verified`. The `[Observation, Heuristic]` mix in these tools — where evidence findings carry their own per-finding severity heuristics — keeps them at `Derived`.
- Does not add `F-LARGE-FILE-DELTA` / `F-EXTRACTOR-COVERAGE-LOW`. Both remain follow-up candidates if dogfood surfaces real need.

### Test summary (Phase 4B Step 4)

- 1 updated unit test in `repolens-app/src/summary.rs` (`comparison_detects_changes`) — assertion switches from `trend == "improved"` to direct `before` / `after` value checks.
- 1 updated unit test in `repolens-cli/src/compare.rs` (`render_markdown_produces_stable_block_for_fixture`) — `Trend` column / glyphs asserted absent.
- 7 updated/added tests in `repolens-mcp/tests/integration_test.rs`:
  - `analyze_health_returns_structure_and_evidence_findings` (renamed from `analyze_health_returns_structure_and_risk`) — asserts `dependencyRisk`, `graphHealth`, `signals` absent; `structure`, `evidenceFindings` present.
  - `analyze_health_envelope_grade_promoted_to_derived` (new, replaces `analyze_health_calibration_calls_out_graph_health_metrics`) — pins envelope to `derived` on a fixture without unaudited findings and `legacyFields` to `[]`.
  - `analyze_health_envelope_falls_back_when_unaudited_finding_present` (new) — npm-without-lockfile fixture triggers `F-LOCKFILE-MISSING-NON-CARGO`; envelope must fall back to `unaudited`.
  - `get_summary_full_detail_omits_retired_legacy_keys` (renamed/rewritten from `get_summary_calibration_calls_out_stats_and_graph_health`) — asserts retired keys absent at full and `legacyFields` empty.
  - `get_summary_standard_detail_omits_retired_legacy_keys` (rewritten from the PR-A-era equivalent) — same shape at standard.
  - `get_summary_compact_detail_omits_retired_legacy_keys` (rewritten from the PR-A-era equivalent) — same shape at compact.
  - `get_summary_envelope_grade_promoted_to_derived` (new) — pins envelope to `derived` across all three detail tiers on a fixture without unaudited findings or memory.
- 1 updated test in `repolens-mcp/tests/integration_test.rs` (`compare_summaries_emits_triage_envelope_and_calibration`) — asserts `legacyFields` empty and envelope `derived`.
- 4 new unit tests in `repolens-mcp/src/epistemic.rs` (`min_envelope_grade_*`) cover the helper: candidate-only, unaudited fallback, all-findings-higher pass-through, ungraded-interpretation override.
- `MetricDelta` round-trip test in `repolens-core` updated to drop `trend` from the fixture.

## Cross-references

- `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` §5 — health signals are tier `excluded`. This spec keeps that posture for legacy `signals` and adds the calibrated path.
- `docs/philosophy/07-current-state-vs-direction.md` — canonical "shipped today" reference; update when this spec lands.
- `docs/superpowers/specs/2026-04-11-epistemic-envelope-design.md` — original envelope design.
- `docs/superpowers/specs/2026-04-06-graph-health-signals-design.md` — original graph-health signal design (now the legacy path).

## Test summary

Unit tests cover all six finding emitters, the four-part calibration invariants, the `metric_names` table, the indicator-conditional `notChecked` filter, and the legacy-fields call-out in `calibration`. An insta snapshot test locks down the consumer-visible triage envelope shape for both a pure-Rust-library repo and a crypto-indicator repo.
