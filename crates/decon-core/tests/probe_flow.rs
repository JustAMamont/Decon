//! Интеграционные тесты S2 (ROADMAP, критерии 2–4).
//!
//! - №2: изменение формулировки → новая версия; старая читаема, тренды
//!   не смешиваются (ряд по version_id);
//! - №3: догоняющий зонд — `delay_minutes` проставляется;
//! - №4: `scale.openness` — один ряд в SQL: pre-замеры и зонды вместе.

use decon_core::db;
use decon_core::db::repo::{FfmqRepo, FfmqResult, ProbeRepo, ScaleRepo, SessionRepo};
use decon_core::model::probe::Probe;
use decon_core::model::scale::{Phase, Response};
use decon_core::model::session::{Session, SessionKind};
use decon_core::model::settings::ProbeSettings;
use decon_core::schedule::{DaySchedule, plan_day};
use uuid::Uuid;

fn seeded_db() -> rusqlite::Connection {
    let conn = db::open_in_memory().expect("open");
    db::migrate(&conn).expect("migrate");
    ScaleRepo::new(&conn).seed_defaults().expect("seed");
    conn
}

/// Критерий №3: полный цикл догоняющего зонда — расписание, показ,
/// ответ задним числом с проставкой задержки.
#[test]
fn catchup_probe_carries_delay_minutes() {
    let conn = seeded_db();

    // Расписание дня из планировщика (детерминизм: сид 42)
    let settings = ProbeSettings::default();
    let plan = plan_day(&settings, 42).expect("plan");
    assert_eq!(plan.minutes.len(), 4);

    // Зонды в БД по меткам дня
    let markers = day_markers_local(&plan, "2026-11-18");
    let probes: Vec<Probe> = markers
        .iter()
        .map(|m| Probe::scheduled(m.clone()))
        .collect();
    let probe_repo = ProbeRepo::new(&conn);
    probe_repo.insert_batch(&probes).expect("insert");

    // Второй зонд «показан» в 11:00 (план ~10:45), ответили в 15:30 —
    // догоняющий, задержка ~275 мин
    let due_list = probe_repo.due("2026-11-18T15:00:00").expect("due");
    assert!(!due_list.is_empty(), "к 15:00 есть незакрытые зонды");

    let mut probe = due_list[0].clone();
    let fired = "2026-11-18T11:00:05";
    assert!(probe.fire(fired.into()));
    probe_repo.update(&probe).expect("update");

    // Догоняющий список содержит показанный
    let catchable = probe_repo.catchable().expect("catchable");
    assert!(catchable.iter().any(|p| p.id == probe.id));

    // Ответ через 4.5 часа: delay_minutes = разница ответ − план
    let planned = &probe.scheduled_at;
    let delay = minutes_between(planned, "2026-11-18T15:30:00");
    assert!(probe.answer("2026-11-18T15:30:00".into(), Some(delay)));
    probe_repo.update(&probe).expect("update");

    // Ответы зондовых шкал пишутся с delay_minutes
    let scales = ScaleRepo::new(&conn);
    let probe_items = scales.list_active_phase(Phase::Probe).expect("probe list");
    assert_eq!(probe_items.len(), 3, "openness + spontaneity + флаг");

    let responses: Vec<Response> = probe_items
        .iter()
        .zip([6.0f64, 4.0, 1.0])
        .map(|((s, v), val)| {
            assert!(Response::is_valid_value(val, s.kind));
            Response::for_probe(probe.id, v.id, val, probe.delay_minutes)
        })
        .collect();
    scales.save_session_responses(&responses).expect("save");

    // Проверка в БД: задержка легла и в зонд, и в ответы
    let stored = probe_repo.get(&probe.id).expect("get").expect("exists");
    assert_eq!(stored.state, decon_core::model::probe::ProbeState::Answered);
    let delay_stored: i64 = stored.delay_minutes.expect("delay проставлен");
    assert!(
        delay_stored > 200,
        "догоняющий зонд: задержка {delay_stored} мин"
    );

    let with_delay: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM responses WHERE probe_id = ?1 AND delay_minutes IS NOT NULL",
            [stored.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(with_delay, 3, "все 3 ответа помечены задержкой");
}

/// Критерий №4 (SQL-уровень): pre-замер и зонд — один scale_id, один ряд.
#[test]
fn openness_single_series_across_pre_and_probe() {
    let conn = seeded_db();
    let scales = ScaleRepo::new(&conn);

    // Pre-замер сессии
    let mut session = Session::new(SessionKind::Practice, "2026-11-18T09:00:00".into(), 9);
    SessionRepo::new(&conn).insert(&session).expect("insert");
    let pre = scales.list_active_phase(Phase::Pre).expect("pre");
    assert_eq!(pre.len(), 2);
    let openness_pre = pre
        .iter()
        .find(|(s, _)| s.id == "scale.openness")
        .expect("openness в pre-фазе");

    let pre_resp = Response::for_session(session.id, openness_pre.1.id, 3.0);
    scales
        .save_session_responses(&[pre_resp])
        .expect("save pre");

    // Зонд днём
    let mut probe = Probe::scheduled("2026-11-18T13:24:00".into());
    probe.fire("2026-11-18T13:24:10".into());
    ProbeRepo::new(&conn)
        .insert_batch(&[probe.clone()])
        .expect("insert");
    let probe_items = scales.list_active_phase(Phase::Probe).expect("probe");
    let openness_probe = probe_items
        .iter()
        .find(|(s, _)| s.id == "scale.openness")
        .expect("openness в probe-фазе");

    // Один и тот же scale_version_id: версия формулировки общая
    assert_eq!(
        openness_pre.1.id, openness_probe.1.id,
        "pre и probe используют одну версию формулировки"
    );

    let probe_resp = Response::for_probe(probe.id, openness_probe.1.id, 7.0, None);
    scales
        .save_session_responses(&[probe_resp])
        .expect("save probe");

    // Аналитический ряд: один scale_id, две точки, оба контекста
    let series = scales.series("scale.openness", 100).expect("series");
    assert_eq!(series.len(), 2, "единый ряд из 2 точек");
    let values: Vec<f64> = series.iter().map(|(_, v, _, _)| *v).collect();
    assert!(values.contains(&3.0) && values.contains(&7.0));

    // Прямой SQL-запрос аналитики: без UNION/JION-склейки, одна таблица
    let (n, n_ctx): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*),
                    COUNT(DISTINCT CASE WHEN r.session_id IS NOT NULL THEN 's'
                                         WHEN r.probe_id IS NOT NULL THEN 'p' END)
             FROM responses r
             JOIN scale_versions sv ON sv.id = r.scale_version_id
             WHERE sv.scale_id = 'scale.openness'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((n, n_ctx), (2, 2), "2 ответа, 2 контекста, 1 scale_id");
}

