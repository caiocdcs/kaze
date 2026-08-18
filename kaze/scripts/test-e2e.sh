#!/usr/bin/env bash
set -euo pipefail

API_URL="http://localhost:6531/v2"
DOWNLOAD_DIR="${1:-./test-downloads}"
CHAPTER_LIMIT="${2:-2}"

echo "=== Kaze End-to-End Test ==="
echo "API: $API_URL"
echo "Downloads: $DOWNLOAD_DIR"
echo "Chapters per manga: $CHAPTER_LIMIT"
echo

mkdir -p "$DOWNLOAD_DIR"

# 1. Create file library
echo ">>> [1/8] Creating file library..."
LIBRARY_RESPONSE=$(curl -s -X POST "$API_URL/file-libraries" \
  -H "Content-Type: application/json" \
  -d "{\"basePath\": \"$DOWNLOAD_DIR\", \"libraryName\": \"Test Library\"}")
LIBRARY_ID=$(echo "$LIBRARY_RESPONSE" | jq -r '.id')
echo "Library ID: $LIBRARY_ID"
echo

# 2. Search MangaDex for Jujutsu Kaisen
echo ">>> [2/8] Searching MangaDex for Jujutsu Kaisen..."
MANGADEX_RESULTS=$(curl -s "$API_URL/search/MangaDex/Jujutsu%20Kaisen")
MANGADEX_MANGA=$(echo "$MANGADEX_RESULTS" | jq '.[0]')
MANGADEX_MANGA_ID=$(echo "$MANGADEX_MANGA" | jq -r '.id')
MANGADEX_CONNECTOR_ID=$(echo "$MANGADEX_MANGA" | jq -r '.connectors[0].id')
echo "Manga: $(echo "$MANGADEX_MANGA" | jq -r '.name')"
echo "Manga ID: $MANGADEX_MANGA_ID"
echo "Connector ID: $MANGADEX_CONNECTOR_ID"
echo

# 3. Add to library
echo ">>> [3/8] Adding Jujutsu Kaisen to library..."
curl -s -X POST "$API_URL/manga/$MANGADEX_MANGA_ID/add-to-library" \
  -H "Content-Type: application/json" \
  -d "{\"libraryId\": \"$LIBRARY_ID\"}" > /dev/null
echo "Done"
echo

# 4. Enable download for MangaDex connector
echo ">>> [4/8] Enabling download for MangaDex connector..."
curl -s -X PATCH "$API_URL/manga/$MANGADEX_MANGA_ID/connectors/MangaDex/download" \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}' > /dev/null
echo "Done"
echo

# 5. Wait for chapters to be retrieved
echo ">>> [5/8] Waiting for chapters to be retrieved (15s)..."
sleep 15
CHAPTERS=$(curl -s "$API_URL/manga/$MANGADEX_MANGA_ID/chapters?perPage=10")
CHAPTER_COUNT=$(echo "$CHAPTERS" | jq '.data | length')
echo "Found $CHAPTER_COUNT chapters"
echo

# 6. Select first N chapters for download
echo ">>> [6/8] Selecting first $CHAPTER_LIMIT chapters for download..."
CHAPTER_MAPPING_IDS=$(echo "$CHAPTERS" | jq -r ".data[:$CHAPTER_LIMIT][].connectors[0].id")
for CID in $CHAPTER_MAPPING_IDS; do
  curl -s -X PATCH "$API_URL/chapters/$CID/download" \
    -H "Content-Type: application/json" \
    -d '{"enabled": true}' > /dev/null
  echo "  Enabled chapter mapping: $CID"
done
echo

# 7. Wait for downloads
echo ">>> [7/8] Waiting for downloads to complete (30s)..."
sleep 30
echo

# 8. Check downloaded files
echo ">>> [8/8] Checking downloaded files..."
DOWNLOADED=$(find "$DOWNLOAD_DIR" -name "*.cbz" -type f 2>/dev/null | head -10)
if [ -n "$DOWNLOADED" ]; then
  echo "Downloaded files:"
  echo "$DOWNLOADED"
else
  echo "No .cbz files found yet. Check worker status:"
  curl -s "$API_URL/workers" | jq '.[] | {id, state, workerType}'
fi
echo

# --- WeebCentral Test ---
echo
echo "=========================================="
echo ">>> [WEEB] Testing WeebCentral with One Piece..."
WEEB_RESULTS=$(curl -s "$API_URL/search/WeebCentral/One%20Piece")
WEEB_MANGA=$(echo "$WEEB_RESULTS" | jq '.[0]')
WEEB_MANGA_ID=$(echo "$WEEB_MANGA" | jq -r '.id')
echo "Found: $(echo "$WEEB_MANGA" | jq -r '.name')"
echo "Manga ID: $WEEB_MANGA_ID"
echo

echo ">>> [WEEB] Adding to library..."
curl -s -X POST "$API_URL/manga/$WEEB_MANGA_ID/add-to-library" \
  -H "Content-Type: application/json" \
  -d "{\"libraryId\": \"$LIBRARY_ID\"}" > /dev/null
echo "Done"
echo

echo ">>> [WEEB] Enabling download for WeebCentral connector..."
curl -s -X PATCH "$API_URL/manga/$WEEB_MANGA_ID/connectors/WeebCentral/download" \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}' > /dev/null
echo "Done (requires chromium for chapters)"
echo

echo "=========================================="
echo "=== Test Complete ==="
echo "MangaDex (Jujutsu Kaisen) chapters: $DOWNLOAD_DIR"
echo "WeebCentral (One Piece) requires chromium - check logs"
echo
echo "To monitor workers: curl -s $API_URL/workers | jq"
echo "To check stats: curl -s $API_URL/stats | jq"
