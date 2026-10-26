# ACE-002 — Constraint adherence (run `round-5.1-002`)

> Sanitized archive of a controlled dogfood run. Original transcript and
> per-run scaffold lived in a synthetic temp `.repolens/eval-runs/` workspace
> that was discarded after the run.
>
> **This is case-level controlled dogfood evidence, not product-level proof.**

## Case

ACE-002 — Constraint adherence (see
[`docs/evals/agent-context-evals.md`](../agent-context-evals.md) for the
canonical case definition).

## Run id

`round-5.1-002`

## Repo / binary provenance

- **Era.** Round 5.1 — between
  `de9fe5e` feat(cli): Round 5.1 — scorecard metric definitions and accepted
  values (repolens-rs) and the
  Round 5.2 work that followed.
- **Source commit at scaffold time.** Not recorded in the run artifacts —
  Round 5.1 `repolens eval start` did not stamp a `source_commit` field.
  Round 5.2 added the `<!-- repolens:provenance -->` block (template_set,
  scorecard_schema, generated_at, source_commit) precisely so future runs do
  not have this gap.
- **Binary at scaffold time.** Initially a stale `target/debug/repolens`
  built before the Round 5.1 scorecard rewording landed; rebuilt mid-run
  (see Findings).

## Setup summary

1. Synthetic temp git repo created via `mkdir`/`git init`.
2. `repolens init` — scaffolded the standard six-file `.repolens/` workspace.
3. `.repolens/known-constraints.md` manually filled with a single real
   constraint:
   - **C-001 — JSON camelCase.** "All JSON output uses camelCase
     (Java contract parity)." Evidence cited: `CLAUDE.md` "Key Patterns".
4. `repolens eval start --case ACE-002 --run-id round-5.1-002`.
5. Brief task given to the agent under test:
   > "Add a new field `total_count` to the `compare-summaries` output."

No source files were opened during the run; the agent produced a plan only.

## Scorecard summary

| Metric | Value | Note |
|---|---:|---|
| `time_to_useful_context` | 1 turn | Constraint surfaced on first read of brief. |
| `repeated_discovery_count` | 0 | |
| `grep_fallback_count` | 0 | |
| `wrong_file_attempts` | `n/a` | Plan-only run; no source-tree navigation expectation. **Originally recorded as blank under Round 5.1 wording**; Round 5.2 metric definition explicitly classifies plan-only runs as `n/a`, so this archive uses the Round 5.2 form. The original Round 5.1 blank is preserved here as a footnote of measurement-vocabulary drift. |
| `clarification_count` | 0 | |
| `known_constraint_miss` | 0 | Plan used `totalCount`, not `total_count`. |
| `first_plan_quality` | `pass` | First draft conformed to C-001. |
| `human_intervention_count` | 0 | |
| `session_handoff_quality` | `n/a` | Single-session case. |
| `stale_checkout_reconciled` | `n/a` | Case does not exercise shipped-CLI dogfood. |
| `memory_safety_notice_used` | `n/a` | Task does not touch memory. |

**Verdict:** pass.

## Observed behavior

- Brief surfaced C-001 verbatim; the agent's first plan named the field
  `totalCount` rather than `total_count` without prompting.
- Brief did not point at a specific source file because
  `.repolens/repo-overview.md` was unfilled — see Findings.

## Findings

1. **Stale-binary scorecard drift.** First scaffold used an old
   `target/debug/repolens` and emitted Round 5.0 scorecard wording even
   though the workspace docs were on Round 5.1. After `cargo build`, the
   re-scaffolded scorecard carried Round 5.1 wording. Reviewers had no way
   to distinguish "Round 5.0 binary, Round 5.1 docs" from "everything
   on 5.1" from artifact inspection alone.
   - This finding directly motivated the Round 5.2 provenance block. Going
     forward, every scaffolded artifact records `generated_by`,
     `template_set`, `scorecard_schema`, `generated_at`, and
     `source_commit` so this drift is detectable from the artifact itself.
2. **`repo-overview.md` unfilled.** The brief could surface C-001 but
   could not point the agent at `crates/repolens-cli/src/compare.rs`. Real
   ACE-002 cases against this repo will want
   `.repolens/repo-overview.md` filled before the run.
3. **Surface vs. integration coverage.** ACE-002 as currently defined
   tests whether constraints **surface** in the brief and survive into
   the first plan. It can be passed by pattern-matching
   `total_count → totalCount` without ever opening
   `crates/repolens-app/src/summary.rs::ComparisonService` or the
   serialization site. This is consistent with the case's stated
   pass criteria but is worth recording — deeper "constraint integration"
   coverage would need a separate case where the agent actually edits
   serialization code.

## Methodological caveats

- Single agent process. Whether a real agent under instructional load
  would still respect C-001 on first draft is not measured here.
- Synthetic temp repo, not the real `repolens-rs` checkout. The brief
  surfaced exactly one constraint; cases against the live repo would
  surface more and stress the agent's prioritization differently.
- No Round 5.2 provenance block in the original scaffold — see Finding 1.
  The "Round 5.1 era" framing is the most precise statement available.

## Product implications

- **Confirms shipped:** `repolens brief` will surface a single,
  human-filled constraint to the agent in a way that an agent can pick up
  without any MCP/scan integration.
- **Does not confirm:** that the agent would honor a constraint deep in
  serialization code, that multi-constraint briefs scale gracefully,
  or that ACE-002 distinguishes shallow from deep adherence.
- **Motivated:** the Round 5.2 eval artifact provenance block.

---

> **Reminder.** This is case-level controlled dogfood evidence, not
> product-level proof. Read it as one data point in the eval suite, not
> as a guarantee about RepoLens behavior in the wild.
