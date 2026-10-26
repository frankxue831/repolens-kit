<!--
Template: current-agent-brief.md
Lives at:  .repolens/current-agent-brief.md (after `repolens init`)
Purpose:   The preflight document an agent reads BEFORE it starts a task.
           Overwritten by each `repolens brief --task "<x>"` (planned v0.1).
Today:     Compose by hand from MCP `get_summary` + `get_conventions` + `recall_longterm`.
Owner:     Generated; do not edit by hand (your edits will be overwritten next brief).
-->

# Current Agent Brief

**Task:** _<one-sentence task description from the human>_
**Generated:** YYYY-MM-DDTHH:MM:SSZ
**Pack:** `.repolens/pack-YYYYMMDD-HHMMSS/`
**Memory entries used:** _<n>_

---

## 1. Task summary

_Restated from the human's input. Add what is in scope and what is out of scope._

## 2. Relevant files

| Path | Why it matters | Confidence |
|---|---|---|
| `crates/.../main.rs` | _CLI entry; new flag goes here_ | ScanExtracted |
| `crates/.../scan.rs` | _scan service; argument flows here_ | ScanExtracted |
| `crates/.../tests/scan_test.rs` | _existing tests for this surface_ | TestValidated |

## 3. Conventions to respect

_From `CLAUDE.md`, `AGENTS.md`, and `.claude/rules/*.md`. Filter to those that plausibly bind this task._

- _e.g., All JSON output uses camelCase (Java contract parity)_
- _e.g., Atomic file I/O via temp + rename_
- _e.g., Snapshot tests via insta_

## 4. Known constraints (from `.repolens/known-constraints.md`)

- **C-NNN:** _short statement_
- **C-NNN:** _short statement_

## 5. Prior failed attempts

_Memories of type `failed_attempt` matching the task keywords. If empty: "None recorded."_

- _F-YYYY-MM-DD: tried X, rejected because Y. Invalidates when Z._

## 6. Relevant tests

_Memories of type `test_guidance` and tests by file proximity._

- `crates/.../tests/<test>.rs` — what it asserts
- Snapshot tests under `crates/.../snapshots/`

## 7. Suggested first checks

_Concrete commands the agent should run before editing._

```sh
cargo test -p <crate> -- <test-keyword>
cargo run -- scan --root .
```

## 8. Risks

- _Refactoring this touches the camelCase contract — be careful._
- _Memory schema change here would invalidate existing packs._

## 9. Acceptance criteria

- _Behavior X works end-to-end._
- _Test Y passes._
- _No `warn`-severity findings reported by `repolens validate-plan` (warnings only — clean output does not prove the plan is valid)._
- _Session summary written._

## 10. Things NOT to change

- _Module Z is frozen until plan A lands._
- _Public function signatures of `<symbol>` (Java contract parity)._

## 11. Open questions for the human

- _..._

---

_To regenerate: `repolens brief --task "<task>"` (planned v0.1). Today: re-run MCP `get_summary` + `get_conventions` + `recall_longterm` and update sections 1–11 by hand._
