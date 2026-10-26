# Documentation Canon Consolidation Design

Authority: Supporting
Lifecycle: Living

**Date:** 2026-04-18
**Status:** Accepted
**Scope:** Internal maintainer/agent-facing reorganization of product, philosophy, and touched engineering/spec docs

> This spec is intentionally not tracked in `docs/doc-registry.yaml`: design specs only enter the registry when the rule-driven sweep surfaces them (see "Registry Scope" below). The Authority/Lifecycle header is still set so the spec is consistent with the scheme it defines.

## Purpose

RepoLens now has enough philosophy, runtime, and trust-contract material that the bigger risk is no longer "missing docs." The bigger risk is **conceptual drift**: multiple partially overlapping taxonomies, uneven status signaling, and no single machine-readable answer to "what is canonical right now?"

This design reorganizes the docs for **internal maintainers and agents**. It is not a marketing rewrite and it is not an archive move. The goal is to make the active canon cheap to navigate without breaking the existing web of numbered-doc and line-level citations.

## Goals and Non-Goals

This reorg adds two root-level entry docs, a machine-readable registry, and a standardized two-part status scheme, applied in place without moving files. The substantive detail is in "Canonical Information Architecture" below.

**Non-Goals** (stated explicitly because every one of these is a plausible failure mode for a reorg of this shape):

- Moving docs into an `archive/` tree
- Renumbering the philosophy docs
- Renaming existing files — filenames are preserved for link stability, including eyesore filenames like `Mathematics of Structured Perception ... .md`; renames are a separate, later decision
- Rewriting all specs in the repository
- Turning the new entry docs into narrative sources of truth
- Changing runtime behavior or MCP envelopes as part of this documentation-only reorganization

## Decision Summary

| Question | Decision | Rationale |
|---|---|---|
| What kind of reorg is this? | Consolidation | Resolve conceptual drift without a whole-tree rewrite |
| Who is it optimized for? | Internal maintainers and agents | They need one canonical navigation path more than polished external storytelling |
| What is the top-level IA? | Hybrid | Add role-based entry docs, keep numbered docs as substantive canon |
| Where do entry docs live? | `docs/` root | Visible from a cold `ls docs/` without demoting numbered docs |
| How are outdated docs handled? | Mark in place with banners | Preserves existing links and spec citations |
| How is doc status represented? | Two-part scheme | Separates authority from lifecycle |
| Authority vocabulary | `Canonical` / `Supporting` / `Historical` | Clear and stable |
| Registry format | YAML | Human-editable, greppable, mechanizable later |
| Spec-sweep scope | Rule-driven | Avoids under- or over-scoping touched-spec updates |

## Canonical Information Architecture

### Root Entry Docs

Two new root-level docs become the entrypoints for internal readers:

- `docs/product-canon.md`
- `docs/engineering-canon.md`

These docs are **orientation only**. They may contain:

- audience framing
- a short explanation of when to start here
- a read-in-this-order list
- links to canonical/supporting/historical docs with one-line descriptions

They may **not** contain:

- substantive product claims that are not anchored elsewhere
- restatements of table rows or factual details from numbered docs
- spec-level requirements
- policy text that would need to be maintained in two places

### Numbered Philosophy Docs

The numbered philosophy docs remain the substantive canon. The reorganization does **not** demote them behind the new entry docs. Instead:

- the root entry docs point into them
- each numbered doc gets an explicit status header
- historical or superseded sections gain banners where needed

### Registry

Add a new machine-readable registry at:

- `docs/doc-registry.yaml`

This file becomes the single grep-friendly source for:

- path
- authority
- lifecycle
- subtype when applicable
- short role/description
- audience
- read-order grouping
- successor pointer where applicable

The registry is not prose. It is navigation and governance metadata.

### Registry Scope

The registry does **not** track every Markdown file in the repository. Its scope is bounded to the docs touched by the first consolidation pass plus anything added by later passes under the same scheme. In practice, the first-pass registry covers:

- both root entry docs
- every file under `docs/philosophy/` including `notes/`, registered **individually per file**, not as a folder rollup — greppability beats compactness here, and a per-file row is the unit the drift check validates
- any spec under `docs/superpowers/specs/` that the rule-driven sweep surfaces

Plans and runbooks under `docs/superpowers/plans/` are explicitly out of scope for registry tracking because they are operational artifacts with their own lifecycle; the same goes for changelogs and one-off reports.

