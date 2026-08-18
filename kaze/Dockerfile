# Build stage - compiles WITHOUT chromium feature to avoid OOM
FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && \
    echo 'fn main() {}' > src/main.rs && \
    echo '' > src/lib.rs

# Build WITHOUT chromium feature to avoid memory issues
RUN cargo build --release || true
RUN rm -f target/release/deps/kaze* target/release/kaze

# Build the actual binary
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release && \
    cp target/release/kaze /kaze

# Runtime stage
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r kaze && useradd -r -g kaze -d /home/kaze -s /sbin/nologin kaze

COPY --from=builder /kaze /usr/local/bin/kaze

RUN mkdir -p /usr/share/kaze /Manga /home/kaze && \
    chown -R kaze:kaze /usr/share/kaze /Manga /home/kaze

ENV PORT=6531
ENV DATABASE_URL=sqlite:///usr/share/kaze/kaze.db
ENV DOWNLOAD_LOCATION=/Manga
ENV HOME=/home/kaze

EXPOSE 6531

USER kaze

CMD ["kaze"]
