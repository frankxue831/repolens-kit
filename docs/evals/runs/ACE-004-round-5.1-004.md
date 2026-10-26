# ACE-004 — Session continuity (run `round-5.1-004`)

> Sanitized archive of a controlled dogfood run. Original transcript and
> per-run scaffold lived in a synthetic temp `.repolens/eval-runs/` workspace
> that was discarded after the run.
>
> **This is case-level controlled dogfood evidence, not product-level proof.**

## Case

ACE-004 — Session continuity (see
[`docs/evals/agent-context-evals.md`](../agent-context-evals.md) for the
canonical case definition).

## Run id

`round-5.1-004`

## Repo / binary provenance

- **Era.** Round 5.1 — between
  `de9fe5e` feat(cli): Round 5.1 — scorecard metric definitions and accepted
  values (repolens-rs) and the
  Round 5.2 work that followed.
- **Source commit at scaffold time.** Not recorded in the run artifacts —
  Round 5.1 `repolens eval start` did not stamp a `source_commit` field.
  Round 5.2 added the `<!-- repolens:provenance -->` block precisely so
  future runs do not have this gap.
- **Binary at scaffold time.** A `target/debug/repolens` built during the
  Round 5.1 era; no provenance block, so the exact build is not
  reconstructable from the artifact.

## Setup summary

1. Synthetic temp git repo created via `mkdir`/`git init`.
2. `repolens init` — scaffolded the standard six-file `.repolens/` workspace.
3. `repolens eval start --case ACE-004 --run-id round-5.1-004`.
4. **Session A** (simulated):
   - Hand-written `session-A-summary.md` describing fictional in-progress
     work on "add `--quiet` flag to `repolens scan`".
   - The summary included an explicit `## Pick up here` block naming the
     concrete next step.
   - `repolens remember --file session-A-summary.md` — imported the
     summary into `.repolens/session-memory.md` with the standard
     `**Status:** agent-provided, unverified` and
     `**Use as:** hypothesis until confirmed by …` framing.
5. **Session B** (the run under test):
   - `repolens brief --task "Continue the --quiet flag work from previous
     session."`
   - Agent under test asked to continue without rediscovering the task.

No source files were opened; the agent produced a continuation plan only.

## Scorecard summary

| Metric | Value | Note |
|---|---:|---|
| `time_to_useful_context` | 1 turn | Imported session memory was the first thing the agent referenced. |
| `repeated_discovery_count` | 0 | |
| `grep_fallback_count` | 0 | |
| `wrong_file_attempts` | `n/a` | Plan-only run; no source-tree navigation expectation. **Originally recorded as blank under Round 5.1 wording**; Round 5.2 metric definition explicitly classifies plan-only runs as `n/a`, so this archive uses the Round 5.2 form. The original Round 5.1 blank is preserved here as a footnote of measurement-vocabulary drift. |
| `clarification_count` | 0 | |
| `known_constraint_miss` | `n/a` | Case does not assert constraints. |
| `first_plan_quality` | `pass` | Plan picked up at the explicit next step from Session A's "Pick up here" block. |
| `human_intervention_count` | 0 | |
| `session_handoff_quality` | `pass` | Next reader could identify what was done, what remained, and the next concrete step without rediscovery — meets the Round 5.2 explicit pass bar. |
| `stale_checkout_reconciled` | `n/a` | Case does not exercise shipped-CLI dogfood. |
| `memory_safety_notice_used` | `pass` | Passive pass — agent consumed prior session memory as unverified handoff context, did not upgrade it to confirmed fact, and did not strip the `agent-provided / unverified / hypothesis` framing imported by `repolens remember`. |

**Verdict:** pass.

## Observed behavior

- `repolens remember --file <session-A-summary.md>` →
  `.repolens/session-memory.md` → next `repolens brief` end-to-end flow
  works without manual stitching.
- The agent's first action in Session B was to read the imported handoff;
  the next step from Session A's "Pick up here" block was carried into
  Session B's plan verbatim.
- The imported summary surfaced inside `## Prior Session Memory` of the
  brief preserving the `agent-provided, unverified` framing.

## Findings

1. **Pass depends on Session A having written an explicit "Pick up here"
   block.** Without that block, the next agent would have had to infer
   the next step from a longer narrative, and the "1-turn" time-to-useful
   context would not have held. The Round 5.2 work added a deterministic
   "Latest 'Pick up here' (lifted from prior session memory)" lift to
   `repolens brief` so this load-bearing signal is no longer buried under
   sub-headings.
2. **TemplateOnly handling inconsistency at brief-generation time.**
   Repo Context (rendered from a pristine `repo-overview.md` template)
   was collapsed under a clear notice, while Known Constraints could
   inline raw placeholder text such as `C-001 _<short-name>_` where a
   literal-markdown reader could mistake a placeholder for a real
   constraint. The Round 5.2 work removed inlined raw template bodies
   for `TemplateOnly` sections entirely; only the warning + a reference
   to the underlying file is now emitted.
3. **Brief inlines the full imported summary.** Once a session has been
   `remember`-ed, `## Prior Session Memory` carries the entire imported
   summary verbatim. With many sessions accumulated, this section will
   start to dominate the brief. The Round 5.2 "Pick up here" lift
   surfaces the most load-bearing cue at the top, but the underlying
   "all session memory inlined" behavior is unchanged and remains worth
   revisiting.
4. **Methodological limit — single Claude process.** Session A's
   `session-A-summary.md` and Session B's continuation were both produced
   by one Claude process. The run therefore demonstrates that the
   artifacts (`session-memory.md`, `repolens brief`) are **sufficient for
   a fresh agent given identical handoff content**, more than it
   demonstrates how a genuinely cold second agent would behave. A real
   two-agent or two-vendor run is needed to make a stronger claim.

## Methodological caveats

- Single Claude process simulating both Session A and Session B (see
  Finding 4). Cross-vendor / cold-start behavior is not measured here.
- Plan-only run; the agent never opened source. Whether the handoff
  would survive integration with real code editing is not measured.
- Synthetic temp repo. Real cases against the live `repolens-rs`
  checkout would interact with a much larger brief.
- No Round 5.2 provenance block in the original scaffold — Round 5.1
  framing is the most precise statement available.

## Product implications

- **Confirms shipped:** `repolens remember --file` →
  `.repolens/session-memory.md` → `repolens brief` is a working
  end-to-end workspace handoff that preserves agent-provided /
  unverified / hypothesis framing.
- **Does not confirm:** that a genuinely cold second agent (different
  vendor, different process) would resume cleanly; that the handoff
  remains usable once `## Prior Session Memory` has accumulated many
  sessions; that the lift / framing survives once the agent starts
  editing source code, not just planning.
- **Motivated (Round 5.2):** the deterministic "Pick up here" lift in
  `repolens brief`, the TemplateOnly handling change, and the
  `memory_safety_notice_used` definition update that explicitly
  recognises remember-import framing as memory-safety framing.

---

> **Reminder.** This is case-level controlled dogfood evidence, not
> product-level proof. Read it as one data point in the eval suite, not
> as a guarantee about RepoLens behavior in the wild.
