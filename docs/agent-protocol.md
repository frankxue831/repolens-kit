# Agent Protocol

> **Audience:** AI coding agents (Claude Code, Cursor, Copilot, Codex, Devin, etc.) using RepoLens as a memory and context layer.
>
> **Status:** `repolens init` (Round 2), `repolens brief --task` (Round 3), `repolens remember --file` (Round 4), `repolens eval list` / `repolens eval start` (Round 5), `repolens validate-plan --file` (Round 6), and `repolens state` ship today. `state` is read-only local inspection of scan packs, typed memory, and session handoff markdown. `validate-plan` is warnings only — a guardrail, not proof. `remember` records workspace markdown only; it does not write the typed memory graph.

This protocol exists for one reason: to keep agent assumptions from quietly becoming permanent facts. It is not a style guide. It is a memory-safety contract.

---

## The 9 steps

### 1. Request a task brief before editing

Before reading code with the intent to change it, ask RepoLens for the task-specific brief.

- **Shipped (Round 3):** `repolens brief --task "<task description>"` writes `.repolens/current-agent-brief.md` deterministically from `.repolens/` workspace files. It does not yet read the scan pack, call MCP tools, or use an LLM — agents wanting richer context should still compose `get_summary` + `get_conventions` + `recall_longterm` via MCP on top of the brief.
- **Shipped:** `repolens state` shows the local persistence surfaces side-by-side before you rely on them: scan packs are derived and regenerable, `.repolens/memory/memory.json` is the typed memory graph, and `.repolens/session-memory.md` is unverified handoff markdown.
- **Shipped today (MCP):** call `get_summary` (tier `standard`), then `get_conventions`, then `recall_longterm` with keywords from the task. Compose them yourself.

A good brief contains: repo overview, relevant files, conventions, prior failed attempts, relevant tests, suggested first checks, risks, acceptance criteria, things not to change, and open questions. If yours does not, either compose it yourself from the MCP tools or ask the human for clarification.

### 2. Inspect constraints and prior attempts before planning

Before drafting the plan, read:

- `.repolens/known-constraints.md` (or call `get_conventions` and recall memories of type `Constraint` once that type ships).
- Memories of type `FailedAttempt` related to the task (planned type — until then, search `recall_longterm` for "tried", "rolled back", "rejected", "failed").
- Memories with `groundingStatus: Contradicted` — these are warnings.

If a `FailedAttempt` matches your intended approach, **do not repeat it without understanding why it failed**.

### 3. Cite when making repo claims

Every claim about the repo must point to one of:

- A specific file path and line number.
- A docs path under `docs/`.
- A test file under `crates/*/tests/`.
- A memory entry with its `id`.
- The current pack manifest (e.g., `.repolens/pack-YYYYMMDD-HHMMSS/manifest.json`).

Claims without citations are hypotheses. Mark them as such.

### 4. Produce a short plan before changing code

A plan is at minimum:

- The task in one sentence.
- The files you intend to change, with rationale.
- The tests you will rely on (existing or new).
- The risks you are aware of.
- The acceptance criteria.

Write the plan to a markdown file. Do not skip this step for "small" changes; the plan is what `validate-plan` consumes.

### 5. Validate the plan against known constraints

- **Shipped (Round 6, warnings only; VP004 added in Round 6.1):** `repolens validate-plan --file plan.md [--root <repo>]` applies four deterministic checks against the plan and `.repolens/known-constraints.md`: **VP001** (snake_case identifier near output / JSON / schema language while a camelCase-for-output constraint is on file), **VP002** (output language without an applicable constraint id citation), **VP003** (no named affected file/module/type AND no explicit verification step), **VP004** (output-contract change under a camelCase-for-output constraint without a test / snapshot / doc mention). Output always begins with a "warnings only — a guardrail, not proof" disclaimer. Exit code is `0` for findings; `1` for I/O / parse errors.
- **Fallback (no RepoLens workspace):** read `known-constraints.md` (or `get_conventions`) and walk through your plan line by line, asking "does this violate any rule here?" Document the answer.

