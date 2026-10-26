#!/usr/bin/env bash
# refresh-fixture-inputs.sh
#
# Initializes each fixture input repo as a deterministic Git repository.
# Safe to run multiple times — skips git init if .git already exists.
#
# Determinism guarantees:
#   - Fixed commit date: 2026-01-01T00:00:00+00:00
#   - Fixed author: Test User <test@example.com>
#   - Leaves working tree clean

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES_DIR="$(cd "${SCRIPT_DIR}/../fixtures/inputs" && pwd)"

FIXED_DATE="2026-01-01T00:00:00+00:00"
AUTHOR_NAME="Test User"
AUTHOR_EMAIL="test@example.com"

SCENARIOS=(
    "java-small"
    "mixed-small"
    "excluded-small"
)

init_fixture_repo() {
    local scenario="$1"
    local repo_dir="${FIXTURES_DIR}/${scenario}"

    echo "--- ${scenario} ---"

    if [ ! -d "${repo_dir}" ]; then
        echo "  ERROR: directory not found: ${repo_dir}" >&2
        return 1
    fi

    if [ -d "${repo_dir}/.git" ]; then
        echo "  .git already exists, skipping init"
    else
        echo "  Initializing git repo"
        git -C "${repo_dir}" init -q
        git -C "${repo_dir}" config user.name "${AUTHOR_NAME}"
        git -C "${repo_dir}" config user.email "${AUTHOR_EMAIL}"
    fi

    # Stage all files (including ignored ones that are already tracked, but not .gitignored untracked)
    git -C "${repo_dir}" add -A

    # Check if there is anything to commit
    if git -C "${repo_dir}" diff --cached --quiet 2>/dev/null; then
        echo "  Nothing to commit, working tree already clean"
        return 0
    fi

    echo "  Creating deterministic initial commit"
    GIT_AUTHOR_NAME="${AUTHOR_NAME}" \
    GIT_AUTHOR_EMAIL="${AUTHOR_EMAIL}" \
    GIT_AUTHOR_DATE="${FIXED_DATE}" \
    GIT_COMMITTER_NAME="${AUTHOR_NAME}" \
    GIT_COMMITTER_EMAIL="${AUTHOR_EMAIL}" \
    GIT_COMMITTER_DATE="${FIXED_DATE}" \
        git -C "${repo_dir}" commit -q -m "initial commit"

    echo "  Done"
}

echo "Refreshing fixture input repos in: ${FIXTURES_DIR}"
echo ""

for scenario in "${SCENARIOS[@]}"; do
    init_fixture_repo "${scenario}"
done

echo ""
echo "Verifying clean working trees..."
all_clean=true
for scenario in "${SCENARIOS[@]}"; do
    repo_dir="${FIXTURES_DIR}/${scenario}"
    status="$(git -C "${repo_dir}" status --short 2>&1)"
    if [ -n "${status}" ]; then
        echo "  FAIL: ${scenario} has unclean working tree:"
        echo "${status}" | sed 's/^/    /'
        all_clean=false
    else
        echo "  OK: ${scenario}"
    fi
done

echo ""
if [ "${all_clean}" = true ]; then
    echo "All fixture repos are clean."
else
    echo "ERROR: Some repos have unclean working trees." >&2
    exit 1
fi
