//! Точка входа Decon: desktop-приложение на Dioxus, резидентный режим S2.5.
//!
//! Сценарии старта:
//! - обычный запуск — окно сразу (или в трей, если в настройках выключено
//!   «показывать окно при старте»);
//! - `--hidden` (из автозапуска ОС) — старт сразу в трей, окно скрыто.
//!
//! Закрытие окна = свернуться в трей (`WindowCloseBehaviour::WindowHides`);
//! выход — пункт «Выход» в трее или SIGTERM/SIGINT (Linux). Сессия
//! продолжается при скрытом/закрытом окне — окно лишь пульт, процесс
//! живёт в app-состоянии (см. views/mod.rs::app_shell).

mod i18n;
mod platform;
mod state;
mod views;

use dioxus::desktop::{Config, WindowBuilder, WindowCloseBehaviour};

fn main() {
    // Диагностика паник (временно, S4-фикс): Dioxus глотает payload,
    // показывая «Any { .. }» — печатаем место и backtrace сами.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("=== PANIC ===");
        eprintln!("location: {:?}", info.location());
        if let Some(s) = info.payload().downcast_ref::<&str>() {
            eprintln!("payload(str): {s}");
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            eprintln!("payload(String): {s}");
        } else {
            eprintln!("payload: (non-string)");
        }
        eprintln!("backtrace:\n{}", std::backtrace::Backtrace::force_capture());
    }));

    // Сигналы ОС до запуска event loop (Linux): SIGTERM/SIGINT → флаг.
    platform::tray::install_signal_handlers();

    // Автозапуск передаёт --hidden; настройка «показывать окно при старте»
    // из БД применяется в app_shell после загрузки состояния (окно уже
    // создано — прячем set_visible(false)), здесь только флаг CLI.
    let hidden_cli = std::env::args().any(|a| a == "--hidden");

    let mut window = WindowBuilder::new().with_title("Decon");
    if hidden_cli {
        window = window.with_visible(false);
    }
    // Close-to-tray: крестик прячет окно, процесс живёт в трее.
    let cfg = Config::new()
        .with_window(window)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides);
    dioxus::LaunchBuilder::new()
        .with_cfg(cfg)
        .launch(views::root);
}
