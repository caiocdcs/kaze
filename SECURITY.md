# Security

## Reporting a vulnerability

Please don't open a public issue. Use GitHub's [private vulnerability reporting](https://github.com/caiocdcs/kaze/security/advisories/new) instead, with the version, what you found and how to reproduce it. I'll reply as soon as I can.

Fixes go into the latest release only.

## Running kaze safely

- kaze is meant for a trusted network. Out of the box the API has no authentication: set `api_key` in Settings or in `kaze.toml` to require one.
- To reach kaze from the internet, put a reverse proxy with TLS in front of it.
- Saved secrets (tokens, passwords, API keys, webhook paths, Apprise URLs) stay on the server and the UI only shows them masked. They are stored unencrypted in the SQLite database under `data_dir`, so keep that folder and its backups private.
