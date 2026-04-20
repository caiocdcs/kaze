FROM rust:1.87-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations

ARG FEATURES=chromium
RUN cargo build --release --features "${FEATURES}" && \
    cp target/release/tranga-rs /tranga-rs

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libnss3 \
    libatk1.0-0 \
    libatk-bridge2.0-0 \
    libcups2 \
    libdrm2 \
    libxkbcommon0 \
    libxcomposite1 \
    libxdamage1 \
    libxrandr2 \
    libgbm1 \
    libpango-1.0-0 \
    libcairo2 \
    libasound2 \
    libxshmfence1 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r tranga && useradd -r -g tranga -d /usr/share/tranga-api -s /sbin/nologin tranga

COPY --from=builder /tranga-rs /usr/local/bin/tranga-rs

RUN mkdir -p /usr/share/tranga-api /Manga && \
    chown -R tranga:tranga /usr/share/tranga-api /Manga

ENV PORT=6531
ENV DATABASE_URL=sqlite:///usr/share/tranga-api/tranga.db
ENV DOWNLOAD_LOCATION=/Manga

EXPOSE 6531

USER tranga

CMD ["tranga-rs"]
