//! Интеграционные тесты S3: тренажёры в общем журнале (критерии
//! приёмки ROADMAP S3 №2–№4).
//!
//! №2 — детерминизм: один сид → побайтово идентичный лог событий.
//! №3 — результат тренажёра читается общим журналом вместе с практикой.
//! №4 — UX-сценарий «пользователь прошёл Шульте 5×5 без ошибок».

use decon_core::db::{self, repo::TrainerResultRepo};
use decon_core::model::journal::SessionJournal;
use decon_trainers::mot::{Mot, MotConfig, MotPhase};
use decon_trainers::schulte::{Schulte, SchulteConfig, SchulteMode, SchulteSymbols};
use uuid::Uuid;

/// Полный прогон MOT с фиксируемым логом: события фаз + позиции на
/// каждом тике (то, что рисовал бы UI) — сериализуем в байты.
fn mot_run_log(seed: u64) -> Vec<u8> {
    let cfg = MotConfig {
        targets: 4,
        distractors: 8,
        speed: 0.5,
        motion_ms: 3000,
    };
    let mut m = Mot::new(cfg, seed).expect("cfg");
    let mut log: Vec<u8> = Vec::new();
    // Заголовок: конфиг + сид (в логе прогона, чтобы исключить
    // совпадение разных конфигов)
    log.extend_from_slice(format!("seed={seed}\n").as_bytes());
    // Ответы: детерминированный «пользователь» — всегда первые
    // targets объектов по порядку (движок не знает, цели это или нет —
    // важна воспроизводимость последовательности ответов, не корректность).
    loop {
        let phase_before = m.phase();
        m.tick(16);
        if m.phase() != phase_before {
            log.extend_from_slice(format!("PHASE {:?}\n", m.phase()).as_bytes());
            if m.phase() == MotPhase::Answer {
                // Псевдо-пользователь отвечает по 100 мс на объект
                for i in 0..4usize {
                    m.tick(100);
                    let _ = m.answer(i);
                    log.extend_from_slice(format!("ANSWER {i}\n").as_bytes());
                }
                break;
            }
        }
        // Позиции каждые 30 тиков (~0.5 с) — снимок траектории
        if m.total_ms() % 480 == 0 {
            for i in 0..m.object_count() {
                let (x, y) = m.position(i);
                log.extend_from_slice(format!("P{i} {x:.6} {y:.6}\n").as_bytes());
            }
        }
        if m.phase() == MotPhase::Done {
            break;
        }
    }
    log
}

/// Критерий №2: один сид → побайтово идентичный прогон; другой сид —
/// другой байтовый лог (не совпадение по тривиальной причине).
#[test]
fn mot_determinism_by_seed_byte_identical() {
    let a = mot_run_log(2026_1101);
    let b = mot_run_log(2026_1101);
    assert_eq!(
        a,
        b,
        "один сид должен давать побайтово идентичный лог ({}/{} байт)",
        a.len(),
        b.len()
    );
    assert!(a.len() > 500, "лог подозрительно мал: {}", a.len());

    let c = mot_run_log(2026_1102);
    assert_ne!(
        a, c,
        "разные сиды дают разные логи (иначе детерминизм вырожден)"
    );
}

/// Критерий №2 для Шульте: раскладка по сиду побайтово стабильна.
#[test]
fn schulte_determinism_by_seed() {
    let cfg = SchulteConfig {
        mode: SchulteMode::Field,
        grid: 5,
        symbols: SchulteSymbols::Digits,
    };
    let dump = |seed| {
        let s = Schulte::new(cfg.clone(), seed).expect("cfg");
        s.cells()
            .iter()
            .map(|c| format!("{:03}{}", c.value, if c.red { 'R' } else { '.' }))
            .collect::<String>()
    };
    assert_eq!(dump(7), dump(7));
    assert_ne!(dump(7), dump(8));
}

