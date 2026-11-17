//! Экраны приложения (Dioxus). Тонкий слой над `state::App`: только
//! отображение и события, вся логика — в состоянии (ARCHITECTURE.md §3).
//!
//! Разбито по смысловым зонам (правило №7): журнал, зонды дня,
//! периодические инструменты, настройки.

pub mod analytics;
pub mod journal;
pub mod probes;
pub mod settings;
pub mod tools;
pub mod trainers;
pub mod widgets;

use crate::i18n::{I18n, Str};
use crate::state::app::{App, Screen};
use decon_core::model::session::SessionEndStatus;
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
.timer {
    font-size: 44px;
    font-variant-numeric: tabular-nums;
    text-align: center;
    margin: 18vh 0 8px;
    color: #eef1f5;
}
.signal-hint { text-align: center; color: #7d8590; font-size: 13px; margin-bottom: 14px; }
/* Управление сессией — приглушённые кнопки внизу экрана: доступны,
 * но не тянут взгляд (ноль интерактива в практике дКВ). */
.session-controls {
    position: fixed;
    bottom: 16px;
    left: 0;
    right: 0;
    text-align: center;
}
.session-controls button { opacity: 0.35; transition: opacity 0.25s; }
.session-controls button:hover { opacity: 1; }
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

    // Трей (S2.5): меню «Открыть / Выход», регистрируется один раз.
    use_tray();

    // Резидентный старт (S2.5): настройка «не показывать окно при старте»
    // — прячем окно первым тиком (настройка из БД читается в boot, окно
    // уже создано). Флаг CLI --hidden обрабатывается в main.rs напрямую.
    let hide_on_start = !state.read().app.settings.residency.show_window_on_start;
    use_hook(move || {
        if hide_on_start {
            dioxus::desktop::window().window.set_visible(false);
        }
    });

    // Фоновый цикл (S2.5): раз в 10 с — зонды, повторы-«будильники»,
    // сигналы ОС, стрим-флаш. Тик без кражи фокуса: окно не показываем,
    // только уведомление ОС + звук; окно открывает пользователь из трея.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let mut s = state.write();
            let app = &mut s.app;

            // Сигнал ОС / выход: финализируем сессию честно и уходим
            if crate::platform::tray::shutdown_requested() {
                let _ = app.finalize_running(SessionEndStatus::Interrupted);
                crate::platform::tray::clear_shutdown_flag();
                std::process::exit(0);
            }

            // Тик живой сессии (таймер/сигналы/стрим-флаш) — работает и
            // при скрытом окне: сессия живёт в состоянии, не в окне.
            if app.screen == Screen::Running {
                let _ = app.tick_running();
            } else if app.running.is_some() {
                // Окно скрыли/закрыли во время сессии — сессия идёт
                let _ = app.tick_running();
            }

            // Зонды: показать due (уведомление + звук, окно не трогаем).
            // Правило №9: во время живой практики — тишина, ни уведомлений,
            // ни звука (вмешательство разрушает состояние); зонд остаётся
            // непоказанным и сработает после завершения сессии (догоняющий
            // режим S2), либо ответится из списка зондов.
            let in_practice = app.running.is_some();
            if app.settings.probe.enabled && !in_practice && !matches!(app.screen, Screen::Probe(_))
            {
                let due = crate::state::probes_flow::due_probes(app);
                if let Some(first) = due.first() {
                    let id = first.id;
                    let res = crate::state::probes_flow::fire_probe_without_focus(app, id);
                    match res {
                        Ok(true) => {
                            crate::platform::notify::notify_probe(
                                app.i18n.get(Str::AppTitle),
                                app.i18n.get(Str::ProbeTitle),
                            );
                            play_alert_sound(app);
                        }
                        Ok(false) => {}
                        Err(e) => s.error = Some(e),
                    }
                } else {
                    // Повторы-«будильники»: fired без ответа дольше N мин —
                    // снова уведомление + звук (метка уже в last_reminder_at)
                    let reminded = crate::state::probes_flow::remind_stale_probes(app).unwrap_or(0);
                    if reminded > 0 {
                        crate::platform::notify::notify_probe(
                            app.i18n.get(Str::AppTitle),
                            app.i18n.get(Str::ProbeTitle),
                        );
                        play_alert_sound(app);
                    }
                }
            } else if app.settings.probe.enabled && !in_practice {
                // Зонд открыт на экране — повторов не нужно, но expire
                // прошлых дней живёт (recovery-чистка без оповещений)
                let _ = crate::state::probes_flow::expire_old_probes(app);
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
                nav_button { label: i18n.get(Str::Trainers), target: Screen::Trainers,
                    active: matches!(screen, Screen::Trainers | Screen::TrainerRun) }
                nav_button { label: i18n.get(Str::Analytics), target: Screen::Analytics,
                    active: screen == Screen::Analytics }
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
                Screen::Trainers => rsx! { trainers::list_screen {} },
                Screen::TrainerRun => rsx! { trainers::run_screen {} },
                Screen::Analytics => rsx! { analytics::analytics_screen {} },
            }
            if let Some(msg) = toast {
                div { class: "toast", "{msg}" }
            }
        }
    }
}

