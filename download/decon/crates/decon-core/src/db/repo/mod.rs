//! Репозитории: по одному модулю на таблицу.
//!
//! Репозиторий не владеет подключением — принимает `&Connection`, чтобы
//! UI-поток и поток записи делили одну точку входа в БД.

pub mod event_repo;
pub mod ffqm_repo;
pub mod probe_repo;
pub mod scale_repo;
pub mod session_repo;
pub mod settings_repo;
pub mod trainer_repo;

pub use event_repo::EventRepo;
pub use ffqm_repo::{FfmqRepo, FfmqResult};
pub use probe_repo::ProbeRepo;
pub use scale_repo::ScaleRepo;
pub use session_repo::SessionRepo;
pub use settings_repo::SettingsRepo;
pub use trainer_repo::{
    MotMetricsRecord, SchulteMetricsRecord, TrainerKind, TrainerResult, TrainerResultRepo,
};
