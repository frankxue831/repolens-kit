# Repeatable First-Agent Eval Decision

**Date:** 2026-04-21
**Status:** Accepted
**Depends on:** [2026-04-20-workflow-closure-phase-1-design.md](./2026-04-20-workflow-closure-phase-1-design.md), [2026-04-20-workflow-closure-phase-2-dogfood-design.md](./2026-04-20-workflow-closure-phase-2-dogfood-design.md), ../../opinion_pool/opinion_pool_006.md, ../../opinion_pool/opinion_pool_007.md, [../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md](../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md)

## Goal

Record the sequencing rule for what comes after workflow-closure phase 2 shipped.

The next move is not another isolated capability. It is making the first-agent workflow repeatable enough that the next broad product phase is chosen from repeated evidence instead of one-off friction.

## Current State

Workflow closure is no longer only a thesis:

- Phase 1 already defined the first-agent workflow, the recurring eval questions, and the PR snapshot rule.
- Phase 2 already shipped the maintainer-facing dogfood helper and checklist that make the mechanical half of that snapshot repeatable.
- `compare-summaries` now has a CLI route, so eval question 5 ("what changed between the latest two packs?") is no longer MCP-only.

That means the remaining uncertainty has moved. The question is no longer "what is workflow closure?" It is "what repeated first-agent friction should drive the next broad phase?"

## Options Considered

### Option A — Choose the next broad capability now

Pick the next phase immediately from the current known misses, for example by declaring `search_code` the next product center of gravity now.

**Why reject it:** this overfits one concrete failure before the repeatable eval loop has had a chance to show whether that failure is the recurring one or just the first one we happened to notice.

### Option B — Keep shipping isolated follow-up capabilities

Treat each surfaced miss as its own next slice and continue adding focused capabilities as they appear.

**Why reject it:** this loses the workflow-closure discipline. It would make the product feel reactive again instead of evidence-led.

### Option C — Thin repeatable eval contract around the existing loop

Keep the current workflow-closure surface, preserve concrete misses as regressions, run the same first-agent loop across a small number of real PRs, and let repeated friction determine the next broad phase.

**Recommendation:** Option C.

## Decision

The next product move is a thin repeatable-eval contract around the existing first-agent loop.

Concretely:

1. Keep using the workflow-closure routine already on disk instead of inventing a new dogfood surface.
2. Preserve any known concrete failure as a named regression as soon as it is observed.
3. Run the loop across a small number of real qualifying PRs.
4. Choose the next broad product phase from the clearest repeated first-agent friction, not from the first single miss.

This is a sequencing decision, not a new implementation program by itself.

## Existing Loop

The loop to repeat is the existing workflow-closure path:

1. resolve or create the current pack;
2. get a compact repo summary;
3. inspect one focused hotspot;
4. recall relevant memory or conventions;
5. compare what changed and decide the next action.

The purpose of the next pass is not to redesign this loop. It is to repeat it enough times that the first recurring breakage point is obvious.

## Regression Preservation Rule

Any concrete miss discovered during the repeatable eval pass should be preserved immediately as a named regression seed, not left as an anecdote in a PR body.

The first such seed is already known:

- the `ProfileAnalyzer::analyze` lookup miss should be captured immediately as a `search_code` regression.

This does **not** mean `search_code` is already the chosen broad phase. It means the known miss is concrete enough to preserve now so later decisions are anchored to observed failure rather than memory.

## Search Code Candidate Rule

`search_code` is promoted from "candidate next broad phase" to "chosen next broad phase" only if the repeatable eval pass shows that symbol/code lookup is the recurring first-agent friction.

Evidence that would justify that promotion looks like this:

- across multiple real PRs, the earliest repeated workflow failure is "I cannot find the code/symbol I obviously need";
- the failure is not better explained by summary quality, profile naming, conventions recall, or compare-summaries output;
- the preserved regressions cluster around code/symbol lookup rather than around unrelated surfaces.

If those conditions are not met, the next broad phase should be whichever repeated friction actually dominates.

## Repeatable Eval Contract

For the next `2-3` real PRs that already qualify for the workflow-closure dogfood routine:

1. run the existing dogfood helper and checklist;
2. answer the workflow-closure eval questions against the real PR;
3. record the first meaningful friction points, not just the final frustrations;
4. preserve any concrete miss as a named regression seed.

This contract is intentionally light. It is meant to produce prioritization evidence, not a new grading framework.

## Out of Scope

- a new `search_code` implementation plan
- automatic grading of the workflow-closure eval questions
- a new dogfood helper surface, CI gate, or PR-template requirement
- a blanket claim that retrieval or symbol lookup is already the next phase
- re-arguing the workflow-closure phase-1 or phase-2 boundaries

## Acceptance Criteria

This decision is in force when:

1. the sequencing rule is written down in a canonical spec;
2. the known `ProfileAnalyzer::analyze` lookup miss is treated as a preserved `search_code` regression seed rather than a loose note;
3. the next broad phase is explicitly deferred until `2-3` real PRs produce repeated first-agent evidence.

## Follow-on Decision Rule

After `2-3` real qualifying PRs run the repeatable eval contract, write one short synthesis that names:

- the repeated first-agent frictions;
- the preserved regression seeds that recurred;
- the recommended next broad phase;
- why that phase beat the other candidates.

If symbol/code lookup is the recurring first-agent friction, `search_code` becomes the next broad product phase. If not, it stays a candidate and the dominant repeated friction wins instead.
