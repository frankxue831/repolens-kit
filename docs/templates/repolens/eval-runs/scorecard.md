<!--
Template: eval-runs/scorecard.md
Lives at:  .repolens/eval-runs/<CASE_ID>-<RUN_ID>/scorecard.md  (after `repolens eval start`)
Purpose:   Human-fillable metrics + pass/fail verdict for one ACE run.
Owner:     The reviewer fills this in after the run. RepoLens never auto-grades.
-->

# Eval Scorecard

## Metadata

- **Case:** {{CASE_ID}} — {{CASE_TITLE}}
- **Run ID:** `{{RUN_ID}}`
- **Repo path:**
- **Branch:**
- **HEAD:**
- **Agent / tool:**
- **Agent version:**
- **Human reviewer:**
- **Pack id (if applicable):**
- **Schema version (if applicable):**
- **Worktree dirty (yes/no):**

## Metrics

Numeric metrics use non-negative integers; lower is better. Categorical
metrics use `pass` / `partial` / `fail` / `n/a`. Full per-metric rules are in
**Metric Definitions** below.

See the "Metric definitions" section in `docs/evals/agent-context-evals.md`
(in the repo this run is for) for the canonical reference.

| Metric | Value | Notes |
|---|---:|---|
| time_to_useful_context |  |  |
| repeated_discovery_count |  |  |
| grep_fallback_count |  |  |
| wrong_file_attempts |  |  |
| clarification_count |  |  |
| known_constraint_miss |  |  |
| first_plan_quality |  |  |
| human_intervention_count |  |  |
| session_handoff_quality |  |  |
| stale_checkout_reconciled |  |  |
| memory_safety_notice_used |  |  |

> **How to fill cells.**
> - Leave blank when you did not measure the metric. A blank cell is honest data.
> - Use `n/a` when the metric does not apply to this case (e.g.,
>   `memory_safety_notice_used` for a task that never touches memory).
> - Do not infer or fabricate values. **Do not guess.**
> - Explain every blank or `n/a` in the Notes column so future reviewers can
>   tell "not measured" from "not applicable".

## Metric Definitions

Each entry: **Definition** — what the metric means. **Accepted values** —
what may be written in the Value column. **Leave blank** — when no honest
value can be recorded. **Use `n/a`** — when the metric is not exercised by
this case.

### `time_to_useful_context`

- **Definition.** Time, turns, or command-count from task start until the
  agent identifies the key context needed to make progress (the right file,
  the right constraint, the right prior decision).
- **Accepted values.** A duration (e.g., `90s`, `2m`), a turn count
  (e.g., `4 turns`), a command count (e.g., `7 cmds`), or blank if not
  measured.
- **Leave blank.** When no stopwatch, turn count, or command count was
  recorded during the run.
- **Use `n/a`.** Rare — the metric applies to almost every case.

### `repeated_discovery_count`

- **Definition.** Number of times the agent rediscovers information already
  present in the brief, memory, session memory, or case file.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the transcript was not reviewed for repeats.
- **Use `n/a`.** Only if the case explicitly forbids prior context.

### `grep_fallback_count`

- **Definition.** Number of unstructured search fallbacks (`grep`, `rg`,
  `find`) that occurred *because* structured context (brief, MCP tool,
  memory) was missing or ignored. **Do not** count routine `rg`/`grep` use
  that is the natural tool for the job; count only fallback behavior caused
  by missing or ignored structured context.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the tool log was not reviewed.
- **Use `n/a`.** When the case has no structured context to fall back from.

### `wrong_file_attempts`

- **Definition.** Number of inspected or edited files that were clearly
  irrelevant to the task. "Clearly" means a reviewer can name why the file
  did not belong.
- **Accepted values.**
  - **`0` or any non-negative integer** — file navigation/editing was
    expected or actually occurred. `0` means the agent opened/edited
    files but none were wrong; record the count otherwise.
  - **`n/a`** — the case is plan-only (no source-tree navigation
    expectation) or otherwise has no file-navigation surface.
  - **blank** — the transcript or artifacts are insufficient to judge
    (unknown, not "zero").
- **Leave blank.** When the run did not capture enough evidence to
  decide. **Blank means unknown — not zero.** A plan-only case is `n/a`,
  not blank.
- **Use `n/a`.** When the case is plan-only or otherwise asserts no
  file-navigation expectation.

### `clarification_count`

- **Definition.** Number of human clarifications the agent required because
  it missed, ignored, or lacked available context. Do not count
  clarifications that were already answered in the brief or case.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the transcript was not reviewed for clarifications.
- **Use `n/a`.** When the case has no human in the loop.

### `known_constraint_miss`

- **Definition.** Number of known constraints — from the brief, workspace
  files, case body, or referenced docs — that the agent violated or omitted
  from its plan when relevant.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the agent did not produce a plan.
