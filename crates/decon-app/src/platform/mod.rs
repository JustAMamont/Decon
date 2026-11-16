//! Платформенный слой: всё, что зависит от ОС (правило №7.4).
//!
//! - `secret_store`: мастер-ключ в OS keyring (Secret Service / Wincred /
//!   Keychain) с фолбэком на память в headless-окружении;
//! - `notify`: уведомления ОС для зондов дня.

pub mod notify;
pub mod secret_store;
