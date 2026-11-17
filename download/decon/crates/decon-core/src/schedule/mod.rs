//! Планирование фоновых активностей: зонды дня (ESM).
//!
//! Детерминизм по сиду — обязательное свойство (ARCHITECTURE.md §5):
//! планировщик тестируется property-based, без него никак.

pub mod probe_scheduler;

pub use probe_scheduler::{DaySchedule, ScheduleError, plan_day};
