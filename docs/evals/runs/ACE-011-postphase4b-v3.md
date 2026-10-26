# ACE-011 — Evidence finding integration — `postphase4b-v3` (blind sub-agent)

**Date:** 2026-06-05
**PR context:** First run on the criterion amended by PR #90 (criterion 1 now credits `repolens findings`, not only MCP — the latent false-FAIL fix). Slice 2 of the consumption-binding arc.
**HEAD:** `0df5b07` (post-#90 `main`)
**Pack:** `pack-20260604-163900` (full scan; 429 files, 427 included)
**Runner:** a **blind general-purpose sub-agent** given *only* the canonical ACE-011 task ("Identify one high-leverage structural improvement opportunity in this repository, and produce a first-draft implementation plan"). It was told nothing about evals, ACE-011, the scoring criteria, or `repolens findings`. The scoring author (me) was contaminated — I had just rewritten the criteria in PR #90 — so a self-run would have taught to the test. Dispatching a rubric-blind agent restores the independence axis that matters most.
**Scope:** Third run of ACE-011 (after v1, v2). Purpose, per v2's stated limitation, is to add a data point that helps separate the "consumption-without-binding" signal from run-to-run variance.

---

## Setup (ACE-011 precondition)

`repolens findings --format json | jq length` → **4** calibrated findings present (precondition ≥1 met). The source-of-truth id set recorded before the run, for scoring criterion 2:

- `F-ORPHAN-FILES`
- `F-HIGH-FAN-IN-FILE`
- `F-CARGO-LOCK-STATE`
- `F-DEPENDENCY-CYCLES`

All four are **graph-health / repo-shape** observations. Note this for the headline finding below: none of them points at the improvement the agent chose.

## Method & trace provenance

The harness did not expose the sub-agent's per-tool-call sequence in a parseable form in the session transcript (the sub-agent's tool events are not materialized as greppable `command` records). So the trace is the agent's **self-reported basis**, cross-checked against side-effects — the same method v2 used ("Trace is self-reported. Cross-checked against side effects"):

- **No new pack** was created after the harness's `pack-20260604-163900` → the agent never ran `repolens scan`, so it never produced a live pack to read findings from.
- **`/tmp/sample.py` exists** (created during the run) → the agent hand-built a Python sample and ran the *regex* extractor logic against it as its own experiment.
- **Working tree clean at `0df5b07`** → no tracked-file mutation (the agent was told investigation-only and complied).
- A scan of the recorded command region for any calibrated-finding-surface invocation (`get_summary` / `analyze_health` / `compare_summaries` / `repolens findings` / `scan`) returned **0** matches attributable to the sub-agent.

## Per-criterion breakdown

| Criterion | Status | Evidence |
|---|---|---|
| 1. Consume the calibrated finding surface before drafting | ❌ **FAIL** | The agent never invoked any calibrated-finding surface (no scan → no live pack → no `repolens findings`/`get_summary`/`analyze_health`). It read the **static fixture pack** `fixtures/scenarios/mixed-small/pack/files-0001.json` instead of a live finding surface. Surface-skip — the v1 failure mode, not v2's consume-no-bind. |
| 2. Cite a finding id matching the recorded response | ❌ **FAIL** | No surface consumed ⇒ no recorded response ⇒ no `F-*` id cited. The plan's "Basis" cites file:line source evidence and doc-06/07, but zero calibrated finding ids. |
| 3. Ground claims to evidence | ✅ PASS | Every structural claim is grounded in **direct source inspection with file/line citations** (`py_symbol.rs` `line.starts_with(' ')` skip; `js_symbol.rs:219-229`; `registry.rs:104-130`; `graph.rs:172-186`; `ts_util.rs:44-57`) — criterion 3 explicitly accepts "(b) direct source inspection." |
| 4. Action alignment | n/a | No calibrated finding cited → no `.action` to align against. |
| 5. Pre-flight `validate-plan` | ❌ not performed | No plan file written, tree clean, no `validate-plan` invocation; the agent listed `validate-plan` only as a *future* verification step. (Moot — item 1 already fails.) |

**Verdict: `fail` (criterion 1).** Surface-skip.

## Observed behavior

The agent went straight to **source inspection**: it read the six-language extractor set, ran the Python regex logic against a hand-built `/tmp/sample.py` to demonstrate the loss (3 of 6 symbols, 1 of 5 imports captured), inspected the contract fixtures, and grounded the trust argument in doc 06 §5 + `decay.rs`. It then chose — correctly and with strong justification — **"replace Python's regex symbol/relationship extractors with tree-sitter, bringing Python to parity with the other five languages."** The resulting first-draft plan is genuinely high quality: concrete file targets, a correct read of the parse-once fast-path seam, TDD-first verification, golden-fixture re-capture flagged as an intended (non-byte-identical) contract change, and a sharp framing of the bug as a memory-corruption path (missing symbols → false `Contradicted` grounding → accelerated decay).

