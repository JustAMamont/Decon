//! Интеграционный тест S1: полный цикл сессии практики.
//!
//! Критерии приёмки ROADMAP S1:
//! - №1: create → pre-замеры → события → post-замеры → read с roundtrip
//!   всех полей, включая зашифрованную заметку;
//! - №4: зашифрованная заметка не находится в файле БД поиском по
//!   подстроке;
//! - №5: ноль конфиг-файлов — после цикла в каталоге только файл БД.

use decon_core::db;
use decon_core::db::crypto::{derive_key_passphrase, FieldCryptor};
use decon_core::db::repo::{EventRepo, SessionRepo, SettingsRepo};
use decon_core::model::event::EventKind;
use decon_core::model::journal::SessionJournal;
use decon_core::model::session::{Layer, Session, SessionKind, SessionContext};
use decon_core::model::settings::Language;
use decon_core::model::AppSettings;
use std::io::Read;

#[test]
fn full_session_lifecycle_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("journal.db");

    // --- Подключение и миграции (как при первом запуске приложения)
    let conn = db::open(&db_path).expect("open");
    db::migrate(&conn).expect("migrate");

    // --- Ключ: режим passphrase (argon2id), как в SPEC §7.2
    let salt = "c2FsdC1zYWx0LXNhbHQtc2FsdA==";
    let key = derive_key_passphrase("фраза владельца практики", salt).expect("key");
    let cryptor = FieldCryptor::new(&key).expect("cryptor");

    // --- Create: запуск сессии с контекстом
    let started = "2026-11-16T10:15:00.000Z".to_string();
    let mut session = Session::new(SessionKind::Practice, started.clone(), 10);
    session.set_layer(Some(Layer::Visual));
    session.set_technique(Some("дКВ: визуальная, версия 1".into()));
    session.context = SessionContext {
        sleep_hours: Some(7.5),
        caffeine: Some(1),
        posture: Some("сидя, кресло".into()),
    };
    let session_id = session.id;

    let session_repo = SessionRepo::new(&conn).with_cryptor(&cryptor);
    session_repo.insert(&session).expect("insert session");

    // --- Events: буфер журнала + flush (события с таймкодами)
    let event_repo = EventRepo::new(&conn).with_cryptor(&cryptor);
    let mut journal = SessionJournal::new(session_id);
    assert!(journal.append(0, EventKind::Signal));
    assert!(journal.append(45_000, EventKind::CollapseFocus));
    assert!(journal.append(61_500, EventKind::DriftSemantic));
    assert!(journal.append(122_000, EventKind::ClarityFlash));
    assert!(journal.append(300_000, EventKind::Signal));
    let flushed = journal.flush(&event_repo).expect("flush events");
    assert_eq!(flushed, 5);

    // --- Post: завершение + заметка (🔒 поле)
    let note_text = "После 2-й минуты — вспышка равномерности, фон стал цельным. \
                     Личная деталь для проверки шифрования: уникальный маркер Zq71#кси.";
    session.note = Some(note_text.to_string());
    assert!(session.finish("2026-11-16T10:36:00.000Z".into(), 1260));
    session_repo.update(&session).expect("update session");

    // --- Read: полный roundtrip
    let loaded = session_repo
        .get(&session_id)
        .expect("get")
        .expect("session exists");
    assert_eq!(loaded.id, session_id);
    assert_eq!(loaded.kind(), SessionKind::Practice);
    assert_eq!(loaded.layer(), Some(Layer::Visual));
    assert_eq!(loaded.technique(), Some("дКВ: визуальная, версия 1"));
    assert_eq!(loaded.started_at(), started);
    assert_eq!(loaded.ended_at(), Some("2026-11-16T10:36:00.000Z"));
    assert_eq!(loaded.duration_s(), Some(1260));
    assert_eq!(loaded.context.sleep_hours, Some(7.5));
    assert_eq!(loaded.context.caffeine, Some(1));
    assert_eq!(loaded.context.posture.as_deref(), Some("сидя, кресло"));
    assert_eq!(loaded.note.as_deref(), Some(note_text), "заметка roundtrip");

    let events = event_repo
        .list_of_session(&session_id)
        .expect("list events");
    assert_eq!(events.len(), 5);
    assert_eq!(events[1].t_offset_ms, 45_000);
    assert_eq!(events[1].kind, EventKind::CollapseFocus);
    assert_eq!(events[3].kind, EventKind::ClarityFlash);

    // --- Критерий №4: заметки нет в файле БД (поиск по подстроке)
    // WAL мог ещё не флашнуться в основной файл — закрываем чисто.
    drop(event_repo);
    drop(session_repo);
    conn.close().expect("close db");
    let mut db_bytes = Vec::new();
    std::fs::File::open(&db_path)
        .expect("db file")
        .read_to_end(&mut db_bytes)
        .expect("read db");
    let db_str = String::from_utf8_lossy(&db_bytes);
    assert!(
        !db_str.contains(note_text),
        "открытый текст заметки не должен найтись в файле БД"
    );
    assert!(!db_str.contains("Zq71"), "маркер заметки не найден");
    // Но что-то записано: fernet-токен (base64, начинается с 'gAAAA')
    assert!(db_str.contains("gAAAA"), "fernet-токен присутствует");

    // --- Критерий №5: ноль конфиг-файлов — в каталоге только БД (+ WAL/SHM)
    let files: Vec<String> = std::fs::read_dir(tmp.path())
        .expect("read_dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let unexpected: Vec<&String> = files
        .iter()
        .filter(|f| {
            !f.ends_with(".db")
                && !f.ends_with(".db-wal")
                && !f.ends_with(".db-shm")
                && f.as_str() != "."
        })
        .collect();
    assert!(
        unexpected.is_empty(),
        "никаких конфигов кроме БД: {unexpected:?} (файлы: {files:?})"
    );

    // --- Повторное открытие той же базы: миграция идемпотентна, данные на месте
    let conn2 = db::open(&db_path).expect("reopen");
    db::migrate(&conn2).expect("migrate again");
    let repo2 = SessionRepo::new(&conn2).with_cryptor(&cryptor);
    let again = repo2
        .get(&session_id)
        .expect("get again")
        .expect("still there");
    assert_eq!(again.note.as_deref(), Some(note_text));
}