### Registry Maintenance Rule

The registry is only useful while it stays in sync with the tree it describes. To prevent silent drift, reviewers **must** reject changes that:

1. add a new Markdown file under `docs/philosophy/` or `docs/` root without a corresponding registry entry in the same commit
2. flip a doc's authority or lifecycle (for example, adding a Historical banner to a previously canonical doc) without updating its registry row in the same commit
3. rename or move a tracked file without updating its `path` in the registry in the same commit

This is the same pattern as the `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` §5 maintenance rule: a reviewer-enforced lockstep contract, not a CI gate, for the reasons stated in that doc's "Why Reviewer-Enforcement, Not Automation" subsection. A lightweight CI check that asserts every file under the registry's in-scope paths appears in the registry is listed under "Drift-Detection Check" below as a belt-and-braces mechanism.

## Status Model

Every doc in the targeted tree gets an explicit two-part status header. There is no implicit default such as "no header means canonical."

### Authority

- `Canonical`
- `Supporting`
- `Historical`

### Lifecycle

- `Living`
- `Frozen`
- `Superseded`
- `Reference`

### Lifecycle Semantics

`Frozen` and `Reference` are intentionally different:

- `Frozen`: the doc used to be maintained as current guidance, but is now preserved as a point-in-time record as of a specific date
- `Reference`: the doc is background or research material that was never intended to be the current operational canon

This distinction must be stated exactly once, in `docs/engineering-canon.md` under a short "Status Vocabulary" section, so cold readers have a single definitive place to check and every other doc can link to it. Avoid duplicating the gloss elsewhere — duplication is what got us into this reorg in the first place.

### Historical Subtypes and Banner Templates

`Historical` is the authority label, but historical docs must also say **why** they are historical.

Allowed file-level banner templates (placed immediately below the status header at the top of a Historical doc):

- `Historical — Frozen YYYY-MM-DD. Preserved as the authoritative record of its topic as of that date.`
- `Historical — Superseded YYYY-MM-DD by [doc-X]. Read [doc-X] for current guidance.`

