# kaze 風

A light, self-hosted manga downloader. Follow series on sites like WeebCentral or MangaDex. New chapters are downloaded as `.cbz` files with `ComicInfo.xml` metadata, and Komga, Kavita or your phone is notified when they land.

> **Status:** early rewrite, work in progress. Not usable yet.

## Goals

- **Light.** One Rust binary with the web UI embedded, using SQLite. No external services are required.
- **Pluggable sources.** Each site is one module implementing a `Source` trait, behind a cargo feature.
- **Hands-off, \*arr style.** Monitor series and see wanted chapters, plus a live activity queue and history.
- **Plays well with others.** Komga and Kavita library scans; ntfy, webhook (Discord, Gotify) and Apprise notifications; optional FlareSolverr.
- **Easy to self-host.** Nix flake and NixOS module, Docker image, or a plain binary.

## Development

All tooling comes from the Nix dev shell. Run `nix develop`, or use direnv with `direnv allow`.

```sh
just            # list recipes
just check      # fmt + clippy (as CI)
just test       # cargo nextest
just run serve  # run the server
nix build       # build the package
```

## Layout

| Path | What |
|---|---|
| `crates/kaze-core` | Domain types and the `Source` trait (no IO) |
| `crates/kaze-sources` | Site implementations, one cargo feature per site |
| `crates/kaze` | The binary: config, database, job queue, API, integrations, embedded UI |
| `web/` | SvelteKit UI (coming in a later milestone) |

## Acknowledgements

kaze is inspired by [Tranga](https://github.com/C9Glax/tranga) by C9Glax and grew out of an attempt to port it to Rust. Several ideas come from it: the source connectors, the chapter naming scheme and the Komga/Kavita integration. kaze is licensed under GPL-3.0, like Tranga.

## License

[GPL-3.0-or-later](LICENSE)
