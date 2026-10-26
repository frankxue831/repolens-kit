# 2026-06-04 — Optimization Roadmap Closeout

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-06-04. Preserved as the authoritative record of its topic as of that date.

**Scope:** retrospective on the maintainability + performance roadmap (the "v2" roadmap: slices S0,
P1, P2, M1, M2, M3, M4, W1, plus discovery-pruning and two determinism fixes). The arc is
**PRs #78–#88**, merged onto `main` between the S0 landing and `ad5c000`. This note records what
shipped, the measured numbers, and — most importantly — the slices that were **declined on evidence**,
so a future session does not re-propose them from the original plan.

**Read this if you are:** about to propose a refactor or perf change to extraction / graph build /
discovery / the MCP tool layer, or wondering why parser reuse, a Go-path index, or an
MCP-handler-boilerplate wrapper are *not* done despite being in the original roadmap.

> The plan that drove this arc is `docs/superpowers/plans/2026-06-03-parse-once-extraction.md`
> (parse-once, the largest slice). The P1 measurement readout lives at
> `crates/repolens-extract/benches/README.md` (commit `92ac2db`).

---

## Operating rules (how the arc was run)

Every slice followed the same discipline, and it is the reason all eleven PRs landed without a contract regression:

- **Evidence-gated.** No optimization shipped without a measurement behind it (P1 was built *first*,
  precisely so perf findings became evidence instead of guesses). Two slices were killed by their own
  measurements — see "Declined."
- **Small, reviewable, behavior-preserving slices.** One concern per PR; no rewrites.
- **Contracts preserved, verified byte-for-byte.** `.repolens/` pack JSON (camelCase), MCP epistemic
  envelopes, `memory.json` — unchanged across the whole arc. Each output-touching slice was proven
  byte-identical via `scripts/golden-pack-diff.sh` (java-small / rust-small / mixed-small) plus, for
  parse-once, a same-dir in-repo A/B and a thread-count-invariance check.
- **Gated green.** `scripts/gate.sh` (fmt + clippy `-D warnings` + test --workspace) on every slice.

---

## Shipped

| Slice | PR | What landed | Measured / verification |
|---|---|---|---|
| **S0** | #78 | Dedup the tree-sitter read+parse boilerplate into `ts_util::parse_source` (9 files, +66/−166). | Behavior-identical; golden-diff byte-identical. |
| **P1** | #79 | Measurement harness: `criterion` bench (variants A–D) for `extract()` + per-phase scan timing in `run_full`. **The gate for the whole perf track.** | Readout in `benches/README.md` (`92ac2db`). Scan output byte-identical (timing is logs only). |
| **P2** | #80 | Parallelize graph build (parallel extract+resolve, serial ordered merge). | `graph.json` byte-identical + thread-count invariant. |
| **M1** | #81 | Collapse 4× duplicated temp-write+rename+`to_string_pretty` into `core::atomic_write`. | TDD'd; golden-diff + contract tests. |
| (determinism) | #82 | Make `profile` `primaryLanguage` tie-break deterministic (`compute_modules`). | Scan-twice byte-identical + contract test. |
| **M4** | #83 | Split the 3048-LOC `evidence_findings.rs` god-module into a directory module (8 finding families), same public builder. | e2e + 39 inline tests moved with their family; JSON unchanged. |
| **M2** | #84 | Close the remaining MCP characterization-coverage gap (`wiki_suggest` envelope test). | New `call_*_envelope` test pins envelope fields. |
| **W1** | #85 | Codify the prose rituals: `scripts/gate.sh`, `scripts/verify-all.sh`, `scripts/golden-pack-diff.sh`, and `.claude/commands/` for `/gate`, `/verify-all`, `/golden-pack-diff`, `/bench`. | Tooling only; no prod-code diff. |
| (discovery) | #86 | Git-aligned discovery: prune ignored subtrees instead of walking them (`build_gitignores` rewrite + `should_descend_dir`). | **~1430 ms → ~19 ms** on trees with large gitignored subtrees (e.g. `target/`); fixtures byte-identical. |
| **P2(a)** | #87 | Parse-once / read-once: parse each file a single time for both symbols and relationships; hand relationships to graph build **in memory** (never serialized); graph keeps packed-copy extraction as the incremental/Python fallback. | **graph-build phase ~64 ms → ~24 ms (~60%)**; all fixtures + in-repo A/B byte-identical; thread-invariant. |
| (determinism) | #88 | `graph-metrics.json` `perLanguage` map `HashMap` → `BTreeMap`. The last run-to-run nondeterministic pack file. | Reproduced on `main` first; scan-twice now identical. |

