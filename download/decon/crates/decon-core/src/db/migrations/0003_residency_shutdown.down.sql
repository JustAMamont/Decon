-- 0003 down: убрать колонки резидентного режима.
-- SQLite поддерживает ALTER ... DROP COLUMN с 3.35 —bundled rusqlite новее.

ALTER TABLE sessions DROP COLUMN end_status;
ALTER TABLE probes DROP COLUMN last_reminder_at;
