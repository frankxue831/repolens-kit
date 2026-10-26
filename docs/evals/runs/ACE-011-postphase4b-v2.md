# ACE-011 — Evidence finding integration (run `postphase4b-v2`)

> Sanitized archive of a controlled dogfood run. This is controlled
> case-level dogfood evidence, **not** product-level proof.

## Case

ACE-011 — Evidence finding integration (calibrated MCP consumption) (see
[`docs/evals/agent-context-evals.md`](../agent-context-evals.md) for the
canonical case definition). The case tests whether the calibrated finding
surface binds an agent's first plan on an open-ended structural task.

## Run id

`postphase4b-v2` — first follow-up to
[`postphase4b-v1`](ACE-011-postphase4b-v1.md). v1 was the establishing
run (verdict: fail; `mcp_invoked: no`).

## Why this run

v1's recommendation: schedule follow-up runs after each consumption-ergonomics
product change, tracking whether `mcp_invoked` flips no→yes. Two such changes
landed since v1:

- **`repolens findings`** (PR #65) — a CLI wrapper that prints the active
  pack's calibrated `evidenceFindings` as agent-readable markdown (or JSON),
  delegating to the same `SummaryService` the MCP `get_summary` tool uses.
  Removes the "spin up the MCP loop" cost v1 identified.
- **Brief Runtime Freshness + a findings pointer** (PR #64) — the brief's
  Suggested First Checks now contains a bullet directing the agent to run
  `repolens findings`. This is the discoverability lever: v1 showed agents
  only follow what the brief surfaces.

v2 tests whether either change moves agent behavior.

## Repo / binary provenance

- **HEAD at run.** `375e5f9 Merge pull request #65` (main; includes #64 + #65).
- **Pre-run active pack.** `pack-20260516-015206` (same pack v1 ran against).
  Finding-id set recorded before dispatch (source-of-truth for criterion 2):
  `F-ORPHAN-FILES`, `F-HIGH-FAN-IN-FILE`, `F-CARGO-LOCK-STATE`,
  `F-DEPENDENCY-CYCLES`. Parity verified: `repolens findings --format json`
  returned a byte-identical id set to MCP `get_summary --detail full`.
- **The agent re-scanned during the run** (`pack-20260521-143933`, now the
  active pack). Its source-of-truth pack therefore differs from the pre-run
  record, but the returned finding-id set is identical, so criterion-2 scoring
  is unaffected.

## Setup summary

1. Verified the precondition: active pack surfaces ≥1 calibrated finding (4
   findings present, listed above).
2. Dispatched an independent unprimed subagent (general-purpose, via the Agent
   tool) with a faithful replica of v1's prompt: the verbatim task, an
   enumeration of available surfaces (the `repolens` CLI with `--help`, the
   brief workflow, the MCP one-shot helper `scripts/dev/mcp-call.sh` with a
   `get_summary` example and 14 tool names, the Round-6.1 `validate-plan`
   requirement). **No** framing about evidence findings, `F-*` ids, Phase 4B,
   `repolens findings` specifically, or this being an eval was leaked. The
   agent was asked to also report a complete chronological command/tool trace.

No implementation was performed.

## Task given to agent (verbatim)

> Identify a high-leverage structural improvement opportunity in this
> repository and produce a first-draft plan.

## Methodology

| Dimension | Value |
|---|---|
| Agent under test | Independent unprimed subagent dispatched via Agent tool |
| Workspace | Real repo in place — `.repolens/` and pack available |
| Round | 6.1 (validate-plan pre-flight required) |
| Surface choice | Subagent's free choice |
| Delta vs v1 | Repo state only: `repolens findings` exists + brief points at it |
| Trace source | Subagent self-report, cross-checked against side effects |

## Scorecard summary

| Metric | Value | Note |
|---|---:|---|
| `time_to_useful_context` | ~14 cmds pre-draft | ls → manifest → LOC survey → `--help` → read validate_plan.rs → known-constraints → `discover_packs` → `scan` → `get_summary` → `analyze_health` → `findings` → `get_architecture` → source reads → draft. |
| `repeated_discovery_count` | 0 | |
| `grep_fallback_count` | 0 | Used MCP + findings + targeted source reads; no unstructured grep-as-fallback (greps were targeted symbol counts). |
| `wrong_file_attempts` | 0 | |
| `clarification_count` | 0 | |
| `known_constraint_miss` | 0 | Plan respects + cites `C-001`. |
| `first_plan_quality` | `fail` | Criterion 2 unmet. See breakdown. |
| `human_intervention_count` | 0 | |
| `session_handoff_quality` | `n/a` | Single-session case. |
| `stale_checkout_reconciled` | `n/a` | |
| `memory_safety_notice_used` | `n/a` | |

**Case-specific signals:**
- `mcp_invoked: yes` — `get_summary --detail full` and `analyze_health`
  invoked **before** drafting. **This flips v1's `no`.**
- `findings_invoked: yes` (new signal) — `repolens findings` invoked before
  drafting.
- `brief_invoked: no` (new signal) — the agent never ran `repolens brief`;
  confirmed by the brief file's unchanged mtime. **The brief-pointer lever was
  not exercised.**

**Verdict:** **fail** (criterion 2). The flip in `mcp_invoked` is the headline
new signal, but it did not produce a passing plan.

## Per-criterion breakdown

| Criterion | Status | Evidence |
|---|---|---|
| 1. Consume MCP before drafting | ✅ PASS | `get_summary --detail full` + `analyze_health` invoked before the plan was written; trace records them chronologically. (Also ran `repolens findings`.) |
| 2. Cite a finding id from the recorded response | ❌ FAIL | Plan cites `I-GRAPH-ORPHANS-RESPECT-ENTRY-POINTS` (an invariant id, not a finding id) and `graph.orphanRatio` (a metric) "only as context." No `F-*` finding id from the recorded set binds the plan. |
| 3. Ground claims to evidence | ✅ PASS | Claims grounded via direct source inspection with file citations (`rust_symbol.rs`, `java_symbol.rs`, `rust_extract_test.rs`) + the agent's own pack-symbol counts. Criterion 3 explicitly accepts "(b) direct source inspection." |
| 4. Action alignment | n/a | No calibrated finding cited → no `.action` to align with. |
| 5. Pre-flight `validate-plan` | ✅ PASS | Ran twice to clean; cited output verbatim; addressed VP001 by fencing identifiers. |

## Observed behavior

- The agent went **straight to analysis tools**, not the brief: `scan` →
  `get_summary` (MCP) → `analyze_health` (MCP) → `repolens findings` (CLI) →
  `get_architecture` (MCP), all before drafting. It consumed the calibrated
  surface through **both** the new CLI path and MCP.
- Despite consuming the findings, it **did not let any of them bind the plan**.
  It chose its improvement — recurse the Rust symbol extractor into inline
  `mod { … }` bodies — by reading `rust_symbol.rs` (which admits "no deeper
  nesting in v1"), counting symbols in the freshly-scanned pack, and comparing
  against `java_symbol.rs` (which recurses). The calibrated findings
  (`F-ORPHAN-FILES` etc.) were treated as exploratory context, not grounding.
- The plan itself is well-formed, source-grounded, and validate-plan-clean —
  a genuinely useful change. But it is **not** a calibrated-finding-bound plan,
  which is what ACE-011 measures.

## Comparison to baseline (v1)

| Signal | v1 | v2 |
|---|---|---|
| `mcp_invoked` | no | **yes** |
| `findings_invoked` | n/a (command didn't exist) | **yes** |
| `brief_invoked` | yes | **no** |
| Plan bound to a calibrated `F-*` finding | no | no |
| Verdict | fail | fail |

The surface **was consumed** this time (the central v1 failure mode did not
recur), but the plan still was not **bound** to a calibrated finding.
Consumption proved necessary-but-not-sufficient.

## Friction findings

- **consumption-without-binding.** The sharpest new finding. v1's failure was
  "agent never looks at the calibrated surface." v2's failure is one layer
  deeper: the agent *looks* (MCP + `repolens findings`, before drafting) and
  then *sets the findings aside*, grounding its plan in its own source reading
  instead. Making findings cheap and present is not the same as making them
  authoritative for planning. This reframes the product problem: the blocker
  may not be access/discoverability but whether an agent treats a calibrated
  finding as a stronger plan-anchor than its own source judgment.
- **brief-pointer-bypassed.** The discoverability lever added in PR #64 (a
  Suggested First Checks bullet) did nothing here because the agent never ran
  `repolens brief`. On an open-ended "find an improvement" task, the agent
  reached for analysis tools directly. The pointer only helps agents that read
  the brief first; this agent did not.
- **mcp-flip-confounded-by-variance.** MCP was offered identically in v1 and
  v2; my intervention did not touch MCP. One run skipped it, one used it. Per
  v1's own stated limitation (a single run is not a pattern), the flip
  **cannot be attributed to the intervention** — it is consistent with
  run-to-run variance in exploration strategy. `repolens findings` *was* used,
  which is a real positive ergonomics signal for that command, but it sat
  alongside MCP rather than being the thing that ended the skip.
- **findings-vs-MCP criterion gap.** `repolens findings` returns a byte-
  identical finding set to MCP `get_summary`, yet criterion 1 as written only
  credits MCP tool calls and explicitly says non-MCP consumption "does not
  satisfy this criterion." In v2 the agent used MCP anyway so the gap did not
  bite, but a future run that consumes the *identical* calibrated data via
  `repolens findings` alone would be auto-failed on item 1 despite genuine
  consumption. The case body should be amended.

## Limitations

- **Single run, same model family.** v2 is one data point against v1's one
  data point. Two runs cannot separate intervention effect from variance.
  v3 (and ideally a different vendor) is needed before any flip is a pattern.
- **Trace is self-reported.** Cross-checked against side effects (new pack
  `pack-20260521-143933` exists; brief mtime unchanged → no brief run), but
  the chronological ordering of tool calls relative to drafting relies on the
  agent's honest log.
- **Agent re-scanned mid-run**, shifting the active pack. Finding set was
  identical, so scoring is unaffected, but a future protocol may want to mark
  the pack read-only or snapshot it.

## Product implications

- **The cheap path gets used, but doesn't change the outcome.** `repolens
  findings` was invoked unprompted before drafting — agents will reach for a
  cheap calibrated dump. But on this run it did not make a finding bind the
  plan. The original hypothesis ("cost + discoverability is the blocker") is
  only weakly supported: cost was removed and the surface was consumed, yet the
  plan still wasn't finding-bound.
- **The real gap looks like authority, not access.** The agent consumed
  calibrated findings and still preferred its own source-derived structural
  judgment. Backlog #2/#3 (embedding findings in the brief body) would force
  *presence* but, on this evidence, presence alone may not produce *binding*.
  Worth weighing before investing in #2's architecture cost.
- **The brief pointer is unproven.** It was bypassed here. It may still help
  agents that start from the brief (the common dogfood path for scoped tasks),
  but this open-ended task did not exercise it.

## Recommended next changes

- **[case]** Amend ACE-011 criterion 1 to accept `repolens findings` (and any
  CLI surface that emits the same calibrated `evidenceFindings`) as valid
  consumption, since it returns the identical finding set to MCP. Keep the
  "brief alone does not count" exclusion. This is case-evolution-driven-by-run.
- **[case]** Run `postphase4b-v3` (and ideally a different vendor) before
  drawing any conclusion from the `mcp_invoked` flip. Two runs is not a trend.
- **[case]** Consider a task variant that is *scoped* rather than open-ended
  (so the agent starts from `repolens brief`), to actually exercise the
  brief-pointer lever PR #64 added.
- **[product]** Treat "consumption-without-binding" as the new headline
  problem. Before building #2/#3, decide whether the goal is achievable by
  *presence* at all, or whether it needs the finding surfaced as a constraint
  the plan must address (closer to how `known-constraints.md` binds in
  ACE-002, which *passes*).

## Appendix A — Full subagent plan (verbatim)

````markdown
### Plan: Make the Rust symbol extractor recurse into inline `mod { … }` bodies

**1. Problem (verified by dogfooding RepoLens on itself)**

RepoLens is a Rust codebase whose flagship job is extracting code structure,
yet its Rust symbol extractor cannot see any symbol declared inside an inline
module (`mod foo { … }`, including every `#[cfg(test)] mod tests { … }` block).

`crates/repolens-extract/src/rust_symbol.rs` walks only the AST root and `impl`
bodies. Its own comment admits this: "Only recurses into `impl_item` bodies …
no deeper nesting in v1." When it hits a `mod_item`, it emits a single `module`
symbol and never descends into the module body.

Concrete evidence — scanned this repo (pack `pack-20260521-143933`) and
inspected emitted symbols for `crates/repolens-cli/src/validate_plan.rs`: the
file's `mod tests` block holds ~30 `#[test]` functions, but the pack records
only one symbol for that module and zero inner functions. A grep for inline
`mod NAME {` bodies in `crates/*/src` and `crates/*/tests` returns 55 matches.

Symbol search (`search.rs::search_symbol`) iterates `entry.symbols`, so it
silently cannot find anything defined inside an inline module. The Java
extractor already recurses the full tree; the Rust *relationship* extractor
already recurses inline modules — only the Rust *symbol* side lags.

**2. Goal / scope** — teach `RustSymbolExtractor` to descend into inline module
bodies and emit contained symbols, while still emitting the module symbol.
Out of scope: import/relationship graph + `declares` edges; other languages;
the wire contract (the `Symbol` model is untouched, so C-001 camelCase stays
byte-compatible).

**3. Files** — edit `rust_symbol.rs` (only production file); edit
`rust_extract_test.rs`; read-only verify `fixture_parity_test.rs`,
`pack_contract_test.rs`, fixtures.

**4–8.** Up-front verification, a recursive `collect_symbols_in` helper shared
by the root walk and an `extract_module` helper, keep `qualified_name` as-is in
v1, new tests for inline/cfg-test/impl-in-module/nested-module cases, pre-push
gate green, dogfood re-scan proof, and risk fences (snapshot shift, double-
counting, graph scope creep) — full text in the run scaffold.
````

## Appendix B — Command/tool trace (subagent self-report)

Pre-draft (in order): `ls` + manifest; crate listing; per-crate LOC; largest
files; `cargo build`; `cargo run -- --help`; `validate-plan --help`; read
`validate_plan.rs`; `cat known-constraints.md`; `discover_packs`; `scan`
(→ `pack-20260521-143933`); `get_summary --detail full` (MCP); `analyze_health`
(MCP); `repolens findings` (CLI); `get_architecture` (MCP); extract-crate file
listing; pack symbol-count analysis; read `rust_symbol.rs`; inline-`mod` grep +
`validate_plan.rs` symbol inspection; `Symbol`/serde grep; read
`java_symbol.rs`, `go_symbol.rs`, `rust_extract_test.rs`; `search.rs` usage
grep; `registry.rs`; fixture checks.

Post-draft: write `/tmp/plan-v2.md`; `validate-plan` (1 warn); reword;
`validate-plan` (2 warns); fence identifiers; `validate-plan` (clean).
