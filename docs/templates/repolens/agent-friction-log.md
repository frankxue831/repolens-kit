<!--
Template: agent-friction-log.md
Lives at:  .repolens/agent-friction-log.md (after `repolens init`)
Purpose:   Record of points where agents got stuck, confused, or had to fall back to grep.
           Recurring friction (same tag in 3+ sessions) graduates to a roadmap item.
Owner:     Agents append; humans review weekly and promote.
-->

# Agent Friction Log

> One entry per friction event. Use a stable `tag` so recurrence can be counted.

The current tag taxonomy (extend as needed; humans ratify new tags):

| Tag | Meaning |
|---|---|
| `repo-identification` | Agent could not figure out what the repo does without grepping. |
| `hotspot-discovery` | Agent could not find where to start changing for a given area. |
| `memory-conventions` | Agent missed a convention captured in CLAUDE.md / `.claude/rules/`. |
| `risk-inspection` | Agent did not see a known risk or prior failed attempt. |
| `symbol-lookup` | `search_code` did not return the symbol the agent searched for. |
| `test-discovery` | Agent could not identify which tests cover the change. |
| `plan-validation` | Agent shipped a plan that violated a known constraint. |
| `handoff-quality` | Next session could not pick up from the previous summary. |

---

## Active friction events

### F-YYYY-MM-DD-NNN — `<tag>`

- **Session:** _link or timestamp_
- **What happened:** _2–3 sentences. The actual user-visible failure._
- **Workaround:** _what unblocked the agent (grep / asking human / different MCP tool)_
- **Hypothesis on cause:** _agent's guess. Mark as hypothesis — not fact._
- **Suggested fix:** _proposal. Not binding. Humans evaluate._
- **Tag:** `<tag>`
- **Status:** open / under-review / promoted-to-roadmap / resolved

---

## Recurrence ledger (auto-rolled-up, planned)

| Tag | Count | First seen | Last seen | Status |
|---|---|---|---|---|
| `symbol-lookup` | 3 | YYYY-MM-DD | YYYY-MM-DD | promoted-to-roadmap |
| `repo-identification` | 1 | YYYY-MM-DD | YYYY-MM-DD | open |

> Promotion rule: a friction tag that recurs in 3 or more sessions is promoted to a roadmap item under `docs/superpowers/plans/`.

---

## Process

- Agents log every friction they hit, even small ones.
- The log is not a complaint board — only events with a clear failure signature belong here.
- Humans review weekly. Resolved entries move to a "resolved" section (or are kept inline with status updated).

## See also

- [`docs/evals/agent-context-evals.md`](../../evals/agent-context-evals.md) — friction is one of the metrics.
- [`docs/agent-protocol.md`](../../agent-protocol.md) §7 — when in the session friction is logged.
