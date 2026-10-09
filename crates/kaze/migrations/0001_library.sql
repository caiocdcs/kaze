-- Followed series, their source mappings and their chapters.
-- Timestamps are RFC 3339 UTC text, which sorts correctly and decodes as chrono::DateTime<Utc>.

CREATE TABLE series (
    id              INTEGER PRIMARY KEY,
    title           TEXT    NOT NULL,
    description     TEXT,
    authors         TEXT    NOT NULL DEFAULT '[]', -- JSON array of strings
    tags            TEXT    NOT NULL DEFAULT '[]', -- JSON array of strings
    status          TEXT    NOT NULL DEFAULT 'unknown',
    year            INTEGER,
    cover_url       TEXT,
    -- Directory name under the library root.
    folder          TEXT    NOT NULL UNIQUE,
    monitor         TEXT    NOT NULL DEFAULT 'all' CHECK (monitor IN ('all', 'future', 'none')),
    -- Only chapters numbered at least this are wanted; NULL means from the start.
    monitor_from    TEXT,
    added_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_checked_at TEXT
);

-- Where a series is downloaded from. One row per series for now (priority 0);
-- extra rows will allow falling back to other sources later.
CREATE TABLE series_sources (
    series_id  INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    source     TEXT    NOT NULL,
    source_key TEXT    NOT NULL,
    url        TEXT    NOT NULL,
    priority   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (series_id, source),
    UNIQUE (source, source_key)
);

CREATE TABLE chapters (
    id            INTEGER PRIMARY KEY,
    series_id     INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    source        TEXT    NOT NULL,
    source_key    TEXT    NOT NULL,
    number        TEXT    NOT NULL, -- ChapterNumber, e.g. '12.5'
    -- Split out for numeric ordering: 2 < 2.15 < 2.5 < 10.
    number_whole  INTEGER NOT NULL GENERATED ALWAYS AS (CAST(number AS INTEGER)) VIRTUAL,
    number_frac   TEXT    NOT NULL GENERATED ALWAYS AS (
        CASE WHEN instr(number, '.') > 0 THEN substr(number, instr(number, '.') + 1) ELSE '' END
    ) VIRTUAL,
    volume        INTEGER,
    title         TEXT,
    url           TEXT    NOT NULL,
    language      TEXT,
    scanlator     TEXT,
    published_at  TEXT,
    monitored     INTEGER NOT NULL DEFAULT 1 CHECK (monitored IN (0, 1)),
    -- Set once downloaded; a monitored chapter without a file is "wanted".
    file_path     TEXT,
    downloaded_at TEXT,
    discovered_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (source, source_key)
);

CREATE INDEX chapters_by_series ON chapters (series_id, number_whole, number_frac);
CREATE INDEX chapters_wanted ON chapters (monitored, file_path) WHERE monitored = 1 AND file_path IS NULL;
