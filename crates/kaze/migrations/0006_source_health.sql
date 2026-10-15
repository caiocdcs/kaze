-- How each source's jobs have been going, for Settings → Sources.
CREATE TABLE source_health (
    source          TEXT    PRIMARY KEY,
    last_success_at TEXT,
    last_error_at   TEXT,
    last_error      TEXT,
    failures_in_row INTEGER NOT NULL DEFAULT 0
);
