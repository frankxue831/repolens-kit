# Demo Case Study — Adding a Small CLI Feature With and Without RepoLens

> **Task:** Add a `--dry-run` flag to `repolens scan` that walks the repo, reports what *would* be written, but does not produce a pack on disk.
>
> **Format:** Two parallel transcripts on the same task. The "before" transcript is reconstructed from real friction patterns observed in the FAC eval runs ([`docs/philosophy/notes/2026-04-22-first-agent-eval-run.md`](philosophy/notes/2026-04-22-first-agent-eval-run.md), [`2026-04-23-first-agent-eval-baseline.md`](philosophy/notes/2026-04-23-first-agent-eval-baseline.md)). The "after" transcript shows the v0.1 target workflow.
>
> **Honesty caveat.** `repolens brief --task` is **shipped (Round 3, workspace-only)**: it composes `.repolens/` files deterministically without calling MCP or an LLM. `repolens remember --file` is **shipped (Round 4, workspace-markdown only)**: it records unverified handoffs in `.repolens/session-memory.md` and does not write the typed memory graph. `repolens validate-plan` in the "after" transcript remains **planned for v0.1**.

---

## Setup (same for both)

- Repo: `repolens-rs` at HEAD.
- Agent: a fresh coding-agent session, no prior memory of this codebase.
- Human request: "Please add a `--dry-run` flag to `repolens scan` that does everything a normal scan does — discovery, extraction, graph building — but doesn't write the pack to disk. Print a summary of what would have been written."

The repo's relevant signals (the agent has to find these somehow):

- CLI is at `crates/repolens-cli/src/main.rs`; subcommand types via `clap`.
- Scan service is `repolens_app::scan::ScanService`.
- Pack write happens in `crates/repolens-core/src/pack_writer.rs`.
- Convention from `CLAUDE.md`: atomic file I/O via temp + rename. Implication: any "would-write" report should pass through the same code path, just stub the final rename.
- Snapshot tests via `insta`. `cargo insta review` to accept changes.
- Tests live next to scan, e.g., `crates/repolens-core/tests/discovery_test.rs`.
- A historical `failed_attempt` memory: a previous agent tried to add scan flags via env vars, broke parallel test runs, was rolled back. (Imagined for the demo, but representative of friction patterns we have actually seen.)

---

## Before — agent without RepoLens

### Minute 0–3: cold start

> **Agent:** "Let me look around to understand this repo."
>
> _Runs `ls`, `cat README.md` → no README exists. (Verified: pre-Round-1, README.md was missing.)_
>
> _Runs `find . -name 'Cargo.toml'`, `cat Cargo.toml`._

**Friction:** No README. The agent has to infer the project shape from `Cargo.toml` and crate names. `time_to_useful_context` clock starts.

### Minute 3–7: blind grep for the CLI

> _Runs `rg -n 'fn main' crates/`, `rg -n '#\[derive\(Subcommand\)\]'`, `rg -n 'scan'`._

The agent finds `crates/repolens-cli/src/main.rs` after several greps. **`grep_fallback_count = 4`** so far.

### Minute 7–12: misreads the entry point

> **Agent:** "I see `repolens-app/src/scan.rs` has a `ScanService`. I'll add the `--dry-run` flag there as a struct field."
>
> _Edits `ScanService::new` to accept a `dry_run: bool`._

**Wrong file:** the flag belongs in the CLI args, then plumbed into the service. **`wrong_file_attempts = 1`.** The agent will catch this in the next pass, but it's already wasted edits.

### Minute 12–18: writes a fragile plan

> **Plan:**
> 1. Add `dry_run` to `ScanArgs` in `main.rs`.
> 2. Pass it as an env var into `ScanService` so existing callers don't break.
> 3. In `pack_writer.rs`, if `DRY_RUN=1`, print the manifest JSON to stdout instead of writing.

**Constraint miss #1:** the plan repeats a known-failed approach (env vars for scan flags) — the historical `failed_attempt` is not surfaced anywhere. The agent has no way to know.

**Constraint miss #2:** "Print to stdout" violates the camelCase JSON contract guarantee — and worse, it conflates the "would-write" payload with the actual MCP envelope shape. The agent has not read `CLAUDE.md`.

### Minute 18–35: edits, breaks tests

> _Implements the plan as written. Runs `cargo test`. 4 failures: `pack_writer` snapshot tests, scan integration tests, parallel test that asserts no env-var leakage._

**Friction:** the agent now has to debug and undo. `human_intervention_count = 1` ("the human asks: did you read CLAUDE.md?").

