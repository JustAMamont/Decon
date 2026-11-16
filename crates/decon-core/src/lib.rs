//! # decon-core
//!
//! Домен Decon: модель данных, хранилище SQLite, аналитика. Крейт не знает
//! про UI и не содержит async-рантайма — тот же код позже поднимается на
//! сервере (S7). Структура модулей — ARCHITECTURE.md §2.
//!
//! Правила: `#![deny(missing_docs)]`, паники только в тестах.

#![deny(missing_docs)]
// Правило №1.5: unwrap() только в тестах — там он разрешён явно.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod db;
pub mod model;
