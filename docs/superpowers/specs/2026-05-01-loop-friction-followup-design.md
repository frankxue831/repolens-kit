# Loop Friction Follow-up Design

**Date:** 2026-05-01
**Status:** Approved
**Scope:** Close three concrete gaps the agent-eval loop has surfaced over its first two iterations, and run iteration 3 as the first genuine `/loop`-driven session that validates the loop as the original spec designed it.
**Depends on:** [2026-04-27-agent-eval-loop-design.md](./2026-04-27-agent-eval-loop-design.md)

## Context

The agent-eval loop spec ([2026-04-27-agent-eval-loop-design.md](./2026-04-27-agent-eval-loop-design.md)) and its implementation plan shipped via PR #40 on 2026-04-27. Two real iterations followed: PR #41 (iter-01-abde809, 2026-04-27) and PR #42 (iter1, 2026-05-01). Both passed FAC-001..004 via `mcp_primary` and cited FAC-005 known_fail correctly.

Both iterations also recorded `claude_code_loop_invocation: external (Claude Code session driven by user prompt; not /loop)` — the spec's design trigger is `/loop`, but no run has actually used it. Both flagged the same two friction candidates with no new signal:

- **`cargo-target-pollutes-scan`** — `CARGO_TARGET_DIR=.cargo-target` (per CLAUDE.md "Parallel agent sessions" guidance) is not in root `.gitignore`, so 1800+ build artifacts inflate `analyze_health.orphanCount`.
- **`mcp-invocation-channel-undefined`** — neither `.claude/settings.local.json` MCP registration nor `scripts/dev/mcp-call.sh` exists, so each iteration improvises.

Per the original spec's stop conditions ("All FAC cases produced stable observed status across two consecutive iterations with no new friction"), running iter 3 without closing these gaps would re-record the same friction. Act on the friction first, then iterate.

## Section 1 — Goal and Boundary

**In scope:**
- One-line `.gitignore` fix (`.cargo-target/`)
- `scripts/dev/mcp-call.sh` + hermetic test harness
- `docs/superpowers/loop-prompts/agent-eval.md` (the missing prompt body)
- `docs/superpowers/loop-journal/README.md` (schema + channel docs + operator bootstrap)
- A committed copy of this design + plan

**Out of scope (deferred):**
- Auto-commit of small fixes (Option C from original spec — explicitly v2)
- `/schedule`-driven cadence (separate decision after iter 3 lands)
- Promoting any current friction candidate to a real FAC case
- Trend analysis across runs

The boundary is tight on purpose: make the loop self-contained and reproducible *before* iterating again, not pile new features onto an unreproducible base.

## Section 2 — Architecture

```
Phase 0  Branch + baseline (cargo fmt/clippy/test green on main)
   │
Phase 1  Friction A: .cargo-target/ → root .gitignore
   │     ├─ Why one line: RulesEngine has no hardcoded defaults; it
   │     │  composes .gitignore + .reposnapshot.rules. Adding the
   │     │  pattern to .gitignore propagates through discovery,
   │     │  graph build, and HealthAnalyzer with zero code touch.
   │     └─ HealthAnalyzer trusts the scan's included files (no
   │        post-hoc filter) → fixing exclusion at the source.
   │
Phase 2  Friction B: scripts/dev/mcp-call.sh + test harness
   │     ├─ Bash wrapper around `python3 -c` heredoc, matching the
   │     │  iter-2 inline implementation that already worked.
   │     ├─ Spec decision #9 authorizes this as canonical fallback;
   │     │  two iterations couldn't reach native → promote fallback.
   │     └─ Hermetic test: build → scan → discover_packs → assert
   │        latestPack present.
   │
Phase 3  docs/superpowers/loop-prompts/agent-eval.md
   │     ├─ Materializes the spec's "Loop Prompt Shape" sketch
   │     │  (lines 166-208) into the runnable prompt body.
   │     └─ References mcp-call.sh as canonical fallback channel.
   │
Phase 4  docs/superpowers/loop-journal/README.md
   │     └─ YAML schema, surface classification, three invocation
   │        channels, operator bootstrap recipe.
   │
Phase 5  Verification: iter 3 as first /loop-driven session
         ├─ PR + merge phases 0-4
         ├─ Bootstrap loop worktree (../repolens-rs-loop)
         ├─ Invoke /loop docs/superpowers/loop-prompts/agent-eval.md
         └─ Assert journal records:
            • claude_code_loop_invocation begins with "/loop"
            • mcp_invocation_method ∈ {one-shot-script, native}
            • FAC-004 orphan count ≪ iter1/iter2's 1821
```

**Key architectural decisions:**

1. **Project-wide gitignore vs loop-local `CARGO_TARGET_DIR`:** project-wide. CLAUDE.md *recommends* `.cargo-target` for parallel sessions; without the gitignore entry, anyone following that guidance hits the same friction.
2. **Committed script vs maintainer-local docs:** committed script. Both prior iterations had to inline the JSON-RPC dance. A committed helper makes the loop reproducible across machines without per-maintainer setup.
3. **Single-PR (phases 0-4) vs split-PR:** single PR. The four artifacts are interdependent. Iter 3 is its own PR, following the per-iteration-PR convention from PRs #41, #42.

