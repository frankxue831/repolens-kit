# Research Landscape

Authority: Supporting
Lifecycle: Reference

Findings from exploration on 2026-04-04. Covers industry problems, academic foundations, and competitive context.

## 1. The Problem Space: Why AI Agents Fail on Real Codebases

### Context Window Paradox
- Stanford/Berkeley's "Lost in the Middle" (Liu et al., 2023): Models show 20-25% accuracy variance depending on where information sits in context. U-shaped attention — beginning and end get attention, middle gets lost.
- Model correctness drops around 32K tokens even for models claiming much larger windows (VentureBeat, 2025).
- Practical implication: more raw code in context often *reduces* understanding.

### Known Failure Modes
- **Hallucinated APIs**: Agents generate calls to functions that don't exist in the codebase
- **Convention violations**: The loudest developer complaint — agents miss project-specific patterns, helper libraries, UX components
- **Cascade failures**: Multi-file changes get 80% of files right, miss edge cases in tests, config, serialization
- **Build system blindness**: Agents don't understand workspace structure, dependency constraints, module boundaries
- **Confident incorrectness**: AI code creates 1.75x more logic errors, 1.57x more security findings, 2.74x more XSS vulnerabilities vs. human-written code (The Register, 2025)

### The Core Insight
> "Context window isn't the real problem — understanding is." Developers report agents don't truly "understand" code the way a human does after weeks with a codebase. They lack implicit knowledge: why decisions were made, what was tried and failed, what constraints exist.

## 2. Academic Foundations

### Situation Awareness (Endsley, 1995)
Three levels of understanding in complex systems:
- **Level 1 — Perception**: Detect elements (what files exist, what changed)
- **Level 2 — Comprehension**: Understand meaning (why is this structured this way, what conventions apply)
- **Level 3 — Projection**: Predict future states (what breaks if I change this)

Current AI agents are mostly stuck at Level 1. RepoLens's 26 tools map directly onto all three levels.

Ko, DeLine, and Venolia (2007) at Microsoft Research confirmed developers spend enormous time seeking information to maintain SA, with needs mapping to Endsley's levels.

Gasparic, Murphy, and Petrich (2017) found most developer tools only support Level 1, few support Level 2, almost none support Level 3.

### Program Comprehension
- **Von Mayrhauser and Vans (1995)**: Expert programmers switch fluidly between top-down (domain knowledge), bottom-up (reading code), and situation model (functional understanding). The situation model is the glue.
- **Littman et al. (1987)**: Systematic strategists build better mental models; as-needed strategists are faster but make more errors. Tools should support both without forcing either.
- **Storey, Fracchia, and Muller (1999)**: Effective tools must support multiple abstraction levels simultaneously.

### Memory and Forgetting
- **Ebbinghaus (1885)**: Memory retention drops exponentially. Direct theoretical basis for decay-based persistence.
- **Anderson and Schooler (1991)**: Memory decay mirrors environmental statistics — forgetting is adaptive, not a flaw.
- **Bjork and Bjork (1992)**: Storage strength vs. retrieval strength — old knowledge persists but becomes harder to access. Retrieval resets decay.
- **Proactive/retroactive interference** (Underwood, 1957): Old knowledge interferes with new and vice versa. `Supersedes` edges model this.

### Software Evolution
- **Lehman's Laws (1980)**: Software must be continually adapted; complexity increases unless actively reduced; quality appears to decline without maintenance.
- **Parnas (1994)**: Software aging — codebases degrade from failure to adapt AND accumulated damage from changes by people who don't fully understand the design.
- **Conway's Law (1968)**: Codebase structure reflects team structure. As teams change, structural contradictions accumulate.

### Map vs. Territory
- **Korzybski (1933)**: All models are incomplete abstractions. The danger is confusing the model with reality.
- **Bateson (1972)**: Maps are useful *because* they omit detail. A 1:1 map is useless.
- **Fairbanks (2010)**: Architecture models should be "just enough" — detailed enough to be useful, abstract enough to be comprehensible.
- **Brooks (1987)**: Essential complexity (the actual logic) vs. accidental complexity (the representation). Tools should target essential complexity.

## 3. Industry Context (2025-2026)

### MCP Ecosystem
- MCP went from a handful of servers (Nov 2024) to 5,800+ servers and 300+ clients (early 2026)
- Donated to the Agentic AI Foundation in late 2025 — now an industry standard, not just Anthropic's protocol
- Adopted by OpenAI, Google DeepMind, and major IDE vendors

### Competitive Landscape
- **Augment Code**: Builds real-time knowledge graph of entire codebase. Indexes up to 500K files. Scored +12.8 on Elasticsearch benchmark vs. -13.9/-11.8 for competitors. Exposing Context Engine via MCP gave Claude Code 80% quality improvement and Cursor 71%. Closed-source, $50/seat SaaS.
- **Cursor**: Semantic vector search, up to 50K files. Strong for single-repo, weaker on large multi-service codebases.
- **GitHub Copilot**: Local indexing caps at 2,500 files. Best for individual productivity, not codebase understanding.
- **Aider**: Pioneered "repo map" — compressed representation of file paths, class names, function signatures.

### Agent Memory Research (2025-2026)
- "Memory in the Age of AI Agents" survey (arXiv, Dec 2025): Memory is now recognized as a core capability, not an add-on
- "Codified Context" (arXiv, Feb 2026): Three-tier memory (hot/warm/cold) for 108K-line system across 283 sessions. Found single-file manifests (.cursorrules, CLAUDE.md) don't scale beyond modest codebases.
- MAGMA, EverMemOS (Jan 2026): Multi-graph memory architectures for structured reasoning

### RepoLens Positioning
Open, self-hosted, protocol-native (MCP). Fills the gap between raw file access (too little understanding) and proprietary SaaS platforms (vendor lock-in). Built for any agent, not one vendor's IDE.

## Sources

- [Lost in the Middle (Liu et al., Stanford/Berkeley)](https://arxiv.org/abs/2307.03172)
- [Why AI coding agents aren't production-ready (VentureBeat)](https://venturebeat.com/ai/why-ai-coding-agents-arent-production-ready-brittle-context-windows-broken)
- [AI-authored code contains worse bugs (The Register)](https://www.theregister.com/2025/12/17/ai_code_bugs/)
- [Developer complaints about AI coding tools (Medium)](https://medium.com/@anoopm75/the-uncomfortable-truth-about-ai-coding-tools-what-reddit-developers-are-really-saying-f04539af1e12)
- [How Augment Code Solved the Large Codebase Problem (Codacy)](https://blog.codacy.com/ai-giants-how-augment-code-solved-the-large-codebase-problem)
- [Codified Context (arXiv 2026)](https://arxiv.org/abs/2602.20478)
- [Memory in the Age of AI Agents (arXiv 2025)](https://arxiv.org/abs/2512.13564)
- [A Year of MCP (Pento)](https://www.pento.ai/blog/a-year-of-mcp-2025-review)
- [Repository Intelligence in AI Coding Tools (2026)](https://www.buildmvpfast.com/blog/repository-intelligence-ai-coding-codebase-understanding-2026)
- [Context Engineering for Developers (Faros)](https://www.faros.ai/blog/context-engineering-for-developers)
- [Endsley SA Model (ResearchGate)](https://www.researchgate.net/publication/210198492_Endsley_MR_Toward_a_Theory_of_Situation_Awareness_in_Dynamic_Systems_Human_Factors_Journal_371_32-64)
- [Model Context Protocol (Wikipedia)](https://en.wikipedia.org/wiki/Model_Context_Protocol)
