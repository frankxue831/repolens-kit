# 06 — Observation Invariants and Grounding Eligibility

Authority: Canonical
Lifecycle: Living

**Status**: Draft
**Date**: 2026-04-13
**Depends on**: [03-deep-reasons.md](03-deep-reasons.md), [05-structure-and-tensions.md](05-structure-and-tensions.md), [2026-04-05-grounded-memory-design.md](../superpowers/specs/2026-04-05-grounded-memory-design.md), [2026-04-11-epistemic-envelope-design.md](../superpowers/specs/2026-04-11-epistemic-envelope-design.md), [2026-04-13-grandfathered-families-invariant-catalogue.md](../superpowers/specs/2026-04-13-grandfathered-families-invariant-catalogue.md), [2026-04-13-file-content-invariant-catalogue.md](../superpowers/specs/2026-04-13-file-content-invariant-catalogue.md), [2026-04-14-graph-import-edges-invariant-catalogue.md](../superpowers/specs/2026-04-14-graph-import-edges-invariant-catalogue.md), [2026-04-14-graph-declares-cycles-orphans-catalogue.md](../superpowers/specs/2026-04-14-graph-declares-cycles-orphans-catalogue.md), [2026-04-14-language-and-profile-catalogue.md](../superpowers/specs/2026-04-14-language-and-profile-catalogue.md), [2026-04-14-java-entry-point-catalogue.md](../superpowers/specs/2026-04-14-java-entry-point-catalogue.md), [2026-04-15-profile-signal-catalogue.md](../superpowers/specs/2026-04-15-profile-signal-catalogue.md), [2026-04-15-entry-point-catalogue-expansion.md](../superpowers/specs/2026-04-15-entry-point-catalogue-expansion.md), [2026-04-15-loc-family-catalogue.md](../superpowers/specs/2026-04-15-loc-family-catalogue.md)

## Motivation

Path B's foundation shipped in April 2026: `repolens-app/src/grounding.rs`, grounding-on-recall in `recall_longterm`, and `GroundingStatus` values surfaced in memory-tool responses. That foundation intentionally stops short of the opinion pool's full roadmap — high-stakes on-scan checking, configurable decay for contradicted nodes, absence-as-signal, and broader grounding beyond Phase 1's exact-anchor scope. Those future phases will expand **which observations are allowed to flag memory as `CONTRADICTED`**.

Without an explicit contract governing that expansion, RepoLens is one naive PR away from letting a buggy observation overwrite a correct interpretation. This document specifies the contract. It separates two questions that are currently conflated:

- **Observation soundness.** Does the extraction pipeline produce observations that match the code it read from? A property of individual observations.
- **Grounding eligibility.** Which observation families are permitted to contradict memory, which may only confirm it, and which must be excluded from grounding altogether? A policy over families, not individuals.

The first question is answered by invariants. The second is answered by a tiered eligibility scheme. Both must be articulated before Path B phases 2+ land, because together they are the admission criteria for memory-eroding power.

## Why This Matters: Evidence From Self-Scan

In April 2026, a self-scan of `repolens-rs` run by the same tool surfaced three observation bugs in a single session. Each bug was mechanical — produced by code, not by heuristic judgment. Each carried `confidence: mechanical` in the epistemic envelope. None was guarded by an invariant suite that would have caught the failure mode.

| Bug | Consequence under naive Path B | Status |
|---|---|---|
| `declares` edges were counted in SCC detection, producing a false 15-file cycle in `repolens-extract` | A memory asserting "repolens-extract has no internal cycles" would be flagged `CONTRADICTED` by a wrong observation | Fixed in `68845c5` |
| Rust binary entry points were not detected at all | A memory anchored to `crates/repolens-cli/src/main.rs` as the binary entry would appear stale | Fixed in `70fdfa5` |
| Cargo workspace project name resolved to `"unknown"` | A memory tagged with the actual project name would never match observed metadata | Fixed in `68f27e9` |

The findings above come from a self-bootstrap session dated 2026-04-12, in which RepoLens scanned its own repository and the resulting pack was audited by hand. All three bugs were fixed in that same session.

