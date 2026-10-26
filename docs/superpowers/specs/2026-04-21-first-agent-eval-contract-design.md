# First-Agent Eval Contract Design

**Date:** 2026-04-21
**Status:** Proposed
**Scope:** Add a thin repo-local eval contract around the existing workflow-closure dogfood loop
**Depends on:** [2026-04-20-workflow-closure-phase-1-design.md](./2026-04-20-workflow-closure-phase-1-design.md), [2026-04-20-workflow-closure-phase-2-dogfood-design.md](./2026-04-20-workflow-closure-phase-2-dogfood-design.md), [../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md](../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md), ../../opinion_pool/opinion_pool_009.md

## Goal

Make the first-agent workflow repeatable enough to evaluate across a few real PRs without prematurely turning the dogfood loop into product surface.

Phase 1 defined the workflow and recurring questions. Phase 2 made the mechanical snapshot cheap to produce. The next slice is narrower than a new feature phase and stricter than a checklist: preserve known concrete misses as regressions, standardize a small set of repeatable first-agent cases, and make the resulting friction comparable across PRs.

## Problem Statement

The current workflow-closure loop is only half-standardized.

What exists today:

- `scripts/dogfood-snapshot.sh` produces the mechanical scan snapshot
- `scripts/test-dogfood-snapshot.sh` protects that script's markdown and failure contract
- the checklist explains when to run the helper and what remains manual

What is still missing:

- no committed set of repeatable first-agent eval cases
- no stable way to preserve a known concrete miss as a regression
- no comparable output across multiple qualifying PRs beyond free-form notes
- no disciplined rule for deciding when repeated friction justifies a broader product phase

Without that middle layer, the loop remains anecdotal. Individual PRs can surface useful friction, but the repo still lacks a light contract that says which cases are being re-run, which ones are known failures, and which repeated failures are actually recurring enough to influence the roadmap.

## Current Evidence

The first manual dogfood pass on PR #26 already showed the pattern this slice should codify:

- a concrete product miss existed (`project-profile.json` naming mismatch)
- some friction was script-fixable (`pack size`, `dirty worktree`, `scan duration`)
- some friction exposed missing product routes (`compare_summaries` only through MCP)

The next known miss is more focused: symbol lookup for `ProfileAnalyzer::analyze`.

Today, MCP `search_code` symbol mode delegates to `SearchService::search_symbol`, and that implementation matches only `sym.name` with a case-insensitive substring search. It does not match `sym.qualified_name`. As a result, a query for `ProfileAnalyzer::analyze` is a credible first-agent probe that currently misses even when the pack contains the qualified symbol identity.

That makes it appropriate as an immediate regression seed. It does **not** yet prove that `search_code` should become the next broad product phase. That broader move should depend on repeated evidence across real PR runs, not on one known miss alone.

## Options Considered

### Option A — Strengthen the existing snapshot helper only

Keep the current script and checklist as the center of the loop and add a few more narrative conventions.

**Why reject it:** it keeps the process mostly documentary. It does not preserve known misses as explicit regressions, and it still makes cross-PR comparison depend too much on prose discipline.

### Option B — Repo-local eval cases plus runner

Add a committed manifest of repeatable first-agent cases and a runner that wraps the existing snapshot loop, renders a standardized eval block, and marks mechanically checkable cases as pass, fail, or known-fail.

**Recommendation:** Option B.

### Option C — Promote the eval contract into product surface now

Add a `repolens` subcommand, new MCP tools, or some other user-facing contract for the dogfood loop itself.

**Why reject it for now:** this is the same premature escalation Phase 2 explicitly deferred. The immediate goal is evidence collection and repeatability inside this repo, not another product capability.

## Decision

This slice adds a thin repo-local eval contract around the existing dogfood loop.

The contract has four pieces:

1. a committed case manifest describing repeatable first-agent probes
2. a maintainer-facing runner script that emits standardized eval markdown for the current worktree
3. regression coverage for mechanically checkable probes
4. a decision rule that treats repeated friction as the trigger for broader product prioritization

The existing snapshot helper remains the mechanical scan snapshot source. The new contract wraps that helper instead of replacing it.

## Boundary

This slice remains intentionally repo-local and maintainer-facing.

It does **not** introduce:

- a new `repolens` CLI subcommand
- a new MCP tool or MCP schema contract
- CI enforcement
- PR-template enforcement
- automatic scoring of the five workflow-closure questions
- a fix for the `ProfileAnalyzer::analyze` search miss in the same slice

The known miss is preserved as a regression input first. Whether it graduates into the next broad product phase depends on repeated friction evidence.

## Artifacts

The slice should produce these new artifacts:

1. `docs/superpowers/evals/first-agent-cases.json`
2. `scripts/dogfood-eval.sh`
3. `scripts/test-dogfood-eval.sh`
4. a mechanically checkable regression test under `crates/repolens-mcp/tests/`

Existing artifacts that remain authoritative:

- `scripts/dogfood-snapshot.sh`
- `scripts/test-dogfood-snapshot.sh`
- `docs/superpowers/checklists/workflow-closure-dogfood-snapshot.md`

## Case Manifest Contract

The committed manifest is the heart of the repeatability contract.

Each case should record:

- `id` — stable identifier such as `FAC-001`
- `workflowStep` — which step of the first-agent workflow the case probes
- `question` — human-readable statement of the user need
- `surface` — the RepoLens surface being probed, such as `search_code`, `get_summary`, or snapshot-derived pack files
- `probe` — exact query or action to run
- `expectedStatus` — one of `pass`, `fail`, or `known_fail`
- `frictionTag` — short stable tag used to group repeated friction across PRs
- `notes` — short explanation of why the case exists

