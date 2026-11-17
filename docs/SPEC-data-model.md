# SPEC-data-model.md — Модель данных Decon

> Версия схемы: **1** (миграция `0001_init`). Нумерация миграций сквозная,
> необратимая в проде (down-тесты только для разработки). Все изменения схемы —
> через новую миграцию и обновление этого файла в том же PR.

## 1. Общие принципы

1. **UUID v7** для всех первичных ключей (`id TEXT PRIMARY KEY`, хранение —
   каноничная 36-символьная строка; генерация — крейт `uuid`).
2. **Sync-готовность с первого дня:** у каждой синхронизируемой таблицы
   `updated_at TEXT NOT NULL` (ISO-8601 UTC), `deleted_at TEXT NULL`
   (soft-delete). Ни одно удаление не стирает физически.
3. **Timezone:** всё UTC в ISO-8601 (`YYYY-MM-DDTHH:MM:SS.sssZ`); локальное
   время суток дня сессии хранится отдельно (`local_time_bucket`) для
   аналитики «время дня ↔ качество».
4. **Шифрование:** чувствительные текстовые поля помечены 🔒 — хранится
   fernet-токен (`TEXT`, base64: version ‖ iv ‖ ciphertext ‖ hmac), расшифровка
   только в памяти. Остальные поля — открытым текстом (нужны SQL-запросам).
5. **Справочники без жёстких FK на enum-значения:** перечисления (слой, тип
   события, режим тренажёра) хранятся как `TEXT` с CHECK-ограничением;
   расширенные наборы пользовательских значений — через справочные таблицы.

## 2. ER-обзор

```
settings ──┐
           │
scales ── scale_versions ──┐
                           ├── responses (pre/post/probe/layer)
sessions ──┬───────────────┤            ┌─ ffqm_results
           ├─ session_events            └─ (общий журнал)
           ├─ trainer_results
           └─ probes ──────────── probe_responses (догоняющие: delay_minutes)
```

## 3. DDL (миграция 0001)

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous  = NORMAL;

-- ============================================================ settings
CREATE TABLE settings (
    key         TEXT PRIMARY KEY,            -- 'probe.window_start' и т.п.
    value       TEXT NOT NULL,               -- JSON-значение
    updated_at  TEXT NOT NULL
);
-- Справедливо правило №6: настроек в файлах нет, все — здесь.

-- ============================================================ scales
CREATE TABLE scales (
    id          TEXT PRIMARY KEY,            -- 'post.uniformity'
    title       TEXT NOT NULL,
    anchor_low  TEXT NOT NULL,
    anchor_high TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('slider','flag','factor')),
    active      INTEGER NOT NULL DEFAULT 1,  -- деактивация скрывает из UI
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);

CREATE TABLE scale_versions (
    id          TEXT PRIMARY KEY,
    scale_id    TEXT NOT NULL REFERENCES scales(id),
    version     INTEGER NOT NULL,
    question    TEXT NOT NULL,
    anchors_json TEXT NOT NULL,               -- {"low": "...", "high": "..."}
    created_at  TEXT NOT NULL,
    UNIQUE (scale_id, version)
);
-- Изменение формулировки → новая версия; тренды строятся внутри version_id.

-- ============================================================ sessions
CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('practice','trainer')),
    layer       TEXT NULL,                    -- 'visual','audio','vital',
                                              -- 'semantic','polymodal'
    technique   TEXT NULL,                    -- свободное название техники
    started_at  TEXT NOT NULL,
    ended_at    TEXT NULL,
    duration_s  INTEGER NULL,
    -- чем закончилась сессия (S2.5, миграция 0003): 'finished' — штатный
    -- стоп; 'interrupted' — сигнал ОС/выход/краш (recovery-проходом при
    -- старте); 'aborted' — пользователь прервал вручную. NULL — идёт
    -- сейчас или завершена до S2.5.
    end_status  TEXT NULL CHECK (end_status IS NULL OR end_status IN
                      ('finished','interrupted','aborted')),
    -- локальное время дня для аналитики (не зависит от TZ-переходов)
    local_time_bucket TEXT NULL CHECK (local_time_bucket IN
                      ('morning','day','evening','night')),
    -- контекст условий (n-of-1 ковариаты)
    sleep_hours     REAL NULL,
    caffeine       INTEGER NULL,              -- чашек/доз за сутки до сессии
    posture        TEXT NULL,
    note_enc       TEXT NULL,                 -- 🔒 fernet-токен заметки
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);
CREATE INDEX idx_sessions_started ON sessions(started_at);
CREATE INDEX idx_sessions_layer   ON sessions(layer) WHERE deleted_at IS NULL;

