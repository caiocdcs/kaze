# kaze 風

A light, self-hosted manga downloader. Follow series on WeebCentral, MangaDex, Asura Scans or Mangaworld. kaze downloads new chapters as `.cbz` files with `ComicInfo.xml`, tells Komga or Kavita to scan, and notifies you.

It's one Rust binary with the web UI built in and SQLite for storage. It needs no other services.

## Features

- **Follow series, *arr style.** For each series, pick all chapters, new ones only, or none. You can also start from a given chapter number. The **Wanted** page lists monitored chapters that aren't on disk yet.
- **Live activity.** The queue shows progress per page, with cancel and retry. A history records downloads, failures and new chapters.
- **Gentle on sites.**
  - Each site has its own rate limit and a cap on parallel jobs.
  - Failed downloads retry with backoff.
  - kaze reads each site's "latest updates" page every few minutes instead of polling every series.
- **Clean files.** Downloads go to `library/<Series>/<Series> - Vol.1 Ch.12.cbz`, with `ComicInfo.xml` and `cover.jpg`. The file name template can be changed.
- **Integrations.**
  - Komga and Kavita library scans, debounced so a backfill triggers one scan.
  - A templated webhook with presets for ntfy, Discord and Gotify.
  - Apprise, which reaches 100+ services.
- **Import.** Point kaze at an existing library, for example from Tranga. It matches each folder to a series and marks the chapters already on disk.
- **Cloudflare.** An optional [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) gets past challenges. kaze then reuses the cookie it earns.
- **Easy to run.** Docker image, Nix flake and NixOS module, or a plain binary.

### Sources

| Source | Language | Notes |
|---|---|---|
| [WeebCentral](https://weebcentral.com) | English | |
| [MangaDex](https://mangadex.org) | English | Official API. Publisher-only chapters are skipped. |
| [Asura Scans](https://asurascans.com) | English | Premium and early-access chapters are skipped until they're free. |
| [Mangaworld](https://www.mangaworld.mx) | Italian | |

Each source is a cargo feature, on by default. Build with `--no-default-features --features weebcentral,...` to pick your own set.

## Running

### Docker

```sh
cp compose.example.yml compose.yml   # remove the services you don't need
docker compose up -d
```

Open <http://localhost:6531>. Data goes to `./data` and chapters to `./library`.

### Nix

```sh
nix run github:caiocdcs/kaze -- serve
```

On NixOS:

```nix
{
  inputs.kaze.url = "github:caiocdcs/kaze";

  # in your configuration:
  imports = [ inputs.kaze.nixosModules.default ];
  services.kaze = {
    enable = true;
    libraryDir = "/srv/manga";
    openFirewall = true;
    settings.settings.flaresolverr_url = "http://127.0.0.1:8191";
  };
}
```

### Binary

```sh
kaze serve --config kaze.toml
```

The CLI also works without the server. Run `kaze --help` for every command.

```sh
kaze search frieren
kaze info https://weebcentral.com/series/…
kaze download https://weebcentral.com/series/… --chapters 1-10 --out ./manga
```

## Migrating from Tranga

kaze reads the files Tranga downloaded. It doesn't read Tranga's database, so you match each series once.

1. Stop Tranga, so the two don't download the same chapters.
2. Point kaze's `library_dir` at Tranga's download folder. In Docker, mount the same folder at `/library`. Komga and Kavita keep working because the files don't move.
3. Open **Import** in kaze. It lists every series folder kaze doesn't track yet, with the chapter numbers it found. They come from `ComicInfo.xml` when there is one, otherwise from the file name.
4. For each folder, click **Find match** and check the suggested series. Then choose what happens to missing chapters:
   - download them;
   - only new ones from now on;
   - none.

   Then click **Import**.

Chapters already on disk are marked as downloaded and never fetched again. New chapters use kaze's naming template, which by default is the same as Tranga's (`Series - Vol.1 Ch.12 - Title.cbz`), so folders stay consistent.

## Configuration

Everything is optional. See [`kaze.example.toml`](kaze.example.toml).

- **Boot settings:** `listen`, `data_dir` and `library_dir` are read at startup.
- **Everything else** can be changed in the UI under **Settings** while kaze runs:
  - the naming template
  - concurrency
  - check intervals
  - the API key
  - the FlareSolverr URL
- **Locked settings:** a value set under `[settings]` in `kaze.toml`, or as `KAZE_SETTINGS__*` in the environment, wins over the UI. The UI shows it as locked. This keeps declarative setups (NixOS, compose files) the source of truth.

Environment variables use the `KAZE_` prefix, with `__` for nesting. For example, `KAZE_LIBRARY_DIR=/manga` or `KAZE_SETTINGS__CONCURRENCY=2`.

Logs go to stderr. Set `KAZE_LOG` to change the level, for example `KAZE_LOG=kaze=debug`. `RUST_LOG` also works. Set `KAZE_LOG_FORMAT=json` for structured logs.
- **Startup:** kaze logs its setup (sources, FlareSolverr, integrations).
- **While running:** each download, failed job, integration send or scan, and change made through the API gets a line.

kaze is meant for a trusted network. To require an API key, set `api_key` in Settings or in `kaze.toml`. Put a reverse proxy in front if you expose it.

## Development

All tooling comes from the Nix dev shell. Run `nix develop`, or use direnv with `direnv allow`.

```sh
just            # list recipes
just dev        # API on :6531 plus the UI with hot reload on :5173
just check      # fmt, clippy, sqlx metadata, API types, svelte-check (as CI)
just test       # cargo nextest
just test-live  # tests that hit the real sites
just types      # regenerate web/src/lib/api/types from the Rust structs
nix build       # the package, UI included
nix flake check # everything CI runs
```

To release, push a version tag, for example `git tag v0.1.0 && git push github v0.1.0`. The release workflow builds the image for amd64 and arm64, pushes it to `ghcr.io/caiocdcs/kaze`, and creates a GitHub release.

| Path | What |
|---|---|
| `crates/kaze-core` | Domain types and the `Source` trait (no IO) |
| `crates/kaze-sources` | Site implementations, one cargo feature per site; the shared HTTP client and FlareSolverr |
| `crates/kaze` | The binary: config, SQLite, job queue, API, integrations, library import, embedded UI |
| `web/` | SvelteKit UI (Svelte 5, Tailwind 4), built statically and embedded with `rust-embed` |
| `nix/module.nix` | The NixOS module |

To add a source, write one module in `crates/kaze-sources/src/`. Add a cargo feature and a line in `Registry::new`. Test it against saved fixtures. See `mangadex/` for an API, or `weebcentral/` for HTML.

## Acknowledgements

kaze is inspired by [Tranga](https://github.com/C9Glax/tranga) by C9Glax and grew out of an attempt to port it to Rust. Several ideas come from it:
- the source connectors
- the chapter naming scheme
- the Komga/Kavita integration

kaze is licensed under GPL-3.0, like Tranga.

## License

[GPL-3.0-or-later](LICENSE)
