//! Хранилище: SQLite-подключение, миграции, репозитории, шифрование.
//!
//! Прагмы и правила миграций — SPEC-data-model.md §6: WAL, foreign_keys,
//! synchronous=NORMAL; down-миграции только для разработки.

pub mod crypto;
pub mod keyring;
pub mod migrations;
pub mod repo;

use thiserror::Error;

/// Ошибки слоя хранилища.
#[derive(Debug, Error)]
pub enum DbError {
    /// Ошибка SQLite.
    #[error("sqlite: {0}")]
    Sqlite(String),
    /// Ошибка шифрования/расшифровки поля.
    #[error("crypto: {0}")]
    Crypto(String),
    /// Ошибка парсинга значения из БД.
    #[error("parse: {0}")]
    Parse(String),
}

/// Открыть подключение к базе с прагмами SPEC §6: WAL, foreign_keys,
/// synchronous=NORMAL, busy_timeout.
pub fn open(path: &std::path::Path) -> Result<rusqlite::Connection, DbError> {
    let conn = rusqlite::Connection::open(path).map_err(|e| DbError::Sqlite(e.to_string()))?;
    configure(&conn)?;
    Ok(conn)
}

/// Открыть временную in-memory базу (тесты).
pub fn open_in_memory() -> Result<rusqlite::Connection, DbError> {
    let conn =
        rusqlite::Connection::open_in_memory().map_err(|e| DbError::Sqlite(e.to_string()))?;
    configure(&conn)?;
    Ok(conn)
}

/// Применить стандартные прагмы к подключению.
fn configure(conn: &rusqlite::Connection) -> Result<(), DbError> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
    Ok(())
}

/// Применить все неприменённые миграции (транзакция на миграцию).
pub fn migrate(conn: &rusqlite::Connection) -> Result<(), DbError> {
    for m in migrations::MIGRATIONS {
        // На чистой базе таблицы учёта ещё нет — это значит «не применена».
        let tracked = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'
                 AND name='schema_version'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?
            > 0;
        let applied = if !tracked {
            false
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM schema_version WHERE version = ?1",
                [m.version],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?
                > 0
        };
        if applied {
            continue;
        }
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        tx.execute_batch(m.up_sql)
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        tx.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![m.version, crate::model::session::now_iso()],
        )
        .map_err(|e| DbError::Sqlite(e.to_string()))?;
        tx.commit().map_err(|e| DbError::Sqlite(e.to_string()))?;
    }
    Ok(())
}

