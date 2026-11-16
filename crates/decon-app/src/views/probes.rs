//! Экраны зондов дня (S2): ответ на зонд, догоняющий список.

use crate::i18n::{I18n, Str};
use crate::state::app::Screen;
use crate::views::AppState;
use dioxus::prelude::*;
use uuid::Uuid;

/// Экран ответа на зонд: 2 слайдера + флаг практики.
#[component]
pub fn probe_screen(probe_id: Option<Uuid>) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let is_catchup = state
        .read()
        .app
        .active_probe
        .as_ref()
        .map(|p| p.delay_minutes.is_some() || p.fired_at.is_some())
        .unwrap_or(false);
    let planned = state
        .read()
        .app
        .active_probe
        .as_ref()
        .map(|p| p.scheduled_at.clone())
        .unwrap_or_default();

    rsx! {
        div {
            h2 { {i18n.get(Str::ProbeTitle)} }
            if is_catchup {
                div { class: "empty", style: "margin: 8px 0;",
                    "{i18n.get(Str::ProbeCatchupHint)} ({planned})"
                }
            }
            for i in 0..state.read().app.probe_answers.len() {
                super::widgets::scale_row { idx: i, phase: "probe" }
            }
            div { class: "row", style: "margin-top:16px;",
                button {
                    onclick: move |_| {
                        // Пропустить зонд: state → skipped, без ответов
                        let mut s = state.write();
                        if let Some(p) = s.app.active_probe.as_mut() {
                            use decon_core::model::probe::ProbeState;
                            p.state = ProbeState::Skipped;
                        }
                        s.app.active_probe = None;
                        s.app.screen = Screen::Setup;
                    },
                    {i18n.get(Str::Skip)}
                }
                button {
                    class: "primary",
                    onclick: move |_| {
                        let res = crate::state::probes_flow::save_probe_answers(&mut state.write().app);
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

/// Догоняющий список: показанные, но неотвеченные зонды + план дня.
pub fn catchup_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let probes = state.read().app.today_probes.clone();
    // Владеющая запись для строки списка (rsx-замыкания не заимствуют)
    struct Row {
        id: Uuid,
        at: String,
        catchable: bool,
    }
    let items: Vec<Row> = probes
        .iter()
        .map(|p| Row {
            id: p.id,
            at: p.scheduled_at.clone(),
            catchable: p.is_catchable(),
        })
        .collect();

    rsx! {
        div {
            h2 { {i18n.get(Str::ProbeList)} }
            if items.is_empty() {
                div { class: "empty", {i18n.get(Str::HistoryEmpty)} }
            } else {
                for item in items {
                    div { class: "history-item",
                        div { "{item.at}" }
                        div { class: "row", style: "margin-top:6px;",
                            if item.catchable {
                                button {
                                    class: "primary",
                                    onclick: move |_| {
                                        let res = crate::state::probes_flow::open_probe(&mut state.write().app, item.id);
                                        if let Err(e) = res {
                                            state.write().error = Some(e);
                                        }
                                    },
                                    {i18n.get(Str::AnswerNow)}
                                }
                            } else {
                                span { class: "badge",
                                    { i18n.get(Str::ProbeDone) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