#[test]
fn wrong_passphrase_cannot_decrypt_but_data_intact() {
    // SPEC §7.4: ошибка расшифровки — типизированная, запись цела.
    let conn = db::open_in_memory().expect("open");
    db::migrate(&conn).expect("migrate");

    let good = FieldCryptor::new(&derive_key_passphrase("правильная фраза", "c2FsdC1zYWx0LXNhbHQtc2FsdA==").unwrap())
        .unwrap();
    let bad = FieldCryptor::new(&derive_key_passphrase("неправильная фраза", "c2FsdC1zYWx0LXNhbHQtc2FsdA==").unwrap())
        .unwrap();

    let mut s = Session::new(SessionKind::Practice, "2026-11-16T12:00:00.000Z".into(), 12);
    s.note = Some("секретная заметка".into());
    let sid = s.id;
    SessionRepo::new(&conn).with_cryptor(&good).insert(&s).unwrap();

    // Правильный ключ читает
    let ok = SessionRepo::new(&conn).with_cryptor(&good).get(&sid).unwrap().unwrap();
    assert_eq!(ok.note.as_deref(), Some("секретная заметка"));

    // Неправильный ключ — ошибка, не тихая потеря
    let err = SessionRepo::new(&conn).with_cryptor(&bad).get(&sid);
    assert!(err.is_err(), "расшифровка чужим ключом должна упасть");

    // Строка в БД не пострадала: правильный ключ всё ещё читает её
    let still = SessionRepo::new(&conn).with_cryptor(&good).get(&sid).unwrap().unwrap();
    assert_eq!(still.note.as_deref(), Some("секретная заметка"));
}

#[test]
fn settings_survive_reopen_and_defaults_on_first_run() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("settings.db");

    let conn = db::open(&db_path).expect("open");
    db::migrate(&conn).expect("migrate");
    let repo = SettingsRepo::new(&conn);

    // Первый запуск — дефолты
    assert_eq!(repo.load().unwrap(), AppSettings::default());

    // Меняем и сохраняем
    let mut s = AppSettings::default();
    s.probe.enabled = true;
    s.probe.count = 5;
    s.probe.min_gap_minutes = 45;
    s.session.signal_interval_s = 240;
    s.language = Language::En;
    repo.save(&s).unwrap();
    conn.close().unwrap();

    // Переоткрытие: значения сохранились
    let conn2 = db::open(&db_path).expect("reopen");
    db::migrate(&conn2).unwrap();
    let back = SettingsRepo::new(&conn2).load().unwrap();
    assert_eq!(back, s);
    assert_eq!(back.probe.count, 5);
    assert_eq!(back.session.signal_interval_s, 240);
}

#[test]
fn session_history_orders_by_start_desc() {
    let conn = db::open_in_memory().expect("open");
    db::migrate(&conn).expect("migrate");
    let repo = SessionRepo::new(&conn);

    for (h, min) in [(10, 0), (9, 30), (11, 15)] {
        let mut s = Session::new(
            SessionKind::Practice,
            format!("2026-11-16T{h:02}:{min:02}:00.000Z"),
            h,
        );
        s.finish(format!("2026-11-16T{h:02}:{min:02}:30.000Z"), 30);
        repo.insert(&s).unwrap();
    }
    let list = repo.list_recent(10).unwrap();
    assert_eq!(list.len(), 3);
    assert!(list[0].started_at() > list[1].started_at());
    assert!(list[1].started_at() > list[2].started_at());
    assert_eq!(list[0].started_at(), "2026-11-16T11:15:00.000Z");

    // Soft-delete скрывает из истории, но get всё ещё находит
    let mut victim = list[2].clone();
    victim.soft_delete("2026-11-16T12:00:00.000Z".into());
    repo.update(&victim).unwrap();
    let after = repo.list_recent(10).unwrap();
    assert_eq!(after.len(), 2, "удалённая скрыта из истории");
    assert!(repo.get(&victim.id).unwrap().is_some(), "но физически жива");
}
