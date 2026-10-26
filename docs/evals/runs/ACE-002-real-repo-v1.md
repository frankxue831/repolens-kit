# ACE-002 — Constraint adherence (run `real-repo-v1`)

> Sanitized archive of a controlled dogfood run. Original transcript and
> per-run scaffold lived in a temp copy of the real `repolens-rs` checkout.
>
> **This is controlled case-level dogfood evidence, not product-level proof.**

## Case

ACE-002 — Constraint adherence (integration, not just surfacing) (see
[`docs/evals/agent-context-evals.md`](../agent-context-evals.md) for the
canonical case definition).

## Run id

`real-repo-v1`

## Repo / binary provenance

- **Source repo at setup.** Main checkout was on `main` at `0217aed`.
- **Workspace under test.** Temp copy of the real repo, excluding `.git`,
  `.repolens`, and `target`.
- **Scaffold provenance.** `generated_by: repolens@0.1.0`,
  `template_set: ace-eval@round-5.2`,
  `scorecard_schema: repolens-scorecard@round-5.2`.
- **Source commit in scaffold.** `unknown`, because the temp copy excluded
  `.git`. The source repo state was recorded separately before the run.

## Setup summary

1. Confirmed Prompt 3's tightened ACE-002 spec was present: first draft must
   use `totalCount`, name the output surface, ground the implementation path,
   and include verification.
2. Built a fresh `repolens` binary.
3. Created a temp copy of the real `repolens-rs` source tree, excluding
   `.git`, `.repolens`, and `target`.
4. Ran `repolens init` in the temp copy.
5. Seeded `.repolens/known-constraints.md` with:
   - **C-001 — JSON/output camelCase.** User/tool-facing JSON/output fields
     should use camelCase, not snake_case, for Java contract parity.
   - Evidence: `CLAUDE.md` "Key Patterns" says all JSON uses
     `#[serde(rename_all = "camelCase")]`.
   - Corroboration: `AGENTS.md` repeats the same rule and calls out
     multi-word MCP params such as `maxDepth`.
6. Ran `repolens eval start --case ACE-002 --run-id real-repo-v1`.
7. Ran `repolens brief --task "Add a new field 'total_count' to the
   compare-summaries output."`.

No implementation was performed.

## Scorecard summary

| Metric | Value | Note |
|---|---:|---|
| `time_to_useful_context` | 2 commands | Brief surfaced the constraint; one `rg` command was needed to find implementation files. |
| `repeated_discovery_count` | 0 | |
| `grep_fallback_count` | 1 | `rg` was needed because the brief did not point to compare-summaries implementation files. |
| `wrong_file_attempts` | 0 | All inspected files were relevant. |
| `clarification_count` | 0 | |
| `known_constraint_miss` | 0 | First draft used `totalCount`, not `total_count`. |
| `first_plan_quality` | `pass` | First draft satisfied all four tightened ACE-002 criteria. |
| `human_intervention_count` | 0 | |
| `session_handoff_quality` | `n/a` | Case does not exercise handoff/resume behavior. |
| `stale_checkout_reconciled` | `n/a` | Stale-checkout behavior was not under test. |
| `memory_safety_notice_used` | `n/a` | No memory behavior was involved. |

**Verdict:** pass.

## Observed behavior

- `repolens brief` surfaced the seeded camelCase constraint clearly.
- The brief did not point to compare-summaries implementation files.
- One `rg` search was needed to ground the implementation path.
- Real repo files inspected:
  - `crates/repolens-core/src/model/summary.rs::SummaryComparison`
  - `crates/repolens-app/src/summary.rs::ComparisonService::compare`
  - `crates/repolens-cli/src/main.rs::run_compare_summaries`
  - `crates/repolens-cli/src/compare.rs::render_comparison_markdown`
  - `crates/repolens-cli/tests/compare_summaries_cli_test.rs`
- The first-draft plan named `totalCount`, identified the output
  schema/serialization surface, grounded the implementation path in real
  files/types/functions, and included verification steps for CLI tests, JSON
  shape, docs, and snapshots.

## Limitations

- The run used the main Codex session, not an independent unprimed subagent.
- The temp copy excluded `.git`, so the scaffold provenance recorded
  `source_commit: unknown`; the source checkout state was recorded separately.
- Because `.repolens/repo-overview.md` and scan-pack context were absent in
  the temp copy, file localization still depended on `rg`.

## Product implications

- Constraint surfacing works on the real repo: the brief made C-001 visible
  before planning, and the plan preserved `totalCount`.
- Grounding still depends on unstructured search unless `repo-overview`, a
  scan pack, or future brief enrichment points to likely implementation files.
- This motivates `validate-plan` as a plan-grounding guardrail, but
  `validate-plan` should not be claimed as file localization.
- The tightened ACE-002 spec is useful because it distinguishes constraint
  integration from superficial `total_count` -> `totalCount` string
  replacement.

---

> **Reminder.** This is controlled case-level dogfood evidence, not product-level
> proof. Read it as one data point in the eval suite, not as a guarantee about
> RepoLens behavior in the wild.
