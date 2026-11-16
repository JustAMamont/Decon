//! Модель домена: сущности журнала практики дКВ.
//!
//! Каждый модуль — одна сущность (правило №7 из CONVENTIONS.md).

pub mod event;
pub mod journal;
pub mod session;
pub mod settings;

pub use settings::AppSettings;
