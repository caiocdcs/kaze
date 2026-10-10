# AGENTS.md

kaze: a self-hosted manga downloader. One Rust binary with an embedded SvelteKit UI and SQLite.

## Environment

- Tooling comes only from the Nix flake: `nix develop --command <cmd>` (or direnv).
- Prefer `just` recipes: `just check`, `just test`, `just audit`, `just dev`.
- Flakes only see tracked files: `git add` new files before `nix build` / `nix flake check`.

## Layout

- `crates/kaze-core`: domain types and the `Source` trait. No IO.
- `crates/kaze-sources`: one module per site, each behind a cargo feature.
- `crates/kaze`: the binary (config, db, jobs, api, integrations, embedded UI).
- `web/`: SvelteKit, Svelte 5 runes. Types in `web/src/lib/api/types` are generated (`just types`).

## Rules

- Clippy pedantic, warnings denied. Fix lints; a justified `#[allow]` stays local with a reason.
- No `unwrap`/`expect` outside tests, no `unsafe`.
- `thiserror` in libraries, `anyhow` only in the CLI.
- Background work goes through the job queue, not `tokio::spawn`.
- Log with `tracing`.
- Source tests use saved fixtures; live tests are `#[ignore]` (`just test-live`).
- Comments explain why, sparingly.

## Git

- Small commits, `type(scope): summary`.
- Never commit secrets, `data/`, databases or `result`.
