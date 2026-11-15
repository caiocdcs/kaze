# Security

## Reporting a vulnerability

Please don't open a public issue. Use GitHub's [private vulnerability reporting](https://github.com/caiocdcs/kaze/security/advisories/new) instead, with the version, what you found and how to reproduce it. I'll reply as soon as I can.

Fixes go into the latest release only.

## Running kaze safely

- On its first start kaze generates an API key and requires it. The first browser to open kaze gets the key, so open kaze yourself right after you install it. `api_key = ""` turns the key off, which is only safe on a trusted network.
- For single sign-on, use forward auth at the reverse proxy (see the README). kaze has no login of its own.
- To reach kaze from the internet, put a reverse proxy with TLS in front of it.
- Saved secrets (tokens, passwords, API keys, webhook paths, Apprise URLs) stay on the server and the UI only shows them masked. They are stored unencrypted in the SQLite database under `data_dir`, so keep that folder and its backups private.
