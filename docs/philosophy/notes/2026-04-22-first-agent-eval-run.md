# First Answered First-Agent Eval Run — Post-merge of PR #33

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-22. Preserved as the authoritative record of its topic as of that date.

**Date:** 2026-04-22
**PR context:** post-merge of repolens-rs#33 — `Add first-agent eval contract` + review follow-ups
**Pack:** `.repolens/pack-20260421-155044` (full scan)
**Runner:** `bash scripts/dogfood-eval.sh`
**Scope:** First run that actually fills in FAC-001..FAC-005 answers. Exercised on `main` as a shape-and-friction check, not on an in-flight qualifying PR.

---

## Workflow-Closure Dogfood Snapshot

Commit: `ddd296e` | Pack: `pack-20260421-155044` | Scan mode: `full`

| Field | Value |
| --- | --- |
| Files walked | 384 |
| Files included | 313 |
| Excluded rows | 71 |
| Pack size | 4.2M |
| Scan duration | 1.60s |
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
- Answer: Cargo-built Rust CLI named `repolens-rs` (`projectType: CLI`, no frameworks). ~41k LOC of Rust across 142 files in `crates/` (dominant module, 52.6% share); 33k LOC of Markdown docs; shell `scripts/` tree; JSON `fixtures/`. One Rust binary entry point at `crates/repolens-cli/src/main.rs`. Dependency risk low; lockfile present. **Shape is clear from normal output; purpose is not — `project-profile.json.description` is `null` and `frameworks` is empty, so "what this tool does" requires reading CLAUDE.md/README outside the structured surface.**
- Evidence: `project-profile.json` — `name`, `projectType`, `buildSystem.name`, `modules[0]`, `entryPoints`, `health.dependencyRisk`. Snapshot table for file counts.

#### FAC-002 — Where should I start if I need to change a specific area?
- Workflow step: Inspect one focused hotspot
- Surface: search_code(path+symbol)+read_file_content
- Probe: Use RepoLens search plus one focused file read to identify the first edit point
- Expected status: pass
- Observed status: manual
- Friction tag: hotspot-discovery
- Notes: Capture whether the answer comes from normal tool flow or from manual pack spelunking.
- Answer: Concrete "specific area" tested — the `description: null` gap surfaced in FAC-001. RepoLens symbol search for `ProjectProfile` returned 1 hit at `crates/repolens-core/src/model/profile.rs` (struct declared at :58, `description: Option<String>` field at :65). The intended follow-up was one focused file read, but the current product surface did not expose the construction site, so the run fell back to `rg` over `crates/repolens-profile/` and found `crates/repolens-profile/src/profile.rs:42-48` where `description: None` is hardcoded. That line is the first edit point.
- Evidence: RepoLens symbol result for `ProjectProfile`; `crates/repolens-core/src/model/profile.rs:58,65`; fallback repo grep locating `crates/repolens-profile/src/profile.rs:42,48`.

#### FAC-003 — What prior memory or convention matters before making a change?
- Workflow step: Recall relevant memory or conventions
- Surface: get_summary(memory)+conventions
- Probe: Check whether one normal RepoLens pass surfaces the memory or convention that should constrain the change
- Expected status: pass
- Observed status: manual
- Friction tag: memory-conventions
- Notes: Preserve whether the answer was directly available or required manual reconstruction.
- Answer: **Normal RepoLens output surfaces nothing.** The intended `get_summary(memory)+conventions` surface had no data to return because `.repolens/memory/` does not exist on this repo and `.repolens/wiki/` is also missing. The run therefore fell back to direct repo instruction files rather than RepoLens surfaces. The quoted examples here came from `CLAUDE.md`; `AGENTS.md` independently carries the same binding rules, so the relevant conventions live in the repo instructions, not in RepoLens artifacts.
- Evidence: `.repolens/memory/` missing; `.repolens/wiki/` missing; `.repolens/session.json`; fallback direct reads of `CLAUDE.md` and `AGENTS.md`.

