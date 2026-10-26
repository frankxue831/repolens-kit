# RepoLens

Gives your AI coding agent a memory of your repo that carries over between sessions and fades when it isn't used.

RepoLens scans a repository, flags things worth knowing (unused files, files everything depends on), and keeps typed memories (decisions, conventions, trade-offs) that lose weight over time unless they're used. Agents reach it through an MCP server; you can try the core of it from the command line.

## Try it (about 5 minutes)

Needs Rust 1.88 or newer (`rustup update stable`).

```sh
cargo install --git https://github.com/frankxue831/repolens-kit repolens-cli
cd your-repo
repolens scan --root .
repolens init
```

On ripgrep, `init` prints:

```
RepoLens is set up: 6 files in .repolens/
Git ignores .repolens/ via its own .gitignore; your files are unchanged.

The last scan (pack-20261025-052924) found 2 things worth a look:

  Med   10 widely imported files, e.g. crates/regex/src/error.rs (34)
  Low   5 groups of files import each other in a loop (largest: 8 files)
  Coverage: imports aren't read for 3 languages: Lua, Ruby, Shell (876 lines)
  Findings are heuristics; each one lists what it doesn't check.
  Run `repolens findings` to see them all; add --verbose for the evidence.

Drafted for you to check (nothing is enforced yet):
  known-constraints.md  7 draft constraints, D-001 to D-007
    5 CI commands (cargo 5)
  repo-overview.md      package, languages, build and test commands
  Read from: README.md, Cargo.toml, 2 CI workflows

Next: open .repolens/known-constraints.md. Rename a draft from D-NNN to C-NNN
once you've checked it; only C-NNN rules are enforced by validate-plan.
```

<sub>Captured on ripgrep at commit `3fce3b5`. Counts and the pack id will differ on your repo.</sub>

## Save a memory and watch it fade

The examples below were run on a ripgrep clone. On your own repo, point `--anchor` at one of your files.

```sh
repolens memory save --type decision --importance 0.5 \
  "ripgrep prints results in file order only when --sort path is passed"
repolens memory save --type convention --importance 0.8 \
  --anchor crates/core/main.rs \
  "ripgrep CLI entry point is crates/core/main.rs and flag parsing lives in crates/core/flags"
repolens memory recall "ripgrep"
```

Types are `PATTERN`, `DECISION`, `CONVENTION`, `TRADE_OFF` and `CODE_ANCHOR`. Recall prints:

```
2 memories match "ripgrep":

1. CONVENTION  score 0.80 = importance 0.80 x keywords 100%
   ripgrep CLI entry point is crates/core/main.rs and flag parsing lives in
   crates/core/flags
   Decay: last used less than an hour ago; fades while unused (a confirmed
   anchor keeps a minimum)
   Grounding: CONFIRMED (crates/core/main.rs found in pack-20261025-052924)
   id: fc014290-fe63-4c72-aecc-308b64c4f300

2. DECISION  score 0.50 = importance 0.50 x keywords 100%
   ripgrep prints results in file order only when --sort path is passed
   Decay: last used less than an hour ago; fades while unused
   Grounding: none (no file anchors)
   id: 4abd6326-9738-4e29-800b-67cbb2803f42
```

A few weeks later, with neither memory used:

```sh
REPOLENS_CLOCK_OVERRIDE=$(date -u -d '+21 days' +%FT%TZ) repolens memory recall "ripgrep"
```

```
2 memories match "ripgrep":

1. CONVENTION
   Decay: last used 21.0 days ago; fades while unused (a confirmed anchor keeps
   a minimum)
   Grounding: CONFIRMED (crates/core/main.rs found in pack-20261025-052924)

2. DECISION
   Decay: last used 21.0 days ago; fades while unused
   Grounding: none (no file anchors)
```

Scores, memory text and ids are left out here; run the command to see how far each memory has faded.

A memory tied to a file that still exists stops fading at a minimum. Rarely used memories are eventually pruned, and recall tells you when a match was removed this way.

<sub>`REPOLENS_CLOCK_OVERRIDE` is a test hook that pretends time has passed; a recall with it set saves nothing. You don't need it in normal use. On macOS, use `date -u -v+21d +%FT%TZ`. Output from an 80-column terminal; your ids and pack id will differ.</sub>

`repolens state` then shows what's stored:

```
RepoLens state

Root: /tmp/ripgrep

Scan packs
  Status: present
  Surface: derived repo context, regenerable
  Path: .repolens/pack-*
  Packs on disk: 1
  Active pack: pack-20261025-052924
  Last scan mode: full
  Written by: repolens scan
  Note: not long-term memory; safe to regenerate.

Typed memory graph
  Surface: structured long-term memory
  Path: .repolens/memory/memory.json
  Status: present
  Nodes: 2
  Edges: 0
  Types:
    CONVENTION: 1
    DECISION: 1
  Newest: CONVENTION "ripgrep CLI entry point is crates/core/main.rs and flag pars…" (importance 0.80)
  Last updated: 2026-10-25 13:29:24 +08:00 (2026-10-25 05:29:24 UTC)
  Written by: repolens memory save/recall, MCP memorize_insight / recall_longterm / forget_insight
  Note: repolens remember does not write this file.

Session handoff markdown
  Surface: unverified session handoff notes
  Path: .repolens/session-memory.md
  Status: template-only
  Lines: 62
  Written by: repolens remember --file
  Note: included by repolens brief; not typed memory.
```

To see every saved memory, or remove one, use its id (at least the first 4 characters):

```sh
repolens memory list
repolens memory forget 4abd6326        # preview only
repolens memory forget 4abd6326 --yes  # remove it
```

## Connect your agent

```sh
repolens mcp     # MCP server over stdin/stdout, 26 tools
```

Point your agent's MCP config at that command. The same memories you saved above are available to it through `recall_longterm` and `memorize_insight`.

## What works today, and what doesn't yet

- **Works:** scan and findings, `init` drafts, the typed memory store (5 types), where unused memories fade over time, `brief`, `validate-plan` (warnings only), `remember` (markdown handoff), `eval` scaffolding, and the 26 MCP tools.
- **Partly:** grounding checks that the files a memory points to still exist. If one was edited since the note was saved, recall says so, but the note stays `Confirmed`.
- **Not yet (planned):** failed-attempt memories, and a confidence label that separates checked facts from agent guesses.

Details: [`docs/philosophy/07-current-state-vs-direction.md`](docs/philosophy/07-current-state-vs-direction.md).

## The problem

Modern AI coding agents (Claude Code, Cursor, Copilot, Codex, Devin) generate code well, but lose project context the moment a session ends. Every new session starts cold:

- They re-discover the same files via `grep` and `find`.
- They re-derive the same architectural conventions.
- They re-make mistakes that were caught and rolled back weeks ago.
- They miss constraints humans implicitly know but never wrote down.
- They guess plausibly but wrongly because nothing pushes back.

The naive fix — bigger context windows, more files dumped into the prompt — does not work; relevant signal gets diluted in noise ("Lost in the Middle"). The real bottleneck is not intelligence, it is **durable, structured perception**.

## Target user

You are running coding agents on a non-trivial codebase and have hit at least one of:

- "Why is the agent grepping again? It read this file last session."
- "The agent just suggested a change we already tried and rolled back."
- "I keep re-explaining this codebase to the agent every morning."
- "I want a benchmark for whether agent context is actually improving."

RepoLens is for the human-in-the-loop running those agents — and for the agents themselves.

## What RepoLens does not do

- **It does not write code.** Agents do that.
- **It does not replace tests.** Tests verify behavior; RepoLens helps agents find and respect them.
- **It does not guarantee correctness.** Memory is decay-based and grounded, not a source of truth.
- **It does not remove humans from the loop.** Important facts, constraints, and decisions should be human-confirmed.
- **It is not a code-search engine.** Search exists, but it is one tool among 26.
- **It is not a generic RAG store.** Memory is typed, checked against the files in the current pack, and decays — not a flat embedding bucket.
- **It is not a multi-repo memory layer.** One repo per memory store, today.

## How is this different from grep / IDE search / RAG / docs?

|  | grep / ripgrep | IDE search | RAG (embeddings) | Project docs | RepoLens |
|---|---|---|---|---|---|
| Cross-session memory | No | No | Indirect | If maintained | **Yes** (typed, decaying) |
| Knows what was tried and failed | No | No | No | Rarely | **Not yet (planned)** |
| Distinguishes facts from guesses | No | No | No | No | **Not yet (planned)** |
| Checks claims against current files | No | No | No | Often stale | **Partly** (anchored files must still exist; edits are flagged, not judged) |
| Built for agent-shaped queries | No | No | Partially | No | **Yes** (MCP) |
| Designed to be wrong, then corrected | n/a | n/a | No | n/a | **Yes** (decay + supersedes + contradicts) |

RepoLens is closer to **a notebook the team and the agents share** than to any of the above. It records what was learned, who/what verified it, and how confident to be — and it lets that knowledge age and be contradicted.