**Correction note — 2026-04-13.** An earlier revision of this document listed a fourth bug: "incremental-scan manifests dropped summary fields (`repoRoot`, `fileCount`, `languageCounts`, etc.)." Writing the first real invariant against the actual `Manifest` struct (`crates/repolens-core/src/model/manifest.rs`) revealed that those field names were never in the schema — they were misread as "dropped" from a `jq` output showing absent keys. The bug was phantom: an ungrounded observation about the observation layer, smuggled into the motivation section of a document whose purpose is to prevent exactly that failure mode. The incremental manifest does retain its real schema fields under incremental scans, and the `I-SCAN-INCR-MANIFEST-CORE` invariant added in commit `16a4591` now guards that property. The phantom bug has been removed from the table above and from §5. This correction is itself evidence that the contract is needed: even motivated, attentive authors produce mechanical-looking claims that decay on contact with an invariant. The lesson generalizes — the value of an invariant is not that it catches code bugs, it is that it catches *claims* about code.

The lesson is not that the observation layer is bad. It is that **"produced mechanically" and "verified correct" are different claims, and the current envelope conflates them.** Path B amplifies this conflation: a mechanically-produced, unverified observation that contradicts a memory will eventually accelerate that memory's decay. Without a contract, wrong observations outlive correct memories.

This is the operational reading of [05-structure-and-tensions.md §"The Implication for Humans"](05-structure-and-tensions.md): "the quality of observation has ethical weight." Ethical weight, to be actionable, needs a contract.

## Two Contracts, Separable

### Observation Invariants

An **observation invariant** is a checkable property of the extraction pipeline. It takes the form "for every *X* produced by RepoLens, *Y* must hold." Examples:

- For every `graph.edges[].source`, the source path is present in `graph.nodes[].id`.
- For every Rust `declares` edge, a `mod <name>;` AST node exists in the source file.
- Every `FileEntry.path` is a relative path within the scan root.
- An incremental-scan manifest carries the same top-level summary fields as a full-scan manifest for the same scope.

Invariants live as tests — property-based assertions, golden-fixture diffs, or targeted unit tests. They have two states: passing or failing. A failing invariant is a bug, not a quality signal.

Invariants are a property of the **extraction pipeline**, not of individual observations. When an invariant is green, the entire family of observations it guards can be trusted to that degree. When red, no observation in that family should carry verification-grade confidence until the regression is fixed.

### Grounding Eligibility

A **grounding eligibility tier** is a policy over an observation family. It declares what the family is permitted to do when memory is recalled:

| Tier | May confirm memory | May contradict memory | Used as evidence at all |
|---|---|---|---|
| `contradictionEligible` | yes | yes | yes |
| `confirmOnly` | yes | no | yes |
| `excluded` | no | no | no |

Eligibility is a property of the *family*, not of an individual observation. "The file-presence family is `contradictionEligible`" is a statement about how RepoLens treats any observation in that family, regardless of the specific `FileEntry` being checked.

**Default posture.** New observation families are `excluded` until invariant coverage exists. They become `confirmOnly` once invariants are written and passing. They are promoted to `contradictionEligible` only by an explicit spec change that cites the invariant suite. Widening eligibility is deliberate and documented. Narrowing eligibility (after a discovered false positive) can happen at any time.

### Why Separate Them

An observation can be sound (passes its invariants) and still not be eligible for contradiction rights. A cycle count computed from a resolver with full invariant coverage might still be denied contradiction rights if the SCC filter or severity thresholds have latent design bugs that no invariant guards. Soundness is a property of production; eligibility is a policy of trust. Conflating them collapses a useful degree of freedom — specifically the freedom to write invariants first and grant contradiction rights second.

## What Phase 1 Already Enforces

The Phase 1 grounded-memory spec encodes eligibility rules implicitly, as scope restrictions. Each restriction below maps to a tier or a safety default.

| Phase 1 restriction | Implicit rule |
|---|---|
| Groundable anchors limited to exact file paths and exact qualified symbols | Bare names and free-form strings are `excluded` from grounding |
| Evidence source is `PackIndex` only, never `graph.json` | Graph-derived observations are `excluded` until graph invariants exist |
| `GroundingStatus::Unchecked` is defined broadly, not narrowly | Unknown anchor formats default to `excluded`, not `contradictionEligible` |
| Missing pack returns `Unverifiable`, not `Contradicted` | Absence of evidence is not contradiction |
| No decay acceleration based on grounding in Phase 1 | Contradiction carries no operational consequence yet; eligibility is in dry-run |

Phase 1 is safe precisely because it restricted scope. This document generalizes what the restriction was doing so that later phases can relax scope deliberately.

## Observation Families, Graded Today