-- ============================================================ events
CREATE TABLE session_events (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id),
    t_offset_ms INTEGER NOT NULL,             -- таймкод от старта сессии
    kind        TEXT NOT NULL CHECK (kind IN
                  ('collapse_focus',   -- срыв: рецентрация на объекте
                   'drift_semantic',   -- уход в семантику (мысли, смыслы)
                   'drift_body',       -- уход в локальные телесные ощущения
                   'clarity_flash',    -- вспышка ясности/равномерности
                   'pause','resume','signal')),  -- служебные
    note_enc     TEXT NULL                    -- 🔒 опциональная расшифровка
);
CREATE INDEX idx_events_session ON session_events(session_id);

-- ============================================================ responses
CREATE TABLE responses (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NULL REFERENCES sessions(id),   -- NULL для зондов дня
    probe_id    TEXT NULL REFERENCES probes(id),     -- заполнен для зондов
    scale_version_id TEXT NOT NULL REFERENCES scale_versions(id),
    value       REAL NOT NULL,                -- 0..10 (slider) / 0|1 (flag)
    answered_at TEXT NOT NULL,
    delay_minutes INTEGER NULL,               -- >0 для догоняющих зондов
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL,
    CHECK (session_id IS NOT NULL OR probe_id IS NOT NULL)
);
CREATE INDEX idx_responses_scale ON responses(scale_version_id, answered_at);

-- ============================================================ probes
CREATE TABLE probes (
    id          TEXT PRIMARY KEY,
    scheduled_at TEXT NOT NULL,               -- плановое время среза
    fired_at    TEXT NULL,                    -- когда показано уведомление
    answered_at TEXT NULL,
    delay_minutes INTEGER NULL,
    -- метка последнего повтора-«будильника» (S2.5, миграция 0003):
    -- fired-зонд без ответа напоминает каждые reminder_interval_min
    last_reminder_at TEXT NULL,
    window_state TEXT NOT NULL CHECK (window_state IN
                    ('scheduled','fired','answered','missed','skipped')),
    updated_at  TEXT NOT NULL
);
CREATE INDEX idx_probes_scheduled ON probes(scheduled_at);
-- Пропущенный зонд: window_state='fired' без answered_at → доступен в
-- догоняющем списке до конца дня; в полночь → 'missed' (данные честности).

-- ============================================================ trainer_results
CREATE TABLE trainer_results (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id),  -- kind='trainer'
    trainer     TEXT NOT NULL CHECK (trainer IN
                    ('schulte','mot','ufov','audio_field','layer_switch')),
    config_json TEXT NOT NULL,                -- полная конфигурация прогона
    metrics_json TEXT NOT NULL,               -- метрики (см. §4)
    seed        TEXT NOT NULL,                -- воспроизводимость прогона
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);
CREATE INDEX idx_trainer_results ON trainer_results(trainer, session_id);

-- ============================================================ ffqm
CREATE TABLE ffqm_results (
    id          TEXT PRIMARY KEY,
    taken_at    TEXT NOT NULL,
    observing       REAL NOT NULL,            -- 5 факторных баллов FFMQ-15
    describing      REAL NOT NULL,
    acting_aware    REAL NOT NULL,
    nonreactivity   REAL NOT NULL,
    nonjudging      REAL NOT NULL,
    source_file_hash TEXT NOT NULL,           -- хэш подключённого файла опросника
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);
-- Текст пунктов FFMQ в БД и коде не хранится (лицензионная гигиена, SCALES.md §6.1).

-- ============================================================ meta
CREATE TABLE schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT NOT NULL
);
```

## 4. `metrics_json` по тренажёрам

Контракт: JSON-объект с фиксированными ключами по типу тренажёра — валидируется
в `decon-core` (serde) перед записью; неизвестные ключи отклоняются.

```jsonc
// schulte
{ "mode": "classic|field|dkv", "grid": "5x5", "symbols": "digits|letters|bicolor",
  "duration_ms": 41230, "errors": 1, "cell_rt_ms": [812, 655, /* ... */] }

// mot
{ "targets": 4, "distractors": 8, "speed": 1.0, "trial_ms": 8000,
  "correct_pct": 62.5, "answer_rt_ms": [1450, 1620, /* ... */] }

// layer_switch
{ "rounds": 20, "mean_switch_rt_ms": 810, "p95_switch_rt_ms": 1400,
  "success_pct": 85.0 }
