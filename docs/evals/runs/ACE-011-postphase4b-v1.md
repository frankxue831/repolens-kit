# ACE-011 — Evidence finding integration (run `postphase4b-v1`)

> Sanitized archive of a controlled dogfood run. Original per-run scaffold
> lived at `.repolens/eval-runs/ACE-011-postphase4b-v1/` in the real
> `repolens-rs` checkout.
>
> **This is controlled case-level dogfood evidence, not product-level proof.**

## Case

ACE-011 — Evidence finding integration (calibrated MCP consumption) (see
[`docs/evals/agent-context-evals.md`](../agent-context-evals.md) for the
canonical case definition). ACE-011 is the MCP-consumption counterpart to
ACE-002's workspace-markdown constraint binding — it tests whether the
calibrated MCP-tool surface (the surface Phase 4B narrowed to "calibrated
findings only") binds an agent's first plan on an open-ended task.

## Run id

`postphase4b-v1` — establishing run. ACE-011 is new in this PR; there is
no prior baseline for direct comparison.

## Why this run

[`ACE-002-postphase4b-v1`](ACE-002-postphase4b-v1.md) surfaced a
structural gap in the ACE suite: cases 001–010 test workspace-markdown,
memory, and handoff surfaces, none of which Phase 4B changed. To close
the Phase 4B measurement loop, the suite needs a case that requires
MCP-tool consumption. ACE-011 is that case. This run is the case's first
exercise, against the same `repolens-rs` HEAD (`afd7a6b`) used for
`ACE-002-postphase4b-v1` so the two runs are directly comparable.

## Repo / binary provenance

- **Source repo at setup.** `feat/ace-011-evidence-finding-integration`
  branched from `main` at
  `afd7a6b Merge pull request #62 from frankxue831/fix/ci-include-cli-doc-inputs`.
  ACE-011 case body and three test updates were in-tree at run time;
  commits land after the run is scored.
- **Workspace under test.** Real repo in place — `.repolens/` and the
  active pack (`pack-20260516-015206`) were available to the agent.
- **Scaffold provenance.** `generated_by: repolens@0.1.0`,
  `template_set: ace-eval@round-5.2`,
  `scorecard_schema: repolens-scorecard@round-5.2`.
- **Source commit in scaffold.** `afd7a6b7bda07d54c1dad6032baffd1ceaa2848a`.

## Setup summary

1. Verified the active pack surfaces calibrated evidence findings:
   `scripts/dev/mcp-call.sh get_summary '{"root":".","detail":"full"}'`
   returned 4 findings (`F-ORPHAN-FILES`, `F-HIGH-FAN-IN-FILE`,
   `F-CARGO-LOCK-STATE`, `F-DEPENDENCY-CYCLES`). ACE-011 Setup
   precondition satisfied.
2. Phase 4B fully shipped (Steps 1–4 + calibration coherence
   `00b5f6b`). The MCP-tool surface emits only calibrated evidence
   findings.
3. Scaffolded the run with
   `repolens eval start --case ACE-011 --run-id postphase4b-v1`.
4. Dispatched an independent unprimed subagent (general-purpose, via
   the Agent tool) with the open-ended task and an enumeration of
   available RepoLens surfaces, including the MCP one-shot helper
   `scripts/dev/mcp-call.sh` with examples and 14 relevant tool names.
   No prior framing about evidence findings, `F-*` ids, Phase 4B, or
   this being an eval was leaked.

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
| MCP / brief consumption | Subagent's free choice |
| Phase 4B retirements | Fully shipped |

## Scorecard summary

| Metric | Value | Note |
|---|---:|---|
| `time_to_useful_context` | 9 cmds | `ls` → `--help` → `brief` → read brief → 3 source reads → `cat session.json` → plan drafting. |
| `repeated_discovery_count` | 0 | |
| `grep_fallback_count` | 0 | Brief + source reads were sufficient; no unstructured grep. The MCP-skip is captured under item 1 of pass criteria, not as a grep fallback. |
| `wrong_file_attempts` | 0 | All inspected files relevant to the brief composer. |
| `clarification_count` | 0 | |
| `known_constraint_miss` | 0 | Plan respects and cites C-001 / C-002 / C-004 / C-006. |
| `first_plan_quality` | `fail` | Items 1 and 2 unmet. See below. |
| `human_intervention_count` | 0 | |
| `session_handoff_quality` | `n/a` | Single-session case. |
| `stale_checkout_reconciled` | `n/a` | Case does not exercise shipped-command dogfood. |
| `memory_safety_notice_used` | `n/a` | Task did not write memory. |

**Case-specific signal:** `mcp_invoked: no` (per ACE-011 friction
metrics). Per the case body, `mcp_invoked = no` automatically resolves
item 1 to `fail`.

**Verdict:** **fail**.

## Per-criterion breakdown

| Criterion | Status | Evidence |
|---|---|---|
| 1. Consume MCP | ❌ FAIL | Subagent stated "MCP tools: not used." |
| 2. Cite calibrated MCP evidence finding by id | ❌ FAIL | Plan cites `F-2026-04-25-001` (friction-log entry, date-stamped form) and `C-001/C-002/C-004/C-006` (constraints) but no calibrated MCP evidence finding id (`F-WORD-WORD-...` form). |
| 3. Ground claims to evidence | ✅ PASS | Plan cites `brief.rs:159-287, :87-88, :147, :230-243, :519+, :797-802` plus the empirically verified pack-id discrepancy. |
| 4. Action alignment | n/a | No `F-*` calibrated finding cited; no `action` field to align with. |
| 5. Pre-flight `validate-plan` | ✅ PASS | 3 revisions to clean. |

## Observed behavior

- Subagent opened with `ls` and `cargo run -- --help`, then invoked
  `repolens brief --task "..."` to generate
  `.repolens/current-agent-brief.md`.
- Reading the brief surfaced its own staleness: the brief reports
  `pack-20260427-153301` while `.repolens/session.json:activePackPath`
  is `pack-20260516-015206`. A Round-6 session-memory entry embedded in
  the brief explicitly proposes the fix that the subagent's plan ends
  up executing.
- Three source files anchored the plan: `crates/repolens-cli/src/brief.rs`
  (pure-composer property), `crates/repolens-core/src/storage.rs` (session
  helper), `crates/repolens-cli/src/validate_plan.rs` (fenced-code
  carve-out).
- First validate-plan run surfaced 7 VP001 warnings on Rust function
  names adjacent to JSON prose. Subagent revised twice (moved
  identifiers into fenced ```rust``` blocks) to reach a clean run.
- **MCP tools were not invoked.** Subagent's stated reason: "For a
  10-bullet structural plan on a known surface, the brief + targeted
  source reads were faster than spinning up the MCP loop."
- The resulting plan is a well-formed 10-bullet implementation roadmap
  for adding `## Runtime Freshness` to the brief: read `session.json`,
  read git head, render an extra section, warn on stale `pack-*`
  references, add tests, run pre-push gate. Verification steps are
  explicit. The plan would be a useful change to ship on its own
  merits.

## Comparison to baseline

- **Compared against run:** none — this is the establishing run for
  ACE-011.
- **Cross-reference:**
  [`ACE-002-postphase4b-v1`](ACE-002-postphase4b-v1.md) is the matching
  workspace-markdown case at the same HEAD. ACE-002 passed without
  exercising MCP. ACE-011 confirms that even on an open-ended
  structural task — exactly the kind that should reward calibrated
  finding consumption — agents still do not reach for MCP.

## Friction findings

- **mcp-skipped-on-open-ended-task.** The case's central failure mode,
  caught as designed. The MCP surface was offered with a one-shot
  helper and a 14-tool enumeration. The subagent still chose
  `brief + source read`. **Even on broad tasks where calibrated
  findings would surface directly relevant data, agents skip MCP.**
- **f-prefix-collision.** The `F-` prefix is shared between calibrated
  MCP evidence findings (`F-HIGH-FAN-IN-FILE`) and friction-log entries
  (`F-2026-04-25-001`). The subagent cited the latter as if it
  satisfied item 2. **The case body was amended in the same PR as
  this run** to disambiguate: calibrated findings use named ids
  (`F-WORD-WORD-...`), friction-log entries use date-stamped ids
  (`F-YYYY-MM-DD-NNN`). This is case-evolution-driven-by-run, exactly
  how the eval suite should evolve.
- **brief-self-staleness-discoverable-from-brief.** The brief cites
  `pack-20260427-153301` from `repo-overview.md` while the actual
  active pack is `pack-20260516-015206`. The brief composer is a pure
  function of workspace markdown ([`crates/repolens-cli/src/brief.rs:147`](../../../crates/repolens-cli/src/brief.rs))
  and does not read `session.json` or git. Independent of ACE-011's
  verdict, this is a real product defect with an obvious fix.

## Limitations

- **Single run, single agent vendor.** "Agents skip MCP" is a
  one-data-point finding. It needs at least 2–3 independent runs
  (different vendor, different task framings) before it can be
  promoted to a confirmed pattern.
- **Dispatch prompt may have under-emphasized MCP.** The MCP helper
  was listed alongside `brief` and `validate-plan`; a future run
  framing MCP as "the primary surface for structural questions" might
  produce different behavior. The case body does not currently
  prescribe a specific framing, so this is a methodology question.
- **The fail verdict speaks to surface consumption, not plan
  quality.** The subagent's plan is well-formed and would be a useful
  change to ship. Promoting "plan quality" to a separate metric
  alongside "surface consumption" might be worth a future case-body
  amendment.

## Product implications

- **Phase 4B's calibrated MCP surface is unmeasured by current agent
  behavior.** Two runs (ACE-002 and ACE-011) at the same HEAD both
  resolved without invoking any MCP tool. The producer-side
  calibration work — retiring `signals` / `recommendations` /
  `fitnessScore` / `dependencyRisk` / `graphHealth.*` /
  `metrics.trend`, promoting evidence findings — does not appear in
  agent behavior because agents do not consume the surface.
- **Probable cause: invocation cost.** `bash scripts/dev/mcp-call.sh
  get_summary '{"root":".","detail":"full"}' | jq` is operationally
  heavier than `cat .repolens/current-agent-brief.md`. An agent's
  cost function selects against MCP for plan-only tasks.
- **High-leverage product fix candidates** (each independent;
  consumption ergonomics, not new analytical capability):
  1. **An agent-facing CLI wrapper that dumps findings.** Something
     like `repolens findings [--full]` that wraps `get_summary
     --detail full` and prints `evidenceFindings` as agent-readable
     markdown. Removes the "spin up MCP loop" cost while preserving
     calibration structure.
  2. **Embed evidence findings in the brief.** Brief is the default
     consumption path. Carrying a `## Evidence Findings` section pulled
     from the active pack would put calibration in front of every
     agent. This contradicts brief.rs's "pure function of workspace
     markdown" property — a deliberate architecture call.
  3. **A `repolens brief --include findings` flag.** Opt-in form of
     #2 for tasks where the agent or human anticipates
     finding-grounded planning.
- **The brief's self-staleness is a real defect** independent of
  ACE-011. The subagent's plan fixes it. Worth executing on its own
  merits.

## Recommended next changes

- **[case]** (already landed in this PR alongside the case itself)
  Disambiguate "F-* finding id" in the ACE-011 case body — must be a
  calibrated MCP evidence finding (named-id form), not a friction-log
  entry (date-stamped-id form).
- **[product]** Evaluate the `repolens findings` CLI wrapper. Cheapest
  fix candidate; delegates to `get_summary`; measurable via ACE-011
  re-run.
- **[product]** Decide on brief-vs-MCP coupling. Option (2) above
  (embed findings in brief) is the strongest agent-behavior lever but
  requires breaking the brief's purity property. Option (3) is the
  conservative path.
- **[product]** Execute the subagent's plan to add `## Runtime
  Freshness` to the brief. Independent of ACE-011's verdict — real
  defect, execution-ready plan, Round-6 friction has been open since
  2026-04-25.
- **[case]** Schedule ACE-011 follow-up runs (`postphase4b-v2`,
  `postphase4b-v3`) after each product change above lands. Track
  whether `mcp_invoked` flips from `no` to `yes`. Without follow-ups,
  product decisions on MCP ergonomics are guesswork.

## Appendix A — Full subagent plan (verbatim from `/tmp/ace011-plan.md`)

Reproduced here so the archive is the durable record of what the run
actually produced. The plan is preserved verbatim (only the outer
fenced markdown delimiter is added to render the inner backticks).

````markdown
# Plan — Stale-Workspace Reconciliation in `repolens brief`

## Problem (grounded in source + session memory)

`repolens brief` composes `.repolens/current-agent-brief.md` as a pure function of the five workspace markdown files (`crates/repolens-cli/src/brief.rs:159-287`). It never reads `.repolens/session.json` or queries git, so:

- The brief reports the pack id that a human typed into `.repolens/repo-overview.md`. Concrete miss observed today: the rendered brief shows `pack-20260427-153301`, but `.repolens/session.json:activePackPath` is `pack-20260516-015206`. About 19 days stale on a fresh scan run.
- Friction `F-2026-04-25-001` (handoff-quality) and the Round-6 session-memory entry both ask for exactly this fix: "show active pack/current HEAD and warn when rendered workspace inputs appear older than the active pack or cite older pack ids."
- Constraint `C-004` keeps `validate-plan` warnings-only; the same posture (deterministic, warnings-only) applies here.

This is high-leverage because every downstream session reads this brief first; stale pack ids and missing freshness cues silently poison the next agent's plan.

## Plan (10 bullets)

1. Add a new public reader to `crates/repolens-core/src/storage.rs`, placed next to the existing helper that returns the session file path. It loads `.repolens/session.json` and returns an optional snapshot struct; missing or unreadable file yields `None`. The wire-side keys preserve the existing camelCase contract per `C-001` and the Rust-side identifiers stay idiomatic. The exact signature and struct live in this fenced block (validate-plan ignores fenced code, which avoids unrelated VP001 adjacency warnings):

   ```rust
   // crates/repolens-core/src/storage.rs — alongside RepoStoragePaths::session_path
   pub struct SessionSnapshot {
       pub active_pack_path: Option<PathBuf>,
       pub updated_at: Option<String>,
       pub last_scan_mode: Option<String>,
       pub reprocessed_files: Option<u64>,
       pub carried_forward_files: Option<u64>,
   }
   pub fn read_session(repo_root: &Path) -> Option<SessionSnapshot> { /* ... */ }
   ```

   Every field is `#[serde(default)]` so a partial file still decodes. Deserialize via the existing crate dependency already used elsewhere in this workspace; no new dependency is added.

2. Add a git-snapshot helper in a new module under `crates/repolens-cli/src/` (filename matches the module name in its bullet-1-style fenced code block):

   ```rust
   // crates/repolens-cli/src/git_snapshot_mod.rs
   pub(crate) struct GitHeadSnapshot {
       pub branch: Option<String>,
       pub short: Option<String>, // abbreviated commit hash
       pub dirty: bool,
   }
   pub(crate) fn read_git_head_snapshot(repo_root: &Path) -> Option<GitHeadSnapshot> { /* ... */ }
   ```

   Implementation: parse `.git/HEAD` directly to recover branch name (the `ref: refs/heads/<name>` form) or detached commit hash; look up `.git/refs/heads/<name>` or `.git/packed-refs` for the full hash; treat the existence of `.git/index.lock` as a cheap disk-only dirty signal, and optionally run `git status --short` via `std::process::Command` with the repo root as working directory when the binary is on PATH. If git is absent, leave `dirty = false`. The subprocess path sits behind a `GitProbe` trait so unit tests inject a fake and the brief stays byte-deterministic.

3. In `crates/repolens-cli/src/brief.rs`, extend the existing pure composer with one new parameter `freshness: FreshnessSnapshot`. The struct holds three sub-fields: a `session` snapshot from step 1, a `git` snapshot from step 2, and a `BTreeMap` from workspace filename to its file modification time as `Option<i64>` (Unix seconds). The map ordering is alphabetical because the structure is a `BTreeMap`, preserving the determinism property asserted at `brief.rs:797-802`. The CLI entry point in `run_brief` constructs `FreshnessSnapshot` from real readers; tests pass synthetic values.

4. Render a new section `## Runtime Freshness` inserted immediately after `## Workspace Status` (so it sits above `## Repo Context`, which currently carries the stale pack id from `repo-overview.md`). Table columns: `Source | Value | Note`. Rows: active pack name (basename of `activePackPath`), session `updatedAt`, `lastScanMode`, git branch, git short sha, dirty flag. When `session` is `None`, render a single row "No `.repolens/session.json` found — run `repolens scan` first." When `git` is `None`, render "Not a git checkout or git unavailable."

5. Add a stale-input detector helper that takes the `repo-overview.md` body and the active pack basename and returns a `Vec` of warning strings. Use a precompiled `Lazy<Regex>` for the pattern `pack-(\d{8})-(\d{6})` (sibling to `TEMPLATE_PLACEHOLDER_RE` at `brief.rs:87-88`). Any captured pack name that differs from the active one becomes one warning line in the `## Runtime Freshness` section, rendered as ``Repo overview cites stale pack `pack-YYYYMMDD-HHMMSS` (active: `<active>`). Update `.repolens/repo-overview.md`.`` This catches today's concrete dogfood miss: the brief cites `pack-20260427-153301` while active is `pack-20260516-015206`.

6. Add a workspace-age helper in the same module. It takes the per-file mtime map plus the session timestamp converted to Unix seconds, and returns one warning per workspace file whose mtime predates the session by more than the grace constant `STALE_WORKSPACE_GRACE_SECS = 86_400` (i.e. 24 hours). Rendered line example under `## Runtime Freshness`: ``Workspace input `repo-overview.md` predates the active pack by 19 days; contents may be stale.`` Skip silently when either side is missing — warnings-only posture per `C-004`.

7. Append a `### Freshness` bullet to the existing `## Suggested First Checks` list at `brief.rs:230-243`: ``Compare the `## Runtime Freshness` table against `git log -1 --oneline` and `ls .repolens/pack-*` before trusting pack ids cited elsewhere in this brief.`` Keep the existing "Repo freshness checklist (do not run automatically)" intact — that's prose advice, this is new live data.

8. Tests in the existing `mod tests` block of `crates/repolens-cli/src/brief.rs` (extend at line 519+, do not replace existing cases). New cases, each using `tempfile::tempdir` and a synthetic `FreshnessSnapshot` so output stays deterministic: (a) asserts the new section header is present; (b) writes a `repo-overview.md` citing `pack-20260101-000000` while the snapshot active is `pack-20260516-015206`, then asserts the stale-pack warning line appears; (c) sets one workspace mtime two days before the session timestamp, asserts the age warning; (d) supplies no session and asserts the "No `.repolens/session.json`" row; (e) re-runs the composer twice and asserts byte-equal output, guarding the existing determinism property. Plus two unit tests for the stale-pack regex helper: one positive match, one no-false-positive on `pack-` text inside a fenced code block.

9. Add an integration test at `crates/repolens-cli/tests/brief_runtime_freshness.rs` that (i) writes a minimal `.repolens/` workspace via the embedded templates the way `crates/repolens-cli/src/init.rs` does, calling library helpers rather than starting a subprocess, (ii) writes a hand-crafted `.repolens/session.json` with a known active pack path, (iii) invokes the composer along the same code path `run_brief` uses, (iv) asserts the rendered markdown contains both the active pack basename and the stale-pack warning. The fake `GitProbe` keeps the test hermetic and deterministic regardless of host git presence.

10. Verify with the pre-push gate from `CLAUDE.md`: run formatter, then clippy with `-D warnings`, then `cargo test -p repolens-cli`. Manual dogfood: from repo root run `cargo run --quiet -- brief --task "verify freshness"` and confirm `.repolens/current-agent-brief.md` (a) contains a `## Runtime Freshness` heading, (b) lists `pack-20260516-015206` as the active pack, (c) prints a warning that the repo overview cites `pack-20260427-153301`. Document the new section briefly in `README.md` under the `repolens brief` paragraph — one sentence; no files under `docs/philosophy/`, `docs/product-canon.md`, or `docs/engineering-canon.md` are touched, so the `C-006` Doc Canon registry-sync gate stays untriggered.

## Why this is high-leverage

Brief output is the first thing every dogfood agent reads (the composer is invoked on every `repolens brief` call, `brief.rs:147`). Today's defect is observable and 19 days old in the current `.repolens/`. The fix is contained to two crates — `repolens-cli` and `repolens-core` — preserves determinism, adds no transitive dependencies, respects `C-001`/`C-002`/`C-004`/`C-006`, and removes the highest-impact source of agent context poisoning short of changing memory semantics.

## Non-goals

- Modifying `repolens remember` or the typed memory graph (`C-003` territory).
- Promoting the freshness check into a hard error or blocking exit code — stays warnings-only to match `validate-plan` posture (`C-004`).
- Touching `docs/philosophy/`, `docs/product-canon.md`, or `docs/engineering-canon.md` (`C-006` gate would require a registry update).
- Adding a network call or fetching from `origin/main`. Disk-only.
````

---

> **Reminder.** This is controlled case-level dogfood evidence, not
> product-level proof. Read it as one data point — particularly thin
> here because ACE-011 is new and this is its establishing run.