### Minute 35–55: half-fixes, half-ships

> _Reverts env var to a struct field. Patches one snapshot. Skips `cargo insta review` and edits the snapshot file directly. Some tests still fail._

**Constraint miss #3:** Edited a snapshot directly instead of running `cargo insta review`. This is in `AGENTS.md` but the agent never read it.

### Minute 55: weak handoff

> **Agent's session summary** (transcribed verbatim from a typical session-end output):
>
> > "Added a `--dry-run` flag to scan. Some tests are failing but I think they're flaky. Should be straightforward to finish."

**Handoff quality:** poor. Specifics: which tests? Which files were touched? What was the plan, and what changed? `session_handoff_quality = cold-start` for the next agent.

### Tally — Before

| Metric | Value |
|---|---|
| `time_to_useful_context` | ~7 min |
| `grep_fallback_count` | ≥ 4 |
| `wrong_file_attempts` | 1 |
| `repeated_discovery_count` | 2 |
| `clarification_count` | 1 |
| `known_constraint_misses` | 3 (env-var anti-pattern, camelCase contract, snapshot-edit) |
| `first_plan_quality` | major-rework |
| `human_intervention_count` | 1 |
| `session_handoff_quality` | cold-start |

The change is half-shipped, two constraints are violated, and the next session will have to start over.

---

## After — agent with RepoLens (v0.1 target)

### Minute 0: brief generated

The agent is told: "Before you start, run `repolens brief --task 'add --dry-run flag to scan'`."  [^1]

[^1]: **Shipped (Round 3, workspace-only):** `repolens brief --task` composes `.repolens/` workspace files into the brief deterministically — no LLM, no MCP, no scan pack required. For richer pack-derived context, an agent can additionally call MCP `get_summary` (tier `standard`), `get_conventions`, and `recall_longterm` on top of the brief.

The brief at `.repolens/current-agent-brief.md` arrives in seconds. Excerpt:

```markdown
# Current Agent Brief

Task: Add a --dry-run flag to repolens scan that does discovery,
extraction, and graph building, but does not write the pack to disk.

## 2. Relevant files
- crates/repolens-cli/src/main.rs        (ScanArgs lives here)
- crates/repolens-app/src/scan.rs        (ScanService)
- crates/repolens-core/src/pack_writer.rs (final write step)

## 3. Conventions to respect
- Atomic file I/O: temp + rename (CLAUDE.md "Key Patterns")
- All JSON output uses camelCase (Java contract parity)
- Snapshot tests via insta; review with `cargo insta review`

## 5. Prior failed attempts
- F-2025-XX-XX: scan flags via env vars — rejected, broke parallel runs.
  Invalidates when test_parallel_scan is reworked.

## 6. Relevant tests
- crates/repolens-core/tests/discovery_test.rs
- pack-writer snapshot tests under crates/repolens-core/tests/snapshots/

## 8. Risks
- pack_writer is on the atomic-write path; a dry-run that early-returns
  inside pack_writer must not skip the assertion that the path was valid.

## 9. Acceptance criteria
- repolens scan --dry-run prints a summary, exits 0, no pack on disk.
- All existing tests still pass.
- New test asserts no .repolens/pack-* directory created with --dry-run.
- No constraint violations from validate-plan.
```

**`time_to_useful_context` = 30 sec.**

### Minute 1: plan

> **Agent's plan:**
> 1. Add `dry_run: bool` to `ScanArgs` (clap `#[arg(long)]`) in `main.rs`.
> 2. Plumb through `ScanService::new` as a struct field. (NOT via env vars — F-2025-XX-XX warned against this.)
> 3. In `PackWriter::write`, branch early: if `dry_run`, build the manifest in memory and return `WrittenSummary` without touching disk; do not skip path validation.
> 4. Add an integration test asserting no pack directory is created.
> 5. Update one snapshot test (the manifest preview); review with `cargo insta review`, not by hand.

### Minute 2: validate-plan

```sh
$ repolens validate-plan --file plan.md     # planned v0.1
plan.md: 0 violations against .repolens/known-constraints.md
plan.md: 1 prior-attempt match — F-2025-XX-XX (this plan AVOIDS the failed approach)
```

[^2]: **Today's shipped equivalent:** read `known-constraints.md` and `failed_attempt` memories by hand; tick each plan step against them.

### Minute 3–25: edits, tests, snapshot review

> _Edits the three files, runs `cargo test`. One snapshot diff. Runs `cargo insta review`, accepts the change, explains the diff in the session summary._

