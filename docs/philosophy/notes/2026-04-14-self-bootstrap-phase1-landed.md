# 2026-04-14 — Self-Bootstrap Phase 1 Landed

Authority: Historical
Lifecycle: Frozen

Historical — Frozen 2026-04-14. Preserved as the authoritative record of its topic as of that date.

**Scope:** marks the landing of the first executable self-bootstrap runtime for RepoLens itself.

**Read this if you are:** trying to understand when RepoLens moved from "bootstrap paperwork and contract only" to "a real bounded runtime an agent can drive end-to-end."

---

## Why this is a milestone

This is the first point where RepoLens can participate in its own repair workflow as a product component, not just as a design idea.

Before this arc, the project had:

- a design for self-bootstrap skills
- a machine-readable `bootstrap.yaml` contract
- schema, loader, validation, and registry support in `repolens-core`

But it did **not** have a shipped runtime that could actually open a bounded bootstrap run, freeze its contract, validate what changed, and write a durable terminal report.

After this arc, it does.

That makes this a meaningful project boundary: RepoLens now has a real, inspectable Phase 1 self-bootstrap workflow.

---

## What landed

Phase 1 ships an **agent-driven** bootstrap runtime for:

- `docs_only`
- `trust_contract`

The entrypoints are:

- `repolens bootstrap start`
- `repolens bootstrap finish`
- `repolens bootstrap abort`

The shipped behavior is:

1. `start` requires a clean worktree, captures a git baseline, freezes the effective bootstrap contract, and writes `plan.json`.
2. RepoLens records the bounded intended path set and selected skill ids for the run.
3. RepoLens can cold-start its own perception when allowed by the narrow self-repair rule.
4. An external agent performs the actual file edits.
5. `finish` validates the real changed paths against the frozen path boundary.
6. Required checks run under the frozen start-time contract, not the live contract file.
7. RepoLens rescans and writes a terminal `report.json`.
8. `abort` writes a terminal aborted report and clears run markers cleanly.

This runtime is durable and inspectable by construction:

- `plan.json` is the frozen run contract
- `report.json` is the final run outcome
- `ACTIVE` / `CURRENT` markers model run ownership and are cleared on terminal transitions

---

## What did not land

Phase 1 is intentionally narrow.

RepoLens still does **not**:

- edit files itself
- perform autonomous planning with an embedded model
- write memory automatically
- ship `bounded_rust_change` as a live bootstrap runtime path

`write_anchored_memory` remains deferred. The runtime is honest about the current product boundary: RepoLens owns orchestration, policy freeze, verification, rescans, and reporting; the external agent still owns the concrete code or docs edit.

---

## Why the boundary matters

This arc matters because it converts "self-bootstrap" from narration into product behavior without overstating what the product can do.

The runtime now enforces:

- start-time contract freeze
- git baseline capture
- one-active-run-per-worktree semantics
- bounded path validation
- durable success and failure reporting

That means an agent can now use RepoLens to run a constrained self-bootstrap workflow and get a machine-readable answer about whether the attempted change stayed in bounds and passed the required checks.

This is not the end state. It is the first trustworthy runtime slice.

---

## Project meaning

The practical meaning of this milestone is:

> RepoLens can now supervise a bounded self-repair workflow on RepoLens itself.

Not:

> RepoLens is now a fully autonomous self-improving coding system.

That distinction is the important one to preserve in future narration.

---

## Suggested tag

If you want a durable git marker for this moment, tag the **merge commit** on `main`, not the feature branch tip.

Recommended tag:

`milestone/self-bootstrap-phase1`

Recommended annotation:

> Phase 1 self-bootstrap runtime landed
>
> - adds bootstrap start|finish|abort
> - freezes plan/report contract
> - ships bounded docs_only and trust_contract workflow
> - enforces path guards, contract freeze, and rescan

---

## Governance note

This note should be read together with:

- [07-current-state-vs-direction.md](../07-current-state-vs-direction.md)
- [2026-04-14-self-bootstrap-runtime-design.md](../../superpowers/specs/2026-04-14-self-bootstrap-runtime-design.md)

The point of this milestone is not just that new code landed. The point is that a new runtime boundary landed and can now be described accurately.
