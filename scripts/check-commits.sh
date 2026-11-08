#!/usr/bin/env bash
# Validates that all commits in the range follow Conventional Commits.
# Usage: scripts/check-commits.sh <base>..<head>
set -euo pipefail

range="${1:?usage: check-commits.sh <base>..<head>}"
pattern='^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9._-]+\))?!?: .+'

# Fails if the range is invalid (process substitution would hide the error).
commits="$(git log --no-merges --format='%h %s' "$range")"

bad=0
while IFS= read -r line; do
    [ -z "$line" ] && continue
    subject="${line#* }"
    if ! printf '%s\n' "$subject" | grep -Eq "$pattern"; then
        echo "✗ commit does not follow the convention: $line"
        bad=1
    fi
done <<< "$commits"

if [ "$bad" -ne 0 ]; then
    echo
    echo "Use Conventional Commits, e.g.: 'feat(pomodoro): allow multiple pomodoros'"
    exit 1
fi
echo "✓ all commits follow Conventional Commits"
