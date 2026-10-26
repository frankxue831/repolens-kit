# Structure, Tensions, and Three Paths Forward

Authority: Canonical
Lifecycle: Living

Analysis of the open question: "A good product must have a good structure from some perspective."

## Three Meanings of "Structure"

### Structure 1: The Structure of the Codebase (What RepoLens Already Sees)

Technical topology: files, symbols, dependencies, modules, import graphs, build systems. Extractable mechanically. Captured as immutable pack snapshots. Traversable via graph tools.

This is the **map of the territory**.

### Structure 2: The Structure of the Product (What RepoLens Doesn't Yet See Clearly)

Conceptual topology: why does this module exist? What business domain does it serve? What product decisions shaped this architecture? Where are trade-off boundaries?

This structure is **not mechanically extractable from code**. A payment module and a user module might have identical technical structure but radically different product significance. The code doesn't tell you "this module exists because of PCI compliance requirements."

This meaning lives in decisions, conventions, trade-offs, and patterns — which is to say, **it lives in the memory system**.

### Structure 3: The Structure of RepoLens Itself (Meta-Question)

Does RepoLens have a coherent internal structure that makes its own capabilities legible? Can a consumer look at the tool suite and understand: "These tools see technical structure. These tools see product structure. These tools see how both evolve."

## Identified Tensions

### Tension A: "Observe, Don't Judge" vs. Product-Level Meaning

At the technical level, observation is clean. "File A imports file B" is a fact.

At the product level, observation requires interpretation. "This module handles payment processing" is already a judgment. The memory node types — `Decision`, `Convention`, `TradeOff`, `Pattern` — are inherently interpretive. They're claims about what code *means*, not facts extracted from code.

**Current resolution**: RepoLens *stores* interpretations but doesn't *generate* them. It's a medium for memory, not a source of opinion. This preserves "Observe, Don't Judge" because RepoLens is passive with respect to product meaning.

**Question**: Is this enough? If memory quality depends entirely on what agents put in, garbage-in/garbage-out is the failure mode.

### Tension B: Who Authors Product Structure?

Technical structure is authored by the compiler, import system, file system. Objective.

Product structure is authored by humans — through decisions, conversations, design docs, commit messages, and implicit choices. Currently enters RepoLens only through:
1. Agent calls to `memorize_insight` (agent-authored interpretation)
2. `get_conventions` reading CLAUDE.md / rules files (human-authored documentation)

Neither scales. Agent memory is only as good as the agent's understanding. Human documentation is chronically incomplete and outdated (Parnas's "Software Aging").

### A Hidden Principle: Legibility of Provenance

Every piece of information RepoLens produces can be traced back to its source:
- **Observation**: traceable to code
- **Codified heuristic**: traceable to threshold applied to observation
- **Memory**: traceable to specific `memorize_insight` call with timestamp, access count, decay score

At no point does RepoLens produce knowledge whose origin is opaque. This is critical for "Evidence Over Confidence" but also for the product structure question — if product-level claims enter the system, their provenance must be equally clear.

## Three Paths Forward

### Path A: Stay Passive (Memory as Dumb Store)

RepoLens stores whatever agents tell it. Memory nodes have types but RepoLens doesn't validate, cross-reference, or flag contradictions. Decay is purely time-based.

**Pros**: Clean, simple, preserves "Observe, Don't Judge" perfectly.
**Cons**: Memory quality depends entirely on the agent. A frequently-accessed but *wrong* memory accumulates a high access floor and persists indefinitely.

**This is where we are today.**

### Path B: Active Validation (Memory with Grounding)

RepoLens stores agent interpretations but cross-references them against observations. If an agent memorizes "module X has no external dependencies" and the graph shows 5 outgoing edges, RepoLens flags the contradiction. If a Convention memory says "we use Result<T, AppError> everywhere" and the next scan finds divergence, the memory's importance decays faster.

**Pros**: Product-level memory becomes grounded in technical reality. Provenance strengthens.
**Cons**: Crosses into comprehension — RepoLens starts reasoning about the relationship between observations and interpretations.

**Key insight**: This crosses the line in a way that **remains legible**. The agent sees: "Memory node X was flagged because observation Y contradicts it." Provenance is traceable. The system validates existing interpretations against evidence — it doesn't generate new ones.