A constraint hit is not automatically a stop sign — it is a "stop and explain." If you choose to override a constraint, record the override and rationale in your session summary.

### 6. Run suggested tests

Read the brief's "test guidance" section and the project's test commands (in this repo, `CLAUDE.md` and `AGENTS.md` have them). Before declaring work complete:

- Run the listed test commands.
- If snapshot tests changed, run the project's snapshot review command (`cargo insta review` here) and explain each accepted change.
- Do not claim "tests pass" unless you ran them in this session and saw the output.

### 7. Write a session summary before ending

Before the session ends, write a session summary covering:

- What you set out to do.
- What you actually did, with file references.
- What you tried and rolled back (these are candidates for `FailedAttempt` memories once that type ships).
- What surprised you (these are friction findings — log them).
- What you are unsure about (these are hypotheses, not facts).
- What the next agent should know.

- **Shipped (Round 4):** `repolens remember --file session-summary.md` records the summary in `.repolens/session-memory.md` as agent-provided, unverified workspace markdown. Treat it as a handoff hypothesis until confirmed by files, tests, docs, or human review.
- **Typed memory graph:** `remember` does not write `.repolens/memory/memory.json`. If you need typed long-term memory today, call `memorize_insight` per finding, with anchors.

### 8. Mark uncertain findings as hypotheses, not facts

Default confidence is `AgentAsserted` (planned `Confidence` enum). To upgrade to `verified_fact`, you need one of:

- A passing test that exercises the claim.
- A human-confirmed marker in the session summary.
- Source-code inspection with file + line citation, recorded in the memory's `evidence` field.

If you cannot supply one of those, write the memory as type `hypothesis`. This is the single most important rule. **Wrong agent guesses turning into permanent facts is the failure mode this whole tool exists to prevent.**

### 9. Never upgrade agent guesses into verified facts without evidence

Specifically:

- Do not edit an existing `Hypothesis` memory's type to `verified_fact` unless step 8's evidence rule is met.
- Do not write `confidence: HumanVerified` for a memory the human has not confirmed in writing.
- Do not paper over a `Contradicted` recall by re-asserting the same memory under a new id. Instead, write a `Contradicts` edge or supersede it with a new memory and state why.
- Do not use `forget_insight` to silently delete contradicting memories. If you forget, document why in the session summary.

---

## Anti-patterns to avoid

| Anti-pattern | Why it's harmful |
|---|---|
| Skipping step 1 because "this is a small change" | Small changes are where unstated constraints get violated most often. |
| Citing "the codebase shows" without file:line | Unverifiable assertion → ungrounded memory. |
| Writing high-importance memories with no anchors | They will return `Unchecked` forever and never be auto-corrected. |
| Re-running grep instead of calling `recall_longterm` | The point of RepoLens is to remember what was already discovered. |
| Marking session summary as "TODO" or empty | The next agent starts cold, defeating the purpose. |
| Asserting test results without running tests this session | Stale claims poison memory. |
| Suppressing a `Contradicted` grounding status | The contradiction is a signal — surface it. |

## Quick checklist

```
[ ] 1. Got brief (or composed one from get_summary / get_conventions / recall_longterm)
[ ] 2. Read known-constraints + searched FailedAttempt memories
[ ] 3. Every repo claim cited
[ ] 4. Wrote a short plan
[ ] 5. Ran `validate-plan` (warnings only) — or, if not in a RepoLens workspace, walked plan against constraints by hand
[ ] 6. Ran the tests the brief suggested
[ ] 7. Wrote session summary
[ ] 8. Marked uncertain findings as hypotheses
[ ] 9. Did not upgrade guesses to facts
```

## See also

- [`README.md`](../README.md) — product overview.
- [`docs/memory-safety.md`](memory-safety.md) — memory schema and poisoning risks.
- [`docs/templates/repolens/`](templates/repolens/) — markdown templates this protocol references.
- [`docs/evals/agent-context-evals.md`](evals/agent-context-evals.md) — how protocol adherence is evaluated.
- [`docs/philosophy/07-current-state-vs-direction.md`](philosophy/07-current-state-vs-direction.md) — what is actually shipped.
