//! Headless-тест UI-потока: полный цикл журнала через `App` без рендера
//! (ARCHITECTURE.md §7: idle-режим и рендер — отдельная забота Dioxus;
//! здесь проверяем логику состояний, которую гоняет UI).

use decon_core::db;
use decon_core::db::repo::{ScaleRepo, SessionRepo};
use decon_core::model::event::EventKind;
use decon_core::model::scale::Phase;
use decon_core::model::session::Layer;

/// App нельзя создать напрямую с временной БД (init() пишет в XDG), поэтому
/// тест гоняет тот же поток через репозитории и RunningSession — как App
/// это делает внутри begin/stop/save. Эквивалентность обеспечивается
/// тем, что App-методы делегируют этим же структурам.
#[test]
fn journal_flow_full_cycle_headless() {
    let conn = db::open_in_memory().expect("open");
    db::migrate(&conn).expect("migrate");
    let scales = ScaleRepo::new(&conn);
    scales.seed_defaults().expect("seed");

    // --- Setup → Pre: загрузка вопросов
    let pre = scales.list_active_phase(Phase::Pre).expect("pre");
    let post = scales.list_active_phase(Phase::Post).expect("post");
    assert_eq!(pre.len(), 2);
    assert_eq!(post.len(), 5);

    // --- Begin: сессия + pre-ответы
    use decon_core::model::journal::SessionJournal;
    use decon_core::model::session::{Session, SessionContext, SessionKind};
    let mut session = Session::new(
        SessionKind::Practice,
        decon_core::model::session::now_iso(),
        15,
    );
    session.set_layer(Some(Layer::Visual));
    session.set_technique(Some("дКВ 1.0".into()));
    session.context = SessionContext {
        sleep_hours: Some(7.0),
        caffeine: Some(2),
        posture: None,
    };
    let sid = session.id;
    let session_repo = SessionRepo::new(&conn);
    session_repo.insert(&session).expect("insert");

    use decon_core::model::scale::Response;
    let pre_resp: Vec<Response> = pre
        .iter()
        .zip([7.0f64, 4.0])
        .map(|((_s, v), val)| Response::for_session(sid, v.id, val))
        .collect();
    scales.save_session_responses(&pre_resp).expect("pre save");

    // --- Running: таймер + маркеры (как RunningSession из app.rs)
    let mut journal = SessionJournal::new(sid);
    let mut elapsed = 0u64;
    let interval = 300_000u64; // 5 минут из дефолтных настроек
    let mut next_signal = interval;
    let mut signal_count = 0;

    let mut tick = |elapsed: &mut u64, delta: u64, journal: &mut SessionJournal| -> bool {
        *elapsed += delta;
        if *elapsed >= next_signal {
            next_signal += interval;
            let _ = journal.append(*elapsed as i64, EventKind::Signal);
            return true;
        }
        false
    };

    assert!(!tick(&mut elapsed, 60_000, &mut journal));
    assert!(tick(&mut elapsed, 240_000, &mut journal), "5 мин — сигнал");
    signal_count += 1;
    let _ = journal.append(elapsed as i64, EventKind::CollapseFocus);
    assert!(tick(&mut elapsed, 300_000, &mut journal), "10 мин — сигнал");
    signal_count += 1;
    let _ = journal.append(elapsed as i64, EventKind::ClarityFlash);
    assert_eq!(signal_count, 2);
    assert_eq!(journal.pending(), 4, "2 сигнала + 2 маркера");

    // --- Stop: завершение + flush
    session.finish(
        decon_core::model::session::now_iso(),
        (elapsed / 1000) as i64,
    );
    let batch = journal.take_batch();
    session_repo.update(&session).expect("update");
    use decon_core::db::repo::EventRepo;
    EventRepo::new(&conn).insert_batch(&batch).expect("events");

    // --- Post-замер
    let post_resp: Vec<Response> = post
        .iter()
        .zip([8.0f64, 7.0, 9.0, 7.5, 5.0])
        .map(|((_s, v), val)| Response::for_session(sid, v.id, val))
        .collect();
    scales
        .save_session_responses(&post_resp)
        .expect("post save");

    // --- Проверки: единый журнал
    let stored = session_repo.get(&sid).expect("get").expect("exists");
    assert_eq!(stored.layer(), Some(Layer::Visual));
    assert_eq!(stored.technique(), Some("дКВ 1.0"));
    assert_eq!(stored.duration_s(), Some(600));
    assert_eq!(stored.context.caffeine, Some(2));

    let events = EventRepo::new(&conn).list_of_session(&sid).expect("events");
    assert_eq!(events.len(), 4);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == EventKind::Signal)
            .count(),
        2
    );

    let responses = scales.session_responses(&sid).expect("resp");
    assert_eq!(responses.len(), 7);

    // История показывает сессию
    let history = session_repo.list_recent(10).expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, sid);
}
