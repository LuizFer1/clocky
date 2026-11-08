#!/usr/bin/env bash
# Prints the next version (e.g. v0.3.0), or nothing if no commits trigger a release.
# Releasable: feat, fix, perf and any breaking change (`type!:` or `BREAKING CHANGE:`).
set -euo pipefail

last="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
range="${last:+$last..}HEAD"

releasable='^(feat|fix|perf)(\([^)]*\))?!?: |^[a-z]+(\([^)]*\))?!: |^BREAKING[ -]CHANGE: '
if ! git log --no-merges --format='%s%n%b' "$range" | grep -Eq "$releasable"; then
    exit 0
fi

next="$(git cliff --bumped-version)"
if ! [[ "$next" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "git-cliff returned an invalid version: '$next'" >&2
    exit 1
fi
echo "$next"
