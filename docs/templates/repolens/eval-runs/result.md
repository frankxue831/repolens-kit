<!--
Template: eval-runs/result.md
Lives at:  .repolens/eval-runs/<CASE_ID>-<RUN_ID>/result.md  (after `repolens eval start`)
Purpose:   Final write-up: summary, verdict, friction findings, product
           implications, recommended next changes.
Owner:     The reviewer fills this in after `agent-transcript.md` and
           `scorecard.md` are complete.
-->

# Eval Result

**Case:** {{CASE_ID}} — {{CASE_TITLE}}
**Run ID:** `{{RUN_ID}}`

## Summary

_2–4 sentences. What was tried, what happened, why it passed or failed.
No spin — describe friction even if the verdict is pass._

## Verdict

- **Result:** _pass / fail / partial_
- **Justification:** _which pass criteria from `case.md` were met, which were not_

## Friction findings

_List concrete friction patterns observed during the run. Each entry:
short tag, one-line description, file/line citation if relevant. These
feed the friction log._

- **Tag:**
  **Description:**
  **Citation:**

## Product implications

_What does this run say about RepoLens, not about the agent under test?
A pass tells you the brief/memory/handoff scaffolding worked for this
prompt; a fail tells you the scaffold did not bind. Be specific._

-

## Recommended next changes

_Concrete deltas to the product, the brief format, the templates, or the
case itself. Mark each as **product** / **case** / **doc** / **agent**._

- **[product]**
- **[case]**
- **[doc]**

## Friction log update

- **Should this run promote a finding to `.repolens/agent-friction-log.md`?**
  _yes / no_
- **If yes, friction tag and one-line entry to add:**

  ```
  ## YYYY-MM-DD — _<one-line description>_
  - **Tag:**
  - **What happened:**
  - **Cost:**
  - **Lesson:**
  ```

> Reminder: `repolens eval start` does **not** modify
> `.repolens/agent-friction-log.md` automatically. If this run warrants a
> friction-log entry, add it by hand or via your own tooling. Round 5
> intentionally stops at scaffolding.
