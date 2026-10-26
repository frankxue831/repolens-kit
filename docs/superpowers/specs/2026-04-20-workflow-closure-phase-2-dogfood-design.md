# Workflow Closure Phase 2 Dogfood Routine Design

**Date:** 2026-04-20
**Status:** Shipped
**Scope:** Phase 2 shipped a maintainer-only dogfood snapshot helper and checklist for `repolens-rs`
**Depends on:** [2026-04-20-workflow-closure-phase-1-design.md](./2026-04-20-workflow-closure-phase-1-design.md), [../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md](../../philosophy/notes/2026-04-20-first-dogfood-snapshot-pr-26.md), ../../opinion_pool/opinion_pool_006.md, ../../opinion_pool/opinion_pool_007.md, ../../opinion_pool/opinion_pool_008.md, ../../opinion_pool/opinion_pool_009.md

## Goal

Make the workflow-closure dogfood snapshot cheap enough to repeat on qualifying PRs without turning it into product surface or CI policy.

Phase 1 established the recurring evaluation questions, the PR snapshot rule, and the omitted-root CLI ergonomics needed for first-contact use. Phase 2 adds the missing maintainer routine: a lightweight helper that produces the mechanical half of the snapshot in a paste-ready form, plus a short checklist that tells contributors when to run it and what still requires human judgment.

Status as of 2026-04-20: this design shipped in repolens-rs PR #30 (merge commit `908e5ff`). The shipped artifacts are:

- `scripts/dogfood-snapshot.sh`
- `scripts/test-dogfood-snapshot.sh`
- `docs/superpowers/checklists/workflow-closure-dogfood-snapshot.md`

What deliberately did not ship remains just as important:

- no `repolens` subcommand
- no CI gate
- no PR-template enforcement
- no machine-readable JSON output

## Problem Statement

The first manual workflow-closure snapshot on PR #26 surfaced the right kind of friction, but it also showed that the routine is still too expensive to repeat casually:

- pack size requires a second command;
- scan metadata is split across `scan` stdout, `manifest.json`, and `project-profile.json`;
- dirty-worktree status is easy to miss;
- the qualitative evaluation questions still need a stable place in the PR narrative.

If that routine remains fully manual, it will drift back into narration or be skipped entirely. If it is over-automated, it will produce polished-looking but low-trust output. Phase 2 therefore standardizes only the evidence-collection half.

## Options Considered

### Option A — Docs-only checklist

Add a markdown checklist and rely on contributors to gather every field by hand.

**Why reject it:** too easy to ignore, too easy to vary from PR to PR, and too dependent on contributor memory.

### Option B — Helper script plus checklist

Add a small maintainer-facing script under `scripts/` that runs a fresh scan, gathers the mechanical fields, and prints PR-ready markdown. Pair it with a short checklist doc that explains when to run it and what must still be filled in manually.

**Recommendation:** Option B.

### Option C — Product CLI subcommand or CI enforcement

Promote the routine into `repolens` itself or make the snapshot a required CI/PR template artifact.

**Why reject it for now:** the routine is not stable enough yet to deserve product surface or process weight. It is project hygiene, not user-facing capability.

## Decision

Phase 2 ships two primary artifacts:

1. `scripts/dogfood-snapshot.sh`
2. `docs/superpowers/checklists/workflow-closure-dogfood-snapshot.md`

The helper is a maintainer-only script. It is intentionally not part of the `repolens` CLI, does not emit JSON, and does not attempt to answer the workflow-closure eval questions automatically. The implementation also shipped `scripts/test-dogfood-snapshot.sh` as a hermetic local regression harness because Phase 2 deliberately stops short of CI enforcement.
The new `docs/superpowers/checklists/` subtree is deliberate: checklists are contributor-behavior docs, distinct from design specs in `specs/` and execution docs in `plans/`.

## Script Contract

The helper's v1 contract is intentionally narrow:

- Entry point: `scripts/dogfood-snapshot.sh`
- Scope: current git worktree only
- Inputs: no required flags
- Tooling assumptions: `bash`, `git`, `cargo`, and `python3`
- Output:
  - `stdout`: PR-ready markdown only
  - `stderr`: progress and failure messages
- Exit status:
  - `0` on success
  - non-zero on any root-resolution, scan, or pack-read failure

The script always performs a fresh scan. It does not support a "reuse latest pack" mode in v1, because that would make the snapshot's freshness ambiguous.

## Components

### 1. Shell Entry Point

The outer wrapper is a bash script to match the repository's existing maintainer tooling in `scripts/`.

Responsibilities:

- resolve the repo root with `git rev-parse --show-toplevel`;
- run the fresh scan;
- orchestrate pack-file reads;
- print the markdown block.

### 2. JSON Extraction via `python3`

The helper uses short embedded `python3` snippets to read:

- `.repolens/session.json`
- `<pack>/manifest.json`
- `<pack>/project-profile.json`

