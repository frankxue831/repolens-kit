# Archived ACE eval runs

Sanitized, permanent summaries of past `repolens eval start` runs.
The original per-run scaffold (under a temp `.repolens/eval-runs/`) is
meant to be discarded by the author after the archive is written — the
`eval` command does not auto-remove it; the archive here is the durable record.

## Conventions

- One file per run: `ACE-NNN-<run-id>.md` matching the
  `--case` and `--run-id` arguments to `repolens eval start`.
- Each archive carries: case, run id, repo/binary provenance, setup,
  scorecard summary, observed behavior, findings, methodological
  caveats, product implications.
- Each archive ends with the explicit reminder: case-level controlled
  dogfood evidence, **not** product-level proof.
- Numeric and categorical metric values follow the definitions in
  [`../agent-context-evals.md`](../agent-context-evals.md). When an
  earlier run's vocabulary differs from the current one (e.g. blank vs.
  `n/a`), the archive records both forms explicitly.
- Scope: ACE eval runs **and** sanitized post-ship verification snapshots
  for shipped CLI features (e.g., `validate-plan`) when the verification
  bears on agent context evals. The earlier first-agent-eval (FAC) suite
  has its own retrospectives in
  [`docs/philosophy/notes/`](../../philosophy/notes/).

## ACE eval runs

