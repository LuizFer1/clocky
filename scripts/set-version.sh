#!/usr/bin/env bash
# Writes the version (e.g. v0.3.0 or 0.3.0) to the workspace Cargo.toml and Cargo.lock.
# Used only in the release pipeline; the change is not committed.
set -euo pipefail

version="${1#v}"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "invalid version: '$1'" >&2
    exit 1
fi

sed -i.bak -E "s/^version = \"[^\"]*\"/version = \"$version\"/" Cargo.toml
rm -f Cargo.toml.bak
cargo update --workspace
grep -E '^version = ' Cargo.toml
