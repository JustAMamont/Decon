//! Переиспользуемые виджеты: строка шкалы (вопрос + слайдер/флаг + якоря).

use crate::state::app::ScaleAnswer;
use crate::views::AppState;
use dioxus::prelude::*;

/// Строка шкалы: вопрос, слайдер, якоря.
#[component]
pub fn scale_row(idx: usize, phase: &'static str) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let answers: Vec<ScaleAnswer> = match phase {
        "pre" => state.read().app.pre_answers.clone(),
        "post" => state.read().app.post_answers.clone(),
        "probe" => state.read().app.probe_answers.clone(),
        _ => state.read().app.layer_answers.clone(),
    };
    let Some(a) = answers.get(idx) else {
        return rsx! {};
    };
    let value = a.value;
    let is_flag = a.scale.kind == decon_core::model::scale::ScaleKind::Flag;

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
                            set_answer(&mut state, phase, idx, 1.0);
                        },
                        "{a.scale.anchor_high}"
                    }
                    button {
                        class: if value <= 0.5 { "primary" },
                        onclick: move |_| {
                            set_answer(&mut state, phase, idx, 0.0);
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
                        set_answer(&mut state, phase, idx, v);
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

/// Записать значение ответа в нужный список состояния.
fn set_answer(state: &mut Signal<AppState>, phase: &str, idx: usize, value: f64) {
    let mut s = state.write();
    let answers = match phase {
        "pre" => &mut s.app.pre_answers,
        "post" => &mut s.app.post_answers,
        "probe" => &mut s.app.probe_answers,
        _ => &mut s.app.layer_answers,
    };
    if let Some(a) = answers.get_mut(idx) {
        a.value = value;
    }
}