The manifest is input data for the runner. It is not product API and does not need forward-compatibility promises outside this repo.

## Initial Case Set

The initial committed case set should stay small and focused on the first-agent workflow:

1. one case for practical repo identification
2. one case for focused hotspot discovery
3. one case for memory or convention recall
4. one case for structural risk inspection
5. one case for the known `search_code` symbol miss

The fifth case is the only one that needs an explicit `known_fail` status on day one.

Its probe should be equivalent to:

- surface: `search_code`
- mode: `symbol`
- query: `ProfileAnalyzer::analyze`

Its purpose is to preserve the current miss deliberately and visibly until either:

- the miss is fixed in a later slice, or
- repeated eval runs show that symbol lookup is not actually recurring first-agent friction

## Runner Contract

`scripts/dogfood-eval.sh` should:

1. resolve the current git worktree root
2. run the existing `scripts/dogfood-snapshot.sh`
3. load the committed case manifest
4. render a standardized eval section for each case
5. mark mechanically checkable cases with their observed status
6. leave qualitative interpretation in human hands

The runner should emit markdown to `stdout` and fail cleanly to `stderr`, matching the general conventions of the existing snapshot helper.

The output should make these fields explicit for each case:

- case id
- workflow step
- surface
- probe
- expected status
- observed status
- friction tag
- notes

Cases that require human interpretation may still end with a manual answer block, but they should share the same structure each run so PR-to-PR comparison does not depend on contributor phrasing.

## Regression Strategy

This slice draws a hard line between two kinds of coverage:

### Shell-level contract coverage

`scripts/test-dogfood-eval.sh` should protect:

- successful markdown emission
- stable inclusion of the case ids and eval headings
- no partial markdown on failure
- nested-directory execution

### Mechanically checkable product regressions

The known `ProfileAnalyzer::analyze` miss should be locked in a Rust test near the MCP `search_code` surface.

That test should verify current behavior honestly:

- a symbol search for `App` still succeeds
- a symbol search for `ProfileAnalyzer::analyze` does not currently return a result

The failure should be recorded as a deliberate `known_fail` case in the eval contract, not hidden as an undocumented gap.

This preserves the regression without forcing the same slice to broaden into search redesign.

## Automatic vs Manual Boundary

The contract standardizes structure and preserves mechanically checkable truth. It does not automate product judgment.

### Standardized automatically

- which cases are run
- their stable ids and friction tags
- their expected status
- their observed status when mechanically checkable
- the emitted markdown shape

### Left to human judgment

- whether a qualitative answer is good enough for first-agent use
- whether a repeated friction tag is severe enough to change roadmap priority
- whether a failing case represents an acceptable defer, a narrow bugfix, or the start of a broader phase

That boundary is load-bearing. The point of this slice is repeatable evidence, not automated roadmap decisions.

## Workflow

For any qualifying PR that changes scan, extraction, graph, profile, summary, memory, or first-agent-facing tooling:

1. run normal local verification first
2. run `scripts/dogfood-eval.sh`
3. paste the emitted markdown into the PR body or a linked note
4. keep the case ids, friction tags, and observed statuses intact
5. add short narrative commentary only where the contract leaves manual interpretation open

This makes each qualifying PR contribute comparable evidence rather than one-off prose.

## Decision Rule After 2-3 Real PRs

After 2-3 real PRs use the contract, review the resulting cases by friction tag and workflow step.

The prioritization rule is:

- preserve isolated known misses as regressions immediately
- promote a friction area into the next broad product phase only if it repeats across real PR evals in the same workflow region

For `search_code`, that means:

- the `ProfileAnalyzer::analyze` miss is enough to justify a preserved regression now
- it is **not** enough, by itself, to justify turning `search_code` into the next major product theme
- that broader promotion should happen only if repeated eval runs show symbol lookup as recurring first-agent friction

This keeps the roadmap tied to observed workflow pain instead of the loudest individual miss.

## Verification Plan

This slice is successful when the following are true:

1. the eval case manifest exists on disk and is readable by the runner
2. `scripts/dogfood-eval.sh` emits stable markdown for the current worktree
3. `scripts/test-dogfood-eval.sh` protects the runner's output and failure behavior
4. the known `ProfileAnalyzer::analyze` search miss is preserved as a named regression
5. a qualifying PR can produce both the mechanical snapshot and the structured eval block without reconstructing the workflow from memory

## Acceptance Criteria

- `docs/superpowers/evals/first-agent-cases.json` exists
- `scripts/dogfood-eval.sh` exists
- `scripts/test-dogfood-eval.sh` exists
- at least one committed case is marked `known_fail`
- that `known_fail` is the `search_code` symbol probe for `ProfileAnalyzer::analyze`
- the slice stays outside product CLI and MCP surface
- no CI gate or PR-template policy is introduced

## Out of Scope

- fixing `search_code` qualified-symbol matching in this slice
- broadening `search_code` into a multi-case product phase before repeated eval evidence exists
- new CLI or MCP product surface for the eval contract
- auto-grading qualitative answers
- CI enforcement
- PR-template enforcement

## Bottom Line

The next move is not another isolated capability. It is a repeatability layer.

This design keeps the shipped Phase 2 dogfood helper, adds a small committed contract around repeatable first-agent cases, preserves the known `ProfileAnalyzer::analyze` symbol miss as a visible regression, and delays any broader `search_code` phase decision until real repeated eval evidence says it is warranted.
