-- The one local account. kaze has a single user; every way of signing in
-- ends in the same kind of session.
CREATE TABLE account (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    username      TEXT    NOT NULL,
    password_hash TEXT    NOT NULL
);

-- Signed-in browsers. Only a hash of the cookie's token is kept, so the
-- database or a backup of it signs no one in.
CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