The table below catalogues RepoLens's current observation families, their evidence source, whether any invariant suite currently exists, and a recommended starting tier. These tiers are the document's proposed initial state — the boundary from which future phases negotiate upward — not a final decree.

**Grandfathering clause retired — 2026-04-13.** Three families — *included file presence in the active pack*, *exact qualified symbols*, and *file content extraction* — were originally adopted at `contradictionEligible` without a rubric-conforming catalogue. The original grandfathering clause undercount the affected set as two; closing the third family surfaced the omission. The two follow-up specs are [2026-04-13-grandfathered-families-invariant-catalogue.md](../superpowers/specs/2026-04-13-grandfathered-families-invariant-catalogue.md) (file presence + qualified symbols, eight invariants) and [2026-04-13-file-content-invariant-catalogue.md](../superpowers/specs/2026-04-13-file-content-invariant-catalogue.md) (file content extraction, five invariants). All three families now meet §3's promotion rule through their named catalogues. The clause is retired; the three families no longer sit outside the rule.

| Family | Example | Current invariant coverage | Proposed starting tier |
|---|---|---|---|
| Included file presence in the active pack | `FileEntry.path` appears in the resolved pack's included entries | `I-FILE-PATH-RELATIVE`, `I-FILE-PATH-FORWARD-SLASH`, `I-FILE-PATH-UNIQUE`, `I-FILE-PATH-SERDE-ROUNDTRIP` (see [invariant catalogue](../superpowers/specs/2026-04-13-grandfathered-families-invariant-catalogue.md)) | `contradictionEligible` |
| Exact qualified symbols (groundable subset) | `com.example.AuthService` in `FileEntry.symbols` | `I-SYMBOL-QNAME-NONEMPTY`, `I-SYMBOL-QNAME-UNIQUE-IN-FILE`, `I-SYMBOL-QNAME-TRIMMED`, `I-SYMBOL-QNAME-SERDE-ROUNDTRIP` (see [invariant catalogue](../superpowers/specs/2026-04-13-grandfathered-families-invariant-catalogue.md)) | `contradictionEligible` |
| Bare symbol names | `Baz` | None; name collisions not checked | `excluded` |
| File content extraction | `read_file_content` output | `I-FILE-CONTENT-INDEX-COMPLETE`, `I-FILE-CONTENT-EXCLUDED-NO-CONTENT`, `I-FILE-CONTENT-PATH-PREFIX`, `I-FILE-CONTENT-PRESENT-ON-DISK`, `I-FILE-CONTENT-SHA256-PARITY` (see [file content catalogue](../superpowers/specs/2026-04-13-file-content-invariant-catalogue.md)) | `contradictionEligible` |
| Language classification | `FileEntry.language` | `I-LANG-CORE-EXTENSIONS`, `I-LANG-MARKUP-AND-CONFIG`, `I-LANG-MISC-SUPPORT`, `I-LANG-UNKNOWN-FALLBACK`, `I-LANG-CASE-INSENSITIVE`, `I-LANG-FIXTURE-PARITY` (see [language and profile catalogue](../superpowers/specs/2026-04-14-language-and-profile-catalogue.md)) | `confirmOnly` (exotic extensions can misclassify) |
| Build system detection | `buildSystem` | `I-PROFILE-BUILD-SYSTEM-MAVEN`, `I-PROFILE-BUILD-SYSTEM-GRADLE`, `I-PROFILE-BUILD-SYSTEM-NPM`, `I-PROFILE-BUILD-SYSTEM-CARGO`, `I-PROFILE-BUILD-SYSTEM-GO`, `I-PROFILE-BUILD-SYSTEM-PYTHON`, `I-PROFILE-BUILD-SYSTEM-NONE` (see [profile signal catalogue](../superpowers/specs/2026-04-15-profile-signal-catalogue.md)) | `confirmOnly` |
| Framework detection | `frameworks: [...]` | `I-PROFILE-FRAMEWORK-REACT`, `I-PROFILE-FRAMEWORK-EXPRESS`, `I-PROFILE-FRAMEWORK-SPRING-BOOT`, `I-PROFILE-FRAMEWORK-SPRING`, `I-PROFILE-FRAMEWORK-DJANGO`, `I-PROFILE-FRAMEWORK-FLASK` (see [profile signal catalogue](../superpowers/specs/2026-04-15-profile-signal-catalogue.md)) | `confirmOnly` |
| Profile language distribution | `languageDistribution` | `I-PROFILE-LANGUAGE-DISTRIBUTION-SUMS-LOC` (see [profile signal catalogue](../superpowers/specs/2026-04-15-profile-signal-catalogue.md)) | `confirmOnly` |
| Graph import edges | `a.rs → b.rs` with detail `crate::X` | `I-GRAPH-EDGE-CLOSURE`, `I-GRAPH-NODE-ID-UNIQUE`, `I-GRAPH-NODE-MATCHES-PACK-PATH`, `I-GRAPH-NO-SELF-LOOP`, `I-GRAPH-IMPORT-EDGE-DETAIL` (see [graph import catalogue](../superpowers/specs/2026-04-14-graph-import-edges-invariant-catalogue.md)) | `confirmOnly` |
| Graph `declares` edges | `lib.rs → foo.rs` via `mod foo;` | `I-RUST-MOD-DECLARES-EMITTED`, `I-GRAPH-CYCLES-EXCLUDE-DECLARES`, `I-GRAPH-ORPHANS-RESPECT-DECLARES` (see [declares/cycles/orphans catalogue](../superpowers/specs/2026-04-14-graph-declares-cycles-orphans-catalogue.md)) | `confirmOnly` |
| Graph cycles (`CycleReport`) | `largestCycleSize: 15` | `I-GRAPH-CYCLES-EXCLUDE-DECLARES`, `I-GRAPH-CYCLES-EXCLUDE-CALLS`, `I-GRAPH-CYCLES-EXCLUDE-SELF-LOOP`, `I-GRAPH-CYCLE-REPORT-SHAPE` (see [declares/cycles/orphans catalogue](../superpowers/specs/2026-04-14-graph-declares-cycles-orphans-catalogue.md)) | `confirmOnly` |
| Orphan detection | `orphanFiles: [...]` | `I-GRAPH-ORPHANS-BASELINE`, `I-GRAPH-ORPHANS-EXCLUDE-ENTRY-POINTS`, `I-GRAPH-ORPHANS-IGNORE-CALLS`, `I-GRAPH-ORPHANS-RESPECT-DECLARES`, `I-GRAPH-ORPHAN-REPORT-SHAPE` (see [declares/cycles/orphans catalogue](../superpowers/specs/2026-04-14-graph-declares-cycles-orphans-catalogue.md)) | `confirmOnly` |
| Health signals | `severity: high` for `dependency-cycles` | None beyond threshold constants; heuristic by design | `excluded` (already `knowledgeLayer: heuristic`) |
| Project profile name | `project-profile.json:name` | `I-PROFILE-NAME-NPM`, `I-PROFILE-NAME-CARGO-ROOT-PACKAGE`, `I-PROFILE-NAME-CARGO-WORKSPACE`, `I-PROFILE-NAME-CARGO-WORKSPACE-BINARY-PREFERENCE`, `I-PROFILE-NAME-CARGO-WORKSPACE-FALLBACK`, `I-PROFILE-NAME-GENERIC-UNKNOWN-FALLBACK` (see [language and profile catalogue](../superpowers/specs/2026-04-14-language-and-profile-catalogue.md)) | `confirmOnly` |
| Entry point detection | `entryPoints: [...]` | `I-PROFILE-RUST-BIN-TARGETS` (see [rollout spec](../superpowers/specs/2026-04-13-observation-invariant-rollout-design.md)); `I-PROFILE-JAVA-MAIN-ENTRY-POINT` (see [Java entry point catalogue](../superpowers/specs/2026-04-14-java-entry-point-catalogue.md)); `I-PROFILE-PYTHON-DUNDER-MAIN-ENTRY-POINT`, `I-PROFILE-PYTHON-IF-NAME-ENTRY-POINT`, `I-PROFILE-GO-MAIN-ENTRY-POINT`, `I-PROFILE-JS-FAMILY-ENTRY-POINTS`, `I-PROFILE-JS-FAMILY-ENTRY-POINT-EXCLUSIONS` (see [entry point catalogue expansion](../superpowers/specs/2026-04-15-entry-point-catalogue-expansion.md)) | `confirmOnly` |
| Module mapping | `modules: [...]` | `I-PROFILE-MODULES-AGGREGATE-TOP-LEVEL-LOC`, `I-PROFILE-MODULES-SKIP-DOTFOLDERS` (see [profile signal catalogue](../superpowers/specs/2026-04-15-profile-signal-catalogue.md)) | `confirmOnly` |
| Line count (`loc`) | `FileEntry.loc` | `I-LOC-COUNTS-BLANK-LINES`, `I-LOC-COUNTS-FINAL-LINE-WITHOUT-TRAILING-NEWLINE`, `I-LOC-FIXTURE-PARITY` (see [LOC family catalogue](../superpowers/specs/2026-04-15-loc-family-catalogue.md)) | `confirmOnly` |
| Incremental manifest metadata | `manifest.json` top-level fields | `I-SCAN-INCR-MANIFEST-CORE` green as of `16a4591` | `confirmOnly` |
| Test-file detection | `scan.testFileCount`, `scan.sourceFileCount`, `scan.testToSourceFileRatio` (driven by `repolens_app::summary::is_test_file`) | `I-TEST-DETECT-RUST`, `I-TEST-DETECT-JAVA`, `I-TEST-DETECT-PYTHON`, `I-TEST-DETECT-GO`, `I-TEST-DETECT-JSTS`, `I-TEST-DETECT-PATH-PREFIX`, `I-TEST-DETECT-NO-FALSE-POSITIVE-CONTEST`, `I-TEST-DETECT-FIXTURE-PARITY` (see [test-file detection catalogue](../superpowers/specs/2026-05-11-test-file-detection-invariant-catalogue.md)) | `confirmOnly` (inline-language tests intentionally excluded; `INLINE_RUST_TESTS_NOT_COUNTED` limitation surfaces this on consuming findings) |
| Lockfile state | `profile.lockfileState` (driven by `repolens_profile::lockfile::CargoLockStateDetector::detect`) | `I-LOCKFILE-STATE-ABSENT`, `I-LOCKFILE-STATE-PRESENT-TRACKING-UNKNOWN`, `I-LOCKFILE-STATE-GITIGNORED-UNANCHORED`, `I-LOCKFILE-STATE-GITIGNORED-ANCHORED`, `I-LOCKFILE-STATE-TRAILING-SLASH-IS-DIRECTORY-ONLY`, `I-LOCKFILE-STATE-COMMENT-NOT-MATCH`, `I-LOCKFILE-STATE-RESPECTS-NEGATION`, `I-LOCKFILE-STATE-NO-COMMITTED-AND-TRACKED-EMISSION` (see [lockfile-state catalogue](../superpowers/specs/2026-05-11-lockfile-state-invariant-catalogue.md)) | `confirmOnly` (git-index inspection out of scope; `LOCKFILE_GIT_TRACKING_INSPECTION_PARTIAL` limitation surfaces this on consuming findings) |

