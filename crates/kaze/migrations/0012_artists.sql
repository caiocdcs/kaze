-- People credited for the art (JSON array), apart from the writers in
-- `authors`. Sources that don't tell roles apart leave it empty and list
-- everyone in `authors`.
ALTER TABLE series ADD COLUMN artists TEXT NOT NULL DEFAULT '[]';
