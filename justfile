default:
    @just --list

# rustfmt.toml (shared with moltis) uses unstable options, so formatting runs on
# nightly while builds, clippy and tests use the stable rust-toolchain.toml.
nightly_toolchain := "nightly-2026-06-20"

# Browse your archive (a `msgvault serve` daemon, else $MSGVAULT_HOME or ~/.msgvault).
run *args:
    cargo run --release -- {{args}}

# Open msgvault-core's generated demo archive instead of yours.
demo:
    MSGVAULT_DEMO=1 cargo run --release

# Load the app headlessly: every mod, component and provider, without a window.
check:
    MSGVAULT_DEMO=1 cargo run -- --check

# Write a synthetic archive (real msgvault schema) into DIR.
fixture dir="demo-archive":
    cargo run -p msgvault-core --bin msgvault-fixture -- {{dir}}

# Format Rust (nightly rustfmt) and TOML (taplo).
format:
    cargo +{{nightly_toolchain}} fmt --all
    taplo fmt

# Check Rust and TOML formatting without writing.
format-check:
    cargo +{{nightly_toolchain}} fmt --all -- --check
    taplo fmt --check

# What CI runs.
ci: format-check
    cargo fetch --locked
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --workspace
    just check

# Licenses, advisories, bans and sources.
deny:
    cargo deny check

# Audit the GitHub workflows (zizmor, actionlint) and the dependency graph
# (cargo-deny) as CI does.
audit:
    zizmor .github/
    actionlint
    cargo deny check advisories bans licenses sources

# Build this Mac's release packages, unsigned, as .github/workflows/package.yml
# does: msgvault.app and its disk image in target/dist.
package-macos out="target/dist":
    cargo build --locked --release
    scripts/release/package-macos.sh {{out}} target/release/msgvault-moonframe

# Render the app icon (assets/icon.png, app/icon.png) from assets/icon.svg.
icon:
    rsvg-convert -w 1024 -h 1024 assets/icon.svg -o assets/icon.png
    cp assets/icon.png crates/msgvault-moonframe/app/icon.png

# Publish main's HEAD as v<version in Cargo.toml> (bump it first) through the
# Release workflow, which waits for CI on that commit; see docs/releasing.md.
release:
    #!/usr/bin/env bash
    set -euo pipefail
    git fetch --quiet origin main
    sha=$(git rev-parse origin/main)
    version=$(scripts/release/version.sh)
    test "$(git rev-parse HEAD)" = "$sha" || { echo "check out origin/main ($sha) first" >&2; exit 1; }
    test -z "$(git status --porcelain)" || { echo "the working tree is not clean" >&2; exit 1; }
    echo "Releasing v$version from $sha"
    gh workflow run release.yml --ref main -f expected_sha="$sha"
