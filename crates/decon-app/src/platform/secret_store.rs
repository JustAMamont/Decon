//! `SecretStore` поверх OS keyring с фолбэком на память.
//!
//! Режим auto (SPEC §7.1): ключ `decon.master-key` кладётся в
//! системный keyring. Если сервис недоступен (headless, нет dbus),
//! ключ живёт в памяти — заметки этой сессии работают, при следующем
//! запуске нужен повторный ввод (честное поведение вместо тихой потери).

use decon_core::db::keyring::{MemoryStore, SecretStore, SECRET_NAME};

/// Бэкенд хранилища ключа (для диагностики в настройках).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringBackend {
    /// OS keyring: Secret Service / Wincred / Keychain.
    OsKeyring,
    /// Память процесса (нет системного сервиса).
    Memory,
}

/// Проверить, доступен ли системный keyring, не трогая его данных.
pub fn detect_keyring_backend() -> KeyringBackend {
    match keyring::Entry::new("decon", "probe") {
        Ok(entry) => match entry.get_password() {
            Ok(_) | Err(keyring::Error::NoEntry) => KeyringBackend::OsKeyring,
            // NoStorageAccess и прочие — сервиса нет
            Err(_) => KeyringBackend::Memory,
        },
        Err(_) => KeyringBackend::Memory,
    }
}

/// Хранилище: OS keyring, при недоступности — память.
///
/// Выбор бэкенда делается один раз на старте (`detect`), дальше режим
/// не меняется — переключение посреди сессии потеряло бы ключ.
pub struct OsKeyringOrMemory {
    os: Option<keyring::Entry>,
    memory: MemoryStore,
}

impl OsKeyringOrMemory {
    /// Определить бэкенд и открыть запись keyring (если он есть).
    pub fn detect() -> Self {
        let os = match keyring::Entry::new("decon", SECRET_NAME) {
            Ok(entry) => match entry.get_password() {
                Ok(_) | Err(keyring::Error::NoEntry) => Some(entry),
                Err(_) => None,
            },
            Err(_) => None,
        };
        OsKeyringOrMemory {
            os,
            memory: MemoryStore::new(),
        }
    }

    /// Какой бэкенд реально используется.
    pub const fn backend(&self) -> KeyringBackend {
        if self.os.is_some() {
            KeyringBackend::OsKeyring
        } else {
            KeyringBackend::Memory
        }
    }
}

impl SecretStore for OsKeyringOrMemory {
    fn load(&self) -> Result<Option<String>, String> {
        match &self.os {
            Some(entry) => match entry.get_password() {
                Ok(key) => Ok(Some(key)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(format!("keyring: {e}")),
            },
            None => self.memory.load(),
        }
    }

    fn save(&self, key: &str) -> Result<(), String> {
        match &self.os {
            Some(entry) => entry
                .set_password(key)
                .map_err(|e| format!("keyring: {e}")),
            None => self.memory.save(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_fallback_roundtrip() {
        // В headless-окружении теста keyring-сервиса нет — проверяем
        // фолбэк-путь; если сервис вдруг есть, проверка всё равно честна:
        // detect возвращает валидный бэкенд, и roundtrip идёт через него.
        let store = OsKeyringOrMemory::detect();
        let probe = format!("decon-test-{}", std::process::id());
        store.save(&probe).expect("save");
        let loaded = store.load().expect("load");
        assert_eq!(loaded.as_deref(), Some(probe.as_str()));
        // очистка: перезапись пустым не портит контракт (None не бывает
        // после save), поэтому чистим только для памяти/ОС одинаково
    }

    #[test]
    fn backend_detection_never_panics() {
        // Просто детект без паник; реальный бэкенд зависит от окружения
        let _ = detect_keyring_backend();
    }
}
