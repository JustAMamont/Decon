//! Экраны тренажёров (S3): список + запуск + активный прогон.
//!
//! Тонкий слой (правило №7.4): вся логика — в движках decon-trainers и
//! `state::trainers_flow`, здесь только рендер и обработчики.
//! Время прогона — `Instant` стейта, тик MOT — `use_future` 60 Гц.

use crate::i18n::{I18n, Str};
use crate::state::app::TrainerEngine;
use crate::state::trainers_flow;
use crate::views::AppState;
use decon_trainers::mot::MotPhase;
use decon_trainers::schulte::{SchulteMode, SchulteSymbols};
use dioxus::prelude::*;

/// Список тренажёров + выбор конфигурации.
pub fn list_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let i18n = I18n::new(state.read().app.lang());
    // Черновики конфигов (локальные сигналы — не в App: значения
    // нужны только этому экрану, в БД не пишутся)
    let mut schulte_grid = use_signal(|| String::from("5"));
    let mut schulte_mode = use_signal(|| SchulteMode::Field);
    let mut schulte_symbols = use_signal(|| SchulteSymbols::Digits);
    let mut mot_targets = use_signal(|| String::from("4"));
    let mut mot_distractors = use_signal(|| String::from("8"));
    let mut mot_speed = use_signal(|| String::from("0.5"));
    let summary = state.read().app.trainer_summary.clone();

    rsx! {
        h2 { {i18n.get(Str::Trainers)} }
        div { style: "color:#7d8590; font-size:13px; margin: 6px 0 14px;",
            {i18n.get(Str::TrainersHint)}
        }
        if let Some(s) = summary {
            div { class: "toast", style: "background:#2f6f4f;", {s} }
        }

        // --- Шульте
        h3 { {i18n.get(Str::SchulteTitle)} }
        div { class: "row",
            div {
                label { {i18n.get(Str::SchulteGrid)} }
                input {
                    r#type: "number", min: "3", max: "10",
                    value: "{schulte_grid}",
                    oninput: move |e| schulte_grid.set(e.value()),
                }
            }
        }
        div { class: "row",
            for (mode, name) in [
                (SchulteMode::Classic, i18n.get(Str::SchulteClassic)),
                (SchulteMode::Field, i18n.get(Str::SchulteField)),
                (SchulteMode::Dkv, i18n.get(Str::SchulteDkv)),
            ] {
                button {
                    class: if schulte_mode() == mode { "primary" },
                    onclick: move |_| schulte_mode.set(mode),
                    {name}
                }
            }
        }
        div { class: "row",
            for (sym, name) in [
                (SchulteSymbols::Digits, i18n.get(Str::SymbolsDigits)),
                (SchulteSymbols::Letters, i18n.get(Str::SymbolsLetters)),
                (SchulteSymbols::Bicolor, i18n.get(Str::SymbolsBicolor)),
            ] {
                button {
                    class: if schulte_symbols() == sym { "primary" },
                    onclick: move |_| schulte_symbols.set(sym),
                    {name}
                }
            }
        }
        button {
            class: "primary",
            onclick: move |_| {
                let grid: u8 = schulte_grid().parse().unwrap_or(5);
                let cfg = decon_trainers::schulte::SchulteConfig {
                    mode: schulte_mode(),
                    grid: grid.clamp(3, 10),
                    symbols: schulte_symbols(),
                };
                let mut s = state.write();
                if let Err(e) = trainers_flow::start_schulte(&mut s.app, cfg) {
                    s.error = Some(e);
                }
            },
            {i18n.get(Str::Begin)}
        }

        // --- MOT
        h3 { style: "margin-top: 22px;", {i18n.get(Str::MotTitle)} }
        div { class: "row",
            div {
                label { {i18n.get(Str::MotTargets)} }
                input {
                    r#type: "number", min: "1", max: "10",
                    value: "{mot_targets}",
                    oninput: move |e| mot_targets.set(e.value()),
                }
            }
            div {
                label { {i18n.get(Str::MotDistractors)} }
                input {
                    r#type: "number", min: "0", max: "22",
                    value: "{mot_distractors}",
                    oninput: move |e| mot_distractors.set(e.value()),
                }
            }
            div {
                label { {i18n.get(Str::MotSpeed)} }
                input {
                    r#type: "number", min: "0.1", max: "2.0", step: "0.1",
                    value: "{mot_speed}",
                    oninput: move |e| mot_speed.set(e.value()),
                }
            }
        }
        button {
            class: "primary",
            onclick: move |_| {
                let cfg = decon_trainers::mot::MotConfig {
                    targets: mot_targets().parse().unwrap_or(4).clamp(1, 10),
                    distractors: mot_distractors().parse().unwrap_or(8).min(22),
                    speed: mot_speed().parse().unwrap_or(0.5),
                    motion_ms: 6000,
                };
                let mut s = state.write();
                if let Err(e) = trainers_flow::start_mot(&mut s.app, cfg) {
                    s.error = Some(e);
                }
            },
            {i18n.get(Str::Begin)}
        }
    }
}

