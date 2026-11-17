//! Событие сессии с таймкодом — event-лог практики.
//!
//! DDL — `session_events` в SPEC §3. Кнопки-маркеры в UI пишут события
//! без остановки сессии; `t_offset_ms` — миллисекунды от старта.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Тип события сессии.
///
/// Семантика — ARCHITECTURE.md §8: «срыв» — рецентрация внимания.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Срыв: рецентрация на объекте (уход из дКВ в концентрацию).
    CollapseFocus,
    /// Уход в семантику (мысли, смыслы).
    DriftSemantic,
    /// Уход в локальные телесные ощущения.
    DriftBody,
    /// Вспышка ясности/равномерности.
    ClarityFlash,
    /// Пауза (служебное).
    Pause,
    /// Возобновление (служебное).
    Resume,
    /// Равномерный фоновый сигнал (служебное, пишется таймером).
    Signal,
}

impl EventKind {
    /// Код для колонки `kind` (совпадает с CHECK в DDL).
    pub const fn as_code(self) -> &'static str {
        match self {
            EventKind::CollapseFocus => "collapse_focus",
            EventKind::DriftSemantic => "drift_semantic",
            EventKind::DriftBody => "drift_body",
            EventKind::ClarityFlash => "clarity_flash",
            EventKind::Pause => "pause",
            EventKind::Resume => "resume",
            EventKind::Signal => "signal",
        }
    }

    /// Обратное преобразование из кода БД.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "collapse_focus" => Some(EventKind::CollapseFocus),
            "drift_semantic" => Some(EventKind::DriftSemantic),
            "drift_body" => Some(EventKind::DriftBody),
            "clarity_flash" => Some(EventKind::ClarityFlash),
            "pause" => Some(EventKind::Pause),
            "resume" => Some(EventKind::Resume),
            "signal" => Some(EventKind::Signal),
            _ => None,
        }
    }
}

/// Событие сессии — строка `session_events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    /// UUID v7.
    pub id: Uuid,
    /// Сессия-владелец.
    pub session_id: Uuid,
    /// Таймкод от старта сессии, мс.
    pub t_offset_ms: i64,
    /// Тип события.
    pub kind: EventKind,
    /// Открытый текст заметки к событию (шифруется при записи, 🔒).
    pub note: Option<String>,
}

impl SessionEvent {
    /// Новое событие с генерацией ID.
    pub fn new(session_id: Uuid, t_offset_ms: i64, kind: EventKind) -> Self {
        SessionEvent {
            id: Uuid::now_v7(),
            session_id,
            t_offset_ms,
            kind,
            note: None,
        }
    }

    /// Создать событие с заметкой (кнопка-маркер + расшифровка).
    pub fn with_note(mut self, note: Option<String>) -> Self {
        self.note = note;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_codes_roundtrip() {
        let all = [
            EventKind::CollapseFocus,
            EventKind::DriftSemantic,
            EventKind::DriftBody,
            EventKind::ClarityFlash,
            EventKind::Pause,
            EventKind::Resume,
            EventKind::Signal,
        ];
        assert_eq!(all.len(), 7, "CHECK в DDL допускает ровно эти 7");
        for kind in all {
            assert_eq!(EventKind::from_code(kind.as_code()), Some(kind));
        }
        assert_eq!(EventKind::from_code("bogus"), None);
    }

    #[test]
    fn new_event_has_v7_id() {
        let e = SessionEvent::new(Uuid::now_v7(), 1_500, EventKind::ClarityFlash);
        assert_eq!(e.id.get_version_num(), 7);
        assert_eq!(e.t_offset_ms, 1_500);
    }
}
