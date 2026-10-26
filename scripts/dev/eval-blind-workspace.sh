#!/usr/bin/env bash
# eval-blind-workspace.sh — Create an isolated workspace for a BLIND ACE eval run.
#
# Why: a rubric-blind agent told to investigate this repo will find and read the
# prior run records under docs/evals/ — they name the task, the "right" answer,
# and earlier agents' choices, so reading them contaminates the result. This was
# observed directly in docs/evals/runs/ACE-011-postsurface-v4.md, where a blind
# sub-agent read a prior run record as an answer key, voiding that run's
# consume/bind measurement.
#
# This produces a clean snapshot of the repo at HEAD with docs/evals/ removed.
# Point the blind agent at the printed path as its repo root.
#
# Usage:
#   scripts/dev/eval-blind-workspace.sh [dest-dir]
#     dest-dir  optional; defaults to a fresh temp dir (printed on completion).
set -euo pipefail

ROOT="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"

if [ "$#" -ge 1 ]; then
  DEST="$1"
  rm -rf "$DEST"
  mkdir -p "$DEST"
else
  DEST="$(mktemp -d "${TMPDIR:-/tmp}/repolens-blind-eval.XXXXXX")"
fi

# `git archive HEAD` emits committed state only: no .repolens/ packs, no
# untracked scratch, and — the point here — a clean tree we can strip the eval
# corpus from. Untracked local files (which could also leak context) never enter.
git -C "$ROOT" archive HEAD | tar -x -C "$DEST"
rm -rf "$DEST/docs/evals"

if [ -e "$DEST/docs/evals" ]; then
  echo "ERROR: docs/evals/ still present in $DEST after strip" >&2
  exit 1
fi

echo "Blind eval workspace ready: $DEST"
echo "  - snapshot of HEAD ($(git -C "$ROOT" rev-parse --short HEAD)) with docs/evals/ removed"
echo "  - point the blind agent at this path as its repo root, then discard it"
