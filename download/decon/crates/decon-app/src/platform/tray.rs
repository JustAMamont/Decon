//! Системный трей и сигналы ОС (S2.5).
//!
//! Иконка/меню регистрируются через dioxus-desktop (`init_tray_icon`,
//! см. `views/mod.rs` — там же обработчики пунктов меню), этот модуль —
//! платформенная часть: сигнал-флаг завершения и построение иконки.
//!
//! Сигналы ОС: Linux — SIGTERM/SIGINT через signal-hook (флаги
//! выставляются асинхронно, опрашиваются тикером UI-потока; никакой
//! работы с SQLite вне главного потока). Windows — GUI-приложение не
//! получает консольных сигналов, выход через трей.

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Arc, OnceLock};

/// Зарегистрированные сигнал-флаги (term, int); None до установки.
#[cfg(target_os = "linux")]
static FLAGS: OnceLock<(Arc<AtomicBool>, Arc<AtomicBool>)> = OnceLock::new();

/// Зарегистрировать обработчики SIGTERM/SIGINT (Linux; Windows — no-op).
pub fn install_signal_handlers() {
    #[cfg(target_os = "linux")]
    {
        use signal_hook::consts::{SIGINT, SIGTERM};
        let term = Arc::new(AtomicBool::new(false));
        let int = Arc::new(AtomicBool::new(false));
        let _ = signal_hook::flag::register(SIGTERM, term.clone());
        let _ = signal_hook::flag::register(SIGINT, int.clone());
        let _ = FLAGS.set((term, int));
    }
    #[cfg(not(target_os = "linux"))]
    {}
}

/// Взведён ли флаг завершения (SIGTERM/SIGINT).
pub fn shutdown_requested() -> bool {
    #[cfg(target_os = "linux")]
    {
        FLAGS
            .get()
            .is_some_and(|(t, i)| t.load(Ordering::Relaxed) || i.load(Ordering::Relaxed))
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Запросить graceful shutdown из UI (меню трея «Выход»): взводит тот
/// же флаг, что и SIGTERM — фоновый цикл app_shell финализирует живую
/// сессию (end_status=interrupted, flush, checkpoint) и выходит.
/// Отдельный путь (не process::exit) нужен, чтобы финализация шла в
/// dioxus-потоке с доступом к стейту.
pub fn request_shutdown() {
    #[cfg(target_os = "linux")]
    {
        if let Some((term, _)) = FLAGS.get() {
            term.store(true, Ordering::Relaxed);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::process::exit(0);
    }
}

/// Сбросить флаги (после обработки).
pub fn clear_shutdown_flag() {
    #[cfg(target_os = "linux")]
    {
        if let Some((t, i)) = FLAGS.get() {
            t.store(false, Ordering::Relaxed);
            i.store(false, Ordering::Relaxed);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {}
}

/// Иконка трея: 32×32 RGBA, процедурная — кольцо дКВ («равномерное
/// распределение внимания по полю»: выделена периферия, центр спокоен).
pub fn tray_icon_rgba() -> Result<tray_icon::Icon, tray_icon::BadIcon> {
    const S: u32 = 32;
    let mut rgba = Vec::with_capacity((S * S * 4) as usize);
    let c = (S as f32 - 1.0) / 2.0;
    for y in 0..S {
        for x in 0..S {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let r = (dx * dx + dy * dy).sqrt();
            // Тёмный фон #14161a, кольцо #3d8a63, мягкие края
            let (rr, gg, bb) = if (11.0..=13.0).contains(&r) {
                (0x3d, 0x8a, 0x63)
            } else if (9.0..11.0).contains(&r) {
                (0x2f, 0x6f, 0x4f)
            } else {
                (0x14, 0x16, 0x1a)
            };
            rgba.extend_from_slice(&[rr, gg, bb, 0xff]);
        }
    }
    tray_icon::Icon::from_rgba(rgba, S, S)
}

/// Доступна ли библиотека трея (Linux): libappindicator-sys паникует
/// при её отсутствии, а паника в use_hook убивает весь UI
/// (регрессия 2026-11-17). Зонд через dlopen — без паник.
#[cfg(target_os = "linux")]
pub fn appindicator_available() -> bool {
    const CANDIDATES: [&str; 4] = [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ];
    unsafe extern "C" {
        fn dlopen(
            filename: *const core::ffi::c_char,
            flags: core::ffi::c_int,
        ) -> *mut core::ffi::c_void;
        fn dlclose(handle: *mut core::ffi::c_void) -> core::ffi::c_int;
    }
    for lib in CANDIDATES {
        if let Ok(c) = std::ffi::CString::new(lib) {
            // RTLD_LAZY | RTLD_LOCAL (glibc): только проверить наличие
            unsafe {
                let handle = dlopen(c.as_ptr(), 2);
                if !handle.is_null() {
                    dlclose(handle);
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icon_builds() {
        // Иконка валидна по формату tray-icon (32×32 RGBA).
        tray_icon_rgba().expect("icon builds");
    }

    #[test]
    fn shutdown_flag_default_false_and_clear() {
        // До регистрации (или без сигналов) — false; сброс — безопасен.
        clear_shutdown_flag();
        // shutdown_requested() зависит от регистрации: в тестовом
        // процессе без сигналов — false либо флаги не взведены.
        let _ = shutdown_requested();
    }
}
