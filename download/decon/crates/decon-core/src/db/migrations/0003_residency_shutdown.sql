-- 0003_residency_shutdown (up): резидентный режим и честный shutdown.
-- SPEC §3: sessions.end_status — чем закончилась сессия (данные честности);
-- probes.last_reminder_at — метка последнего повтора-«будильника».

ALTER TABLE sessions ADD COLUMN end_status TEXT NULL
    CHECK (end_status IS NULL OR end_status IN ('finished','interrupted','aborted'));

ALTER TABLE probes ADD COLUMN last_reminder_at TEXT NULL;

-- Уже завершённые сессии честно считаются «finished» (статус появился
-- только сейчас; исторические строки не трогаем — NULL остаётся валидным).
