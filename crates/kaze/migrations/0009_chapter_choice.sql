-- A series can list one chapter number several times: from several sources,
-- or from several groups on one source. The view ranks the copies of each
-- number; `pick = 1` is the copy to show and to download.

-- When each source of a series was first read, so a source added later
-- knows its first check (for `Future` monitoring) apart from the series'.
ALTER TABLE series_sources ADD COLUMN checked_at TEXT;
UPDATE series_sources
SET checked_at = (SELECT last_checked_at FROM series WHERE series.id = series_sources.series_id);

-- The view looks up each copy's latest download job.
CREATE INDEX jobs_by_key ON jobs (dedupe_key);

CREATE VIEW chapter_choice AS
SELECT c.id, c.series_id, c.source, c.source_key, c.number, c.number_whole, c.number_frac,
       c.volume, c.title, c.url, c.language, c.scanlator, c.published_at, c.monitored,
       c.file_path, c.size_bytes, c.downloaded_at, c.discovered_at,
       ROW_NUMBER() OVER (
           PARTITION BY c.series_id, c.number
           ORDER BY
               -- A copy on disk always wins: files are never replaced.
               c.file_path IS NULL,
               -- 3 = db::sources::FAILING_AFTER: a broken source's copies go last.
               COALESCE(h.failures_in_row, 0) >= 3,
               -- So do copies whose last download failed for good.
               COALESCE((SELECT j.state FROM jobs j
                         WHERE j.dedupe_key = 'download:' || c.id
                         ORDER BY j.id DESC LIMIT 1), '') = 'failed',
               ss.priority,
               c.id
       ) AS pick
FROM chapters c
JOIN series_sources ss ON ss.series_id = c.series_id AND ss.source = c.source
LEFT JOIN source_health h ON h.source = c.source;
