//! Экран настроек (S2): зонды, сигналы сессии, язык, статус keyring.

use crate::i18n::{I18n, Str};
use crate::platform::secret_store::KeyringBackend;
use crate::views::AppState as ShellState;
use decon_core::model::settings::Language;
use dioxus::prelude::*;

/// Экран настроек: все параметры в БД (правило №6: ноль конфигов).
pub fn settings_screen() -> Element {
    let mut state = use_context::<Signal<ShellState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    let ws = state.read().app.probe_window_start.clone();
    let we = state.read().app.probe_window_end.clone();
    let count = state.read().app.probe_count.clone();
    let gap = state.read().app.probe_min_gap.clone();
    let interval = state.read().app.signal_interval.clone();
    let probes_enabled = state.read().app.settings.probe.enabled;
    let keyring_label = match state.read().app.keyring_backend {
        KeyringBackend::OsKeyring => i18n.get(Str::KeyringOs),
        KeyringBackend::Memory => i18n.get(Str::KeyringMemory),
    };
    let probes_label = format!(
        "{}: {}",
        i18n.get(Str::ProbesEnabled),
        if probes_enabled {
            i18n.get(Str::On)
        } else {
            i18n.get(Str::Off)
        }
    );

    rsx! {
        div {
            h2 { {i18n.get(Str::Settings)} }

            // --- Зонды дня
            label { {i18n.get(Str::ProbeWindowStart)} }
            input {
                r#type: "text",
                placeholder: "09:00",
                value: "{ws}",
                oninput: move |e| state.write().app.probe_window_start = e.value(),
            }
            label { {i18n.get(Str::ProbeWindowEnd)} }
            input {
                r#type: "text",
                placeholder: "23:00",
                value: "{we}",
                oninput: move |e| state.write().app.probe_window_end = e.value(),
            }
            div { class: "row",
                div {
                    label { {i18n.get(Str::ProbeCount)} }
                    input {
                        r#type: "number",
                        min: "1",
                        max: "10",
                        value: "{count}",
                        oninput: move |e| state.write().app.probe_count = e.value(),
                    }
                }
                div {
                    label { {i18n.get(Str::ProbeMinGap)} }
                    input {
                        r#type: "number",
                        min: "0",
                        max: "1439",
                        value: "{gap}",
                        oninput: move |e| state.write().app.probe_min_gap = e.value(),
                    }
                }
            }

            // --- Сессия
            label { {i18n.get(Str::SignalInterval)} }
            input {
                r#type: "number",
                min: "10",
                max: "3600",
                value: "{interval}",
                oninput: move |e| state.write().app.signal_interval = e.value(),
            }

            // --- Зонды вкл/выкл
            div { class: "row", style: "margin-top: 12px;",
                button {
                    class: if probes_enabled { "primary" },
                    onclick: move |_| {
                        let mut s = state.write();
                        s.app.settings.probe.enabled = !s.app.settings.probe.enabled;
                        if s.app.settings.probe.enabled {
                            let _ = crate::state::probes_flow::rebuild_today_probes(&mut s.app);
                        }
                    },
                    "{probes_label}"
                }
            }

            // --- Статус keyring
            div { style: "color:#7d8590; font-size:13px; margin-top: 14px;",
                "🔑 {i18n.get(Str::KeyringStatus)}: {keyring_label}"
            }

            // --- Язык
            label { {i18n.get(Str::Language)} }
            div { class: "row",
                for (l, name) in [(Language::Ru, "Русский"), (Language::En, "English")] {
                    button {
                        class: if lang == l { "primary" },
                        onclick: move |_| {
                            let res = crate::state::probes_flow::set_language(&mut state.write().app, l);
                            if let Err(e) = res {
                                state.write().error = Some(e);
                            }
                        },
                        {name}
                    }
                }
            }

            div { style: "margin-top:18px;",
                button {
                    class: "primary",
                    onclick: move |_| {
                        let res = crate::state::probes_flow::save_settings(&mut state.write().app);
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
