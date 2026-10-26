<!--
Template: repo-overview.md
Lives at:  .repolens/repo-overview.md (after `repolens init`)
Purpose:   Human + agent-readable summary of what this repo IS.
Generated: Today, by composing MCP `get_summary` (tier `standard`) + `get_conventions`.
           Future: by `repolens brief --task ...` and/or a dedicated `repolens overview`.
Owner:     Generated. Edit only the "Notes from humans" section.
-->

# Repo Overview

**Repo name:** _<repo-name>_
**Primary languages:** _<lang-1>, <lang-2>_
**Last scan:** _<pack-id>_ (`.repolens/pack-YYYYMMDD-HHMMSS/`)
**Schema version:** _<n>_

## One-paragraph description

_What does this repo do, in plain language? 3-5 sentences. Avoid jargon. A new agent should be able to read this and know whether the next task fits this repo at all._

## Top-level layout

```
crates/         # if Rust
src/            # if other
tests/
docs/
scripts/
fixtures/       # test inputs
```

_List the actual directories. Mark which ones are entry points and which are infrastructure._

## Build & test

```
<build command>
<test command>
<lint command>
<format command>
```

## Key entry points

| Component | Path | What it does |
|---|---|---|
| CLI | `crates/.../main.rs` | _one line_ |
| Server | `crates/.../server.rs` | _one line_ |
| Tests | `crates/.../tests/` | _one line_ |

## Conventions (extracted)

_From `CLAUDE.md` / `AGENTS.md` / `.claude/rules/*.md`. Compose with MCP `get_conventions` and trim._

- _convention 1_
- _convention 2_

## Notes from humans

_Free-form. Things humans want every agent to know that don't fit elsewhere. Edit this section by hand._

---

_Auto-regenerate this file with `repolens overview` (planned v0.1) or by re-running `get_summary` + `get_conventions` and pasting the output above._
