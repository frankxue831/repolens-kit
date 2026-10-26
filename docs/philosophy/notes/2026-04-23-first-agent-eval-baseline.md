# First-Agent Eval Baseline Re-Run on `main` — 2026-04-23

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-23. Preserved as the authoritative record of its topic as of that date.

**Date:** 2026-04-23
**PR context:** None — baseline re-run on `main` at the same commit as the 2026-04-22 run.
**Pack:** `.repolens/pack-20260422-174605` (incremental scan)
**Runner:** `bash scripts/dogfood-eval.sh`
**Scope:** Second run of the eval contract. **Not** a qualifying-PR data point. Purpose: confirm reproducibility, establish a baseline for comparison when the next real PR lands, and check whether any friction has shifted since 2026-04-22.

---

## Workflow-Closure Dogfood Snapshot

Commit: `3eac1dd` | Pack: `pack-20260422-174605` | Scan mode: `incremental`

| Field | Value |
| --- | --- |
| Files walked | 385 |
| Files included | 314 |
| Excluded rows | 71 |
| Pack size | 4.2M |
| Scan duration | 0.87s |
| Schema version | 1.3.0 |
| Worktree dirty | false |

### Notable Profile Entry Points

- `crates/repolens-cli/src/main.rs` (`main`) — Rust binary ([[bin]] target)
- `scripts/sanitize-fixtures.py` (`main`) — Python entry point

### First-Agent Eval Contract

#### FAC-001 — What is this repo, in practical terms?
- Workflow step: Get a compact repo summary
- Surface: snapshot-pack-files
- Probe: Review the workflow-closure dogfood snapshot output and project-profile.json
- Expected status: pass
- Observed status: manual
- Friction tag: repo-identification
- Notes: Prefer normal RepoLens output; call out any answer that still depends on manual pack inspection.
- Answer: Identical shape to 2026-04-22. `project-profile.json` reports `name=repolens-rs`, `projectType=CLI`, `buildSystem=Cargo`, `frameworks=[]`, `dependencyRisk=low`. **`description` is still `null`.** Purpose ("what this tool does") is still not recoverable from the structured pack alone; the same manual step against `CLAUDE.md`/`README.md` is required. No drift in the identifying fields between runs.
- Evidence: `pack-20260422-174605/project-profile.json` — `name`, `projectType`, `buildSystem.name`, `frameworks`, `description` (null), `health.dependencyRisk.riskLevel`.

#### FAC-002 — Where should I start if I need to change a specific area?
- Workflow step: Inspect one focused hotspot
- Surface: search_code(path+symbol)+read_file_content
- Probe: Use RepoLens search plus one focused file read to identify the first edit point
- Expected status: pass
- Observed status: manual
- Friction tag: hotspot-discovery
- Notes: Capture whether the answer comes from normal tool flow or from manual pack spelunking.
- Answer: Not re-probed end-to-end on this baseline run (the underlying code is unchanged from 2026-04-22, so the hotspot for the same "specific area" resolves to the same location: `crates/repolens-profile/src/profile.rs:42-48`). The **fallback-to-grep** friction the prior run documented is expected to persist because no code in that path has changed. Leaving this case as a carried-forward observation rather than a fresh probe so the evidence stays honest.
- Evidence: carried-forward from 2026-04-22 note; no fresh probe executed this run.

#### FAC-003 — What prior memory or convention matters before making a change?
- Workflow step: Recall relevant memory or conventions
- Surface: get_summary(memory)+conventions
- Probe: Check whether one normal RepoLens pass surfaces the memory or convention that should constrain the change
- Expected status: pass
- Observed status: manual
- Friction tag: memory-conventions
- Notes: Preserve whether the answer was directly available or required manual reconstruction.
- Answer: **Still no data on the RepoLens surface.** `.repolens/memory/` and `.repolens/wiki/` are absent again on this scan; `.repolens/session.json` is the only non-pack artifact. Same fallback to `CLAUDE.md`/`AGENTS.md` as before. Friction is durable on `main`, as expected — nothing has primed those stores since 2026-04-22.
- Evidence: `ls .repolens/` shows `session.json` + pack dirs only; no `memory/` or `wiki/` subdirectory.

