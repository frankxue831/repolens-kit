# 2026-04-14 — Arc Review and Next Steps

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-14. Preserved as the authoritative record of its topic as of that date.

**Scope:** retrospective on the 14-commit observation-invariant + evidenceGrade arc (from `3d20aef` through `2a6ba74`), plus a concrete backlog for future development sessions to pick up from.

**Read this if you are:** about to continue the observation-invariant / grounding-eligibility / evidenceGrade work, or trying to understand why the product narrative and the product behaviour diverged.

---

## Context

Between 2026-04-13 and 2026-04-14 the project moved from "observation invariants are a philosophy proposal" (`3d20aef`) to "observation invariants are a graded runtime contract on some MCP tools" (`2a6ba74`). Fourteen commits. The arc was driven by a self-scan that found three mechanical observation bugs (`68845c5` cycles, `70fdfa5` Rust bin entries, `68f27e9` workspace name), and the realization that without an invariant contract, a naive Path B rollout would let those bugs erode correct memory.

The arc concluded with a big-picture narration session where I described the destination as if it were current behaviour. A reviewer caught six specific mismatches. This note records both the narration and the corrections so that future sessions don't have to rediscover either.

---

## Commit-by-commit summary of the arc

| # | Commit | One-line description |
|---|---|---|
| 1 | `3d20aef` | Doc 06 — observation invariants + grounding eligibility contract (philosophy) |
| 2 | `02bf3e9` ≈ `02b9f2d` | First rollout spec (`2026-04-13-observation-invariant-rollout-design.md`) naming 3 invariants |
| 3 | `16a4591` | First 3 invariants implemented in code (graph edge closure, Rust bin targets, incremental manifest core) |
| 4 | `883565b` | Fix Rust `self::` / `super::` import resolution (real self-scan bug) |
| 5 | `efe3d16` | Correct phantom incremental-manifest bug in doc 06 (ironic: doc 06 itself had an ungrounded observation) |
| 6 | `c324d32` | Retire grandfathering clause — catalogue file-presence + qualified-symbols (8 invariants) |
| 7 | `cede41e` | File-content catalogue (5 invariants) — closes the undercount in the grandfathering clause |
| 8 | `5be3169` | Graph import-edges catalogue (5 invariants) |
| 9 | `133af39` | Declares/cycles/orphans catalogue (10 invariants, mostly renames of existing tests) |
| 10 | `30a01bd` | Language + profile catalogue (9 invariants, file:line citation form) |
| 11 | `3e70c06` | Envelope amendment: `evidenceGrade` enum + `ok_epistemic_graded` helper (type foundation only, no tool promotions) |
| 12 | `2e341b3` | First tool-promotion batch: 6 pure-observation tools to `EvidenceGrade::Verified` |
| 13 | `071aa25` | Four review-finding fixes: demote `get_graph` + `get_impact` + `read_file_content outline` to `derived`; add error-exception note to envelope spec; cite Java in doc 06 §5 entry-point row |
| 14 | `2a6ba74` | Java entry-point catalogue (1 invariant) — first commit of the catalogue-expansion arc after the derived-batch plan was withdrawn |

All fourteen are on local `main`, not pushed. At the time this note was first written, the withdrawn plan file (`2026-04-14-derived-tool-promotions.md`) was left untracked in the working tree rather than committed into history.

---

## The big-picture vision I narrated (this is DIRECTION, not shipped)

> RepoLens becomes the structured perception layer for AI coding agents — a service that reads a repository and hands back observations that carry their own trust metadata, so an agent can tell apart "the code literally says X" from "a rule of thumb scored Y" from "a memory I wrote last Tuesday says Z."

The narration described an agent workflow like:

1. `discover_packs` → find what RepoLens knows.
2. `get_summary` → compact, graded picture with health, cycles, memories.
3. `recall_longterm("auth")` → memories with `groundingStatus`. Confirmed leans in, contradicted is stale, unverifiable is caution.
4. `get_dependencies` / `get_callers` / `get_impact` — structural queries with `evidenceGrade: verified`.
5. Memory that grounds successfully decays slower; memory that contradicts decays faster.
6. `memorize_insight` pins memories to anchors so later grounding passes can verify.

**This narration is aspirational.** See `docs/philosophy/07-current-state-vs-direction.md` for which parts are shipped today. Specifically flawed claims are recorded in the "Known Mismatches" section of that doc — do not re-narrate them without checking.

