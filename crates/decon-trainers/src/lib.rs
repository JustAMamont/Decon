//! # decon-trainers
//!
//! Движки тренажёров Decon: чистая логика без рендера, детерминизм по сиду.
//! UI-крейт (`decon-app`) тонким слоем поверх потока событий этих движков.
//!
//! Наполняется в S3 (`schulte.rs`, `mot.rs`, `rng.rs`) и S5
//! (`ufov.rs`, `audio_field.rs`, `layer_switch.rs`) — см. docs/ROADMAP.md.

#![deny(missing_docs)]
