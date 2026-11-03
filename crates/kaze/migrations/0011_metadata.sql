-- The series' AniList entry. While set, title, summary, genres, staff and
-- cover come from AniList instead of the primary source.
ALTER TABLE series ADD COLUMN anilist_id INTEGER;
-- 'auto': kaze links and relinks by itself. 'manual': chosen (or unlinked)
-- by hand, so automatic linking leaves it alone.
ALTER TABLE series ADD COLUMN metadata_link TEXT NOT NULL DEFAULT 'auto'
    CHECK (metadata_link IN ('auto', 'manual'));
-- Last metadata refresh, linked or not; NULL means never tried.
ALTER TABLE series ADD COLUMN metadata_checked_at TEXT;
