//! Периодические инструменты (S2): FFMQ-15 (ввод баллов), слойная карта.

use crate::i18n::{I18n, Str};
use crate::views::AppState;
use decon_core::db::repo::FfmqResult;
use decon_core::model::session::Layer;
use dioxus::prelude::*;

/// Экран инструментов: FFMQ-ввод + слойная карта.
pub fn tools_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let ffqm = state.read().app.ffqm_results.clone();
    let has_layer = !state.read().app.layer_answers.is_empty();

    rsx! {
        div {
            h2 { {i18n.get(Str::Tools)} }

            // --- FFMQ-15 (SCALES.md §6.1)
            h2 { "FFMQ-15" }
            div { style: "color:#7d8590; font-size:13px; margin: 6px 0;",
                {i18n.get(Str::FfmqHint)}
            }
            ffmq_form {}
            if !ffqm.is_empty() {
                h2 { {i18n.get(Str::FfmqHistory)} }
                for r in &ffqm {
                    ffqm_row { r: r.clone() }
                }
            }

            // --- Слойная карта (SCALES.md §6.2)
            h2 { {i18n.get(Str::LayerMap)} }
            div { style: "color:#7d8590; font-size:13px; margin: 6px 0;",
                {i18n.get(Str::LayerMapHint)}
            }
            if has_layer {
                layer_map_form {}
            } else {
                button {
                    onclick: move |_| {
                        let res = crate::state::probes_flow::load_layer_scales(&mut state.write().app);
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

/// Форма FFMQ: 5 факторных баллов 0–100.
fn ffmq_form() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    // Черновики полей хранятся в сигнале-словаре? Проще: локальный вектор
    // в состоянии — но чтобы не раздувать App, используем HashMap в
    // useState-подобном Signal через use_signal.
    let mut drafts: Signal<[String; 5]> = use_signal(|| {
        [
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ]
    });
    let labels = [
        i18n.get(Str::FfmqObserving),
        i18n.get(Str::FfmqDescribing),
        i18n.get(Str::FfmqActing),
        i18n.get(Str::FfmqNonreactivity),
        i18n.get(Str::FfmqNonjudging),
    ];

    rsx! {
        div {
            for i in 0..5 {
                div { class: "row", style: "margin: 6px 0;",
                    label { style: "flex:1; margin: 0;", "{labels[i]}" }
                    input {
                        r#type: "number",
                        style: "width: 110px;",
                        min: "0",
                        max: "100",
                        step: "0.5",
                        value: "{drafts.read()[i]}",
                        oninput: move |e| {
                            let mut d = drafts.write();
                            d[i] = e.value();
                        },
                    }
                }
            }
            button {
                class: "primary",
                style: "margin-top: 8px;",
                onclick: move |_| {
                    let parsed: Vec<f64> = drafts
                        .read()
                        .iter()
                        .map(|s| s.trim().parse::<f64>().unwrap_or(-1.0))
                        .collect();
                    if parsed.len() == 5 && parsed.iter().all(|v| (0.0..=100.0).contains(v)) {
                        let res = crate::state::probes_flow::save_ffmq(
                            &mut state.write().app,
                            [parsed[0], parsed[1], parsed[2], parsed[3], parsed[4]],
                            "manual",
                        );
                        if let Err(e) = res {
                            state.write().error = Some(e);
                        } else {
                            *drafts.write() = Default::default();
                        }
                    } else {
                        state.write().error = Some(i18n.get(Str::Error).to_string());
                    }
                },
                {i18n.get(Str::Save)}
            }
        }
    }
}

/// Строка истории FFMQ: 5 баллов + дата.
#[component]
fn ffqm_row(r: FfmqResult) -> Element {
    let title = r.taken_at.clone();
    let detail = format!(
        "{:.1} / {:.1} / {:.1} / {:.1} / {:.1}",
        r.observing, r.describing, r.acting_aware, r.non_reactivity, r.nonjudging
    );
    rsx! {
        div { class: "history-item",
            div { "{title}" }
            div { class: "meta", "{detail}" }
        }
    }
}

/// Форма слойной карты: выбор слоя + 2 шкалы (availability/holding).
fn layer_map_form() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);
    let mut layer: Signal<Option<Layer>> = use_signal(|| None);

    rsx! {
        div {
            div { class: "row",
                for (l, name) in [
                    (Layer::Visual, "Зрительный / Visual"),
                    (Layer::Audio, "Слуховой / Audio"),
                    (Layer::Vital, "Витальный / Vital"),
                    (Layer::Semantic, "Семантический / Semantic"),
                    (Layer::Polymodal, "Полимодальный / Polymodal"),
                ] {
                    button {
                        class: if *layer.read() == Some(l) { "primary" },
                        onclick: move |_| {
                            *layer.write() = Some(l);
                        },
                        {name}
                    }
                }
            }
            for i in 0..state.read().app.layer_answers.len() {
                super::widgets::scale_row { idx: i, phase: "layer" }
            }
            button {
                class: "primary",
                style: "margin-top: 8px;",
                disabled: layer.read().is_none(),
                onclick: move |_| {
                    if let Some(l) = *layer.read() {
                        let res = crate::state::probes_flow::save_layer_map(&mut state.write().app, l);
                        if let Err(e) = res {
                            state.write().error = Some(e);
                        }
                    }
                },
                {i18n.get(Str::Save)}
            }
        }
    }
}
