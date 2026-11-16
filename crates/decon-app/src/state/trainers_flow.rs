//! Flow тренажёров (S3): старт прогона, тик MOT, завершение и запись
//! результата в общий журнал.
//!
//! Запись — критерий приёмки №3: сессия `kind='trainer'` в `sessions`
//! + строка `trainer_results`; читается тем же журналом, что и практика.
//!
//! Сид прогона выводится из системного времени и сохраняется в БД,
//! что даёт воспроизводимость каждого прогона.

use decon_core::db::repo::{SessionRepo, TrainerKind, TrainerResult, TrainerResultRepo};
use decon_core::model::session::{Session, SessionKind, now_iso};
use decon_trainers::mot::{Mot, MotConfig};
use decon_trainers::schulte::{Schulte, SchulteConfig};
use uuid::Uuid;

use crate::state::app::{ActiveTrainer, App, TrainerEngine};
use crate::state::clock;

/// Начать прогон Шульте: сид из времени (наносекунды), сессия создаётся
/// сразу (kind='trainer'), результат допишется в `finish_trainer`.
pub fn start_schulte(app: &mut App, config: SchulteConfig) -> Result<(), String> {
    let seed = clock::unix_now_ms() as u64 ^ std::process::id() as u64;
    let engine = Schulte::new(config, seed).map_err(|e| e.to_string())?;
    app.trainer_seed = seed;
    app.trainer_summary = None;
    app.active_trainer = Some(ActiveTrainer {
        engine: TrainerEngine::Schulte(engine),
        started: std::time::Instant::now(),
        last_tick: std::time::Instant::now(),
    });
    app.screen = crate::state::app::Screen::TrainerRun;
    Ok(())
}

/// Начать прогон MOT.
pub fn start_mot(app: &mut App, config: MotConfig) -> Result<(), String> {
    let seed = clock::unix_now_ms() as u64 ^ std::process::id() as u64;
    let engine = Mot::new(config, seed).map_err(|e| e.to_string())?;
    app.trainer_seed = seed;
    app.trainer_summary = None;
    app.active_trainer = Some(ActiveTrainer {
        engine: TrainerEngine::Mot(Box::new(engine)),
        started: std::time::Instant::now(),
        last_tick: std::time::Instant::now(),
    });
    app.screen = crate::state::app::Screen::TrainerRun;
    Ok(())
}

/// Тик MOT (вызывается рендер-циклом экрана): передаёт движку реальный
/// dt между кадрами. Для Шульте — no-op (время считает tap).
pub fn tick_trainer(app: &mut App) {
    let Some(active) = app.active_trainer.as_mut() else {
        return;
    };
    if let TrainerEngine::Mot(mot) = &mut active.engine {
        let now = std::time::Instant::now();
        let dt = now.duration_since(active.last_tick).as_millis() as u32;
        active.last_tick = now;
        mot.tick(dt.clamp(1, 100)); // защита от лагов рендера
    }
}

/// Завершить прогон и записать результат в общий журнал.
///
/// Сессия `kind='trainer'` получает `started_at` = старту прогона и
/// `end_status='finished'` (прервать тренажёр нельзя — это измерение,
/// а не практика; закрытие окна посреди прогона = незаписанный прогон,
/// честнее, чем мусорная строка).
pub fn finish_trainer(app: &mut App) -> Result<(), String> {
    let Some(active) = app.active_trainer.take() else {
        return Ok(());
    };
    // Двигок по владению: MOT при незавершённой фазе финализируем
    // на месте (finish) — метрики тогда честно посчитают неответы.
    let (trainer, config_json, metrics_json) = match active.engine {
        TrainerEngine::Schulte(s) => {
            let m = s.metrics().map_err(|e| e.to_string())?;
            let config = serde_json::to_string(s.config()).map_err(|e| e.to_string())?;
            let metrics = serde_json::to_string(&m).map_err(|e| e.to_string())?;
            (TrainerKind::Schulte, config, metrics)
        }
        TrainerEngine::Mot(mut m) => {
            m.finish();
            let Some(mm) = m.metrics().map_err(|e| e.to_string())? else {
                return Err("mot: no metrics after finish".into());
            };
            let config = serde_json::to_string(m.config()).map_err(|e| e.to_string())?;
            let metrics = serde_json::to_string(&mm).map_err(|e| e.to_string())?;
            (TrainerKind::Mot, config, metrics)
        }
    };

    // Сессия тренажёра в общем журнале
    let session = Session::new(
        SessionKind::Trainer,
        now_iso(),
        clock::local_now_compact()[11..13].parse().unwrap_or(12),
    );
    SessionRepo::new(&app.conn)
        .insert(&session)
        .map_err(|e| e.to_string())?;

    let result = TrainerResult {
        id: Uuid::now_v7(),
        session_id: session.id,
        trainer,
        config_json,
        metrics_json,
        seed: app.trainer_seed.to_string(),
    };
    TrainerResultRepo::new(&app.conn)
        .insert(&result)
        .map_err(|e| e.to_string())?;

    // Краткое резюме для тоста на экране списка
    app.trainer_summary = Some(summary_text(&result));
    app.history = app
        .history
        .iter()
        .cloned()
        .chain(std::iter::once(session))
        .collect();
    app.screen = crate::state::app::Screen::Trainers;
    Ok(())
}

/// Текст-резюме прогона (локализация экраном; здесь — только данные).
fn summary_text(result: &TrainerResult) -> String {
    let value: serde_json::Value =
        serde_json::from_str(&result.metrics_json).unwrap_or(serde_json::Value::Null);
    match result.trainer {
        TrainerKind::Schulte => {
            let dur = value
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let errors = value.get("errors").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("schulte: {} мс, ошибок {}", dur, errors)
        }
        TrainerKind::Mot => {
            let pct = value
                .get("correct_pct")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            format!("mot: {:.0}%", pct)
        }
        _ => result.trainer.as_code().to_string(),
    }
}