**Concrete mechanism**: A Grounding layer between Packs and Memory. After each scan:
1. Check active memory nodes against new pack observations
2. Add `groundedBy` or `contradictedBy` edges linking memory to pack observations
3. Accelerate decay for contradicted memories
4. Surface grounding status when agents call `recall_longterm`

### Path C: Active Inference (Memory with Synthesis)

RepoLens infers product structure from patterns. Detects domain boundaries by clustering. Infers conventions from code patterns. Generates architectural hypotheses.

**Pros**: Powerful. Closes the gap for undocumented codebases.
**Cons**: Breaks "Observe, Don't Judge" fundamentally. Provenance becomes murky. The system generates opinions, not observations.

**Assessment**: Premature. Requires domain knowledge you can't hardcode. It's a fundamentally different product — an AI that understands code, rather than a perception tool that helps AIs understand code. If pursued, should be a deliberate evolution, not drift.

## What "Good Structure" Means for RepoLens

### Conceptual Integrity (Frederick Brooks)

A product has conceptual integrity when every part feels like it came from one coherent mind — when the user can predict how an unseen feature works because the internal logic is consistent.

For RepoLens, conceptual integrity means:

1. **The tool hierarchy has a legible logic.** Tools organized along two axes: resolution level (overview -> architecture -> file -> symbol) and knowledge type (structural observation -> interpretive memory -> projective analysis).

2. **The boundary between observation and interpretation is explicit.** Packs are observations. Memory is interpretation. An agent (and a human reviewing the agent) should always know which kind of knowledge they're looking at.

3. **The system acknowledges what it can't see.** When there's no memory, no conventions, no human-authored context for an area, RepoLens should say so rather than silently returning only technical data. The absence of product structure is itself a signal.

## Three Layers of Truth

> Historical — Superseded 2026-04-18 by [08-taxonomy.md](08-taxonomy.md). Read [08-taxonomy.md](08-taxonomy.md) for current guidance on this section's topic; the rest of this doc remains canonical.

This section is retained for historical context. The table collapses two axes (knowledgeLayer and confidence) into one row per tier and elides the wiki storage tier entirely. The canonical reconciliation is [08-taxonomy.md](08-taxonomy.md): four orthogonal axes — storage tier, claim-production layer, trust state, and retention/ranking state. Those axes are not interchangeable labels for the same thing.

| Layer | Captures | Source | Confidence | Mutability |
|-------|----------|--------|------------|------------|
| **Observation** (Packs) | What the code *is* | Extracted from source | High — mechanical | Immutable per scan |
| **Heuristic** (Health, Fitness) | What observations *might imply* | Rule-based thresholds | Medium — codified opinion | Changes only when rules change |
| **Interpretation** (Memory, Wiki) | What agents/humans *believe* | Agent calls, human docs, compiled articles | Variable — depends on source | Decays (memory) or re-grounded on anchor staleness (wiki) |

Good structure = the four axes in doc 08 are **always distinguishable** to the consumer. "Layer" in this legacy table is imprecise; prefer "axis" and the specific field names.

## The Implication for Humans

If agents see codebases through RepoLens, and humans trust agents, then humans are *transitively trusting RepoLens's perception*. This creates accountability:

- Wrong technical observation (bad graph) -> agent makes bad change -> human approves because agent seemed confident
- Stale memory (superseded decision that hasn't decayed enough) -> agent acts on outdated interpretation

This doesn't violate "Observe, Don't Judge" — but it means the **quality of observation has ethical weight**. A wrong map isn't a judgment, but it causes harm if the traveler trusts it.

This may be why "good structure" matters not just as a feature requirement, but as a **quality standard for perception itself**. A well-structured perception system is one where the consumer can always answer: What am I looking at? How fresh is it? How complete is it? What's missing?

## Suggested Direction

The four principles are strong. The question isn't whether to add a fifth — it's whether to deepen the second one:

> **"Memory Is Alive"** could evolve to mean not just "memory decays over time" but **"memory stays honest against evolving evidence."**

This would naturally lead toward Path B (grounded memory) without requiring a new principle or breaking existing ones.

## Open Questions for Decision

1. Should memory and observation interact? (The Grounding Question)
2. Can the consumer always trace any claim to its source? (The Legibility Question)
3. Is passive memory sufficient for long-lived projects, or will ungrounded memory degrade agent performance over time? (The Quality Question)
4. If Path B, what triggers grounding — every scan, on recall, or on demand? (The Mechanism Question)
5. Does the tool hierarchy need restructuring to make the three layers visually/conceptually distinct? (The UX Question)
