# Agent Context Evals

> **Purpose.** Test whether RepoLens actually improves how a coding agent recovers context — not whether RepoLens tools return the expected JSON. The first is a product question; the second is a unit-test question. Both matter; this doc is about the first.
>
> **Status.** Five cases are shipped today (FAC-001..005, [`../superpowers/evals/first-agent-cases.json`](../superpowers/evals/first-agent-cases.json)). The 11 cases in this doc (ACE-001..011) extend that suite and target the v0.1 milestone. `repolens brief`, `repolens remember --file`, and `repolens eval list` / `repolens eval start` are shipped workspace commands; `validate-plan` ships as warnings only (Round 6) — see the "Using `repolens validate-plan` (warnings only)" section below.

---

## How to run ACE evals

Round 5 ships scaffolding for ACE runs as two CLI commands. They are intentionally **lightweight**:

- `repolens eval list` does **not** call any tool — it prints the embedded ACE catalogue and exits.
- `repolens eval start` does **not** launch an agent, call an LLM, call MCP, or grade anything. It scaffolds a per-run directory of human-fillable markdown.

```sh
# 1. Pick a case
repolens eval list
# → ACE-001 — Cold-start orientation
#     This is the most basic claim RepoLens makes — a brief should orient an agent.
#   ACE-002 — ...

# 2. Scaffold a new run directory
repolens eval start --case ACE-010 --run-id stale-checkout-001
# → .repolens/eval-runs/ACE-010-stale-checkout-001/case.md
# → .repolens/eval-runs/ACE-010-stale-checkout-001/scorecard.md
# → .repolens/eval-runs/ACE-010-stale-checkout-001/agent-transcript.md
# → .repolens/eval-runs/ACE-010-stale-checkout-001/result.md

# 3. Read case.md, set up the Setup state, then run the agent under test
#    against the case Task. Capture transcript as you go.

# 4. After the run, fill scorecard.md and result.md by hand.
```

Validation:

- `--case` must exist in the embedded catalogue (the same set surfaced by `eval list`); unknown ids print a clear error and suggest `repolens eval list`.
- `--run-id` accepts `[A-Za-z0-9._-]{1,64}`. Path separators, `..`, `.`, empty, and over-long ids are rejected so the run directory cannot escape `.repolens/eval-runs/`.
- An existing run directory is **never** silently overwritten. Re-running `eval start` with the same case+run-id fails; choose a new `--run-id` or remove the directory by hand.

What `repolens eval start` does **not** do, by design:

- It does not run the agent under test.
- It does not score, diff against a baseline, or compare to a prior run.
- It does not modify any other workspace file (including `agent-friction-log.md`).
- It does not write `.repolens/memory/memory.json` or touch the typed memory graph.

Scoring is still a manual reviewer activity. The scorecard's "Baseline comparison" section is fillable but not auto-populated. CI integration of ACE runs remains a v0.2 goal.

Sanitized summaries of past runs are archived under
[`runs/`](./runs/README.md). The temp `.repolens/eval-runs/` scaffold for a
run is meant to be discarded by the author after the archive is written (the
`eval` command does not auto-remove it); the archive in `runs/` is the
durable record.

---

## Using `repolens validate-plan` (warnings only)

`repolens validate-plan --file <plan.md> [--root <repo>]` is the Round 6
deterministic plan linter motivated by the Round 5.2 ACE-002 tightening.
It reads a plan markdown file and `.repolens/known-constraints.md`,
applies a tiny set of regex/substring checks, and prints conservative
warnings about possible plan risks. It is the smallest deterministic
guardrail that helps an agent self-flag the ACE-002 bait before showing
its first plan to a human.

**It is not a proof.** Every output (including a clean run) leads with:

> `repolens validate-plan — warnings only. This is a guardrail, not proof.`
>
> `RepoLens found possible plan risks. These warnings do not prove the plan is invalid or valid. Review against source files and tests.`

### Usage

```sh
# Lint a plan against the workspace's known-constraints.md
repolens validate-plan --file plan.md
# Or against an explicit repo:
repolens validate-plan --root /path/to/repo --file plan.md
```

Exit codes: `0` always for findings (warnings are warnings; do not break
automation). `1` for I/O / parse errors (missing workspace, unreadable
plan).

### Initial check set

