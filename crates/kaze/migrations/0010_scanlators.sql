-- Scanlation groups to prefer, and never to download, per series (JSON
-- arrays of names, matched case-insensitively). They only choose between
-- copies of a number not on disk yet: files are never replaced.
ALTER TABLE series ADD COLUMN preferred_scanlators TEXT NOT NULL DEFAULT '[]';
ALTER TABLE series ADD COLUMN blocked_scanlators   TEXT NOT NULL DEFAULT '[]';

-- 0009's view with the groups ranked in; a view can't be altered. The
-- inner query works out each copy's flags, the outer one ranks them.
DROP VIEW chapter_choice;
CREATE VIEW chapter_choice AS
SELECT x.*,
       ROW_NUMBER() OVER (
           PARTITION BY series_id, number
           ORDER BY
               -- A copy on disk always wins: files are never replaced.
               file_path IS NULL,
               blocked,
               broken,
               failed,
               -- Below health, so a preferred group on a broken source
               -- still hands over.
               NOT preferred,
               -- Files of a removed source stay picked (above); other copies
               -- of it come last.
               priority IS NULL,
               priority,
               id
       ) AS pick
FROM (
    SELECT c.id, c.series_id, c.source, c.source_key, c.number, c.number_whole, c.number_frac,
           c.volume, c.title, c.url, c.language, c.scanlator, c.published_at, c.monitored,
           c.file_path, c.size_bytes, c.downloaded_at, c.discovered_at,
           ss.priority,
           EXISTS (SELECT 1 FROM json_each(s.blocked_scanlators)
                   WHERE lower(value) = lower(c.scanlator)) AS blocked,
           EXISTS (SELECT 1 FROM json_each(s.preferred_scanlators)
                   WHERE lower(value) = lower(c.scanlator)) AS preferred,
           -- 3 = db::sources::FAILING_AFTER.
           COALESCE(h.failures_in_row, 0) >= 3 AS broken,
           -- The copy's last download failed for good.
           COALESCE((SELECT j.state FROM jobs j
                     WHERE j.dedupe_key = 'download:' || c.id
                     ORDER BY j.id DESC LIMIT 1), '') = 'failed' AS failed
    FROM chapters c
    JOIN series s ON s.id = c.series_id
    LEFT JOIN series_sources ss ON ss.series_id = c.series_id AND ss.source = c.source
    LEFT JOIN source_health h ON h.source = c.source
) x;
