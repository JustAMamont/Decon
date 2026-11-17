//! Репозиторий результатов тренажёров: таблица `trainer_results` (S3).
//!
//! Общий журнал (критерий приёмки S3 №3): результат тренажёра всегда
//! привязан к сессии `kind='trainer'`, поэтому журнал практики и
//! тренажёры читаются одними запросами по `sessions`.
//!
//! Контракт `metrics_json`/`config_json` — SPEC §4: валидация перед
//! записью через serde-модели с `deny_unknown_fields`; схема хранится
//! в decon-core (не в trainers), чтобы БД не зависела от движков
//! (направление зависимостей — CONVENTIONS §7.1: trainers → core).

use crate::db::DbError;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Тип тренажёра — колонка `trainer` (CHECK в DDL, SPEC §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainerKind {
    /// Таблицы Шульте (S3).
    Schulte,
    /// Multiple object tracking (S3).
    Mot,
    /// Useful field of view (S5).
    Ufov,
    /// Звуковое поле (S5).
    AudioField,
    /// Переключение слоёв (S5).
    LayerSwitch,
}

impl TrainerKind {
    /// Код для колонки `trainer`.
    pub const fn as_code(self) -> &'static str {
        match self {
            TrainerKind::Schulte => "schulte",
            TrainerKind::Mot => "mot",
            TrainerKind::Ufov => "ufov",
            TrainerKind::AudioField => "audio_field",
            TrainerKind::LayerSwitch => "layer_switch",
        }
    }

    /// Из кода БД.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "schulte" => Some(TrainerKind::Schulte),
            "mot" => Some(TrainerKind::Mot),
            "ufov" => Some(TrainerKind::Ufov),
            "audio_field" => Some(TrainerKind::AudioField),
            "layer_switch" => Some(TrainerKind::LayerSwitch),
            _ => None,
        }
    }
}

/// Метрики Шульте — контракт SPEC §4 (ключи фиксированы,
/// неизвестные отклоняются).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchulteMetricsRecord {
    /// Режим: classic / field / dkv.
    pub mode: String,
    /// Сетка, "5x5".
    pub grid: String,
    /// Набор символов: digits / letters / bicolor.
    pub symbols: String,
    /// Полное время, мс.
    pub duration_ms: u64,
    /// Число ошибок.
    pub errors: u32,
    /// Времена реакции по ячейкам, мс.
    #[serde(default)]
    pub cell_rt_ms: Vec<u64>,
}

/// Метрики MOT — контракт SPEC §4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotMetricsRecord {
    /// Число целей.
    pub targets: u8,
    /// Число дистракторов.
    pub distractors: u8,
    /// Скорость.
    pub speed: f32,
    /// Длительность прогона, мс.
    pub trial_ms: u64,
    /// % корректных.
    pub correct_pct: f32,
    /// Латентности ответов, мс.
    #[serde(default)]
    pub answer_rt_ms: Vec<u64>,
}

/// Результат тренажёра — строка `trainer_results`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainerResult {
    /// UUID v7.
    pub id: Uuid,
    /// Сессия `kind='trainer'` (общий журнал).
    pub session_id: Uuid,
    /// Тип тренажёра.
    pub trainer: TrainerKind,
    /// Полная конфигурация прогона (JSON).
    pub config_json: String,
    /// Метрики (JSON, контракт §4).
    pub metrics_json: String,
    /// Сид прогона (воспроизводимость).
    pub seed: String,
}

