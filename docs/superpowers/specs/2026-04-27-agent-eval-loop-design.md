# Agent Eval Loop Design

**Date:** 2026-04-27
**Status:** Approved with revisions — ready for implementation plan
**Scope:** Automate the human-judgment half of the existing first-agent eval contract by running it under a Claude Code `/loop`, using real MCP tool calls as probes
**Depends on:** [2026-04-21-first-agent-eval-contract-design.md](./2026-04-21-first-agent-eval-contract-design.md), [2026-04-20-workflow-closure-phase-2-dogfood-design.md](./2026-04-20-workflow-closure-phase-2-dogfood-design.md)

## Goal

Turn the existing first-agent eval contract from a manual "paste markdown, fill in TODO answers" routine into a repeatable autonomous routine, so the project gets a cheap, reproducible signal about whether RepoLens's MCP surface actually works for an LLM agent — and so that signal can drive the project's own roadmap.

The user described this idea in conversation as "dogfood-driven self-improvement" — a loop that uses RepoLens's tools on RepoLens itself, with the tool output picking what to fix. This spec converts that idea into the smallest concrete shape that fits the existing dogfood/eval scaffolding without inventing a parallel system.

## Problem Statement

`scripts/dogfood-eval.sh` already defines the contract: five FAC cases (`FAC-001`..`FAC-005`), a stable markdown layout, and one mechanically-checked known-fail case (FAC-005, locked by a Rust regression test). For the four other cases the runner emits:

```
- Observed status: manual
- Answer: TODO
- Evidence: TODO
```

A human PR author fills the TODOs in by hand. That's fine for ceremony but bad for cadence. Each manual run costs contributor attention, the answers vary by author, and there is no automatic check on whether an LLM agent could actually reach those answers using only the tools RepoLens exposes.

Without an automated execution layer:

- The "first-agent" framing is asserted, not measured. We claim the cases probe an LLM workflow, but no LLM has actually been observed running them on this repo at any cadence.
- Cross-PR friction comparison still depends on prose discipline at the answer level, even though structure is standardized.
- Newly-discovered failures (an MCP tool returning confusing output, a missing schema field, a slow path) only surface when a contributor happens to notice them, never on a routine.

## Existing Constraints (Important)

This loop must respect what the project has already decided:

- **Phase 2 dogfood routine** (`docs/superpowers/specs/2026-04-20-workflow-closure-phase-2-dogfood-design.md`) deliberately stayed maintainer-local and out of the product CLI. The loop must too. **No new `repolens` subcommand. No new MCP tool. No CI gate. No PR-template enforcement.**
- **First-agent eval contract** (`2026-04-21-first-agent-eval-contract-design.md`) draws a hard line between mechanically-checked cases (FAC-005) and human-judgment cases (FAC-001..004). The loop replaces the *human* in the second category with an *agent under test*, but the line itself stays.
- **Parallel-agent guidance** (`CLAUDE.md` "Parallel agent sessions"): one live session per worktree, foreign worktrees are read-only, `target/` is shared and lock-contends. The loop must run in its own worktree on its own branch.
- **Phase 2 follow-on rule** ("after 2-3 real PRs use this routine, re-evaluate") still governs roadmap promotion. The loop produces evidence; it does not promote anything by itself.
- **Surface integrity:** A FAC case is only considered a RepoLens-surface success if the answer was reached through the intended RepoLens surface. Direct source reads, grep, pack JSON spelunking, or generic Bash are allowed as fallbacks, but must be recorded as friction — not silently counted as a pass. Without this rule, the loop tests Claude's repo-navigation ability, not RepoLens's MCP usefulness.
- **Generated-context distrust:** `.repolens/current-agent-brief.md` is a preflight aid only. It is generated from local workspace markdown and does not inspect source, run tests, or call MCP tools. Loop probes must verify current behavior through source, CLI help, tests, scan output, and MCP tool calls — not the brief.
- **Project lineage:** This loop targets the current Rust/Cargo RepoLens project. Historical Java/Maven plans are not part of this workflow unless explicitly revived by the user.

