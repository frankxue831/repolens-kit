# Engineering Canon

Authority: Canonical
Lifecycle: Living

This doc is an orientation layer for internal maintainers and agents. It is not a source of substantive runtime claims; use it to find the current engineering canon and status vocabulary.

## Start Here When

- you need the current shipped/runtime boundary
- you need the trust contract for envelope, grounding, and evidence grades
- you need the canonical meaning of document authority and lifecycle labels

## Status Vocabulary

- `Authority: Canonical` — current source of truth for its topic
- `Authority: Supporting` — useful context that informs the canon but does not outrank it
- `Authority: Historical` — preserved for historical value and must carry a frozen or superseded banner
- `Lifecycle: Living` — current and maintained
- `Lifecycle: Frozen` — formerly maintained current guidance, now preserved as a dated snapshot
- `Lifecycle: Superseded` — no longer current; a successor doc now carries current guidance
- `Lifecycle: Reference` — background material that was never intended to be the live operational canon

## Read In This Order

The first-pass Authority/Lifecycle rollout covers [07-current-state-vs-direction.md](philosophy/07-current-state-vs-direction.md), [06-observation-invariants-and-grounding-eligibility.md](philosophy/06-observation-invariants-and-grounding-eligibility.md), and [08-taxonomy.md](philosophy/08-taxonomy.md). The remaining engineering specs below are implementation-detail references outside that first-pass registry/status coverage.

1. [07-current-state-vs-direction.md](philosophy/07-current-state-vs-direction.md) — current shipped behavior
2. [06-observation-invariants-and-grounding-eligibility.md](philosophy/06-observation-invariants-and-grounding-eligibility.md) — trust and evidence contract
3. [2026-04-11-epistemic-envelope-design.md](superpowers/specs/2026-04-11-epistemic-envelope-design.md) — envelope field semantics
4. [2026-04-05-grounded-memory-design.md](superpowers/specs/2026-04-05-grounded-memory-design.md) — grounding behavior and scope
5. [2026-04-11-wiki-compiled-knowledge-layer-design.md](superpowers/specs/2026-04-11-wiki-compiled-knowledge-layer-design.md) — wiki-layer design, with taxonomy normalized to doc 08
6. [2026-04-14-self-bootstrap-runtime-design.md](superpowers/specs/2026-04-14-self-bootstrap-runtime-design.md) — bounded self-bootstrap runtime

## Cross-Cutting Canon

- [08-taxonomy.md](philosophy/08-taxonomy.md) — canonical four-axis reconciliation
- [00-status.md](philosophy/00-status.md) — current project status and open questions
