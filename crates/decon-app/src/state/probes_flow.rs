//! Поток зондов дня и периодических инструментов (S2): расписание дня,
//! показ зонда, догоняющие ответы, FFMQ, слойная карта, настройки.
//!
//! Функции вынесены из `app.rs` (правило №7.2: файл ≤ ~400 строк);
//! состояние остаётся единым — свободные функции над `&mut App`.

use crate::i18n::{I18n, Str};
use crate::state::app::{App, ScaleAnswer, Screen};
use crate::state::clock::{catchup_delay_minutes, date_seed, local_date_today, local_now_compact};
use decon_core::db::repo::{FfmqRepo, FfmqResult, ProbeRepo, ScaleRepo, SessionRepo, SettingsRepo};
use decon_core::model::probe::{Probe, ProbeState};
use decon_core::model::scale::{Phase, Response};
use decon_core::model::session::{Layer, Session, SessionKind, now_iso};
use decon_core::model::settings::Language;
use decon_core::schedule::plan_day;
use uuid::Uuid;

/// Перестроить план и записи зондов на сегодня (при включённых зондах).
///
/// Идемпотентно: существующие записи дня не дублируются — строим план
/// детерминированно (день → сид) и вставляем только недостающие.
pub fn rebuild_today_probes(app: &mut App) -> Result<(), String> {
    let today = local_date_today();
    let seed = date_seed(&today);
    let plan = plan_day(&app.settings.probe, seed).map_err(|e| e.to_string())?;
    app.today_plan = Some(plan.clone());

    let repo = ProbeRepo::new(&app.conn);
    let existing = repo.list_day(&today).map_err(|e| e.to_string())?;
    let existing_at: std::collections::HashSet<String> =
        existing.iter().map(|p| p.scheduled_at.clone()).collect();

    let fresh: Vec<Probe> = plan
        .minutes
        .iter()
        .filter_map(|&m| {
            let at = format!("{today}T{:02}:{:02}:00", m / 60, m % 60);
            if existing_at.contains(&at) {
                None
            } else {
                Some(Probe::scheduled(at))
            }
        })
        .collect();
    if !fresh.is_empty() {
        repo.insert_batch(&fresh).map_err(|e| e.to_string())?;
    }
    app.today_probes = repo.list_day(&today).map_err(|e| e.to_string())?;
    Ok(())
}

/// Зонды, которые пора показать (scheduled и время пришло).
pub fn due_probes(app: &App) -> Vec<Probe> {
    let now = local_now_compact();
    ProbeRepo::new(&app.conn)
        .due(&now)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Показать зонд: state → fired, открыть экран ответа.
pub fn open_probe(app: &mut App, probe_id: Uuid) -> Result<(), String> {
    let repo = ProbeRepo::new(&app.conn);
    let mut probe = repo
        .get(&probe_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| app.i18n.get(Str::Error).to_string())?;
    if probe.state == ProbeState::Scheduled {
        probe.fire(local_now_compact());
        repo.update(&probe).map_err(|e| e.to_string())?;
    }
    app.active_probe = Some(probe);

    // Вопросы зондовых шкал
    app.probe_answers = ScaleRepo::new(&app.conn)
        .list_active_phase(Phase::Probe)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(scale, version)| ScaleAnswer {
            scale,
            version,
            value: 5.0,
        })
        .collect();
    app.screen = Screen::Probe(Some(probe_id));
    Ok(())
}

/// Сохранить ответы зонда (свежие или догоняющие — по состоянию).
pub fn save_probe_answers(app: &mut App) -> Result<(), String> {
    let Some(probe) = app.active_probe.clone() else {
        return Err(app.i18n.get(Str::Error).to_string());
    };
    // Задержка: минуты между плановым и текущим моментом; если зонд
    // свежий (ответ в пределах 5 мин) — None (аналитике проще).
    let delay = catchup_delay_minutes(&probe.scheduled_at);
    let responses: Vec<Response> = app
        .probe_answers
        .iter()
        .map(|a| Response::for_probe(probe.id, a.version.id, a.value, delay))
        .collect();

    let repo = ProbeRepo::new(&app.conn);
    let mut probe = probe;
    let answered = probe.answer(now_iso(), delay);
    if answered {
        repo.update(&probe).map_err(|e| e.to_string())?;
    }
    if !responses.is_empty() {
        ScaleRepo::new(&app.conn)
            .save_session_responses(&responses)
            .map_err(|e| e.to_string())?;
    }
    app.active_probe = None;
    app.today_probes = repo
        .list_day(&local_date_today())
        .map_err(|e| e.to_string())?;
    app.screen = Screen::Setup;
    app.toast = Some(app.i18n.get(Str::SessionSaved).to_string());
    Ok(())
}

