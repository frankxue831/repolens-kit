# Current Boundaries: Observation vs. Interpretation in RepoLens

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-04. Preserved as the authoritative record of its topic as of that date.

Precise mapping of where the current codebase sits on the observation-interpretation-comprehension spectrum.
Analyzed 2026-04-04 against the codebase at commit `f8b6ee9`.

> **Point-in-time snapshot — do not treat as current.** This document is a frozen 2026-04-04 analysis. Several specifics have drifted since then: the MCP tool set grew from the 16 listed below to 26 (adding `find_cycles`, `find_dead_code`, and eight wiki tools); `recall_longterm` is listed here as "Observation" but is interpretation layer; and the "Where Comprehension Would Start" list names *"Memory that self-validates against observations"* as a future indicator while Phase 1 grounding-on-recall actually ships that behaviour today (see `docs/superpowers/specs/2026-04-05-grounded-memory-design.md`).
>
> For **current-HEAD state** (shipped product behaviour vs. direction of travel), read [07-current-state-vs-direction.md](07-current-state-vs-direction.md). That doc is the living reference; this one is preserved for its 2026-04-04 snapshot value and the reasoning that led to doc 06.
>
> **Terminology note:** this doc uses "Rule-Based Interpretation" for what [06-observation-invariants-and-grounding-eligibility.md](06-observation-invariants-and-grounding-eligibility.md) calls the **Heuristic** layer. The two terms refer to the same thing — hardcoded thresholds over observations — and doc 06 standardized on "Heuristic" to free the word "Interpretation" for agent-authored memory/wiki content.

## Three Layers in Current Code

| Layer | What It Does | Nature |
|-------|-------------|--------|
| **Observation** | Extracts facts from code | Mechanical, deterministic |
| **Rule-Based Interpretation** | Applies fixed thresholds to observations | Deterministic, but embeds design opinions as compile-time constants |
| **Comprehension** | Synthesizes meaning from context | **Does not exist in RepoLens today** |

## Observation Layer (Pure Extraction)

These operations extract facts without any judgment:

- File/directory presence/absence detection
- Line-of-code summation
- Symbol extraction (struct, fn, trait, enum names and visibility)
- Relationship extraction (imports, implements)
- Graph construction (nodes = files, edges = import relationships)
- Degree counting (in/out edges per node)
- Time difference calculation
- Ratio calculations (test-to-source, resolution-rate, module LOC share)
- Language detection from file extensions

## Rule-Based Interpretation Layer (Codified Heuristics)

These operations apply hardcoded thresholds — each one is a frozen design opinion:

### FitnessScorer (`summary.rs:278-317`)

| Component | Formula | Judgment |
|-----------|---------|----------|
| `resolution_rate` | resolved / total | Pure ratio — no threshold |
| `coupling_quality` | 1.0 - (max_in / count) | Assumes optimal = evenly distributed in-degrees |
| `noise_reduction` | included / total | Pure ratio — no threshold |
| `overall` | 0.5*res + 0.3*coupling + 0.2*noise | Hardcoded weight priorities |

### HealthAnalyzer (`health.rs:90-169`)

| Signal | Threshold | Nature |
|--------|-----------|--------|
| `dominant-module` | module_share > 0.8 | 80% is a judgment call |
| `no-lockfile` | has_build AND !lockfile | Presence check (observation) |
| `no-entry-points` | count == 0 | Cardinality (observation) |
| `high-framework-count` | count > 5 | 5 is a judgment threshold |
| Risk level | Nested conditionals: >2, >3 | Heuristic combination |

### RecommendationBuilder (`summary.rs:326-495`)

| Rule | Threshold | Nature |
|------|-----------|--------|
| `high-coupling-file` | max(2.0 * avg, 3.0) | Two magic numbers |
| `low-test-ratio` | < 0.1 AND source > 5 | 10% floor, 5-file minimum |
| `large-file` | > 500 LOC | Arbitrary cutoff |
| `dominant-module` | > 0.8 share | Same as health |
| `low-resolution-rate` | < 0.5 | 50% is judgment |

### MemoryDecayEngine (`decay.rs:20-201`)

The decay engine's tunable values are the half-life, the access floor (two coefficients), the multipliers for `Supersedes` edges, `Contradicts` edges and `Contradicted` grounding, the `Confirmed` grounding floor, and the pruning threshold. All of them are design judgments, not measured values.

## Comprehension Layer (Does Not Exist)

RepoLens never:
- Generates context-dependent language ("Given this is a library, consider...")
- Adapts thresholds based on domain or project type
- Performs comparative reasoning ("Compared to similar repos, your test ratio is...")
- Does inductive learning across sessions ("This pattern emerged over time...")
- Infers product-level meaning from code patterns
- Generates hypotheses about architectural intent

## MCP Tools: Where Each Sits

| Tool | Primary Layer | Notes |
|------|--------------|-------|
| `discover_packs` | Observation | File system enumeration |
| `read_file_content` | Observation | Raw file access |
| `search_code` | Observation | Pattern matching |
| `get_profile` | Observation | Build system / framework detection |
| `get_graph` | Observation | Pre-computed graph retrieval |
| `get_callers` | Observation | Edge filtering |
| `get_dependencies` | Observation | Edge filtering |
| `get_architecture` | Observation | Grouping + degree counting |
| `get_conventions` | Observation | File content retrieval (CLAUDE.md, rules) |
| `analyze_health` | Interpretation | Threshold-based signal generation |
| `get_summary` | Interpretation | Fitness scores, recommendations, truncation |
| `get_impact` | Observation | BFS traversal (mechanical) |
| `compare_summaries` | Observation + Interpretation | Delta computation + heuristic scoring |
| `memorize_insight` | Passive storage | Stores agent-provided interpretation |
| `recall_longterm` | Observation | Retrieves with mechanical decay scoring |
| `forget_insight` | Passive deletion | Removes node |

## Key Finding

**All "interpretation" in RepoLens is deterministic, parameter-driven, and compile-time fixed.** Given the same input and the same thresholds, the output is identical. There is no adaptive inference, no context-dependent reasoning, no open-ended synthesis.

This means the current system is: **Observation + Codified Heuristic**, with a clear gap where **Comprehension** would begin.

## Where Comprehension Would Start

If RepoLens ever crosses into comprehension, these would be the indicators:
1. Thresholds that adapt based on project context
2. Recommendations that reference domain-specific knowledge
3. Memory that self-validates against observations
4. Pattern detection that generates new interpretive claims
5. Cross-session inductive learning
