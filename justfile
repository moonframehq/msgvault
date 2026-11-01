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
