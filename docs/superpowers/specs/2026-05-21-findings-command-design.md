# `repolens findings` — agent-readable calibrated findings

**Date:** 2026-05-21
**Status:** design approved (delegated: "go with your recommendations")

## Problem

Phase 4B made every MCP finding calibrated (four-part evidence/inference/
limitations/action + grade/confidence). The ACE-011 establishing run showed
the calibration is *invisible to agent behavior*: an agent on an open-ended
task reached for `repolens brief` + source grep and never invoked MCP, so it
never saw the calibrated findings. Two blockers: (1) consuming findings means
spinning up the MCP loop (cost), and (2) nothing on the agent's default path
(the brief) points at the calibrated surface (discoverability).

There is also no markdown rendering of evidence findings anywhere today:
`compare-summaries` renders only the Metrics table; findings exist only as
JSON via MCP `get_summary --detail full`.

## Goal

A thin CLI command, `repolens findings`, that prints the active pack's
calibrated `evidenceFindings` as agent-readable markdown — removing the
MCP-loop cost — plus a pointer to it from the brief's Suggested First Checks,
so it sits on the agent's default path. This is the cheapest intervention
that directly tests the MCP-consumption hypothesis; success is measured by a
follow-up ACE-011 run (does the agent consume calibrated findings?).

## Non-goals

- No new finding logic. Delegates to existing `SummaryService::summarize`.
- No memory mutation (uses `SummaryMemoryTier::Skip`, like `compare-summaries`).
- Not embedding findings *into* the brief body (that is backlog #2/#3; heavier
  coupling, muddier attribution). This ships the cheap, discoverable wrapper.

## Design

### Command

`repolens findings [--root <path>] [--pack <id>] [--format markdown|json]`

- `--root` — optional; resolved like other subcommands.
- `--pack <id>` — optional override; `pack-YYYYMMDD-HHMMSS` or bare
  `YYYYMMDD-HHMMSS`, resolved via `repolens_app::discover::resolve_pack_ref`.
- `--format` — reuse the existing `OutputFormat { Markdown, Json }`; default
  `Markdown`.

### Pack resolution (`resolve_findings_pack`)

1. If `--pack` given → `resolve_pack_ref(raw, root)` (validates + checks
   `manifest.json` exists).
2. Else prefer the authoritative active pack from `SessionStore::load(root)`
   (`activePackPath`) when that directory still has a `manifest.json`.
3. Else fall back to the latest pack on disk (`list_pack_dirs(root).last()`).
4. No usable pack → error: "No pack found … Run 'repolens scan' first."

Preferring the session's active pack (over latest-on-disk) is consistent with
the Runtime Freshness fix: session.json is the authoritative pointer.

### Data

`SummaryService::summarize(&pack_dir, Some(root), SummaryMemoryTier::Skip)`
→ `RepoSummary.evidence_findings: Vec<EvidenceFinding>`. Read-only.

### Output — markdown (default), "calibration-first compact"

Header:

```
# Evidence Findings — <pack-id>

_Calibrated observations for this pack. Each finding separates what was
observed (evidence) from what is inferred, with explicit limitations and a
calibration grade. Treat them as premises to verify, not verdicts._

<N> finding(s).
```

Per finding (rendered in the builder's existing deterministic order):

```
### <title>
`<classification>` · severity=<severity> · grade=<evidenceGrade> · confidence=<confidence> · layer=<knowledgeLayer>

**Inference:** <statement> _(<strength>)_
**Depends on:** a; b            (omitted if empty)
**Does not imply:** x; y        (omitted if empty)

**Evidence:**
- <metric> = <value>                              (Observation)
- rule `<rule>` (observed <v>, threshold <t>)     (HeuristicJudgment)
- external `<path>` (<signalKind>)                (ExternalSignal)

**Limitations:** CODE1, CODE2   (codes only; omitted if empty)
**Not checked:** CODE1, CODE2   (codes only; omitted if empty)
**Action:** <rendered action>
```

Action rendering: `NoAction` → "No action."; `ReviewIfMaintainerPain` →
"Review if this causes maintainer pain."; `ConcreteAction{steps}` → "Steps: "
+ steps joined "; "; `ExternalVerification{commands_or_checks}` → "Verify
externally: " + joined "; ".

Empty findings → header + "No evidence findings for this pack."

### Output — json

`serde_json::to_string_pretty(&summary.evidence_findings)` — parity with what
an agent sees via MCP `get_summary --detail full` (the `evidenceFindings`
array).

### Brief discoverability

Add one bullet to the brief's Suggested First Checks
(`crates/repolens-cli/src/brief.rs`):

> Run `repolens findings` to see calibrated evidence findings (entry points,
> dependency cycles, orphan modules, test/source ratio, …) for the active pack
> before assuming anything about repo structure or health.

## Files

- `crates/repolens-cli/src/findings.rs` (new) — args, `run_findings`,
  `resolve_findings_pack`, markdown renderer + helpers, unit tests.
- `crates/repolens-cli/src/main.rs` — `mod findings;`, wire
  `Findings(findings::FindingsArgs)` subcommand; make `OutputFormat` reachable.
- `crates/repolens-cli/src/brief.rs` — Suggested First Checks pointer bullet +
  a test asserting the brief mentions `repolens findings`.
- `crates/repolens-cli/tests/findings_cli_test.rs` (new) — integration: scan a
  small fixture, run `repolens findings`, assert markdown header + a known
  finding renders; `--format json` parses as an array.
- `README.md` — one line under the brief/commands area.

## Testing

- Renderer unit tests: tag strip present; inference/action/limitations
  rendered; empty-findings message; deterministic (two renders byte-equal);
  each `RecommendedAction` / `EvidenceItem` variant renders.
- Pack-resolution unit tests: `--pack` override, active-pack preference,
  latest fallback, no-packs error.
- Integration: end-to-end against a `fixtures/inputs` repo.

## Branch / sequencing

Branch `feat/findings-command` is stacked on `fix/brief-runtime-freshness`
(PR #64) because both edit the brief's Suggested First Checks block. PR base =
`fix/brief-runtime-freshness`; retarget to `main` once #64 merges.
