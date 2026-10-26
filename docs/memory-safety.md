# Memory Safety

> **Why this doc exists.** RepoLens stores agent-written memories that future agents will read. If a wrong guess gets written as a "fact" and then read, repeated, and reinforced over months, the memory layer becomes worse than useless — it becomes a confident liar. This doc is the contract that prevents that.
>
> **Status.** The schema below is the v0.1 target. The current Rust implementation supports a subset (5 node types, decay, grounding). Fields and types marked **planned** are part of the v0.1 milestone. See [`docs/philosophy/07-current-state-vs-direction.md`](philosophy/07-current-state-vs-direction.md) for what's shipped today.

---

## The poisoning risk

Memory poisoning happens when:

1. An agent makes a plausible but wrong assumption.
2. The agent writes that assumption to memory with high importance.
3. Future agents recall it, treat it as fact, and act on it.
4. Their actions reinforce the wrong assumption (more access count, no contradiction).
5. The wrong assumption persists indefinitely, propagating across sessions.

Concrete examples seen in agent-coding contexts:

- "This codebase uses `pytest`" — written by an agent that saw a stray dependency. It actually uses `unittest`. Agents now keep generating `pytest` fixtures.
- "The `User` model lives in `src/models/user.py`" — written before a refactor. The file no longer exists. Agents now blindly cite a missing path.
- "We tried adding caching to this endpoint and it broke" — written without context. The next agent avoids caching forever, even when the original blocker is gone.

The countermeasures below are designed to make each of these harder.

---

## Memory item schema

Every memory item must carry these fields. Fields marked **planned** are not yet enforced in the Rust implementation; agents writing memories today should fill them in the `tags` or `content` until the schema lands.

| Field | Type | Required | Notes |
|---|---|---|---|
| `id` | string (UUID) | Yes | Stable identifier. |
| `type` | enum (see below) | Yes | One of the 7 types in the next section. |
| `content` | string | Yes | Human-readable claim, plan-relevant. |
| `confidence` | enum | Yes (planned) | `TestValidated` / `HumanVerified` / `ScanExtracted` / `AgentAsserted`. Defaults to `AgentAsserted`. |
| `evidence` | string | Yes (planned) | File:line citation, test name, doc path, or human quote. |
| `verifiedBy` | enum | Yes (planned) | `human` / `test` / `docs` / `source` / `agent_only`. |
| `createdAt` | RFC3339 timestamp | Yes | Set on write. |
| `invalidatesWhen` | string \| null | Optional | Free-text rule: e.g., "anchor file deleted", "test X removed". |
| `status` | enum | Yes (planned) | `active` / `superseded` / `contradicted` / `expired` / `pending_review`. |
| `anchors` | list of file/symbol refs | See rules | Required for `confidence ≥ ScanExtracted` (planned). |
| `importance` | float [0, 1] | Yes | Decay weight. Distinct from confidence — see below. |
| `provenance` | string | Yes | Origin: `mcp:memorize_insight`, `mcp:scan-triggered`, `human:manual-edit`, etc. |
| `tags` | list of strings | Optional | Searchable labels. |

**Importance vs. confidence — keep them separate.**
- `importance` = "how often this memory should be surfaced." Decays with time.
- `confidence` = "how much this memory should be trusted." Does not decay; can only be raised by adding evidence or lowered by contradiction.

A high-importance, low-confidence memory is fine. A high-importance, high-confidence memory with no evidence is the poisoning case and must be rejected at the API.

---

## Memory types — v0.1 target (7 types)

> **Shipped today:** the actual `MemoryType` enum at `crates/repolens-core/src/model/memory.rs:9` has **5 variants**: `CodeAnchor`, `Decision`, `Convention`, `TradeOff`, `Pattern`. The 7 types below are the v0.1 target schema (only `decision` overlaps with the shipped names). Until the full schema lands, agents should fold the v0.1-target intent into the shipped variants and use the `tags` field to record the target type.

Each type has a different burden of proof and a different lifecycle.

### `verified_fact`

A claim that has been confirmed by a test, source-code inspection, or a human.

- **confidence:** `TestValidated`, `HumanVerified`, or `ScanExtracted`.
- **anchors:** required (non-empty).
- **evidence:** required.
- **Decay:** slow; can be pinned by access.
- **Example:** `"crates/repolens-cli/src/main.rs defines binary name 'repolens' (line 13)"`, evidence `crates/repolens-cli/src/main.rs:13`, verifiedBy `source`.

### `hypothesis`

An agent's best guess that has not been verified.

- **confidence:** `AgentAsserted`.
- **anchors:** optional but encouraged.
- **evidence:** optional; if absent, must be marked.
- **Decay:** fast; auto-expires unless promoted to `verified_fact` with evidence.
- **Status transitions:** `active → verified_fact` (with evidence), or `active → contradicted`.
- **Example:** `"Compare-summaries probably reuses the diff logic from compare.rs"`, evidence none, verifiedBy `agent_only`.

**This type is the workhorse.** When in doubt, write a `hypothesis`, not a `verified_fact`.

### `constraint`

A rule the codebase imposes on changes.

- **confidence:** `HumanVerified` is the default (constraints encode human intent).
- **anchors:** required.
- **evidence:** must cite the source of the rule (CLAUDE.md, AGENTS.md, ADR, etc.).
- **Decay:** very slow; constraints are sticky.
- **Used by:** `repolens validate-plan` (shipped, Round 6) — flags warnings against `.repolens/known-constraints.md`.
- **Note:** the linter consumes markdown today; the typed `constraint` memory node is still planned for v0.1.
- **Example:** `"All JSON output uses camelCase (Java contract parity)"`, evidence `CLAUDE.md` "Key Patterns" section, verifiedBy `human`.