All tests pass. **`known_constraint_misses = 0`. `wrong_file_attempts = 0`.**

### Minute 25–28: handoff

```sh
$ repolens remember --file session-summary.md
Session summary recorded in .repolens/session-memory.md
Next: run repolens brief --task "<next task>" to include this handoff in the next preflight brief.
```

[^3]: **Round 4 shipped behavior:** this command captures a workspace-markdown handoff only. It marks the imported content as agent-provided and unverified; it does not ingest typed memories. Use MCP `memorize_insight` separately for anchored typed memory.

The session summary itself (excerpt):

```markdown
## Session 2026-04-25T... — add --dry-run to scan

Outcome: shipped.

What I did:
- crates/repolens-cli/src/main.rs:NN-MM — added --dry-run to ScanArgs.
- crates/repolens-app/src/scan.rs:NN-MM — plumbed dry_run into ScanService.
- crates/repolens-core/src/pack_writer.rs:NN-MM — early-return on dry_run.
- crates/repolens-core/tests/scan_dry_run_test.rs — new integration test.

Friction findings:
- F-...: pack_writer's path-validation step is duplicated between dry-run
  and normal-write code paths. Tag: code-duplication. Suggested fix: extract
  a `validate_target` helper. Marked hypothesis until reviewed.

Constraints checked:
- camelCase contract: ✓ (no JSON shape changed)
- atomic I/O: ✓ (dry_run does not bypass; it returns before write)
- failed_attempt F-2025-XX-XX (env vars): ✓ (avoided)

Next agent should know:
- The `WrittenSummary` struct was reused for dry-run output. If anyone
  later changes its shape, BOTH paths must update.
```

### Tally — After

| Metric | Value | Δ vs Before |
|---|---|---|
| `time_to_useful_context` | ~30 sec | -90% |
| `grep_fallback_count` | 0 | -100% |
| `wrong_file_attempts` | 0 | -100% |
| `repeated_discovery_count` | 0 | -100% |
| `clarification_count` | 0 | -100% |
| `known_constraint_misses` | 0 | -100% |
| `first_plan_quality` | passes-validation | major-rework → passes |
| `human_intervention_count` | 0 | -1 |
| `session_handoff_quality` | next-agent-resumes-cleanly | cold-start → clean |

---

## Expected improvements (claims to test)

These are the deltas the case study suggests RepoLens should produce. They are **claims**, not measured results. The full ACE eval suite ([`docs/evals/agent-context-evals.md`](evals/agent-context-evals.md)) is what would actually measure them.

- **Time-to-useful-context: 5–15× faster** when a brief is generated up-front.
- **Constraint adherence: substantially higher**, especially for unwritten conventions that live in `CLAUDE.md` / `AGENTS.md`.
- **Repeat-failure avoidance: meaningful only if `failed_attempt` memories exist** — needs prior session investment.
- **Handoff quality: structurally improved** because the template forces the agent to write the things the next session needs.

## Remaining limitations

- **The brief is only as good as the memory store.** Day-1 briefs on a fresh `.repolens/` will look more like the "before" transcript. Value compounds as memory accumulates.
- **`repolens brief --task` is shipped but workspace-only.** It does not yet read the scan pack or call MCP tools. `repolens remember --file` is shipped but workspace-markdown only; it does not ingest typed memory. `repolens validate-plan` remains unimplemented, so today's path is to check plans against constraints by hand.
- **`failed_attempt` and `friction_finding` memory types are planned, not shipped.** Until they land, there is no clean way to write or query them.
- **No automated metrics collection.** `time_to_useful_context` etc. are still manual; this case study is not a benchmark, it is an illustration.
- **Single-repo memory.** Cross-repo learnings (e.g., "we always do X in this org") are out of scope for v0.1.
- **Confidence drift over time.** Even with the schema, a `verified_fact` whose anchor file is later deleted is only caught at recall — not proactively. On-scan grounding is a v0.2+ goal.

## See also

- [`README.md`](../README.md) — product overview.
- [`docs/agent-protocol.md`](agent-protocol.md) — the protocol both transcripts implicitly follow (or fail).
- [`docs/memory-safety.md`](memory-safety.md) — schema referenced by the "after" handoff.
- [`docs/evals/agent-context-evals.md`](evals/agent-context-evals.md) — eval suite that would measure the deltas claimed above.
- [`docs/templates/repolens/`](templates/repolens/) — templates the "after" agent fills in.
