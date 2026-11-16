//! # decon-trainers
//!
//! Движки тренажёров Decon: чистая логика без рендера, детерминизм по сиду.
//! UI-крейт (`decon-app`) тонким слоем поверх потока событий этих движков.
//!
//! Контракты метрик — SPEC-data-model.md §4 (JSON с фиксированными
//! ключами, неизвестные отклоняются на уровне serde). Бюджет аллокаций
//! — ARCHITECTURE.md §7: тик любого тренажёра — 0 аллокаций.
//!
//! Наполняется в S3 (`schulte.rs`, `mot.rs`, `rng.rs`) и S5
//! (`ufov.rs`, `audio_field.rs`, `layer_switch.rs`) — см. docs/ROADMAP.md.

#![deny(missing_docs)]

pub mod mot;
pub mod rng;
pub mod schulte;
