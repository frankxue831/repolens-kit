# Tranche-1 surfaces — blind instrumentation run — `v1` (impact-trace task)

**Case:** not an ACE-numbered case — this is the "LINE-ANCHOR (P0)" / impact-trace
instrumentation measurement from the corrected audit's eval plan
(`docs/reviews/2026-06-10-agentic-code-map-audit.md` §7B), run immediately after
tranche 1 merged. Scope precedent for non-ACE entries: `round-6-validate-plan-shipped.md`.

**Repo/binary provenance:** HEAD `83c064e` (all of tranche 1 merged: PRs #105-#107
— get_callers bounding + edge disclosure, search_code error routing, Symbol line
anchors). Binary built from that HEAD and copied into the snapshot. Pack
`pack-20260610-162605` (430 files), `session.json` active.

**Method:** blind sub-agent (Claude general-purpose) dispatched into an isolated
snapshot created by `scripts/dev/eval-blind-workspace.sh` (HEAD archive,
`docs/evals/` stripped — the v4 contamination fix, applied). The task prompt named
the surfaces as available ("prebuilt binary, mcp-call.sh, fresh pack") but gave no
rubric, no tool steering, and no mention of measurement — same free-choice posture
as `ACE-011-postphase4b-v3-codex`. Trace: agent-produced approach log (33 actions,
order preserved), cross-checked against the harness tool-use count (33 — exact
match) and against side effects (`/tmp/mcp_oneshot.py` exists; no repo mutation).

**Task:** impact assessment for changing `read_graph`
(`crates/repolens-app/src/summary.rs:320`) from `Option<FileGraph>` to
`Result<FileGraph, GraphReadError>`: affected files, exact call sites (file:line),
affected tests, recommended order. Ground truth was established by the scorer
BEFORE dispatch (grep): 4 files, 8 call sites + definition, one same-prefix
distractor (`read_graph_unlocked` in repolens-memory), one None-path test.

## Scorecard summary

| Metric | Value | Note |
|---|---|---|
| `mcp_invoked` | **yes** | `get_callers` + `get_impact` — voluntarily, mid-run |
| Surface order | grep-first | grep was action #1 (and answered the caller set); MCP at #21-22 as cross-validation |
| `pack_primary` / `repo_fallback` | repo-primary, pack-corroboration | self-described: "Primary evidence for line-level claims is direct file reads; the pack graph served as cross-validation" |
| `wrong_file_attempts` | 0 | distractor `read_graph_unlocked` correctly excluded |
| Correctness vs ground truth | exact match, then beyond | all files/sites/lines; plus PackWriter-no-graph.json discovery, no-thiserror dep check, `NoUsablePackError` precedent, doc-canon frozen-doc discipline |
| Tranche-1 `limit` param | **consumed** | agent passed `"limit":200` to get_callers unprompted — it read the new schema |
| Tranche-1 edge disclosure | **observably effective** | agent's claim language: "the only `imports read_graph` edges point at the three mcp tool files" — calibrated as imports edges, NOT over-claimed as callers. The exact miscalibration PR #105's disclosure was built to prevent did not occur |
| Tranche-1 line anchors | **unused** | no `search_code` symbol mode, no outline; lines came from direct reads the agent needed anyway for handling semantics |
| Post-MCP grep-for-lines | n/a | reads preceded MCP; the audit's root-cause-#1 flow (MCP → grep for line) did not occur, but not because anchors replaced it |

**Verdict: instrumentation run — no pass/fail.** Signals, not proof.

## Findings

1. **NEW FRICTION BUG (headline): `scripts/dev/mcp-call.sh` is unusable outside a
   git worktree.** It hard-requires `git rev-parse --show-toplevel`; blind
   snapshots are `git archive` extracts with no `.git`, so the shipped one-shot
   MCP path failed (exit 1, twice). The agent recovered by hand-writing a
   JSON-RPC STDIO driver against the prebuilt binary — after which
   `get_callers`/`get_impact` "worked first try". Adoption intent was present;
   tooling, not tool design, was the barrier. Fix tracked separately
   (fall back to script-relative root resolution).
2. **Two tranche-1 surfaces produced observable value on first contact:** the
   `limit` param was consumed naturally, and the edge-type disclosure shaped
   correctly-calibrated claim language ("imports edges", not "callers").
3. **Line anchors did not change behavior on this task type.** For impact
   analysis, the agent reads the call-site files anyway (it needs the
   surrounding handling semantics), so declaration-line anchors add little. The
   anchor value proposition concentrates on symbol→open-file navigation flows —
   supports prioritizing the P1 result-derived `nextCalls` recipes (consumption
   entry) over further anchor surfacing.
4. **grep-first remains rational and is not, by itself, a product failure.** One
   grep answered the file-set question; the agent then used MCP for what it is
   actually better at (structured corroboration + blast-radius at depth). This
   refines the audit's adoption framing: the realistic target is
   "MCP-as-corroboration → MCP-as-primary" for the question types where the
   tools are strictly cheaper, not the elimination of grep.

## Methodological caveats

- Single run, single vendor, single task type. The ACE-011 binding question
  (evidence-finding consumption) is a DIFFERENT measurement — this run does not
  update it.
- The scorer (parent session) designed the tranche-1 surfaces; the dispatched
  agent was blind (fresh context, no rubric, stripped corpus) but the task
  selection was scorer-chosen and could favor the new surfaces. Mitigation:
  ground truth fixed before dispatch; metrics reported regardless of outcome
  (line anchors scored "unused" — an unfavorable result for the scorer's own
  work, reported as such).
- Approach log is agent-self-reported; corroborated by exact harness tool-count
  match (33/33) and side effects, but not a command-stream ground truth of the
  `codex exec --json` grade.
- The mcp-call.sh failure means MCP-invocation friction was HIGHER in this run
  than in a real checkout — the voluntary-use signal is therefore conservative
  (a lower bound), and the grep-first ordering may partly reflect that friction.

> Case-level controlled dogfood evidence, **not** product-level proof.
