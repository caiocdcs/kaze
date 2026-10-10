# syntax=docker/dockerfile:1

# --- Web UI -------------------------------------------------------------
FROM node:22-alpine AS web
WORKDIR /web
# The pnpm version the lockfile was written with.
RUN npm install --global pnpm@12.9.0
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# --- Static binary (musl) -----------------------------------------------
FROM rust:1-alpine AS build
# A C toolchain for bundled SQLite and aws-lc (rustls).
RUN apk add --no-cache build-base cmake perl
WORKDIR /src
COPY . .
COPY --from=web /web/build web/build
# Queries are checked against the committed .sqlx/ metadata.
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --package kaze \
    && cp target/release/kaze /kaze

# --- Runtime --------------------------------------------------------------
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /kaze /usr/local/bin/kaze
ENV KAZE_LISTEN=0.0.0.0:6531 \
    KAZE_DATA_DIR=/data \
    KAZE_LIBRARY_DIR=/library
VOLUME ["/data", "/library"]
EXPOSE 6531
ENTRYPOINT ["kaze"]
CMD ["serve"]
