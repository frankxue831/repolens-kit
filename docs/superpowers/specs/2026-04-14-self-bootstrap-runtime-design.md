# Self-Bootstrap Runtime Design

**Date:** 2026-04-14
**Status:** Shipped
**Scope:** Phase 1 runtime for agent-driven self-bootstrap workflows in `repolens-rs`
**Depends on:** [2026-04-14-self-bootstrap-skills-design.md](2026-04-14-self-bootstrap-skills-design.md), [2026-04-14-self-bootstrap-skill-specs.md](2026-04-14-self-bootstrap-skill-specs.md), [../../philosophy/06-observation-invariants-and-grounding-eligibility.md](../../philosophy/06-observation-invariants-and-grounding-eligibility.md), [../../philosophy/07-current-state-vs-direction.md](../../philosophy/07-current-state-vs-direction.md)

## Context

The earlier bootstrap-contract phase established an explicit bootstrap contract:

- `bootstrap.yaml` defines task types, allowed paths, required checks, trust policy, and machine-readable skill summaries
- `repolens-core` can load, validate, and index that contract
- the first bootstrap skills are documented as bounded human-readable workflow contracts

Phase 1 now ships an agent-driven runtime for `docs_only` and `trust_contract` through `repolens bootstrap start|finish|abort`. RepoLens still does not edit files itself, and `write_anchored_memory` remains deferred.

This design defines the smallest honest runtime that makes bootstrap workflows end-to-end:

- RepoLens owns workflow setup, boundary checks, policy freeze, verification, rescan, and reporting
- an external agent still proposes the goal and edits files directly
- RepoLens does not claim to plan or edit autonomously

## Goals

- ship a real bootstrap runtime that is runnable from the CLI and testable from `repolens-app`
- support `docs_only` and `trust_contract` tasks only
- freeze the effective bootstrap contract at run start so finish-time behavior is reproducible
- enforce path boundaries, command checks, and rescan/report rules from inside RepoLens
- keep every run explicit and inspectable on disk under `.repolens/`

## Non-Goals

- autonomous file editing
- `bounded_rust_change` runtime support
- automatic memory writes
- MCP-internal orchestration
- background daemons or schedulers
- implicit multi-run coordination across shared worktrees

## Decision Summary

| Question | Decision | Rationale |
|---|---|---|
| What kind of runtime is this? | Agent-driven runtime | RepoLens can honestly own orchestration and policy without pretending to generate edits |
| What is the first workflow scope? | `docs_only` and `trust_contract` only | Keeps the first runtime narrow and consistent with current trust guarantees |
| What is the entrypoint shape? | `bootstrap start`, `bootstrap finish`, `bootstrap abort` | Separates boundary setup, external editing, and guarded completion |
| What is the diff baseline? | Freeze `HEAD` SHA plus `git status --porcelain` at start | Makes finish-time diff validation stable and reviewable |
| How many active runs may exist per worktree? | At most one | Avoids overlapping runs in the same clone producing ambiguous diffs |
| Which contract wins if files change mid-run? | Start-time frozen snapshot | Prevents live edits to `bootstrap.yaml` from retroactively changing an in-flight run |
| How are verification commands executed? | Normalized argv via `std::process::Command`, inheriting parent env | Avoids shell footguns while keeping existing cargo/PATH behavior usable |
| How is rescan performed? | `ScanService` directly | The runtime is already inside RepoLens; no need to start MCP just to rescan |

## Runtime Boundary

The first runtime is end-to-end only in the following sense:

1. RepoLens creates a bounded bootstrap run
2. the external agent reads the run plan and edits files
3. RepoLens validates the resulting diff, runs required checks, rescans, and writes the final report

That is sufficient to call the workflow end-to-end because the bootstrap contract is actually enforced from start through finish, even though editing remains external.

## CLI Shape

`repolens-cli` should add a top-level `bootstrap` command with three subcommands:

```text
repolens bootstrap start \
  --root <path> \
  --task-type <docs_only|trust_contract> \
  --goal "<text>" \
  --intended-path <repo-relative-glob>...

repolens bootstrap finish \
  --root <path> \
  [--run <run-id>]

repolens bootstrap abort \
  --root <path> \
  [--run <run-id>]
```

