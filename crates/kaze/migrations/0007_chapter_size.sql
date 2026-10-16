-- Size of each chapter's file, for disk usage. Set on download; the
-- "Measure library" task fills it for older files and outside changes.
ALTER TABLE chapters ADD COLUMN size_bytes INTEGER;
