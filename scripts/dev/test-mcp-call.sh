#!/usr/bin/env bash
# scripts/dev/test-mcp-call.sh — hermetic smoke test for mcp-call.sh.
#
# Builds the binary if needed, runs a scan to ensure a pack exists, then
# calls discover_packs via mcp-call.sh and asserts the response contains
# a latestPack field. Exits non-zero on any failure.

set -euo pipefail

log() { echo "[test-mcp-call] $*" >&2; }
fail() { echo "FAIL: $*" >&2; exit 1; }

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

log "building debug binary"
cargo build --quiet --bin repolens

log "ensuring a pack exists"
cargo run --quiet -- scan --root . >/dev/null

log "calling discover_packs via scripts/dev/mcp-call.sh"
OUTPUT="$(./scripts/dev/mcp-call.sh discover_packs '{"root":"."}')"

[[ -n "$OUTPUT" ]] || fail "mcp-call.sh returned empty output"

if ! echo "$OUTPUT" | python3 -c '
import json, sys
payload = json.loads(sys.stdin.read())
data = payload.get("data", payload)
assert "latestPack" in data, "missing latestPack: " + str(list(data.keys()))
sys.stderr.write("latestPack=" + str(data["latestPack"]) + "\n")
'; then
  fail "response did not contain latestPack: $OUTPUT"
fi

# --- Non-git snapshot case ---------------------------------------------------
# eval-blind-workspace.sh produces `git archive` extracts with no .git; the
# helper must still resolve its repo root (script-relative fallback) there.
# Regression test for the 2026-06-10 blind-run failure ("Not inside a git
# worktree") that made the MCP surface unreachable in an eval snapshot.
log "non-git snapshot case: helper must work without a .git directory"
SNAP="$(mktemp -d "${TMPDIR:-/tmp}/mcp-call-nongit.XXXXXX")"
trap 'rm -rf "$SNAP"' EXIT
git archive HEAD | tar -x -C "$SNAP"
# Overlay the WORKING-TREE helper: `git archive` emits committed state, but
# this test must exercise the script as it stands (pre-commit).
cp "$REPO_ROOT/scripts/dev/mcp-call.sh" "$SNAP/scripts/dev/mcp-call.sh"
mkdir -p "$SNAP/target/debug"
cp "$REPO_ROOT/target/debug/repolens" "$SNAP/target/debug/repolens"
"$SNAP/target/debug/repolens" scan --root "$SNAP" >/dev/null

NONGIT_OUTPUT="$(cd "$SNAP" && ./scripts/dev/mcp-call.sh discover_packs "{\"root\":\"$SNAP\"}")"
[[ -n "$NONGIT_OUTPUT" ]] || fail "mcp-call.sh returned empty output in non-git snapshot"

if ! echo "$NONGIT_OUTPUT" | python3 -c '
import json, sys
payload = json.loads(sys.stdin.read())
data = payload.get("data", payload)
assert "latestPack" in data, "missing latestPack: " + str(list(data.keys()))
sys.stderr.write("non-git latestPack=" + str(data["latestPack"]) + "\n")
'; then
  fail "non-git response did not contain latestPack: $NONGIT_OUTPUT"
fi

log "PASS"