## Options Considered

### Option A — Static report-only journal

Have the loop run the FAC probes, write findings to a journal file under `docs/superpowers/loop-journal/`, and stop. No paste-ready markdown, no contract output.

**Why reject it:** breaks comparability with manual runs. The whole value of the FAC contract is that every run produces the same shape of evidence. A separate journal format fragments the corpus.

### Option B — Automated FAC runner emitting contract-shaped markdown (RECOMMENDED)

The loop emits the *same* markdown that `scripts/dogfood-eval.sh` emits today, with the `Answer: TODO / Evidence: TODO` lines filled in by the agent. Each run also writes a journal entry with the raw tool calls/outputs the agent used to reach the answer, plus an "agent-experience" note (was the tool's output clear, was a follow-up required, did anything return contradictory data). Optionally, the same markdown can be pasted into a PR body, with a small footer indicating it was agent-generated rather than human-authored.

**Recommendation: Option B.** It piggybacks on existing structure, produces directly comparable artifacts, and cleanly separates "what the contract says" (unchanged) from "who answers it" (now optionally an agent).

### Option C — Two-track: contract markdown + auto-commit on objective small fixes

Same as B, plus: when the agent's run reveals a small objective fix (dead code, broken doc reference, completed-then-stale TODO), it commits the fix to a `loop/auto-fixes` branch.

**Why defer to v2 (not reject):** the value-add is real, but auto-commit during a 6-hour autonomous run is exactly the kind of action that needs the user awake. v1 ships the observation-only loop; v2 reconsiders auto-commit after we see one full run end-to-end and know which finding-types are reliably safe.

## Decision

Build Option B. The loop's deliverable is **agent-authored FAC markdown** — the same shape as today's manual output, runnable on demand, without the human-judgment placeholders.

Structurally:

1. The user invokes `/loop <prompt>` in a worktree. **No cron, no autostart, no scheduled task.** This is the user's hard go/no-go gate.
2. The loop prompt instructs the agent to: refresh the pack via `cargo run -- scan`, run each FAC case **MCP-first** through the natively-registered RepoLens-RS MCP server (or the one-shot fallback helper if registration fails), record the surface classification per case, render the contract-shaped markdown, and append a per-run journal entry.
3. The loop runs N iterations (assumed cap: 6). On each iteration, the agent either re-runs the full case set against current state (useful when the user is actively editing) or picks a sub-set focused on a friction tag from the previous iteration (useful when the user wants the loop to drill into one area). The choice is encoded in the prompt itself, not in code.
4. The user reviews the journal and the emitted markdown when they return. Anything actionable becomes a normal plan/spec/PR via the user's existing workflow.

## Architecture

Three artifact types per run:

### 1. Contract markdown (paste-ready, comparable)

Byte-shape compatible with `scripts/dogfood-eval.sh` output today: the FAC contract block is byte-identical to manual runs except `Answer:` and `Evidence:` are filled by the agent. FAC-005 still defers to its Rust regression test at `crates/repolens-mcp/tests/first_agent_eval_test.rs` (the agent does NOT manually re-evaluate the known-fail; it cites the test and moves on).

Loop metadata is emitted as HTML comment lines **outside** the comparable contract block, so any future parser of FAC-contract output can ignore loop-specific lines safely:

```
<!-- agent-eval-loop metadata; ignored for FAC contract comparison -->
<!-- run_id: <id> | timestamp: <ISO> | head_sha: <sha> | pack_id: <id> | model: <model> | mcp_invocation_method: <method> -->

## Workflow-Closure Dogfood Snapshot
... (byte-identical to dogfood-eval.sh output) ...

### First-Agent Eval Contract
... (byte-identical layout, Answer/Evidence filled by agent) ...
```

The HTML comments are the only structural addition. Everything inside the FAC contract block matches the manual format so the corpus stays homogeneous and parser-safe.

### 2. Per-run journal (raw evidence, agent-experience signal)

New directory: `docs/superpowers/loop-journal/`. One file per run: `YYYY-MM-DD-<run-id>.md`.

#### Required run-metadata header

Every journal file starts with a YAML front-matter block. Missing or unverifiable fields are themselves recorded as friction, not silently omitted:

```yaml
---
run_id: <stable per-session id>
timestamp: <ISO 8601 with timezone>
worktree_path: <absolute path>
branch: <git branch name>
head_sha: <commit sha>
dirty_status_before: <output of git status --short before run, or 'clean'>
dirty_status_after: <output of git status --short after run, or 'clean'>
pack_id: <active pack directory name>
repolens_command: <the cargo run -- scan invocation used>
claude_code_loop_invocation: <the /loop command used to start the session>
mcp_invocation_method: <native | one-shot-script | unavailable>
model: <claude model id>
---
```

#### Per-case sections record

- Surface classification (see "Surface classification" below) — the answer-path category for this case
- Which tools the agent invoked, including exact CLI commands, MCP tool names + arguments, and file paths read
- The raw output excerpts the agent based its answer on (truncated as needed but with locators preserved)
- The answer reached, and confidence: `high | medium | low`
- Whether a fallback surface was required (see classification below) — `yes/no` and which surface
- Friction tag (must match an existing tag in `first-agent-cases.json` or be proposed in `friction-candidates.md`)
- Rough time spent on the case
- Any unexpected friction (tool returned empty when populated expected; output schema looked wrong; a follow-up call was needed where one shouldn't be)

The journal is the *real* data product. The contract markdown is for cross-PR comparison; the journal is for understanding what's actually breaking.

### 2.5. Surface classification (required per case)

Each per-case journal entry must classify the answer path into one of:

- `mcp_primary` — answer reached through RepoLens MCP tools
- `cli_primary` — answer reached through RepoLens CLI output (e.g., `cargo run -- scan` console output, `--help`)
- `pack_primary` — answer reached by reading generated pack JSON files (`manifest.json`, `project-profile.json`, file chunks)
- `repo_fallback` — answer required direct source/docs/grep inspection outside RepoLens-managed surfaces
- `unreachable` — answer could not be reached at all

Only `mcp_primary` and explicitly intended `cli_primary` paths count as **clean passes**. `pack_primary` and `repo_fallback` may still produce a *correct* answer, but they are friction signals: they say the agent had to dig for something the MCP surface should have surfaced directly.

**Hard rule:** A case cannot be marked `pass` if the answer was only reachable by direct file spelunking and not through the intended RepoLens surface. Such a case is `partial` at best, and the journal entry must capture that.

### 3. Friction backlog deltas (optional, v1 lightweight)

When a journal entry surfaces friction not already tagged in `first-agent-cases.json`, the agent appends a short proposal to a fixed file (assumed: `docs/superpowers/loop-journal/friction-candidates.md`) with one bullet per candidate: friction tag, surface, one-line description, evidence pointer (journal section).

The user, on review, decides whether any candidate graduates to a real spec/plan via the normal workflow. The agent does not promote candidates itself.

## Worktree & Branch Isolation

Per `CLAUDE.md`, the loop must run in a dedicated worktree:

- **Path (assumption):** `../repolens-rs-loop` — sibling to the main worktree, off the user's existing path
- **Branch (assumption):** `loop/agent-eval` — long-running branch, rebased on `main` between sessions
- **`CARGO_TARGET_DIR` (assumption):** loop sets its own (`../repolens-rs-loop/.cargo-target`) to avoid lock contention with concurrent main-worktree builds

The loop writes journal files inside the loop worktree on branch `loop/agent-eval`. The user reviews them by inspecting the loop worktree or branch. **Nothing is pushed, merged, or copied to `main` automatically.** Accepted journal entries may be cherry-picked or copied into `main` through normal review.

## Loop Prompt Shape (Sketch, Not Final)

The actual loop prompt will live in a new file (assumption: `docs/superpowers/loop-prompts/agent-eval.md`) and is not produced by this spec — it's the implementation plan's job. But the shape is:

```
You are running iteration N of M of the Agent Eval Loop on RepoLens-RS.

Context loaded from previous iterations: <journal summary>

Pre-flight (must pass before iteration body runs):
- Verify `git rev-parse --show-toplevel` matches the configured loop worktree path. If not, STOP.
- Verify `git status --short` contains nothing outside the runtime write allowlist. If not, STOP.
- Do not treat .repolens/current-agent-brief.md as evidence of current behavior. It is a preflight
  aid only.

This iteration's job:
1. Refresh the pack: cargo run -- scan --root <repo-root>
2. For each FAC case in docs/superpowers/evals/first-agent-cases.json:
   - Read the probe + surface + question.
   - Try the probe MCP-first: invoke the corresponding RepoLens MCP tool through the natively-
     registered server (or scripts/dev/mcp-call.sh if native registration is unavailable).
   - If MCP cannot answer, fall back through this priority order, recording each step:
     cli_primary -> pack_primary -> repo_fallback. A repo_fallback that produces a "correct"
     answer is NOT a clean pass; mark it `partial` with surface=repo_fallback.
   - Record: tools used (exact CLI / MCP tool name + arguments), raw output excerpt, time spent,
     surface classification, confidence (high/medium/low).
   - Decide observed status: pass | partial | fail | known_fail.
   - Write a one-sentence Answer and one-sentence Evidence.
3. Render contract-shaped markdown byte-shape compatible with scripts/dogfood-eval.sh, with
   Answer/Evidence filled. Loop metadata goes in HTML comments outside the FAC contract block.
4. Append a journal entry under docs/superpowers/loop-journal/<date>-<run-id>.md with the required
   YAML run-metadata header (run_id, timestamp, worktree_path, branch, head_sha, dirty_status_*,
   pack_id, repolens_command, claude_code_loop_invocation, mcp_invocation_method, model).
5. If new friction surfaced that doesn't match an existing case's frictionTag, append a candidate
   bullet to docs/superpowers/loop-journal/friction-candidates.md.

Hard guardrails (canonical lists in "Write Surface Allowlists"):
- Only write inside docs/superpowers/loop-journal/.
- Never edit src/, crates/, Cargo.{toml,lock}, fixtures/, scripts/, docs/philosophy/,
  docs/{product-canon,engineering-canon}.md, docs/doc-registry.yaml, docs/superpowers/{specs,plans}/,
  CLAUDE.md, AGENTS.md, README.md.
- Stop the loop and report if any stop condition fires (see "Cadence and Stopping").
```

The user reviews and tunes this prompt before turning the loop on. The prompt is the operator's surface; the spec is the contract.

## Write Surface Allowlists

The implementation PR and the runtime loop have **different** write rights. This is load-bearing: scaffolding the loop is allowed to create files in places the runtime loop must never touch.

### Implementation-phase write allowlist

The one-time, human-reviewed implementation PR may write:

- `docs/superpowers/loop-prompts/agent-eval.md`
- `docs/superpowers/loop-journal/README.md` (or `.gitkeep`)
- `docs/superpowers/loop-journal/friction-candidates.md` (initial empty/seed file)
- this spec, the implementation plan, and any checklist linkage updates
- if needed: `scripts/dev/mcp-call.sh` (maintainer-local helper, not product surface)
- if needed: `.claude/settings.local.json` for MCP server registration

### Runtime loop write allowlist

When the loop is actually running, it may only write:

- `docs/superpowers/loop-journal/YYYY-MM-DD-<run-id>.md`
- `docs/superpowers/loop-journal/friction-candidates.md`
- optionally one emitted contract markdown file under `docs/superpowers/loop-journal/`

### Runtime loop denylist (hard)

The runtime loop must never edit:

- `src/`
- `crates/`
- `Cargo.toml`, `Cargo.lock`
- `fixtures/`
- `scripts/`
- `docs/philosophy/`
- `docs/product-canon.md`
- `docs/engineering-canon.md`
- `docs/doc-registry.yaml`
- `docs/superpowers/specs/`, `docs/superpowers/plans/`
- `CLAUDE.md`, `AGENTS.md`, `README.md`

If the loop believes one of those files needs a change, it records a proposal in `friction-candidates.md` and stops. It does not edit.

## Cadence and Stopping

- **Trigger:** `/loop` invocation by user. No automatic firing.
- **Iteration cap (assumption):** 6 per session. Self-paced via the dynamic `/loop` mode (agent picks its own next-run delay between 60s and 1h).
- **Stop conditions (any one fires):**
  - Iteration cap hit
  - All FAC cases produced stable observed status across two consecutive iterations with no new friction
  - Any iteration broke local tests (`cargo test --workspace` regression)
  - Agent reports an unclassifiable error
  - Loop cannot prove it is in the dedicated worktree (`git rev-parse --show-toplevel` does not match the configured loop worktree path)
  - `git status --short` contains changes outside the runtime write allowlist
  - Same-commit runs disagree on a FAC case's observed status without a journal-recorded explanation (nondeterminism becomes signal)
- **Per-iteration time budget (assumption):** ~10 minutes wall time. A scan takes <1s on this repo; the rest is tool calls and journaling.

A 6-iteration session at ~10 min/iter ≈ 1 hour of agent activity, which fits comfortably inside a 6-hour AFK window with margin for slow iterations.

## Boundary (In / Out)

### In scope (v1)

- Loop prompt that drives the agent through one or more FAC iterations on a dedicated worktree
- Per-run journal under `docs/superpowers/loop-journal/`
- Contract-shaped markdown emission matching `dogfood-eval.sh` layout
- Friction-candidates appendix file
- Worktree + branch setup for isolated runs

### Out of scope (v1, deferred to v2 or later)

- Auto-commit of small objective fixes (Option C)
- Cron / scheduled / cross-session triggering
- A `repolens` CLI subcommand for the loop
- A new MCP tool exposing loop state
- CI gates or PR-template enforcement
- Auto-merging journal entries to `main`
- Trend analysis across runs (week-over-week friction tag frequency, etc.)
- Direct LLM-evaluation of the agent's own answers (no self-grading)

### Out of scope permanently

- Running the loop in CI on every PR (too expensive, wrong cadence — Phase 2 deliberately rejected this)
- Promoting loop output to product surface

## Decisions and Assumptions

The following were confirmed (✓) or revised (↻) during user review on 2026-04-27. Items still flagged with ✏️ are minor knobs the implementation plan may tune.

1. ✓ Worktree path: `../repolens-rs-loop`.
2. ✓ Branch: `loop/agent-eval`.
3. ✏️ Iteration cap: 6 per session. (Implementation may tune.)
4. ✓ Journal directory: `docs/superpowers/loop-journal/`.
5. ✓ Friction-candidate file: same dir, `friction-candidates.md`.
6. ✓ Loop prompt path: `docs/superpowers/loop-prompts/agent-eval.md` (new directory).
7. ✓ Loop must NOT auto-merge journal entries to `main`; same review gate as code.
8. ✓ Auto-commit (Option C) deferred to v2 explicitly, not silently rejected.
9. ↻ **MCP invocation mechanic — v1 decision:** The loop uses Claude Code's native MCP-server registration. The implementation plan adds RepoLens-RS as an MCP server entry in `.claude/settings.local.json` (project-local, not user-global) so the loop session can call MCP tools directly. **Fallback path:** if pre-registration proves unworkable, the loop uses a thin maintainer-local helper at `scripts/dev/mcp-call.sh` that performs single one-shot stdio MCP calls. **Not** a product CLI subcommand. The journal's `mcp_invocation_method` field records which path was used (`native | one-shot-script | unavailable`).
10. ↻ **Surface integrity rule** added to "Existing Constraints": fallback surfaces (CLI, pack JSON, repo grep) are allowed but recorded as friction; only `mcp_primary` and explicitly intended `cli_primary` paths count as clean passes.
11. ↻ **Implementation vs runtime write allowlists** split into two distinct lists (see "Write Surface Allowlists").
12. ↻ **Contract markdown** is byte-shape compatible with `dogfood-eval.sh`; loop metadata moved into HTML comments outside the comparable block (parser-safe).

## Verification Plan

The loop is successful when:

1. A single `/loop` session of 1+ iterations produces:
   - Contract-shaped markdown with FAC-001..004 `Answer:`/`Evidence:` filled (not "TODO")
   - Contract-shaped markdown with FAC-005 status correctly cited from its Rust regression test
   - A journal entry under `docs/superpowers/loop-journal/` with the required run-metadata header
2. The emitted markdown can be pasted into a PR body and is byte-shape compatible with `scripts/dogfood-eval.sh` output inside the FAC contract block (modulo HTML-comment loop metadata)
3. The loop never touches files outside the runtime write allowlist
4. The loop halts cleanly when any stop condition is met
5. A second run on the same commit produces a substantially-similar journal (low-noise; high reproducibility) — if two consecutive same-commit runs disagree wildly on observed status, the stop condition fires and that disagreement is itself the most valuable artifact of the run
6. Each FAC-001..004 result records its surface classification (`mcp_primary | cli_primary | pack_primary | repo_fallback | unreachable`)
7. At least one real RepoLens MCP call is made during the run, and preferably one per FAC case unless the case is explicitly CLI-only
8. Any direct repo spelunking is marked as `repo_fallback` friction rather than hidden inside a `pass`

## Acceptance Criteria

- A `docs/superpowers/loop-prompts/agent-eval.md` file exists with the loop instructions
- A `docs/superpowers/loop-journal/` directory exists with at least one real run
- At least one journal entry shows the agent reaching answers via real MCP-surface invocations (not just by reading source files)
- The emitted contract markdown matches `dogfood-eval.sh` layout
- No CLI subcommand, MCP tool, CI gate, or PR-template change is introduced
- The loop runs only on a dedicated worktree on a dedicated branch

## Follow-on Decision Rule (After First Real Run)

After one real loop session completes:

- If the agent's answers track what a human reviewer would have written → keep v1 as-is, plan v2 (auto-commit on objective fixes).
- If the agent's answers diverge sharply from human judgment → the divergence pattern is itself the most valuable output of this slice. Use it to drive either FAC manifest tightening, prompt revision, or a real product change to whichever MCP tool gave the misleading output.
- If the loop reveals friction tags not currently in `first-agent-cases.json` and they recur on a second run → those are candidates for new FAC cases via a separate spec, not via the loop itself.

## Bottom Line

This loop is an **eval routine**, not an autonomous code-improver. The user's original framing was "self-boosting via dogfood loop" — and self-boosting *is* the desired outcome, but it is not what the loop does directly. The loop does one thing: automate the answering of FAC-001..004 by running them as a Claude Code `/loop` against the live repo, with an agent under test instead of a human filler. v1 is strictly observation-only — same contract, same markdown shape, same review gate.

The "boost" is downstream and indirect: the project improves because the loop produces cheap, repeatable evidence — surface classifications, friction tags, agent-experience notes — that feeds the user's existing plan/spec/PR workflow. The loop never edits product code, never auto-promotes findings, never merges anything.

Auto-commit, scheduled runs, and product-surface promotion are explicitly deferred until one real run gives us evidence about how the simpler version actually behaves.
