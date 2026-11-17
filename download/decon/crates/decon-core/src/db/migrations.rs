//! Версионированные миграции схемы.
//!
//! Правила (SPEC-data-model.md §6): файл `NNNN_name.sql` (up) +
//! `NNNN_name.down.sql` (down), применение — в транзакции, факт — в
//! `schema_version`. Down используется только в разработке и тестах.
//!
//! Новая миграция = новая пара файлов + запись в `MIGRATIONS` + обновление
//! SPEC в том же PR.

/// Одна миграция: номер, имя, SQL up и down (вкомпилированы в бинарник).
pub struct Migration {
    /// Номер версии схемы.
    pub version: i64,
    /// Короткое имя (для логов).
    pub name: &'static str,
    /// SQL применения.
    pub up_sql: &'static str,
    /// SQL отката (только разработка/тесты).
    pub down_sql: &'static str,
}

/// Все миграции по порядку. Новые — только в конец.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "0001_init",
        up_sql: include_str!("migrations/0001_init.sql"),
        down_sql: include_str!("migrations/0001_init.down.sql"),
    },
    Migration {
        version: 2,
        name: "0002_unify_openness_scale",
        up_sql: include_str!("migrations/0002_unify_openness_scale.sql"),
        down_sql: include_str!("migrations/0002_unify_openness_scale.down.sql"),
    },
    Migration {
        version: 3,
        name: "0003_residency_shutdown",
        up_sql: include_str!("migrations/0003_residency_shutdown.sql"),
        down_sql: include_str!("migrations/0003_residency_shutdown.down.sql"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrations_are_sequential() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, (i + 1) as i64, "номер версии должен идти 1,2,3…");
            assert!(!m.up_sql.is_empty());
            assert!(!m.down_sql.is_empty());
        }
    }

    #[test]
    fn up_down_up_on_empty_database() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        for m in MIGRATIONS {
            conn.execute_batch(m.up_sql).expect("up");
        }
        assert_eq!(tables(&conn), expected_tables());
        for m in MIGRATIONS.iter().rev() {
            conn.execute_batch(m.down_sql).expect("down");
        }
        assert_eq!(tables(&conn), Vec::<String>::new());
        for m in MIGRATIONS {
            conn.execute_batch(m.up_sql).expect("up again");
        }
        assert_eq!(tables(&conn), expected_tables());
    }

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query");
        rows.filter_map(|r| r.ok()).collect()
    }

    fn expected_tables() -> Vec<String> {
        vec![
            "ffqm_results".into(),
            "probes".into(),
            "responses".into(),
            "scale_versions".into(),
            "scales".into(),
            "schema_version".into(),
            "session_events".into(),
            "sessions".into(),
            "settings".into(),
            "trainer_results".into(),
        ]
    }
}
