# Self-Bootstrap Skills Design

**Date:** 2026-04-14
**Status:** Shipped
**Scope:** Phase 1 shipped bootstrap-skills foundation for `repolens-rs`

## Context

RepoLens already has the core ingredients for a self-bootstrap loop:

- scans produce packs under `.repolens/`
- `repolens-mcp` exposes the pack surface to agents
- MCP responses carry `knowledgeLayer`, `confidence`, `evidenceGrade`, `provenance`, and `data`
- memory recall already reports grounding status against observed code

What does **not** exist yet is an explicit, inspectable skills layer telling an agent how to use that surface safely while working on RepoLens itself. Today, that operating logic lives in prompts, ad hoc workflow notes, and human judgment.

Phase 1 ships the smallest real foundation for that layer:

- a repo-native definition of what a bootstrap skill is
- trust-aware operating rules grounded in shipped behavior, not aspiration
- an explicit root contract (`bootstrap.yaml`)
- a concrete plan for the first operational skills

This is intentionally narrower than "autonomous RepoLens maintainer." The goal is to make disciplined agent work possible, not to build a hidden runtime.

### What this is not

- Not a daemon, scheduler, or autonomous background loop
- Not a replacement for direct file editing, `cargo`, or `git`
- Not a claim that current mixed-layer MCP tools are already strong enough to orchestrate the whole session
- Not a promise that RepoLens can always bootstrap itself without cold-start exceptions

Phase 1 now ships an agent-driven runtime for `docs_only` and `trust_contract` through `repolens bootstrap start|finish|abort`. RepoLens still does not edit files itself, and `write_anchored_memory` remains deferred.

## Shipped Reality This Design Must Respect

This design is anchored to the current implementation on this branch, not the philosophy destination:

- `evidenceGrade` is shipped and defaults to `unaudited`
- verified structural tools are currently limited to `get_callers`, `get_dependencies`, and `find_cycles`
- `get_graph`, `get_impact`, and `get_profile` are only `derived`
- `read_file_content` outline mode remains `unaudited`
- `get_summary`, `analyze_health`, `compare_summaries`, and `find_dead_code` remain `unaudited`
- memory anchors are optional in the current MCP surface, so any stricter anchor rule must be a bootstrap-skills policy, not a claim about current RepoLens behavior
- `discover_packs` reports pack path and session ID, but does not itself answer freshness or MCP readiness
- Phase 1 now ships an agent-driven runtime for `docs_only` and `trust_contract` through `repolens bootstrap start|finish|abort`; RepoLens still does not edit files itself, and `write_anchored_memory` remains deferred

Those constraints come directly from:

- `AGENTS.md`
- `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`
- `docs/philosophy/07-current-state-vs-direction.md`
- `docs/superpowers/specs/2026-04-11-epistemic-envelope-design.md`

## Decision Summary

| Question | Decision | Rationale |
|---|---|---|
| What is a bootstrap skill? | A named, inspectable contract describing how an agent performs a bounded RepoLens-aware workflow | Keeps behavior reviewable and repo-native |
| Where should the contract live? | Root `bootstrap.yaml` plus docs under `docs/superpowers/` | Matches the repo's existing "spec first, then implementation" workflow while giving agents one explicit operating entry point |
| Should Phase 1 add a new crate? | No | Existing boundaries are enough: `repolens-core` can own durable schema/types plus loader/registry support, and `repolens-app` can own orchestration later |
| Should RepoLens be mandatory for all understanding? | RepoLens-first, not RepoLens-only | Current tool grades are too uneven to support a hard ban on direct inspection |
| Is cold start allowed? | Yes, but only as a narrow, explicit exception to restore RepoLens perception capability | Prevents deadlock when packs or MCP are missing/broken |
| Should skills be executable code now? | Only as thin, explicit stubs in later phases | Phase 1 prioritizes inspectability over hidden automation |
| What is the first durable memory rule? | Bootstrap skills that write memory must require explicit anchors | Matches the trust model and prevents ungrounded project memory |

## Skill Model

A bootstrap skill is a bounded workflow unit with five parts:

1. **Purpose** — the narrow job the skill performs
2. **Inputs** — the minimum artifacts or task framing required
3. **Evidence policy** — which RepoLens outputs are required, preferred, or forbidden
4. **Execution rules** — what the agent may do, and where
5. **Outputs** — what the skill must leave behind for humans or later agents

These shipped skills are deliberately infrastructural. They exist to make later feature work safer and more legible.
The skill descriptions in this document are summary-only; `self-bootstrap-skill-specs` is the detailed source of truth.

### Initial skill categories

| Category | Role in the loop |
|---|---|
| Discovery | Determine whether RepoLens perception is usable |
| Trust-aware planning | Decide what can be concluded from current evidence |
| Bounded execution | Make a scoped change without drifting out of bounds |
| Verification and rescan | Check whether the claimed change matches observed repo reality |
| Anchored memory writing | Persist only durable, grounded findings |

## Trust-Aware Execution Rules

Bootstrap skills must reason about evidence grade explicitly.

### Allowed use by evidence grade

| Evidence grade | Allowed use |
|---|---|
| `verified` | Can support structural decisions and bounded autonomous changes |
| `derived` | Can narrow search, propose review targets, or support human-reviewable plans, but cannot be the sole basis for structural edits |
| `unaudited` | Can be surfaced, compared, or discussed, but cannot independently justify structural changes |

### Additional rules