This locks the dependency set to the repo's existing maintainer-tooling pattern: bash entry points in `scripts/` and lightweight scripting only where structured parsing is needed. `scripts/verify-fixtures.sh` already uses `python3` for JSON validation, so Phase 2 should follow that precedent rather than introduce `jq` as a new dependency.

### 3. Checklist Doc

The checklist doc explains:

- which PRs should include a snapshot;
- when to run the script;
- which parts of the output remain manual;
- how to paste the output into the PR body and replace placeholders.

## Data Flow

The helper's end-to-end flow is:

1. Resolve the git worktree root.
2. Run `cargo run -- scan --root <repo_root>`.
3. Read `.repolens/session.json` to locate the active pack.
4. Read `manifest.json` and `project-profile.json` from that pack.
5. Gather pack size from the pack directory on disk.
6. Print one markdown block to `stdout`.

The scan is the authoritative refresh step. The session file is the authoritative link from repo root to active pack. The pack files are the authoritative source for snapshot metadata.
V1 intentionally accepts `cargo run` startup and compile overhead to avoid introducing a separate build/install step into maintainer workflow. The script should therefore report only RepoLens' self-reported scan duration from `scan` output; total script wall time may be longer and must not be presented as scan duration.

## Markdown Output

The emitted markdown should have this shape:

1. `## Workflow-Closure Dogfood Snapshot`
2. One short metadata line covering commit scanned, pack id/path, and scan mode if present
3. One mechanical table containing:
   - files walked
   - files included
   - excluded rows
   - pack size
   - scan duration
   - schema version
   - worktree dirty
4. `### Notable Profile Entry Points`
   - short bullet list from `project-profile.json`
   - capped to a small count such as 5
5. `### Eval Questions`
   - the five workflow-closure questions with `Answer: TODO` and `Evidence: TODO`
6. `### Notes`
   - placeholder bullets for friction observations, seeded as `F1`, `F2`, `F3`

The script should emit markdown that is paste-ready, but it must not try to auto-grade the qualitative sections.

## Automatic vs Manual Boundary

### Auto-filled by the helper

- commit scanned
- pack id/path
- scan mode
- files walked
- files included
- excluded rows
- pack size
- scan duration
- schema version
- worktree dirty
- notable profile entry points

### Left manual for the PR author

- the five eval-question answers
- the evidence source for each answer
- friction notes
- any judgment about whether the product behavior improved or regressed

This boundary is load-bearing. The helper standardizes evidence collection; it does not automate product judgment.

## Error Handling

The script should fail hard and clearly on missing prerequisites:

- not inside a git worktree;
- scan command failure;
- missing `.repolens/session.json`;
- missing active pack directory;
- unreadable `manifest.json` or `project-profile.json`;
- missing required fields needed for the markdown table.

On any failure:

- print the error to `stderr`;
- exit non-zero;
- print no partial markdown to `stdout`.

## Checklist Doc Contract

The checklist doc should say:

- run this only for PRs that change scan, extraction, graph, profile, summary, or memory behavior;
- run it after local verification is green;
- paste the emitted markdown into the PR body;
- replace every qualitative `TODO` before asking for final review.

The checklist is documentation for contributor behavior, not a governance mechanism.

## Verification Plan

Phase 2 is successful when the following are true:

1. The script runs successfully from the repo root.
2. The script runs successfully from a nested subdirectory and resolves the same repo root.
3. The emitted markdown contains the required heading, metadata, mechanical table, entry-point section, eval placeholders, and notes section.
4. Failure cases return non-zero and do not emit partial markdown.
5. The checklist doc is discoverable and clear enough that the next qualifying PR can use the routine without verbal reconstruction.

## Acceptance Criteria

- `scripts/dogfood-snapshot.sh` exists
- `docs/superpowers/checklists/workflow-closure-dogfood-snapshot.md` exists
- the helper emits PR-ready markdown only
- the helper always runs a fresh scan
- the helper remains outside the product CLI
- no CI gate or PR-template enforcement is introduced in this slice

## Out of Scope

- a new `repolens dogfood` or `repolens snapshot` subcommand
- machine-readable JSON output
- auto-grading the five eval questions
- CI enforcement
- PR template changes
- wider workflow-closure product work such as `compare_summaries` CLI support or summary-surface redesign

## Follow-on Decision Rule

After 2-3 real PRs use this routine, re-evaluate:

- whether the helper is stable enough to keep as-is;
- whether the snapshot fields need narrowing;
- whether one mixed-tool target (`compare_summaries`, `analyze_health`, or `find_dead_code`) should move next based on recurring qualitative friction.
- whether the routine has become stable enough to justify a higher-friction escalation such as PR-template guidance or CI enforcement.

That follow-on choice should be evidence-driven. Phase 2's job is to make that evidence cheap to collect.
