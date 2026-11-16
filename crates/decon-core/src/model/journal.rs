//! Журнал сессии: живой буфер событий + flush батчами в SQLite.
//!
//! Бюджет (ARCHITECTURE.md §7): append события — 0 аллокаций; событие
//! кладётся в предвыделенный арена-массив, UUID v7 генерируется в счётчик
//! кадра. Flush — одна транзакция на батч.
//!
//! Почему арена, а не `Vec::push`: `Vec` аллоцирует при росте; здесь буфер
//! фиксированной ёмкости `CAPACITY`, переполнение — flush и переиспользование
//! (кольцевого перезапуска нет: сессия дольше 4096 событий — экзотика,
//! но даже тогда корректность сохраняется: батч уходит в БД, буфер
//! продолжается с нуля).

use crate::db::DbError;
use crate::db::repo::EventRepo;
use crate::model::event::{EventKind, SessionEvent};
use uuid::Uuid;

/// Ёмкость буфера событий (4096 × ~64 байта ≈ 256 КБ на сессию).
pub const CAPACITY: usize = 4096;

/// Сырое событие в арене: всё фиксированного размера, ноль String.
#[derive(Debug, Clone, Copy)]
struct Slot {
    t_offset_ms: i64,
    kind: EventKind,
}

/// Журнал событий одной сессии.
///
/// `SessionJournal` не знает про время сам: таймкоды передаёт владелец
/// (таймер сессии), что делает журнал детерминированным в headless-тестах.
pub struct SessionJournal {
    session_id: Uuid,
    slots: Vec<Slot>,
    len: usize,
}

impl SessionJournal {
    /// Новый журнал с предвыделенным буфером.
    pub fn new(session_id: Uuid) -> Self {
        SessionJournal {
            session_id,
            slots: Vec::with_capacity(CAPACITY),
            len: 0,
        }
    }

    /// Добавить событие. 0 аллокаций (слот фикс. размера, буфер уже выделен).
    ///
    /// Возвращает `false`, если буфер полон — владелец должен вызвать
    /// `flush` и повторить.
    pub fn append(&mut self, t_offset_ms: i64, kind: EventKind) -> bool {
        if self.len >= CAPACITY {
            return false;
        }
        // capacity ≥ CAPACITY гарантируется new(); push не аллоцирует,
        // пока len < capacity.
        self.slots.push(Slot { t_offset_ms, kind });
        self.len += 1;
        true
    }

    /// Записать все события в БД одной транзакцией и очистить буфер.
    ///
    /// Аллокации здесь допустимы: flush — не hot path (раз в N секунд).
    pub fn flush(&mut self, repo: &EventRepo<'_>) -> Result<usize, DbError> {
        if self.len == 0 {
            return Ok(0);
        }
        let batch = self.take_batch();
        let n = batch.len();
        repo.insert_batch(&batch)?;
        Ok(n)
    }

    /// Извлечь буфер в вектор готовых событий (без записи в БД) и очистить
    /// его. Путь UI: забрать батч для фонового потока записи.
    pub fn take_batch(&mut self) -> Vec<SessionEvent> {
        self.len = 0;
        self.slots
            .drain(..)
            .map(|s| SessionEvent::new(self.session_id, s.t_offset_ms, s.kind))
            .collect()
    }

    /// Число событий в буфере (без flush).
    pub const fn pending(&self) -> usize {
        self.len
    }

    /// Сессия-владелец журнала.
    pub const fn session_id(&self) -> Uuid {
        self.session_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::repo::SessionRepo;
    use crate::model::session::{Session, SessionKind};

    #[test]
    fn append_zero_allocation_until_capacity() {
        // Косвенная проверка бюджета: append не растит буфер —
        // capacity стабилен, аллокаций нет (Vec::push при len<capacity
        // не аллоцирует; проверяем инвариант).
        let mut j = SessionJournal::new(Uuid::now_v7());
        assert_eq!(j.slots.capacity(), CAPACITY);
        for i in 0..CAPACITY as i64 {
            assert!(j.append(i * 100, EventKind::Signal));
        }
        assert_eq!(j.pending(), CAPACITY);
        // Переполнение отклоняется, не падает
        assert!(!j.append(999_999, EventKind::Pause));
        assert_eq!(j.pending(), CAPACITY);
    }

    #[test]
    fn flush_writes_and_clears() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let s = Session::new(SessionKind::Practice, "2026-11-16T10:00:00.000Z".into(), 10);
        SessionRepo::new(&conn).insert(&s).unwrap();

        let repo = EventRepo::new(&conn);
        let mut j = SessionJournal::new(s.id);
        assert!(j.append(500, EventKind::CollapseFocus));
        assert!(j.append(1_200, EventKind::ClarityFlash));
        assert!(j.append(2_000, EventKind::Signal));
        assert_eq!(j.pending(), 3);

        let n = j.flush(&repo).unwrap();
        assert_eq!(n, 3);
        assert_eq!(j.pending(), 0);

        let stored = repo.list_of_session(&s.id).unwrap();
        assert_eq!(stored.len(), 3);
        assert_eq!(stored[0].t_offset_ms, 500);
        assert_eq!(stored[0].kind, EventKind::CollapseFocus);
        assert_eq!(stored[2].kind, EventKind::Signal);
        // Все ID — v7 и уникальны
        assert!(stored.iter().all(|e| e.id.get_version_num() == 7));
        assert_eq!(
            stored
                .iter()
                .map(|e| e.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );

        // Повторный flush пустого буфера — no-op
        assert_eq!(j.flush(&repo).unwrap(), 0);
    }

    #[test]
    fn flush_after_overflow_recovers() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let s = Session::new(SessionKind::Practice, "2026-11-16T11:00:00.000Z".into(), 11);
        SessionRepo::new(&conn).insert(&s).unwrap();
        let repo = EventRepo::new(&conn);

        let mut j = SessionJournal::new(s.id);
        for i in 0..CAPACITY as i64 {
            assert!(j.append(i, EventKind::Signal));
        }
        assert!(!j.append(0, EventKind::Pause), "буфер полон");
        j.flush(&repo).unwrap();
        // После flush буфер снова принимает события
        assert!(j.append(0, EventKind::Pause));
        assert_eq!(j.pending(), 1);
        j.flush(&repo).unwrap();
        let stored = repo.list_of_session(&s.id).unwrap();
        assert_eq!(stored.len(), CAPACITY + 1);
    }
}