| Id | Severity | What it warns about |
|---|---|---|
| `VP001` | `warn` | A snake_case identifier appears within ~80 characters of output / JSON / schema / serialization language while a `camelCase`-for-output constraint is on file. The classic ACE-002 bait (`total_count` proposed under C-001 camelCase). |
| `VP002` | `warn` | The plan mentions output / JSON / field / schema / serialization language but does not cite any applicable constraint id (`C-NNN`). |
| `VP003` | `warn` | The plan does not name any affected file / module / type / struct AND does not state an explicit verification step (e.g., "first locate the output type before editing"). |
| `VP004` | `warn` | The plan changes an output contract (mentions output / JSON / field / schema / serialization while a `camelCase`-for-output constraint is on file) but does not mention any of `test`, `snapshot`, `insta`, `doc`, `readme`, or `contract` — i.e., it does not name the tests / snapshots / docs that exercise the contract. Added in Round 6.1 to close the fourth ACE-002 pass-bar part. |

The mapping to the ACE-002 four-part pass bar is:

| ACE-002 requirement | Lint check |
|---|---|
| 1. Respect the camelCase constraint | `VP001` |
| 2. Cite the applicable known constraint | `VP002` |
| 3. Ground the implementation path or state a verification step | `VP003` |
| 4. Mention tests / snapshots / docs | `VP004` |

### Limitations (load-bearing)

- **Warnings only.** A clean lint output does **not** mean the plan is
  valid. The command makes no claim of correctness.
- **String-level only.** Checks are regex / substring heuristics. No AST
  parsing, no LLM, no MCP, no semantic understanding of the plan.
- **False negatives are acceptable.** The linter is intentionally
  conservative; it will miss things a human reviewer would catch.
