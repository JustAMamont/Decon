//! Платформенный слой: всё, что зависит от ОС (правило №7.4).
//!
//! Здесь — реализация `SecretStore` поверх OS keyring (Secret Service
//! на Linux через libdbus, Wincred на Windows, Keychain на macOS) с
//! фолбэком на память, когда системного сервиса нет (headless-прогон,
//! CI, первый запуск без dbus-сессии).

pub mod secret_store;
