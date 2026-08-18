-- 001_initial.sql

CREATE TABLE IF NOT EXISTS file_libraries (
    key TEXT PRIMARY KEY NOT NULL,
    base_path TEXT NOT NULL,
    library_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mangas (
    key TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    cover_url TEXT NOT NULL DEFAULT '',
    release_status TEXT NOT NULL DEFAULT 'Continuing',
    library_id TEXT REFERENCES file_libraries(key) ON DELETE SET NULL,
    ignore_chapters_before REAL NOT NULL DEFAULT 0.0,
    directory_name TEXT NOT NULL,
    cover_file_name_in_cache TEXT,
    year INTEGER,
    original_language TEXT
);

CREATE TABLE IF NOT EXISTS authors (
    key TEXT PRIMARY KEY NOT NULL,
    author_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS manga_authors (
    manga_key TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    author_key TEXT NOT NULL REFERENCES authors(key) ON DELETE CASCADE,
    PRIMARY KEY (manga_key, author_key)
);

CREATE TABLE IF NOT EXISTS manga_tags (
    tag TEXT PRIMARY KEY NOT NULL
);

CREATE TABLE IF NOT EXISTS manga_tag_associations (
    manga_key TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    tag TEXT NOT NULL REFERENCES manga_tags(tag) ON DELETE CASCADE,
    PRIMARY KEY (manga_key, tag)
);

CREATE TABLE IF NOT EXISTS alt_titles (
    key TEXT PRIMARY KEY NOT NULL,
    manga_key TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    language TEXT NOT NULL,
    title TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS links (
    key TEXT PRIMARY KEY NOT NULL,
    manga_key TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    link_provider TEXT NOT NULL,
    link_url TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS manga_connector_to_manga (
    key TEXT PRIMARY KEY NOT NULL,
    obj_id TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    manga_connector_name TEXT NOT NULL,
    id_on_connector_site TEXT NOT NULL,
    website_url TEXT,
    use_for_download INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS chapters (
    key TEXT PRIMARY KEY NOT NULL,
    parent_manga_id TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    volume_number INTEGER,
    chapter_number TEXT NOT NULL,
    title TEXT,
    file_name TEXT,
    downloaded INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS manga_connector_to_chapter (
    key TEXT PRIMARY KEY NOT NULL,
    obj_id TEXT NOT NULL REFERENCES chapters(key) ON DELETE CASCADE,
    manga_connector_name TEXT NOT NULL,
    id_on_connector_site TEXT NOT NULL,
    website_url TEXT,
    use_for_download INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS metadata_entries (
    metadata_fetcher_name TEXT NOT NULL,
    identifier TEXT NOT NULL,
    manga_id TEXT NOT NULL REFERENCES mangas(key) ON DELETE CASCADE,
    PRIMARY KEY (metadata_fetcher_name, identifier)
);

CREATE TABLE IF NOT EXISTS library_connectors (
    key TEXT PRIMARY KEY NOT NULL,
    library_type TEXT NOT NULL,
    base_url TEXT NOT NULL,
    auth TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS manga_connectors (
    name TEXT PRIMARY KEY NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS notifications (
    key TEXT PRIMARY KEY NOT NULL,
    urgency TEXT NOT NULL DEFAULT 'Normal',
    title TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    is_sent INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS notification_connectors (
    name TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    headers TEXT NOT NULL DEFAULT '{}',
    http_method TEXT NOT NULL DEFAULT 'POST',
    body TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS actions (
    key TEXT PRIMARY KEY NOT NULL,
    action TEXT NOT NULL,
    performed_at TEXT NOT NULL,
    manga_id TEXT,
    chapter_id TEXT,
    filename TEXT,
    from_path TEXT,
    to_path TEXT,
    metadata_fetcher TEXT,
    file_library_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_mangas_name ON mangas(name);
CREATE INDEX IF NOT EXISTS idx_mangas_library_id ON mangas(library_id);
CREATE INDEX IF NOT EXISTS idx_chapters_parent_manga ON chapters(parent_manga_id);
CREATE INDEX IF NOT EXISTS idx_chapters_downloaded ON chapters(downloaded);
CREATE INDEX IF NOT EXISTS idx_manga_connector_to_manga_obj ON manga_connector_to_manga(obj_id);
CREATE INDEX IF NOT EXISTS idx_manga_connector_to_chapter_obj ON manga_connector_to_chapter(obj_id);
CREATE INDEX IF NOT EXISTS idx_manga_connector_to_manga_connector ON manga_connector_to_manga(manga_connector_name);
CREATE INDEX IF NOT EXISTS idx_manga_connector_to_chapter_connector ON manga_connector_to_chapter(manga_connector_name);
CREATE INDEX IF NOT EXISTS idx_manga_connector_to_chapter_use_download ON manga_connector_to_chapter(use_for_download);
CREATE INDEX IF NOT EXISTS idx_alt_titles_manga ON alt_titles(manga_key);
CREATE INDEX IF NOT EXISTS idx_links_manga ON links(manga_key);
CREATE INDEX IF NOT EXISTS idx_metadata_entries_manga ON metadata_entries(manga_id);
CREATE INDEX IF NOT EXISTS idx_notifications_is_sent ON notifications(is_sent);
CREATE INDEX IF NOT EXISTS idx_actions_action ON actions(action);
CREATE INDEX IF NOT EXISTS idx_actions_performed_at ON actions(performed_at);
CREATE INDEX IF NOT EXISTS idx_actions_manga_id ON actions(manga_id);