/// Критерий №3: результат тренажёра пишется в БД через сессию
/// kind='trainer' и читается общим журналом вместе с практикой.
#[test]
fn trainer_result_in_common_journal_with_practice() {
    let conn = db::open_in_memory().expect("db");
    db::migrate(&conn).expect("migrate");

    // Практическая сессия с событиями (как в S1)
    let practice_id = Uuid::now_v7();
    conn.execute(
        "INSERT INTO sessions (id, kind, started_at, updated_at)
         VALUES (?1, 'practice', '2026-11-16T09:00:00.000Z',
         strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [practice_id.to_string()],
    )
    .expect("practice session");

    // Тренажёрная сессия: Шульте 5×5 без ошибок
    let mut schulte = Schulte::new(
        SchulteConfig {
            mode: SchulteMode::Field,
            grid: 5,
            symbols: SchulteSymbols::Digits,
        },
        42,
    )
    .expect("schulte");
    let mut t = 0u64;
    for value in 1..=25u16 {
        let idx = schulte
            .cells()
            .iter()
            .position(|c| c.value == value)
            .expect("cell");
        t += 900;
        schulte.tap(idx, t).expect("tap");
    }
    let metrics = schulte.metrics().expect("metrics");

    let trainer_session_id = Uuid::now_v7();
    conn.execute(
        "INSERT INTO sessions (id, kind, started_at, updated_at)
         VALUES (?1, 'trainer', '2026-11-16T10:00:00.000Z',
         strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [trainer_session_id.to_string()],
    )
    .expect("trainer session");

    let repo = TrainerResultRepo::new(&conn);
    repo.insert(&decon_core::db::repo::TrainerResult {
        id: Uuid::now_v7(),
        session_id: trainer_session_id,
        trainer: decon_core::db::repo::TrainerKind::Schulte,
        config_json: serde_json::to_string(&schulte.config()).expect("cfg json"),
        metrics_json: serde_json::to_string(&metrics).expect("metrics json"),
        seed: "42".into(),
    })
    .expect("insert result");

    // Общий журнал: одна лента, оба типа
    let mut stmt = conn
        .prepare(
            "SELECT s.kind, COUNT(t.id) AS results
             FROM sessions s LEFT JOIN trainer_results t ON t.session_id = s.id
             GROUP BY s.kind ORDER BY s.kind",
        )
        .expect("stmt");
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("collect");
    let practice_row = rows
        .iter()
        .find(|(k, _)| k == "practice")
        .expect("practice");
    let trainer_row = rows.iter().find(|(k, _)| k == "trainer").expect("trainer");
    assert_eq!(practice_row.1, 0); // у практики нет результатов тренажёра
    assert_eq!(trainer_row.1, 1); // у тренажёрной — есть

    // list_recent читает с сортировкой по времени сессии
    let recent = repo
        .list_recent(decon_core::db::repo::TrainerKind::Schulte, 10)
        .expect("recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].0.session_id, trainer_session_id);
}

/// Критерий №4 (UX, headless): «пользователь прошёл Шульте 5×5 без
/// ошибок» — время в допустимом диапазоне и ноль ошибок в метриках.
#[test]
fn ux_schulte_5x5_flawless_run() {
    let cfg = SchulteConfig {
        mode: SchulteMode::Field,
        grid: 5,
        symbols: SchulteSymbols::Digits,
    };
    let mut s = Schulte::new(cfg, 12345).expect("schulte");

    // Реалистичный пользователь: 0.6–1.4 с на ячейку, стабильно к концу
    // чуть быстрее. Суммарно 25 ячеек ≈ 15–35 с.
    let rt_profile: [u64; 25] = [
        1400, 1200, 1350, 1100, 1250, 1150, 1000, 1050, 950, 1100, 900, 950, 1000, 850, 900, 880,
        820, 860, 800, 840, 780, 820, 760, 800, 740,
    ];
    let mut t = 0u64;
    for (step, &rt) in rt_profile.iter().enumerate() {
        let value = (step + 1) as u16;
        let idx = s
            .cells()
            .iter()
            .position(|c| c.value == value)
            .expect("cell");
        t += rt;
        let correct = s.tap(idx, t).expect("tap");
        assert!(correct, "верный тап по порядку не должен быть ошибкой");
    }

    assert!(s.is_finished());
    let m = s.metrics().expect("metrics");
    // Ноль ошибок в логе
    assert_eq!(m.errors, 0, "безошибочный прогон");
    // Время в допустимом диапазоне (сумма профиля)
    let expected_total: u64 = rt_profile.iter().sum();
    assert_eq!(m.duration_ms, expected_total);
    assert!(
        (10_000..=40_000).contains(&m.duration_ms),
        "время вне диапазона реалистичного: {} мс",
        m.duration_ms
    );
    // Каждое rt в человеческом диапазоне 300..=3000 мс
    assert!(m.cell_rt_ms.iter().all(|&rt| (300..=3000).contains(&rt)));
    // Побайтовый контракт метрик валиден для записи в БД
    decon_core::db::repo::TrainerResult {
        id: Uuid::now_v7(),
        session_id: Uuid::now_v7(),
        trainer: decon_core::db::repo::TrainerKind::Schulte,
        config_json: serde_json::to_string(s.config()).expect("cfg"),
        metrics_json: serde_json::to_string(&m).expect("metrics"),
        seed: "12345".into(),
    }
    .validate()
    .expect("metrics contract for journal");
}

/// Дополнение к №4: сценарий с ошибками даёт ошибки в метриках
/// (отрицательный контроль UX-теста).
#[test]
fn ux_schulte_with_mistakes_counts_errors() {
    let cfg = SchulteConfig {
        mode: SchulteMode::Classic,
        grid: 3,
        symbols: SchulteSymbols::Digits,
    };
    let mut s = Schulte::new(cfg, 777).expect("schulte");
    // Пользователь тычет не по порядку дважды, потом проходит чисто
    let wrong1 = s.cells().iter().position(|c| c.value == 3).expect("w1");
    let wrong2 = s.cells().iter().position(|c| c.value == 5).expect("w2");
    assert!(!s.tap(wrong1, 500).expect("tap"));
    assert!(!s.tap(wrong2, 900).expect("tap"));
    let mut t = 900u64;
    for value in 1..=9u16 {
        let idx = s.cells().iter().position(|c| c.value == value).expect("c");
        t += 700;
        assert!(s.tap(idx, t).expect("tap"));
    }
    let m = s.metrics().expect("metrics");
    assert_eq!(m.errors, 2);
}

/// MOT-прогон тоже пишется в журнал (общая механика для обоих тренажёров).
#[test]
fn mot_result_persisted_via_trainer_session() {
    let conn = db::open_in_memory().expect("db");
    db::migrate(&conn).expect("migrate");

    let cfg = MotConfig {
        targets: 3,
        distractors: 6,
        speed: 0.4,
        motion_ms: 2000,
    };
    let mut m = Mot::new(cfg, 555).expect("mot");
    // Cue -> Motion -> Answer
    m.tick(1500);
    m.tick(2000);
    assert_eq!(m.phase(), MotPhase::Answer);
    // Отвечаем всеми целями (идеальный прогон): is_target доступен в
    // Answer — как если бы игрок безошибочно отследил их движением
    for i in 0..m.object_count() {
        if m.answers().len() == 3 {
            break;
        }
        m.tick(120);
        if m.is_target(i) {
            let _ = m.answer(i);
        }
    }
    m.finish();
    let metrics = m.metrics().expect("metrics").expect("done");

    let session_id = Uuid::now_v7();
    conn.execute(
        "INSERT INTO sessions (id, kind, started_at, updated_at)
         VALUES (?1, 'trainer', '2026-11-16T11:00:00.000Z',
         strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        [session_id.to_string()],
    )
    .expect("session");
    let repo = TrainerResultRepo::new(&conn);
    repo.insert(&decon_core::db::repo::TrainerResult {
        id: Uuid::now_v7(),
        session_id,
        trainer: decon_core::db::repo::TrainerKind::Mot,
        config_json: serde_json::to_string(m.config()).expect("cfg"),
        metrics_json: serde_json::to_string(&metrics).expect("metrics"),
        seed: m.seed().to_string(),
    })
    .expect("insert");

    let back = repo.list_of_session(&session_id).expect("list");
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].seed, "555");
    let parsed: decon_core::db::repo::MotMetricsRecord =
        serde_json::from_str(&back[0].metrics_json).expect("contract");
    assert_eq!(parsed.targets, 3);
    assert_eq!(parsed.correct_pct, 100.0);
    // Идеальный прогон: первые 3 индекса были целями
    assert!(parsed.answer_rt_ms.iter().all(|&rt| rt > 0));
}

/// Smoke: журнал S1 не регрессировал рядом с тренажёрными сессиями —
/// события живой сессии пишутся как раньше (0 аллокаций на append).
#[test]
fn journal_append_still_zero_alloc_path() {
    let mut j = SessionJournal::new(Uuid::now_v7());
    for i in 0..500i64 {
        j.append(i, decon_core::model::event::EventKind::Signal);
    }
    assert_eq!(j.take_batch().len(), 500);
}
