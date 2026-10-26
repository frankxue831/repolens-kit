# Background and Mission

Authority: Canonical
Lifecycle: Living

## 1. Background — Why RepoLens Exists

Modern AI coding agents (Claude Code, Cursor, Copilot, Devin, etc.) are extremely powerful at generating code at super-human speed. However, they suffer from one fundamental defect: they are **intelligent but nearly blind**.

They can reason brilliantly about the code they can see, but they have almost no coherent, persistent, or accurate understanding of the codebase as a whole system. This leads to the most common real-world failures:

- Missing architectural conventions and product-level patterns
- Reinventing existing helpers or libraries
- Making changes that look syntactically correct but break downstream dependencies or product logic
- Generating code that creates significantly more logic errors, security issues, and architectural debt (studies show 1.75x-2.74x worse than human-written code in several dimensions)

The naive fix — "just give the agent a bigger context window or dump more files" — does not work. Research such as Stanford/Berkeley's "Lost in the Middle" paper and context-window degradation studies proves that more raw data often reduces, rather than improves, true understanding.

At the deepest level, we realized the real bottleneck in AI-assisted software development is **no longer intelligence — it is perception and structured cognition**. Expert human engineers maintain a rich, evolving mental model of the entire system (Mica Endsley's Situation Awareness Levels 1-3). Current agents are mostly stuck at Level 1.

RepoLens was created precisely to close this gap.

## 2. Current Mission

RepoLens's mission is to be the **structured perception layer and epistemic infrastructure** for AI coding agents.

Its single most important job is to enable agents to:

- Build accurate, contextual, multi-resolution, and continuously evolving understanding of codebases
- Reason and act based on strong, grounded evidence instead of weak or partial information (**Evidence Over Confidence**)
- Maintain living memory and situational awareness across long-term, multi-session work

RepoLens does **not** write code, judge code quality, or act as a guardrail. It exists solely to give agents "eyes to see code" — providing multi-resolution maps of technical repo structure, plus explicit infrastructure for agents and humans to record, ground, and maintain higher-level product structure with provenance.

## 3. Four Core Principles

### Observe, Don't Judge
Present structured facts. Never pretend to know what matters more than the consumer does. Health uses discrete signals, not aggregate scores. Fitness measures RepoLens's own comprehension quality, not code quality. Recommendations are suggestions, not verdicts.

### Memory Is Alive
Understanding a codebase is a living process, not a snapshot. Knowledge ages, gets replaced, sometimes contradicts itself. The memory system models this honestly with exponential decay, supersession edges, and contradiction tracking — rather than pretending each scan starts fresh.

### Match the Consumer's Budget
Information should be available at the resolution the consumer needs. Tiered summaries (2KB/8KB/25KB), scoped vs. unscoped architecture views, and the 26-tool zoom hierarchy let agents enter at any level without forcing them to filter down from everything or build up from nothing.

### Evidence Over Confidence
Every claim RepoLens makes should be traceable to its source. Observations come from code. Heuristics come from thresholds applied to observations. Memories come from agent/human input with timestamps, access counts, and decay scores. The consumer should always be able to assess confidence.

## 4. Architectural Philosophy

**Immutable Packs** capture structural truth — what the code *is* at a point in time. Each pack is self-contained, comparable, and frozen.

**Mutable Memory** captures interpretive truth — what agents/humans *believe* about the code. Memory decays, evolves, can be superseded or contradicted.

These two systems model two different kinds of truth, and keeping them distinct is a core design decision.
