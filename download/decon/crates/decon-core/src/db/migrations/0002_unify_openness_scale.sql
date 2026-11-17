-- 0002: unify openness scale (ROADMAP S2, критерий 4).
-- pre.openness и probe.openness — одна и та же шкала в разных контекстах
-- (SCALES.md §5): один стабильный scale_id "scale.openness", контекст
-- ответа выводится из responses.session_id / responses.probe_id.
--
-- PK scales.id переименовать UPDATE нельзя (scale_versions.scale_id — FK
-- без CASCADE, SPEC §6 этого и требует: переименование = новая строка +
-- backfill). Поэтому: создаём новое определение, переносим версии и
-- ответы, старые определения деактивируем (soft-delete, история
-- идентификаторов сохраняется).
-- Идемпотентность: каждый шаг условный, повторный прогон — no-op.

-- 1. Новое определение scale.openness (если ещё нет)
INSERT INTO scales (id, title, anchor_low, anchor_high, kind, active, updated_at)
SELECT 'scale.openness', title, anchor_low, anchor_high, 'slider', 1,
       strftime('%Y-%m-%dT%H:%M:%fZ','now')
FROM scales WHERE id = 'pre.openness' AND deleted_at IS NULL
ON CONFLICT(id) DO NOTHING;

-- 2. Версии pre.openness переводим на scale.openness (номера версий
--    сохраняются — формулировки те же). FK свободен: ответы пока на старых.
UPDATE scale_versions SET scale_id = 'scale.openness'
WHERE scale_id = 'pre.openness'
  AND NOT EXISTS (SELECT 1 FROM scale_versions v2
                  WHERE v2.scale_id = 'scale.openness'
                    AND v2.version = scale_versions.version);

-- 3. Ответы probe.openness → версия v1 scale.openness (текст идентичен).
UPDATE responses SET scale_version_id = (
    SELECT sv.id FROM scale_versions sv
    WHERE sv.scale_id = 'scale.openness' AND sv.version = 1
)
WHERE scale_version_id IN (SELECT id FROM scale_versions WHERE scale_id = 'probe.openness');

-- 4. Версии-дубли probe.openness: ответы уже переведены, FK свободен.
DELETE FROM scale_versions WHERE scale_id = 'probe.openness';

-- 5. Старые определения — soft-delete (история идентификаторов,
--    SPEC §6: сущности не удаляются жёстко).
UPDATE scales SET active = 0, deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
WHERE id IN ('pre.openness', 'probe.openness') AND deleted_at IS NULL;