```

## 5. Ключи `settings` (справочник, не исчерпывающий)

| Ключ | Значение |
|------|----------|
| `probe.window_start` / `probe.window_end` | `"09:00"` / `"23:00"` — окно активности |
| `probe.count` / `probe.min_gap_minutes` | `4` / `90` |
| `probe.enabled` | `true` |
| `session.signal_interval_s` | `300` — интервал равномерных сигналов |
| `i18n.lang` | `"ru"` / `"en"` |
| `crypto.mode` | `"auto"` / `"passphrase"` |
| `events.custom_kinds` | (устарело) JSON-массив пользовательских кнопок-маркеров; кнопки-маркеры удалены из практики (ARCHITECTURE.md §4.1: ноль интерактива во время сессии), ключ не читается и вперёд не развивается |
| `residency.autostart` | `true` — запускаться с системой (XDG / HKCU Run) |
| `residency.sound` | `"off"` / `"chime"` / `"bell"` — звук оповещений |
| `residency.reminder_interval_min` | `15` — повтор-«будильник» неотвеченного зонда (0 — выкл) |
| `residency.show_window_on_start` | `true` — показывать окно при запуске (false — старт в трей) |

Мастер-ключ auto-режима в `settings` не хранится — только в OS keyring
под именем `decon.master-key` (SPEC §7.1, реализация S2:
`db/keyring.rs` + `decon-app/src/platform/secret_store.rs`).

**Миграция 0002** (S2): `pre.openness` + `probe.openness` → единая шкала
`scale.openness` (новая строка + backfill ответов, PK не переименовывается
из-за FK `scale_versions.scale_id`; дубли формулировок удаляются после
переноса ответов, старые определения — soft-delete). Контекст ответа
(pre-замер / зонд) выводится из `responses.session_id` / `probe_id`.

**Миграция 0003** (S2.5): резидентный режим и честный shutdown —
`sessions.end_status` (CHECK `finished`/`interrupted`/`aborted`; NULL —
идёт сейчас или завершена до S2.5) и `probes.last_reminder_at`.
Исторические строки не перезаписываются; recovery-проход при старте
закрывает висящие сессии (`ended_at IS NULL` → `interrupted`,
`duration_s` от `started_at`).

## 6. Миграции

- Расположение: `decon-core/src/db/migrations/` — по файлу на версию,
  `NNNN_name.sql` (up) + `NNNN_name.down.sql` (down).
- Применение: при старте приложения в транзакции; записывается в
  `schema_version`.
- Правила: только добавление таблиц/колонок; переименование = новая колонка +
  backfill в той же миграции; `ALTER ... DROP` запрещён (soft-delete вместо
  этого). Down-файлы обязательны и тестируются (ROADMAP S1, критерий 2).
- Пользовательская миграция не нужна: обновление схемы — часть обновления
  приложения, невидимая для пользователя (ноль конфигов).

## 7. Шифрование (детали реализации `db/crypto.rs`)

1. **Режим auto (по умолчанию):** ключ fernet (32 байта) генерируется при
   первом запуске, сохраняется в OS keyring как `decon.master-key`.
   `argon2id` не используется.
2. **Режим passphrase:** ключ = argon2id(passphrase, salt), salt — случайный,
   хранится в `settings` открыто (salt не секретен). Passphrase нигде не
   пишется. Ключ в keyring кладётся только если пользователь включил
   «запомнить на этом устройстве».
3. **Ротация ключа** (смена passphrase): повторное шифрование всех 🔒-полей
   в одной транзакции; при сбое — полный откат.
4. **Ошибки расшифровки** (не тот ключ): поле возвращается как
   `DecryptionError`, запись остаётся целой, UI показывает причину; никаких
   тихих потерь.

## 8. Зарезервировано под биометрию (не в скоупах S0–S7)

При появлении оборудования — отдельная миграция, ядро схемы не меняется:

```sql
-- CREATE TABLE measurements (
--     id TEXT PRIMARY KEY,
--     kind TEXT NOT NULL,          -- 'hrv_rmssd','hrv_hf','eeg_bandpower',...
--     session_id TEXT NULL REFERENCES sessions(id),
--     taken_at TEXT NOT NULL,
--     value REAL NOT NULL,
--     unit TEXT NOT NULL,
--     device_meta_json TEXT NOT NULL,
--     updated_at TEXT NOT NULL, deleted_at TEXT NULL
-- );
```

## 9. Экспорт

- **CSV:** денормализованные таблицы (sessions, responses, trainer_results,
  probes) по одной на файл; 🔒-поля — только с явным флагом «включить заметки»;
  иначе `"[encrypted]"`.
- **JSON:** полное дерево пользователя (без 🔒-полей по умолчанию), схема
  `"export_format": "decon/v1"`. Roundtrip импорт — тест (ROADMAP S4,
  критерий 3). Импорт не перезаписывает: новые ID, дедупликация по
  `(source_updated_at, content_hash)`.

Дерево `decon/v1` (реализация S4, `analysis/export.rs`):

- `scales` + `scale_versions` едут в файле: без формулировок значения
  ответов неинтерпретируемы (SCALES.md §8.4). Версии с совпадающей парой
  `(scale_id, version)` не дублируются — ответы переклеиваются на
  фактический `scale_version_id` базы.
- Дедупликация: сессии по `(source_updated_at, content_hash)`, зонды — по
  `content_hash` (FNV-1a по содержимому без ID); повторный импорт того же
  файла не добавляет ни одной строки.
- `updated_at` импортированной сессии = исходный из файла (это ключ
  дедупликации; `sync_updated_at` обновит его при первом изменении).
- Ответы на пропущенные (дубликатные) сессии/зонды пропускаются; ответы с
  неизвестной версией шкалы — тоже (без формулировки значение не имеет
  смысла).
