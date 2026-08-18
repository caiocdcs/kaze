# Kaze

A manga download manager written in Rust.

## Features

- Download manga chapters from MangaDex, WeebCentral, AsuraComic, Mangaworld
- CBZ archive creation with ComicInfo metadata
- Library integration with Komga and Kavita
- Notifications via Gotify, Ntfy, Pushover, webhooks
- Chromium support for JavaScript-rendered sites

## Installation

### From Source

```bash
cargo build --release --features chromium
```

### With Nix

```bash
nix build .#default
```

## Usage

```bash
WORKING_DIR=./data DATABASE_URL=sqlite://./data/kaze.db ./kaze
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `6531` | API server port |
| `DATABASE_URL` | `sqlite://./data/kaze.db` | Database connection string |
| `DOWNLOAD_LOCATION` | `/Manga` | Directory for downloaded manga |
| `WORKING_DIR` | `./debug` | Working directory for cache, settings |
| `FLARESOLVERR_URL` | - | FlareSolverr endpoint for Cloudflare bypass |
| `CHROMIUM_WAIT_TIME` | `15` | Seconds to wait for JS rendering |

## API

API available at `http://localhost:6531/v2/` with Swagger UI at `/swagger-ui/`.

### Quick Start

```bash
# Create a file library
curl -X POST http://localhost:6531/v2/file-libraries \
  -H "Content-Type: application/json" \
  -d '{"basePath": "/Manga", "libraryName": "My Library"}'

# Search for manga
curl http://localhost:6531/v2/search/MangaDex/Jujutsu%20Kaisen

# Add to library and enable download
curl -X POST http://localhost:6531/v2/manga/{manga_id}/add-to-library \
  -H "Content-Type: application/json" \
  -d '{"libraryId": "library_id"}'

curl -X PATCH http://localhost:6531/v2/manga/{manga_id}/connectors/MangaDex/download \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}'
```

## Inspiration

This project was inspired by [Tranga](https://github.com/C9Glax/tranga), an automatic manga and metadata downloader written in C#.

## License

MIT
