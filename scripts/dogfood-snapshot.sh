#!/usr/bin/env bash
set -euo pipefail

# mapfile below needs bash 4+; macOS ships 3.2.
if (( BASH_VERSINFO[0] < 4 )); then
  echo "dogfood-snapshot.sh needs bash 4 or newer (this is $BASH_VERSION); on macOS: brew install bash" >&2
  exit 1
fi

log() {
  echo "[dogfood] $*" >&2
}

fail() {
  echo "$*" >&2
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail "Missing required command: $1"
  fi
}

resolve_repo_root() {
  git rev-parse --show-toplevel 2>/dev/null || fail "Not inside a git worktree"
}

# parse_scan_output treats the `repolens scan` stdout format as a stable
# contract. If the CLI's user-facing "Scan complete (...)" / "Pack written
# to: ..." lines are reworded, update this parser in the same change —
# counts and duration are not currently exposed in manifest.json, so
# stdout is the only source for those fields. A longer-term fix is to
# surface them in the manifest and read from there instead.
parse_scan_output() {
  local scan_output="$1"
  local summary_line pack_line

  summary_line="$(printf '%s\n' "$scan_output" | awk '/^Scan complete \(/ { line=$0 } END { print line }')"
  pack_line="$(printf '%s\n' "$scan_output" | awk '/^Pack written to: / { line=$0 } END { print line }')"

  [[ -n "$summary_line" ]] || fail "Could not find scan summary in cargo output"
  [[ -n "$pack_line" ]] || fail "Could not find pack path in cargo output"

  # Regex stored in a variable so bash 5.x's [[ =~ ]] parser does not
  # misinterpret the literal parentheses as grouping tokens.
  local summary_regex='^Scan complete \(([^)]*)\): ([0-9]+) files \(([0-9]+) included, ([0-9]+) excluded\) in ([0-9]+\.[0-9]+)s$'
  if [[ "$summary_line" =~ $summary_regex ]]; then
    SCAN_MODE="${BASH_REMATCH[1]}"
    FILES_WALKED="${BASH_REMATCH[2]}"
    FILES_INCLUDED="${BASH_REMATCH[3]}"
    EXCLUDED_ROWS="${BASH_REMATCH[4]}"
    SCAN_DURATION="${BASH_REMATCH[5]}s"
  else
    fail "Could not parse scan summary: $summary_line"
  fi

  PACK_PATH="${pack_line#Pack written to: }"
}

read_session_pack_path() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    session = json.load(handle)

pack_path = session.get("activePackPath")
if not pack_path:
    raise SystemExit("session.json missing activePackPath")

print(pack_path)
PY
}

read_manifest_fields() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    manifest = json.load(handle)

schema_version = manifest.get("schemaVersion")
worktree_dirty = manifest.get("worktreeDirty")

if schema_version is None:
    raise SystemExit("manifest.json missing schemaVersion")
if worktree_dirty is None:
    raise SystemExit("manifest.json missing worktreeDirty")

print(schema_version)
print(str(worktree_dirty).lower())
PY
}

read_entry_points_markdown() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    profile = json.load(handle)

entry_points = profile.get("entryPoints")
if entry_points is None:
    raise SystemExit("project-profile.json missing entryPoints")

if not entry_points:
    print("- None detected")
    raise SystemExit(0)

for entry in entry_points[:5]:
    entry_type = entry.get("type", "unknown")
    path = entry.get("path")
    description = entry.get("description", "")
    if not path:
        raise SystemExit("project-profile.json entryPoints item missing path")
    print(f"- `{path}` (`{entry_type}`) — {description}")
PY
}

require_command git
require_command cargo
require_command python3
require_command du

REPO_ROOT="$(resolve_repo_root)"
COMMIT_SHORT="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
SESSION_PATH="$REPO_ROOT/.repolens/session.json"

log "Running fresh scan for $REPO_ROOT"
if ! SCAN_OUTPUT="$(cargo run -- scan --root "$REPO_ROOT" 2>&1)"; then
  printf '%s\n' "$SCAN_OUTPUT" >&2
  fail "Scan command failed"
fi

parse_scan_output "$SCAN_OUTPUT"

[[ -f "$SESSION_PATH" ]] || fail "Missing session file: $SESSION_PATH"
SESSION_PACK_PATH="$(read_session_pack_path "$SESSION_PATH")" || fail "Failed to read session.json"
[[ "$SESSION_PACK_PATH" == "$PACK_PATH" ]] || fail "Session activePackPath does not match scan output pack path"

MANIFEST_PATH="$PACK_PATH/manifest.json"
PROFILE_PATH="$PACK_PATH/project-profile.json"
[[ -f "$MANIFEST_PATH" ]] || fail "Missing manifest file: $MANIFEST_PATH"
[[ -f "$PROFILE_PATH" ]] || fail "Missing project profile file: $PROFILE_PATH"

mapfile -t MANIFEST_FIELDS < <(read_manifest_fields "$MANIFEST_PATH") || fail "Failed to read manifest.json"
SCHEMA_VERSION="${MANIFEST_FIELDS[0]}"
WORKTREE_DIRTY="${MANIFEST_FIELDS[1]}"
PACK_SIZE="$(du -sh "$PACK_PATH" | awk '{print $1}')"
PACK_ID="$(basename "$PACK_PATH")"
ENTRY_POINTS_MARKDOWN="$(read_entry_points_markdown "$PROFILE_PATH")" || fail "Failed to read project-profile.json"

cat <<EOF
## Workflow-Closure Dogfood Snapshot

Commit: \`$COMMIT_SHORT\` | Pack: \`$PACK_ID\` | Scan mode: \`$SCAN_MODE\`

| Field | Value |
| --- | --- |
| Files walked | $FILES_WALKED |
| Files included | $FILES_INCLUDED |
| Excluded rows | $EXCLUDED_ROWS |
| Pack size | $PACK_SIZE |
| Scan duration | $SCAN_DURATION |
| Schema version | $SCHEMA_VERSION |
| Worktree dirty | $WORKTREE_DIRTY |

### Notable Profile Entry Points

$ENTRY_POINTS_MARKDOWN
EOF
