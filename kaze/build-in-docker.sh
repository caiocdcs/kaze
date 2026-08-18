#!/bin/bash
# Build kaze inside a Docker container and extract the binary

docker run --rm -v "$(pwd)":/app -w /app rust:1.88-bookworm \
    bash -c "apt-get update && apt-get install -y pkg-config libssl-dev && \
             cargo build --release --features chromium && \
             cp target/release/kaze ./kaze-linux"
