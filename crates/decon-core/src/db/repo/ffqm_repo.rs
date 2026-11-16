//! Репозиторий FFMQ-15: таблица `ffqm_results`.
//!
//! Внешний инструмент (SCALES.md §6.1): текст пунктов в Decon не хранится
//! (лицензионная гигиена), результат — 5 факторных баллов + хэш файла
//! опросника, по которому они получены.

use crate::db::DbError;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Результат FFMQ-15 — строка `ffqm_results`.
///
/// Имена полей — как в DDL, кроме `non_reactivity` (в SQL-колонке
/// `nonreactivity`; подчёркивание в Rust-имени оставлено сознательно).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FfmqResult {
    /// UUID v7.
    pub id: Uuid,
    /// Момент прохождения (ISO-8601).
    pub taken_at: String,
    /// Наблюдение (фактор 1), 0–100.
    pub observing: f64,
    /// Описание (фактор 2), 0–100.
    pub describing: f64,
    /// Действие с осознаванием (фактор 3), 0–100.
    pub acting_aware: f64,
    /// Нереактивность (фактор 4), 0–100.
    pub non_reactivity: f64,
    /// Неосуждение (фактор 5), 0–100.
    pub nonjudging: f64,
    /// SHA-256 подключённого файла опросника.
    pub source_file_hash: String,
}

impl FfmqResult {
    /// Все пять факторных баллов в диапазоне 0..=100.
    pub fn is_valid(&self) -> bool {
        [
            self.observing,
            self.describing,
            self.acting_aware,
            self.non_reactivity,
            self.nonjudging,
        ]
        .iter()
        .all(|v| (0.0..=100.0).contains(v))
    }
}

/// Репозиторий FFMQ-результатов.
pub struct FfmqRepo<'a> {
    conn: &'a Connection,
}

impl<'a> FfmqRepo<'a> {
    /// Репозиторий поверх подключения.
    pub fn new(conn: &'a Connection) -> Self {
        FfmqRepo { conn }
    }

    /// Сохранить результат.
    pub fn insert(&self, r: &FfmqResult) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO ffqm_results (id, taken_at, observing, describing,
                 acting_aware, nonreactivity, nonjudging, source_file_hash, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                params![
                    r.id.to_string(),
                    r.taken_at,
                    r.observing,
                    r.describing,
                    r.acting_aware,
                    r.non_reactivity,
                    r.nonjudging,
                    r.source_file_hash,
                ],
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Последние N результатов (для слойной карты и трендов).
    pub fn list_recent(&self, limit: usize) -> Result<Vec<FfmqResult>, DbError> {
        let conn = self.conn;
        let mut stmt = conn
            .prepare(
                "SELECT id, taken_at, observing, describing, acting_aware, nonreactivity,
                 nonjudging, source_file_hash
                 FROM ffqm_results WHERE deleted_at IS NULL
                 ORDER BY taken_at DESC LIMIT ?1",
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(FfmqResult {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            "bad uuid".into(),
                        )
                    })?,
                    taken_at: row.get(1)?,
                    observing: row.get(2)?,
                    describing: row.get(3)?,
                    acting_aware: row.get(4)?,
                    non_reactivity: row.get(5)?,
                    nonjudging: row.get(6)?,
                    source_file_hash: row.get(7)?,
                })
            })
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| DbError::Sqlite(e.to_string()))?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn sample() -> FfmqResult {
        FfmqResult {
            id: Uuid::now_v7(),
            taken_at: "2026-11-18T18:00:00".into(),
            observing: 62.5,
            describing: 70.0,
            acting_aware: 55.0,
            non_reactivity: 48.5,
            nonjudging: 66.0,
            source_file_hash: "a".repeat(64),
        }
    }

    #[test]
    fn insert_and_list_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = FfmqRepo::new(&conn);

        let s = sample();
        assert!(s.is_valid());
        repo.insert(&s).unwrap();

        let list = repo.list_recent(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].observing, 62.5);
        assert_eq!(list[0].non_reactivity, 48.5);
        assert_eq!(list[0].source_file_hash, "a".repeat(64));
    }

    #[test]
    fn validation_rejects_out_of_range() {
        let mut s = sample();
        s.observing = 101.0;
        assert!(!s.is_valid());
        s.observing = -1.0;
        assert!(!s.is_valid());
        s.observing = 100.0;
        assert!(s.is_valid());
    }
}
