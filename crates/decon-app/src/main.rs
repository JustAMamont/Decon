//! Точка входа Decon: запуск desktop-приложения (Dioxus).
//!
//! Экраны появятся в S1 (журнал сессий): `views/journal/` — запуск сессии,
//! pre/post-замеры, экран сессии, история (docs/ARCHITECTURE.md §2).

use dioxus::prelude::*;

fn main() {
    let window = dioxus::desktop::WindowBuilder::new().with_title("Decon");
    let cfg = dioxus::desktop::Config::new().with_window(window);
    dioxus::LaunchBuilder::new()
        .with_cfg(cfg)
        .launch(app);
}

/// Корневой компонент: S0 — пустое окно-плейсхолдер.
fn app() -> Element {
    rsx! {}
}
