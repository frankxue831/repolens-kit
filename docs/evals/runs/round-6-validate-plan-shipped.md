# Round 6 — `repolens validate-plan` shipped (verification snapshot)

> Compact post-merge verification of the Round 6 deterministic plan
> linter. Not an ACE eval run — this is a shipped-feature verification
> recorded against six explicit user checks.
>
> **This is a feature-shipping verification snapshot, not product-level proof.**

## Subject

`repolens validate-plan` — the warnings-only deterministic plan linter
described in `docs/superpowers/plans/2026-04-26-lint-plan-warnings-only.md`.

## Provenance

- **Branch:** `main`
- **HEAD at verification:** `ea81e81`
- **Commits in scope:**
  - `ab8d3fa` docs(plans): add Round 6 lint-plan implementation plan
  - `ea81e81` feat(cli): Round 6 — repolens validate-plan (warnings-only deterministic plan linter)
- **Verification date:** 2026-04-26
- **Pre-push gate:** `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` 0 failures; `bash scripts/verify-doc-canon.sh` exit 0.
- **Tests:** 258 pass / 0 fail across 8 CLI test binaries (153 unit + 6+33+2+29+6+16+13 integration; the new `validate_plan_cli_test` contributes 13).

## Six-check verdict

| # | User check | Verdict | Evidence |
|---|---|---|---|
| 1 | Warns, does not certify | ✅ | Header always: "warnings only. This is a guardrail, not proof." Findings disclaimer: "do not prove the plan is invalid or valid." No "valid" / "invalid plan" language anywhere. |
| 2 | Deterministic and narrow | ✅ | Pure regex/substring; no LLM, no MCP, no scoring. Output sorted by `(id, plan_lineno)`; pinned by `validate_plan_output_is_deterministic_across_runs`. |
| 3 | Covers ACE-002 | ✅ | `total_count` under camelCase C-001 → VP001 + VP002 fire; integration-tested by `validate_plan_warns_on_total_count_under_camelcase_constraint`. |
| 4 | Enforces grounding | ✅ | VP003 fires when plan names no file/type and no verification step. Pinned by `validate_plan_warns_when_plan_lacks_grounding_or_verification`. |
| 5 | No fake warnings | ✅ | Pristine `repolens init` (TemplateOnly constraints) → emits "no real constraints loaded" notice; only constraint-independent VP003 fires. Pinned by `validate_plan_emits_no_constraint_warnings_when_no_known_constraints`. |
| 6 | `--help` explains uncertainty | ✅ | Short about: "Lint a plan markdown file (warnings only — does not prove plans valid)". Long-form: "warnings only ... false negatives are acceptable. This is a guardrail, not a replacement for tests or human review." Pinned by `validate_plan_help_states_warnings_only_and_uncertainty`. |

## Sample outputs (live, abbreviated)

**Bad plan (snake_case under camelCase rule):** 3 warnings — `VP001` (cites C-001 with line number), `VP002` (missing constraint citation), `VP003` (no grounding). Exit 0.

**Good plan (camelCase + verification step + named file + tests mention):** 0 warnings; "RepoLens found no possible plan risks. This does not prove the plan is valid." Exit 0.

**Neutral plan, workspace with no real constraints:** "no real constraints loaded ... Skipping constraint-aware checks (VP001, VP002)." Only VP003 fires. Exit 0.

(Full output captured in the prior turn's verification report; not duplicated here for compactness.)

## Honest caveats

- **Coverage gap vs. the Round 5.2 ACE-002 four-part bar.** The fourth
  part — "mention tests / snapshots / docs when output contract changes"
  — is **not** covered. Tracked as VP004 in the Round 6.1 follow-up plan.
- **Naming under "validate".** Shipped under `repolens validate-plan`
  per the user spec, even though the Round 6 plan recommended
  `lint-plan` for naming-honesty reasons. Mitigated by leading every
  output and `--help` line with "warnings only — does not prove plans
  valid". Whether the disclaimer carries enough load is a question for
  the next dogfood run, not this verification.
- **Synthetic dogfood, single Claude process.** All six checks were
  verified in temp git-init workspaces, not against a live agent eating
  real ACE-002 plans. The unit + integration tests pin behavior; the
  product question — "does an agent self-correct after reading the
  warnings?" — is open.
- **Constraint-aware checks are heuristic.** VP001's 80-character window
  and VP002's "applicable constraint" heuristic both catch the obvious
  ACE-002 bait but will miss subtle phrasings. False negatives are
  intentional per the design.

## Status

**Round 6 is shipped at `ea81e81` and pushed to `origin/main`.** The
follow-up scope (VP004, optional alias revisit, integration into an
ACE-002 dogfood case) is captured in
`2026-04-26-round-6.1-validate-plan-followups.md`.

---

> **Reminder.** This is a feature-shipping verification snapshot, not
> product-level proof. A clean six-check verdict means the implementation
> matches the spec under controlled inputs. It does not mean
> `validate-plan` will catch every plan risk in real agent workflows.
