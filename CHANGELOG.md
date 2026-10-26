# Changelog

Issue and PR numbers before 1.0 refer to the private development repository.

All notable changes to RepoLens are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). RepoLens is pre-1.0, so behaviour can still change between versions.

## [Unreleased]

These are fixes from dogfooding RepoLens on real repositories. The version in `Cargo.toml` is still `0.1.0`.

### Added

- **CI command detection:** `init` now drafts build and test commands from your CI workflows for more tools, including Maven, Gradle, Ruby, Python and more `cargo` subcommands. (part of #13)
- **CI drafts grouped by tool:** `known-constraints.md` and the brief open with one line per CI tool, giving its draft count and ids, above the full list. `init` shows the counts per tool. (#45)
- **`brief --task`** now lists the files from the latest scan and the saved notes that share words with the task, or says that nothing matched. (#34)
- **`memory recall`** shows how each score is made up, including the grounding factor when it lowered the score. It says when a faded match was removed, and notes when an anchored file changed since the note was saved. (part of #14)
- **`memory list`** (alias `ls`) prints every saved note, oldest first, with its type, saved importance, content and full id. The importance shown is the value when last saved; `memory recall` applies fading. It writes nothing. (#54)
- **`memory forget <id>`** removes a saved note. It takes a full id or a prefix of at least 4 characters, in any case. By default it only shows what would be deleted; add `--yes` to delete. Unknown, too-short and ambiguous ids are refused without writing anything. (#54)

### Changed

- **MCP `forget_insight`** now needs at least 4 characters of the id for a preview as well as a delete, and its ambiguous-id error lists up to 10 matching ids. (#54, part of #56)
- **`memory recall`'s `Decay:` line** says in plain words how a memory is fading (it fades while unused, and a confirmed anchor keeps a minimum) instead of printing the tuning values. (#88)

### Fixed

- **Scan data loss:** if two `repolens scan` runs started in the same second, every stored file could be emptied until a `scan --full`. Each scan now gets its own pack folder, and an incremental scan never writes into the pack it builds on. (#60)
- **Memory data loss:** a `memory.json` that couldn't be parsed, for example one written by a newer version, was treated as empty and then overwritten on the next save. RepoLens now reports an error naming the file and leaves the file unchanged. (#60)
- **Incremental scan:** changes staged in git (`git mv`, `git rm`) and edits that were later undone are now picked up, so memory grounding no longer stays stale after a rename. (#30)
- **`scan --keep N` / `clean --keep N`:** the newest clean pack is always kept as the base for the next incremental scan, so up to N+1 packs can remain. The CLI now says why a scan fell back to a full scan. (#37)
- **`init`:** RepoLens no longer edits your root `.gitignore`. It writes `.repolens/.gitignore` instead, so `init` leaves your tracked files unchanged. (#39)
- **`init` and `findings`** now agree, and the language-coverage note no longer takes the top slots. (#32)
- **Graph answers** (dependencies, impact, fan-in) now say what was skipped instead of looking complete. The MCP server says "results may be incomplete" when unresolved imports weren't stored. (#31, #42)
- **Unresolved imports** are matched before the storage cap, and the Rust `[lib]` name and wasm-pack `--out-name` are recognised. (part of #42)
- **Imports of missing files** no longer count toward the callers of an unrelated file with the same name. Only the importing file's answer is marked derived, with a note naming the import. Python relative imports are shown as written, and `from . import name` for a module that doesn't exist is reported. (part of #42)
- **Go packages:** a Go package folder is now treated as one unit, so its files aren't flagged "may be unused" when the package is imported. Imports of the repo's own module now resolve, and the widely-imported line names the package. (#24)
- **Duplicate Java classes:** when the same class exists in two source trees, a reference now resolves to the copy in its own source root or module, or stays unresolved instead of guessing. (#29)
- **CI step drafts** follow how steps actually run: heredocs, quoted env prefixes, comments, pipes, folded blocks, per-run working directories and `if:` conditions are handled. (part of #13)
- **"May be unused" findings** no longer flag test files, build configs, modules loaded with `#[path = "..."]`, or Next.js route and convention files. (#26, #19, #46)
- **`memory recall`** matches plural forms, ignores common stop words, and wraps to the terminal width (80 columns when output is piped). The MCP `forget_insight` tool accepts the 8-character id that `memory save` prints, in any case. (#35)
- **Closed output:** the CLI exits quietly when its output is closed, for example `repolens findings | head`, instead of panicking. (#57)
- **Scan summary:** the reprocessed count is now measured against the previous scan, and reprocessed plus carried-forward files add up to the total. (#40)
- **Log colours:** log lines have no colour codes unless stderr is a terminal. (#36)
- **Lockfile advice** now matches the project's ecosystem. Maven, Gradle and plain pip, setuptools or requirements.txt projects no longer get "No lockfile found", and neither do Node, Python or Java libraries. A tracked `Cargo.lock` is reported. (#25)
- **CI commands split with a trailing `\`** are joined into one command. (#17)
- **Orphan-file finding** now uses plain wording, with the right singular or plural. (part of #20)
- **Dirty-worktree fallback and `bootstrap finish`:** when a scan falls back to a full scan because no earlier scan ran on a clean worktree, it explains how to get incremental scans. `bootstrap finish` lists every outside write once, up to 10, and then "…and N more". (#58, #59)
- **Clock override:** a save under `REPOLENS_CLOCK_OVERRIDE` now dates the memory file's `updatedAt`, and the "Last updated" line in `repolens state`, with the override time. (#8)
- **Symbol anchors:** `memory save --anchor` no longer warns "not a file in this repo" for a symbol from the latest scan, such as `Widget.run`. (#9)
- **`memory recall` word matching:** "used" and "using" now find notes about "use", acronym plurals such as "apis" find "api", and "news", "series" and "species" are no longer treated as plurals. A one-word query that is a common word, such as `IT` or `who`, is now searched. (part of #56)
- **Full disk:** writing to a full stdout now fails with a one-line error instead of panicking, and the hardlink copy fallback no longer floods stderr. (#59)

### Known issues

The open issues are listed in the issue tracker.
