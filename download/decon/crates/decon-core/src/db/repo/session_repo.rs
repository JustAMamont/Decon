//! Репозиторий сессий: CRUD таблицы `sessions`.
//!
//! Заметка шифруется на записи и расшифровывается на чтении — слой БД
//! никогда не видит открытый текст (🔐 в памяти, 🔒 на диске).

use crate::db::DbError;
use crate::db::crypto::FieldCryptor;
use crate::model::session::{LocalTimeBucket, Session, SessionKind};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

/// Репозиторий `sessions`.
pub struct SessionRepo<'a> {
    conn: &'a Connection,
    cryptor: Option<&'a FieldCryptor>,
}

impl<'a> SessionRepo<'a> {
    /// Репозиторий поверх существующего подключения (без шифрования —
    /// для тестов и миграций).
    pub fn new(conn: &'a Connection) -> Self {
        SessionRepo {
            conn,
            cryptor: None,
        }
    }

    /// Привязать шифровальщик заметок.
    pub fn with_cryptor(mut self, cryptor: &'a FieldCryptor) -> Self {
        self.cryptor = Some(cryptor);
        self
    }

    /// Вставить сессию.
    pub fn insert(&self, s: &Session) -> Result<(), DbError> {
        let note_enc = self.encrypt_note(s.note.as_deref())?;
        self.conn.execute(
            "INSERT INTO sessions (id, kind, layer, technique, started_at, ended_at,
             duration_s, end_status, local_time_bucket, sleep_hours, caffeine, posture,
             note_enc, updated_at, deleted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                s.id.to_string(),
                kind_code(s.kind()),
                s.layer().map(|l| l.as_code()),
                s.technique(),
                s.started_at(),
                s.ended_at(),
                s.duration_s(),
                s.end_status().map(|st| st.as_code()),
                s.local_time_bucket().map(|b| b.as_code()),
                s.context.sleep_hours,
                s.context.caffeine,
                s.context.posture,
                note_enc,
                s.updated_at(),
                s.deleted_at(),
            ],
        )?;
        Ok(())
    }

    /// Загрузить сессию по ID (живую или удалённую).
    pub fn get(&self, id: &Uuid) -> Result<Option<Session>, DbError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, kind, layer, technique, started_at, ended_at, duration_s,
                 end_status, local_time_bucket, sleep_hours, caffeine, posture, note_enc,
                 updated_at, deleted_at
                 FROM sessions WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok(Row {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        layer: row.get(2)?,
                        technique: row.get(3)?,
                        started_at: row.get(4)?,
                        ended_at: row.get(5)?,
                        duration_s: row.get(6)?,
                        end_status: row.get(7)?,
                        bucket: row.get(8)?,
                        sleep_hours: row.get(9)?,
                        caffeine: row.get(10)?,
                        posture: row.get(11)?,
                        note_enc: row.get(12)?,
                        updated_at: row.get(13)?,
                        deleted_at: row.get(14)?,
                    })
                },
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some(r) => Ok(Some(self.hydrate(r)?)),
        }
    }

    /// Recovery-проход (S2.5): все сессии без `ended_at` (краш/сигнал мимо
    /// обработчика) финализируются как `interrupted`. `duration_s`
    /// считается от `started_at` до `ended_at` обеих меток ISO-8601 UTC
    /// (расчёт через `unix_ms_from_iso` — без парсинга в SQL).
    ///
    /// Возвращает число закрытых сессий. Идемпотентно: повторный вызов на
    /// здоровой базе — 0.
    pub fn finalize_dangling(&self, ended_at: &str) -> Result<usize, DbError> {
        let dangling: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, started_at FROM sessions WHERE ended_at IS NULL")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };
        if dangling.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE sessions SET ended_at = ?2, duration_s = ?3,
                 end_status = 'interrupted',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 WHERE id = ?1",
            )?;
            for (id, started_at) in &dangling {
                let duration_s = crate::model::session::unix_ms_from_iso(started_at)
                    .and_then(|start_ms| {
                        crate::model::session::unix_ms_from_iso(ended_at)
                            .map(|end_ms| (end_ms - start_ms) / 1000)
                    })
                    .unwrap_or(0);
                stmt.execute(params![id, ended_at, duration_s])?;
            }
        }
        tx.commit()?;
        Ok(dangling.len())
    }

    /// Список живых сессий по убыванию даты начала (история журнала).
    pub fn list_recent(&self, limit: u32) -> Result<Vec<Session>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, layer, technique, started_at, ended_at, duration_s,
             end_status, local_time_bucket, sleep_hours, caffeine, posture, note_enc,
             updated_at, deleted_at
             FROM sessions WHERE deleted_at IS NULL
             ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(Row {
                id: row.get(0)?,
                kind: row.get(1)?,
                layer: row.get(2)?,
                technique: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                duration_s: row.get(6)?,
                end_status: row.get(7)?,
                bucket: row.get(8)?,
                sleep_hours: row.get(9)?,
                caffeine: row.get(10)?,
                posture: row.get(11)?,
                note_enc: row.get(12)?,
                updated_at: row.get(13)?,
                deleted_at: row.get(14)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(self.hydrate(r?)?);
        }
        Ok(out)
    }

    /// Сохранить изменения существующей сессии (updated_at уже обновлён
    /// сеттерами модели).
    pub fn update(&self, s: &Session) -> Result<(), DbError> {
        let note_enc = self.encrypt_note(s.note.as_deref())?;
        self.conn.execute(
            "UPDATE sessions SET kind=?2, layer=?3, technique=?4, started_at=?5,
             ended_at=?6, duration_s=?7, end_status=?8, local_time_bucket=?9,
             sleep_hours=?10, caffeine=?11, posture=?12, note_enc=?13,
             updated_at=?14, deleted_at=?15
             WHERE id=?1",
            params![
                s.id.to_string(),
                kind_code(s.kind()),
                s.layer().map(|l| l.as_code()),
                s.technique(),
                s.started_at(),
                s.ended_at(),
                s.duration_s(),
                s.end_status().map(|st| st.as_code()),
                s.local_time_bucket().map(|b| b.as_code()),
                s.context.sleep_hours,
                s.context.caffeine,
                s.context.posture,
                note_enc,
                s.updated_at(),
                s.deleted_at(),
            ],
        )?;
        Ok(())
    }

    /// Превращение строки БД в доменный объект: коды → enum, токен → текст.
    fn hydrate(&self, r: Row) -> Result<Session, DbError> {
        let kind = match r.kind.as_str() {
            "practice" => SessionKind::Practice,
            _ => SessionKind::Trainer,
        };
        let layer = r
            .layer
            .as_deref()
            .and_then(crate::model::session::Layer::from_code);
        let bucket = r.bucket.as_deref().and_then(LocalTimeBucket::from_code);
        let end_status = r
            .end_status
            .as_deref()
            .and_then(crate::model::session::SessionEndStatus::from_code);
        let note = match r.note_enc {
            None => Ok(None),
            Some(token) => match self.cryptor {
                None => Err(DbError::Crypto(
                    "note is encrypted but no cryptor bound".into(),
                )),
                Some(c) => c.decrypt(&token).map(Some).map_err(DbError::from),
            },
        }?;
        let updated_at = r.updated_at.clone();
        let mut s = Session::from_parts(
            Uuid::parse_str(&r.id).map_err(|e| DbError::Parse(e.to_string()))?,
            kind,
            layer,
            r.technique,
            r.started_at,
            r.ended_at,
            r.duration_s,
            end_status,
            bucket,
            crate::model::session::SessionContext {
                sleep_hours: r.sleep_hours,
                caffeine: r.caffeine,
                posture: r.posture,
            },
            note,
            r.updated_at,
            r.deleted_at,
        );
        s.sync_updated_at(updated_at);
        Ok(s)
    }

    /// Шифрование заметки перед записью.
    fn encrypt_note(&self, note: Option<&str>) -> Result<Option<String>, DbError> {
        match (note, self.cryptor) {
            (None, _) => Ok(None),
            (Some(text), Some(c)) => Ok(Some(c.encrypt(text))),
            (Some(_), None) => Err(DbError::Crypto("cannot save note without cryptor".into())),
        }
    }
}

/// Внутреннее представление строки (все поля — как в БД).
struct Row {
    id: String,
    kind: String,
    layer: Option<String>,
    technique: Option<String>,
    started_at: String,
    ended_at: Option<String>,
    duration_s: Option<i64>,
    end_status: Option<String>,
    bucket: Option<String>,
    sleep_hours: Option<f64>,
    caffeine: Option<i64>,
    posture: Option<String>,
    note_enc: Option<String>,
    updated_at: String,
    deleted_at: Option<String>,
}

/// Код типа сессии для колонки `kind`.
const fn kind_code(kind: SessionKind) -> &'static str {
    match kind {
        SessionKind::Practice => "practice",
        SessionKind::Trainer => "trainer",
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e.to_string())
    }
}

impl From<crate::db::crypto::CryptoError> for DbError {
    fn from(e: crate::db::crypto::CryptoError) -> Self {
        DbError::Crypto(e.to_string())
    }
}