impl TrainerResult {
    /// Валидация контракта §4 перед записью: JSON метрик должен
    /// соответствовать типу тренажёра. S5-типы (ufov/audio_field/
    /// layer_switch) до появления своих движков проходят как
    /// свободный JSON (контракт закрепится в S5).
    pub fn validate(&self) -> Result<(), DbError> {
        let bad = |msg: &str| DbError::Sqlite(msg.to_string());
        match self.trainer {
            TrainerKind::Schulte => {
                serde_json::from_str::<SchulteMetricsRecord>(&self.metrics_json)
                    .map_err(|e| bad(&format!("schulte metrics contract: {e}")))?;
            }
            TrainerKind::Mot => {
                serde_json::from_str::<MotMetricsRecord>(&self.metrics_json)
                    .map_err(|e| bad(&format!("mot metrics contract: {e}")))?;
            }
            _ => {
                serde_json::from_str::<serde_json::Value>(&self.metrics_json)
                    .map_err(|e| bad(&format!("metrics not valid JSON: {e}")))?;
            }
        }
        // config_json и seed обязаны быть валидным JSON/строкой
        serde_json::from_str::<serde_json::Value>(&self.config_json)
            .map_err(|e| bad(&format!("config not valid JSON: {e}")))?;
        if self.seed.trim().is_empty() {
            return Err(bad("seed must not be empty"));
        }
        Ok(())
    }
}

/// Репозиторий `trainer_results`.
pub struct TrainerResultRepo<'a> {
    conn: &'a Connection,
}

impl<'a> TrainerResultRepo<'a> {
    /// Репозиторий поверх подключения.
    pub fn new(conn: &'a Connection) -> Self {
        TrainerResultRepo { conn }
    }

    /// Сохранить результат (с валидацией контракта §4).
    pub fn insert(&self, r: &TrainerResult) -> Result<(), DbError> {
        r.validate()?;
        self.conn
            .execute(
                "INSERT INTO trainer_results
                 (id, session_id, trainer, config_json, metrics_json, seed, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                params![
                    r.id.to_string(),
                    r.session_id.to_string(),
                    r.trainer.as_code(),
                    r.config_json,
                    r.metrics_json,
                    r.seed,
                ],
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Все результаты одной сессии.
    pub fn list_of_session(&self, session_id: &Uuid) -> Result<Vec<TrainerResult>, DbError> {
        let conn = self.conn;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, trainer, config_json, metrics_json, seed
                 FROM trainer_results
                 WHERE session_id = ?1 AND deleted_at IS NULL",
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([session_id.to_string()], map_row)
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DbError::Sqlite(e.to_string()))?);
        }
        Ok(out)
    }

    /// Последние N результатов по типу тренажёра (для аналитики S4):
    /// соединение с `sessions` даёт `started_at` и фильтр удалённых.
    pub fn list_recent(
        &self,
        trainer: TrainerKind,
        limit: usize,
    ) -> Result<Vec<(TrainerResult, String)>, DbError> {
        let conn = self.conn;
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.session_id, t.trainer, t.config_json,
                        t.metrics_json, t.seed, s.started_at
                 FROM trainer_results t JOIN sessions s ON s.id = t.session_id
                 WHERE t.trainer = ?1 AND t.deleted_at IS NULL
                 ORDER BY s.started_at DESC LIMIT ?2",
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params![trainer.as_code(), limit as i64], |row| {
                let res = map_row(row)?;
                let started_at: String = row.get(6)?;
                Ok((res, started_at))
            })
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| DbError::Sqlite(e.to_string()))?);
        }
        Ok(out)
    }
}

