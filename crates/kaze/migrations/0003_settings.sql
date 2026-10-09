-- Settings changed from the UI. Keys set in kaze.toml / KAZE_* win over these.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL -- JSON
);
