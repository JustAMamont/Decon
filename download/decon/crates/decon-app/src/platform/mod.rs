//! Платформенный слой: всё, что зависит от ОС (правило №7.4).
//!
//! - `secret_store`: мастер-ключ в OS keyring (Secret Service / Wincred /
//!   Keychain) с фолбэком на память в headless-окружении;
//! - `notify`: уведомления ОС для зондов дня;
//! - `sound`: процедурные звуки оповещений (S2.5);
//! - `autostart`: запуск вместе с системой — XDG / HKCU Run (S2.5);
//! - `tray`: сигнал-флаг завершения и иконка трея (S2.5).

pub mod autostart;
pub mod notify;
pub mod secret_store;
pub mod sound;
pub mod tray;
