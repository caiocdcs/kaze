# Security

## Reporting a vulnerability

Please don't open a public issue. Use GitHub's [private vulnerability reporting](https://github.com/caiocdcs/kaze/security/advisories/new) instead, with the version, what you found and how to reproduce it. I'll reply as soon as I can.

Fixes go into the latest release only.

## Running kaze safely

- kaze has one account, and until it exists the first browser to open kaze creates it. Open kaze yourself right after you install it.
- Browsers sign in with the password or an OpenID Connect provider (see the README) and get a session cookie (`HttpOnly`, `SameSite=Lax`). Only a hash of each session is stored. Failed sign-ins are logged with the peer address and any `X-Forwarded-For`, as separate fields: trust the second only when your proxy sets it.
- Scripts and other apps use the API key kaze generates on its first start. `api_key = ""` turns it off.
- To reach kaze from the internet, put a reverse proxy with TLS in front of it.
- Saved secrets (tokens, passwords, API keys, webhook paths, Apprise URLs) stay on the server and the UI only shows them masked. They are stored unencrypted in the SQLite database under `data_dir`, so keep that folder and its backups private.