| Run | Case | Verdict | Note |
|---|---|---|---|
| [`ACE-002-real-repo-v1`](ACE-002-real-repo-v1.md) | Constraint adherence | pass | Real-repo temp copy; constraint surfaced, one `rg` needed to ground implementation path. |
| [`ACE-002-postphase4b-v1`](ACE-002-postphase4b-v1.md) | Constraint adherence | pass | Post-Phase-4B re-baseline via independent subagent + round-6.1 `validate-plan` gate; surfaced that the ACE suite as written does not exercise the MCP-tool surface Phase 4B changed. |
| [`ACE-011-postphase4b-v1`](ACE-011-postphase4b-v1.md) | Evidence finding integration | fail | Establishing run for new ACE-011 (closes ACE-002-postphase4b-v1's structural gap). Subagent skipped MCP on an open-ended structural task even with calibrated findings available — exactly the failure mode the case is designed to catch. Plan was well-formed but grounded in brief + source-reads, not in any calibrated `F-*` evidence finding. |
| [`ACE-011-postphase4b-v2`](ACE-011-postphase4b-v2.md) | Evidence finding integration | fail | First follow-up to v1 after `repolens findings` (PR #65) + brief findings-pointer (PR #64). `mcp_invoked` flipped no→yes — the agent consumed the calibrated surface before drafting — yet still failed criterion 2: the plan was grounded in its own source-reads, with `F-*` findings cited only as context. Consumption without binding (access ≠ authority). Authors gate any trend conclusion on a v3 / different-vendor run. |
| [`ACE-011-postphase4b-v3`](ACE-011-postphase4b-v3.md) | Evidence finding integration | fail | Blind (rubric-blind) sub-agent; first run on the PR #90 criterion. **Surface-skip** (v1 mode, not v2's consume-no-bind): grounded an excellent Python→tree-sitter parity plan in source reads + a manual regex experiment + doc-06, never consuming the live findings. Headline: the highest-leverage improvement (Python extraction lossiness) is **absent** from the 4 graph-health findings, so grounding elsewhere was rational — reframes the case's bait as coverage-dependent. 0/3 runs bound a finding; binding never reached. |
| [`ACE-011-postphase4b-v3-codex`](ACE-011-postphase4b-v3-codex.md) | Evidence finding integration | fail | **Cross-vendor** companion to v3 (OpenAI Codex CLI, ground-truth `--json` trace). Blind; also **surface-skipped** — chose a *different* high-leverage improvement (split `graph.rs` resolution), grounded in source + doc-06 + a spec, never consuming the calibrated findings. Closes the cross-vendor limitation: across 4 runs / 2 vendors, **0/4 bound, 3/4 skipped the surface**; surface-skip *and* the finding-coverage gap both reproduce cross-vendor (Claude chose Python-parity, Codex chose graph-resolution — neither among the 4 findings). Binding is premature; the levers are coverage + consumption-entry. |
| [`ACE-011-postsurface-v4`](ACE-011-postsurface-v4.md) | Evidence finding integration | inconclusive | First run after this session's surface-enrichment (coverage #93, brief-inlined findings #94, cycle actions #95), provided-brief workflow. Agent consumed the inlined findings and cited `F-LANGUAGE-EXTRACTION-GAP` — but **contaminated**: it read the prior `v3` record (in-repo) as an answer key, so consume/bind can't be attributed to the surface. Durable value: **verified a memory-correctness bug** — Python regex lossiness → unresolved anchors → false `Contradicted` (`grounding.rs:73`) → faster decay (`decay.rs:103`). Methodology fix: isolate future runs from `docs/evals/runs/`. |
| [`ACE-004-independent-session-b-v1`](ACE-004-independent-session-b-v1.md) | Session continuity | pass | Independent Session B via unforked subagent; artifact-first handoff succeeded with explicit protocol prompt. |
| [`ACE-002-round-5.1-002`](ACE-002-round-5.1-002.md) | Constraint adherence | pass | Surfaced stale-binary scorecard drift; motivated Round 5.2 provenance block. |
| [`ACE-004-round-5.1-004`](ACE-004-round-5.1-004.md) | Session continuity | pass | End-to-end remember → brief flow; motivated Round 5.2 "Pick up here" lift and TemplateOnly hardening. |

## Shipped-feature verification snapshots

| Snapshot | Subject | HEAD | Note |
|---|---|---|---|
| [`round-6-validate-plan-shipped`](round-6-validate-plan-shipped.md) | `repolens validate-plan` (Round 6) | `ea81e81` | Six-check verdict against the user-spec verification bar; coverage gap (VP004) tracked separately. |
| [`tranche1-surfaces-blind-instrumentation-v1`](tranche1-surfaces-blind-instrumentation-v1.md) | Tranche-1 MCP surfaces (PRs #105-#107) | `83c064e` | Blind impact-trace run via isolated workspace. `limit` consumed + edge disclosure shaped calibrated language; line anchors unused on this task type; **new friction bug: `mcp-call.sh` hard-fails outside a git worktree** (agent hand-rolled a STDIO driver to reach MCP at all). Instrumentation, no pass/fail. |

| [`dogfood-round2-gm-crypto-rs`](dogfood-round2-gm-crypto-rs.md) | Integrated stack (#105-#109) vs the original field finding | local merge of #108+#109 on `0034814` | Real FFI-review task on the original field-eval repo, both vendors. **The field finding's complaints did not reproduce**: 0 grep (Codex, 116-cmd ground-truth trace), full funnel consumed, get_callers cited as evidence, hub ranking computed from get_graph, bounding consumed correctly. Foreign-code call-edge precision 100% adjudicated; unresolved +76% replicates. Four new friction findings: brief --task required (both vendors), brief side-effect write, get_impact file/files asymmetry, get_impact declares-edge inflation (agent-identified). |
| [`resolver-map-split-acceptance`](resolver-map-split-acceptance.md) | Resolver map split + Strategy 3.6 (spec 2026-06-12) | `bf193dc` | Old-vs-new full scans, both corpora. Unresolved −19%/−20% (growth vs pre-call-edges baselines halved: +76%→+40% foreign); **45/45 truly-new calls edges verified, 21/21 drops confirmed false-edge kills** (primitive `::from`, aliased-std `fs::*`, external-crate name collisions); structural edge sets byte-identical. Mid-arc discovery: post-split `use std::fs` aliasing became a 10%-of-calls false class → Layer 1.6 extractor filter. **Corrects dogfood round 2**: gm `public_key.rs` 40→26 inbound — the reported call-edge fan-in spike was substantially primitive-`From` false edges. |

## Adding a new archive

> **Blind runs must isolate the eval corpus.** A rubric-blind agent told to
> investigate the repo will find and read these run records as an answer key —
> see [`ACE-011-postsurface-v4.md`](ACE-011-postsurface-v4.md), where a blind
> sub-agent did exactly that and voided the consume/bind measurement. Before a
> blind run, create an isolated workspace with
> `scripts/dev/eval-blind-workspace.sh` (a HEAD snapshot with `docs/evals/`
> removed) and point the agent at that path.

1. Run `repolens eval start --case ACE-NNN --run-id <id>` in a temp
   workspace (or a real run).
2. Fill `scorecard.md`, `agent-transcript.md`, `result.md` by hand.
3. Distill the run into a new file in this directory using the same
   section structure as the existing archives.
4. Add a row to the **Index** table above.
5. Discard the temp workspace; the archive is the durable record.