/// Критерий №2: изменение формулировки → новая версия; старая читаема,
/// тренды не смешиваются.
#[test]
fn wording_change_creates_new_version_trends_not_mixed() {
    let conn = seeded_db();
    let scales = ScaleRepo::new(&conn);

    // v1 из сида
    let v1 = scales
        .latest_version("post.uniformity")
        .expect("v1")
        .expect("exists");
    assert_eq!(v1.version, 1);

    // Ответ на v1
    let mut session = Session::new(SessionKind::Practice, "2026-11-18T10:00:00".into(), 10);
    SessionRepo::new(&conn).insert(&session).expect("insert");
    let r1 = Response::for_session(session.id, v1.id, 5.0);
    scales.save_session_responses(&[r1]).expect("save v1");

    // Формулировка меняется (конструктор): bump через внутренний API —
    // имитируем сидинг новой формулировки: UPDATE определение + новая версия
    conn.execute(
        "UPDATE scales SET anchor_low='всё в одной точке', anchor_high='ровно по полю',
         updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id='post.uniformity'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO scale_versions (id, scale_id, version, question, anchors_json, created_at)
         VALUES (?1,'post.uniformity',2,'Внимание распределено по полю равномерно?',
                 '{\"low\":\"всё в одной точке\",\"high\":\"ровно по полю\"}',
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [Uuid::now_v7().to_string()],
    )
    .unwrap();

    // Старая версия читаема, номер вырос
    let v2 = scales
        .latest_version("post.uniformity")
        .expect("v2")
        .expect("exists");
    assert_eq!(v2.version, 2);
    assert_ne!(v2.id, v1.id, "разные версии — разные формулировки");

    // Ответ на v2
    let r2 = Response::for_session(session.id, v2.id, 8.0);
    scales.save_session_responses(&[r2]).expect("save v2");

    // Тренд по version_id: v1 — одна точка 5.0, v2 — одна точка 8.0,
    // никогда не смешиваются
    let collect_trend = |version_id: Uuid| -> Vec<f64> {
        let mut stmt = conn
            .prepare("SELECT r.value FROM responses r WHERE r.scale_version_id = ?1")
            .unwrap();
        stmt.query_map([version_id.to_string()], |r| r.get::<_, f64>(0))
            .unwrap()
            .filter_map(|v| v.ok())
            .collect()
    };
    let trend1 = collect_trend(v1.id);
    let trend2 = collect_trend(v2.id);
    assert_eq!(trend1, vec![5.0], "тренд v1 — только ответы v1");
    assert_eq!(trend2, vec![8.0], "тренд v2 — только ответы v2");
}

/// FFMQ-результаты пишутся и читаются (SCALES.md §6.1).
#[test]
fn ffqm_results_roundtrip() {
    let conn = seeded_db();
    let repo = FfmqRepo::new(&conn);

    let r = FfmqResult {
        id: Uuid::now_v7(),
        taken_at: "2026-11-18T19:00:00".into(),
        observing: 60.0,
        describing: 65.0,
        acting_aware: 58.0,
        non_reactivity: 44.0,
        nonjudging: 62.0,
        source_file_hash: "b7e2".repeat(16),
    };
    repo.insert(&r).expect("insert");
    let list = repo.list_recent(5).expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].source_file_hash, "b7e2".repeat(16));
}

