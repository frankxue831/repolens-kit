# Project Management Workflow Design

**Date:** 2026-04-27
**Status:** Proposed
**Scope:** Add a thin "right now" view over the existing planning substrate so a solo + agent-driven workflow can hold many threads without losing them
**Depends on:** [../../philosophy/notes/2026-04-14-arc-review-and-next-steps.md](../../philosophy/notes/2026-04-14-arc-review-and-next-steps.md), ../plans/2026-04-26-validate-plan-status-sync.md, ../plans/2026-04-26-lint-plan-warnings-only.md, ../checklists/workflow-closure-dogfood-snapshot.md

## Goal

Make the in-flight state of `repolens-rs` legible at a glance — for the maintainer and for any agent dropped into the repo — without introducing tooling the project does not need.

The repo already has unusually mature planning infrastructure: per-feature plans and specs under `docs/superpowers/{plans,specs}/`, a decision-council pattern in `docs/opinion_pool/`, the `validate-plan` CLI for plan-status hygiene, eval rounds as a recurring ritual, and `philosophy/07-current-state-vs-direction.md` as a shipped-vs-aspirational governance tool. What it lacks is a single place where in-flight work, near-term next steps, blocked threads, and lurking journey artifacts (blog drafts, retrospectives) are all visible at once.

The point of this slice is not to add a project-management *tool*. It is to operationalize the substrate that already exists by adding three small markdown surfaces and four lightweight rituals, so that "what is open right now" stops being an act of archaeology.

## Problem Statement

The current state has four observable symptoms.

1. The "A–E backlog" defined in [docs/philosophy/notes/2026-04-14-arc-review-and-next-steps.md](../../philosophy/notes/2026-04-14-arc-review-and-next-steps.md) is referenced from five docs but lives buried in a retrospective from 13 days ago. Nothing reads or updates it as live state.
2. Plans accumulate in `docs/superpowers/plans/` with no index. To answer "what plans are in-flight versus shipped versus superseded?" you must open each file and read the status header — `validate-plan` enforces correctness of those headers but does not surface them.
3. Cross-cutting threads that are not plan-shaped (blog post candidates, follow-ups from `opinion_pool/` discussions, "land or close out the readme-status-drift worktree", "decide on the `loc` observation family") have no home and are forgotten until they resurface accidentally.
4. There is no scheduled moment for narration discipline. `philosophy/07-current-state-vs-direction.md` is the canonical "shipped today" reference, but nothing pulls the maintainer back to it on a cadence — drift is caught only when an eval round or a reviewer surfaces it.

The friction is a workflow-ergonomics problem, not a tooling gap. GitHub Issues and Project boards are not absent because they were rejected; they are absent because the project has been moving fast enough that the existing per-feature planning convention has carried most of the load. The remaining gap is the cross-cutting view.

## Current Evidence

A scan of the repo on 2026-04-27 confirms the diagnosis:

- No `TODO.md`, `BACKLOG.md`, `ROADMAP.md`, `WIP.md`, or `NEXT.md` at any level.
- `gh issue list --state open` returns `[]`.
- Five docs reference the A–E backlog, none of them are written as live state.
- `git worktree list` shows `.worktrees/readme-status-drift` with five uncommitted modified files. That worktree was created during a feature push and has been quiet for several days. It is exactly the kind of lurking thread this slice exists to surface.
- The `docs/superpowers/checklists/workflow-closure-dogfood-snapshot.md` precedent shows that small, committed checklists are an accepted shape in this repo.
- The `validate-plan` CLI (Round 6, shipped 2026-04-26) already produces deterministic warnings on plan-status drift, including pending-without-checkbox-state and shipped-without-merge-evidence checks. An index file that lists active plans benefits from `validate-plan` for free.

## Options Considered

### Option A — Single living `BACKLOG.md` only

