//! Репозиторий шкал: `scales`, `scale_versions`, `responses`.
//!
//! Сидинг дефолтного набора идемпотентен: существует шкала — пропускаем;
//! формулировка изменилась (другая версия Decon) — добавляется новая
//! версия строки, старые ответы остаются привязаны к старой версии
//! (SCALES.md §8.2: тренды не смешиваются).

use crate::db::DbError;
use crate::model::scale::{
    Phase, Phases, Response, Scale, ScaleKind, ScaleVersion, default_questions_v1,
    default_scales_v1,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

/// Репозиторий шкал и ответов.
pub struct ScaleRepo<'a> {
    conn: &'a Connection,
}

impl<'a> ScaleRepo<'a> {
    /// Репозиторий поверх подключения.
    pub fn new(conn: &'a Connection) -> Self {
        ScaleRepo { conn }
    }

    /// Посеять дефолтный набор v1 «Baseline-2026», если ещё не посеян.
    ///
    /// Вызывается при первом запуске (после миграций). Для уже
    /// существующих шкал с той же формулировкой — no-op; с другой
    /// формулировкой — новая версия (обновляются якоря в `scales`).
    pub fn seed_defaults(&self) -> Result<(), DbError> {
        let questions = default_questions_v1();
        for scale in default_scales_v1() {
            let question = questions
                .get(&scale.id)
                .map(|q| q.as_str())
                .unwrap_or(scale.title.as_str());
            let existing = self.get(&scale.id)?;
            match existing {
                None => {
                    self.insert_scale(&scale, question)?;
                }
                Some(current) => {
                    // Якоря в определении шкалы изменились → версия+1.
                    if current.anchor_low != scale.anchor_low
                        || current.anchor_high != scale.anchor_high
                    {
                        self.bump_version(&scale, question)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Вставить шкалу с первой версией формулировки.
    fn insert_scale(&self, scale: &Scale, question: &str) -> Result<(), DbError> {
        let now = crate::model::session::now_iso();
        let conn = self.conn;
        conn.execute(
            "INSERT INTO scales (id, title, anchor_low, anchor_high, kind, active,
             updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                scale.id,
                scale.title,
                scale.anchor_low,
                scale.anchor_high,
                scale.kind.as_code(),
                scale.active as i64,
                now,
            ],
        )
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let version_id = Uuid::now_v7();
        conn.execute(
            "INSERT INTO scale_versions (id, scale_id, version, question, anchors_json,
             created_at) VALUES (?1,?2,1,?3,?4,?5)",
            params![
                version_id.to_string(),
                scale.id,
                question,
                anchors_json(&scale.anchor_low, &scale.anchor_high),
                now,
            ],
        )
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Добавить новую версию формулировки существующей шкалы.
    fn bump_version(&self, scale: &Scale, question: &str) -> Result<(), DbError> {
        let now = crate::model::session::now_iso();
        let conn = self.conn;
        conn.execute(
            "UPDATE scales SET title=?2, anchor_low=?3, anchor_high=?4, updated_at=?5
             WHERE id=?1",
            params![
                scale.id,
                scale.title,
                scale.anchor_low,
                scale.anchor_high,
                now,
            ],
        )
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let next: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM scale_versions WHERE scale_id = ?1",
                [&scale.id],
                |row| row.get(0),
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let version_id = Uuid::now_v7();
        conn.execute(
            "INSERT INTO scale_versions (id, scale_id, version, question, anchors_json,
             created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                version_id.to_string(),
                scale.id,
                next,
                question,
                anchors_json(&scale.anchor_low, &scale.anchor_high),
                now,
            ],
        )
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Загрузить определение шкалы по ID (без версий).
    pub fn get(&self, id: &str) -> Result<Option<Scale>, DbError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, title, anchor_low, anchor_high, kind, active FROM scales
                 WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |row| {
                    let kind_code: String = row.get(4)?;
                    Ok(Scale {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        anchor_low: row.get(2)?,
                        anchor_high: row.get(3)?,
                        kind: ScaleKind::from_code(&kind_code)
                            .ok_or_else(|| parse_err(&format!("bad kind: {kind_code}")))?,
                        phases: Phases::NONE, // не хранится в БД — вычисляется по id
                        active: row.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .optional()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        // Фазы достаём из дефолтного набора (производная от id);
        // пользовательские шкалы без фазы — только pre (конструктор S2+).
        Ok(row.map(|mut s| {
            s.phases = default_scales_v1()
                .into_iter()
                .find(|d| d.id == s.id)
                .map(|d| d.phases)
                .unwrap_or_else(|| Phases::single(Phase::Pre));
            s
        }))
    }

    /// Активные шкалы фазы (для экрана pre/post/probe), актуальные версии.
    ///
    /// Возвращает (шкала, версия формулировки) — UI рисует вопрос из
    /// версии, а не из определения.
    pub fn list_active_phase(&self, phase: Phase) -> Result<Vec<(Scale, ScaleVersion)>, DbError> {
        let mut out = Vec::new();
        for scale in default_scales_v1() {
            if !scale.phases.contains(phase) || !scale.active {
                continue;
            }
            if let Some(version) = self.latest_version(&scale.id)? {
                out.push((scale, version));
            }
        }
        Ok(out)
    }

    /// Актуальная версия формулировки шкалы.
    pub fn latest_version(&self, scale_id: &str) -> Result<Option<ScaleVersion>, DbError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, scale_id, version, question, anchors_json
                 FROM scale_versions WHERE scale_id = ?1
                 ORDER BY version DESC LIMIT 1",
                [scale_id],
                |row| {
                    Ok(ScaleVersion {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?)
                            .map_err(|_| parse_err("bad uuid"))?,
                        scale_id: row.get(1)?,
                        version: row.get(2)?,
                        question: row.get(3)?,
                        anchor_low: String::new(),
                        anchor_high: String::new(),
                    })
                },
            )
            .optional()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        // Якоря достаём из определений (anchors_json — только для истории).
        Ok(row.map(|mut v| {
            let def = default_scales_v1().into_iter().find(|d| d.id == v.scale_id);
            if let Some(d) = def {
                v.anchor_low = d.anchor_low;
                v.anchor_high = d.anchor_high;
            }
            v
        }))
    }

    /// Записать ответы pre/post/carryover сессии одной транзакцией.
    pub fn save_session_responses(&self, responses: &[Response]) -> Result<(), DbError> {
        if responses.is_empty() {
            return Ok(());
        }
        let conn = self.conn;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO responses (id, session_id, probe_id, scale_version_id,
                     value, answered_at, delay_minutes, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                )
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            for r in responses {
                stmt.execute(params![
                    r.id.to_string(),
                    r.session_id.map(|u| u.to_string()),
                    r.probe_id.map(|u| u.to_string()),
                    r.scale_version_id.to_string(),
                    r.value,
                    r.answered_at,
                    r.delay_minutes,
                    r.answered_at,
                ])
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Ответы сессии: (scale_id, версия, значение) — для экрана итогов
    /// и будущей аналитики дельт.
    pub fn session_responses(&self, session_id: &Uuid) -> Result<Vec<(String, i64, f64)>, DbError> {
        let conn = self.conn;
        let mut stmt = conn
            .prepare(
                "SELECT sv.scale_id, sv.version, r.value
                 FROM responses r
                 JOIN scale_versions sv ON sv.id = r.scale_version_id
                 WHERE r.session_id = ?1 AND r.deleted_at IS NULL
                 ORDER BY sv.scale_id, sv.version",
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([session_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| DbError::Sqlite(e.to_string()))?);
        }
        Ok(out)
    }
}

/// anchors_json для scale_versions.
fn anchors_json(low: &str, high: &str) -> String {
    format!("{{\"low\":{},\"high\":{}}}", quote(low), quote(high))
}

/// Мини-экранирование JSON-строки (без тяги serde_json в hot path).
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_err(msg: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn seeding_is_idempotent() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = ScaleRepo::new(&conn);

        repo.seed_defaults().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM scales", [], |r| r.get(0))
            .unwrap();
        let nv: i64 = conn
            .query_row("SELECT COUNT(*) FROM scale_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 12, "12 дефолтных шкал (openness единая)");
        assert_eq!(nv, 12, "по одной версии на шкалу (openness единая)");

        // Повторный сидинг — no-op
        repo.seed_defaults().unwrap();
        let n2: i64 = conn
            .query_row("SELECT COUNT(*) FROM scales", [], |r| r.get(0))
            .unwrap();
        let nv2: i64 = conn
            .query_row("SELECT COUNT(*) FROM scale_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!((n2, nv2), (12, 12));
    }

    #[test]
    fn phase_listing_returns_versions() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = ScaleRepo::new(&conn);
        repo.seed_defaults().unwrap();

        let pre = repo.list_active_phase(Phase::Pre).unwrap();
        assert_eq!(pre.len(), 2);
        assert!(pre.iter().all(|(s, v)| {
            s.phases.contains(Phase::Pre) && v.version == 1 && !v.question.is_empty()
        }));

        let post = repo.list_active_phase(Phase::Post).unwrap();
        assert_eq!(post.len(), 5);
        assert!(post.iter().any(|(s, _)| s.id == "post.uniformity"));

        let probe = repo.list_active_phase(Phase::Probe).unwrap();
        assert_eq!(probe.len(), 3);
    }

    #[test]
    fn responses_roundtrip_with_session() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = ScaleRepo::new(&conn);
        repo.seed_defaults().unwrap();

        let mut session = crate::model::session::Session::new(
            crate::model::session::SessionKind::Practice,
            "2026-11-16T10:00:00.000Z".into(),
            10,
        );
        crate::db::repo::SessionRepo::new(&conn)
            .insert(&session)
            .unwrap();

        // pre-замер: шум 6, открытость 3
        let pre = repo.list_active_phase(Phase::Pre).unwrap();
        let responses: Vec<Response> = pre
            .iter()
            .zip([6.0f64, 3.0f64])
            .map(|((_, v), val)| Response::for_session(session.id, v.id, val))
            .collect();
        repo.save_session_responses(&responses).unwrap();

        // post-замер: 5 шкал
        let post = repo.list_active_phase(Phase::Post).unwrap();
        let values = [7.0, 5.0, 8.0, 6.0, 4.0];
        let post_responses: Vec<Response> = post
            .iter()
            .zip(values)
            .map(|((_, v), val)| Response::for_session(session.id, v.id, val))
            .collect();
        repo.save_session_responses(&post_responses).unwrap();

        let stored = repo.session_responses(&session.id).unwrap();
        assert_eq!(stored.len(), 7, "2 pre + 5 post");
        let get = |sid: &str| -> f64 {
            stored
                .iter()
                .find(|(id, _, _)| id == sid)
                .map(|(_, _, v)| *v)
                .unwrap_or(-1.0)
        };
        assert_eq!(get("pre.noise"), 6.0);
        assert_eq!(get("scale.openness"), 3.0);
        assert_eq!(get("post.uniformity"), 7.0);
        assert_eq!(get("post.dissolution"), 4.0);

        // завершаем сессию — цикл S1 закрыт
        assert!(session.finish("2026-11-16T10:21:00.000Z".into(), 1260));
    }

    #[test]
    fn json_quote_escapes() {
        assert_eq!(quote("abc"), "\"abc\"");
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(quote("много\nстрок"), "\"много\\nстрок\"");
        let parsed: serde_json::Value =
            serde_json::from_str(&anchors_json("низ \"1\"", "верх")).unwrap();
        assert_eq!(parsed["low"], "низ \"1\"");
        assert_eq!(parsed["high"], "верх");
    }
}
