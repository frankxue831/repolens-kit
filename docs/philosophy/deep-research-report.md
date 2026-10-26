# Structure, Perspective, and Agentic Understanding

Authority: Supporting
Lifecycle: Reference

> Background research written in reply to a maintainer’s research prompt, kept as reference material. “You” and “your” refer to that prompt and its author. The export’s title was cut off after a trailing “in”, which has been dropped. The export also carried inline citation markers that pointed into the research tool’s own session and can’t be resolved outside it, so they were removed. Where the text doesn’t name a source itself, treat the claim as unverified.

## Framing the exploration

“Structure” is doing a lot of work in your prompt, and it helps to start from a definition that already treats structure as multi-dimensional rather than purely “code layout”. In the architecture-description standard ISO/IEC/IEEE 42010 (as summarised in architectural documentation guidance), *architecture* is not merely a physical decomposition: it is the “fundamental organisation” of a system (components + relationships + environment) **and** the “principles guiding its design and evolution”. That framing matches your instinct that “product structure” includes business modules, conventions, trade-offs, and decision rationale—not just folders and call graphs.

The same guidance also makes two distinctions that map cleanly to your “repo structure vs product structure” boundary:

- Architecture is, fundamentally, a *conception* (“in the human mind”), and an “architecture description is not the architecture” (the “map is not the territory”).  
- A *view* is a representation of a system from a set of concerns, and a *viewpoint* is the specification/template for how to construct such views.  

Those two points are a strong research-backed reason to treat “good structure” as something that can (and often must) be represented through **multiple lenses** and **multiple artefacts**, rather than a single canonical diagram or rulebook.

In other words, your “some perspective” clause is not a hedge—it is an accurate statement about how structure is defined and communicated in real engineering practice: structure is tied to stakeholders, concerns, and the work of explanation.

## What “good structure” can mean across the perspectives you listed

A productive way to slow down (as you requested) is to treat the perspectives you listed as **orthogonal but interacting** dimensions—each naming a different failure mode you are trying to avoid. The literature and classic practice can be used to “ground” each dimension.

### Conceptual integrity as coherence of ideas

In The Mythical Man-Month, Frederick P. Brooks Jr. argues that “conceptual integrity” is “the most important consideration in system design”, explicitly preferring one set of design ideas over a pile of individually good but uncoordinated ideas. He then ties conceptual integrity to *ease of use* and to the ratio of function to conceptual complexity—pointing out that “simplicity and straightforwardness proceed from conceptual integrity”.

This matters to RepoLens-style “structured perception” because an agent can “see” a great deal of code and still fail the user if the product’s concept-model is incoherent: the agent would learn local truths, but not the product’s governing idea (what changes are “in character”, what changes are “out of character”). Brooks explicitly treats “architecture” as the complete and detailed specification of the user interface and distinguishes architecture (“what happens”) from implementation (“how it is made to happen”). That distinction is a strong candidate for the “deep resonance” you’re feeling: a product’s structure can be “good” by being *conceptually controlling* (what is invariant) while still being permissive about implementation creativity (how).

### Architectural structure as modular boundaries and hidden decisions

Classic modularity work by David L. Parnas frames modularisation as a mechanism to improve (a) managerial independence, (b) product flexibility, and (c) comprehensibility, explicitly aiming for a system that can be studied and changed “one module at a time”. Parnas also defines a module as a “responsibility assignment” and stresses that “system level” decisions are those that affect more than one module. A key theme is hiding design decisions inside modules so that inevitable changes do not cascade across the whole system.

Your “repo structure is objective” point aligns: files, dependencies, and boundaries are observable. But the *reason* those boundaries exist (the decision you are protecting, the trade-off you chose) is not always directly inferable from code. Parnas’s “design decisions must be hidden” framing hints at why “product structure” is often partly invisible to pure code observation: good architecture *intentionally hides* certain choices behind boundaries.

### Cognitive structure as “understandability” and representation trade-offs