## Section 3 — Component Contracts

**3.1 — `.gitignore` change**
- Interface: declarative file.
- Contract: after the change, `cargo run -- scan --root .` produces a pack whose `files-*.json` chunks contain zero entries with paths under `.cargo-target/`, and whose `profile.json:graphHealth.orphanFiles[]` contains zero `.cargo-target/...` entries.
- Reuses: existing `RulesEngine::load` ([crates/repolens-core/src/rules.rs:58-71](../../../crates/repolens-core/src/rules.rs)) and `FileDiscovery::discover` ([crates/repolens-core/src/discovery.rs:49-82](../../../crates/repolens-core/src/discovery.rs)) — no code change.

**3.2 — `scripts/dev/mcp-call.sh`**
- Interface: `mcp-call.sh <tool-name> <json-args>`.
- Contract: resolves repo root via `git rev-parse --show-toplevel`; prefers a pre-built binary at `$CARGO_TARGET_DIR/{debug,release}/repolens` over `cargo run --quiet`; drives `initialize` → `notifications/initialized` → `tools/call` → close stdin; stdout = text content of tool response; stderr = build noise + RPC errors; exit 0 on success, 1 on RPC error.
- Reuses: style template [scripts/dogfood-snapshot.sh:1-29](../../../scripts/dogfood-snapshot.sh); Python pattern from iter-2 inline impl; MCP entry point [crates/repolens-cli/src/main.rs:249-253](../../../crates/repolens-cli/src/main.rs).

**3.3 — `scripts/dev/test-mcp-call.sh`**
- Interface: no args, run from repo root.
- Contract: builds debug binary, runs scan, calls `mcp-call.sh discover_packs '{"root":"."}'`, asserts `latestPack` field present; exits 0 with `[test-mcp-call] PASS` on stderr; non-zero on any failure.
- Reuses: same style template as 3.2.

**3.4 — `docs/superpowers/loop-prompts/agent-eval.md`**
- Interface: markdown file invoked as `/loop docs/superpowers/loop-prompts/agent-eval.md`.
- Contract: contains pre-flight checks, iteration body (refresh pack, per-FAC probe MCP-first → fallbacks, render contract markdown, append journal entry, append friction candidate if new), hard guardrails (write surface allowlist + denylist), stop conditions.
- Reuses: spec sketch [2026-04-27-agent-eval-loop-design.md:166-208](./2026-04-27-agent-eval-loop-design.md); FAC manifest [first-agent-cases.json](../evals/first-agent-cases.json); output shape scripts/dogfood-eval.sh:41-56; proven journal shape 2026-05-01-iter1.md.

**3.5 — `docs/superpowers/loop-journal/README.md`**
- Interface: documentation, no runtime contract.
- Contract: file-type taxonomy, full YAML run-metadata schema, three `mcp_invocation_method` channels (with sample `.claude/settings.local.json` for `native`), surface classification table, operator bootstrap (`git worktree add ../repolens-rs-loop loop/agent-eval`).
- Reuses: schema verbatim from spec's "Required run-metadata header"; sample registration matches MCP entry point.

## Section 4 — Verification, Risks, Handoff

**Verification — friction-followup PR (phases 0-4):**
1. `git ls-files .gitignore | xargs grep -c '\.cargo-target'` returns ≥ 1.
2. `bash scripts/dev/test-mcp-call.sh` exits 0 with `[test-mcp-call] PASS`.
3. `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` all pass.
4. `bash scripts/test-dogfood-eval.sh` still passes.
5. The four new artifacts exist and reference each other correctly.

**Verification — loop-as-designed (Phase 5):**
6. Iter-3 journal records `claude_code_loop_invocation:` starting with `/loop` (NOT `external`). Single most important signal.
7. `mcp_invocation_method:` is `one-shot-script` or `native` (NOT `unavailable`).
8. FAC-004 orphan count substantially smaller than 1821.
9. `friction-candidates.md` removes or marks resolved both `cargo-target-pollutes-scan` and `mcp-invocation-channel-undefined`.

**Risks:**

| Risk | Likelihood | Mitigation |
|---|---|---|
| Native MCP registration fails on the user's machine | Medium | Committed script is canonical fallback. |
| `python3` not on PATH | Low | Repo already requires it for `sanitize-fixtures.py`; document in README. |
| `loop/agent-eval` branch drifted on remote | Medium | Phase 5 explicitly rebases on `origin/main`. |
| Iter 3 surfaces new recurring friction | Medium-High | That's the *point* of the loop — captured, user decides. |

**Handoff:** Implementation plan at `../plans/2026-05-01-loop-friction-followup.md`. Execution via `superpowers:executing-plans` (inline) or `superpowers:subagent-driven-development` (subagents).