Add one file at the repo root sectioned `In-flight | Next | Blocked | Icebox`. Treat it as the single source of truth for everything open. Plans and specs continue under `docs/superpowers/` but are referenced from the backlog rather than indexed separately.

**Why reject as the only mechanism:** plans already have rich structure (status header, validate-plan coverage, dated identity). Funneling them through a flat backlog file flattens that structure and creates two places where the same plan must be reasoned about. The maintainer would either duplicate status or stop trusting the backlog within a week.

### Option B — GitHub Issues plus a single Project board

Promote each open thread to an issue; use a Project board for the "right now" view. Issues link cleanly to PRs and are accessible to agents via `gh`. Web-visible.

**Why reject for this slice:** the project is solo and the `gh issue list` history is empty — there is no existing issue habit to extend. Importing the existing backlog into Issues is a one-time data-entry cost the maintainer would not amortize, and Issues do not give a more useful view than markdown for this scale of work. Web visibility is a benefit only if the work needs to be visible to people outside the repo, which it currently does not. This option remains available later if the project grows.

### Option C — Promote the existing planning convention plus a thin top-level surface

Treat `docs/superpowers/plans/` as the de-facto plan backlog. Add one new index file, docs/superpowers/INDEX.md, that lists active plans with their status. Add one new file, BACKLOG.md at the repo root, for cross-cutting threads that are not plan-shaped. Add one new file, docs/review/journey-2026-04.md, as the home for the journey log feeding future blog posts. Wire all three into a small set of recurring rituals.

**Recommendation:** Option C.

Option C uses what already exists. The plan/spec convention does the heavy lifting; the new surfaces only fill the gaps the convention does not cover (cross-cutters, an at-a-glance index, journey-shaped writing). Every artifact is markdown, git-tracked, and readable by both maintainer and agents on first contact.

## Decision

This slice adds three markdown surfaces and four rituals.

The three surfaces form a layered "right now" view, scaled to the kind of work each surface tracks:

1. **`BACKLOG.md`** at repo root — cross-cutting threads that are not plan-shaped.
2. **`docs/superpowers/INDEX.md`** — a thin index of active plans with their status, using the data `validate-plan` already validates.
3. **`docs/review/journey-YYYY-MM.md`** — the journey log on the `docs/review` branch, feeding future blog posts.

The four rituals are scheduled moments that keep the surfaces honest. They are recommendations, not enforced gates: this slice does not add CI checks or hooks.

The existing planning convention remains authoritative for plans and specs. The existing `philosophy/07-current-state-vs-direction.md` remains authoritative for shipped-vs-aspirational. The new surfaces are coordination glue, not new sources of truth.

## Boundary

This slice is intentionally lightweight and reversible.

It does **not** introduce:

- a new `repolens` CLI subcommand
- a new MCP tool or schema contract
- CI enforcement or pre-commit hooks (the `validate-plan` CLI already runs in CI for plan hygiene; nothing new is added)
- automated index regeneration (the index is hand-maintained; staleness is caught by ritual, not CI)
- GitHub Issues, Project boards, or any external service integration
- a deadline-tracking, estimation, or velocity-tracking convention
- a roadmap document — the existing plan/spec set already serves that purpose

If the project grows into multi-person collaboration or external visibility becomes important, Option B (GitHub Issues + Project) is the natural next step. This slice does not foreclose that.

## Artifacts

The slice should produce these new artifacts:

1. **`BACKLOG.md`** at the repo root — see "Backlog Contract" below.
2. **`docs/superpowers/INDEX.md`** — see "Index Contract" below.
3. **`docs/review/journey-2026-04.md`** on the `docs/review` branch — see "Journey Log Contract" below. The first entry seeds the file with an opening journey entry distilled from the existing `digest.md` and `reading-map.md`, plus a "what's happened since 2026-04-16" paragraph covering the workflow-closure phases, scanner-fidelity, eval rounds 5/5.2/6/6.1, and the validate-plan CLI.
4. **`docs/superpowers/checklists/weekly-rhythm.md`** — a one-page checklist describing the four rituals so they are not lost to memory.