A striking bridge between product design and software structure comes from Dieter Rams, via Vitsœ’s canonical write-up: “Good design makes a product understandable… it clarifies the product’s structure… at best, it is self-explanatory.” This isn’t a mere aesthetic claim; it’s a statement about how structure exists *in the user’s mental model*.

In HCI, the Cognitive Dimensions of Notations framework (developed from Thomas Green’s work and later consolidated by Blackwell & Green) formalises why representations that look “structured” can still be costly to use: dimensions like **visibility** (can you see key components/relationships), **hidden dependencies**, and **viscosity** (resistance to change) create predictable trade-offs between understanding and modifiability. This is highly relevant to your agent question: whatever “product structure” is, it will be a *notation / information artefact* that can either support or hinder the user/agent’s cognitive work.

### Evolutionary structure as sustained change without collapse

Meir Lehman’s laws of software evolution frame “E-type” systems (software embedded in a changing real-world domain) as requiring continuous adaptation: if an E-type program is used, it must be continually adapted or it becomes less satisfactory. They also state that complexity tends to increase unless work is done to maintain or reduce it, and quality will be perceived as declining unless rigorously maintained and adapted to the changing environment.

This “evolutionary structure” lens is exactly where “product structure” (as decisions, trade-offs, conventions, business logic boundaries) becomes load-bearing: those artefacts are part of the feedback system that lets teams evolve without re-learning everything from scratch. Lehman explicitly describes evolution as feedback-driven. Without a living account of why the system is shaped the way it is, evolution becomes either accidental (drift) or paralysed (fear of breaking hidden constraints).

## Why the “some perspective” is rarely singular

In practice and in standards, “good structure” is inherently plural because different stakeholders need different structural truths.

ISO/IEC/IEEE 42010’s viewpoint concept encodes the idea that what is “fundamental” depends on stakeholders and concerns (“fundamental to whom?”). Philippe Kruchten’s 4+1 view model describes architecture through multiple concurrent views precisely to address different stakeholder concerns, and warns that single “boxes and arrows” diagrams often collapse incompatible meanings (deployment, code organisation, runtime flows, etc.) into one muddy picture.

Similarly, Simon Brown’s C4 model explicitly makes structure a set of hierarchical abstractions and diagrams, with different “zoom levels” used to “tell different stories to different audiences”—and explicitly notes you don’t need all levels, only those that add value.

This gives a rigorous answer to your first deep question:

A good product “must have good structure” in at least one perspective because (a) architecture is defined as the system’s fundamental organisation and guiding principles, and (b) “fundamental” is stakeholder-relative, so structure must be representable in the forms that stakeholders can actually use.

For RepoLens, the key stakeholder is *the agent*, but not in isolation: the agent is mediating the human’s goals, so the “fundamental” perspective likely needs to be both **agent-operational** (what supports correct reasoning and safe action) and **human-governable** (what supports steering, auditability, and change).

## Repo structure versus product structure through the lens of situation awareness

Your suggestion to go via Endsley’s Situation Awareness is extremely well-aimed, because it gives a clean scaffold for “structured perception” without prematurely prescribing a representation.

Mica R. Endsley defines situation awareness, in a general form, as comprising: perception of elements in the environment (within time/space), comprehension of their meaning, and projection of their status in the near future. This yields three levels: Level 1 (perception), Level 2 (comprehension—integrating information and determining relevance to goals), and Level 3 (projection—forecasting future events/dynamics).

Two consequences drop out for RepoLens:

Repo structure is best treated as Level 1 SA (and parts of Level 2), because file trees, dependency graphs, and call chains are the “elements in the environment” the agent must perceive reliably.

Product structure is primarily Level 2 and Level 3 SA, because it encodes meaning relative to goals (“why is this boundary here?”, “what is this module for?”, “what future failures does this trade-off avoid?”), and provides the basis for projection (“if we refactor X, which invariants or regulatory constraints do we violate?”).