- **Use `n/a`.** When the case does not assert any constraints.

### `first_plan_quality`

- **Definition.** Quality of the agent's first actionable plan after
  reading the brief and case.
- **Accepted values.** `pass`, `partial`, `fail`, or blank if no plan was
  produced.
- **Leave blank.** When the agent did not produce a first plan (e.g., the
  task did not ask for one).
- **Use `n/a`.** When the case is not about plan quality.

### `human_intervention_count`

- **Definition.** Number of times the human had to correct direction, stop
  unsafe work, or supply context that should have been available to the
  agent.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the human did not interact during the run.
- **Use `n/a`.** When the case explicitly forbids human input.

### `session_handoff_quality`

- **Definition.** Whether the resulting session handoff (e.g., a session
  summary, `repolens remember`, or memory writes) gives the next agent
  enough actionable state to continue without rediscovering the task.
- **Accepted values.** `pass`, `partial`, `fail`, `n/a`, or blank.
  - **`pass`** — prior session memory lets the next agent identify
    **what was done**, **what remains**, and the **next concrete step**
    without rediscovering the task from scratch or asking the human.
  - **`partial`** — the handoff carries useful context, but the next
    step is ambiguous, requires avoidable rediscovery, or is missing
    one of done / remaining / next step.
  - **`fail`** — handoff is absent, misleading, ignored, or causes
    repeated discovery / wrong continuation by the next agent.
  - **`n/a`** — single-session case, or no handoff/resume behavior is
    under test.
  - **blank** — transcript or artifacts are insufficient to judge.
- **Leave blank.** When the run did not capture enough evidence to
  decide pass / partial / fail.
- **Use `n/a`.** When the case is intentionally single-session or does
  not exercise handoff/resume.

### `stale_checkout_reconciled`

- **Definition.** Whether the agent checked repo freshness before
  concluding that shipped behavior was missing or broken. A local upstream
  fast-forward counts as reconciliation if the agent reported `pwd`,
  branch, HEAD/`git status`, checked ahead/behind against the upstream,
  and updated **before** concluding.
- **Accepted values.** `pass`, `partial`, `fail`, `n/a`.
  - `pass` — full freshness check ran *before* the agent's conclusion.
  - `partial` — some freshness signals were collected but the agent still
    concluded from a stale tree.
  - `fail` — the agent concluded shipped behavior was missing without any
    freshness check.
  - `n/a` — the case does not exercise shipped-command dogfood.
- **Leave blank.** Only if the run was abandoned before any conclusion.
- **Use `n/a`.** When the case does not involve dogfooding shipped
  behavior (most non-ACE-010 cases).

### `memory_safety_notice_used`

- **Definition.** Whether the agent noticed and respected memory-safety
  framing when the task involved memory capture, recall, graph storage,
  or session summaries. Memory-safety framing includes:
  - the **standard Memory Safety Notice** banner emitted by `repolens
    brief` for memory-related tasks;
  - **remember-import framing** preserved by `repolens remember --file`
    (e.g., the `**Status:** agent-provided, unverified` and
    `**Use as:** hypothesis until confirmed by …` lines);
  - **provenance / confidence annotations** carried in prior session
    memory (e.g., `Confidence:` / `Provenance:` fields).
- **Accepted values.** `pass`, `partial`, `fail`, `n/a`, or blank.
  - **`pass`** — the agent used or respected the relevant
    memory-safety framing. Two sub-modes:
    - **active pass** — the agent cites or applies the notice before
      writing memory or making memory-derived claims.
    - **passive pass** — the agent consumes prior session memory as
      unverified handoff context, does **not** upgrade it to confirmed
      fact, and does **not** remove or contradict the provenance
      framing.
  - **`partial`** — the agent acknowledged the notice or framing but
    acted inconsistently (e.g., partially upgraded unverified claims).
  - **`fail`** — the agent treated unverified memory as confirmed
    fact, removed provenance/safety framing, wrote typed memory
    unsafely, or relied on prior memory in a way contradicted by
    available evidence.
  - **`n/a`** — the case does not involve prior memory, memory writes,
    or memory-derived claims.
  - **blank** — transcript or artifacts are insufficient to judge.
- **Leave blank.** When the run did not capture enough evidence to
  decide whether framing was respected.
- **Use `n/a`.** For non-memory tasks. Most ACE cases that are not
  about the memory graph or session handoff are `n/a` here.

## Pass / Fail

- **Verdict:**
- **Reason:**

## Baseline comparison (if any)

Compare to the previous run's scorecard for this case (if one exists).
Note any regressions or improvements explicitly.

- **Compared against run:**
- **Better than baseline on:**
- **Worse than baseline on:**
- **Unchanged:**
