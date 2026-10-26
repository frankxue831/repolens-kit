# First Workflow-Closure Dogfood Snapshot — PR #26

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-20. Preserved as the authoritative record of its topic as of that date.

**Date:** 2026-04-20
**PR:** repolens-rs#26 — `feat(cli): default --root to cwd for scan + wiki commands` + `fix(cli): infer enclosing git worktree root when --root is omitted`
**Pack:** `.repolens/pack-20260420-051102` (full scan)
**Commit scanned:** `8e92a17` (post-merge `main`)

This is the first PR exercised against the workflow-closure phase-1 snapshot rule defined in [docs/superpowers/specs/2026-04-20-workflow-closure-phase-1-design.md](../../superpowers/specs/2026-04-20-workflow-closure-phase-1-design.md). Done by hand. The point is to *produce friction evidence for the helper-plus-checklist design*, not to ship a polished routine.

---

## Mechanical Snapshot

| Field | Value | Source |
|---|---|---|
| Files walked | 326 | `repolens scan` stdout |
| Files included | 297 | `repolens scan` stdout |
| Excluded rows | 29 | `repolens scan` stdout |
| Pack size | 3.9 M | `du -sh .repolens/pack-…` (separate command) |
| Largest chunk | `files-0001.json` (352 K) | `du -h .repolens/pack-…/*.json` |
| Scan duration | 0.80 s (self-reported) / 1.75 s (wall, includes binary startup) | scan stdout / `time` |
| Schema version | 1.3.0 | `manifest.json` |
| Worktree dirty | `true` | `manifest.json` (untracked opinion-pool files) |

### Notable Profile Entry Points

From `project-profile.json`:

- `crates/repolens-cli/src/main.rs` — Rust binary (`[[bin]]` target)
- `scripts/sanitize-fixtures.py` — Python entry point

Fixture-shaped paths (`fixtures/inputs/**`) are correctly suppressed (the recent fix mentioned in doc 07 is holding).

### Health Signals

- 2 dependency cycles (5 files, largest cycle = 3) — severity `low`
- 206 orphan files (config, docs, top-level metadata) — severity `low`
- Test-to-source ratio: 0.15

---

## Eval Questions — Answerability from this Scan

