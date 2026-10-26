#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_SCRIPT="$REPO_ROOT/scripts/dogfood-snapshot.sh"

require_source_script() {
  if [[ ! -x "$SOURCE_SCRIPT" ]]; then
    echo "missing source script: $SOURCE_SCRIPT"
    return 1
  fi
}

write_file() {
  local path="$1"
  shift
  mkdir -p "$(dirname "$path")"
  cat >"$path" <<EOF
$*
EOF
}

make_sandbox() {
  require_source_script

  local sandbox
  sandbox="$(mktemp -d)"
  mkdir -p "$sandbox/bin" "$sandbox/scripts"
  cp "$SOURCE_SCRIPT" "$sandbox/scripts/dogfood-snapshot.sh"
  chmod +x "$sandbox/scripts/dogfood-snapshot.sh"
  printf '%s\n' "$sandbox"
}

init_git_repo() {
  local repo="$1"
  mkdir -p "$repo"
  git -C "$repo" init -q
  git -C "$repo" config user.name "RepoLens Test"
  git -C "$repo" config user.email "repolens@example.com"
  write_file "$repo/README.md" "# sandbox"
  git -C "$repo" add README.md
  git -C "$repo" commit -qm "init"
}

# seed_success_cargo_stub <sandbox> [worktree_dirty=false]
seed_success_cargo_stub() {
  local sandbox="$1"
  local worktree_dirty="${2:-false}"
  write_file "$sandbox/bin/cargo" '#!/usr/bin/env bash
set -euo pipefail

repo_root=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --root)
      repo_root="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [[ -z "$repo_root" ]]; then
  echo "missing --root" >&2
  exit 2
fi

pack_dir="$repo_root/.repolens/pack-20260420-051102"
mkdir -p "$pack_dir"

cat >"$repo_root/.repolens/session.json" <<EOF
{
  "schemaVersion": "1.0",
  "sessionId": "11111111-2222-3333-4444-555555555555",
  "repoRoot": "$repo_root",
  "activePackPath": "$pack_dir",
  "activeReportPath": null,
  "updatedAt": "2026-04-20T05:11:02Z",
  "lastAccessed": "2026-04-20T05:11:02Z",
  "lastScanMode": "full",
  "reprocessedFiles": 56,
  "deletedFiles": null,
  "carriedForwardFiles": null
}
EOF

cat >"$pack_dir/manifest.json" <<EOF
{
  "schemaVersion": "1.3.0",
  "schemaUrl": "https://example.com/schema/1.3.0",
  "timestamp": 1774753241.321919,
  "chunks": ["files-0001.json"],
  "scanMode": "full",
  "scanScope": "complete",
  "basePackId": null,
  "baseHeadCommit": null,
  "headCommit": "0123456789abcdef0123456789abcdef01234567",
  "worktreeDirty": '"$worktree_dirty"',
  "scanConfigHash": "hash",
  "reprocessedFiles": 56,
  "deletedFiles": null,
  "carriedForwardFiles": null
}
EOF

cat >"$pack_dir/project-profile.json" <<EOF
{
  "name": "sandbox",
  "buildSystem": {
    "name": "Cargo",
    "details": null,
    "configFile": "Cargo.toml"
  },
  "frameworks": [],
  "projectType": "CLI",
  "entryPoints": [{
    "type": "main",
    "path": "src/main.rs",
    "description": "Binary entry point"
  }],
  "description": null,
  "languageDistribution": {
    "Rust": 38
  },
  "modules": []
}
EOF

printf "%s\n" "Scan complete (full): 56 files (38 included, 18 excluded) in 0.80s"
printf "%s\n" "Pack written to: $pack_dir"
'
  chmod +x "$sandbox/bin/cargo"
}

seed_failing_cargo_stub() {
  local sandbox="$1"
  write_file "$sandbox/bin/cargo" '#!/usr/bin/env bash
set -euo pipefail
echo "scan exploded" >&2
exit 17
'
  chmod +x "$sandbox/bin/cargo"
}

