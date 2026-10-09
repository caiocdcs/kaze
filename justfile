# kaze task runner — `just` lists recipes.
# Inside `nix develop` (or direnv) every tool used here is on PATH.

set dotenv-load := true

default:
    @just --list

# Format all Rust code
fmt:
    cargo fmt --all

migrations := "crates/kaze/migrations"

# Create/migrate the dev database that sqlx checks queries against
db:
    @mkdir -p target
    sqlx database setup --source {{migrations}}

# Drop and recreate the dev database
db-reset:
    sqlx database reset -y --source {{migrations}}

# Refresh .sqlx/ (offline query metadata used by Nix and CI); commit the result
sqlx-prepare: db
    cargo sqlx prepare --workspace -- --all-targets

# Formatting + lints + up-to-date query metadata, as CI runs them
check: db
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- --deny warnings
    cargo sqlx prepare --workspace --check -- --all-targets

# Run the test suite
test *args: db
    cargo nextest run --workspace --no-tests=pass {{args}}

# Run tests that hit real websites (ignored by default)
test-live *args:
    cargo nextest run --workspace --run-ignored only {{args}}

# Run the kaze CLI, e.g. `just run search one piece`
run *args:
    cargo run --package kaze -- {{args}}

# Run the server with data and library under target/dev
serve:
    KAZE_DATA_DIR=target/dev/data KAZE_LIBRARY_DIR=target/dev/library cargo run --package kaze -- serve

# Rebuild and restart the server on change
watch:
    KAZE_DATA_DIR=target/dev/data KAZE_LIBRARY_DIR=target/dev/library cargo watch --exec 'run --package kaze -- serve'

# Release build of the binary
build:
    cargo build --release --package kaze

# Nix build (same as CI); result in ./result
nix-build:
    nix build

# Everything CI checks, through Nix
ci:
    nix flake check --print-build-logs
