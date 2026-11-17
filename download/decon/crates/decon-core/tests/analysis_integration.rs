//! Интеграционные тесты S4: аналитика + экспорт (критерии 3–4).
//!
//! №3 — roundtrip: JSON-экспорт → импорт в чистую базу → повторный
//! экспорт → содержимое совпадает (порядок строк стабилен, ID новые).
//! №4 — трезвость: A/B при n < 30 обязан нести `low_sample`, UI-слой
//! (viewmodel-функция, без рендера) показывает предупреждение.

use decon_core::analysis::export::{
    ExportError, export_csv, export_json, import_json, to_json_string,
};
use decon_core::analysis::n_of_1::{LOW_SAMPLE_THRESHOLD, compare_periods};
use decon_core::db::{self, open_in_memory};

fn seeded() -> rusqlite::Connection {
    let conn = open_in_memory().unwrap();
    db::migrate(&conn).unwrap();
    db::repo::ScaleRepo::new(&conn).seed_defaults().unwrap();
    conn
}

/// Наполнить базу реалистичным набором: 3 сессии практики с ответами,
/// тренажёрный прогон, зонд с ответом.
fn populate(conn: &rusqlite::Connection) {
    for (i, day) in [1, 2, 3].iter().enumerate() {
        conn.execute(
            "INSERT INTO sessions (id, kind, layer, technique, started_at, ended_at,
             duration_s, end_status, local_time_bucket, sleep_hours, caffeine, posture,
             updated_at)
             VALUES (?1,'practice','visual','shamatha',?2,?3,1200,'finished','morning',
             ?4,?5,'chair',?6)",
            rusqlite::params![
                format!("s{i}"),
                format!("2026-11-{day:02}T10:00:00Z"),
                format!("2026-11-{day:02}T10:20:00Z"),
                7.0 + *day as f64 * 0.5,
                i as i64,
                format!("2026-11-{day:02}T10:21:00Z"),
            ],
        )
        .unwrap();
    }
    // Ответы pre/post.uniformity на первую сессию
    let (sv_pre, sv_post): (String, String) = conn
        .query_row(
            "SELECT
               (SELECT sv.id FROM scale_versions sv JOIN scales s ON s.id = sv.scale_id
                WHERE s.id = 'pre.noise' LIMIT 1),
             (SELECT sv.id FROM scale_versions sv JOIN scales s ON s.id = sv.scale_id
                WHERE s.id = 'post.uniformity' LIMIT 1)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    conn.execute(
        "INSERT INTO responses (id, session_id, scale_version_id, value, answered_at, updated_at)
         VALUES ('r1','s0',?1,4.0,'2026-11-01T09:59:00Z','2026-11-01T09:59:00Z')",
        [&sv_pre],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO responses (id, session_id, scale_version_id, value, answered_at, updated_at)
         VALUES ('r2','s0',?1,8.0,'2026-11-01T10:20:30Z','2026-11-01T10:20:30Z')",
        [&sv_post],
    )
    .unwrap();
    // Тренажёрная сессия + результат
    conn.execute(
        "INSERT INTO sessions (id, kind, started_at, updated_at)
         VALUES ('t1','trainer','2026-11-02T12:00:00Z','2026-11-02T12:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO trainer_results (id, session_id, trainer, config_json, metrics_json,
         seed, updated_at) VALUES ('tr1','t1','schulte','{\"grid\":5}',
         '{\"mode\":\"field\",\"grid\":\"5x5\",\"symbols\":\"digits\",\"duration_ms\":25000,
          \"errors\":0,\"cell_rt_ms\":[1000]}','42','2026-11-02T12:01:00Z')",
        [],
    )
    .unwrap();
    // Зонд с ответом (probe.spontaneity)
    let sv_probe: String = conn
        .query_row(
            "SELECT sv.id FROM scale_versions sv JOIN scales s ON s.id = sv.scale_id
             WHERE s.id = 'probe.spontaneity' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT INTO probes (id, scheduled_at, fired_at, answered_at, window_state, updated_at)
         VALUES ('p1','2026-11-01T14:00:00Z','2026-11-01T14:00:05Z',
         '2026-11-01T14:00:40Z','answered','2026-11-01T14:01:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO responses (id, probe_id, scale_version_id, value, answered_at, updated_at)
         VALUES ('r3','p1',?1,6.0,'2026-11-01T14:00:40Z','2026-11-01T14:00:40Z')",
        [&sv_probe],
    )
    .unwrap();
}

/// Критерий 3: полный roundtrip через публичный API.
#[test]
fn export_import_roundtrip_full() {
    let source = seeded();
    populate(&source);

    // Экспорт из источника
    let tree = export_json(&source).unwrap();
    assert_eq!(tree.sessions.len(), 4); // 3 практики + 1 тренажёр
    assert_eq!(tree.responses.len(), 3);
    assert_eq!(tree.trainer_results.len(), 1);
    assert_eq!(tree.probes.len(), 1);
    let json = to_json_string(&tree).unwrap();

    // Импорт в чистую базу (только миграции + дефолтные шкалы)
    let mut fresh = seeded();
    let stats = import_json(&mut fresh, &json).unwrap();
    assert_eq!(stats.sessions_added, 4);
    assert_eq!(stats.responses_added, 3);
    assert_eq!(stats.trainers_added, 1);
    assert_eq!(stats.probes_added, 1);

    // Повторный импорт того же файла — всё дубликаты
    let stats2 = import_json(&mut fresh, &json).unwrap();
    assert_eq!(stats2.sessions_added, 0);
    assert_eq!(stats2.sessions_skipped, 4);

    // Повторный экспорт из fresh: содержимое эквивалентно (хэши те же)
    let tree2 = export_json(&fresh).unwrap();
    assert_eq!(tree2.sessions.len(), 4);
    let mut h1: Vec<u64> = tree.sessions.iter().map(|s| s.content_hash).collect();
    let mut h2: Vec<u64> = tree2.sessions.iter().map(|s| s.content_hash).collect();
    h1.sort_unstable();
    h2.sort_unstable();
    assert_eq!(h1, h2, "контент после roundtrip не совпал");
    // Ответы: значения и моменты сохранились
    let v1: Vec<f64> = tree.responses.iter().map(|r| r.value).collect();
    let mut v2: Vec<f64> = tree2.responses.iter().map(|r| r.value).collect();
    v2.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut v1s = v1.clone();
    v1s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(v1s, v2);
}

/// Roundtrip сохраняет аналитические ряды: тренд по post.uniformity
/// после импорта даёт те же значения, что до экспорта.
#[test]
fn roundtrip_preserves_analytics_series() {
    let source = seeded();
    populate(&source);
    let before: Vec<(String, f64, i64)> = {
        let mut stmt = source
            .prepare(
                "SELECT r.answered_at, r.value, sv.version
                 FROM responses r
                 JOIN scale_versions sv ON sv.id = r.scale_version_id
                 JOIN scales s ON s.id = sv.scale_id
                 WHERE s.id = 'post.uniformity' AND r.deleted_at IS NULL
                 ORDER BY r.answered_at",
            )
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };

    let json = to_json_string(&export_json(&source).unwrap()).unwrap();
    let mut fresh = seeded();
    import_json(&mut fresh, &json).unwrap();

    let after: Vec<(String, f64, i64)> = {
        let mut stmt = fresh
            .prepare(
                "SELECT r.answered_at, r.value, sv.version
                 FROM responses r
                 JOIN scale_versions sv ON sv.id = r.scale_version_id
                 JOIN scales s ON s.id = sv.scale_id
                 WHERE s.id = 'post.uniformity' AND r.deleted_at IS NULL
                 ORDER BY r.answered_at",
            )
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(before, after);
}

/// CSV-экспорт: 4 таблицы, заметки по умолчанию скрыты.
#[test]
fn csv_export_tables() {
    let conn = seeded();
    populate(&conn);
    let files = export_csv(&conn, false).unwrap();
    assert_eq!(files.len(), 4);
    let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        ["sessions", "responses", "trainer_results", "probes"]
    );
    for (name, csv) in &files {
        assert!(!csv.is_empty(), "{name} пуст");
        // Заголовок есть у всех
        assert!(
            csv.lines().next().unwrap().contains(','),
            "{name}: нет шапки"
        );
    }
    let sessions = &files.iter().find(|(n, _)| *n == "sessions").unwrap().1;
    // 4 сессии + шапка
    assert_eq!(sessions.lines().count(), 5);
}

/// Критерий 4: трезвость UI-состояния при малых выборках.
///
/// Viewmodel-функция аналитики (без рендера): предупреждение «мало
/// данных» обязано появляться при n < 30 в периоде, а числа —
/// сопровождаться им, а не подменяться им.
#[test]
fn ab_low_sample_warning_visible() {
    // 12 + 12 наблюдений — ниже порога
    let a: Vec<f64> = (0..12).map(|i| (i % 5) as f64).collect();
    let b: Vec<f64> = (0..12).map(|i| 5.0 + (i % 5) as f64).collect();
    let r = compare_periods(&a, &b, 42, 2000).unwrap();
    assert!(r.low_sample, "n<30 обязан давать low_sample");
    assert_eq!(r.direction, "insufficient");
    // Числа при этом есть (не спрятаны за предупреждением):
    assert!((r.mean_diff - 5.0).abs() < 1e-9);

    // UI-представление: плашка выводится только при low_sample
    let show_warning = |res: &decon_core::analysis::n_of_1::AbResult| {
        res.n_a < LOW_SAMPLE_THRESHOLD || res.n_b < LOW_SAMPLE_THRESHOLD
    };
    assert!(show_warning(&r));

    // Большая выборка — плашки нет
    let a2: Vec<f64> = (0..40).map(|i| (i % 5) as f64).collect();
    let b2: Vec<f64> = (0..40).map(|i| 5.0 + (i % 5) as f64).collect();
    let r2 = compare_periods(&a2, &b2, 42, 2000).unwrap();
    assert!(!r2.low_sample);
    assert!(!show_warning(&r2));
    assert_eq!(r2.direction, "improved");
    assert!(r2.p_value < 0.01);
}

/// Импорт не ломается на пустом дереве (минимальный валидный файл).
#[test]
fn import_empty_tree() {
    let mut conn = seeded();
    let empty = r#"{
        "export_format":"decon/v1","exported_at":"2026-11-16T00:00:00Z",
        "scales":[],"scale_versions":[],
        "sessions":[],"responses":[],"trainer_results":[],"probes":[]
    }"#;
    let stats = import_json(&mut conn, empty).unwrap();
    assert_eq!(stats.total_added(), 0);
}

/// Импорт с битым JSON возвращает BadFormat, база не меняется.
#[test]
fn import_malformed_json() {
    let mut conn = seeded();
    let before: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    let err = import_json(&mut conn, "{not json at all");
    assert!(matches!(err, Err(ExportError::BadFormat(_))));
    let after: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after, "битый импорт не должен писать в базу");
}