| # | Question | Answerable from RepoLens output alone? |
|---|---|---|
| 1 | What is this repo, in practical terms? | **Partial.** `project-profile.json` carries name/type/build/modules/languages, but the CLI doesn't synthesize a single human-readable summary. Requires reading JSON or invoking `get_summary` over MCP. |
| 2 | Where should I start to change a specific area? | **Partial.** Entry points + module list + graph data are all in the pack, but cross-referencing them is manual. No "where do I start for X?" command exists. |
| 3 | What prior memory or convention matters? | **No, from CLI alone.** Memory graph lives in `.repolens/memory/memory.json` and is surfaced through the MCP `get_summary` tier-aware path (per doc 07's "shipped" notes). Not visible from `repolens scan` output. |
| 4 | What are the main structural risks right now? | **Yes.** `project-profile.json` `health.signals` directly answers this — cycles + orphans called out by name and severity. |
| 5 | What changed between the latest two packs? | **No.** No CLI equivalent of MCP `compare_summaries`. A contributor producing the snapshot must either skip this question or run an MCP client just to answer it. |

**Score: 1.5 / 5 directly answerable from the CLI scan output.** The other 3.5 need either MCP or manual JSON inspection.

---

## Friction Observations (Inputs for the Helper Plan)

### F1 — Pack name regression: workspace scanned, but project named after a member crate

`project-profile.json` reports `"name": "repolens-cli"` even though the scan covered the entire `repolens-rs` workspace from the workspace root. That mismatch is worth logging, but this note should not attribute it to a "first or nearest `Cargo.toml`" rule: `ProfileAnalyzer::detect_name` currently prefers a root `[package]`, then binary crates, then libraries, and this workspace root only exposes `[workspace]` / `[workspace.package]` metadata.

Treat this as an open naming/product question surfaced by the dogfood pass, not proof that the detector is accidentally selecting an arbitrary member crate. Logging it for separate investigation; not blocking the helper design.

### F2 — Pack size requires a second command

The snapshot rule asks for "pack size" but the `scan` command does not print it. A contributor has to know to `du -sh` the pack directory afterward. Helper script is the obvious place to fix this.

### F3 — Pack absolute path is printed, but exploration assumes JSON layout

Once the pack is written, answering eval questions 1–4 means reading `project-profile.json`, `manifest.json`, `graph.json`, and `files-NNNN.json` directly. There is no `repolens summarize <pack>` or `repolens snapshot` subcommand that emits the snapshot fields plus a human-readable digest in one go.

### F4 — Eval question 5 has no CLI route

`compare_summaries` is MCP-only. If we want PR snapshots to actually answer "what changed between latest two packs?", either the helper has to drive an MCP client (heavy), or `repolens compare-summaries` needs to exist as a CLI subcommand (small, but a product surface change).

For phase-1's snapshot purposes, the pragmatic answer is: drop question 5 from the *manual* snapshot and only require it once a helper exists that can call MCP. Worth flagging in the helper plan.

### F5 — Scan duration is reproducible enough; wall time is not

Self-reported `0.80 s` was stable across two test runs to within ~10%. Wall-clock `time` swung 1.5–2.0 s due to binary startup variance. The helper should record the self-reported number, not wall time, and the design doc should say so explicitly.

### F6 — `worktreeDirty: true` is in `manifest.json` but not in `scan` stdout

A contributor producing a snapshot from a clean checkout vs. a dirty one is in materially different territory. The flag exists in the manifest but isn't surfaced by the CLI — easy to miss.

### F7 — `cd` for nested-dir testing leaks into subsequent shell calls

Procedural, not a product issue: when verifying gix-discovery I `cd`'d into `crates/repolens-cli` to run `repolens scan`, and the next shell invocation in the same workflow inherited the new pwd. Not a snapshot field, but worth noting that any future helper script should always operate with absolute paths so contributors don't trip on this.

---

## Things That Worked

- **gix-discovery (the actual PR #26 change)** — running `repolens scan` from `crates/repolens-cli/` correctly resolved up to the repo root and produced a full 326-file pack. The ergonomic improvement is real and observable.
- **Default `--keep 3` cleanup** — fired automatically; no prompt, no surprise. Old pack from 2026-04-18 was retained, so the trail wasn't lost.
- **Fixture entry-point suppression** — `fixtures/inputs/**` correctly absent from `entryPoints`. Doc 07's "shipped" claim verified.
- **Health signals** — `health.signals` array is structured and self-explanatory; this is the closest thing the pack has to a one-glance answer to "is anything wrong?".

---

## Implications for the Helper-Plus-Checklist Plan

Resolving the three open design questions from opinion_pool_008.md §4:

1. **Output contract.** This pass needed *both* a markdown table (for the PR body) *and* a way to inspect the pack JSON. A helper that just emits markdown is fine; the JSON is already on disk.
2. **Run surface.** Lean toward `scripts/dogfood-snapshot.sh` for v1 — F2/F3/F6 can all be handled by a shell wrapper without touching the binary. Promote to a subcommand only if multiple PRs prove the wrapper insufficient.
3. **Reproducibility floor.** F5 settles this: report the self-reported `scan` duration, not wall time. Document the choice in the helper script's header comment.

**Net evidence for the helper's required scope:** read scan stdout, `du -sh` the pack, `jq` a few fields out of `project-profile.json` and `manifest.json`, emit a markdown table. That's a ~30-line shell script. The qualitative half (eval questions 1–5) stays manual.

**Out of scope for the helper, but flagged here:**
- F1 (workspace-name regression) — separate bug.
- F4 (compare_summaries CLI route) — separate product question; defer.

---

## Bottom Line

One real PR's worth of snapshot work surfaced one product bug (F1), one missing CLI route (F4), and four ergonomic gaps a small shell script would fix (F2, F3, F5, F6). That is exactly the evidence opinion_pool_008 said we should wait for before writing the helper plan. The next plan can now be written with concrete shape, not speculation.