### `start`

`start` should:

- canonicalize `--root`
- reject unsupported task types
- reject a dirty worktree
- reject an existing active bootstrap run in the same worktree
- capture baseline `HEAD` SHA and `git status --porcelain`
- load and validate `bootstrap.yaml`
- freeze the effective runtime contract into a normalized plan snapshot
- discover whether RepoLens perception is usable
- perform narrow cold-start recovery when allowed
- write the run directory and mark it active

It returns a JSON object on stdout containing:

- `runId`
- `runDir`
- `planPath`
- `boundedPaths`
- `requiredChecks`
- `discovery`

Phase 1 CLI output is JSON-only for `start`, `finish`, and `abort`; a human-readable `--format text` mode can land later. Keeping Phase 1 machine-parseable avoids forcing agents to regex CLI chatter.

### `finish`

`finish` should:

- resolve `runId` from `--run` or `.repolens/bootstrap/CURRENT`
- load the frozen plan snapshot
- refuse completion if the run is already terminal, except for idempotent re-read of the stored report
- verify current `HEAD` still matches the captured baseline commit
- compute actual changed files from current git state
- reject any changed file outside the approved boundary
- run the frozen required checks
- run a RepoLens rescan through `ScanService`
- write a terminal report and clear active-run markers

### `abort`

`abort` exists because finish is not logically re-entrant after partial failure without explicit cleanup. `abort` should:

- resolve the active run
- write an `aborted` terminal report if one does not already exist
- clear active-run markers
- leave the worktree untouched

## Worktree and Concurrency Rules

The runtime must enforce:

- at most one active bootstrap run per worktree
- different worktrees may run independently
- the same worktree may not host overlapping runs

This is represented by:

- `.repolens/bootstrap/ACTIVE`
- `.repolens/bootstrap/CURRENT`

`ACTIVE` should be created atomically on `start` (e.g. `O_CREAT|O_EXCL`). If it already exists, `start` fails with a clear error. `CURRENT` is a convenience pointer to the same run id so agents can omit `--run` on `finish` or `abort`.

`finish` and `abort` must also observe `ACTIVE` before mutating run state. If two `finish`/`abort` calls race, the first one to atomically transition the run to a terminal status owns the outcome; the second either returns the stored terminal report (idempotent `finish`) or fails cleanly (`abort` on a terminal run).

All git inspections — baseline commit capture, porcelain snapshot, and finish-time diff — should use the existing `gix` dependency in `repolens-core` rather than shelling out to `git`, to keep the runtime free of a new process-level dependency.

The runtime should treat `ACTIVE` as authoritative for concurrency and `CURRENT` as ergonomic sugar only.

## Diff Baseline and Contract Freeze

`start` must freeze both the repository baseline and the runtime contract.

### Repository baseline

The frozen baseline contains:

- `baselineHeadCommit`
- `baselinePorcelain` (must be empty at start; recorded as evidence that the clean-worktree check passed, not as allowed drift)
- `baselineContractHash` (SHA-256 of `bootstrap.yaml` bytes at load time)
- start timestamp

`finish` validates against the captured commit, not the current meaning of `HEAD`. If `HEAD` moved since `start`, the run fails with an explicit report status such as `invalidated_by_head_change`.

### Contract freeze

The frozen plan snapshot contains:

- `schemaVersion`
- `runId`
- `taskType`
- `goal`
- selected bootstrap skills
- normalized allowed path patterns
- normalized required checks
- trust-policy values used for this run
- cold-start decision
- discovery evidence summary
- normalized command argv vectors used by the runtime

If the live `bootstrap.yaml` changes after `start`, `finish` records that fact as `liveContractChanged: true`, but still validates against the frozen snapshot. Detection is a byte-level SHA-256 comparison between the frozen `baselineContractHash` and the hash of the current file on disk at finish time. A missing or unreadable live file is also recorded as `liveContractChanged: true` with an explanatory reason field; the frozen snapshot remains authoritative.

## Path Semantics and Safety

`--intended-path` is repo-relative only.

