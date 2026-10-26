# get_summary Scope Design

**Date:** 2026-04-18
**Status:** Draft
**Depends on:** ../../opinion_pool/opinion_pool_006.md, [../../philosophy/07-current-state-vs-direction.md](../../philosophy/07-current-state-vs-direction.md), [2026-04-15-get-profile-derived-promotion-design.md](2026-04-15-get-profile-derived-promotion-design.md)

## Goal

Resolve the product fork for `get_summary` before any code lands in the "workflow closure" phase (see `opinion_pool_006.md` §3, Step 1).

The fork: should `get_summary` be a **memory-aware homepage** that surfaces the observation + heuristic + interpretation layers together, or an **observation-first front door** that explicitly delegates memory to `recall_longterm`?

This document states the fork, cites the current state, enumerates the two options, and makes a recommendation. It does not design the eventual envelope-grade promotion; that is downstream work per the precedent set in [2026-04-15-get-profile-derived-promotion-design.md](2026-04-15-get-profile-derived-promotion-design.md).

## Current State

The `RepoSummary` struct (`crates/repolens-core/src/model/summary.rs:116-133`) carries a `memory_insights: Vec<MemoryInsight>` field. `MemoryInsight` itself (lines 101-111) has: `id`, `content`, `memory_type`, `importance`, `last_accessed`, `access_count`, `anchors`, `tags`. No `groundingStatus` field.

The service layer does not populate it:

- `SummaryService::summarize` hardcodes `memory_insights: vec![]` at `crates/repolens-app/src/summary.rs:81` (full path) and `:766` (the test/compact path).

The MCP handler treats absence of memory as the signal to drop the interpretation layer from the envelope:

- `has_memory = !summary.memory_insights.is_empty()` at `crates/repolens-mcp/src/tools/summary.rs:93`
- Three tiers (compact/standard/full) each serialize `"hasMemoryInsights"` at lines 315, 408, 515 and each emit the envelope through `ok_epistemic(...)` — i.e. `EvidenceGrade::Unaudited`.

Doc 07's "Known Mismatches" §1 explicitly lists `get_summary` as a place where narration has drifted ahead of behavior: "'get_summary' returns grounded memories. False."

Net: the field exists, the wire format can carry it, the MCP handler already has a gate for it, and a canonical narration doc has flagged the gap. The fork is primarily a product decision, but landing Option A also requires a schema change (`MemoryInsight.grounding_status`) and a new memory-loading path in `SummaryService`.

## The Fork

### Option A — Memory-Aware Homepage

`get_summary` becomes the single entrypoint that presents *observation + heuristic + memory* for a repo. `memory_insights` is populated by default.

**Contract changes:**

- `SummaryService::summarize` gains a `MemoryStore` dependency (or receives pre-loaded memories via a parameter; existing pattern depends on how `recall_longterm` threads it).
- The memory-loading path should share the same lazy-decay rule `recall_longterm` uses before ranking, so "current importance" means the same thing in both tools.
- `MemoryInsight` gains a `grounding_status: GroundingStatus` field mirroring `recall_longterm`'s output (`Confirmed` / `Contradicted` / `Unverifiable` / `Unchecked`). Re-using the same enum keeps the envelope semantics consistent.
- A default selection rule decides which memories surface at each tier.
- Envelope can stay `unaudited` in the first cut; promotion to `derived` is a downstream spec once memory-insight cataloguing lands.

**What this costs:** `SummaryService` now depends on memory state. The summary surface grows by one selection rule that has to be defended. The three-tier fanout in `summary.rs` MCP handler needs tier-dependent logic for how many memories to include.

**What this buys:** The "first agent workflow" loop in `opinion_pool_006.md` §1 closes in one tool call. Doc 07 §"Known Mismatches" item 1 can be replaced with a narrower, truthful statement: `get_summary` surfaces selected memory insights with per-item `groundingStatus`. The product's most distinctive idea (three truth layers in one honest interface) is expressed at the natural entrypoint.

### Option B — Observation-First Front Door

`get_summary` stays deliberately silent on memory. The tool's job is: scan + profile + health + graph + recommendations. Memory queries go through `recall_longterm`.

**Contract changes:**

- Remove `memory_insights` from `RepoSummary`, OR keep the field but document it as always-empty (migration-softened).
- Remove the `has_memory` branching in `summary.rs:93` and the `"hasMemoryInsights"` serialization — the envelope becomes observation+heuristic only, and grade promotion to `derived` follows the `get_profile` precedent.
- The MCP tool's description text changes to explicitly point agents at `recall_longterm` for memory.

**What this costs:** The first agent workflow needs two tool calls to answer "what is this repo *and* what prior memory matters?" The homepage metaphor loses its clearest candidate; agents have to discover `recall_longterm` themselves.

**What this buys:** Tighter, truer tool surface. Faster path to `evidenceGrade: derived` because the mixed-layer blocker disappears. `recall_longterm` becomes unambiguously the memory entry point. Fewer product decisions to defend per tool.

