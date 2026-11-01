#!/usr/bin/env bash
# Prints the workspace version (Cargo.toml's [workspace.package] version),
# the one every release asset and tag is named after.
set -euo pipefail
version=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
    echo "no valid workspace version in Cargo.toml: '$version'" >&2
    exit 1
}
echo "$version"