Rejected at `start`:

- absolute paths
- paths containing `..`
- paths that lexically escape the repo root

Allowed patterns are matched only after normalization to repo-relative form. Glob matching should reuse `globset` (already transitively available via the `ignore` crate in `repolens-core`) to stay consistent with the existing `RulesEngine`; Phase 1 must not introduce a new glob dependency.

At least one `--intended-path` is required. A run with an empty bounded path set is rejected at `start`.

At `finish`, the runtime validates actual changed files using this order:

1. resolve the changed repo-relative path from git
2. canonicalize the underlying filesystem path when possible
3. confirm the canonicalized path remains under canonicalized `--root`
4. reject symlink escapes
5. match the normalized repo-relative path against the approved path patterns

New files may not canonicalize yet, so the runtime must validate:

- canonicalized existing parent path
- lexical child path under that parent

The runtime must explicitly test:

- `..` traversal attempts
- absolute path attempts
- symlink escapes

## Cold-Start Policy

The narrower Phase 1 cold-start rule remains in force.

Auto-scan is allowed only when:

- no usable pack exists or the pack state is insufficient for the run
- RepoLens itself is the component being repaired
- the scan is needed to restore RepoLens-first perception for this same repo

This runtime is specifically for RepoLens self-bootstrap, so that qualifier is normally satisfied. It should still be stated explicitly in the report, not silently widened into a generic “pack missing implies scan” rule.

## Runtime Semantics By Skill

### `discover_repo_state`

Phase 1 uses internal RepoLens APIs, not MCP transport:

- `.repolens/session.json`
- latest pack discovery/resolution
- git metadata
- `ScanService` if cold-start recovery is needed

Output includes:

- pack availability
- session/pack location
- cold-start decision
- evidence records

### `plan_with_trust`

Phase 1 planning is a bounded run-plan record, not an LLM planner.

For `docs_only`:

- no structural evidence is required
- the plan still records trust-relevant evidence for any policy or shipped-behavior claim

For `trust_contract`:

- the plan records the trust-policy inputs and any relevant shipped-vs-direction sources

### `execute_bounded_change`

Phase 1 does not edit files.

In this runtime, `execute_bounded_change` means:

- freeze the approved boundary at start
- let the external agent edit within that boundary
- validate the actual diff at finish

### `verify_and_rescan`

This phase:

- runs the frozen required checks
- captures stdout/stderr logs per command
- rescans through `ScanService`, preferring incremental mode (`repolens-app::incremental`) when a valid base pack exists and the baseline commit is reachable; falls back to a full scan otherwise
- records the resulting pack id and pack path

`commands.scan` from `bootstrap.yaml` is bookkeeping-only in Phase 1 because rescan uses internal APIs directly. The frozen contract should still preserve it for visibility and future parity.

### `write_anchored_memory`

Deferred in Phase 1. Reports should record `memoryDecision: "deferred"`.

## Command Execution Model

The runtime must not execute commands through a shell.

Phase 1 should normalize the command contract to argv arrays:

```yaml
commands:
  fmt: ["cargo", "fmt", "--all"]
  test: ["cargo", "test"]
```

The current string form in `bootstrap.yaml` should be treated as migration input only. The loader may temporarily parse strings into argv vectors for compatibility, but the runtime uses only the normalized argv form after freeze.

`BootstrapCommandRunner` should:

- execute argv directly with `std::process::Command`
- inherit the parent environment by default
- preserve `PATH` and related toolchain env vars
- capture stdout/stderr separately
- record exit code and log file paths
- return a clean failure if the executable cannot be found

Missing `cargo` on `PATH` must become a normal reportable command failure, not a panic.

## Storage Layout

Each run gets a dedicated directory:

```text
.repolens/bootstrap/
  ACTIVE
  CURRENT
  run-<timestamp>-<uuid>/
    plan.json
    report.json
    commands/
      <check>.stdout.log
      <check>.stderr.log
```

`plan.json` and `report.json` both use:

