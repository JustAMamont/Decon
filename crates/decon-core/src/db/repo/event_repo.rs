//! Репозиторий событий сессий (`session_events`).
//!
//! Кнопки-маркеры UI пишут события в буфер журнала (в памяти, 0 аллокаций
//! на append), буфер флашится батчами в одну транзакцию — SPEC §7 hot path.

use crate::db::DbError;
use crate::db::crypto::FieldCryptor;
use crate::model::event::{EventKind, SessionEvent};
use rusqlite::{Connection, params};
use uuid::Uuid;

/// Репозиторий `session_events`.
pub struct EventRepo<'a> {
    conn: &'a Connection,
    cryptor: Option<&'a FieldCryptor>,
}

impl<'a> EventRepo<'a> {
    /// Репозиторий поверх подключения.
    pub fn new(conn: &'a Connection) -> Self {
        EventRepo {
            conn,
            cryptor: None,
        }
    }

    /// Привязать шифровальщик заметок.
    pub fn with_cryptor(mut self, cryptor: &'a FieldCryptor) -> Self {
        self.cryptor = Some(cryptor);
        self
    }

    /// Вставить пачку событий одной транзакцией (батч из буфера журнала).
    pub fn insert_batch(&self, events: &[SessionEvent]) -> Result<(), DbError> {
        if events.is_empty() {
            return Ok(());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO session_events (id, session_id, t_offset_ms, kind, note_enc)
                     VALUES (?1,?2,?3,?4,?5)",
                )
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            for e in events {
                let note_enc = match (&e.note, self.cryptor) {
                    (None, _) => None,
                    (Some(text), Some(c)) => Some(c.encrypt(text)),
                    (Some(_), None) => {
                        return Err(DbError::Crypto(
                            "cannot save event note without cryptor".into(),
                        ));
                    }
                };
                stmt.execute(params![
                    e.id.to_string(),
                    e.session_id.to_string(),
                    e.t_offset_ms,
                    e.kind.as_code(),
                    note_enc,
                ])
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Все события сессии по возрастанию таймкода.
    pub fn list_of_session(&self, session_id: &Uuid) -> Result<Vec<SessionEvent>, DbError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, t_offset_ms, kind, note_enc
                 FROM session_events WHERE session_id = ?1
                 ORDER BY t_offset_ms ASC, id ASC",
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([session_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            let (id, sid, t, kind_code, note_enc) =
                r.map_err(|e| DbError::Sqlite(e.to_string()))?;
            let note = match note_enc {
                None => None,
                Some(token) => match self.cryptor {
                    None => {
                        return Err(DbError::Crypto(
                            "note is encrypted but no cryptor bound".into(),
                        ));
                    }
                    Some(c) => Some(
                        c.decrypt(&token)
                            .map_err(|e| DbError::Crypto(e.to_string()))?,
                    ),
                },
            };
            out.push(SessionEvent {
                id: Uuid::parse_str(&id).map_err(|e| DbError::Parse(e.to_string()))?,
                session_id: Uuid::parse_str(&sid).map_err(|e| DbError::Parse(e.to_string()))?,
                t_offset_ms: t,
                kind: EventKind::from_code(&kind_code)
                    .ok_or_else(|| DbError::Parse(format!("bad kind: {kind_code}")))?,
                note,
            });
        }
        Ok(out)
    }
}
