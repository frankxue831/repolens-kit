# Workflow Closure Phase 1 Design

**Date:** 2026-04-20
**Status:** Accepted
**Depends on:** ../../opinion_pool/opinion_pool_004.md, ../../opinion_pool/opinion_pool_005.md, ../../opinion_pool/opinion_pool_006.md, [../../philosophy/07-current-state-vs-direction.md](../../philosophy/07-current-state-vs-direction.md)

## Goal

Define the first execution slice of the "workflow closure" phase agreed in the April opinion pool.

The phase thesis is settled: RepoLens should improve the first agent workflow on a real repo before it adds more grounding breadth, ontology, or retrieval machinery. This document narrows that thesis into a first slice that is small enough to ship now and concrete enough to evaluate afterward.

## Current State

Several concerns raised in the opinion pool are already addressed in the codebase:

- `get_summary` now surfaces tier-selected `memory_insights` with per-item `groundingStatus`; doc 07 records this as shipped behavior.
- discovery now prunes ignored subtrees at walk time while preserving re-includes.
- profile entry-point detection suppresses fixture-shaped paths such as `fixtures/inputs/*`.

That means the remaining "workflow closure" work should not re-argue those decisions. The unresolved gap is operational: the repo still lacks an explicit workflow-closure artifact, recurring task-grade eval questions, and first-contact CLI ergonomics for the common "I am already in the repo root" case.

## Options Considered

### Option A — Continue broad Path B work

Keep extending grounding, memory decay coupling, or ontology now, and let workflow closure emerge as a side effect.

**Why reject it:** this pushes the center of gravity back toward substrate work even though the opinion pool converged on first-contact reliability as the immediate bar.

### Option B — Full workflow-closure program in one pass

Land the entire phase at once: eval loop, PR snapshot discipline, CLI ergonomics, mixed-tool audits, and whatever follow-up code those evals expose.

**Why reject it for now:** the scope is right for a phase, but too wide for a first implementation slice. It risks creating another narrative artifact before there is a small shipped win.

### Option C — Phase 1 slice

Write down the phase in repo docs, define the recurring evaluation questions and PR snapshot fields, and ship the smallest remaining ergonomics change that directly affects first contact: letting `scan` and `wiki` infer the current repository root when `--root` is omitted.

**Recommendation:** Option C.

## Decision

Workflow Closure Phase 1 consists of two deliverables:

1. A canonical workflow-closure artifact in `docs/superpowers/` that defines:
   - the first-agent workflow to optimize;
   - 4-5 recurring task-grade evaluation questions;
   - the PR dogfood snapshot fields for scan/profile/summary/memory-affecting changes;
   - explicit deferrals for embeddings, broader grounding triggers, and richer ontology work.
2. A CLI ergonomics change that makes the common first-contact commands work without repeating `--root .` by resolving the enclosing repo root when invoked inside a git worktree (and falling back to the current directory outside git):
   - `repolens scan`
   - `repolens wiki lint`
   - `repolens wiki rebuild-index`

This slice is intentionally narrow. It does not attempt to promote mixed tools, redesign the summary surface again, or change MCP contracts.

## First-Agent Workflow

The target workflow remains:

1. resolve or create the current pack;
2. get a compact repo summary;
3. inspect one focused hotspot;
4. recall relevant memory or conventions;
5. decide the next action.

Phase 1 improves steps 1 and 2 indirectly by reducing avoidable CLI friction and by creating a stable evaluation loop for future iterations.

## Evaluation Questions

Every PR that changes scan, extraction, graph, profile, summary, or memory behavior should answer these questions against `repolens-rs` itself:

1. What is this repo, in practical terms?
2. Where should I start if I need to change a specific area?
3. What prior memory or convention matters before making a change?
4. What are the main structural risks right now?
5. What changed between the latest two packs that matters to an agent?

These are intentionally qualitative. The purpose is not to create a heavyweight eval framework; it is to force a direct check that the product can support the workflow it claims to optimize.

## PR Snapshot Rule

For any PR that changes scan, extraction, graph, profile, summary, or memory behavior, include a short dogfood snapshot gathered from a fresh run on `repolens-rs` itself:

- files walked
- files included
- excluded rows emitted
- pack size
- scan duration
- notable profile entry points
- whether the five evaluation questions above are answerable from normal RepoLens output

The snapshot is a review aid, not a CI gate.

## CLI Scope

`scan` and `wiki` are the only commands in scope for the omitted-root inference change.

Why only these commands:

- they are the first commands a human or agent is likely to run after `cd` into a repo;
- they already canonicalize root inputs, so omitted-root inference can stay narrow and predictable;
- bootstrap commands remain intentionally explicit because they operate on a bounded run state and already have focused tests around explicit `--root` handling.

## Out of Scope

- `get_summary` redesign or evidence-grade promotion
- mixed-tool audit/promotion work for `compare_summaries`, `analyze_health`, or `find_dead_code`
- on-scan/high-stakes grounding
- embeddings or hybrid retrieval
- branch/worktree policy changes

## Acceptance Criteria

This slice is complete when:

1. the workflow-closure phase has a canonical design/plan pair on disk;
2. `repolens scan`, `repolens wiki lint`, and `repolens wiki rebuild-index` parse and run without `--root`, resolving the enclosing repo root when inside a git worktree and falling back to the current directory otherwise;
3. explicit `--root <path>` still overrides the default behavior;
4. tests cover the new CLI defaults.

## Follow-on Work

Once this slice lands, the next workflow-closure candidates should be chosen from:

- mixed-tool narrowing or promotion based on the eval questions;
- a tiny reusable dogfood script or checklist that standardizes the PR snapshot;
- first-contact surface cleanup revealed by the eval loop.
