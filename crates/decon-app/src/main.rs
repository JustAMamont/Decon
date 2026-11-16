//! Точка входа Decon: desktop-приложение на Dioxus.

mod app;
mod i18n;
mod views;

fn main() {
    let window = dioxus::desktop::WindowBuilder::new().with_title("Decon");
    let cfg = dioxus::desktop::Config::new().with_window(window);
    dioxus::LaunchBuilder::new()
        .with_cfg(cfg)
        .launch(views::root);
}