Two patterns stand out.

First, everything derived from graph edges — cycles, orphans, impact analysis — currently sits at `confirmOnly`. The resolver is the weakest link, and resolver bugs propagate silently through SCC filters and traversal queries.

Second, any family with a known open bug drops to `excluded`. This is not punitive, it is protective: until the bug is fixed, the family cannot earn trust, and a buggy observation family must not be allowed to weigh in against memory even as confirmation, because repeated false confirmations are as corrosive as false contradictions.

**Maintenance rule.** This table MUST be updated in the same change that adds a new observation family to the extraction pipeline. Reviewers of changes touching `crates/repolens-core/src/model/`, `crates/repolens-extract/src/`, `crates/repolens-app/src/graph.rs`, or `crates/repolens-profile/src/profile.rs` should reject changes that introduce new observation families without a corresponding table row. Without this rule, the table becomes a trap: authoritative by placement but silently stale.

## Invariant Rubric

Observation invariants themselves are a concrete engineering artifact and belong in a dedicated specification (`docs/superpowers/specs/`), not in a philosophy doc. This section states only what a conforming invariant catalogue must include.

A conforming catalogue:

1. Names each invariant with a short stable ID.
2. Maps each invariant to exactly one observation family.
3. Cites the evidence form (property-based test, golden fixture, targeted unit test).
4. States the family's current tier and the tier it unlocks when green.
5. Covers at least one invariant per family sitting at `confirmOnly` or `contradictionEligible`.
6. Is maintained in lockstep with `grounding.rs` — if an invariant is deleted or weakened, the affected family must drop a tier.

