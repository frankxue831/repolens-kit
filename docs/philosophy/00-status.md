# RepoLens Philosophy Exploration — Status

Authority: Canonical
Lifecycle: Living

**Phase**: Active exploration (not finalized)
**Started**: 2026-04-04
**Last updated**: 2026-04-17

## What's Established

- **One-liner**: "Structured perception for AI agents working on codebases"
- **Mission**: Be the structured perception layer and epistemic infrastructure for AI coding agents
- **Root entry docs**: [../product-canon.md](../product-canon.md) and [../engineering-canon.md](../engineering-canon.md) are orientation-only entrypoints for internal maintainers and agents; the philosophy docs below remain the detailed canon
- **Four Core Principles**:
  1. Observe, Don't Judge
  2. Memory Is Alive
  3. Match the Consumer's Budget
  4. Evidence Over Confidence
- **Architectural philosophy**: Immutable Packs (structural truth) vs. mutable Memory (interpretive truth)
- **Four orthogonal axes**: storage tier (`pack / memory / wiki / repo-filesystem`), claim-production layer (`knowledgeLayer`: `observation / heuristic / interpretation`), trust state (`confidence`, `evidenceGrade`, grounding/staleness/freshness), and retention/ranking state (importance, decay, search ranking) are independent axes; the canonical reconciliation is [08-taxonomy.md](08-taxonomy.md)
- **Direction of travel**: Path B (grounded memory) is the chosen direction; RepoLens should validate memory against observations without drifting into synthesized product meaning
- **Output contract**: MCP responses now surface the truth-layer distinction explicitly via the epistemic envelope
- **Grounding gate**: doc 06 §5 plus the shipped named-invariant catalogues fix how grounding power is gated by observation soundness. Rubric §6 requirement 5 is satisfied for every family in doc 06 §5 currently graded `confirmOnly` or `contradictionEligible`. Enforcement is reviewer-discipline by design — see the "Why Reviewer-Enforcement, Not Automation" subsection in doc 06 for the named revisit trigger.
- **Verification axis on envelope**: `evidenceGrade: "verified" | "derived" | "unaudited"` is the fifth envelope field as of `3e70c06`. It is orthogonal to `knowledgeLayer` and `confidence`, defaults to `unaudited` per doc 06 §3, and is adoption-opt-in via `ok_epistemic_graded`. Three pure-observation tools emit `verified` (`get_callers`, `get_dependencies`, `find_cycles`); three tools emit `derived` (`get_graph`, `get_impact`, `get_profile`).
- **Narration discipline**: doc 07 (`07-current-state-vs-direction.md`) is the canonical shipped-vs-direction reference. When describing the product in any forward-facing context, quote its "Shipped today" column — not doc 06's aspirations.
- **Audience**: 100% agent-oriented (humans interact through agents empowered by RepoLens)

## What's Open

- What "good structure" means for the product (conceptual integrity, cognitive structure, etc.)
- Whether/how RepoLens should observe product structure (business logic, decisions, conventions) in addition to repo structure (technical/dependency)
- The perception-comprehension boundary: where exactly should RepoLens stop?
- Implications for human users who trust agents that trust RepoLens
- Which remaining mixed-payload tools need catalogue closure before they can emit `evidenceGrade: "derived"` under the current tool-level grade rule (`analyze_health`, `get_summary`, `compare_summaries`, `find_dead_code`), and whether any of them will still need per-field grading after their blocking gaps are catalogued

## Document Index

| File | Contents |
|------|----------|
| [../product-canon.md](../product-canon.md) | Orientation-only root entrypoint for internal maintainers and agents. High-level product canon index, not the detailed philosophy canon |
| [../engineering-canon.md](../engineering-canon.md) | Orientation-only root entrypoint for internal maintainers and agents. High-level engineering canon index, not the detailed philosophy canon |
| [01-background-and-mission.md](01-background-and-mission.md) | Why RepoLens exists, current mission statement |
| [02-research-landscape.md](02-research-landscape.md) | Industry landscape, academic foundations, competitive context |
| [03-deep-reasons.md](03-deep-reasons.md) | The deep-down reasons behind our design choices |
| [04-current-boundaries.md](04-current-boundaries.md) | Precise mapping of observation vs. interpretation in current code |
| [05-structure-and-tensions.md](05-structure-and-tensions.md) | Analysis of "good structure", identified tensions, three paths forward |
| [06-observation-invariants-and-grounding-eligibility.md](06-observation-invariants-and-grounding-eligibility.md) | Contract that gates Path B: observation soundness invariants plus per-family grounding-eligibility tiers |
| [07-current-state-vs-direction.md](07-current-state-vs-direction.md) | Shipped-vs-roadmap reference. Governance tool for narration: quote the "Shipped today" column when describing what the product actually does |
| [08-taxonomy.md](08-taxonomy.md) | Canonical reconciliation of the four orthogonal axes (storage tier, claim-production layer, trust state, retention/ranking state). Cite this whenever a doc uses overlapping "layer", "tier", or "state" framings |
| [notes/2026-04-14-arc-review-and-next-steps.md](notes/2026-04-14-arc-review-and-next-steps.md) | Retrospective on the observation-invariant + evidenceGrade arc, reviewer corrections, and the next-development backlog |