/// Разбор строки таблицы (общий для list-методов).
fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrainerResult> {
    let trainer_code: String = row.get(2)?;
    let trainer =
        TrainerKind::from_code(&trainer_code).ok_or(rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            "bad trainer kind".into(),
        ))?;
    Ok(TrainerResult {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                "bad uuid".into(),
            )
        })?,
        session_id: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                "bad uuid".into(),
            )
        })?,
        trainer,
        config_json: row.get(3)?,
        metrics_json: row.get(4)?,
        seed: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, open_in_memory};

    fn setup() -> (Connection, Uuid) {
        let conn = open_in_memory().expect("db");
        db::migrate(&conn).expect("migrate");
        let session_id = Uuid::now_v7();
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at)
             VALUES (?1, 'trainer', '2026-11-16T10:00:00.000Z',
             strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![session_id.to_string()],
        )
        .expect("session");
        (conn, session_id)
    }

    fn schulte_result(session_id: Uuid) -> TrainerResult {
        TrainerResult {
            id: Uuid::now_v7(),
            session_id,
            trainer: TrainerKind::Schulte,
            config_json: r#"{"mode":"field","grid":5,"symbols":"digits"}"#.into(),
            metrics_json: r#"{"mode":"field","grid":"5x5","symbols":"digits",
                "duration_ms":41230,"errors":1,"cell_rt_ms":[812,655]}"#
                .into(),
            seed: "42".into(),
        }
    }

    #[test]
    fn insert_and_read_back_roundtrip() {
        let (conn, sid) = setup();
        let repo = TrainerResultRepo::new(&conn);
        let r = schulte_result(sid);
        repo.insert(&r).expect("insert");
        let list = repo.list_of_session(&sid).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].trainer, TrainerKind::Schulte);
        assert_eq!(list[0].seed, "42");
        assert_eq!(list[0].id, r.id);
    }

    #[test]
    fn unknown_metrics_keys_rejected() {
        let (conn, sid) = setup();
        let repo = TrainerResultRepo::new(&conn);
        let mut r = schulte_result(sid);
        r.metrics_json = r
            .metrics_json
            .replace("\"errors\":1", "\"errors\":1,\"hack\":true}");
        assert!(repo.insert(&r).is_err());
    }

    #[test]
    fn wrong_metrics_type_rejected() {
        let (conn, sid) = setup();
        let repo = TrainerResultRepo::new(&conn);
        let mut r = schulte_result(sid);
        // mot-метрики в schulte-строке — нарушение контракта
        r.metrics_json = r
            .metrics_json
            .replace("\"mode\":\"field\"", "\"targets\":4");
        assert!(repo.insert(&r).is_err());
    }

    #[test]
    fn empty_seed_rejected() {
        let (conn, sid) = setup();
        let repo = TrainerResultRepo::new(&conn);
        let mut r = schulte_result(sid);
        r.seed = "  ".into();
        assert!(repo.insert(&r).is_err());
    }

    #[test]
    fn list_recent_joins_sessions_for_ordering() {
        let (conn, sid) = setup();
        let repo = TrainerResultRepo::new(&conn);
        // Вторая, более поздняя сессия
        let sid2 = Uuid::now_v7();
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at)
             VALUES (?1, 'practice', '2026-11-17T10:00:00.000Z',
             strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![sid2.to_string()],
        )
        .expect("session2");
        repo.insert(&schulte_result(sid)).expect("r1");
        let mut r2 = schulte_result(sid2);
        r2.id = Uuid::now_v7();
        repo.insert(&r2).expect("r2");
        let recent = repo.list_recent(TrainerKind::Schulte, 10).expect("recent");
        assert_eq!(recent.len(), 2);
        // Позже начатая — первой
        assert_eq!(recent[0].1, "2026-11-17T10:00:00.000Z");
    }

    #[test]
    fn session_kind_trainer_in_journal() {
        // Критерий 3: тренажёрные сессии читаются общим журналом
        // (kind='trainer' в тех же sessions, что и практика).
        let conn = open_in_memory().expect("db");
        db::migrate(&conn).expect("migrate");
        let sid_t = Uuid::now_v7();
        let sid_p = Uuid::now_v7();
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at)
             VALUES (?1,'trainer','2026-11-16T10:00:00.000Z',
                     strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                    (?2,'practice','2026-11-16T11:00:00.000Z',
                     strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![sid_t.to_string(), sid_p.to_string()],
        )
        .expect("sessions");
        let n_trainer: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE kind='trainer'",
                [],
                |r| r.get(0),
            )
            .expect("c1");
        let n_all: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .expect("c2");
        assert_eq!(n_trainer, 1);
        assert_eq!(n_all, 2); // общий журнал: оба типа вместе
    }
}
