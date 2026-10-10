-- Library servers to scan and notification targets (Settings → Connect).
CREATE TABLE integrations (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    config       TEXT    NOT NULL, -- JSON of integrations::Config, tagged by kind
    events       TEXT    NOT NULL DEFAULT '[]', -- JSON array of integrations::Trigger
    -- Outcome of the latest send, scan or test, for the UI.
    last_ok      INTEGER,
    last_status  TEXT,
    last_sent_at TEXT,
    created_at   TEXT    NOT NULL
);