### `decision`

An architectural choice and its rationale.

- **confidence:** `HumanVerified` typical.
- **anchors:** the affected modules/files.
- **evidence:** typically a docs path or commit hash.
- **Decay:** slow; supersedable by newer decisions.
- **Example:** `"Pack and Memory split: packs are immutable, memory mutates"`, evidence `docs/philosophy/01-background-and-mission.md:49-55`, verifiedBy `docs`.

### `failed_attempt`

A specific approach that was tried and did not work.

- **confidence:** `HumanVerified` or `TestValidated` (you observed the failure).
- **anchors:** the files/tests involved.
- **evidence:** test output, commit hash of the rollback, or human note.
- **Decay:** slow; failures stay relevant.
- **invalidatesWhen:** "the underlying blocker is fixed" — write the resolving condition explicitly so future agents know when to retry.
- **Example:** `"Tried stuffing scan flags into env vars (PR #12) — rejected, broke parallel runs"`, evidence commit `abc123`, verifiedBy `human`.

### `test_guidance`

Information about which tests cover which behavior.

- **confidence:** `TestValidated` typical.
- **anchors:** the test file and the production code.
- **evidence:** test name and what it asserts.
- **Decay:** medium; invalidated when the test is removed or its scope changes.
- **Example:** `"first_agent_eval_test.rs locks FAC-005 (search_code symbol bare-name)"`, evidence `crates/repolens-mcp/tests/first_agent_eval_test.rs`, verifiedBy `test`.

### `friction_finding`

An observed point where the agent (or human) got stuck or confused.

- **confidence:** `AgentAsserted` or `HumanVerified` (the experience itself is grounded).
- **anchors:** optional.
- **evidence:** brief narrative + frictionTag (see [`docs/superpowers/evals/first-agent-cases.json`](superpowers/evals/first-agent-cases.json)).
- **Decay:** medium; promoted to a roadmap item after recurring across runs.
- **Example:** `"Agent had to grep for ProfileAnalyzer::analyze; symbol search dropped the qualifier"`, evidence `docs/philosophy/notes/2026-04-22-first-agent-eval-run.md`, verifiedBy `agent_only`, frictionTag `symbol-lookup`.

---

## Status lifecycle

```
active ──────────► superseded   (newer memory replaces; via Supersedes edge, faster decay)
   │
   ├─────────────► contradicted (Contradicts edge or Contradicted grounding: faster decay)
   │
   ├─────────────► expired      (importance falls below threshold; pruned)
   │
   └─────────────► pending_review (human has flagged for re-verification)
```

Notes:

- Today, `status` is implicit: a node either exists (active) or has been pruned (its importance fell below the pruning threshold). The richer status is part of the v0.1 schema.
- A `superseded` memory is preserved for history — never silently deleted.
- A `contradicted` memory does not auto-disappear; it sticks around so future agents can see "we used to believe this" and learn.

---

## Memory poisoning countermeasures

| Risk | Countermeasure | Status |
|---|---|---|
| Wrong guess written as fact | `confidence` defaults to `AgentAsserted`; promotion requires `evidence` | Planned (v0.1) |
| High-importance, ungrounded memory | Reject `confidence ≥ ScanExtracted` writes that have empty `anchors` | Planned (v0.1) |
| Stale memory survives via access | A memory whose anchored file was deleted or renamed is marked `Contradicted` on recall after the next scan. Edits to the file are not detected yet; the memory stays `Confirmed`. Agent must respond | Partly shipped (status reported, not enforced; edits to an anchored file are **not** detected yet, only deletion/rename after a rescan) |
| Agent silently overwrites good memory | `Supersedes` edge required; superseded memories decay faster; preserved in history | Shipped (edge exists; not enforced on write path) |
| Confidence drift (guess → fact over time) | `confidence` is a separate field from `importance`; only `evidence` raises it | Planned (v0.1) |
| Forget-to-hide-contradiction | `forget_insight` should be logged in session summary; tracked in friction log | Convention; not enforced |
| Repeated re-assertion of contradicted memory | A `Contradicts` edge persists; new memory inherits warning | Shipped (edge exists, agent discipline required) |

Each countermeasure that is "planned" is part of the v0.1 milestone. Each one that is "shipped" still depends on agent discipline — the protocol in [`docs/agent-protocol.md`](agent-protocol.md) is what makes them effective.

---

## What humans must still do

This is not an autonomous safety system. Humans must:

- Periodically review the memory graph (suggested cadence: weekly for active projects).
- Confirm or reject `pending_review` items.
- Ratify constraints — only humans can mark a memory `HumanVerified`.
- Watch for `friction_finding` recurrence; promote recurring friction to roadmap items.
- Audit `forget_insight` calls in session summaries.

If you are reading this as a human and you have not done a memory review in 30 days, do one now.

## See also

- [`docs/agent-protocol.md`](agent-protocol.md) — how agents are expected to use this schema.
- [`docs/templates/repolens/session-memory.md`](templates/repolens/session-memory.md) — template that produces well-shaped memories.
- [`crates/repolens-core/src/model/memory.rs`](../crates/repolens-core/src/model/memory.rs) — current Rust schema (subset of the above).
- [`crates/repolens-memory/src/decay.rs`](../crates/repolens-memory/src/decay.rs) — current decay implementation.
- [`crates/repolens-app/src/grounding.rs`](../crates/repolens-app/src/grounding.rs) — current grounding evaluator.