#### FAC-004 — What are the main structural risks right now?
- Workflow step: Inspect structural risk
- Surface: project-profile-health
- Probe: Use profile or summary output to identify current risks without ad hoc JSON digging
- Expected status: pass
- Observed status: manual
- Friction tag: risk-inspection
- Notes: This case stays manual because the product judgment is qualitative even when the data source is structured.
- Answer: Two `low`-severity signals: (1) 2 dependency cycles across 5 files (largest cycle 3 files), (2) 221 orphan files not referenced by other scanned files. Test-to-source ratio 0.15 — not flagged as a signal but worth monitoring. Dependency risk itself is `low` (lockfile present, 0 missing versions). Dominant module `crates` at 52.6% share, 4 modules total. **Data was directly available from the health block; no manual digging needed.**
- Evidence: `project-profile.json.health.signals[0..1]`, `health.graphHealth.cycleCount=2`, `orphanCount=221`, `testToSourceRatio=0.15`, `health.dependencyRisk.riskLevel=low`.

#### FAC-005 — Can symbol search find `ProfileAnalyzer::analyze` directly?
- Workflow step: Inspect one focused hotspot
- Surface: search_code(symbol)
- Probe: Query `ProfileAnalyzer::analyze` in symbol mode
- Expected status: known_fail
- Observed status: known_fail
- Friction tag: symbol-lookup
- Notes: Current symbol search matches symbolName but not qualified_name; preserve this miss until repeated eval evidence justifies a broader phase.
- Answer: No. FAC-005 remained `known_fail`, but this note did not invoke MCP `search_code` directly; it characterized the miss via the locked regression test and direct pack inspection. The regression in `crates/repolens-mcp/tests/first_agent_eval_test.rs` preserves that `ProfileAnalyzer::analyze` returns 0 symbol-mode results, while bare-name query `analyze` resolves the method with `qualified = "ProfileAnalyzer.analyze"` (dot separator). That asymmetry is the current miss: extracted qualified names use `.`, agents write `::`.
- Evidence: `crates/repolens-mcp/tests/first_agent_eval_test.rs:57-60`; direct inspection of `pack-20260421-155044/files-0001.json` showing 0 hits for `ProfileAnalyzer::analyze` and 1 hit for bare `analyze` with dotted qualified name.

### Run Notes

Context: first answered run of the eval contract, exercised on `main` post-merge of #33 on 2026-04-22. Not run on an in-flight qualifying PR — this is a shape-and-friction check.

Surfaced friction worth tracking across future runs:

- **F-repo-identification** — `project-profile.json.description` is `null`. An agent cannot answer "what does this tool do?" from the structured pack alone; the field is not populated from any source. Candidate fix surface: `crates/repolens-profile/src/profile.rs:42-48` (same line the FAC-002 probe landed on). Reasonable sources: Cargo.toml `[package].description`, README first paragraph, or CLAUDE.md front-matter.
- **F-memory-conventions** — `.repolens/memory/` and `.repolens/wiki/` are missing on a fresh-scanned repo. The FAC-003 surface has no data unless memorize/wiki-compile has been run. Either the first-scan flow should bootstrap empty artifacts, or the checklist should instruct contributors to prime those stores before running the eval.
- **F-hotspot-discovery (minor)** — Symbol search finds declarations cleanly, but to reach the *construction site* of a type the agent currently falls back to grep. A symbol cross-reference surface (e.g., "where is this struct instantiated?") would close that gap without a broader `search_code` redesign.
- **F-symbol-lookup (pre-existing, FAC-005)** — Confirmed. Not widened.

Decision posture per the contract: none of these promote to a broader product phase yet — the rule is 2-3 repeats across real PR runs. F-repo-identification is the most actionable and narrow; it's a candidate for a standalone fix slice if it appears again on the next qualifying PR.
