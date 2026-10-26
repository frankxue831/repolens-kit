<!--
Template: eval-runs/agent-transcript.md
Lives at:  .repolens/eval-runs/<CASE_ID>-<RUN_ID>/agent-transcript.md  (after `repolens eval start`)
Purpose:   Append-only record of agent actions, tool calls, and human messages
           during the run. Used to back the metrics in scorecard.md.
Owner:     The reviewer (or the agent under test) appends here as the run
           proceeds. Do not edit prior entries — add new ones below.
-->

# Agent Transcript

**Case:** {{CASE_ID}} — {{CASE_TITLE}}
**Run ID:** `{{RUN_ID}}`

> **Honesty rule.** Record only what actually happened. Do not paraphrase
> tool calls into pseudo-actions. If the agent fell back to `grep` after the
> brief failed to surface a file, the fallback IS the data point — do not
> rewrite the transcript to make RepoLens look better.

## Repo state at start of run

_Fill these in before the agent runs. Required for any case that touches
shipped CLI behavior (ACE-010 in particular)._

- `pwd`:
- `git branch --show-current`:
- `git log -1 --oneline`:
- `git status --short`:
- `repolens --help` subcommands present:

## Setup performed

_Describe the Setup state from `case.md` that you established. If the case
requires a stale checkout, a seeded memory, or a specific `.repolens/`
state, record exactly what you did._

-

## Task given to agent

_Paste the prompt verbatim._

```

```

## Transcript

_Append entries chronologically. Each entry: timestamp (optional), actor
(human / agent / tool), content. Code blocks for tool calls and outputs._

### Entry 1

**Actor:**
**Content:**

```

```

### Entry 2

**Actor:**
**Content:**

```

```

## Observations

_Free-form notes that did not fit a metric cell — surprises, friction
patterns worth promoting to `agent-friction-log.md`, or hypotheses for
future cases._

-