/// Деактивация шкалы: скрывается из листинга фаз, история читаема.
#[test]
fn deactivation_hides_from_ui_keeps_history() {
    let conn = seeded_db();
    let scales = ScaleRepo::new(&conn);

    let before = scales.list_active_phase(Phase::Post).expect("post");
    assert_eq!(before.len(), 5);

    scales
        .set_active("post.dissolution", false)
        .expect("deactivate");
    let after = scales.list_active_phase(Phase::Post).expect("post");
    assert_eq!(after.len(), 4);
    assert!(!after.iter().any(|(s, _)| s.id == "post.dissolution"));

    // Определение читаемо (история не теряется)
    let def = scales
        .get("post.dissolution")
        .expect("get")
        .expect("exists");
    assert!(!def.active);

    // Реактивация возвращает
    scales
        .set_active("post.dissolution", true)
        .expect("activate");
    assert_eq!(
        scales.list_active_phase(Phase::Post).expect("post").len(),
        5
    );
}

// --- Утилиты ------------------------------------------------------------

/// Метки дня в локальном ISO (как day_markers, но здесь для теста).
fn day_markers_local(schedule: &DaySchedule, date: &str) -> Vec<String> {
    schedule
        .minutes
        .iter()
        .map(|&m| format!("{date}T{:02}:{:02}:00", m / 60, m % 60))
        .collect()
}

/// Разница в минутах между двумя локальными метками `YYYY-MM-DDTHH:MM:SS`.
fn minutes_between(a: &str, b: &str) -> i64 {
    let parse = |s: &str| -> i64 {
        let hh: i64 = s[11..13].parse().unwrap();
        let mm: i64 = s[14..16].parse().unwrap();
        let ss: i64 = s[17..19].parse().unwrap();
        hh * 60 + mm + ss / 60
    };
    parse(b) - parse(a)
}