Existing artifacts that remain authoritative:

- `docs/superpowers/plans/*.md` — per-feature plans, status-headed, validated by `validate-plan`.
- `docs/superpowers/specs/*.md` — per-feature specs, including this one.
- `docs/philosophy/07-current-state-vs-direction.md` — canonical shipped-vs-aspirational reference.
- `docs/opinion_pool/*.md` — decision-council transcripts.
- `docs/doc-registry.yaml` — Doc Canon registry (no entries are added by this slice; the new files are not under philosophy/ or product-canon/engineering-canon).

## Backlog Contract

`BACKLOG.md` is a single markdown file at the repo root.

It has four sections, in this order:

1. **`## In-flight`** — work the maintainer is actively touching this week. Each entry is one or two lines plus an optional link to a plan, spec, branch, or worktree.
2. **`## Next`** — the next two or three things to pick up. Promotion criteria are deliberately loose: this is intent, not commitment.
3. **`## Blocked`** — threads waiting on something explicit (a decision, an upstream change, a re-eval). Each blocked entry must include the unblock condition.
4. **`## Icebox`** — ideas worth keeping but not active. Items here may move up or be deleted in any weekly triage.

Each entry should record:

- a short imperative title
- (optional) a link to the most relevant existing artifact (plan, spec, opinion-pool entry, branch, worktree, eval round)
- (for `Blocked` only) the unblock condition

The backlog is not the source of truth for plans; an in-flight feature with a plan should be one line in `## In-flight` linking to `docs/superpowers/INDEX.md` or directly to the plan. Cross-cutters that are not plan-shaped (blog candidates, follow-ups, lurking branches) live in the backlog directly.

The maintainer is the only writer. Agents may read it but should not edit it without explicit instruction.

## Index Contract

`docs/superpowers/INDEX.md` is a thin index of active plans.

It has three sections:

1. **`## In progress`** — plans with at least one unchecked task.
2. **`## Recently shipped`** — plans whose status is `shipped` and whose ship date is within the last 14 days. Older shipped plans drop off this list and live only in the dated file itself.
3. **`## Superseded`** — plans whose status is `superseded` along with the plan that supersedes them.

Each entry should record:

- the dated plan filename
- the plan title
- the status as written in the plan's status header
- (for `In progress` only) the count of unchecked task lines

The index is hand-maintained. Staleness is intentionally allowed; the weekly triage ritual catches it. The intent is that `validate-plan` already produces a deterministic per-plan view, and the index is the cross-plan rollup.

If automation pressure rises later, generating this index from `validate-plan` output is a natural follow-up — but it is explicitly out of scope for this slice.

## Journey Log Contract

`docs/review/journey-YYYY-MM.md` is the home for the journey log on the `docs/review` branch. One file per calendar month, lazily created when the first entry of the month is written.

Each entry has:

- a date heading (`### 2026-04-27`)
- one to four short paragraphs of plain prose
- (optional) a `**Seeds:**` line listing one or two transferable patterns or framings that might become standalone blog posts later

Entries are written for future-self first and the agent-tooling community second. Polish is explicitly not the goal — these are the source material from which posts crystallize, not the posts themselves.

The first entry on this file should:

- frame why the file exists (one paragraph)
- summarize what `digest.md` and `reading-map.md` were about as of 2026-04-15 (one paragraph)
- bring the journey forward to 2026-04-27 with one paragraph covering the 170-commit arc since 2026-04-16 (workflow-closure phases 1 and 2, scanner-fidelity, eval rounds 5 → 6.1, validate-plan, project-profile-description, the cleanup of stale remote branches)

The existing `digest.md` and `reading-map.md` on the `docs/review` branch remain unchanged. They are kept as a frozen 2026-04-15 snapshot, with a one-line freshness banner added at the top of each pointing readers to the current journey log.

