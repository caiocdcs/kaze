# AGENTS.md

Guidance for AI coding agents (and humans) working on kaze.

## Project

kaze is a light, self-hosted manga downloader written in Rust, inspired by Tranga. It's a single binary with an embedded SvelteKit UI and SQLite.

## Environment

- The toolchain comes **only** from the Nix flake. Run commands via `nix develop --command <cmd>`, or inside `nix develop` / direnv. There is no global `cargo`.
- Use `just` recipes rather than raw commands where one exists: `just check`, `just test`, `just run <args>`.
- Flakes only see files tracked by git. `git add` new files before running `nix build` or `nix flake check`.

## Workspace

- `crates/kaze-core`: domain types and the `Source` trait. No IO, no tokio, no reqwest.
- `crates/kaze-sources`: one module per site, each behind a cargo feature named after the site (e.g. `weebcentral`).
- `crates/kaze`: the binary. Contains config, db (sqlx + SQLite), http client, jobs, api (axum), integrations and the embedded web UI.
- `web/`: SvelteKit (Svelte 5 runes, TypeScript), built statically and embedded.

## Conventions

- Rust edition 2024. `clippy::pedantic` is on and CI denies warnings. Fix lints rather than allowing them; when an `#[allow]` is justified, keep it local and comment why.
- No `unsafe`. Use `thiserror` for library error types and `anyhow` only at the binary edges (CLI, main).
- Async runs on tokio. Background work goes through the job queue (mpsc dispatcher backed by the `jobs` table), not ad-hoc `tokio::spawn`.
- Use `tracing` for logs, never `println!` outside CLI output.
- Source tests use saved HTML/JSON fixtures. Tests that hit live sites are `#[ignore]` and run with `just test-live`.
- Keep comments sparse and explain *why*, not *what*.

## Git

- Small, focused commits with imperative subject lines.
- **No AI attribution** in commits or PRs: no `Co-Authored-By` lines for agents, no "generated with" footers.
- Never commit secrets, `data/`, databases, or `result` symlinks.
