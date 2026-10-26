#!/usr/bin/env bash
# capture-fixtures.sh — Run Java RepoLens scan against a fixture repo and
# capture the generated pack, session, and memory outputs into the
# fixtures/scenarios/ directory for use as golden test data.
#
# Usage:
#   ./scripts/capture-fixtures.sh --jar <path-to-jar> --scenario <name> --repo <fixture-input-repo>
#
# Example:
#   ./scripts/capture-fixtures.sh \
#     --jar target/repolens-0.1.0-SNAPSHOT.jar \
#     --scenario java-small \
#     --repo repolens-rs/fixtures/inputs/java-small

set -euo pipefail

JAR=""
SCENARIO=""
REPO=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --jar)      JAR="$2"; shift 2 ;;
    --scenario) SCENARIO="$2"; shift 2 ;;
    --repo)     REPO="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$JAR" || -z "$SCENARIO" || -z "$REPO" ]]; then
  echo "Usage: $0 --jar <path> --scenario <name> --repo <fixture-input-repo>" >&2
  exit 1
fi

# Resolve to absolute paths
JAR="$(cd "$(dirname "$JAR")" && pwd)/$(basename "$JAR")"
REPO="$(cd "$REPO" && pwd)"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FIXTURES_DIR="$(cd "$SCRIPT_DIR/../fixtures" && pwd)"
SCENARIO_DIR="$FIXTURES_DIR/scenarios/$SCENARIO"

echo "=== Capturing fixtures for scenario: $SCENARIO ==="
echo "  JAR:  $JAR"
echo "  REPO: $REPO"
echo "  OUT:  $SCENARIO_DIR"

# 1. Clean any previous .repolens state from the fixture repo
rm -rf "$REPO/.repolens"

# 2. Clean previous scenario output
rm -rf "$SCENARIO_DIR"
mkdir -p "$SCENARIO_DIR/pack" "$SCENARIO_DIR/repo"

# 3. Run the Java scan
echo "--- Running scan ---"
java -jar "$JAR" scan --root "$REPO" 2>&1 | tail -5

# 4. Find the generated pack directory (most recent pack-*)
PACK_DIR=$(ls -1d "$REPO/.repolens"/pack-* 2>/dev/null | sort | tail -1)
if [[ -z "$PACK_DIR" ]]; then
  echo "ERROR: No pack directory found after scan" >&2
  exit 1
fi
echo "  Pack: $PACK_DIR"

# 5. Copy pack contents (manifest, chunks, graph, profile, content files)
cp -R "$PACK_DIR"/* "$SCENARIO_DIR/pack/"
echo "  Copied pack contents"

# 6. Copy session.json
if [[ -f "$REPO/.repolens/session.json" ]]; then
  cp "$REPO/.repolens/session.json" "$SCENARIO_DIR/repo/session.json"
  echo "  Copied session.json"
else
  echo "  WARNING: No session.json found"
fi

# 7. Copy memory.json if it exists
if [[ -f "$REPO/.repolens/memory/memory.json" ]]; then
  mkdir -p "$SCENARIO_DIR/repo"
  cp "$REPO/.repolens/memory/memory.json" "$SCENARIO_DIR/repo/memory.json"
  echo "  Copied memory.json"
else
  echo "  No memory.json (expected for fresh scan)"
fi

# 8. MCP capture (optional — skip on failure)
echo "--- Attempting MCP capture ---"
MCP_DIR="$SCENARIO_DIR/mcp"
mkdir -p "$MCP_DIR"

capture_mcp_tool() {
  local tool_name="$1"
  local arguments="$2"
  local output_file="$MCP_DIR/${tool_name}.json"

  # Create a temp file with the MCP conversation
  local tmpfile
  tmpfile=$(mktemp)

  cat > "$tmpfile" <<MCPEOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"fixture-capture","version":"0.1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"${tool_name}","arguments":${arguments}}}
MCPEOF

  # Run MCP server with input, capture stdout, filter for JSON-RPC responses
  local mcp_output
  if mcp_output=$(timeout 30 java -jar "$JAR" mcp < "$tmpfile" 2>/dev/null); then
    # Extract the response to id:2 (the tool call response)
    local tool_response
    tool_response=$(echo "$mcp_output" | grep '"id":2' | head -1)
    if [[ -n "$tool_response" ]]; then
      echo "$tool_response" > "$output_file"
      echo "  Captured MCP: $tool_name"
    else
      echo "  WARNING: No tool response for $tool_name"
      rm -f "$output_file"
    fi
  else
    echo "  WARNING: MCP capture failed for $tool_name (timeout or error)"
    rm -f "$output_file"
  fi
  rm -f "$tmpfile"
}

# Capture a few key MCP tool responses
capture_mcp_tool "discover_packs" "{\"root\":\"$REPO\"}"
capture_mcp_tool "get_summary" "{\"root\":\"$REPO\"}"

# Only capture search_code and read_file_content for scenarios that have source files
if [[ -d "$REPO/src" ]]; then
  capture_mcp_tool "search_code" "{\"root\":\"$REPO\",\"query\":\"class\"}"
fi

# Clean up MCP dir if empty
if [[ -z "$(ls -A "$MCP_DIR" 2>/dev/null)" ]]; then
  rmdir "$MCP_DIR"
  echo "  No MCP captures succeeded (this is OK)"
fi

# 9. Clean up .repolens from fixture repo
rm -rf "$REPO/.repolens"

echo "=== Done: $SCENARIO ==="
echo ""