- camelCase field names (matching the rest of the project's JSON contracts)
- explicit `schemaVersion`, initial value `"1"`

Run directory contents are immutable after `start`, with two exceptions: `report.json` is written once at `finish` or `abort`, and per-command log files are written while required checks run. Agents must not edit `plan.json` or alter the run directory layout; doing so is undefined behavior and may invalidate the run.

`report.json` must include at least:

- `schemaVersion`
- `runId`
- `status`
- `taskType`
- `goal`
- `baselineHeadCommit`
- `finalHeadCommit`
- `contractVersion` (the `version` field from the frozen `bootstrap.yaml`)
- `baselineContractHash`
- `liveContractChanged`
- selected skills
- intended path patterns
- actual changed files
- path validation result
- command results
- cold-start decision
- rescan result
- resulting pack path or pack id
- `memoryDecision`
- terminal timestamps

## App Architecture

`repolens-app` should add a `bootstrap` module with these responsibilities:

- `BootstrapService`
  - public `start`, `finish`, and `abort` entrypoints
- `BootstrapRunStore`
  - read/write run directories and active markers
- `BootstrapPlanner`
  - build frozen plan snapshots from current inputs and contract data
- `WorktreeGuard`
  - clean-worktree check, baseline capture, finish-time diff validation
- `BootstrapCommandRunner`
  - required-check execution and log capture
- `BootstrapReporter`
  - terminal report assembly

`repolens-cli` should stay thin and map CLI args to `BootstrapService`.

`repolens-core` should continue owning:

- bootstrap contract types
- loader and validation
- frozen contract normalization types if they are durable enough to share

## Failure Model

### `start` failures

`start` fails when:

- task type is unsupported
- intended path patterns are invalid or unsafe
- repo is not a git worktree
- worktree is dirty
- another bootstrap run is active
- contract load or normalization fails
- no usable pack exists and narrow cold-start recovery fails

### `finish` failures

`finish` fails when:

- no active run exists
- requested run id is unknown
- current `HEAD` differs from `baselineHeadCommit`
- actual changed files escape the approved boundary
- required checks fail
- rescan fails
- the bounded diff is empty (Phase 1 requires at least one changed file at `finish`; a no-op run must be closed via `abort`, not `finish`)

Every failure still writes or updates `report.json` unless the run directory itself is missing.

### Idempotency

`finish` should be idempotent for terminal runs:

- if the report already records `succeeded`, `failed`, `aborted`, or `invalidated_by_head_change`, a second `finish` returns the stored report instead of rerunning commands

## Testing Strategy

Phase 1 should ship with:

### Unit tests

- command normalization from string and array forms
- path normalization and safe pattern matching
- active-run lock semantics
- state transition rules for `start`, `finish`, and `abort`
- terminal report status assembly

### Integration tests in `repolens-app`

- clean `start` plus bounded docs change plus successful `finish`
- dirty worktree rejected at `start`
- concurrent `start` calls in one worktree reject the second run
- `finish` rejects out-of-bounds changed files
- `finish` rejects `HEAD` movement after `start`
- live contract mutation after `start` still uses frozen snapshot
- `finish` twice returns the stored terminal report
- cold-start scan path when `.repolens/` is absent and self-repair scope applies
- missing executable on `PATH` becomes a clean command failure
- symlink escape and `..` path escape are rejected

### CLI tests

- `bootstrap start` writes `ACTIVE`, `CURRENT`, and `plan.json`
- `bootstrap finish` can omit `--run` and consume `CURRENT`
- `bootstrap abort` clears an active run and writes an aborted report

## Success Criteria

This design is successful if Phase 1 delivers all of the following:

- an agent can start a bounded bootstrap run from the CLI
- RepoLens freezes and reports the policy and diff baseline at start
- the agent can edit files externally within that boundary
- RepoLens can finish the run with real path validation, real verification commands, and a real rescan
- every run produces an inspectable durable report
- the runtime makes no false claim that RepoLens itself authored the edits

## Follow-Up Work

Intentionally deferred after Phase 1:

- `bounded_rust_change`
- automatic anchored memory writes
- richer evidence capture from verified MCP tools inside the runtime
- broader task families
- MCP-native bootstrap control-plane entrypoints
- multi-run coordination beyond “one active run per worktree”