It produced an excellent plan **without ever touching RepoLens's own calibrated finding surface.**

## The headline finding: the best improvement was not in the findings

The four live findings are graph-health observations (orphans, high fan-in, Cargo.lock state, dependency cycles). **None of them points at Python-extraction lossiness** — the improvement the agent identified as highest-leverage. So even a perfect consume-and-bind run could not have produced this plan *from the findings*. On this repo at this moment, the single highest-leverage structural improvement is **not represented in the calibrated finding set**, and grounding the choice in source inspection was therefore the *rational* path, not negligence.

This reframes ACE-011's "bait" assumption. The case is built on the premise that an agent that respects the calibrated substrate will find a high-leverage improvement *in it*. That premise is **coverage-dependent**: it holds only when the findings actually contain the best available improvement. Here they did not.

## Comparison to baseline

| Signal | v1 | v2 | v3 (blind) |
|---|---|---|---|
| Consumed calibrated surface before drafting | no | **yes** | **no** |
| Plan bound to a calibrated `F-*` finding | no | no | no |
| Plan quality (independent of binding) | well-formed | well-formed | well-formed (strong) |
| Grounding basis | brief + source | own source-reads | source + manual experiment + doc-06 |
| Verdict | fail | fail | fail |

Across three runs: **0/3 bound a finding; consumption itself was 1/3.** Binding has never been observed. The variance is in *consumption* (v2 consumed, v1/v3 did not), and v2's consumption still didn't bind. So "consumption-without-binding" (v2) and "surface-skip" (v1, v3) are both live failure modes, and the binding step has not been reached in any run.

## Friction findings (tool-worked-vs-fell-back)

1. **Surface-skip recurs (v1, v3).** Given an open-ended structural task, the agent reached for source reading + a hand-built experiment, not the calibrated surface. *Access alone is insufficient* — and here the cheap `repolens findings` path wasn't reached because the agent never scanned (it had no live pack and didn't make one).
2. **Finding-coverage gap (new, sharp).** The highest-leverage improvement (Python parity) is absent from the calibrated finding set, which is entirely graph-health. An agent optimizing for leverage *should* look past findings that don't contain the best answer. This is a finding about RepoLens's **coverage**, not the agent's discipline.
3. **Brief-pointer likely bypassed (as in v2).** The sub-agent had `CLAUDE.md` (which mentions `repolens` commands) but, on an open-ended task, did not route through `repolens brief` / its findings-pointer. The PR #64 discoverability lever again did nothing for an agent that doesn't start from the brief.

## Limitations

- **N=1 blind run; three runs total, all same model family.** This adds a variance data point but does **not** address cross-vendor variance — the other half of v2's requested control. A different-vendor run is still open.
- **Self-reported trace.** The harness did not expose the sub-agent's per-tool ordering; chronology is from the agent's own basis cross-checked against side-effects (no pack, `sample.py`, clean tree, zero consumption-command matches). Consistent with v2's caveat.
- **Blind-but-not-primed.** The sub-agent received `CLAUDE.md` but was not walked through the dogfood entry path (`repolens init`/`brief`) a primed agent would follow. If it incidentally read `docs/evals/agent-context-evals.md` while grepping `docs/`, that would only *bias toward consuming* the surface to pass — the opposite of the observed skip — so the surface-skip result is **conservative** with respect to rubric contamination.
- **Static-fixture substitution.** The agent read the captured fixture pack JSON as a stand-in for live findings; that is structural evidence, but it is not the live calibrated surface the case measures.

## Product implications (for the slice-3 decision)

- **"Can a calibrated finding *bind* a plan?" may be premature to engineer.** We have not yet observed a run where the agent both (i) consumed the findings **and** (ii) the findings contained the best available improvement. Building binding mechanics now would optimize for "bind to a finding that isn't the best improvement" — which a good agent *should not* do.
- **Two distinct, higher-priority levers surface ahead of binding:**
  1. **Coverage.** If the calibrated findings routinely omit the highest-leverage improvements (here: a whole-language extraction asymmetry), expanding what the finding set *can observe* — or adding a finding family that flags this kind of capability gap — likely matters more than binding ergonomics.
  2. **Consumption entry.** The cheap path (`repolens findings`) wasn't reached because the agent never scanned. The brief-pointer (PR #64) keeps being bypassed on open-ended tasks. The blocker upstream of binding is "agent never reaches a live finding surface."
- **Recommended next measurement:** before investing in binding, run an ACE-011 variant on a repo/state where a high-leverage improvement **is** represented in the calibrated findings (or seed one), to test binding in the case where the findings actually contain the answer. And/or the still-open cross-vendor run. The maintainer decides whether slice 3 becomes "coverage" or "binding."

> **Disclaimer.** This is case-level controlled dogfood evidence from a single blind run, **not** product-level proof. The plan the agent produced is independently strong; the `fail` verdict is strictly about whether RepoLens's calibrated surface bound (or was even consumed by) the agent's first plan.
