<!--
Template: session-memory.md
Lives at:  .repolens/session-memory.md (after `repolens init`)
Purpose:   Append-only log of agent session summaries. The handoff between sessions.
Owner:     Agents record handoffs on session end via `repolens remember --file`.
           Humans may edit retroactively to correct or annotate.
-->

# Session Memory

> Append new entries at the top. Older entries decay; do not delete unless explicitly retracting.

---

## Session YYYY-MM-DDTHH:MM:SSZ — _<one-line task>_

- **Agent:** _<agent name + version>_
- **Task (input):** _what the human asked_
- **Outcome:** _shipped / partial / blocked / abandoned_

### What I did

_3–6 bullets. Cite files and line ranges._

### What I tried and rolled back

_For each: what I tried, why it failed, what blocker would need to lift before retrying. Treat these as candidates for future `failed_attempt` memory._

### What surprised me

_Friction tags (e.g., `symbol-lookup`, `repo-identification`). Brief narrative each. These should be written to the friction log too._

### What I am unsure about

_Plain-language guesses. Mark each as a hypothesis. Do **not** elevate these to facts here._

### What the next agent should know

_A handoff paragraph. If you have to pick one bullet to keep, this is it._

### Files touched

- `path/to/file.rs:NN-MM` — _what changed_

### Tests run

- `cargo test --workspace` — pass/fail
- `cargo insta review` — _accepted N changes, listed_

### Constraints I checked against

- C-001: ✓
- C-007: ✓
- _(or "not applicable")_

### Open questions for the human

- _..._

---

_Older sessions follow below._
