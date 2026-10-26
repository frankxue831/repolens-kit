# ACE-011 — Evidence finding integration — `postphase4b-v3-codex` (cross-vendor)

**Date:** 2026-06-05
**PR context:** Cross-vendor companion to [`ACE-011-postphase4b-v3`](ACE-011-postphase4b-v3.md) (the within-vendor Claude blind run). Together these are the v3 variance control. Slice 3 of the consumption-binding arc.
**HEAD:** `0df5b07` content (run from branch `docs/ace-011-postphase4b-v3`; the Rust source is identical to `main`).
**Pack:** `pack-20260604-163900` (active; 4 calibrated findings — surface live and one command away).
**Runner:** **OpenAI Codex CLI 0.136.0** (`codex exec`, default model), a **different vendor** from v1/v2/v3 (all Claude family). Given the *same* blind task verbatim as the Claude v3 run — no rubric, no mention of evals/findings. Sandbox `workspace-write` so it had the genuine choice to scan/consume.
**Trace provenance:** **ground-truth.** `codex exec --json` emitted the full command stream; all 38 shell commands are recorded. This is a methodological upgrade over the Claude v3 run, whose per-tool order the harness did not expose (self-reported there). Working tree was clean before and after (no tracked-file mutation).

---

## Setup

Identical precondition to v3: active pack `pack-20260604-163900` with **4** calibrated findings (`F-ORPHAN-FILES`, `F-HIGH-FAN-IN-FILE`, `F-CARGO-LOCK-STATE`, `F-DEPENDENCY-CYCLES`), the prebuilt `./target/debug/repolens` binary present, and `session.json` pointing at the pack — so `repolens findings` would have returned the calibrated set in **one command, zero build**. Consuming the surface was maximally frictionless; skipping it was a free choice.

## Blindness (verified from the trace, not assumed)

- The only command touching `docs/evals/` was `rg -n "GraphBuilder|resolve_target|…" docs/superpowers/plans docs/superpowers/specs docs/reviews docs/evals/runs` — a search for *graph-resolver* terms. Those terms do not occur in the ACE-011 run records, and the rubric file `docs/evals/agent-context-evals.md` was **never opened**. So Codex did not see the criteria.
- (Even had it glimpsed a run record, knowing the rubric biases *toward* consuming the surface to pass; a skip is therefore conservative.)

## What Codex did (ground-truth trace, 38 commands)

1. Read its **own** Codex superpowers skills (`using-superpowers`, `writing-plans`, `brainstorming`, `verification-before-completion`) — not repo files.
2. `git status` / `git worktree list`; surveyed the repo (`rg --files`, `wc -l` per source file to find large files), `rg` for extractor/registry/`tree_sitter` patterns.
3. Read `extract/src/lib.rs`, `registry.rs`, then **`graph.rs` extensively** (multiple ranges, `nl -ba`), `scan.rs`, `graph_analysis.rs`.
4. `rg` for `TODO|FIXME|duplicat|drift|friction|too large|monolith|split|graph` across `docs` + `src`; read `docs/superpowers/plans/2026-04-05-rust-graph-improvements.md`, the graph-import-edges invariant-catalogue spec, two tree-sitter extractor specs, **doc 06**, and the parse-once plan.
5. Ran **`cargo test -p repolens-app -- graph`** (baseline; exit 0, 32 tests) — its only execution beyond reads.
6. Produced its plan.

**Chosen improvement:** *extract graph target resolution out of `graph.rs` into a dedicated `crates/repolens-app/src/graph/resolution/` subsystem* (a maintainability refactor: `resolve_target` mixes Java/Rust/Go/JS-TS/Python resolution + external classification in one function). Grounded in **direct source reading** (`graph.rs:384` `resolve_target`, `:733` external classification) + **doc 06** (graph observations historically fragile) + the **graph-import-edges invariant-catalogue spec**.

It **never consumed the calibrated finding surface** (no `scan`, `findings`, `get_summary`, `analyze_health`, or `mcp-call` in the trace — verified) and cited **no `F-*` id**.

## Per-criterion breakdown

