-- 0001_init (up): начальная схема Decon — SPEC-data-model.md §3.
-- Все временные метки — UTC ISO-8601; id — UUID v7 (TEXT).
-- 🔒-поля (note_enc) хранят fernet-токены.

CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE scales (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    anchor_low  TEXT NOT NULL,
    anchor_high TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('slider','flag','factor')),
    active      INTEGER NOT NULL DEFAULT 1,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);

CREATE TABLE scale_versions (
    id          TEXT PRIMARY KEY,
    scale_id    TEXT NOT NULL REFERENCES scales(id),
    version     INTEGER NOT NULL,
    question    TEXT NOT NULL,
    anchors_json TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (scale_id, version)
);

CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('practice','trainer')),
    layer       TEXT NULL,
    technique   TEXT NULL,
    started_at  TEXT NOT NULL,
    ended_at    TEXT NULL,
    duration_s  INTEGER NULL,
    local_time_bucket TEXT NULL CHECK (local_time_bucket IN
                    ('morning','day','evening','night')),
    sleep_hours     REAL NULL,
    caffeine       INTEGER NULL,
    posture        TEXT NULL,
    note_enc       TEXT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);
CREATE INDEX idx_sessions_started ON sessions(started_at);
CREATE INDEX idx_sessions_layer   ON sessions(layer) WHERE deleted_at IS NULL;

CREATE TABLE session_events (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id),
    t_offset_ms INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN
                  ('collapse_focus','drift_semantic','drift_body',
                   'clarity_flash','pause','resume','signal')),
    note_enc     TEXT NULL
);
CREATE INDEX idx_events_session ON session_events(session_id);

CREATE TABLE responses (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NULL REFERENCES sessions(id),
    probe_id    TEXT NULL REFERENCES probes(id),
    scale_version_id TEXT NOT NULL REFERENCES scale_versions(id),
    value       REAL NOT NULL,
    answered_at TEXT NOT NULL,
    delay_minutes INTEGER NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL,
    CHECK (session_id IS NOT NULL OR probe_id IS NOT NULL)
);
CREATE INDEX idx_responses_scale ON responses(scale_version_id, answered_at);

CREATE TABLE probes (
    id          TEXT PRIMARY KEY,
    scheduled_at TEXT NOT NULL,
    fired_at    TEXT NULL,
    answered_at TEXT NULL,
    delay_minutes INTEGER NULL,
    window_state TEXT NOT NULL CHECK (window_state IN
                    ('scheduled','fired','answered','missed','skipped')),
    updated_at  TEXT NOT NULL
);
CREATE INDEX idx_probes_scheduled ON probes(scheduled_at);

CREATE TABLE trainer_results (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id),
    trainer     TEXT NOT NULL CHECK (trainer IN
                    ('schulte','mot','ufov','audio_field','layer_switch')),
    config_json TEXT NOT NULL,
    metrics_json TEXT NOT NULL,
    seed        TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);
CREATE INDEX idx_trainer_results ON trainer_results(trainer, session_id);

CREATE TABLE ffqm_results (
    id          TEXT PRIMARY KEY,
    taken_at    TEXT NOT NULL,
    observing       REAL NOT NULL,
    describing      REAL NOT NULL,
    acting_aware    REAL NOT NULL,
    nonreactivity   REAL NOT NULL,
    nonjudging      REAL NOT NULL,
    source_file_hash TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT NULL
);

CREATE TABLE schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT NOT NULL
);