/// Активный прогон: сетка Шульте или поле MOT.
pub fn run_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let has_trainer = state.read().app.active_trainer.is_some();

    // Тик MOT — 60 Гц, пока открыт экран и движок MOT жив.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            if state.read().app.active_trainer.is_none() {
                break;
            }
            let mut s = state.write();
            let is_mot = matches!(
                s.app.active_trainer.as_ref().map(|t| &t.engine),
                Some(TrainerEngine::Mot(_))
            );
            let finished = matches!(
                s.app.active_trainer.as_ref().map(|t| &t.engine),
                Some(TrainerEngine::Mot(m)) if m.phase() == MotPhase::Done
            );
            if is_mot {
                trainers_flow::tick_trainer(&mut s.app);
            }
            if finished {
                // Авто-финализация идеального/полного прогона
                if let Err(e) = trainers_flow::finish_trainer(&mut s.app) {
                    s.error = Some(e);
                }
                break;
            }
        }
    });

    if !has_trainer {
        // Прогон уже завершён (финиширован) — возвращаемся к списку
        return rsx! { div {} };
    }

    let engine_kind = state
        .read()
        .app
        .active_trainer
        .as_ref()
        .map(|t| match &t.engine {
            TrainerEngine::Schulte(_) => 0,
            TrainerEngine::Mot(_) => 1,
        })
        .unwrap_or(0);

    rsx! {
        match engine_kind {
            0 => rsx! { schulte_run { } },
            _ => rsx! { mot_run { } },
        }
    }
}

/// Сетка Шульте: таблица кнопок; клик → tap(i, elapsed).
fn schulte_run() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    // Снимок данных для рендера (движок под мьютексом стейта — копируем)
    let (grid, cells, next_expected, errors, started, finished) = {
        let s = state.read();
        let Some(active) = s.app.active_trainer.as_ref() else {
            return rsx! { div {} };
        };
        let TrainerEngine::Schulte(schulte) = &active.engine else {
            return rsx! { div {} };
        };
        let cfg = schulte.config().clone();
        let cells: Vec<(u16, bool)> = schulte.cells().iter().map(|c| (c.value, c.red)).collect();
        (
            cfg.grid,
            cells,
            schulte.next_expected(),
            schulte.errors(),
            schulte.is_started(),
            schulte.is_finished(),
        )
    };
    let elapsed_hint = state
        .read()
        .app
        .active_trainer
        .as_ref()
        .map(|t| t.started.elapsed().as_secs());

    rsx! {
        h3 { {i18n.get(Str::SchulteTitle)} }
        div { style: "color:#7d8590; font-size:13px; margin: 4px 0 10px;",
            {i18n.get(Str::SchulteInstruction)}
        }
        div { style: "display:flex; gap:14px; align-items:center; margin-bottom:10px; flex-wrap:wrap;",
            if !finished {
                span {
                    style: "font-size:18px; color:#eef1f5;",
                    {i18n.get(Str::SchulteFindNext).replace("{n}", &next_expected.to_string())}
                }
            }
            if started {
                span { style: "color:#7d8590;",
                    {i18n.get(Str::TrainerErrors).replace("{n}", &errors.to_string())}
                    " · "
                    {elapsed_hint.map(|s| format!("{} {}", s, i18n.get(Str::SecondsShort))).unwrap_or_default()}
                }
            }
        }
        div {
            style: "display:grid; gap:6px; width: min(92vw, 480px);",
            grid_template_columns: "repeat({grid}, 1fr)",
            for (i, (value, red)) in cells.iter().enumerate() {
                button {
                    key: "{i}",
                    style: if *red {
                        "aspect-ratio:1; font-size:18px; padding:0; color:#d98a8a; background:#241a1c; border:1px solid #3a2a2e; border-radius:8px;"
                    } else {
                        "aspect-ratio:1; font-size:18px; padding:0; background:#1d2025; border:1px solid #2a2e35; border-radius:8px;"
                    },
                    onclick: move |_| {
                        let mut s = state.write();
                        let Some(active) = s.app.active_trainer.as_mut() else { return };
                        let TrainerEngine::Schulte(schulte) = &mut active.engine else { return };
                        let t = active.started.elapsed().as_millis() as u64;
                        let _ = schulte.tap(i, t);
                        // Прогон завершён — записать результат
                        if schulte.is_finished() {
                            drop(s);
                            let mut s = state.write();
                            if let Err(e) = trainers_flow::finish_trainer(&mut s.app) {
                                s.error = Some(e);
                            }
                        }
                    },
                    {value.to_string()}
                }
            }
        }
    }
}