## Memory safety rules

Memory is the highest-risk surface in any agent-facing tool. Wrong agent guesses must not become permanent facts. The contract:

1. **Default to `AgentAsserted`.** Anything an agent writes without grounding is a hypothesis, not a fact. *(`Confidence` enum is planned for v0.1; the current implementation uses an `importance: f32` weighted score.)*
2. **Anchors are required for high-confidence memories.** A `verified_fact` with no anchor is rejected at the API. *(Planned for v0.1.)*
3. **Grounding is checked at recall.** [`recall_longterm`](crates/repolens-mcp/src/tools/memory.rs) checks that each file a memory points to still exists in the latest scan and returns `Confirmed` / `Contradicted` / `Unverifiable` / `Unchecked` per anchor. A file that was deleted or renamed (and rescanned) gives `Contradicted`. Editing the file does **not** change the result; recall adds that the file changed since the note was saved.
4. **Contradictions speed up fading.** A memory on the losing side of a `Contradicts` or `Supersedes` edge fades faster ([`crates/repolens-memory/src/decay.rs`](crates/repolens-memory/src/decay.rs)).
5. **Humans confirm constraints.** Constraints (planned type) require a human-verified flag before they bind plan validation.

Full model: [`docs/memory-safety.md`](docs/memory-safety.md).

## Limitations

- **One repo per memory store.** No multi-repo or org-level memory.
- **English-only convention parsing.**
- **Single-author tested.** No production deployments.
- **Grounding checks that files exist, not that the note is still true.** After the file a memory points to is edited and rescanned, the memory stays `Confirmed` and recall adds "changed since this note was saved". It only becomes `Contradicted` once that file is deleted or renamed and the repo is rescanned. Notes saved before any scan get no change note.
- **Untracked files that are not ignored are re-read on every scan.** Add build output such as `dist/`, `build/` or `.next/` to `.gitignore`.
- **Memory has no upper bound today.** Behavior at 10k+ memories is unverified.
- **`repolens state` is read-only human output.** It inventories local files only; it does not call MCP, repair corrupt files, prune packs, run decay, or provide a stable machine-readable JSON contract.
- **`repolens validate-plan` is warnings only.** A clean lint output does **not** prove the plan is valid; checks are deterministic regex/substring heuristics with intentional false negatives. The command is a guardrail, not a replacement for tests or human review.
- **`repolens remember --file` is workspace-markdown only today.** It prepends an unverified agent handoff to `.repolens/session-memory.md`; it does not write `.repolens/memory/memory.json` or the typed memory graph.
- **`repolens brief` is workspace-driven today.** It composes `.repolens/` files into a deterministic markdown brief and adds a `## Runtime Freshness` section that reads `.repolens/session.json` and the git HEAD to surface the authoritative active pack and warn when `repo-overview.md` cites a stale pack id; with `--task`, it also lists files from the latest scan and saved notes that share words with the task (matched by name, not by meaning). It does not call MCP tools or use an LLM.
- **Some memory types (`Hypothesis` / `Constraint` / `FailedAttempt` / `FrictionFinding`) are planned, not shipped.**
- **No CI integration of the eval suite yet.** Eval is a contributor-discipline today, not a build gate.

## Roadmap

- **v0.1 — Agent legibility & memory safety.** `repolens init` ✓ shipped. `repolens brief --task` ✓ shipped (matches the task to files and saved notes by name). `repolens remember --file` ✓ shipped (workspace-markdown only). `repolens eval list` / `repolens eval start` ✓ shipped (scaffolding only). `repolens validate-plan` ✓ shipped (warnings only). `repolens state` ✓ shipped (read-only local inventory of persistence surfaces). New memory types and `Confidence` enum, friction-log primitive, and eval rollup remain v0.1 targets.
- **v0.2 — Cross-session continuity at scale.** Session ID on memory entries, friction-tag aggregation across runs, tested behavior at 10k+ memories.
- **v0.3 — Plan-validation maturity.** Stronger constraint matching, lockstep tests, telemetry on constraint hits.
- **Beyond.** Multi-repo memory, on-scan grounding, absence-as-signal grounding.

Canonical "shipped vs direction" table: [`docs/philosophy/07-current-state-vs-direction.md`](docs/philosophy/07-current-state-vs-direction.md).

## Build & run

```sh
cargo build
cargo test --workspace
cargo run -- scan --root <path>
cargo run -- mcp                  # start MCP server over STDIO
```

Pre-push gate: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues as described in [SECURITY.md](SECURITY.md). Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

MIT. See [LICENSE](LICENSE).
