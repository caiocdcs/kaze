# kaze task runner — `just` lists recipes.
# Inside `nix develop` (or direnv) every tool used here is on PATH.

set dotenv-load := true

default:
    @just --list

# Format all Rust code
fmt:
    cargo fmt --all

# Formatting + lints, as CI runs them
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- --deny warnings

# Run the test suite
test *args:
    cargo nextest run --workspace --no-tests=pass {{args}}

# Run tests that hit real websites (ignored by default)
test-live *args:
    cargo nextest run --workspace --run-ignored only {{args}}

# Run the kaze CLI, e.g. `just run serve`
run *args:
    cargo run --package kaze -- {{args}}

# Rebuild and restart the server on change
watch:
    cargo watch --exec 'run --package kaze -- serve'

# Release build of the binary
build:
    cargo build --release --package kaze

# Nix build (same as CI); result in ./result
nix-build:
    nix build

# Everything CI checks, through Nix
ci:
    nix flake check --print-build-logs