---

## Reviewer findings (2026-04-14 second review)

Six specific mismatches between the narration and current behaviour. Each is preserved here because the *failure mode* of re-narrating the aspiration is more dangerous than the individual errors:

1. **`get_summary` does not return grounded memories.** `SummaryService::summarize` hardcodes `memory_insights: vec![]` at `crates/repolens-app/src/summary.rs:78`. Compact tier returns project/languages/stats/fitness/recommendations only. The tool still emits `unaudited`.

2. **`get_impact` is `derived`, not `verified`.** Demoted in `071aa25`. It runs a reverse-BFS transitive closure whose output has no invariant coverage.

3. **Confirmed memories do not decay slower.** Phase 1 explicitly does not couple decay to grounding status. Decay acceleration is Phase 2+ work not in the current contract.

4. **`memorize_insight` anchors are optional input.** Memories without anchors return `Unchecked` at recall. There is no auto-pinning.

5. **Invariant failure does not auto-demote `evidenceGrade`.** The maintenance rule is a reviewer-discipline contract in doc 06 §6, not a test-harness hook.

6. **The envelope's `knowledgeLayer` and `provenance` are single-or-array.** `KnowledgeLayerField` and `ProvenanceField` are untagged enums. Narration should not show only the single form.

**Secondary observations from the review:**
- Commit count: the reviewer correctly noted fourteen commits from `3d20aef`, not ten. (I had anchored on the `efe3d16`→`2a6ba74` sub-arc.)
- The withdrawn plan file was left untracked in the working tree, not in history. Preserved here as a historical note about what was tried, but not part of the committed arc.

---

## Next development backlog

> **Live source of truth as of 2026-04-27:** `/BACKLOG.md`. The list below is preserved as a 2026-04-14 snapshot; for current state, refer to BACKLOG.md.

Closed on this branch:

- The `get_profile` blocker slice is landed. Root Cargo profile-name coverage/catalogue are in place, `FileEntry.loc` now has a named family in doc 06, and `get_profile` now emits `evidenceGrade: "derived"`.
- Python, Go, and JS-family entry-point catalogue work is landed. Do not keep that gap in the open backlog.

### Remaining mixed-payload gaps

| Tool | Current blocker | Next step |
|---|---|---|
| `get_summary` + `compare_summaries` | Graph metrics / fitness score coverage still needs a scoping pass against the shipped payload fields. | Verify whether the existing `graph_contract_test.rs` coverage is enough, then catalog or trim whatever remains. |
| `analyze_health` | Severity thresholds and any other uncatalogued health fields still keep the response unaudited. | Audit the shipped payload, then decide whether the tool can be promoted without per-field grading. |
| `find_dead_code` | Bare `symbols` are still structurally incompatible with the excluded tier. | Either remove the bare symbols from the payload or accept that the tool stays unaudited. |

### Downstream work

- Phase 2 grounded-memory work still depends on trustworthy mixed-tool grading.
- If the mixed payloads keep expanding, revisit per-field grading instead of stacking more tool-level exceptions.

---

## Open questions for future sessions

1. **When should `evidenceGrade` become per-field instead of per-tool?** Threshold question: how many mixed payloads have to hit the wall before it is better to amend the envelope spec than keep trimming tool surfaces?

2. **Should `find_dead_code` drop bare `symbols` from its response?** Bare symbol names are useful, but they remain structurally incompatible with the excluded tier in doc 06 §5. Options are still: drop them, annotate them per-field, or accept that the tool stays unaudited.

3. **Does Path B Phase 2 need a separate philosophy doc?** Doc 06 describes the gating rule for Phase 2+ widening, but the actual Phase 2 design (decay coupling, absence as signal) is still in the grounded-memory spec. When Phase 2 lands, it should either extend doc 06 or spawn a new numbered doc.

---

## Governance note on narration

The single biggest lesson from this session: **narrating the destination as if it were the current state is the same failure mode the observation-invariant work exists to prevent.** It is an ungrounded observation about the product itself. Doc 07 (`07-current-state-vs-direction.md`) is the canonical cure. When describing RepoLens in any forward-facing context — commit messages, specs, README, agent instructions, even conversational narration — start from doc 07's "Shipped today" column, not from doc 06's "Direction of travel" aspirations.
