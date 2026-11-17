//! Экраны журнала практики (S1): запуск, pre/post-замеры, сессия,
//! история. Заметка на post-экране шифруется ключом auto-режима (S2).

use crate::i18n::{I18n, Str};
use crate::state::app::Screen;
use crate::views::AppState;
use decon_core::model::event::EventKind;
use decon_core::model::session::Layer;
use dioxus::prelude::*;

/// Экран запуска: слой, техника, контекст.
pub fn setup_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let layer = state.read().app.setup_layer;
    let technique = state.read().app.setup_technique.clone();
    let sleep = state.read().app.setup_sleep.clone();
    let caffeine = state.read().app.setup_caffeine.clone();

    rsx! {
        div {
            h2 { {i18n.get(Str::StartSession)} }
            label { {i18n.get(Str::Layer)} }
            div { class: "row",
                for (l, name) in [
                    (Layer::Visual, "Зрительный / Visual"),
                    (Layer::Audio, "Слуховой / Audio"),
                    (Layer::Vital, "Витальный / Vital"),
                    (Layer::Semantic, "Семантический / Semantic"),
                    (Layer::Polymodal, "Полимодальный / Polymodal"),
                ] {
                    button {
                        class: if layer == Some(l) { "primary" },
                        onclick: move |_| {
                            let cur = state.read().app.setup_layer;
                            state.write().app.setup_layer = if cur == Some(l) { None } else { Some(l) };
                        },
                        {name}
                    }
                }
            }
            label { {i18n.get(Str::TechniqueOptional)} }
            input {
                r#type: "text",
                placeholder: "дКВ 1.0",
                value: "{technique}",
                oninput: move |e| state.write().app.setup_technique = e.value(),
            }
            div { class: "row",
                div {
                    label { {i18n.get(Str::SleepHours)} }
                    input {
                        r#type: "number",
                        step: "0.5",
                        min: "0",
                        max: "24",
                        value: "{sleep}",
                        oninput: move |e| state.write().app.setup_sleep = e.value(),
                    }
                }
                div {
                    label { {i18n.get(Str::CaffeineDoses)} }
                    input {
                        r#type: "number",
                        step: "1",
                        min: "0",
                        max: "20",
                        value: "{caffeine}",
                        oninput: move |e| state.write().app.setup_caffeine = e.value(),
                    }
                }
            }
            div { style: "margin-top:18px;",
                button {
                    class: "primary",
                    onclick: move |_| {
                        let _ = state.write().app.load_pre_scales();
                        state.write().app.screen = Screen::Pre;
                    },
                    {i18n.get(Str::Next)}
                }
            }
        }
    }
}

/// Экран pre-замера: слайдеры до практики.
pub fn pre_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    rsx! {
        div {
            h2 { {i18n.get(Str::PreTitle)} }
            for i in 0..state.read().app.pre_answers.len() {
                super::widgets::scale_row { idx: i, phase: "pre" }
            }
            div { class: "row", style: "margin-top:16px;",
                button {
                    onclick: move |_| { state.write().app.screen = Screen::Setup; },
                    {i18n.get(Str::Back)}
                }
                button {
                    class: "primary",
                    onclick: move |_| {
                        let res = state.write().app.begin_session();
                        if let Err(e) = res {
                            state.write().error = Some(e);
                        }
                    },
                    {i18n.get(Str::Begin)}
                }
            }
        }
    }
}

/// Экран сессии: таймер + кнопки-маркеры.
pub fn running_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    // Тик таймера: раз в секунду. Idle-режим ARCHITECTURE.md §7 не
    // нарушается: future гаснет вместе с экраном.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            let mut s = state.write();
            if s.app.screen == Screen::Running {
                // Равномерный сигнал: событие уже пишется в журнал
                // (tick_running), звук — S5 (аудио-движок).
                let _signal = s.app.tick_running();
            }
        }
    });

    let elapsed = state
        .read()
        .app
        .running
        .as_ref()
        .map(|r| r.elapsed_ms)
        .unwrap_or(0);
    let mins = elapsed / 60_000;
    let secs = (elapsed % 60_000) / 1000;

    rsx! {
        div {
            div { class: "timer", "{mins:02}:{secs:02}" }
            div { class: "signal-hint", {i18n.get(Str::SessionRunning)} }
            for (kind, label) in [
                (EventKind::CollapseFocus, Str::EvCollapse),
                (EventKind::DriftSemantic, Str::EvSemantic),
                (EventKind::DriftBody, Str::EvBody),
                (EventKind::ClarityFlash, Str::EvClarity),
            ] {
                button {
                    class: "marker",
                    onclick: move |_| {
                        if let Some(r) = state.write().app.running.as_mut() {
                            r.mark(kind);
                        }
                    },
                    {i18n.get(label)}
                }
            }
            div { style: "margin-top:18px;",
                button {
                    class: "danger",
                    onclick: move |_| {
                        let res = state.write().app.stop_session();
                        if let Err(e) = res {
                            state.write().error = Some(e);
                        } else {
                            let _ = state.write().app.load_post_scales();
                        }
                    },
                    {i18n.get(Str::Stop)}
                }
                // S2.5: ручное прерывание (срочно бежать/плохо себя
                // чувствует) — сессия закрывается честно с end_status=aborted
                button {
                    onclick: move |_| {
                        let res = state.write().app.abort_session();
                        if let Err(e) = res {
                            state.write().error = Some(e);
                        } else {
                            let mut s = state.write();
                            s.app.toast =
                                Some(s.app.i18n.get(Str::SessionInterrupted).to_string());
                            s.app.screen = Screen::Setup;
                        }
                    },
                    {i18n.get(Str::CancelSession)}
                }
            }
        }
    }
}

/// Экран post-замера: 5 слайдеров + зашифрованная заметка.
pub fn post_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let note = state.read().app.note_draft.clone();

    rsx! {
        div {
            h2 { {i18n.get(Str::PostTitle)} }
            for i in 0..state.read().app.post_answers.len() {
                super::widgets::scale_row { idx: i, phase: "post" }
            }
            label { "🔒 {i18n.get(Str::NoteEncrypted)}" }
            textarea {
                value: "{note}",
                oninput: move |e| state.write().app.note_draft = e.value(),
            }
            div { class: "row", style: "margin-top:16px;",
                button {
                    class: "primary",
                    onclick: move |_| {
                        let res = state.write().app.save_session();
                        if let Err(e) = res {
                            state.write().error = Some(e);
                        }
                    },
                    {i18n.get(Str::Save)}
                }
            }
        }
    }
}

/// История сессий.
pub fn history_screen() -> Element {
    let state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let history = state.read().app.history.clone();
    // Форматирование вне rsx-макроса (внутри — только элементы).
    let items: Vec<(String, String)> = history
        .iter()
        .map(|s| {
            (
                format!(
                    "{} · {}",
                    s.technique().unwrap_or(""),
                    s.layer().map(|l| l.display_name()).unwrap_or("—")
                ),
                format!(
                    "{} · {} {}",
                    s.started_at(),
                    s.duration_s().map(|d| d / 60).unwrap_or(0),
                    i18n.get(Str::MinutesShort)
                ),
            )
        })
        .collect();

    rsx! {
        div {
            h2 { {i18n.get(Str::History)} }
            if items.is_empty() {
                div { class: "empty", {i18n.get(Str::HistoryEmpty)} }
            } else {
                for (title, meta) in &items {
                    div { class: "history-item",
                        div { "{title}" }
                        div { class: "meta", "{meta}" }
                    }
                }
            }
        }
    }
}