## Weekly Rhythm

`docs/superpowers/checklists/weekly-rhythm.md` documents the four rituals. The checklist is a one-page reference, not a contract: missing a week is not an incident.

The four rituals are:

1. **Friday compose (≈30 min).** Open the current `journey-YYYY-MM.md`, write one entry covering the week. If a transferable pattern surfaces, mark it on the `**Seeds:**` line. This is the cheap input that feeds future blog posts.
2. **Pre-eval narration sync (≈15 min).** Before each eval round, refresh `philosophy/07-current-state-vs-direction.md` so the eval baseline is honest. Drift caught here is cheaper than drift caught by an eval reviewer.
3. **Worktree triage (≈10 min, weekly).** Run `git worktree list`. For each worktree, decide land / close / continue. Do not let modified-file worktrees age past two weeks without explicit reason. The current `.worktrees/readme-status-drift` thread is the kind of artifact this ritual exists for.
4. **Backlog triage (≈15 min, weekly).** Walk `BACKLOG.md` top to bottom. Promote, demote, or delete each entry. Update the in-progress task counts in `docs/superpowers/INDEX.md` based on `validate-plan` output (`cargo run -- validate-plan`).

Total weekly cost: about 70 minutes. The intent is to keep the rituals small enough to actually do.

## Risks and Mitigations

**Risk:** the new surfaces become stale faster than the work changes.
**Mitigation:** the weekly triage ritual is the explicit countermeasure. If the rituals are skipped for two consecutive weeks, the surfaces should be treated as stale until refreshed; the diagnosis is "rituals are not landing", not "surfaces are bad".

**Risk:** the maintainer ends up duplicating plan status between `INDEX.md` and the per-plan files.
**Mitigation:** the index intentionally records only the status header value and the unchecked-task count. Both are derivable from each plan file via `validate-plan`. If duplication friction grows, generating the index from `validate-plan` output is a clean follow-up.

**Risk:** the journey log on a separate branch creates merge friction.
**Mitigation:** `docs/review` is intentionally an isolated branch. Journey entries do not need to land on `main`; they are branch-local artifacts. If the maintainer later wants the journey on `main`, that is a separate decision.

**Risk:** agents edit `BACKLOG.md` opportunistically and obscure maintainer intent.
**Mitigation:** the backlog contract states the maintainer is the only writer. This is a convention, not enforcement; if violated repeatedly, a CODEOWNERS-style check is a future option.

## Open Questions

These are deliberately deferred:

1. Should the index be generated from `validate-plan` output rather than hand-maintained? Defer until duplication friction is observed in practice.
2. Should the journey log have a "publishable" subdirectory (e.g., `review/posts/`) for entries that have been lifted into standalone blog posts? Defer until the first such post exists.
3. Should `BACKLOG.md` move under `.repolens-meta/` or similar to keep the repo root clean? Defer; one file at the root is acceptable.
4. Should the four rituals become a single `repolens` subcommand later (e.g., `repolens triage`)? Defer; the rituals are markdown-shaped today and adding a subcommand prematurely escalates the surface.

## Success Criteria

After this slice lands and one weekly cycle has passed:

- A maintainer or agent dropped cold into the repo can answer "what is in-flight, what is next, what is blocked" by reading three files (`BACKLOG.md`, `docs/superpowers/INDEX.md`, the latest journey log) without grepping.
- The `.worktrees/readme-status-drift` thread is either landed, closed, or explicitly tracked in `BACKLOG.md` with an unblock condition.
- The first journey entry exists on `docs/review` and seeds the workflow.
- The A–E backlog from 2026-04-14 has been migrated into either `BACKLOG.md` or individual plan files, and the retrospective note is no longer the only place that data lives.
- `validate-plan` output is the input to the `INDEX.md` rollup, not a parallel data source.
