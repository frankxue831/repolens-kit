# 08 — The Four Orthogonal Axes

Authority: Canonical
Lifecycle: Living

**Status**: Canonical reference
**Date**: 2026-04-18
**Depends on**: [00-status.md](00-status.md), [01-background-and-mission.md](01-background-and-mission.md), [05-structure-and-tensions.md](05-structure-and-tensions.md), [2026-04-11-epistemic-envelope-design.md](../superpowers/specs/2026-04-11-epistemic-envelope-design.md)

## Why This Doc Exists

RepoLens docs use several overlapping words such as "layer", "tier", "confidence", "grounding", and "importance". These are not interchangeable. They describe different coordinates in the system, and the canon needs one place that says which coordinate each concept belongs to.

This doc is that reconciliation. It establishes four orthogonal axes:

- storage tier
- claim-production layer
- trust state
- retention / ranking state

It also clarifies how `provenance` sits next to those axes as a citation mechanism rather than replacing any of them.

## The Four Axes

| Axis | Canonical name | Values / fields | Meaning |
|---|---|---|---|
| A | **Storage tier** | `pack / memory / wiki / repo-filesystem` | Which storage tier the response draws from |
| B | **Claim-production layer** | `knowledgeLayer`: `observation / heuristic / interpretation` | How the claim was produced |
| C | **Trust state** | `confidence`, `evidenceGrade`, grounding/staleness/freshness | How strongly RepoLens can vouch for the claim, and whether it remains grounded/current |
| D | **Retention / ranking state** | `importance`, access count, decay, ranking weight | How aggressively the system should retain, recall, or rank the claim |

Axis A names where the artifact comes from. RepoLens produces three derived layers — `pack`, `memory`, and `wiki` — and also reads the underlying `repo-filesystem` directly for some tools. Axis B names claim production: whether a response is an `observation`, `heuristic`, or `interpretation`. Axis C names trust: producer confidence, audit/derivation state, and freshness or grounding status. Axis D names retention and ranking: whether something should persist, surface, or sink over time.

`importance` belongs under retention/ranking, not trust. Likewise wiki article `confidence` belongs under retention/ranking unless and until the wiki trust model is separately reconciled; that article field currently functions as a retrieval/ranking input, not as the MCP envelope's trust-state `confidence`. `provenance` is a structured citation adjacent to, but not identical with, storage tier: storage tier names the layer, while provenance names the exact source object, path, or identifier that backed the response.

## How the Axes Combine

An MCP response has one coordinate on each axis, even when the wire shape surfaces some axes more explicitly than others.

| Tool | Storage tier | Claim-production layer | Trust state | Retention / ranking state |
|---|---|---|---|---|
| `discover_packs` | `repo-filesystem` | `observation` | `confidence: mechanical` | not applicable in the response envelope |
| `read_file_content` | `pack` | `observation` | `confidence: mechanical` | not applicable in the response envelope |
| `analyze_health` | `pack` | `[observation, heuristic]` | `confidence: codified`, `evidenceGrade` per family rollout | not applicable in the response envelope |
| `recall_longterm` | `memory` | `[interpretation, observation]` when grounded | `confidence: variable`, plus grounding/staleness on the recalled memory | `importance`, access count, decay all affect recall ordering |
| `wiki_search` | `wiki` | `interpretation` | envelope trust remains separate from article-level ranking metadata | ranking weight and article metadata determine result ordering |
| `get_conventions` | `repo-filesystem` | `observation` | `confidence: mechanical` | not applicable in the response envelope |

Two rules matter more than any single example:

1. Storage tier does not dictate claim-production layer. `analyze_health` reads from packs but emits both observations and heuristics.
2. Trust state does not dictate retention/ranking state. A stale or low-confidence claim might still rank highly because of access history or authored importance, and a highly trusted observation might have no retention behavior at all.

## Provenance Is Adjacent, Not an Axis Replacement

Every response should still carry `provenance` as the structured citation to its backing source: `packExtraction`, `repositoryRead`, `memoryNode`, `memoryStore`, `wikiArticle`, `wikiIndex`, and related variants. This is adjacent to storage tier because both talk about where the response came from, but they answer different questions. Storage tier answers "which layer?" Provenance answers "which exact object/path/id within that layer?"

This distinction matters when reconciling docs. `repo-filesystem` is the storage-tier value for tools that read raw repository content, while `Provenance::RepositoryRead` is the citation form that tells the consumer which repository path or source was actually read. Same neighborhood, different job.

## Mapping Legacy Framings

**The "Packs vs Memory" framing** is an early slice of axis A. It became incomplete once wiki became a first-class storage tier and once repo-filesystem reads were treated explicitly.

**The old "Three Truth Layers" and "Three Layers of Truth" tables** collapse multiple axes together. They usually mix storage tier with claim-production hints, blur trust into the row labels, and omit retention/ranking altogether. Those framings remain useful historical summaries, but they are not the canon.

**The envelope-centric framing** (`knowledgeLayer`, `confidence`, `evidenceGrade`, `provenance`) is also incomplete by itself. It covers claim production, trust, and citation well, but it does not tell the reader where retention and ranking concepts such as `importance`, decay, or search weight belong.

## When to Cite Which Axis

- Cite **storage tier** when the question is where RepoLens stores or reads the relevant artifact.
- Cite **claim-production layer** when the question is what kind of claim the response contains.
- Cite **trust state** when the question is how strongly RepoLens can vouch for the claim or how stale/grounded it is.
- Cite **retention / ranking state** when the question is why some memory or wiki content persists, decays, or surfaces more prominently than another.

A doc that crosses more than one of those concerns should cite this doc rather than inventing a new "layer" vocabulary.

## What This Doc Does Not Do

- Does not change the MCP envelope. `knowledgeLayer`, `confidence`, `evidenceGrade`, `provenance`, and `data` stay as they are.
- Does not change storage layout. Packs remain in `pack-*/`, memory in `memory/`, wiki in `wiki/`, and repo-filesystem reads remain read-only.
- Does not retire legacy framings from historical docs. It reclassifies them under the four-axis canon.
