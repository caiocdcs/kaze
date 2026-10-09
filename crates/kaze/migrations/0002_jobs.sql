-- Background work. The mpsc channel only carries job ids; this table is
-- the source of truth, so queued work survives restarts.
CREATE TABLE jobs (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL,
    payload      TEXT    NOT NULL, -- JSON of jobs::Job
    -- At most one queued/running job per key, e.g. 'download:42'.
    dedupe_key   TEXT    NOT NULL,
    state        TEXT    NOT NULL DEFAULT 'queued'
                 CHECK (state IN ('queued', 'running', 'done', 'failed', 'cancelled')),
    attempts     INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    run_after    TEXT    NOT NULL,
    last_error   TEXT,
    created_at   TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL
);

CREATE UNIQUE INDEX jobs_active_key ON jobs (dedupe_key) WHERE state IN ('queued', 'running');
CREATE INDEX jobs_by_state ON jobs (state, run_after);

-- History shown in the UI ("Activity → History").
CREATE TABLE events (
    id         INTEGER PRIMARY KEY,
    at         TEXT    NOT NULL,
    kind       TEXT    NOT NULL,
    series_id  INTEGER REFERENCES series (id) ON DELETE SET NULL,
    chapter_id INTEGER REFERENCES chapters (id) ON DELETE SET NULL,
    message    TEXT    NOT NULL
);

CREATE INDEX events_by_time ON events (at DESC);
