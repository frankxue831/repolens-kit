# Self-Bootstrap Skill Specs

**Date:** 2026-04-14
**Status:** Shipped
**Depends on:** [2026-04-14-self-bootstrap-skills-design.md](2026-04-14-self-bootstrap-skills-design.md), [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [../../philosophy/07-current-state-vs-direction.md](../../philosophy/07-current-state-vs-direction.md)

## Purpose

This document is the detailed source of truth for the first infrastructure bootstrap skills described only in summary form in the design doc. Each skill is a bounded contract for how an agent should work on `repolens-rs` while treating RepoLens as the preferred perception layer and respecting the current trust model.

`bootstrap.yaml` carries the machine-readable summary of these skills for the loader/registry layer. This markdown document remains the detailed explanation layer.

The shipped Phase 1 runtime consumes these contracts through `repolens bootstrap start|finish|abort`. RepoLens still does not edit files itself, and `write_anchored_memory` remains deferred.

## Shared Evidence Record

Every skill should report substantive evidence in a uniform record shape.

```yaml
evidenceRecord:
  source: "discover_packs"
  knowledgeLayer: "observation"
  confidence: "mechanical"
  evidenceGrade: "verified"
  provenance:
    kind: "repositoryRead"
    source: ".repolens/"
  freshness: "current-session"
  note: "latestPack exists and sessionId is present"
```

Rules:

- `source` names the tool or artifact actually consulted
- `knowledgeLayer`, `confidence`, `evidenceGrade`, and `provenance` must reflect the shipped RepoLens response when the source is an MCP call
- `freshness` is reported as observed or derived, never invented
- when the source is direct inspection under the cold-start exception, the record must say so explicitly

## `discover_repo_state`

### Purpose

Determine whether RepoLens perception is usable for the current task without overclaiming what current tools prove.

### Inputs

- repo root
- optional task summary

### Outputs

- pack presence status
- MCP reachability status
- bounded freshness assessment
- cold-start exception decision, if needed
- evidence records for each conclusion

### Required Evidence

- `discover_packs` when MCP is available
- `.repolens/session.json` and latest pack manifest when present
- direct command checks only if MCP is unavailable or broken

### Allowed Evidence Grades

- `verified` and `derived` may both be used
- `unaudited` may be reported as context, but cannot alone declare the repo ready for structural edits

### Failure / Rejection Conditions

- no pack exists and scan cannot be run
- MCP path cannot be established and cold-start repair scope is unclear
- freshness cannot be bounded well enough for the requested task

### Expected Artifacts or Logs

- repo-state summary with evidence records
- explicit statement of whether cold-start exception is active

## `plan_with_trust`

### Purpose

Produce a bounded implementation plan that classifies evidence by trust level and refuses to rely on weak evidence as if it were verified structure.

### Inputs

- task request
- evidence records from `discover_repo_state`
- relevant RepoLens outputs or direct-inspection notes

### Outputs

- bounded implementation plan
- evidence table grouped by grade
- explicit assumptions and refusals

### Required Evidence

- at least one evidence record supporting the area being changed
- if structural edits are proposed, at least one `verified` structural source or an explicit human-review gate

### Allowed Evidence Grades

- `verified` for structural decisions
- `derived` for search narrowing, target selection, and reviewable planning
- `unaudited` for context only

### Failure / Rejection Conditions

- plan depends on `unaudited` evidence as the sole structural justification
- plan depends on derived evidence without human-reviewable checkpoints
- pack or MCP state is too stale to support the requested task safely

### Expected Artifacts or Logs

- implementation plan
- evidence-by-grade summary
- refusal note when evidence is too weak

## `execute_bounded_change`

### Purpose

Carry out a scoped change with explicit path boundaries, success criteria, and evidence traceability.

### Inputs

- approved bounded plan
- allowed paths
- success criteria
- evidence records that justify the change

### Outputs

- code/docs/config changes in allowed paths only
- change summary tied back to evidence used
- any deviations from the original plan

### Required Evidence

- approved plan from `plan_with_trust`
- explicit success criteria before editing starts

### Allowed Evidence Grades

- `verified` for autonomous structural changes
- `derived` for bounded execution only when the resulting change will be validated before completion
- `unaudited` cannot independently justify structural edits

### Failure / Rejection Conditions

- requested change crosses denied paths
- success criteria are vague or absent
- execution would require silently widening scope

### Expected Artifacts or Logs

- touched-path list
- success-criteria checklist
- evidence-used summary

## `verify_and_rescan`

### Purpose

Check whether the claimed change holds up under conventional verification and, where feasible, RepoLens observation after the change.

### Inputs

- changed files
- expected checks
- before-change evidence records

### Outputs

- build/test/lint results
- rescan status
- before/after observation comparison where possible

### Required Evidence

- actual command results for the promised checks
- post-change scan or explicit reason why rescan was not feasible

### Allowed Evidence Grades

- command outputs are direct observations
- RepoLens post-change outputs follow their shipped `evidenceGrade`
- derived before/after comparisons must be labeled as such

### Failure / Rejection Conditions

- required checks were not run
- rescan was required by the task but not attempted
- change claim depends on stale pre-change evidence after code moved

### Expected Artifacts or Logs

- verification command list with outcomes
- rescan evidence records
- comparison summary

## `write_anchored_memory`

### Purpose

Persist durable findings only when they are real, bounded, and anchorable to repo artifacts.

### Inputs

- candidate memory text
- explicit anchors
- evidence records supporting the finding

### Outputs

- anchored memory write, or explicit rejection

### Required Evidence

- explicit anchors
- enough context for later grounding against packs

### Allowed Evidence Grades

- memory may summarize verified or derived findings
- pure `unaudited` context is not enough for durable memory on its own

### Failure / Rejection Conditions

- anchors missing
- finding is speculative, broad, or conversational rather than durable
- candidate memory collapses observation and interpretation into one claim

### Expected Artifacts or Logs

- accepted memory payload with anchors
- or rejection note stating why no write occurred

## Example Narrow Workflow

Safe example: doc/code sync for current epistemic behavior.

1. `discover_repo_state`
   - evidence: `discover_packs`, latest pack manifest, `docs/philosophy/07-current-state-vs-direction.md`
   - result: RepoLens usable, no cold-start exception

2. `plan_with_trust`
   - verified evidence: current tool-grade table in doc 07
   - derived evidence: grep of `ok_epistemic_graded` call sites to locate drift candidates
   - output: bounded plan to update only docs/config describing shipped trust behavior

3. `execute_bounded_change`
   - allowed paths: `docs/**`, `bootstrap.yaml`
   - success criteria: no claim about `get_summary` being trusted; no claim about automatic memory anchors
   - change: update the trust-policy contract and design text

4. `verify_and_rescan`
   - checks: YAML parse for `bootstrap.yaml`; targeted doc review against doc 07
   - rescan: optional for docs-only slice; if skipped, state that the slice does not change pack semantics

5. `write_anchored_memory`
   - reject write if the outcome is only "docs were updated"
   - accept only if the slice establishes a durable policy decision with explicit anchors to the design doc and trust-policy contract

## Scope Note

These are the detailed skill contracts for the shipped Phase 1 runtime. They define behavior and outputs, not a hidden controller. The thin YAML loader/registry now exists in `repolens-core`, but RepoLens still does not edit files itself and `write_anchored_memory` remains deferred.
