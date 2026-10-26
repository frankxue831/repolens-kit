<!--
Template: known-constraints.md
Lives at:  .repolens/known-constraints.md (after `repolens init`)
Purpose:   The list of rules a plan must respect. Read by `repolens validate-plan` (warnings only — does not prove plans valid).
Owner:     Humans. Agents may PROPOSE additions in their session summary, but only humans
           promote a constraint to this file with confidence: HumanVerified.
-->

# Known Constraints

> Rules that bind any change in this repo. Each constraint should be a single, testable assertion. If you cannot decide whether a plan violates it without ambiguity, the constraint is too vague — split it.

Each entry follows the schema in [`docs/memory-safety.md`](../../memory-safety.md). Fields: `id`, `type: constraint`, `confidence`, `evidence`, `verifiedBy`, `createdAt`, `invalidatesWhen`, `status`.

---

## Active constraints

### C-001 _<short-name>_

- **Statement:** _One sentence. Imperative. E.g., "All JSON output uses camelCase."_
- **Why:** _One sentence. E.g., "Java RepoLens contract parity."_
- **Evidence:** `CLAUDE.md` (section X) / `AGENTS.md` / commit `<sha>` / test `<path>`
- **Verified by:** `human`
- **Created:** YYYY-MM-DD
- **Invalidates when:** _condition under which this constraint should be retired_
- **Status:** active

### C-002 _<short-name>_

- **Statement:** _..._
- **Why:** _..._
- **Evidence:** _..._
- **Verified by:** human
- **Created:** YYYY-MM-DD
- **Invalidates when:** _..._
- **Status:** active

---

## Retired constraints

### C-000 _<short-name>_ (retired YYYY-MM-DD)

- **Was:** _previous statement_
- **Retired because:** _reason_
- **Replaced by:** _C-NNN, if applicable_

---

## Process

- Adding a constraint requires citing a source: docs path, commit, or explicit human ratification in a session summary.
- Removing a constraint requires writing a `Supersedes` edge or noting it under "Retired" with a reason — never silent deletion.
- Agents proposing constraints should write them as `hypothesis` memories first; humans promote to `constraint` here.

## See also

- [`docs/memory-safety.md`](../../memory-safety.md) — schema and lifecycle.
- [`docs/agent-protocol.md`](../../agent-protocol.md) §5 — how plans are validated against this file.