Three illustrative invariants, to show shape (not to commit RepoLens to them):

- **I-GRAPH-01 (graph node closure).** For every `graph.edges[].source` and `graph.edges[].target`, the endpoint appears in `graph.nodes[].id`. Family: graph import edges. Evidence: fixture assertions over `fixtures/inputs/*` plus a property test on synthetic small graphs. Guards against dangling edges from partial resolver output.
- **I-RUST-SUPER (super:: resolution).** For every Rust `use super::X;` in a file at `{parent}/{child}.rs`, the resolved target is `{parent}/mod.rs` or `{parent}.rs`, never `lib.rs` of the containing crate. Family: graph import edges, Rust-specific. Evidence: unit tests in `graph.rs::resolve_rust_module_path`. Guards against the resolver misbehaviour documented in the self-scan motivation section.
- **I-SCAN-INCR-MANIFEST-CORE (incremental manifest core parity).** For a scan run twice over the same repository — first full, then incremental — both manifests agree on the fields present in the real `Manifest` struct that are scope-invariant (`schemaVersion`, `schemaUrl`, `chunks`, `scanScope`, `headCommit`, `worktreeDirty`, `scanConfigHash`, `reprocessedFiles`), and the incremental-only fields (`basePackId`, `baseHeadCommit`, `deletedFiles`, `carriedForwardFiles`) are populated only in the incremental run. Family: incremental manifest metadata. Evidence: a temp-repo scan pair in `crates/repolens-app/src/scan.rs` tests. Guards against silent field drift between scan modes.

