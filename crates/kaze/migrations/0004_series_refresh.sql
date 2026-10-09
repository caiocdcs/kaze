-- Per-series full refresh interval in hours: NULL = the global setting, 0 = never.
ALTER TABLE series ADD COLUMN refresh_hours INTEGER CHECK (refresh_hours IS NULL OR refresh_hours >= 0);