/// Догоняющий список (показанные, неотвеченные).
pub fn reload_catchable(app: &mut App) -> Result<(), String> {
    app.today_probes = ProbeRepo::new(&app.conn)
        .catchable()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Загрузить последние FFMQ-результаты.
pub fn reload_ffqm(app: &mut App) -> Result<(), String> {
    app.ffqm_results = FfmqRepo::new(&app.conn)
        .list_recent(10)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Сохранить введённые вручную факторные баллы FFMQ (5 полей).
/// Файл опросника пользователь проходит вне Decon (лицензионная
/// гигиена, SCALES.md §6.1); сюда приходят готовые баллы.
pub fn save_ffmq(app: &mut App, scores: [f64; 5], source_hash: &str) -> Result<(), String> {
    let r = FfmqResult {
        id: Uuid::now_v7(),
        taken_at: now_iso(),
        observing: scores[0],
        describing: scores[1],
        acting_aware: scores[2],
        non_reactivity: scores[3],
        nonjudging: scores[4],
        source_file_hash: source_hash.to_string(),
    };
    if !r.is_valid() {
        return Err(app.i18n.get(Str::Error).to_string());
    }
    FfmqRepo::new(&app.conn)
        .insert(&r)
        .map_err(|e| e.to_string())?;
    reload_ffqm(app)
}

/// Загрузить вопросы слойной карты (фаза Layer).
pub fn load_layer_scales(app: &mut App) -> Result<(), String> {
    app.layer_answers = ScaleRepo::new(&app.conn)
        .list_active_phase(Phase::Layer)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(scale, version)| ScaleAnswer {
            scale,
            version,
            value: 5.0,
        })
        .collect();
    Ok(())
}

/// Сохранить слойную карту: 2 ответа (availability, holding) на слой.
pub fn save_layer_map(app: &mut App, layer: Layer) -> Result<(), String> {
    // Ответы слойной карты пишутся как ответы «зондовой» ширины, но
    // без probe/session — отдельный вид замера; до S4 (аналитика)
    // привязываем к технической сессии-карте.
    let responses: Vec<Response> = app
        .layer_answers
        .iter()
        .map(|a| Response::for_probe(Uuid::nil(), a.version.id, a.value, None))
        .collect();
    if responses.is_empty() {
        return Ok(());
    }
    // Пишем напрямую (probe_id = NIL не в таблице probes — CHECK
    // требует session_id или probe_id; используем техническую сессию)
    let mut session = Session::new(
        SessionKind::Practice,
        now_iso(),
        (crate::state::clock::unix_now_ms() / 3_600_000 % 24) as u32,
    );
    session.set_layer(Some(layer));
    session.set_technique(Some("layer-map".into()));
    SessionRepo::new(&app.conn)
        .insert(&session)
        .map_err(|e| e.to_string())?;
    let sid = session.id;
    let responses: Vec<Response> = responses
        .into_iter()
        .map(|r| Response::for_session(sid, r.scale_version_id, r.value))
        .collect();
    ScaleRepo::new(&app.conn)
        .save_session_responses(&responses)
        .map_err(|e| e.to_string())?;
    app.toast = Some(app.i18n.get(Str::SessionSaved).to_string());
    Ok(())
}

/// Сохранить настройки из черновиков (окно зондов, количество,
/// интервалы, язык). Пишется в БД (правило №6: ноль конфигов).
pub fn save_settings(app: &mut App) -> Result<(), String> {
    let parse_hhmm = |s: &str| -> Option<(u8, u8)> {
        let (h, m) = s.split_once(':')?;
        let h: u8 = h.trim().parse().ok()?;
        let m: u8 = m.trim().parse().ok()?;
        (h < 24 && m < 60).then_some((h, m))
    };
    let (sh, sm) =
        parse_hhmm(&app.probe_window_start).ok_or_else(|| app.i18n.get(Str::Error).to_string())?;
    let (eh, em) =
        parse_hhmm(&app.probe_window_end).ok_or_else(|| app.i18n.get(Str::Error).to_string())?;
    let count: u8 = app
        .probe_count
        .trim()
        .parse()
        .ok()
        .filter(|c| (1..=10).contains(c))
        .ok_or_else(|| app.i18n.get(Str::Error).to_string())?;
    let gap: u16 = app
        .probe_min_gap
        .trim()
        .parse()
        .ok()
        .filter(|g| *g < 1440)
        .ok_or_else(|| app.i18n.get(Str::Error).to_string())?;
    let interval: u32 = app
        .signal_interval
        .trim()
        .parse()
        .ok()
        .filter(|i| (10..=3600).contains(i))
        .ok_or_else(|| app.i18n.get(Str::Error).to_string())?;

    app.settings.probe.window.start_h = sh;
    app.settings.probe.window.start_m = sm;
    app.settings.probe.window.end_h = eh;
    app.settings.probe.window.end_m = em;
    app.settings.probe.count = count;
    app.settings.probe.min_gap_minutes = gap;
    app.settings.session.signal_interval_s = interval;

    SettingsRepo::new(&app.conn)
        .save(&app.settings)
        .map_err(|e| e.to_string())?;

    // Зонды: план на сегодня мог измениться
    if app.settings.probe.enabled {
        let _ = rebuild_today_probes(app);
    }
    app.toast = Some(app.i18n.get(Str::SessionSaved).to_string());
    Ok(())
}

/// Сменить язык интерфейса (и сохранить).
pub fn set_language(app: &mut App, lang: Language) -> Result<(), String> {
    app.settings.language = lang;
    app.i18n = I18n::new(lang);
    SettingsRepo::new(&app.conn)
        .save(&app.settings)
        .map_err(|e| e.to_string())
}