| Criterion | Status | Evidence (ground-truth trace) |
|---|---|---|
| 1. Consume the calibrated finding surface before drafting | ❌ **FAIL** | Zero finding-surface commands in 38 executed. Surface-skip. |
| 2. Cite a finding id matching the recorded response | ❌ **FAIL** | No surface consumed; plan cites `graph.rs` line refs + doc-06 + a spec, no `F-*` id. |
| 3. Ground claims to evidence | ✅ PASS | Claims grounded in direct source inspection with file:line (criterion 3 (b)). |
| 4. Action alignment | n/a | No finding cited. |
| 5. Pre-flight `validate-plan` | ❌ not performed | Listed the gate as a *future* step; ran only a baseline `cargo test`. |

**Verdict: `fail` (criterion 1).** Surface-skip — same as Claude v3 and v1.

## Cross-vendor synthesis (the payoff — closes v2/v3's open limitation)

| Run | Vendor | Consumed surface? | Bound a finding? | Chosen improvement | Verdict |
|---|---|---|---|---|---|
| v1 | Claude | no | no | (source-grounded) | fail |
| v2 | Claude | **yes** | no | source-grounded | fail |
| v3 | Claude | no | no | Python → tree-sitter parity | fail |
| **v3-codex** | **GPT/Codex** | **no** | **no** | split `graph.rs` resolution | fail |

**Across 4 runs / 2 vendors: 0/4 bound a finding; 3/4 skipped the calibrated surface entirely; the lone consumer (v2) did not bind.** Two conclusions that single-vendor v1–v3 could not support:

1. **Surface-skip is not run-to-run variance, and not Claude-specific.** A different vendor, blind, with the surface one frictionless command away, *also* went straight to source inspection. This is a consistent agent behavior on open-ended "find a high-leverage improvement" tasks — **access ≠ adoption**, cross-vendor.
2. **The coverage gap is cross-vendor.** Claude independently chose *Python-extraction parity*; Codex independently chose *graph-resolution refactor*. **Both are real, high-leverage structural improvements; neither is in the 4 calibrated findings** (orphans / fan-in / lockfile / cycles). Two vendors, two different high-value improvements, both absent from the finding set — strong evidence that the calibrated findings do not contain the improvements agents naturally identify on this repo.

This **closes the "cross-vendor run still open" limitation** flagged in v2 and v3.

## Implications for the slice-3 / next decision

- **"Can a calibrated finding *bind* a plan?" is now clearly premature.** Binding cannot be the bottleneck when, cross-vendor, agents (a) don't consume the surface and (b) wouldn't find their chosen improvement in it. Engineering binding now optimizes a step no run has reached.
- **The two real levers, now cross-vendor-supported:**
  1. **Coverage** — the finding set is graph-health-only; it misses capability/structural improvements (extraction asymmetry; resolver monolith) that *both* blind agents surfaced. A finding family that observes these would put the high-leverage items *into* the surface.
  2. **Consumption entry** — even a one-command, zero-build `repolens findings` was not reached; the brief-pointer is bypassed on open-ended tasks. The bottleneck is upstream of binding.
- **Maintainer decides** the next slice. The variance control is now complete; the evidence points at coverage and entry, not binding.

## Limitations

- One Codex run (N=1 per vendor; 4 runs total). It establishes cross-vendor *reproduction* of surface-skip + the coverage gap, not a rate.
- Codex read its **own** `brainstorming`/`writing-plans` skills before drafting; that may shape plan *format*, but does not explain the consume-vs-skip choice (the skills are vendor tooling, silent on RepoLens).
- Both v3 arms ran on this repo, so the coverage gap is demonstrated for *this* finding set; the general claim ("findings omit high-leverage structural improvements") is supported but not proven for all repos.
- `workspace-write` was available; the skip was a free choice, not a sandbox artifact (verified: clean tree, no scan).

> **Disclaimer.** Case-level controlled dogfood evidence across two blind runs, **not** product-level proof. Both agents' plans are independently sound; the `fail` verdicts are strictly about whether RepoLens's calibrated surface was consumed by / bound the first plan.