Endsley also explicitly warns against “More Data ≠ More Information” and frames SA as bridging an “information gap” by integrating, sorting, and finding what matters. That directly resonates with your “structured perception” goal: RepoLens shouldn’t just expose more code; it should narrow the gap between “data produced” and “information needed”, which often requires interpretive structure.

## What “product structure” could look like to an agent

Here, two research-backed moves help keep the exploration open while preventing category mistakes:

Treat “product structure” as a set of *views* (stakeholder-relative) rather than one monolithic artefact.

Separate *observed structure* from *interpreted structure*, and attach provenance and confidence to interpretation. This is consistent with both the “map vs territory” distinction in ISO 42010 and with patterns like decision records, which are intentionally lightweight yet explicit about context and consequences.

Within that framing, at least four candidate “shapes” for product structure emerge from the literature and modern practice:

### Decision and trade-off graphs as first-class structure

Architectural Decision Records (ADRs) are explicitly designed to capture a single decision, its context, and consequences, and to preserve a history through *superseding* rather than rewriting. Nygard’s original write-up is highly relevant to your “Observe, Don’t Judge” tension: it says the “Context” section should be written in a value-neutral style, simply describing facts and forces, and then the “Decision” section states “We will…” followed by consequences (positive, negative, neutral).

This is almost a direct blueprint for how an agent could “observe product structure” without treating it as sacred:

- product structure is a set of *forces + decisions + consequences*, not a set of commandments  
- structure is allowed to evolve, but the evolution is legible (superseded decisions remain visible)  
- uncertainty can be recorded explicitly (Fowler suggests noting confidence and triggers for re-evaluation).  

### Domain maps and bounded contexts as “product modules”

Domain-Driven Design’s bounded context concept offers a robust middle layer between “business modules” and “repo modules”. Martin Fowler describes bounded context as a central DDD pattern for dealing with large models/teams by dividing a system into different bounded contexts and being explicit about their relationships; the goal is internal consistency of the model inside each boundary.

For RepoLens, this can be read as: “product structure” might not be a UML diagram; it might be a *context map*—a structured declaration of where language, rules, and invariants change.

### Pattern language as a generative, non-sacred structure

A pattern language is explicitly designed to be re-used without being copied mechanically. In the Open University’s summary of Christopher Alexander, a pattern describes a recurring problem, the context in which it occurs, and a method/recipe for resolving it, quoting Alexander’s idea that you can apply the solution “a million times… without ever doing it the same way twice.”

This is a subtly powerful response to your concern that “seeing product structure” will make an agent conservative: a well-designed pattern language is *structured guidance that explicitly permits variation*. Notably, Nygard explicitly models ADRs as “similar to an Alexandrian pattern” and frames each record as balancing forces, but not necessarily being a “pattern” itself.

### Scenario-centred structure for grounding and verification

Kruchten’s 4+1 model uses scenarios (use cases) as the “+1” view to show how the elements in other views work together and to validate the architecture. For an agent, scenario structure can be the bridge between “product intent” and “repo realities”: “this is the critical flow”, “this is the failure mode we care about”, “this is the contract for correctness”.

Taken together, these suggest that “product structure” for an agent is likely not a single diagram type; it is a **portfolio**: domain boundaries, decision records, patterns, and scenarios—each with explicit links back to evidence in the repo.

## What “seeing the product structure” should do to agent behaviour

Your question splits neatly into “understanding” versus “constraint”, and modern agentic coding tools show both modes in practice.

Anysphere’s Cursor describes semantic search as a major driver of agent performance, and documents a pipeline where it builds an index of the codebase (Merkle-tree-based change detection, chunking, embedding, caching) to support fast retrieval and reasoning over large repos. Whatever else “product structure” is, Cursor’s approach supports the idea that agents need *structured perception primitives* (indexes, references, proofs) rather than trying to stuff a whole codebase into context.

