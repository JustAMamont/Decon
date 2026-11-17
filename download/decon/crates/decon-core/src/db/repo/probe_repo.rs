//! Репозиторий зондов дня: таблица `probes`.
//!
//! Догоняющий режим (SCALES.md §5): показанный и неотвеченный зонд
//! (`state='fired'`) остаётся доступным до конца дня; ответ пишется
//! через `answer_with_delay` с автоматическим `delay_minutes`.

use crate::db::DbError;
use crate::model::probe::{Probe, ProbeState};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

/// Репозиторий зондов.
pub struct ProbeRepo<'a> {
    conn: &'a Connection,
}

impl<'a> ProbeRepo<'a> {
    /// Репозиторий поверх подключения.
    pub fn new(conn: &'a Connection) -> Self {
        ProbeRepo { conn }
    }

    /// Запланировать зонды на день (батч из планировщика).
    pub fn insert_batch(&self, probes: &[Probe]) -> Result<usize, DbError> {
        if probes.is_empty() {
            return Ok(0);
        }
        let conn = self.conn;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO probes (id, scheduled_at, fired_at, answered_at,
                     delay_minutes, last_reminder_at, window_state, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                )
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            for p in probes {
                stmt.execute(params![
                    p.id.to_string(),
                    p.scheduled_at,
                    p.fired_at,
                    p.answered_at,
                    p.delay_minutes,
                    p.last_reminder_at,
                    p.state.as_code(),
                    p.scheduled_at,
                ])
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(probes.len())
    }

    /// Зонд по ID.
    pub fn get(&self, id: &Uuid) -> Result<Option<Probe>, DbError> {
        self.conn
            .query_row(
                "SELECT id, scheduled_at, fired_at, answered_at, delay_minutes,
                 last_reminder_at, window_state
                 FROM probes WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok(Probe {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                "bad uuid".into(),
                            )
                        })?,
                        scheduled_at: row.get(1)?,
                        fired_at: row.get(2)?,
                        answered_at: row.get(3)?,
                        delay_minutes: row.get(4)?,
                        last_reminder_at: row.get(5)?,
                        state: ProbeState::from_code(&row.get::<_, String>(6)?).ok_or_else(
                            || {
                                rusqlite::Error::FromSqlConversionFailure(
                                    6,
                                    rusqlite::types::Type::Text,
                                    "bad state".into(),
                                )
                            },
                        )?,
                    })
                },
            )
            .optional()
            .map_err(|e| DbError::Sqlite(e.to_string()))
    }

    /// Сохранить зонд (модель уже изменена: fire/answer/expire).
    pub fn update(&self, probe: &Probe) -> Result<(), DbError> {
        self.conn
            .execute(
                "UPDATE probes SET fired_at=?2, answered_at=?3, delay_minutes=?4,
                 last_reminder_at=?5, window_state=?6,
                 updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 WHERE id=?1",
                params![
                    probe.id.to_string(),
                    probe.fired_at,
                    probe.answered_at,
                    probe.delay_minutes,
                    probe.last_reminder_at,
                    probe.state.as_code(),
                ],
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Зонды, которые пора показать: scheduled и плановое время ≤ `now`.
    pub fn due(&self, now: &str) -> Result<Vec<Probe>, DbError> {
        self.list_by(
            "SELECT id, scheduled_at, fired_at, answered_at, delay_minutes,
             last_reminder_at, window_state
             FROM probes WHERE window_state = 'scheduled' AND scheduled_at <= ?1
             ORDER BY scheduled_at",
            [now],
        )
    }

    /// Догоняющий список: показанные, но неотвеченные (`fired`).
    pub fn catchable(&self) -> Result<Vec<Probe>, DbError> {
        self.list_by(
            "SELECT id, scheduled_at, fired_at, answered_at, delay_minutes,
             last_reminder_at, window_state
             FROM probes WHERE window_state = 'fired'
             ORDER BY scheduled_at DESC",
            [],
        )
    }

    /// Зонды дня по префиксу даты планового времени (`YYYY-MM-DD`).
    pub fn list_day(&self, date_prefix: &str) -> Result<Vec<Probe>, DbError> {
        self.list_by(
            "SELECT id, scheduled_at, fired_at, answered_at, delay_minutes,
             last_reminder_at, window_state
             FROM probes WHERE scheduled_at LIKE ?1 || '%'
             ORDER BY scheduled_at",
            [date_prefix],
        )
    }

    /// Задать параметры запроса → список зондов (общий код list_*).
    fn list_by<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Probe>, DbError> {
        let conn = self.conn;
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map(params, |row| {
                Ok(Probe {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            "bad uuid".into(),
                        )
                    })?,
                    scheduled_at: row.get(1)?,
                    fired_at: row.get(2)?,
                    answered_at: row.get(3)?,
                    delay_minutes: row.get(4)?,
                    last_reminder_at: row.get(5)?,
                    state: ProbeState::from_code(&row.get::<_, String>(6)?).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            "bad state".into(),
                        )
                    })?,
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

    fn probe_at(minute: u16) -> Probe {
        let h = 9 + minute / 60;
        let m = minute % 60;
        Probe::scheduled(format!("2026-11-18T{h:02}:{m:02}:00"))
    }

    #[test]
    fn insert_due_catchable_flow() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = ProbeRepo::new(&conn);

        let batch = [probe_at(0), probe_at(120), probe_at(240)];
        let n = repo.insert_batch(&batch).unwrap();
        assert_eq!(n, 3);

        // В 10:00 должен «проснуться» только первый
        let due = repo.due("2026-11-18T10:00:00").unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].scheduled_at, "2026-11-18T09:00:00");

        // Показали его
        let mut p = due[0].clone();
        assert!(p.fire("2026-11-18T09:00:05".into()));
        repo.update(&p).unwrap();

        // Не отвечен — попадает в догоняющие
        let catch = repo.catchable().unwrap();
        assert_eq!(catch.len(), 1);
        assert!(catch[0].is_catchable());

        // Отвечен — исчезает из догоняющих
        assert!(p.answer("2026-11-18T11:30:00".into(), Some(150)));
        repo.update(&p).unwrap();
        assert_eq!(repo.catchable().unwrap().len(), 0);
    }

    #[test]
    fn day_listing_and_states() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = ProbeRepo::new(&conn);

        repo.insert_batch(&[probe_at(10), probe_at(200)]).unwrap();
        let day = repo.list_day("2026-11-18").unwrap();
        assert_eq!(day.len(), 2);
        assert!(day.iter().all(|p| p.state == ProbeState::Scheduled));

        // Другой день пуст
        assert_eq!(repo.list_day("2026-11-19").unwrap().len(), 0);

        // Полночь: все неотвеченные → missed (данные честности)
        for mut p in day {
            p.expire();
            repo.update(&p).unwrap();
        }
        let missed = repo.list_day("2026-11-18").unwrap();
        assert!(missed.iter().all(|p| p.state == ProbeState::Missed));
    }

    #[test]
    fn get_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = ProbeRepo::new(&conn);

        let p = probe_at(30);
        let id = p.id;
        repo.insert_batch(&[p]).unwrap();
        let loaded = repo.get(&id).unwrap().expect("exists");
        assert_eq!(loaded.scheduled_at, "2026-11-18T09:30:00");
        assert_eq!(loaded.state, ProbeState::Scheduled);
    }
}
