# Contributing

Bug reports, fixes and new sources are welcome.

- **A source broke?** Sites change their markup often. Open an issue with the source and the series URL. `just fixtures <source>` fetches fresh pages to diff against the saved ones.
- **A new source?** See the end of [Development](README.md#development). Each source is one module behind a cargo feature, tested against saved fixtures.
- **Something bigger?** Open an issue first, so we can agree on the approach.

## Setup

All tooling comes from the Nix dev shell: `nix develop`, or `direnv allow`. `just` lists the recipes.

Before opening a pull request, run:

```sh
just check   # fmt, clippy, sqlx metadata, API types, svelte-check
just test
```

## Conventions

- Clippy pedantic with warnings denied. No `unwrap`/`expect` outside tests, no `unsafe`.
- Background work goes through the job queue, not `tokio::spawn`.
- Source tests use saved fixtures. Tests that hit real sites are `#[ignore]` (`just test-live`).
- After changing a SQL query run `just sqlx-prepare`, and after changing an API type run `just types`. Commit the result.
- Commits are small and named `type(scope): summary`, for example `fix(sources): weebcentral chapter dates`.

[`AGENTS.md`](AGENTS.md) has the same rules in short.

By contributing you agree that your work is licensed under [GPL-3.0-or-later](LICENSE).
