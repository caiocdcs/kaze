-- The language a series is followed in (a BCP 47 tag like 'pt-BR'). It is
-- part of the key, so one series can be followed in several languages.
-- SQLite can't change a UNIQUE constraint in place: rebuild the table.
-- Nothing references series_sources, so dropping it is safe.

CREATE TABLE series_sources_new (
    series_id  INTEGER NOT NULL REFERENCES series (id) ON DELETE CASCADE,
    source     TEXT    NOT NULL,
    source_key TEXT    NOT NULL,
    language   TEXT    NOT NULL,
    url        TEXT    NOT NULL,
    priority   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (series_id, source),
    UNIQUE (source, source_key, language)
);

-- Existing series: the language of their chapters, else the source's only
-- language (Mangaworld is Italian, the rest English).
INSERT INTO series_sources_new (series_id, source, source_key, language, url, priority)
SELECT ss.series_id, ss.source, ss.source_key,
       COALESCE(
           (SELECT c.language FROM chapters c
            WHERE c.series_id = ss.series_id AND c.language IS NOT NULL
            LIMIT 1),
           CASE ss.source WHEN 'mangaworld' THEN 'it' ELSE 'en' END
       ),
       ss.url, ss.priority
FROM series_sources ss;

DROP TABLE series_sources;
ALTER TABLE series_sources_new RENAME TO series_sources;