run_script() {
  local sandbox="$1"
  local cwd="$2"
  local stdout_file="$sandbox/stdout.md"
  local stderr_file="$sandbox/stderr.txt"

  if (
    cd "$cwd"
    PATH="$sandbox/bin:$PATH" bash "$sandbox/scripts/dogfood-snapshot.sh" >"$stdout_file" 2>"$stderr_file"
  ); then
    return 0
  fi

  return 1
}

assert_contains() {
  local needle="$1"
  local file="$2"
  if ! grep -Fq -- "$needle" "$file"; then
    echo "expected to find '$needle' in $file"
    cat "$file"
    return 1
  fi
}

test_emits_markdown_from_nested_dir() {
  local sandbox repo
  sandbox="$(make_sandbox)"
  trap 'rm -rf "$sandbox"' RETURN
  repo="$sandbox/repo"

  init_git_repo "$repo"
  mkdir -p "$repo/crates/repolens-cli"
  seed_success_cargo_stub "$sandbox"

  run_script "$sandbox" "$repo/crates/repolens-cli"

  assert_contains "## Workflow-Closure Dogfood Snapshot" "$sandbox/stdout.md"
  assert_contains "| Files walked | 56 |" "$sandbox/stdout.md"
  assert_contains "| Files included | 38 |" "$sandbox/stdout.md"
  assert_contains "| Excluded rows | 18 |" "$sandbox/stdout.md"
  assert_contains "| Scan duration | 0.80s |" "$sandbox/stdout.md"
  assert_contains "| Schema version | 1.3.0 |" "$sandbox/stdout.md"
  assert_contains "| Worktree dirty | false |" "$sandbox/stdout.md"
  assert_contains "### Notable Profile Entry Points" "$sandbox/stdout.md"
  assert_contains '- `src/main.rs` (`main`) — Binary entry point' "$sandbox/stdout.md"
}

test_emits_markdown_for_dirty_worktree() {
  local sandbox repo
  sandbox="$(make_sandbox)"
  trap 'rm -rf "$sandbox"' RETURN
  repo="$sandbox/repo"

  init_git_repo "$repo"
  seed_success_cargo_stub "$sandbox" true

  run_script "$sandbox" "$repo"

  assert_contains "| Worktree dirty | true |" "$sandbox/stdout.md"
}

test_scan_failure_has_no_partial_stdout() {
  local sandbox repo
  sandbox="$(make_sandbox)"
  trap 'rm -rf "$sandbox"' RETURN
  repo="$sandbox/repo"

  init_git_repo "$repo"
  seed_failing_cargo_stub "$sandbox"

  if run_script "$sandbox" "$repo"; then
    echo "expected script failure but it passed"
    return 1
  fi

  [[ ! -s "$sandbox/stdout.md" ]] || {
    echo "expected no stdout on failure"
    cat "$sandbox/stdout.md"
    return 1
  }

  assert_contains "Scan command failed" "$sandbox/stderr.txt"
}

test_requires_git_worktree() {
  local sandbox repo
  sandbox="$(make_sandbox)"
  trap 'rm -rf "$sandbox"' RETURN
  repo="$sandbox/repo"

  mkdir -p "$repo"
  seed_success_cargo_stub "$sandbox"

  if run_script "$sandbox" "$repo"; then
    echo "expected non-git failure but it passed"
    return 1
  fi

  [[ ! -s "$sandbox/stdout.md" ]] || {
    echo "expected no stdout outside git worktree"
    cat "$sandbox/stdout.md"
    return 1
  }

  assert_contains "Not inside a git worktree" "$sandbox/stderr.txt"
}

test_emits_markdown_from_nested_dir
test_emits_markdown_for_dirty_worktree
test_scan_failure_has_no_partial_stdout
test_requires_git_worktree

echo "dogfood snapshot tests passed"
