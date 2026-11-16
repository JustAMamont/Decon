//! Экраны приложения (Dioxus). Тонкий слой над `state::App`: только
//! отображение и события, вся логика — в состоянии (ARCHITECTURE.md §3).
//!
//! Разбито по смысловым зонам (правило №7): журнал, зонды дня,
//! периодические инструменты, настройки.

pub mod journal;
pub mod probes;
pub mod settings;
pub mod tools;
pub mod widgets;

use crate::i18n::{I18n, Str};
use crate::state::app::{App, Screen};
use dioxus::prelude::*;

/// CSS приложения: тёмная спокойная тема, крупные кнопки-маркеры.
pub const CSS: &str = r#"
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
.nav { display: flex; gap: 8px; margin-bottom: 6px; flex-wrap: wrap; }
.nav button.active { background: #2f6f4f; border-color: #3d8a63; }
.badge {
    display: inline-block;
    background: #2f6f4f;
    color: #eef1f5;
    border-radius: 10px;
    padding: 1px 8px;
    font-size: 12px;
    margin-left: 6px;
}
"#;

/// Корневой компонент.
pub fn root() -> Element {
    use_context_provider(|| Signal::new(AppState::boot()));

    rsx! {
        style { {CSS} }
        app_shell {}
    }
}

/// Обёртка над App для Signal (App не Clone — Signal даёт &mut через
/// write(), клонировать состояние не нужно).
pub struct AppState {
    /// Домен-состояние.
    pub app: App,
    /// Ошибка последней операции (показывается тостом).
    pub error: Option<String>,
}

impl AppState {
    /// Загрузка состояния: БД + keyring; при ошибке — fallback с
    /// сообщением вместо паники.
    pub fn boot() -> Self {
        match App::init() {
            Ok(app) => AppState { app, error: None },
            Err(e) => AppState {
                app: App::init_fallback(),
                error: Some(e),
            },
        }
    }
}

/// Каркас: навигация + текущий экран + тост.
fn app_shell() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let error = state.read().error.clone();
    let screen = state.read().app.screen.clone();
    let toast = state.read().app.toast.clone();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    // Фоновый цикл зондов: раз в минуту проверяем «пора показать».
    // Десктоп-ограничение (ARCHITECTURE.md §5): зонды живут, пока
    // приложение запущено.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let mut s = state.write();
            if s.app.settings.probe.enabled && !matches!(s.app.screen, Screen::Probe(_)) {
                let due = crate::state::probes_flow::due_probes(&s.app);
                if let Some(first) = due.first() {
                    let id = first.id;
                    let res = crate::state::probes_flow::open_probe(&mut s.app, id);
                    if let Err(e) = res {
                        s.error = Some(e);
                    }
                }
            }
        }
    });

    rsx! {
        div { class: "container",
            if let Some(err) = error {
                div { class: "toast", style: "background:#6b3040;", "{err}" }
            }
            h1 { {i18n.get(Str::AppTitle)} }
            div { class: "nav",
                nav_button { label: i18n.get(Str::StartSession), target: Screen::Setup,
                    active: matches!(screen, Screen::Setup | Screen::Pre | Screen::Running | Screen::Post) }
                nav_button { label: i18n.get(Str::History), target: Screen::History,
                    active: screen == Screen::History }
                nav_button { label: i18n.get(Str::ProbeList), target: Screen::ProbeCatchup,
                    active: matches!(screen, Screen::Probe(_) | Screen::ProbeCatchup) }
                nav_button { label: i18n.get(Str::Tools), target: Screen::Tools,
                    active: screen == Screen::Tools }
                nav_button { label: i18n.get(Str::Settings), target: Screen::Settings,
                    active: screen == Screen::Settings }
            }
            match screen.clone() {
                Screen::Setup => rsx! { journal::setup_screen {} },
                Screen::Pre => rsx! { journal::pre_screen {} },
                Screen::Running => rsx! { journal::running_screen {} },
                Screen::Post => rsx! { journal::post_screen {} },
                Screen::History => rsx! { journal::history_screen {} },
                Screen::Probe(id) => rsx! { probes::probe_screen { probe_id: id } },
                Screen::ProbeCatchup => rsx! { probes::catchup_screen {} },
                Screen::Tools => rsx! { tools::tools_screen {} },
                Screen::Settings => rsx! { settings::settings_screen {} },
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
                {
                    let mut s = state.write();
                    match target {
                        Screen::History => { let _ = s.app.reload_history(); }
                        Screen::ProbeCatchup => { let _ = crate::state::probes_flow::reload_catchable(&mut s.app); }
                        Screen::Tools => { let _ = crate::state::probes_flow::reload_ffqm(&mut s.app); }
                        _ => {}
                    }
                    s.app.screen = target.clone();
                    s.app.toast = None;
                }
            },
            {label}
        }
    }
}
