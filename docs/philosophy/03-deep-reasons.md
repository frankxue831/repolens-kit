# Deep-Down Reasons Behind Our Design Choices

Authority: Supporting
Lifecycle: Living

Why RepoLens is built the way it is, traced to root causes.

## The Central Thesis

> The bottleneck in AI-assisted coding is not intelligence — it is perception.

AI agents today are like brilliant surgeons operating blindfolded. They have the reasoning ability to make complex changes, but they lack the perceptual infrastructure to see the codebase as a connected, evolving system.

## Why "Observe, Don't Judge"

### The Problem It Solves
Every static analysis tool (SonarQube, CodeClimate, etc.) produces aggregate quality scores. These scores embed opinions about what "good" means. An AI agent consuming a score of "72% healthy" has no way to reason about *why* — it can only trust or ignore the number.

### The Deeper Reason
RepoLens's consumer is an AI reasoning engine. Reasoning engines need **facts and evidence**, not pre-digested conclusions. When you give an agent "File X has 12 incoming dependencies" (fact) vs. "File X has high coupling" (judgment), the agent can do more with the fact — it can combine it with other facts, weigh it against the specific task, and form its own contextual judgment.

This is directly grounded in Endsley's SA model: Level 2 (comprehension) must be built *by the agent* from Level 1 (perception) data. If the tool pre-computes comprehension, the agent can't adapt it to context.

### The Design Principle
Surface discrete signals and measurements. Let the consumer decide what matters. Never aggregate away information that could be contextually relevant.

## Why "Memory Is Alive"

### The Problem It Solves
LLM-based agents are fundamentally stateless. Each conversation starts from zero. For long-lived work (multi-day development, ongoing maintenance), this means constantly re-establishing context — and losing hard-won understanding between sessions.

### The Deeper Reason
Ebbinghaus (1885) showed memory decays exponentially. But Anderson and Schooler (1991) proved this isn't a bug — it's adaptive. Memories decay at rates reflecting how likely they are to be needed again. Frequently-accessed knowledge stabilizes. Unused knowledge fades.

Human expert programmers maintain a **situation model** (Von Mayrhauser and Vans, 1995) — a persistent, evolving understanding of what the code means in its domain. This model:
- Persists across sessions (you don't re-learn the architecture every Monday)
- Updates incrementally (new knowledge integrates with old)
- Ages naturally (last week's debugging session is less relevant than last month's architecture decision)
- Can be superseded (the old auth system was replaced, but the memory of *why* it was replaced persists)

RepoLens's memory system — five node types, three edge types, exponential decay with access-based floor — is a mechanical implementation of this cognitive process.

### The Design Principle
Model knowledge the way experts actually retain it: with recency weighting, retrieval reinforcement, explicit supersession, and honest uncertainty about what's still valid.

## Why "Match the Consumer's Budget"

### The Problem It Solves
LLMs have finite context windows, and the "Lost in the Middle" research proves that even within those windows, more data can mean *worse* performance. An agent needs the right information at the right resolution, not everything at once.

### The Deeper Reason
Bateson (1972): "A map at 1:1 scale is useless." The value of a map is in what it *omits*. Fairbanks (2010) applied this to software architecture: models should be "just enough."

Expert programmers use **opportunistic strategy switching** (Von Mayrhauser and Vans): sometimes they need the 10,000-foot view, sometimes they need a single function signature. They switch fluidly between abstraction levels. A tool that only provides one level forces a single comprehension strategy.

### The Design Principle
Provide self-contained views at every resolution level. Each tier (compact/standard/full) must be independently useful, not require prior tiers. Let the consumer enter at any level.

## Why "Evidence Over Confidence"

### The Problem It Solves
AI agents are notoriously confident even when wrong. They generate plausible code that contains subtle bugs, assert facts about APIs that don't exist, and rarely say "I'm not sure." The developer complaint: "It doesn't know what it doesn't know."

### The Deeper Reason
If RepoLens is the perception layer agents rely on, its quality has ethical weight. A wrong map isn't a judgment, but it can cause harm if the traveler trusts it. This creates an obligation: every piece of information RepoLens produces must carry its provenance and confidence.

Korzybski (1933): the danger is confusing the map with the territory. When an agent treats a memory node (interpretation) with the same confidence as a graph edge (observation), it makes decisions on shaky ground without knowing it.

### The Design Principle
Every claim must be traceable to its source. Observations come from code (high confidence). Heuristics come from thresholds applied to observations (medium confidence — the thresholds are design opinions). Memories come from agent/human input (variable confidence — tracked via decay, access count, and provenance).

## Why MCP as the Interface

### The Deeper Reason
RepoLens is not a developer tool — it's an AI perception layer. Building it as a CLI-only tool would couple it to human workflows. Building it as an IDE plugin would couple it to one vendor.

MCP is the universal protocol for AI agent tooling (5,800+ servers, 300+ clients as of 2026). By being an MCP server, RepoLens becomes a pluggable perception module for *any* AI system. This matches the product identity: we don't build AI coding tools, we build the **eyes** that any AI coding tool can use.

## Why Immutable Packs + Mutable Memory

### The Deeper Reason
These model two fundamentally different kinds of truth:

**Structural truth** (what the code *is*): objective, extractable, frozen at a point in time. The right representation is an immutable snapshot — comparable across time, self-contained, no dangling references.

**Interpretive truth** (what the code *means*): subjective, evolving, subject to revision. The right representation is a living graph — with decay, supersession, contradiction tracking, and provenance.

Confusing these two kinds of truth is the root cause of many tooling failures. A type system tells you the code's structure (fact). A code review tells you the code's intent (interpretation). They're both valuable, but they have different confidence levels and different shelf lives.

RepoLens keeps them architecturally separate so the consumer always knows which kind of truth they're looking at.
