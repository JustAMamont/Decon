//! Уведомления ОС для зондов дня (S2, ROADMAP: notify-rust / платформы).
//!
//! Linux: DESKTOP_NOTIFICATIONS через D-Bus (notify-rust). Windows:
//! notify-rust использует winrt-notification под cfg(windows) сам.
//! Десктоп-ограничение (ARCHITECTURE.md §5): уведомления идут, пока
//! приложение запущено.

/// Показать уведомление «зонд дня».
///
/// Ошибки не пузыряются в UI: уведомление — мягкая функция; если
/// сервиса нет (headless, CI) — тихо пропускаем, зонд всё равно
/// откроется в окне приложения тикером.
pub fn notify_probe(title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    {
        use notify_rust::Notification;
        use notify_rust::Timeout::Milliseconds;
        let _ = Notification::new()
            .summary(title)
            .body(body)
            .appname("Decon")
            .timeout(Milliseconds(10_000))
            .show();
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Windows/macOS: notify-rust 5.x имеет другие фичи — оставлено
        // до платформенных сборок (см. ROADMAP S2: по платформе).
        let _ = (title, body);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn notify_never_panics_headless() {
        // В CI/D-Bus-less окружении show() просто вернёт Err — тишина.
        super::notify_probe("Decon", "тест уведомления");
    }
}
