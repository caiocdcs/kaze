#!/usr/bin/env bash
set -e

# Build kaze for aarch64-unknown-linux-gnu (matches Docker Desktop on Apple Silicon).
# Runs on the Mac host with full RAM — no Docker memory limit.
#
# Requirements (run once):
#   cargo install cargo-zigbuild
#   rustup target add aarch64-unknown-linux-gnu

TARGET=aarch64-unknown-linux-gnu

rustup target add "$TARGET"
cargo zigbuild --release --features chromium --target "$TARGET"

mkdir -p bin
cp "target/$TARGET/release/kaze" bin/kaze

echo ""
echo "Binary staged to bin/kaze"
echo "Run: docker compose build kaze && docker compose up -d"