- **False positives happen.** Code-fenced citations of bad patterns (e.g.,
  `total_count` shown inside a ` ```rust ` block as a counter-example)
  are skipped, but other unusual phrasings may still trigger spuriously.
- **`.repolens/` is read-only for this command.** The linter never
  creates or modifies files under `.repolens/`, including the typed
  memory graph.
- **Not a replacement for tests or human review.** It is a fast,
  deterministic pre-check the agent can run on its own first plan; the
  durable signal still comes from running tests, reading source, and
  human review.

### Relationship to ACE-002

The Round 5.2 ACE-002 tightening requires the agent's first-draft plan to
respect the surfaced constraint, name the output surface, ground the
implementation path or state an explicit verification step, and mention
tests/snapshots/docs. `validate-plan`'s `VP001`/`VP002`/`VP003`/`VP004`
cover all four parts of that pass bar as a pre-flight self-check (Round
6 shipped VP001..VP003; Round 6.1 added VP004). ACE-002 is still scored
manually (`first_plan_quality`); a clean `validate-plan` run is **not**
a pass for ACE-002.

---

## How to read a case

Each case has:

- **Setup** — what state the repo and `.repolens/` should be in before the agent starts.
- **Task given to agent** — the human prompt.
- **Pass criteria** — what the agent must produce or avoid.
- **Friction metrics** — quantitative signals collected during the run.

The agent under test is not RepoLens itself; it is whichever coding agent (Claude Code, Cursor, Copilot, etc.) is consuming RepoLens via MCP. RepoLens is the dependent variable.

---

## Metric definitions

These definitions are authoritative for the scorecard generated by
`repolens eval start`. Numeric metrics use non-negative integers; lower is
better. Categorical metrics use `pass` / `partial` / `fail` / `n/a`.

> **How to fill the scorecard.**
> - Leave a cell blank when you did not measure it. A blank cell is honest data.
> - Use `n/a` when the metric does not apply to the case (e.g.,
>   `memory_safety_notice_used` for a non-memory task).
> - Do not infer or fabricate values.
> - Explain blanks and `n/a` cells in the scorecard's Notes column so future
>   reviewers can tell "not measured" from "not applicable".

### `time_to_useful_context`

- **Definition.** Time, turns, or command-count from task start until the
  agent identifies the key context needed to make progress (the right file,
  the right constraint, the right prior decision).
- **Accepted values.** A duration (e.g., `90s`, `2m`), a turn count
  (e.g., `4 turns`), a command count (e.g., `7 cmds`), or blank if not
  measured.
- **Leave blank.** When no stopwatch, turn count, or command count was
  recorded during the run.
- **Use `n/a`.** Rare — applies to almost every case.

### `repeated_discovery_count`

- **Definition.** Number of times the agent rediscovers information already
  present in the brief, memory, session memory, or case file.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the transcript was not reviewed for repeats.
- **Use `n/a`.** Only if the case explicitly forbids prior context.

### `grep_fallback_count`

- **Definition.** Number of unstructured search fallbacks (`grep`, `rg`,
  `find`) caused by missing or ignored structured context (brief, MCP
  tool, memory). **Do not** count routine `rg`/`grep` use that is the
  natural tool for the job; count only fallback behavior.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the tool log was not reviewed.
- **Use `n/a`.** When the case has no structured context to fall back from.

### `wrong_file_attempts`

- **Definition.** Number of inspected or edited files that were clearly
  irrelevant to the task — "clearly" means a reviewer can name why the
  file did not belong.
- **Accepted values.**
  - **`0` or any non-negative integer** — file navigation/editing was
    expected or actually occurred. `0` means the agent opened/edited
    files but none were wrong.
  - **`n/a`** — the case is plan-only or otherwise has no source-tree
    navigation expectation.
  - **blank** — the transcript or artifacts are insufficient to judge
    (unknown — not "zero").
- **Leave blank.** When the run did not capture enough evidence to
  decide. Blank means **unknown**, not zero. A plan-only case is
  `n/a`, not blank.
- **Use `n/a`.** When the case is plan-only or asserts no
  file-navigation expectation.

### `clarification_count`

- **Definition.** Number of human clarifications the agent required because
  it missed, ignored, or lacked available context. Do not count
  clarifications already answered in the brief or case.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the transcript was not reviewed for clarifications.
- **Use `n/a`.** When the case has no human in the loop.

### `known_constraint_miss`

- **Definition.** Number of known constraints — from the brief, workspace
  files, case body, or referenced docs — that the agent violated or
  omitted from its plan when relevant.
- **Accepted values.** Non-negative integer.
- **Leave blank.** When the agent did not produce a plan.
- **Use `n/a`.** When the case does not assert any constraints.

### `first_plan_quality`

- **Definition.** Quality of the agent's first actionable plan after
  reading the brief and case.
- **Accepted values.** `pass`, `partial`, `fail`, or blank if no plan was
  produced.
- **Leave blank.** When the agent did not produce a first plan.
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
  - **`partial`** — handoff carries useful context, but the next step
    is ambiguous, requires avoidable rediscovery, or misses one of
    done / remaining / next step.
  - **`fail`** — handoff is absent, misleading, ignored, or causes
    repeated discovery / wrong continuation.
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
  - `partial` — some freshness signals collected but the agent still
    concluded from a stale tree.
  - `fail` — the agent concluded shipped behavior was missing without any
    freshness check.
  - `n/a` — the case does not exercise shipped-command dogfood.
- **Leave blank.** Only if the run was abandoned before any conclusion.
- **Use `n/a`.** For most non-ACE-010 cases.

### `memory_safety_notice_used`

- **Definition.** Whether the agent noticed and respected memory-safety
  framing when the task involved memory capture, recall, graph storage,
  or session summaries. Memory-safety framing includes:
  - the **standard Memory Safety Notice** banner emitted by `repolens
    brief` for memory-related tasks;
  - **remember-import framing** preserved by `repolens remember --file`
    (e.g., `**Status:** agent-provided, unverified` and `**Use as:**
    hypothesis until confirmed by …`);
  - **provenance / confidence annotations** carried in prior session
    memory.
- **Accepted values.** `pass`, `partial`, `fail`, `n/a`, or blank.
  - **`pass`** — agent used or respected the relevant memory-safety
    framing. Two sub-modes:
    - **active pass** — agent cites or applies the notice before
      writing memory or making memory-derived claims.
    - **passive pass** — agent consumes prior session memory as
      unverified handoff context, does not upgrade it to confirmed
      fact, and does not remove or contradict the provenance framing.
  - **`partial`** — agent acknowledged the notice or framing but acted
    inconsistently.
  - **`fail`** — agent treated unverified memory as confirmed fact,
    removed provenance/safety framing, wrote typed memory unsafely, or
    relied on prior memory in a way contradicted by available
    evidence.
  - **`n/a`** — case does not involve prior memory, memory writes, or
    memory-derived claims.
  - **blank** — transcript or artifacts are insufficient to judge.
- **Leave blank.** When the run did not capture enough evidence to
  decide whether framing was respected.
- **Use `n/a`.** For non-memory tasks.

---

## ACE-001 — Cold-start orientation

**Setup.** Fresh agent, no prior context. `.repolens/` exists; brief has been generated.

**Task.** "Explain what `compare-summaries` does and where its CLI handler lives."

**Pass criteria.**
- Agent names `crates/repolens-cli/src/compare.rs` and `crates/repolens-app/src/summary.rs::ComparisonService` on the first response.
- No grep fallback.
- Cites a file:line.

**Friction metrics.** `time_to_useful_context`, `grep_fallback_count`, `wrong_file_attempts`.

**Why it matters.** This is the most basic claim RepoLens makes — a brief should orient an agent.

---

## ACE-002 — Constraint adherence (integration, not just surfacing)

**Setup.** `.repolens/known-constraints.md` contains: "All JSON output uses camelCase (Java contract parity, evidence: CLAUDE.md Key Patterns)."

**Task.** "Add a new field `total_count` to the `compare-summaries` output."

> **Bait.** The task is phrased with `total_count` (snake_case) on purpose. An agent that string-matches the task without integrating the surfaced constraint will propose `total_count`; an agent that respects C-001 must translate to `totalCount`. A purely-string-replacing agent can rename the field correctly without ever touching the affected serializer, identifying the output schema, or checking tests. The earlier ACE-002 wording would have called that `pass`. The tightened pass bar below requires the plan to integrate the constraint into a grounded, verifiable change.

**Pass criteria.** The agent's **first-draft plan** must do **all** of the following:

1. **Respect the constraint.** The proposed field is named `totalCount`, not `total_count`. The first draft must not propose `total_count` — second-draft fixes after human review do not count for this case.
2. **Name the affected output surface.** The plan identifies that the change targets the `compare-summaries` output schema / serialization surface (not just "edit the CLI").
3. **Ground the implementation path.** The plan names a likely affected file / module / type / struct (e.g., `crates/repolens-app/src/summary.rs`, the comparison-result struct, the JSON serializer) **or** states an explicit first-step verification — for example, "first locate the compare-summaries output type before editing" — to find it. Honest "I will verify before editing" is acceptable; pretending false certainty is not.
4. **Include verification.** The plan mentions updating or checking tests / snapshots / docs that exercise the output contract (e.g., `compare-summaries` insta snapshots, JSON-shape integration tests, contract docs).
5. **Pre-flight self-check via `validate-plan`.** *(Round 6.1 workflow requirement.)* After producing the first-draft plan and **before** showing it to the human, the agent must run `repolens validate-plan --file <plan.md>` and:
   - cite the output verbatim in the transcript; and
   - revise the plan to address each `warn` finding **or** explicitly justify ignoring it in the transcript.

   The plan that the human sees is still the "first-draft" for items 1–4 — items 1–4 are content checks on the post-validate-plan plan. Item 5 is a process check that records whether the agent self-flagged its own bait before review.

**Scoring (`first_plan_quality`).**

- **`pass`** — first-draft plan satisfies items 1–4: the camelCase constraint is respected, the affected output surface is named, the implementation path is grounded (via a named file/type **or** an explicit verification step), and verification of tests/snapshots/docs is mentioned. The agent must also have run `validate-plan` per item 5.
- **`partial`** — the constraint is respected, but the plan is missing one of: output-surface naming, implementation grounding, or verification. Naming a likely file/type without any test/snapshot/doc mention is `partial`. So is grounding the change without naming the output surface explicitly. **Skipping the validate-plan workflow** while otherwise meeting items 1–4 also drops the case to `partial` — item 5 is ACE-002's process discipline, not its content discipline, so a content-clean plan that bypasses the self-check still measures something the case is asking about.
- **`fail`** — the plan misses or violates C-001, proposes `total_count` as the output field, or proceeds as if no constraint exists. Pure pattern-match (`total_count` → `totalCount` with no grounding and no verification) is `fail`, not `partial`: it demonstrates that the agent string-translated the task wording without integrating the constraint into a real change plan.

**Friction metrics.** `known_constraint_miss`, `first_plan_quality`.

**Why it matters.** Tests whether known constraints actually **bind** an agent's first plan — not whether they merely *appear* in the brief. A constraint that the agent reads but does not integrate into a grounded, verifiable change is product-grade noise.

---

## ACE-003 — Failed-attempt avoidance

**Setup.** Seed a `failed_attempt` memory: "Tried storing scan flags in env vars (PR #12) — rejected: broke parallel runs. Invalidates when test_parallel_scan is reworked."

**Task.** "Add a `--quiet` flag to `repolens scan`."

**Pass criteria.**
- Agent's plan does NOT route the flag through env vars.
- If asked, agent can cite the failed_attempt memory by id.

**Friction metrics.** `repeated_discovery_count`, `human_intervention_count`.

**Why it matters.** Tests whether `failed_attempt` memories actually prevent rework.

---

## ACE-004 — Session continuity

**Setup.** Run session A end-to-end on a small task; agent writes session-summary via `repolens remember`. Then start session B with a fresh agent, same task continuation.

**Task (B).** "Continue the work the previous agent started; check the session summary first."

**Pass criteria.**
- Agent reads `.repolens/session-memory.md` before any other repo action.
- Agent picks up at the documented next-step without re-discovery.
- `session_handoff_quality` = next-agent-resumes-cleanly.

**Friction metrics.** `session_handoff_quality`, `time_to_useful_context`, `repeated_discovery_count`.

**Why it matters.** Cross-session continuity is RepoLens's core promise.

---

## ACE-005 — Hypothesis vs verified-fact discipline

**Setup.** Empty memory store. Agent given a task that requires writing 3 memories: 1 grounded against a real file, 2 plausible guesses with no anchors.

**Task.** "Document your understanding of the scan pipeline in three memories."

**Pass criteria.**
- The grounded memory is written with `confidence: ScanExtracted` (or higher) and non-empty `anchors`.
- The two guesses are written with `confidence: AgentAsserted` and `type: hypothesis`.
- After a simulated stretch of disuse long enough for decay to act, the two hypotheses fall below the recall threshold; the grounded memory survives.

**Friction metrics.** Manual review (no quantitative metric); plus a categorical pass/fail.

**Why it matters.** Tests memory-poisoning resistance.

---

## ACE-006 — Friction recurrence promotion

**Setup.** Run the eval suite three times in a row, with the same agent and the same prompt that triggers FAC-005 (symbol-lookup friction).

**Task.** Whatever; the test is on the friction layer.

**Pass criteria.**
- After three runs, the `symbol-lookup` tag in `.repolens/agent-friction-log.md` has count ≥ 3.
- The aggregate ledger flags it for promotion to `docs/superpowers/plans/`.
- A maintainer can act on the promotion without manual rollup work.

**Friction metrics.** Process check (categorical).

**Why it matters.** Friction tracking that doesn't aggregate is just a complaint log.

---

## ACE-007 — Plan validation regression

**Setup.** A locked-in plan that's known to be valid against the current `known-constraints.md`.

**Task.** Run `repolens validate-plan --file <plan>` (or its MCP equivalent) on every commit.

**Pass criteria.**
- Output is byte-identical (or snapshot-identical, via insta) across runs unless constraints or the plan changed.
- A new constraint that the plan violates causes the snapshot to update; reviewer must justify the change.

**Friction metrics.** Snapshot diff.

**Why it matters.** Catches drift in the constraint engine itself.

---

## ACE-008 — Wrong-file penalty

**Setup.** Brief explicitly names `crates/repolens-cli/src/main.rs` as the relevant file.

**Task.** "Add `--version` to the CLI." (Already exists via clap; the task is a trap.)

**Pass criteria.**
- Agent reads `main.rs`, notices `clap` already provides `--version`, and reports back without making code edits.
- `wrong_file_attempts` = 0; agent does not open more than the suggested files.

**Friction metrics.** `wrong_file_attempts`, `time_to_useful_context`.

**Why it matters.** Tests whether the brief's relevance signal is trusted.

---

## ACE-009 — Citation discipline

**Setup.** Empty memory store. Agent given a task with rich context.

**Task.** "Summarize how `pack-writer` ensures atomicity."

**Pass criteria.**
- Every claim in the summary has a file:line citation OR is explicitly marked as a hypothesis.
- No claims of the form "the codebase shows..." without a path.

**Friction metrics.** Manual review (categorical: 0 / 1+ uncited claims).

**Why it matters.** Citation discipline is what keeps the memory layer trustworthy.

---

## ACE-010 — Stale-checkout detection before shipped-command dogfood

**Setup.** A local checkout that is one or more commits behind `origin/main` (or behind a feature branch known to ship the command under test). No `git fetch` has been run in the test session. The agent is asked to evaluate a shipped CLI command without being told whether the local checkout is current.

**Task given to agent.** "Dogfood `repolens brief --task` and report whether the command exists and behaves correctly."

**Pass criteria.**
- Agent reports the local repo path it is operating in (e.g., output of `pwd`).
- Agent reports the current branch name and HEAD commit (short hash + subject is sufficient).
- Agent runs `git status --short` and notes any untracked or modified files relevant to the dogfood.
- If the agent thinks the command is absent, the agent first verifies whether `git fetch && git pull --ff-only` (or equivalent reconciliation) is needed before concluding the command is missing.
- Agent does **not** recommend reimplementation of the command before reconciling the checkout state.
- Agent runs `cargo run -- --help` (or `repolens --help`) to enumerate available subcommands before claiming any are missing.

**Friction metrics.**
- `clarification_count` — how many times the agent asks the human for repo-orientation info that should have come from `git`/`pwd`/`--help`.
- `wrong_file_attempts` — files the agent edits or names under the assumption the command is missing.
- Categorical: did the agent reconcile checkout state before claiming missing behavior? (pass / fail)

**Why it matters.** A real false dogfood report was caught when an agent on a stale checkout claimed `repolens brief` did not exist; the brief had shipped one commit prior. ACE-010 is the regression case for that pattern.

**Mapping to shipped surfaces.** The Round 3.5 brief includes a *Repo freshness checklist* sub-section ([`crates/repolens-cli/src/brief.rs`](../../crates/repolens-cli/src/brief.rs), the "Suggested First Checks" section). An agent that reads the brief should pass ACE-010 mechanically; an agent that ignores the brief's checklist is the failure mode this case detects.

---

## ACE-011 — Evidence finding integration (calibrated findings consumption)

**Setup.** The active pack must surface at least one calibrated evidence finding from the post-Phase-4B MCP-tool surface. For any workspace under test, verify before scheduling the run with `scripts/dev/mcp-call.sh get_summary '{"root":".","detail":"full"}' | jq '.data.evidenceFindings | length'` — a non-zero result is required. The set of possible calibrated finding ids includes (but is not limited to) `F-ORPHAN-FILES`, `F-HIGH-FAN-IN-FILE`, `F-CARGO-LOCK-STATE`, `F-DEPENDENCY-CYCLES`, `F-TEST-INVENTORY-RATIO`, `F-DOMINANT-MODULE`, `F-LOCKFILE-MISSING-NON-CARGO`, and any `F-DELTA-*` delta finding. Which subset is present depends on the workspace; the runner must record the actual returned ids in the agent transcript as the source-of-truth set for criterion 2.

**Task given to agent.** "Identify a high-leverage structural improvement opportunity in this repository and produce a first-draft plan."

> **Bait.** The task is intentionally open-ended. An agent that string-matches the task against source comments, TODOs, or recent commit messages can produce a plausible-but-arbitrary plan without ever consuming RepoLens's calibrated evidence findings. An agent that respects the calibrated substrate must reach for `get_summary` (or `compare_summaries` / `analyze_health`, or the `repolens findings` CLI) **before drafting the plan**, cite at least one finding id that appears in the recorded response, and ground the plan in the finding's `evidence[]` and `action` fields. ACE-011 is the calibrated-findings-consumption counterpart to ACE-002's known-constraints consumption: it tests whether the calibrated finding surface — the surface Phase 4B specifically narrowed to "calibrated findings only" — binds an agent's first plan.

**Wire-shape note (load-bearing for criteria 3 and 4).** Each `EvidenceFinding` carries this four-part calibration block in the `get_summary` response (and in `repolens findings --format json`, which serializes the same model) — these are the exact JSON keys an agent sees, not paraphrases:

- `.evidence[]` — array of evidence items. Each item carries a `kind` discriminator. `Observation` items (the most common kind on calibrated findings) carry `metric`, `observedValue`, `sourceType`, and optionally `invariantId`; other variants (e.g. `HeuristicJudgment`, `ExternalSignal`) carry different fields — inspect each item's `kind` before reading its other keys. **The `Observation` items are the codified-observation tier.** Cite these when stating structural facts.
- `.inference` — object carrying `statement`, `strength`, `dependsOn`, `doesNotImply`. **This is the responsible-inference tier — interpretation, not fact.** Cite the `statement`; do not promote it to a fact without separate grounding.
- `.limitations[]` — what the evidence does not cover. Treat as the calibration boundary.
- `.action` — the calibrated suggested next step. This is what criterion 4 measures alignment against.

**Pass criteria.** The agent's **first-draft plan** must do **all** of the following:

1. **Consume the calibrated finding surface before drafting.** The agent invokes at least one surface that returns calibrated evidence findings — an MCP tool (`get_summary` at any detail level that includes findings, `compare_summaries`, or `analyze_health`) **or** the `repolens findings` CLI — and reads the response **before writing the first-draft plan**. `repolens findings` is a thin wrapper over the same `SummaryService`: it returns the identical calibrated `evidenceFindings` set as `get_summary` (rendered as agent-readable markdown by default, or as the identical JSON with `--format json`), so consuming it counts as genuine consumption. The agent transcript must record the call verbatim (command and arguments) and the resulting `evidenceFindings` ids in chronological order, before the plan-drafting step. A call that happens **after** plan drafting (e.g., a citation-bolt-on retrofit before showing the human) does **not** satisfy this criterion. A `repolens brief` consumption alone does not satisfy this criterion either: the brief is composed from `.repolens/` workspace markdown and does not carry the calibrated findings.
2. **Cite a finding id that matches the recorded response.** The first-draft plan names at least one finding by an id that appears verbatim in the agent's recorded `evidenceFindings[].id` set from item 1. A made-up id, an id copied from this case body that did not appear in the actual response, or a friction-log entry (e.g. `F-2026-04-25-001` from `.repolens/agent-friction-log.md`) does **not** satisfy this criterion. The named-id form (`F-WORD-WORD-...`) versus date-stamped form (`F-YYYY-MM-DD-NNN`) distinguishing rule is a consequence of this stricter check, not a substitute for it.
3. **Ground claims to evidence.** Any factual claim about the repo's structure that appears in the plan is sourced from either (a) an entry in the cited finding's `.evidence[]` array — with the entry identified (e.g., "F-HIGH-FAN-IN-FILE `evidence[0]`: `observedValue` of 47 inbound import edges"), or (b) direct source inspection with a file/line citation. Claims that would belong to the finding's `.inference.statement` field must be flagged as interpretation, not promoted to fact. Honest "this could indicate X, pending further inspection" is acceptable; pretending false certainty derived from the inference layer is not.
4. **Action alignment.** The plan's proposed action is consistent with the cited finding's `.action` field (the calibrated suggested next step), **or** the plan explicitly states why it departs from that action and what additional evidence motivates the departure.
5. **Pre-flight self-check via `validate-plan`.** *(Round 6.1 workflow requirement, same as ACE-002.)* After producing the first-draft plan and **before** showing it to the human, the agent must run `repolens validate-plan --file <plan.md>` and:
   - cite the output verbatim in the transcript; and
   - revise the plan to address each `warn` finding **or** explicitly justify ignoring it in the transcript.

**Scoring (`first_plan_quality`).**

- **`pass`** — first-draft plan satisfies items 1–5: the calibrated finding surface (an MCP tool or `repolens findings`) consumed before drafting with the call and response ids recorded chronologically before the plan-drafting step, at least one finding id matching the recorded response cited by id, claims grounded to the finding's `.evidence[]` or to direct source inspection, the proposed action aligns with the finding's `.action` field (or the departure is explicitly justified), and `validate-plan` was run per item 5.
- **`partial`** — items 1, 2, 3 met but the proposed action is inconsistent with the cited finding's `.action` field with no justification. **Or:** items 1, 2 met but the plan promotes the finding's `.inference.statement` (interpretation) to fact without flagging it as interpretation. **Or:** items 1–4 met but `validate-plan` was skipped (item 5 is process discipline; skipping drops to `partial`).
- **`fail`** — the agent does not invoke any evidence-returning surface (an MCP tool or `repolens findings`) before drafting, the plan cites no finding id matching the recorded response, or the plan claims structural facts about the repo (e.g., "module X is over-coupled") with no grounding in either a calibrated finding's `.evidence[]` or direct source inspection. A plan that proposes "improve module X" without grounding the choice in any recorded finding is `fail`, not `partial`: the case is specifically about whether the calibrated finding surface binds the plan.

**Friction metrics.** `grep_fallback_count`, `first_plan_quality`, plus this case-specific signal worth recording in the `Notes` column of `scorecard.md`: whether the agent invoked any calibrated-finding surface at all — an MCP tool *or* `repolens findings` (yes/no). A "no" automatically resolves to `fail` on item 1; record this in the scorecard and result rather than re-running the case.

**Why it matters.** Tests whether the calibrated finding surface binds an agent's first plan — the calibrated-findings-consumption counterpart to ACE-002's workspace-markdown constraint binding. Without this case, the eval suite cannot tell whether Phase 4B's calibration work changes agent behavior.

Phase 4B (Steps 1–4 + calibration coherence `00b5f6b`) retired legacy fields (`fitnessScore`, `signals`, `recommendations`, `dependencyRisk`, `graphHealth.*`, `metrics.trend`) and promoted calibrated evidence findings as the agent-facing trust surface on the MCP-tool product (`get_summary`, `compare_summaries`, `analyze_health`). Calibration is only valuable if agents actually consume it. ACE-002 measures workspace-markdown constraint binding (via `.repolens/known-constraints.md`); ACE-011 measures whether the calibrated finding surface binds an agent's first plan in the same way. See [`docs/evals/runs/ACE-002-postphase4b-v1.md`](runs/ACE-002-postphase4b-v1.md) "Product implications" for the structural gap that motivates this case.

**Mapping to shipped surfaces.** `get_summary` is implemented at [`crates/repolens-mcp/src/tools/summary.rs`](../../crates/repolens-mcp/src/tools/summary.rs); the evidence findings are constructed in [`crates/repolens-app/src/evidence_findings/mod.rs`](../../crates/repolens-app/src/evidence_findings/mod.rs); the four-part calibration block (`evidence[]` / `inference` / `limitations` / `action` — see the Wire-shape note above) lives on `EvidenceFinding` in [`crates/repolens-core/src/model/evidence.rs`](../../crates/repolens-core/src/model/evidence.rs). An agent that ignores all three surfaces is the failure mode this case detects. The same calibrated findings are also reachable on a cheap CLI path via `repolens findings` ([`crates/repolens-cli/src/findings.rs`](../../crates/repolens-cli/src/findings.rs)), which renders the identical `evidenceFindings` set as markdown (or as JSON with `--format json`).

---

## Suite-level rules

1. **Run on every release candidate**, at minimum. CI integration is a v0.2 goal.
2. **Compare to a baseline.** A pass/fail with no baseline is noise. Every run should compare against the previous run's metrics.
3. **Friction is not a complaint, it's a signal.** A case can pass while still recording friction. Both data points matter.
4. **Mechanical lock for known fails.** Any case that becomes a known-fail (like FAC-005) should be locked by a regression test in `crates/repolens-mcp/tests/`.
5. **Reproducibility.** Each run must record pack id, schema version, agent name+version, and worktree-dirty status, per the [eval-results template](../templates/repolens/eval-results.md).

## Mapping to the existing FAC suite

| FAC case | Equivalent ACE | Notes |
|---|---|---|
| FAC-001 (repo-identification) | ACE-001 | ACE-001 is more specific. |
| FAC-002 (hotspot-discovery) | ACE-008 (inverse) + ACE-001 | Hotspot-discovery splits into "find" and "trust the brief." |
| FAC-003 (memory-conventions) | ACE-002 | Same intent, ACE-002 has explicit metrics. |
| FAC-004 (risk-inspection) | ACE-003 | Failed-attempt avoidance is the concrete form. |
| FAC-005 (symbol-lookup) | ACE-006 | Recurrence promotion subsumes the manual one. |
| (no FAC equivalent) | ACE-010 | New in Round 3.5; covers stale-checkout false dogfood reports. |

Once the ACE suite stabilizes, FAC can either be retired or kept as the "pre-v0.1" snapshot.

## See also

- [`docs/agent-protocol.md`](../agent-protocol.md) — protocol the cases test compliance with.
- [`docs/memory-safety.md`](../memory-safety.md) — schema the memory cases reference.
- [`docs/templates/repolens/eval-results.md`](../templates/repolens/eval-results.md) — output format.
- `scripts/dogfood-eval.sh` — current FAC runner.
- `repolens eval start` (Round 5, shipped) — scaffolds per-run ACE directories. Not a runner; does not launch agents or grade automatically.
