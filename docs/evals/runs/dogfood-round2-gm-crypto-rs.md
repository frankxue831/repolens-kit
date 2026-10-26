# Dogfood round 2 — gm-crypto-rs (the original field-eval repo), post-tranche stack

**Subject:** the integrated agent-ergonomics stack — tranche 1 (PRs #105-#107,
merged), tranche 2 (#108) + Rust call edges (#109) via a local integration
merge of both open PRs (clean, no conflicts; 80/80 mcp tests green) — measured
against the repo that generated the original field finding
(`docs/reviews/2026-06-10-agentic-code-map-audit.md` §mission): the
cross-repo evaluation where RepoLens was "orientation-only" and the agent
"bypassed RepoLens MCP tools and read raw pack JSON or used grep."

**Method:** real task, not a quiz — an FFI integration-risk review + SM2
decrypt blast-radius assessment on `gm-crypto-rs` (369 Rust files, cbindgen
FFI crate, 27 fuzz targets), mirroring the original field eval's shape. Two
vendors, identical prompts, no rubric, free tool choice: Claude sub-agent
(32 tool uses, self-reported log cross-checked against harness count) and
OpenAI Codex (`--json` ground-truth trace, 116 commands). Fresh full scan
with the integrated binary (`pack-20260611-102128`); old packs from 05-31 /
06-08 retained as the pre-stack metrics baseline. Mechanical
precision/distortion checks scorer-run before dispatch (seed 20260611).

## Mechanical results (foreign code)

| Measure | gm-crypto-rs | repolens-rs self-scan (reference) |
|---|---|---|
| Calls edges | 198 (of 528 total edges) | 328 |
| Precision, seeded 60-sample | 93.3% mechanical → **100% adjudicated** | 96.7% → 100% |
| Unresolved growth vs old-binary pack | **+76%** (203→357) | +72% |
| Fan-in shift from calls edges | Strong: `public_key.rs` 19→40 inbound, `private_key.rs` into top-2 — real connectivity, but calls-share now dominates the top of the fan-in ranking | Mild reordering |

Adjudication note: all four mechanical flags were `Fp::new`/`Fn::new`-class
calls resolving to `sm2/curve.rs`, where `Fp`/`Fn` are **type aliases**
(`pub type Fp = ConstMontyForm<...>`). Correct at file-level granularity
(the alias-defining file is the right dependency target) with an honest
nuance: the aliased constructor lives in external `crypto_bigint`, so
"calls" here is alias-mediated. The +76% unresolved replication confirms the
resolver free-function/impl-method map split as the required pre-expansion
fix (call-edges spec §acceptance, criterion 2).

## Agent behavior — before/after the original field finding

| Original complaint (06-02 era) | Codex run (116 cmds, ground truth) | Claude run (32 tools) |
|---|---|---|
| "Bypassed RepoLens MCP tools" | **7 RepoLens invocations, first at cmd #11**; full funnel `--help` → `state` → `brief` → `findings` → 3× `get_callers` | RepoLens from action **#5**; `get_callers`, `get_dependencies`, `get_impact`, `get_graph` ×2, `findings`, `brief` |
| "Used grep where get_callers should be ideal" | **0 grep/rg in 116 commands**; `get_callers` aimed at the exact blast-radius chain (decrypt → raw_ciphertext → asn1/ciphertext), results cited in the deliverable: "RepoLens mechanically confirmed direct callers of `sm2/decrypt.rs`" | grep only for line-level semantics; **hub ranking computed FROM `get_graph`** (in-degree analysis), not from grep |
| "Read raw pack JSON" | None — graded envelopes consumed | None — envelopes + calibrated findings consumed (cycle + high-fan-in findings cited as evidence) |
| "Orientation-only value" | Orientation **and** evidence-grade blast radius from the tool surface | Same, plus the tranche-2 bounding consumed correctly in the wild: `get_impact` returned 50 of `totalImpacted: 56` with `truncated` — and the agent reasoned about it |

Both deliverables were high quality (the Claude review surfaced
zeroization-asymmetry, handle-consumption double-free, and undocumented
thread-safety risks in the FFI shim with file:line evidence throughout).

## New friction findings (the dogfood payload)

1. **`repolens brief` requires `--task` — both vendors hit it.** Codex ran
   bare `brief` (exit 2) and never retried; Claude failed the same way, then
   retried with a task string and succeeded. The consumption funnel's front
   door bounces an exploring agent. Candidate fix: make `--task` optional
   with a generic default brief.
2. **`brief` writes a side-effect artifact** (`.repolens/current-agent-brief.md`)
   — the Claude agent, operating under a read-only contract, disclosed it as
   a violation-adjacent event and attempted cleanup. A read-only/stdout-only
   mode (or at minimum documented side-effect disclosure in `--help`) would
   let agents under no-write constraints use the brief.
3. **`get_impact` param asymmetry**: `get_callers`/`get_dependencies` take
   `file`, `get_impact` takes `files` — the Claude agent's first `get_impact`
   call failed on `file` and needed a retry. With `deny_unknown_fields`, an
   additive optional `file` alias would absorb this.
4. **`get_impact` declares-edge inflation — agent-identified product flaw**:
   depth≥2 BFS routes through the `sm2/mod.rs` declares edge and "pulls in
   every module sibling," reporting 56 impacted files where the honest radius
   is the depth-1 set plus behavioral-contract consumers. The agent worked
   around it manually. Candidate fix: exclude `declares` edges from impact
   traversal (mirroring their exclusion from cycles/orphans), or tag
   entries with the edge type that admitted them.

## Verdict

The original field finding's three specific complaints — MCP bypass,
grep-instead-of-get_callers, raw-pack reads — **did not reproduce** on
either vendor with the integrated stack, on the original repo, on a real
task. The tools were consumed early, repeatedly, cross-repo, and cited as
evidence in deliverables. The residual gaps are now small and named:
brief's front door (`--task`, side effect), get_impact's param asymmetry
and declares-inflation, and the pre-expansion resolver map split.

## Caveats

One run per vendor; scorer built the stack (mitigations: Codex trace is
ground-truth `--json`; Claude log count matches harness 32/32; unfavorable
findings reported, including the four friction items). The task names
RepoLens as available — same posture as every run since v3-codex ("surface
one command away; skipping is a free choice"), so this measures
available-tool adoption, not spontaneous discovery. Old-pack metric baseline
predates ~3 days of gm-crypto-rs commits; growth percentages are approximate.
Dogfood residue: two new packs under gm-crypto-rs/.repolens (consistent with
prior eval-era packs) and the gitignored brief artifact noted above.

> Case-level controlled dogfood evidence, **not** product-level proof.
