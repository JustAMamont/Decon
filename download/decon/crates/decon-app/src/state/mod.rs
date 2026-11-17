//! Состояние приложения (правило №7.4: state — отдельно от views).
//!
//! `app.rs` — корневое состояние и методы-переходы; всё, что UI
//! отображает и мутирует, живёт здесь, а не в компонентах.

pub mod app;
pub mod clock;
pub mod probes_flow;
pub mod trainers_flow;