Anthropic’s “effective context engineering” article makes this principle explicit: rather than pre-processing all data up front, agents can carry lightweight identifiers (file paths, stored queries, web links) and dynamically load data just-in-time using tools; it explicitly cites Claude Code as using this approach. This aligns strongly with your Packs/Memory separation intuition: immutable, referenceable “facts” plus higher-level, mutable interpretation over those facts.

Cognition’s Devin tooling has an even more direct “understanding before acting” loop. “Ask Devin” is described as a window into the codebase: the repo is indexed so Devin can reason about code, and Ask Devin supports exploring architecture/dependencies and planning tasks before starting an agent session. Devin’s “Interactive Planning” further emphasises showing a plan that the user can adjust and reviewing it to ensure the agent is operating on the right parts of the codebase; it also notes limits of indexing recency (main branch indexing may miss very recent changes).

Finally, Augment Code markets its approach explicitly in terms of “deep contextual understanding” plus “automatic memories across sessions”, and describes a “Context Engine” that builds maps of what connects to what and curates context rather than dumping code. (As with any vendor material, the implementation details and quantitative claims should be treated cautiously; what’s still informative here is the product thesis: *structure-aware curation + memory + workflow embedding*.)

These examples support a cautious but concrete behavioural hypothesis for RepoLens:

“Product structure visibility” is most valuable when it (a) improves Level 2 comprehension—*what the code means in the product’s goal space*—and (b) improves Level 3 projection—*what will likely break if we change X*.

That does not require imprisoning the agent. It suggests a more subtle contract: product structure acts as **retrieval scaffolding** (“where should we look?”), **explanatory scaffolding** (“why is it like this?”), and **guardrail scaffolding** (“what must remain true?”).

## Conflicts, risks, and how not to accidentally destroy “exploration mode”

Your concern that increased “product structure visibility” might reduce creativity or push the agent into reverence has empirical cousins in two well-studied phenomena: design fixation and automation bias.

Design fixation research (Jansson & Smith) defines fixation as “blind adherence” to a set of ideas limiting conceptual design output, and reports experimental evidence that fixation is measurable. In more recent open-access work, Crilly’s interview study of expert designers reports that fixation is recognised in professional practice and that designers take steps to address it; reflecting on fixation is discussed as a way to guard against it.

Automation bias/complacency research reviews how people interacting with decision support systems can make omission and commission errors when the aids are imperfect, and that such biases can occur even for expert users; the integrated model places attention at the centre. Endsley’s work on automation and situation awareness ties “out-of-the-loop performance” to reduced ability to detect and respond to failures, and reports results where Level 2 SA (comprehension) can be negatively impacted in automated conditions even when Level 1 SA is unaffected—people may monitor data but lose understanding of what it means relative to operational goals.

These sources don’t prove that “product structure graphs will make RepoLens agents conservative”, but they offer a grounded warning: when you give an agent (or a human) a strong representation, you can induce **over-trust**, **over-attachment**, or **tunnelled attention** unless the system design actively counterbalances those tendencies.

One way to resolve the tension—while staying faithful to your “Observe, Don’t Judge” principle—is to adopt ADR/pattern-style structure as the default: describe forces neutrally, record decisions explicitly, preserve superseded alternatives, and attach confidence and triggers for revision. This preserves exploration because it treats structure as a living hypothesis under feedback, which is aligned with Lehman’s feedback-centric view of evolution.

On the human side, the most plausible shift is not “perfect product knowledge” but **more legible steering**: tools like Devin explicitly push users to review/adjust plans and use code citations to verify agent understanding, turning software change into a conversational alignment loop rather than a purely post-hoc diff review loop. Endsley’s automation framing suggests why this matters: intermediate automation levels that keep the human involved in active decision loops can mitigate out-of-the-loop failures by protecting higher-level comprehension.

The open research-backed design tension, then, is not “should an agent see product structure?” but “how do we present product structure so that it increases comprehension and projection **without** inducing fixation, over-trust, or premature convergence?”
