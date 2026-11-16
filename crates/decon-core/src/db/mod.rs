//! Хранилище: SQLite-подключение, миграции, репозитории, шифрование.
//!
//! Прагмы и правила миграций — SPEC-data-model.md §6: WAL, foreign_keys,
//! synchronous=NORMAL; down-миграции только для разработки.

pub mod crypto;
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
        assert_eq!(again, 1);
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap(); // повторный вызов — no-op
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
