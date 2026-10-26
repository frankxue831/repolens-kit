<!--
Template: eval-runs/case.md
Lives at:  .repolens/eval-runs/<CASE_ID>-<RUN_ID>/case.md  (after `repolens eval start`)
Purpose:   Self-contained briefing for the human/agent running this ACE case.
Owner:     `repolens eval start` writes this; do not hand-edit unless adding
           run-specific addenda (use a new heading at the bottom).
-->

# Eval Case: {{CASE_ID}} — {{CASE_TITLE}}

**Run ID:** `{{RUN_ID}}`

> This is a **manual or semi-manual** eval scaffold.
> RepoLens does not launch agents, score automatically, or call any LLM/MCP for this run.
> A human (or the agent under test, narrating its own actions) fills `scorecard.md`,
> `agent-transcript.md`, and `result.md` after the run.

## Case (extracted from `docs/evals/agent-context-evals.md`)

{{CASE_BODY}}

## Instructions

1. Read this entire file before starting the run.
2. Set up the **Setup** state described above. If the case requires a stale checkout
   or a seeded memory, do that *before* the agent is invoked.
3. Run the agent against the **Task**. Capture the full transcript in
   `agent-transcript.md` as you go (or paste it in afterwards).
4. After the run, evaluate against the **Pass criteria** and fill in
   `scorecard.md`. Do not fabricate metrics — leave a cell blank if you did
   not measure it.
5. Write a one-paragraph summary plus pass/fail verdict in `result.md`.

## Repo orientation reminder

When the case touches shipped CLI behavior, record at the top of
`agent-transcript.md` (and require the agent to record at the start of its
run):

- `pwd` — repo path the agent is operating in
- `git branch --show-current` — current branch
- `git log -1 --oneline` — HEAD commit
- `git status --short` — uncommitted / untracked files
- `cargo run -- --help` (or `repolens --help`) — enumerate subcommands

This is the same checklist surfaced by `repolens brief`'s "Repo freshness"
section. ACE-010 specifically tests whether the agent ran it before claiming
a command is missing.