## Recommendation

**Option A, narrow version.**

Rationale:

1. The product's declared identity is the three-truth-layers idea (doc 07, `opinion_pool_003.md` §2). An entrypoint tool that silently hides the interpretation layer makes that identity harder to *feel* on first contact — exactly the failure mode `opinion_pool_006.md` was written against.
2. `recall_longterm` is already the correct tool for directed memory queries. What it cannot do is answer the ambient question "is there anything I should know before I look at this repo?" — which is `get_summary`'s natural job.
3. Option B's main win is the `derived` grade path, but that path already has a spec precedent (`2026-04-15-get-profile-derived-promotion-design.md`) and can be pursued for `get_summary` later without changing the tool's shape. The grade is a downstream cataloguing problem, not a fork-defining one.
4. The "narrow" qualifier matters: we populate `memory_insights` with one specific selection rule, add one field to `MemoryInsight` (`grounding_status`), and do not expand the tool's surface beyond that. Scope creep is the failure mode to defend against.

### Selection rule for Option A

Tier-dependent, anchor-resolution-filtered, importance-ranked:

- **Compact tier**: top 3 memories with at least one anchor that resolves against the current pack (file path or qualified symbol), ranked descending by current `importance`.
- **Standard tier**: top 5, same filter.
- **Full tier**: up to 20, no hard filter, ranked by `importance × anchor_resolution_bonus` where the bonus is `1.5` if at least one anchor resolves in the current pack and `1.0` otherwise.

Here, "current `importance`" means the in-memory `importance` value after applying the same lazy-decay step `recall_longterm` uses when `last_decay_run` is stale. This spec intentionally reuses an existing model field; it does not introduce a separate `decay_score` concept.

Each surfaced memory includes its `grounding_status`, populated by running the same grounding check `recall_longterm` uses (`crates/repolens-app/src/grounding.rs`). Memories without anchors get `Unchecked` — same semantics as `recall_longterm`, no divergence. Symbol-only anchors are eligible in compact and standard tiers if they resolve, because the filter is defined in terms of grounding, not raw file-path intersection.

Rationale for these specific numbers: compact tier is the default for agent first-contact (latency-sensitive), 3 is enough to seed context without overwhelming; standard and full tiers scale proportionally. Anchor-resolution filtering in compact/standard tiers prevents memory drift from dominating the summary when anchors no longer resolve in the pack. Full tier intentionally relaxes the filter because a full-tier caller has asked to see everything.

## Out of Scope

- Promoting `get_summary`'s envelope to `derived`. That is downstream work modeled on `get_profile`'s promotion spec, contingent on memory-insight cataloguing that does not yet exist in doc 06 §5.
- Adding new `MemoryInsight` fields beyond `grounding_status`. The shape is already broader than `recall_longterm`'s inline-return shape; widening it further risks the same "bare-payload" cataloguing debt that blocks other mixed tools per doc 07 row "Pure-observation tool grading."
- Wider grounding triggers (on-scan, high-stakes). Those remain in the deferral list from `opinion_pool_006.md` §6.
- Changing `recall_longterm`. Option A does not compete with it; the two tools answer different questions.

## Next Steps After Decision

If the recommendation is accepted:

1. Write the TDD-driven implementation plan for `memory_insights` population with the selection rule above. Location: `docs/superpowers/plans/2026-04-18-get-summary-memory-population.md`.
2. Extend `MemoryInsight` with `grounding_status` in one commit (schema change, single-test).
3. Implement `SummaryService` memory-loading and selection in a second commit (behavioral change, multi-tier tests).
4. Update doc 07 §"Known Mismatches" item 1 from the current overbroad wording to a narrower shipped statement such as: "`get_summary` can surface selected memory insights with per-item `groundingStatus`."
5. The eval question "what prior memory or convention matters for this change?" from `opinion_pool_006.md` §5 is answered by a single `get_summary` call. Verify via dogfood snapshot.

If the recommendation is rejected in favor of Option B:

1. Write a migration plan that removes `memory_insights` from `RepoSummary` or deprecates it.
2. Update `recall_longterm`'s tool description to claim the memory entry-point role explicitly.
3. The eval question cited above is answered by a `get_summary` + `recall_longterm` pair; the dogfood snapshot verifies the two-tool flow.

## Dissent / Uncertainty

- The anchor-resolution filter in compact/standard tiers may surface too few memories on pack rescans where anchors go stale. If so, fall back to top-N by `importance` with `grounding_status` making the staleness visible. This is a recoverable tuning problem, not a design flaw.
- "Narrow Option A" depends on team discipline to not expand the selection rule into a configurable policy surface. If configurability creeps in, this spec should be revised rather than silently stretched.
- If embeddings arrive earlier than `opinion_pool_006.md` §6 expects, the selection rule here becomes an obvious candidate for hybrid retrieval (embedding-based relevance + graph-based verification). That is a future rewrite, not a reason to delay Option A.
