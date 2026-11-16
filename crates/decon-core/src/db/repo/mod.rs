//! Репозитории: по одному модулю на таблицу.
//!
//! Репозиторий не владеет подключением — принимает `&Connection`, чтобы
//! UI-поток и поток записи делили одну точку входа в БД.

pub mod event_repo;
pub mod session_repo;
pub mod settings_repo;

pub use event_repo::EventRepo;
pub use session_repo::SessionRepo;
pub use settings_repo::SettingsRepo;
