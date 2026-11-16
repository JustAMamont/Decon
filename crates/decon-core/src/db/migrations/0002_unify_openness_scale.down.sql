-- 0002 down: вернуть раздельные pre.openness / probe.openness.
-- Честная оговорка: UUID версии probe.openness v1 удалён в up — здесь
-- создаётся новая строка версии с новым UUID и тем же текстом; значения
-- и моменты ответов сохранены, тренд читаем.

-- 1. Восстановить определения (если soft-deleted)
UPDATE scales SET active = 1, deleted_at = NULL,
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
WHERE id IN ('pre.openness', 'probe.openness') AND deleted_at IS NOT NULL;

-- 2. Новые определения, если строк не было вовсе
INSERT INTO scales (id, title, anchor_low, anchor_high, kind, active, updated_at)
SELECT 'pre.openness', title, anchor_low, anchor_high, 'slider', 1,
       strftime('%Y-%m-%dT%H:%M:%fZ','now')
FROM scales WHERE id = 'scale.openness'
ON CONFLICT(id) DO NOTHING;

INSERT INTO scales (id, title, anchor_low, anchor_high, kind, active, updated_at)
SELECT 'probe.openness', title, anchor_low, anchor_high, 'slider', 1,
       strftime('%Y-%m-%dT%H:%M:%fZ','now')
FROM scales WHERE id = 'scale.openness'
ON CONFLICT(id) DO NOTHING;

-- 3. Версии scale.openness → pre.openness (кроме v1, её克隆 ждут зонды)
UPDATE scale_versions SET scale_id = 'pre.openness'
WHERE scale_id = 'scale.openness';

-- 4. Новая v1 probe.openness (UUID v7 — сортируемость); только если
--    определение шкалы реально существует (пустая база — no-op)
INSERT INTO scale_versions (id, scale_id, version, question, anchors_json, created_at)
SELECT
    lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-7' ||
          substr(hex(randomblob(2)), 2) || '-' || substr('89ab', abs(random()) % 4 + 1, 1) ||
          substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6))),
    'probe.openness', 1,
    'Каково ваше внимание прямо сейчас?',
    '{"low":"Полностью сужено на одном объекте","high":"Ровно открытое поле"}',
    strftime('%Y-%m-%dT%H:%M:%fZ','now')
WHERE EXISTS (SELECT 1 FROM scales WHERE id = 'probe.openness')
ON CONFLICT(scale_id, version) DO NOTHING;

-- 5. Ответы зондов — на воссозданную probe.openness v1
UPDATE responses SET scale_version_id = (
    SELECT id FROM scale_versions WHERE scale_id = 'probe.openness' AND version = 1
)
WHERE probe_id IS NOT NULL
  AND scale_version_id IN (SELECT id FROM scale_versions WHERE scale_id = 'pre.openness');

-- 6. Удалить определение scale.openness и его версии (FK свободен после
--    шага 5; down-миграция работает только в разработке/тестах)
DELETE FROM scale_versions WHERE scale_id = 'scale.openness';
DELETE FROM scales WHERE id = 'scale.openness';