/// Поле MOT: фазы Cue/Motion/Answer, объекты — абсолютные позиции.
fn mot_run() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    let (phase, positions, targets_count, answers, targets_mask) = {
        let s = state.read();
        let Some(active) = s.app.active_trainer.as_ref() else {
            return rsx! { div {} };
        };
        let TrainerEngine::Mot(mot) = &active.engine else {
            return rsx! { div {} };
        };
        let mut positions = Vec::with_capacity(mot.object_count());
        let mut targets_mask = Vec::with_capacity(mot.object_count());
        for i in 0..mot.object_count() {
            let (x, y) = mot.position(i);
            positions.push((x, y));
            targets_mask.push(mot.is_target(i));
        }
        (
            mot.phase(),
            positions,
            mot.config().targets,
            mot.answers().to_vec(),
            targets_mask,
        )
    };

    let hint = match phase {
        MotPhase::Cue => i18n.get(Str::MotCueHint),
        MotPhase::Motion => i18n.get(Str::MotMotionHint),
        MotPhase::Answer => i18n.get(Str::MotAnswerHint),
        MotPhase::Done => i18n.get(Str::TrainerDone),
    };

    // Поле: относительный div, объекты — абсолютно позиционированные
    // кнопки (кликабельны в Answer).
    rsx! {
        h3 { {i18n.get(Str::MotTitle)} }
        div { style: "color:#7d8590; font-size:13px; margin: 4px 0 10px;", {hint} }
        if phase == MotPhase::Answer {
            div { style: "color:#7d8590; font-size:13px; margin-bottom:8px;",
                {i18n.get(Str::MotInstruction)}
                " ("
                {answers.len().to_string()}
                "/"
                {targets_count.to_string()}
                ")"
            }
        }
        div {
            style: "position:relative; width:min(92vw, 480px); aspect-ratio:1; background:#1d2025; border:1px solid #2a2e35; border-radius:10px; overflow:hidden;",
            for (i, ((x, y), is_target)) in positions.iter().zip(targets_mask.iter()).enumerate() {
                {
                    let left = x * 100.0;
                    let top = y * 100.0;
                    let selected = answers.contains(&i);
                    let show_cue = phase == MotPhase::Cue && *is_target;
                    let bg = if selected {
                        "#3d8a63".to_string()
                    } else if show_cue {
                        "#c9a86a".to_string()
                    } else {
                        "#4a5160".to_string()
                    };
                    let clickable = phase == MotPhase::Answer && !selected;
                    let cursor = if clickable { "pointer" } else { "default" };
                    rsx! {
                        button {
                            key: "{i}",
                            style: "position:absolute; left:{left}%; top:{top}%; width:26px; height:26px; margin:-13px 0 0 -13px; border-radius:50%; border:none; background:{bg}; cursor:{cursor}; padding:0;",
                            onclick: move |_| {
                                if !clickable { return; }
                                let mut s = state.write();
                                let Some(active) = s.app.active_trainer.as_mut() else { return };
                                let TrainerEngine::Mot(mot) = &mut active.engine else { return };
                                let _ = mot.answer(i);
                            },
                        }
                    }
                }
            }
        }
    }
}