Allowed section-local banner templates (placed immediately after the `##` or `###` heading of a superseded/frozen section inside an otherwise-canonical doc, as a Markdown blockquote so it's visually distinct from body prose):

- `> Historical — Frozen YYYY-MM-DD. This section is preserved for historical context; the rest of this doc remains canonical.`
- `> Historical — Superseded YYYY-MM-DD by [doc-X]. Read [doc-X] for current guidance on this section's topic; the rest of this doc remains canonical.`

These four forms are mandatory templates, not examples. The point is to keep the subtype language from drifting across docs, and to keep the reader oriented about whether authority is demoted at the file or at the section.

### Scope of File-Level Status vs Section-Local Banners

The two-part status header at the top of a file applies to the **whole file**. The registry tracks file-level status only. When only a section is superseded or frozen, the file-level status stays as whatever applies to the remainder (typically `Canonical / Living`) and a section-local banner scopes the demotion narrowly. A reader who sees `Authority: Canonical` at the file level and a section-local Historical banner further down should interpret this as "this doc is canonical except for the sections that explicitly say otherwise."

The registry does **not** attempt to track section-level status. Authors who need that granularity carry it in the banner prose itself.

## Targeted Tree for the First Pass

The first consolidation pass covers:

- `docs/product-canon.md` (new)
- `docs/engineering-canon.md` (new)
- `docs/doc-registry.yaml` (new)
- all docs under `docs/philosophy/`
- touched specs under `docs/superpowers/specs/` that match the rule-driven sweep below

The first pass does **not** attempt a whole-tree normalization of every design spec in the repository.

## Reading Paths

Both paths are written as what the reader consults **after** landing on the entry doc itself. The entry doc is always the literal step 0 — readers open `docs/product-canon.md` or `docs/engineering-canon.md` first, which then orients them and sends them through the following order.

### Product Reading Path

After `docs/product-canon.md`:

1. `docs/philosophy/00-status.md`
2. `docs/philosophy/01-background-and-mission.md`
3. `docs/philosophy/07-current-state-vs-direction.md`
4. `docs/philosophy/08-taxonomy.md`
5. `docs/philosophy/05-structure-and-tensions.md`
6. `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`

### Engineering Reading Path

After `docs/engineering-canon.md`:

1. `docs/philosophy/07-current-state-vs-direction.md`
2. `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md`
3. envelope / grounding / runtime specs by topic
4. implementation notes and milestone notes where relevant

The root docs may summarize *why* this order exists, but they should not restate the underlying claims. Registry `read_order` integers reflect the position **within the path**. Downstream docs start at 1; the root entry docs themselves may use `0` to mean "entrypoint / step 0" for grep-friendly completeness. The values are path-local, not a global ordering across paths.

## Canonical Taxonomy Rules

The reorganization adopts these rules:

1. `storage tier` is a different axis from `knowledgeLayer`
2. `trust state` is a different axis from `retention/ranking state`
3. the envelope taxonomy is about claim production, not where the artifact lives
4. wiki is a storage tier, not a fourth `KnowledgeLayer`
5. older shorthand such as "Three Layers of Truth" may remain for historical context, but must point to the canonical reconciliation doc

The canonical reconciliation remains `docs/philosophy/08-taxonomy.md`.

## Rule-Driven Touched-Spec Sweep

The spec pass is not a fixed hand-curated list. It is determined mechanically.

For the first pass, update any spec in `docs/superpowers/specs/` that matches either condition:

1. it still uses the retired `Structural / Interpretive / Compiled` framing from the wiki-layer spec in a way that reads as active canon
2. it references `Three Layers of Truth` or an equivalent layer table without already redirecting readers to `docs/philosophy/08-taxonomy.md`

The implementation plan should define this as a search-driven task, using `rg`, so the scope is repeatable and reviewable.

Expected outcomes of the sweep:

- add status headers where missing
- add reconciliation notes where a spec participates in taxonomy/trust framing
- avoid rewriting implementation-contract sections that are not implicated by the taxonomy issue

## Expected Authority/Lifecycle Outcomes

The first pass should classify docs explicitly rather than leaving them ambiguous.

Recommended default outcomes:

- `docs/philosophy/00-status.md` → `Canonical / Living`
- `docs/philosophy/01-background-and-mission.md` → `Canonical / Living`
- `docs/philosophy/02-research-landscape.md` → `Supporting / Reference`
- `docs/philosophy/03-deep-reasons.md` → `Supporting / Living`
- `docs/philosophy/04-current-boundaries.md` → `Historical / Frozen`
- `docs/philosophy/05-structure-and-tensions.md` → `Canonical / Living`, but with superseded-banner treatment on legacy sections
- `docs/philosophy/06-observation-invariants-and-grounding-eligibility.md` → `Canonical / Living`
- `docs/philosophy/07-current-state-vs-direction.md` → `Canonical / Living`
- `docs/philosophy/08-taxonomy.md` → `Canonical / Living`
- `docs/philosophy/deep-research-report.md` → `Supporting / Reference`
- `docs/philosophy/Mathematics of Structured Perception for RepoLens-Style Agentic Code Understanding.md` → `Supporting / Reference`
- milestone/retrospective notes under `docs/philosophy/notes/` → `Historical / Frozen`

The plan should treat the final classification as an explicit reviewable step, not a hidden assumption.

## Banner and Header Format

Every targeted doc should gain a short header block near the top with:

- `Authority: ...`
- `Lifecycle: ...`
- optional subtype/supporting pointer lines where relevant

Historical docs also get the required banner prose immediately below the header.

For partially superseded docs or sections, a section-local banner is acceptable when demoting the whole file would be wrong. This preserves valuable context while keeping the active canon explicit.

## Registry Shape

The registry should be easy to grep and easy to validate later.

**Root key contract:** the top-level YAML key is `docs:` (plural), holding a sequence of per-file mappings. Implementers must not diverge to `registry:`, a flat list, or any other shape — the drift-detection check and any future tooling assume `docs:`.

Shape:

```yaml
docs:
  - path: docs/philosophy/07-current-state-vs-direction.md
    authority: Canonical
    lifecycle: Living
    audience:
      - maintainers
      - agents
    area:
      - product
      - trust
    read_order:
      product: 3
      engineering: 1
    summary: Shipped-vs-direction reference for current product behavior.

  - path: docs/philosophy/04-current-boundaries.md
    authority: Historical
    lifecycle: Frozen
    frozen_as_of: 2026-04-04
    successor: docs/philosophy/07-current-state-vs-direction.md
    audience:
      - maintainers
      - agents
    area:
      - philosophy
    summary: Point-in-time observation-vs-interpretation analysis preserved for historical context.
```

Minimum required fields:

- `path`
- `authority`
- `lifecycle`
- `summary`

Conditionally required fields:

- `superseded_as_of` (date) and `successor` (path) when lifecycle is `Superseded` — both mandatory together; `successor` points to the doc that replaces this one for current guidance
- `frozen_as_of` (date) when lifecycle is `Frozen`; `successor` is **optional** for Frozen entries and when present means "the living equivalent of this snapshot's topic now lives at the successor path" — it is a pointer, not a replacement claim

### `successor` Field Semantics

`successor` carries different weight depending on lifecycle:

- For `Superseded` entries, `successor` is **mandatory** and is a replacement pointer. Readers should treat the successor as the current authority on the same topic, and the superseded doc should not be used for live guidance.
- For `Frozen` entries, `successor` is **optional** and is a navigation pointer. Readers should treat the frozen doc as still authoritative as the snapshot of its topic on `frozen_as_of`, and use the successor to find what current practice looks like today.
- For `Canonical` and `Supporting` entries, `successor` is **not used** and must be absent.

This distinction is load-bearing because the two relationships are different trust claims: "replaced by" vs "continued by." Conflating them collapses a useful degree of freedom.

## Rollout Strategy

Do the consolidation in bounded stages:

1. add the root entry docs and registry
2. standardize status headers across `docs/philosophy/`
3. add historical banners to frozen/superseded docs and sections
4. apply the rule-driven touched-spec sweep
5. update root-level reading paths and status references
6. add the drift-detection check described below

This order ensures the new canon exists before older docs start pointing away from themselves, and that the check lands last so it doesn't fail CI while intermediate stages are partial.

## Drift-Detection Check

Consolidation is only worth doing if the status scheme stays applied. Ship a small verification script alongside the reorg — `scripts/verify-doc-canon.sh` or equivalent — that runs as part of local pre-push and optionally on CI, and asserts:

1. every file under `docs/philosophy/` including `notes/` appears in `docs/doc-registry.yaml`
2. every file listed in the registry exists on disk at the recorded `path`
3. every registered doc whose `authority` is `Historical` contains one of the four banner templates verbatim (file-level or section-local, matching the lifecycle subtype in the registry)
4. every registered doc contains an `Authority:` and `Lifecycle:` line in its top-of-file header block
5. `successor` is present when `lifecycle` is `Superseded` and absent when `authority` is `Canonical` or `Supporting`
6. `frozen_as_of` is present when `lifecycle` is `Frozen`

The check is intentionally shell+`rg`+a tiny YAML parse, not a Rust binary — it should be readable by any maintainer in one sitting and never become a hidden source of truth itself. A failing check emits one line per violation with the path and the rule number so the fix is obvious.

This check does not replace the reviewer-enforced maintenance rule above; it is a belt-and-braces mechanism that catches the mistakes reviewers miss, and it is intentionally narrow (structural assertions only, not prose judgement).

## Acceptance Criteria

The reorganization is complete when:

- `docs/product-canon.md` and `docs/engineering-canon.md` exist and stay orientation-only
- `docs/doc-registry.yaml` exists, uses `docs:` as its root key, and covers every in-scope file per "Registry Scope"
- every in-scope doc — `Canonical`, `Supporting`, **and** `Historical` alike — has an explicit `Authority:` / `Lifecycle:` header block; Supporting and non-Historical docs are not exempt
- every historical doc has the correct frozen or superseded banner text (file-level or section-local, matching one of the four templates verbatim)
- the `Frozen` vs `Reference` distinction is defined exactly once, in `docs/engineering-canon.md` under a "Status Vocabulary" section
- `scripts/verify-doc-canon.sh` (or equivalent) exists and passes against the reorganized tree
- touched specs are found and updated by rule, not by ad hoc memory
- `docs/philosophy/08-taxonomy.md` is the unambiguous reconciliation target wherever legacy truth-layer framing remains
- a maintainer or agent can determine the active canon without crawling the whole tree manually

## Main Risk

The main failure mode is half-application:

- headers added to some docs but not others
- entry docs drifting into substantive duplicate canon
- historical labels added without subtype banners
- spec sweep done by hand instead of by search rule

This design is only worth doing if the status scheme is applied consistently enough that "missing status" itself becomes suspicious.
