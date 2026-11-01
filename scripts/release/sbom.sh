#!/usr/bin/env bash
# Writes the release's CycloneDX SBOM to OUT_DIR: the msgvault-moonframe
# executable's whole locked dependency graph, Moonframe included, across
# every target and feature.
set -euo pipefail
[[ $# == 1 ]] || { echo "usage: sbom.sh OUT_DIR" >&2; exit 2; }
out=$1
version=$(scripts/release/version.sh)
mkdir -p "$out"
name=msgvault-moonframe-release-sbom
trap 'rm -f crates/*/"$name".json' EXIT

cargo cyclonedx -q --manifest-path crates/msgvault-moonframe/Cargo.toml --format json \
    --all-features --target all --spec-version 1.5 --override-filename "$name"
mv "crates/msgvault-moonframe/$name.json" "$out/msgvault-moonframe-$version.cdx.json"
# The lockfile must not have moved: the SBOM describes what was built.
git diff --exit-code -- Cargo.lock
ls "$out"