/// Проиграть звук оповещения по настройке пользователя (S2.5).
fn play_alert_sound(app: &crate::state::app::App) {
    use decon_core::model::settings::SoundChoice;
    match app.settings.residency.sound {
        SoundChoice::Off => {}
        SoundChoice::Chime => crate::platform::sound::play(crate::platform::sound::Sound::Chime),
        SoundChoice::Bell => crate::platform::sound::play(crate::platform::sound::Sound::Bell),
    }
}

/// Трей: меню «Открыть / Выход» (S2.5). Регистрируется один раз на
/// монтировании app_shell, не на каждом рендере — иначе плодятся дубли
/// иконки и обработчиков.
///
/// История регрессий трея:
/// - 2026-11-17 (утро): `use_context`/`use_tray_menu_event_handler`
///   внутри `use_hook`-замыкания — hook-in-hook паника. Тогда хук
///   заменили на поток с `MenuEvent::receiver()` — и сломали вторым
///   способом.
/// - 2026-11-17 (день): поток с receiver() мёртв по дизайну dioxus
///   0.7: при старте dioxus ставит свой глобальный хендлер
///   (`MenuEvent::set_event_handler` в `App::set_tray_icon_receiver`),
///   после чего `receiver()` инертен — события уходят в tao event loop
///   как `UserWindowEvent::TrayMenuEvent`. Плюс `dioxus::desktop::
///   window()` = `consume_context()` паникует вне dioxus-скоупа, т.е.
///   даже получив событие, поток умирал бы паникой на первом «Открыть».
///   Итог: оба пункта меню не работали никогда.
///
/// Правильно (так и сделано): хук `use_tray_menu_event_handler` на
/// верхнем уровне компонента `use_tray` — dioxus сам доставляет
/// `TrayMenuEvent` в замыкание в потоке event loop, где `window()`
/// легален. Иконка — через `use_hook` (без hook-вызовов внутри).
fn use_tray() {
    // Hook-вызовы — только здесь, на верхнем уровне компонента:
    // сначала контекст, потом хук событий меню, и лишь затем use_hook
    // с чистым замыканием (регистрация иконки). Порядок фиксирован:
    // нарушение = hook-in-hook паника «The hook list is already
    // borrowed» (урок S4).
    let _state = use_context::<Signal<AppState>>();

    // Обработчик меню: «open» — показать и поднять окно (легально:
    // замыкание исполняется в потоке event loop, dioxus-скоуп активен);
    // «quit» — взвести SIGTERM-флаг: фоновый цикл app_shell финализирует
    // живую сессию (interrupted, flush, checkpoint) и выходит. Прямой
    // process::exit из меню запрещён — потеряли бы стрим-флаш.
    use dioxus::desktop::use_tray_menu_event_handler;
    use_tray_menu_event_handler(|event| {
        let desktop = dioxus::desktop::window();
        match event.id().as_ref() {
            "open" => {
                desktop.window.set_visible(true);
                desktop.window.set_focus();
            }
            "quit" => crate::platform::tray::request_shutdown(),
            _ => {}
        }
    });

    use_hook(move || {
        use dioxus::desktop::muda::accelerator::Accelerator;
        use dioxus::desktop::trayicon::init_tray_icon;
        use tray_icon::menu::{Menu, MenuItem};

        // Трей опционален: на минимальных Linux без
        // libayatana-appindicator init_tray_icon паникует
        // (libappindicator-sys) и уносит весь UI. Зонд dlopen до init;
        // без библиотеки живём без трея (закрытие окна = скрытие,
        // выход — по сигналу ОС).
        if !crate::platform::tray::appindicator_available() {
            eprintln!("tray unavailable (no appindicator library) — running without tray");
            return;
        }
        let open = MenuItem::with_id("open", "Открыть Decon", true, None::<Accelerator>);
        let quit = MenuItem::with_id("quit", "Выход", true, None::<Accelerator>);
        let menu = Menu::new();
        let _ = menu.append_items(&[&open, &quit]);
        let icon = crate::platform::tray::tray_icon_rgba().ok();
        let _ = init_tray_icon(menu, icon);
    });
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
                        Screen::Analytics => { let _ = s.app.reload_analytics(); }
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
