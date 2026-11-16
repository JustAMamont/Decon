//! Зонд дня (ESM): случайный срез состояния в повседневности.
//!
//! Модель — ARCHITECTURE.md §5, DDL — SPEC §3 (`probes`). Пропущенный зонд
//! остаётся доступным до конца дня (догоняющий режим), ответ сохраняется
//! с `delay_minutes`, чтобы аналитика отделяла свежие срезы от
//! восстановленных (SCALES.md §5).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Состояние жизненного цикла зонда (колонка `window_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    /// Запланирован (момент времени ещё не наступил).
    Scheduled,
    /// Показан (уведомление отправлено), ждёт ответа.
    Fired,
    /// Отвечен.
    Answered,
    /// Истёк в полночь без ответа (данные честности).
    Missed,
    /// Пользователь явно пропустил.
    Skipped,
}

impl ProbeState {
    /// Код для колонки `window_state`.
    pub const fn as_code(self) -> &'static str {
        match self {
            ProbeState::Scheduled => "scheduled",
            ProbeState::Fired => "fired",
            ProbeState::Answered => "answered",
            ProbeState::Missed => "missed",
            ProbeState::Skipped => "skipped",
        }
    }

    /// Из кода БД.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "scheduled" => Some(ProbeState::Scheduled),
            "fired" => Some(ProbeState::Fired),
            "answered" => Some(ProbeState::Answered),
            "missed" => Some(ProbeState::Missed),
            "skipped" => Some(ProbeState::Skipped),
            _ => None,
        }
    }
}

/// Зонд дня — строка `probes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    /// UUID v7.
    pub id: Uuid,
    /// Плановое время (ISO-8601 UTC; окно — локальное, хранится в настройках).
    pub scheduled_at: String,
    /// Когда показано уведомление.
    pub fired_at: Option<String>,
    /// Когда отвечено.
    pub answered_at: Option<String>,
    /// Задержка ответа от планового времени, мин (>0 для догоняющих).
    pub delay_minutes: Option<i64>,
    /// Состояние.
    pub state: ProbeState,
}

impl Probe {
    /// Новый запланированный зонд.
    pub fn scheduled(scheduled_at: String) -> Self {
        Probe {
            id: Uuid::now_v7(),
            scheduled_at,
            fired_at: None,
            answered_at: None,
            delay_minutes: None,
            state: ProbeState::Scheduled,
        }
    }

    /// Зонд показан (уведомление отправлено).
    pub fn fire(&mut self, fired_at: String) -> bool {
        if self.state != ProbeState::Scheduled {
            return false;
        }
        self.fired_at = Some(fired_at);
        self.state = ProbeState::Fired;
        true
    }

    /// Ответить на зонд. `delay_minutes` = разница между ответом и плановым
    /// временем; 0 или None — свежий ответ (в окно ±5 мин), >0 — догоняющий.
    pub fn answer(&mut self, answered_at: String, delay_minutes: Option<i64>) -> bool {
        if self.state == ProbeState::Answered || self.state == ProbeState::Missed {
            return false;
        }
        self.answered_at = Some(answered_at);
        self.delay_minutes = delay_minutes;
        self.state = ProbeState::Answered;
        true
    }

    /// Полночь прошла без ответа → missed (данные честности).
    pub fn expire(&mut self) -> bool {
        if self.state == ProbeState::Answered {
            return false;
        }
        self.state = ProbeState::Missed;
        true
    }

    /// Доступен для догоняющего ответа (показан, но не отвечен).
    pub fn is_catchable(&self) -> bool {
        self.state == ProbeState::Fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_scheduled_fire_answer() {
        let mut p = Probe::scheduled("2026-11-17T10:15:00.000Z".into());
        assert_eq!(p.state, ProbeState::Scheduled);
        assert!(!p.is_catchable());

        assert!(p.fire("2026-11-17T10:15:02.000Z".into()));
        assert_eq!(p.state, ProbeState::Fired);
        assert!(p.is_catchable());

        // Свежий ответ: задержки нет
        assert!(p.answer("2026-11-17T10:16:40.000Z".into(), None));
        assert_eq!(p.state, ProbeState::Answered);
        assert!(p.delay_minutes.is_none());

        // Повторный ответ/огонь невозможны
        assert!(!p.answer("2026-11-17T12:00:00.000Z".into(), Some(105)));
        assert!(!p.fire("2026-11-17T12:00:00.000Z".into()));
        assert!(!p.expire());
    }

    #[test]
    fn catchup_answer_marks_delay() {
        let mut p = Probe::scheduled("2026-11-17T10:15:00.000Z".into());
        assert!(p.fire("2026-11-17T10:15:02.000Z".into()));
        // Догоняющий ответ через 105 минут
        assert!(p.answer("2026-11-17T12:00:00.000Z".into(), Some(105)));
        assert_eq!(p.delay_minutes, Some(105));
        assert_eq!(p.state, ProbeState::Answered);
    }

    #[test]
    fn midnight_expiry_honesty() {
        let mut p = Probe::scheduled("2026-11-17T22:30:00.000Z".into());
        assert!(p.fire("2026-11-17T22:30:01.000Z".into()));
        assert!(p.expire());
        assert_eq!(p.state, ProbeState::Missed);
        // missed уже не отвечает
        assert!(!p.answer("2026-11-18T08:00:00.000Z".into(), Some(570)));
    }

    #[test]
    fn state_codes_roundtrip() {
        for s in [
            ProbeState::Scheduled,
            ProbeState::Fired,
            ProbeState::Answered,
            ProbeState::Missed,
            ProbeState::Skipped,
        ] {
            assert_eq!(ProbeState::from_code(s.as_code()), Some(s));
        }
        assert_eq!(ProbeState::from_code("bogus"), None);
    }
}
