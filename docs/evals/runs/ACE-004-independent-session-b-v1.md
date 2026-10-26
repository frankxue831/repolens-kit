# ACE-004 — Session continuity (run `independent-session-b-v1`)

> Sanitized archive of a controlled dogfood run. Original transcript and
> per-run scaffold lived in a temp `.repolens/eval-runs/` workspace that was
> discarded after the run.
>
> **This is controlled case-level dogfood evidence, not product-level proof.**

## Case

ACE-004 — Session continuity (see
[`docs/evals/agent-context-evals.md`](../agent-context-evals.md) for the
canonical case definition).

## Run id

`independent-session-b-v1`

## Repo / binary provenance

- **Era.** Round 5.2 eval scaffolding, with provenance blocks enabled.
- **Scaffold provenance.** `generated_by: repolens@0.1.0`,
  `template_set: ace-eval@round-5.2`,
  `scorecard_schema: repolens-scorecard@round-5.2`.
- **Source commit at scaffold time.** The temp workspace was a small git repo
  seeded for the eval; the artifact recorded the temp repo's seed commit, not a
  `repolens-rs` product commit.
- **Session B independence.** Session B ran as an unforked Codex subagent with
  `fork_context=false` and no prior conversation context.

## Setup summary

1. Created a temp git workspace with a minimal `README.md`.
2. Ran `repolens init`.
3. Ran `repolens eval start --case ACE-004 --run-id independent-session-b-v1`.
4. Session A created `docs/onboarding-checklist.md` and a
   `session-A-summary.md` containing:
   - what was done;
   - what remained;
   - an explicit `Pick Up Here` block;
   - evidence paths for `README.md` and `docs/onboarding-checklist.md`.
5. Ran `repolens remember --file session-A-summary.md`.
6. Session B was given only the temp workspace path, binary path, the task
   "Continue the previous session's work. Use RepoLens artifacts first.", and
   permission to inspect RepoLens artifacts first.

## Scorecard summary

| Metric | Value | Note |
|---|---:|---|
| `time_to_useful_context` | 1 agent turn | Session B found the `Pick Up Here` handoff in its first response. |
| `repeated_discovery_count` | 0 | |
| `grep_fallback_count` | 0 | |
| `wrong_file_attempts` | 0 | Inspected files were named by the handoff and relevant. |
| `clarification_count` | 0 | |
| `known_constraint_miss` | `n/a` | Case does not assert a known-constraint task. |
| `first_plan_quality` | `n/a` | ACE-004 is a continuation/handoff case; Session B performed the documented next step. |
| `human_intervention_count` | 0 | |
| `session_handoff_quality` | `pass` | Prior session memory exposed what was done, what remained, and the next concrete step. |
| `stale_checkout_reconciled` | `n/a` | Case does not exercise stale-checkout behavior. |
| `memory_safety_notice_used` | `pass` | Session B treated imported memory as unverified and confirmed relevant evidence paths before acting. |

**Verdict:** pass.

## Observed behavior

- Session B ran `repolens brief`.
- It read `.repolens/current-agent-brief.md` and
  `.repolens/session-memory.md` before source files.
- It identified the `Pick Up Here` block.
- It verified `README.md` and `docs/onboarding-checklist.md`.
- It added a minimal onboarding link to `README.md`.

## Limitations

- Session B was independent at the agent-context level, but it was still a
  Codex subagent rather than a separate external Claude Code OS process.
- Session B was explicitly instructed to use RepoLens artifacts first. This
  tests the intended artifact-first protocol; it does not test whether an
  unconstrained agent would discover that protocol unaided.
- The task was intentionally small and documentation-only. It does not measure
  whether the same handoff quality survives a larger source-editing task.

## Product implications

- This is stronger evidence than the earlier single-process ACE-004 run that
  `remember` -> `brief` artifacts are sufficient for a fresh agent context to
  resume a narrow task under the intended protocol.
- The `Pick Up Here` lift remains load-bearing: it made the actionable next
  step easy to find without rereading the entire imported summary.
- The run is not product-level proof. It supports the case-level claim that
  the current handoff artifacts can work when Session A writes a good summary
  and Session B follows the artifact-first protocol.

---

> **Reminder.** This is controlled case-level dogfood evidence, not product-level
> proof. Read it as one data point in the eval suite, not as a guarantee about
> RepoLens behavior in the wild.