- Always branch separately on flat MCP error responses; they are not wrapped in the epistemic envelope
- Never treat `knowledgeLayer: heuristic` as a measurement
- Never treat `interpretation` or memory content as code fact
- If a skill falls back to direct inspection, it must say so explicitly and record why RepoLens-first was insufficient
- If a skill writes memory, it must require explicit anchors and enough context for later grounding

### Task-type note

`trust_contract` is the narrow class of changes that alter how RepoLens trust semantics are documented, configured, or validated: evidence-grade policy, grounding rules, shipped-vs-direction narration rules, or bootstrap-skill trust gates. It is not a catch-all docs label.

## RepoLens-First Rule

For repository understanding, a bootstrap skill should prefer:

1. RepoLens-produced artifacts already on disk
2. RepoLens MCP tools
3. Direct file inspection only when the first two are unavailable or insufficient for a narrow unblock

This is not a purity rule. It is a discipline rule. The intent is to make RepoLens participate in the coding session as the perception layer whenever it is actually usable.

## Cold-Start Exception

Cold-start exception is allowed only when:

- there is no usable pack
- the MCP path is unavailable
- RepoLens itself is the component being repaired

Allowed cold-start actions:

- confirm repo root and known commands
- run scan
- start or test the MCP path
- inspect only the specific code needed to restore RepoLens perception capability

Cold-start is over once RepoLens becomes usable again. Skills should then switch back to RepoLens-first behavior.

## Interaction Model

### Scans and packs

- `discover_repo_state` is responsible for determining whether a pack exists and whether it appears usable for the current task
- freshness is not a single MCP field today; skills must derive it from the available pack/session artifacts and report how they judged it
- a skill must say when it is relying on pack-on-disk artifacts rather than live MCP calls

### MCP tools

- strong structural understanding should start from verified tools where possible
- derived tools may guide exploration and plan-shaping
- mixed-layer unaudited tools are context aids, not autonomous control-plane outputs

### Build and test

- bootstrap skills do not replace repo build/test commands
- any bounded change skill must name the expected checks up front
- `verify_and_rescan` must pair conventional verification with a RepoLens rescan when feasible

### Memory

- bootstrap skills may read memory, but must keep interpretation separate from observation
- `write_anchored_memory` exists to persist durable findings, not to journal speculative reasoning
- no automatic anchor pinning should be assumed from current RepoLens behavior

## Shipped Scope

The shipped Phase 1 slice includes:

- this design contract
- concrete skill specs for the first infrastructure skills
- a root `bootstrap.yaml` contract
- a thin `repolens-core` loader/validation/registry layer for the bootstrap contract

The shipped slice does not include:

- a hidden controller making opaque decisions
- a background reconciliation loop
- autonomous scheduling
- a claim that all skills are runtime-wired already

## Project Structure Proposal

The shipped slice uses a hybrid structure:

```text
bootstrap.yaml                                  # explicit root contract
docs/superpowers/specs/2026-04-14-self-bootstrap-skills-design.md
docs/superpowers/plans/2026-04-14-self-bootstrap-skills.md
```

`bootstrap.yaml` is the machine-readable summary contract for the initial skills. The markdown design and skill-spec docs remain the detailed human-readable source for semantics and rationale.

Later executable support should fit the current crate boundaries:

```text
crates/repolens-core/src/model/bootstrap.rs     # durable schema/types
crates/repolens-core/src/bootstrap.rs           # loader + validation
crates/repolens-app/src/bootstrap/              # orchestration helpers
```

Why this split:

- `repolens-core` already owns durable wire/data models and storage-adjacent contracts
- `repolens-app` already owns orchestration
- Phase 1 does not justify a new crate
- slice 2 should reuse the workspace-pinned `serde_yaml` dependency for config parsing consistency; revisiting parser choice is separate follow-up work

## Initial Skill Set

### `discover_repo_state`

Purpose:
- decide whether RepoLens perception is currently usable

Core outputs:
- pack presence
- bounded freshness assessment
- MCP availability status
- clearly labeled evidence sources

### `plan_with_trust`

Purpose:
- convert current evidence into a bounded implementation plan without overclaiming what weak tools prove

Core outputs:
- usable evidence by grade
- explicit refusal/defer paths when the evidence is too weak

### `execute_bounded_change`

Purpose:
- make a scoped change with named success criteria and path boundaries

Core outputs:
- touched paths
- evidence used
- claimed success criteria

### `verify_and_rescan`

Purpose:
- pair conventional checks with post-change RepoLens observation

Core outputs:
- check results
- rescan status
- before/after observation summary where feasible

### `write_anchored_memory`

Purpose:
- persist durable findings only when they are real and anchored

Core outputs:
- accepted anchored memory write, or explicit rejection

## Example Narrow Workflow

The example workflow should be a safe trust-related task such as:

- doc/code sync for current epistemic behavior
- tightening invariant coverage paperwork
- preparing a bounded tool-promotion slice

The example must show:

- evidence used
- grade-aware planning
- bounded execution
- verification
- optional memory write with anchors or explicit rejection

## Deferred Work

The following are intentionally deferred:

- machine-enforced policy gates
- richer freshness models
- per-field evidence-grade support
- automatic memory-writing workflows
- a broader operational skill pack for non-infrastructure work

## Success Criteria

This design is successful if it gives future implementation work a stable, inspectable target:

- the meaning of a bootstrap skill is explicit
- the RepoLens-first rule is concrete but not deadlocking
- the trust model reflects shipped behavior
- the cold-start exception is narrow and reviewable
- the structure proposal fits current repo conventions
- the next implementation phase can add real framework pieces without renarrating the product