The philosophical commitment is compact: **every widening of Path B eligibility must cite an invariant that is green at the time of widening**. No ad-hoc widening. No implicit widening in feature PRs. Invariants are the admission ticket.

## Envelope Axis: `evidenceGrade`

The current [epistemic envelope](../superpowers/specs/2026-04-11-epistemic-envelope-design.md) carries four top-level fields: `knowledgeLayer`, `confidence`, `provenance`, `data`. Today, `confidence: mechanical` means "produced by mechanical code, not by heuristic thresholds." It is a claim about the production process. It does not answer "was this observation family audited?"

This document proposes a new, orthogonal axis:

```
evidenceGrade: "verified" | "derived" | "unaudited"
```

| Value | Meaning |
|---|---|
| `verified` | The observation family has a passing invariant suite. This is necessary for promotion above `excluded`, but does not by itself grant contradiction rights. |
| `derived` | The observation is computed from verified inputs via a derivation that does not itself have invariant coverage. Eligible for at most `confirmOnly`. |
| `unaudited` | No invariants written for the family. Default posture: `excluded` until coverage is added. |

`evidenceGrade` is orthogonal to `knowledgeLayer`. A heuristic (`knowledgeLayer: heuristic`) can still carry an `evidenceGrade` describing whether its *underlying observations* are verified — consumers need that to calibrate trust even when they already understand the claim is rule-based.

**Rationale for a new axis rather than overloading `confidence`.** `confidence: mechanical` and `evidenceGrade: unaudited` co-occur today: the self-scan's false cycle count was mechanically produced and unaudited. Collapsing the two into one label would force a single claim that is true about production but misleading about verification. `confidence` is what the *producer* chose to emit; `evidenceGrade` is a separate claim made by the *invariant suite* about the producer's track record. Different actors, different timestamps, different update cycles.

The envelope is already additive — the spec allows mixed `knowledgeLayer` arrays and structured provenance. One more orthogonal axis fits the pattern. Values are fixed categorical; the envelope already resists continuous scores for the same reason (no false precision).

**Dependency — landed 2026-04-14.** The amendment adding `evidenceGrade` as a fifth top-level field in [`2026-04-11-epistemic-envelope-design.md`](../superpowers/specs/2026-04-11-epistemic-envelope-design.md) has shipped. Every MCP tool response now carries `evidenceGrade` alongside `knowledgeLayer`, `confidence`, and `provenance`. The field defaults to `"unaudited"` on all tools that have not explicitly opted in via the new `ok_epistemic_graded` helper in `crates/repolens-mcp/src/epistemic.rs`. Promoting a tool from `unaudited` to `verified` or `derived` is now a visible, greppable code change — the path from catalogue to live contract is open. The next stage of this work is promoting individual tools as their underlying observation families cite the rubric-conforming catalogues from §5.

## Path B Phase-Widening Rule

A future Path B phase that widens grounding eligibility must:

1. Identify the observation family being promoted.
2. Cite the invariant(s) that guard that family, with test IDs.
3. Require those invariants to be green on `main` at spec-approval time.
4. State the tier change explicitly (e.g. "promotes graph import edges from `confirmOnly` to `contradictionEligible`") and specify rollback conditions.
5. Ship the tier change in a single spec revision, not as a side effect of a feature PR.

