#!/usr/bin/env bash
# scripts/dev/mcp-call.sh — one-shot stdio MCP call against `cargo run -- mcp`.
#
# Usage: scripts/dev/mcp-call.sh <tool-name> <json-args>
# Example: scripts/dev/mcp-call.sh discover_packs '{"root":"."}'
#
# Prints the tool result's text content to stdout. Build noise and RPC
# errors go to stderr. Exits non-zero on RPC error.
#
# Maintainer-local helper, NOT a product surface. Exists so the agent-eval
# loop has a stable, committed channel for MCP tool invocations.

set -euo pipefail

log() { echo "[mcp-call] $*" >&2; }
fail() { echo "$*" >&2; exit 1; }

[[ $# -eq 2 ]] || fail "usage: $0 <tool-name> <json-args>"

TOOL_NAME="$1"
TOOL_ARGS="$2"

# Resolve the repo root this script belongs to: git first (worktree-aware),
# falling back to the script's own location (scripts/dev/ -> two dirs up) so
# the helper also works in non-git snapshots — e.g. the `git archive` extracts
# eval-blind-workspace.sh produces, where the git form fails and previously
# made the MCP surface unreachable (2026-06-10 blind run).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" \
  || REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
if [[ -x "$TARGET_DIR/debug/repolens" ]]; then
  BIN=("$TARGET_DIR/debug/repolens" mcp)
elif [[ -x "$TARGET_DIR/release/repolens" ]]; then
  BIN=("$TARGET_DIR/release/repolens" mcp)
else
  log "no pre-built binary at $TARGET_DIR; falling back to cargo run"
  BIN=(cargo run --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -- mcp)
fi

python3 - "$TOOL_NAME" "$TOOL_ARGS" "${BIN[@]}" <<'PYEOF'
import json, subprocess, sys

tool_name = sys.argv[1]
tool_args = json.loads(sys.argv[2])
cmd = sys.argv[3:]

proc = subprocess.Popen(
    cmd,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
    text=True,
)

def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()

def recv():
    line = proc.stdout.readline()
    if not line:
        sys.exit("[mcp-call] EOF from server before response")
    return json.loads(line)

send({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "mcp-call.sh", "version": "0.1"},
    },
})
init_resp = recv()
if "error" in init_resp:
    sys.exit(f"[mcp-call] initialize failed: {init_resp['error']}")

send({"jsonrpc": "2.0", "method": "notifications/initialized"})

send({
    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
    "params": {"name": tool_name, "arguments": tool_args},
})
call_resp = recv()

proc.stdin.close()
proc.wait(timeout=10)

if "error" in call_resp:
    print(json.dumps(call_resp["error"], indent=2), file=sys.stderr)
    sys.exit(1)

for item in call_resp.get("result", {}).get("content", []):
    if item.get("type") == "text":
        print(item.get("text", ""))
PYEOF
