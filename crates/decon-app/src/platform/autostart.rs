//! Автозапуск с системой (S2.5).
//!
//! Linux: XDG autostart — `~/.config/autostart/decon.desktop` (спека XDG,
//! работает в GNOME/KDE и большинстве WM). Windows: ключ реестра
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (per-user, без прав
//! администратора).
//!
//! Правило №6 (ноль конфигов) не нарушается: файл — не пользовательский
//! конфиг, а системная регистрация автозапуска, которую пользователь
//! переключает в UI; содержимое полностью детерминировано приложением.

#[cfg(target_os = "linux")]
use std::path::PathBuf;

/// Имя приложения для регистрации автозапуска.
pub const APP_NAME: &str = "decon";

/// Linux: путь к .desktop в XDG autostart.
#[cfg(target_os = "linux")]
fn desktop_entry_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join(format!("{APP_NAME}.desktop")))
}

/// Включён ли автозапуск сейчас.
pub fn is_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        desktop_entry_path().map(|p| p.is_file()).unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        // reg query — без новых зависимостей; код-путь проверен компиляцией
        // Windows-таргетом и CI-джобой windows-latest.
        std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                APP_NAME,
            ])
            .output()
            .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains(APP_NAME))
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

/// Включить автозапуск. Возвращает Err с человекочитаемой причиной.
pub fn enable() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let path = desktop_entry_path().ok_or("нет домашнего каталога")?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // XDG Desktop Entry: запуск в фоне (скрытый старт — настройка
        // приложения), при отсутствии трея в сессии окно просто откроется.
        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Decon\n\
             Comment=Журнал практики дКВ\n\
             Exec={} --hidden\n\
             Terminal=false\n\
             Categories=Utility;\n",
            exe.display()
        );
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let out = std::process::Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                APP_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\" --hidden", exe.display()),
                "/f",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("платформа не поддерживается".into())
    }
}

/// Выключить автозапуск.
pub fn disable() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let Some(path) = desktop_entry_path() else {
            return Ok(()); // нет HOME — нет и автозапуска
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                APP_NAME,
                "/f",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success()
            || String::from_utf8_lossy(&out.stderr).contains("not find")
            || String::from_utf8_lossy(&out.stderr).contains("не удается найти")
        {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("платформа не поддерживается".into())
    }
}

// Тесты только для Linux-пути: Windows-ветка — reg.exe-вызовы, их
// roundtrip проверяется CI-джобой windows-latest компиляцией (cfg-ветки).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn autostart_roundtrip_fake_home() {
        // Критерий 4b: enable/disable/is_enabled roundtrip. Тест меняет
        // только XDG_CONFIG_HOME процесса (фейковая песочница), реальный
        // HOME не трогаем.
        let tmp = tempfile::tempdir().expect("tmpdir");
        // safety: тест однопоточный, изолированный процесс cargo test
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.path()) };
        assert!(!is_enabled());
        enable().expect("enable");
        assert!(is_enabled());
        // Файл по спецификации XDG, с Exec от текущего бинарника
        let entry = tmp.path().join("autostart/decon.desktop");
        let content = std::fs::read_to_string(&entry).expect("read");
        assert!(content.contains("[Desktop Entry]"));
        assert!(content.contains("--hidden"));
        disable().expect("disable");
        assert!(!is_enabled());
        // Повторный disable — не ошибка (идемпотентность)
        disable().expect("disable idempotent");
    }
}