A reviewer's checklist derived from this rule: *Does this change alter which observations may contradict memory? If yes, which invariants does it cite, and are they green?* This is the same pattern as existing review checks for serde wire formats or MCP parameter renames — a specific, cheap, structural question that catches a specific class of bug.

### Why Reviewer-Enforcement, Not Automation

§5's maintenance rule and the invariant-citation rule stated above are reviewer contracts, not CI gates. This is intentional for the current phase. The alternative — auto-demoting `evidenceGrade` when an invariant test fails, or blocking CI when §5's table drifts from the invariant suite — would be mechanically satisfying, but it has two failure modes that matter more than the fragility it removes:

1. **False auto-demotion.** An invariant test can go red for reasons that don't invalidate the family: fixture drift, transient flakiness, a local machine clock skew. Auto-demoting a tool to `unaudited` in response would silently erode agent trust in the tool's output. Reviewer judgement filters these cases at PR time.
2. **Policy encoded as code.** `contradictionEligible` is a policy decision, not a code property. Encoding it as a computed field from test results collapses policy and production into one artifact — precisely the collapse §3's "Why Separate Them" subsection warns against.

Reviewer-enforcement scales as long as two conditions hold: (a) the table in §5 is small enough to read in one sitting, and (b) the team is small enough that a drift in the table is visible to the people authorising the change.

**Revisit trigger.** When **either** (a) the §5 table passes roughly 25 families **or** (b) more than one author independently ships observation families on the same PR train without co-review, this subsection should be revisited. The first trigger is about cognitive load; the second is about the social enforcement model's blast radius. Hitting either trigger is a signal to invest in automation — but neither has been hit as of 2026-04-18.

This subsection is not a commitment to *never* automate. It is a commitment to automate deliberately, after the reviewer-only model has demonstrably failed at a named threshold.

## What This Document Does Not Do

- Does not modify `grounding.rs` or any runtime behaviour. The contract lives in documentation and, when the rubric is populated, in tests.
- Does not commit RepoLens to every invariant in the rubric. The three illustrative invariants are examples; the full catalogue is an engineering backlog, not a philosophical commitment.
- Does not specify which test mechanism each invariant must use. Property-based testing, golden fixtures, and targeted unit tests are all acceptable; the spec-level doc can choose per-invariant.
- Does not change the `knowledgeLayer`, `confidence`, or `provenance` fields of the epistemic envelope. `evidenceGrade` is a new, backward-compatible addition.
- Does not propose a precision or recall metric for the grounding layer itself. That is a separate research question about whether grounded memories drift toward or away from ground truth over time.

## Open Research Questions

- **Who authors invariants?** Engineers writing features that touch the observation pipeline? A dedicated audit pass? Automated property generation derived from golden fixtures? Probably a mix, but the ownership model needs to be explicit before the rubric grows.
- **How do invariants interact with fixture drift?** Some invariants will be written against real codebases (running RepoLens on a known-good project and asserting the graph matches an expected shape). When the upstream fixture changes, the invariant must either re-snapshot or be re-derived. Similar territory to `insta` snapshots but with stricter semantics.
- **Should `evidenceGrade` be per-response or per-field?** Per-response is simpler and fits the envelope's current response-level provenance. Per-field supports mixed responses (e.g., `get_summary` combining verified file counts with unaudited cycle counts). Default per-response; revisit if consumers report calibration loss.
- **Does contradiction admissibility deserve a memory-side opt-out?** A highly-trusted decision might declare itself immune to contradiction even from `contradictionEligible` observations — "this decision is ground truth; observations must change or be reconciled manually." Adjacent to the `highStakes` flag from opinion_pool_002 §2.5, but not identical.

## Relationship to the Four Principles

This contract is an operational deepening of **Evidence Over Confidence** (Principle 4), not an addition to the principle set. The principle already requires every claim to be traceable to its source. The invariant-plus-eligibility contract answers a question the principle implies but does not specify: *when a claim is traceable to observation, how much should the consumer trust the observation's truth value?*

It is also a precondition for the suggested deepening of **Memory Is Alive** (Principle 2) proposed in [05-structure-and-tensions.md](05-structure-and-tensions.md) — that memory stay "honest against evolving evidence." Honesty against evidence requires evidence to be honest first. This document describes what that means concretely.