---

## Declined on evidence (the important part)

These three were in the original plan. They are **not** oversights — each was investigated and rejected
with a specific reason. Do not re-open without first invalidating the reason given here.

### M3 — MCP handler-boilerplate wrapper — **DECLINED**
- **Plan premise:** ~750–1100 duplicated LOC across the 26 MCP tools (timing / `info!` / `err_json` /
  envelope wrapping) begging for one shared wrapper.
- **Census reality:** the duplication estimate did not survive a per-handler read. The repetition that
  exists is mostly irreducible (each tool's parameter binding and payload shape differ), and the
  4-arg `ok_epistemic` vs 5-arg `ok_epistemic_graded` split is **intentional and greppable** — it is the
  trust mechanism that forces a conscious opt-in to `verified`/`derived` grades. A wrapper that hid it
  would erode that, for a far smaller LOC win than projected.
- **Verdict:** declined. If revisited, re-census the *actual* dup LOC first and preserve the
  grade-opt-in's greppability.

### P2(b) — thread-local / reused parser — **DECLINED**
- **P1 measured it directly:** parser *allocation* (`Parser::new` + `set_language`) is **~0–0.7%** of
  per-file extract time. The decision rule written into P1 ("pursue reuse only if ≥10–15% and material")
  was not met by an order of magnitude.
- **Verdict:** rejected by the data. The real per-file win was **parse-once** (parse dominates at
  ~49–67% and the pipeline parsed each file *twice*), which #87 captured. Reuse is moot.

### P2(d) — Go module-path index — **NOT PURSUED**
- Go module resolution is `O(relationships × included_paths)`, a candidate in the plan. After parse-once
  (#87) and parallel graph build (#80) landed, it never surfaced as a material share of graph-build
  wall-clock on the measured repos.
- **Verdict:** left as a candidate, unbuilt. **Measure before building** — only justified if a real repo
  shows Go resolution dominating the (now parallel, now parse-once) graph phase.

---

## Method notes that paid off (carry into the next arc)

- **`golden-pack-diff.sh` is the byte-identical guard, and it earned its keep.** It diffs
  `project-profile.json` too, which caught a `packId` self-reference embedded there (two scans' pack-ids
  differ) — a false-positive that would have read as a regression. The script canonicalizes each scan's
  own pack-id before diffing.
- **Measurement contamination nearly killed a good slice.** An early in-repo discovery timing (~1430 ms)
  looked like parse-once was a no-go — but the number was inflated by the dev tree's `target/` +
  `.worktrees/`, which the *old* unpruned walk descended into. Re-measuring on a clean copy flipped the
  decision to GO. **Lesson: time perf on a clean checkout, not a working dev tree.** (It also turned the
  discovery walk itself into slice #86.)
- **Caught-and-isolated beats conflated.** The `graph-metrics.json` nondeterminism was discovered during
  the parse-once A/B, but it reproduced on `main` — so it was split into its own PR (#88) instead of being
  blamed on, or bundled into, parse-once. Parse-once stayed a clean byte-identical change.
- **Honest declines are findings, not failures.** M3 and parser-reuse being rejected *with reasons* is the
  most reusable output of this arc — it stops the next session from spending the same effort.

---

## What is intentionally NOT done (the boundary)

- **M3** (MCP wrapper), **parser reuse**, **Go-path index** — declined/unpursued, see above.
- **Python extraction** stays regex-based and takes the graph-build fallback path; it was explicitly out of
  parse-once scope (no tree-sitter grammar to share).
- **No pack-schema / envelope / camelCase change** anywhere in the arc — that invariant was the point.
- The roadmap's 7 named slices are exhausted. Any further perf work starts from a *fresh* P1-style
  measurement on a real repo, not from this plan's leftover candidates.