#### FAC-004 — What are the main structural risks right now?
- Workflow step: Inspect structural risk
- Surface: project-profile-health
- Probe: Use profile or summary output to identify current risks without ad hoc JSON digging
- Expected status: pass
- Observed status: manual
- Friction tag: risk-inspection
- Notes: This case stays manual because the product judgment is qualitative even when the data source is structured.
- Answer: Two `low`-severity signals (`dependency-cycles`, `orphan-files`). `cycleCount=2` (unchanged from 2026-04-22). `orphanCount=222` (up from 221 on 2026-04-22 — +1 drift). `testToSourceRatio=0.150` (unchanged). `dependencyRisk=low`. Data was directly available from the health block; no manual digging. The +1 orphan drift tracks the +1 included-file drift reported in the snapshot block (walked: 385 vs 384, included: 314 vs 313); no git commits between runs, so the delta comes from scan nondeterminism or a filesystem-level change the walker picked up — flagged here as a curiosity, not a signal.
- Evidence: `project-profile.json.health.signals[0..1]`, `graphHealth.cycleCount=2`, `graphHealth.orphanCount=222`, `graphHealth.testToSourceRatio≈0.15`, `dependencyRisk.riskLevel=low`.

#### FAC-005 — Can symbol search find `ProfileAnalyzer::analyze` directly?
- Workflow step: Inspect one focused hotspot
- Surface: search_code(symbol)
- Probe: Query `ProfileAnalyzer::analyze` in symbol mode
- Expected status: known_fail
- Observed status: known_fail
- Friction tag: symbol-lookup
- Notes: Current symbol search matches symbolName but not qualified_name; preserve this miss until repeated eval evidence justifies a broader phase.
- Answer: Unchanged. `cargo test -p repolens-mcp --test first_agent_eval_test` passed (`search_code_symbol_mode_keeps_qualified_profile_analyzer_known_fail ... ok`). The miss is still locked by the regression test; the qualified-vs-bare-name asymmetry is still the shape of it.
- Evidence: `cargo test -p repolens-mcp --test first_agent_eval_test` → 1 passed, 0 failed; same regression at `crates/repolens-mcp/tests/first_agent_eval_test.rs`.

### Run Notes

This is the **second run** of the first-agent eval contract, and the **first run that was not also the first answered run** — so it is a reproducibility/baseline data point, not a qualifying-PR data point.

Durability of the 2026-04-22 friction tags after this re-run:

- **F-repo-identification** — persists. `project-profile.json.description` is still `null`. This is expected (no code change touched it), but it confirms the friction is not flaky.
- **F-memory-conventions** — persists. `.repolens/memory/` and `.repolens/wiki/` still absent. Also expected — nothing has primed those stores.
- **F-hotspot-discovery (minor)** — carried forward, not re-probed on this run.
- **F-symbol-lookup (FAC-005)** — confirmed via the regression test.

What this run does **not** do:

- It does not count toward the "2–3 real qualifying PR runs" threshold the contract requires before a friction promotes to a broader product phase. Same commit, same code, no new PR.
- It does not re-probe FAC-002 end-to-end; the underlying code is unchanged, so re-running the probe would re-derive the same answer without adding independent evidence.

Decision posture: unchanged from 2026-04-22. No friction promotes. Next real data point requires a qualifying PR (one that changes scan, extraction, graph, profile, summary, or memory behavior). The small +1 drift in walked/included/orphan counts between runs on the same commit is a curiosity worth noticing but not acting on; if future runs show unstable counts on a frozen commit, that itself becomes a scan-determinism finding.
