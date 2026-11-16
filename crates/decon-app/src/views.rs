//! Экраны журнала (Dioxus). Тонкий слой над `app::App`: только отображение
//! и события, вся логика — в состоянии (ARCHITECTURE.md §3).

use crate::app::{App, Screen};
use crate::i18n::{I18n, Str};
use decon_core::model::event::EventKind;
use decon_core::model::scale::ScaleKind;
use decon_core::model::session::Layer;
use dioxus::prelude::*;

/// CSS приложения: тёмная спокойная тема, крупные кнопки-маркеры.
const CSS: &str = r#"
:root { color-scheme: dark; }
* { box-sizing: border-box; }
body {
    font-family: system-ui, sans-serif;
    background: #14161a;
    color: #d8dce2;
    margin: 0;
    padding: 16px;
}
.container { max-width: 680px; margin: 0 auto; }
h1 { font-size: 20px; font-weight: 600; margin: 4px 0 18px; color: #eef1f5; }
h2 { font-size: 16px; font-weight: 600; margin: 18px 0 8px; color: #cfd5dd; }
button {
    font: inherit;
    background: #232730;
    color: #d8dce2;
    border: 1px solid #343a46;
    border-radius: 8px;
    padding: 10px 16px;
    cursor: pointer;
    margin: 4px 6px 4px 0;
}
button:hover { background: #2c313c; }
button:disabled { opacity: 0.45; cursor: default; }
button.primary { background: #2f6f4f; border-color: #3d8a63; }
button.primary:hover { background: #35805a; }
button.danger { background: #6b3040; border-color: #8a3d52; }
button.marker {
    display: block;
    width: 100%;
    text-align: left;
    padding: 14px 16px;
    margin: 8px 0;
    font-size: 15px;
}
.timer {
    font-size: 44px;
    font-variant-numeric: tabular-nums;
    text-align: center;
    margin: 18px 0 8px;
    color: #eef1f5;
}
.signal-hint { text-align: center; color: #7d8590; font-size: 13px; margin-bottom: 14px; }
.row { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
label { display: block; margin: 10px 0 4px; color: #aab2bd; font-size: 14px; }
input[type=text], input[type=number], textarea {
    font: inherit;
    width: 100%;
    background: #1b1e24;
    color: #d8dce2;
    border: 1px solid #343a46;
    border-radius: 8px;
    padding: 9px 12px;
}
textarea { min-height: 72px; resize: vertical; }
.scale { margin: 14px 0; }
.scale .question { font-size: 15px; color: #e6eaef; }
.scale .anchors {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: #8a929d;
    margin: 2px 0 6px;
}
.scale .value { margin-left: 10px; font-variant-numeric: tabular-nums; color: #9fe3bd; }
input[type=range] { width: 100%; accent-color: #3d8a63; }
.history-item {
    background: #1b1e24;
    border: 1px solid #2c313c;
    border-radius: 8px;
    padding: 10px 14px;
    margin: 8px 0;
    font-size: 14px;
}
.history-item .meta { color: #8a929d; font-size: 12px; margin-top: 2px; }
.toast {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    background: #2f6f4f;
    color: #eef1f5;
    border-radius: 8px;
    padding: 10px 18px;
    font-size: 14px;
}
.empty { color: #7d8590; text-align: center; margin: 30px 0; }
.nav { display: flex; gap: 8px; margin-bottom: 6px; }
.nav button.active { background: #2f6f4f; border-color: #3d8a63; }
"#;

/// Корневой компонент.
pub fn root() -> Element {
    use_context_provider(|| Signal::new(AppState::boot()));

    rsx! {
        style { {CSS} }
        app_shell {}
    }
}

/// Обёртка над App для Signal (App не Clone — внутри RefCell не нужен:
/// Signal даёт &mut через write()).
struct AppState {
    app: App,
    error: Option<String>,
}

impl AppState {
    fn boot() -> Self {
        match App::init() {
            Ok(app) => AppState { app, error: None },
            Err(e) => {
                // Показать ошибку инициализации вместо паники.
                AppState {
                    app: App::init_fallback(),
                    error: Some(e),
                }
            }
        }
    }
}

/// Каркас: навигация + текущий экран + тост.
fn app_shell() -> Element {
    let state = use_context::<Signal<AppState>>();
    let error = state.read().error.clone();
    let screen = state.read().app.screen.clone();
    let toast = state.read().app.toast.clone();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    rsx! {
        div { class: "container",
            if let Some(err) = error {
                div { class: "toast", style: "background:#6b3040;", "{err}" }
            }
            h1 { {i18n.get(Str::AppTitle)} }
            div { class: "nav",
                nav_button { label: i18n.get(Str::StartSession), target: Screen::Setup,
                    active: screen == Screen::Setup }
                nav_button { label: i18n.get(Str::History), target: Screen::History,
                    active: screen == Screen::History }
            }
            match screen {
                Screen::Setup => rsx! { setup_screen {} },
                Screen::Pre => rsx! { pre_screen {} },
                Screen::Running => rsx! { running_screen {} },
                Screen::Post => rsx! { post_screen {} },
                Screen::History => rsx! { history_screen {} },
            }
            if let Some(msg) = toast {
                div { class: "toast", "{msg}" }
            }
        }
    }
}

/// Кнопка навигации.
#[component]
fn nav_button(label: &'static str, target: Screen, active: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    rsx! {
        button {
            class: if active { "active" },
            onclick: move |_| {
                if target == Screen::History {
                    let _ = state.write().app.reload_history();
                }
                state.write().app.screen = target.clone();
                state.write().app.toast = None;
            },
            {label}
        }
    }
}

/// Экран запуска: слой, техника, контекст.
fn setup_screen() -> Element {
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
fn pre_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    rsx! {
        div {
            h2 { {i18n.get(Str::PreTitle)} }
            for i in 0..state.read().app.pre_answers.len() {
                scale_row { idx: i, phase: "pre" }
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

/// Строка шкалы: вопрос, слайдер, якоря.
#[component]
fn scale_row(idx: usize, phase: &'static str) -> Element {
    let state = use_context::<Signal<AppState>>();
    let answers = match phase {
        "pre" => state.read().app.pre_answers.clone(),
        _ => state.read().app.post_answers.clone(),
    };
    let Some(a) = answers.get(idx) else {
        return rsx! {};
    };
    let value = a.value;
    let is_flag = a.scale.kind == ScaleKind::Flag;

    rsx! {
        div { class: "scale",
            div { class: "question",
                "{a.version.question}"
                span { class: "value", " {value:.1}" }
            }
            if is_flag {
                div { class: "row",
                    button {
                        class: if value > 0.5 { "primary" },
                        onclick: move |_| {
                            set_answer(state, phase, idx, 1.0);
                        },
                        "{a.scale.anchor_high}"
                    }
                    button {
                        class: if value <= 0.5 { "primary" },
                        onclick: move |_| {
                            set_answer(state, phase, idx, 0.0);
                        },
                        "{a.scale.anchor_low}"
                    }
                }
            } else {
                input {
                    r#type: "range",
                    min: "0",
                    max: "10",
                    step: "0.5",
                    value: "{value}",
                    oninput: move |e| {
                        let v: f64 = e.value().parse().unwrap_or(5.0);
                        set_answer(state, phase, idx, v);
                    },
                }
            }
            div { class: "anchors",
                span { "{a.scale.anchor_low}" }
                span { "{a.scale.anchor_high}" }
            }
        }
    }
}

fn set_answer(mut state: Signal<AppState>, phase: &str, key: usize, value: f64) {
    let mut s = state.write();
    let answers = if phase == "pre" {
        &mut s.app.pre_answers
    } else {
        &mut s.app.post_answers
    };
    if let Some(a) = answers.get_mut(key) {
        a.value = value;
    }
}

/// Экран сессии: таймер + кнопки-маркеры.
fn running_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    // Тик таймера: раз в секунду. Pererисовка только по сигналу (idle-режим
    // ARCHITECTURE.md §7 не нарушается: interval гаснет вместе с экраном).
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            let mut s = state.write();
            if s.app.screen == Screen::Running {
                let _signal = s.app.tick_running();
                // Равномерный сигнал S1: без звука (аудио-движок — S5),
                // визуальная индикация — мигание счётчика не требуется:
                // событие уже в журнале, экран не дёргаем.
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
            }
        }
    }
}

/// Экран post-замера: 5 слайдеров + заметка.
fn post_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    rsx! {
        div {
            h2 { {i18n.get(Str::PostTitle)} }
            for i in 0..state.read().app.post_answers.len() {
                scale_row { idx: i, phase: "post" }
            }
            // Заметка отключена до keyring-интеграции (S2): ключ
            // auto-режима сейчас живёт одно включение приложения.
            div { style: "color:#7d8590; font-size:13px; margin: 10px 0;",
                "🔒 {i18n.get(Str::NoteEncrypted)} — доступно после настройки хранилища ключа"
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
fn history_screen() -> Element {
    let state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let history = state.read().app.history.clone();
    // Форматирование вне rsx-макроса (в макросе допустимы только элементы).
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