/// Откатить миграцию (только разработка/тесты).
pub fn migrate_down(conn: &rusqlite::Connection, to_version: i64) -> Result<(), DbError> {
    for m in migrations::MIGRATIONS.iter().rev() {
        if m.version <= to_version {
            break;
        }
        // Сначала убрать факт применения (down_sql может дропнуть
        // schema_version — 0001 так и делает), потом сам down.
        conn.execute("DELETE FROM schema_version WHERE version = ?1", [m.version])
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        conn.execute_batch(m.down_sql)
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_up_down_up_filled_database() {
        // Критерий приёмки S1 №2: up → down → up на заполненной базе.
        let conn = open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, migrations::MIGRATIONS.len() as i64);

        // Данные: сессия + событие
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at) VALUES
             ('018f0000-0000-7000-8000-000000000001','practice','2026-11-16T10:00:00.000Z','2026-11-16T10:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_events (id, session_id, t_offset_ms, kind) VALUES
             ('018f0000-0000-7000-8000-000000000002',
              '018f0000-0000-7000-8000-000000000001', 1500, 'clarity_flash')",
            [],
        )
        .unwrap();

        migrate_down(&conn, 0).unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "после down таблиц не осталось");

        // Восстановление схемы не падает на грязном состоянии
        migrate(&conn).unwrap();
        let again: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(again, migrations::MIGRATIONS.len() as i64);
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // повторный вызов — no-op
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, migrations::MIGRATIONS.len() as i64);
    }

    /// S2: миграция 0002 унифицирует openness — ответы зондов и pre-замеры
    /// ссылаются на одну шкалу, дубль формулировки исчезает.
    #[test]
    fn migration_0002_unifies_openness_responses() {
        let conn = open_in_memory().unwrap();
        // Схема v1 без миграции данных
        conn.execute_batch(migrations::MIGRATIONS[0].up_sql)
            .unwrap();

        // Данные старой формы: pre.openness в сессии, probe.openness в зондах
        conn.execute(
            "INSERT INTO scales (id, title, anchor_low, anchor_high, kind, active, updated_at)
             VALUES ('pre.openness','Открытость поля','низ','верх','slider',1,'2026-11-16T10:00:00.000Z'),
                    ('probe.openness','Открытость поля','низ','верх','slider',1,'2026-11-16T10:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scale_versions (id, scale_id, version, question, anchors_json, created_at)
             VALUES ('018f0000-0000-7000-8000-0000000000a1','pre.openness',1,'q?','{}','2026-11-16T10:00:00.000Z'),
                    ('018f0000-0000-7000-8000-0000000000a2','probe.openness',1,'q?','{}','2026-11-16T10:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at)
             VALUES ('018f0000-0000-7000-8000-0000000000s1','practice','2026-11-16T09:00:00.000Z','2026-11-16T09:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO probes (id, scheduled_at, window_state, updated_at)
             VALUES ('018f0000-0000-7000-8000-0000000000p1','2026-11-16T12:00:00.000Z','fired','2026-11-16T12:00:00.000Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO responses (id, session_id, probe_id, scale_version_id, value, answered_at, updated_at)
             VALUES
              ('018f0000-0000-7000-8000-0000000000r1','018f0000-0000-7000-8000-0000000000s1',NULL,'018f0000-0000-7000-8000-0000000000a1',4.0,'2026-11-16T09:01:00.000Z','2026-11-16T09:01:00.000Z'),
              ('018f0000-0000-7000-8000-0000000000r2',NULL,'018f0000-0000-7000-8000-0000000000p1','018f0000-0000-7000-8000-0000000000a2',7.5,'2026-11-16T12:01:00.000Z','2026-11-16T12:01:00.000Z')",
            [],
        )
        .unwrap();

        // Миграция 0002
        conn.execute_batch(migrations::MIGRATIONS[1].up_sql)
            .unwrap();

        // Оба ответа указывают на одну версию scale.openness
        let (n_unified, v1): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), MAX(r.scale_version_id) FROM responses r
                 JOIN scale_versions sv ON sv.id = r.scale_version_id
                 WHERE sv.scale_id = 'scale.openness'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(n_unified, 2, "оба ответа — на одной шкале");
        assert_eq!(
            v1, "018f0000-0000-7000-8000-0000000000a1",
            "версия pre.openness сохранена (тот же UUID)"
        );

        // Дубль удалён, определение soft-deleted
        let n_probe_versions: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scale_versions WHERE scale_id='probe.openness'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_probe_versions, 0);
        let probe_active: i64 = conn
            .query_row(
                "SELECT active FROM scales WHERE id='probe.openness'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(probe_active, 0, "probe.openness деактивирован");

        // Down: ответы зондов возвращаются на воссозданную probe.openness
        conn.execute_batch(migrations::MIGRATIONS[1].down_sql)
            .unwrap();
        let (n_pre, n_probe): (i64, i64) = conn
            .query_row(
                "SELECT
                   SUM(CASE WHEN sv.scale_id='pre.openness' THEN 1 ELSE 0 END),
                   SUM(CASE WHEN sv.scale_id='probe.openness' THEN 1 ELSE 0 END)
                 FROM responses r JOIN scale_versions sv ON sv.id = r.scale_version_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((n_pre, n_probe), (1, 1), "down вернул раздельные шкалы");
    }
}
