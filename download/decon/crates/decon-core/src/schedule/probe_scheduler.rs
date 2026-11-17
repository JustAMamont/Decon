//! Планировщик зондов дня (ESM): N случайных срезов в окне активности,
//! детерминизм по сиду (ARCHITECTURE.md §5).
//!
//! Алгоритм — равномерное прореживание окна: окно делится на N равных
//! сегментов, в каждом сегменте берётся случайная точка ( seeded RNG).
//! Гарантии при `count ≥ 1` и окне, вмещающем `count` зондов с интервалом
//! `min_gap_minutes`: все зонды в окне, соседние интервалы ≥ минимума,
//! количество ровно N, сортировка по возрастанию.

use crate::model::settings::ProbeSettings;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Расписание дня: плановые минуты зондов от полуночи (локальное время).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaySchedule {
    /// Минуты от полуночи, по возрастанию.
    pub minutes: Vec<u16>,
}

/// Ошибка планирования.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// Окно короче, чем нужно для N зондов с мин. интервалом
    /// (`N*min_gap` не влезает).
    #[error("окно {window_minutes} мин короче {count}×{min_gap} мин")]
    WindowTooShort {
        /// Длина окна, мин.
        window_minutes: u16,
        /// Число зондов.
        count: u8,
        /// Минимальный интервал, мин.
        min_gap: u16,
    },
    /// Ноль зондов запрошен (окно бессмысленно).
    #[error("количество зондов = 0")]
    ZeroCount,
}

/// Спланировать день. Сид + настройки → одно и то же расписание
/// (property-тест в `tests/probe_schedule.rs`).
pub fn plan_day(settings: &ProbeSettings, seed: u64) -> Result<DaySchedule, ScheduleError> {
    if settings.count == 0 {
        return Err(ScheduleError::ZeroCount);
    }
    let (start, end) = settings.window.minutes();
    if end <= start {
        return Err(ScheduleError::WindowTooShort {
            window_minutes: 0,
            count: settings.count,
            min_gap: settings.min_gap_minutes,
        });
    }
    let window = end - start; // u16, окно суток ≤ 1440
    let count = settings.count as u16;

    // Требуемая длина: (N-1) интервалов по min_gap, причём строго меньше
    // окна (при window == needed единственная раскладка упирается в end,
    // а зонд в самом конце окна бесполезен). Если не влезает — ошибка
    // конфигурации, а не молчаливое уменьшение N.
    let needed = u32::from(settings.min_gap_minutes).saturating_mul(u32::from(count - 1));
    if u32::from(window) <= needed {
        return Err(ScheduleError::WindowTooShort {
            window_minutes: window,
            count: settings.count,
            min_gap: settings.min_gap_minutes,
        });
    }

    // Равномерное прореживание: сегмент = window / N, точка внутри
    // сегмента — случайно. Сегмент может быть меньше min_gap (напр.
    // 300 мин / 4 = 75 < 90), поэтому после прореживания — детерминированный
    // ремонт: вперёд для min_gap, назад если вылетели за конец окна.
    let seg_len = u32::from(window) / u32::from(count);
    let mut rng = StdRng::seed_from_u64(seed);
    let mut minutes: Vec<u16> = Vec::with_capacity(count as usize);
    for i in 0..count {
        let seg_start = u32::from(start) + u32::from(i) * seg_len;
        let seg_end = if i + 1 < count {
            seg_start + seg_len
        } else {
            u32::from(end) // последний сегмент забирает остаток окна
        };
        // Точка в [seg_start, seg_end): последний зонд строго до конца окна.
        let m = rng.gen_range(seg_start..seg_end);
        minutes.push(m as u16);
    }

    // Ремонт min_gap: слева направо, нарушитель прижимается к prev+gap.
    let min_gap = u32::from(settings.min_gap_minutes);
    let end_u = u32::from(end);
    for i in 1..minutes.len() {
        let prev = u32::from(minutes[i - 1]);
        let want = prev + min_gap;
        if u32::from(minutes[i]) < want {
            minutes[i] = want as u16;
        }
    }
    // Ремонт переполнения: если хвост вылез за end, сдвигаем назад от
    // последнего к первому (end-1, end-1-gap, …). Инвариант window > needed
    // гарантирует, что результат остаётся ≥ start и интервалы сохраняются.
    if u32::from(minutes[minutes.len() - 1]) >= end_u {
        let mut want = end_u - 1;
        for i in (0..minutes.len()).rev() {
            let cur = u32::from(minutes[i]);
            if cur > want {
                minutes[i] = want as u16;
            }
            want = u32::from(minutes[i]).saturating_sub(min_gap);
        }
    }

    debug_assert!(minutes.windows(2).all(|w| w[1] >= w[0]), "сортировка");
    debug_assert!(
        minutes
            .iter()
            .all(|&m| u32::from(m) < end_u && u32::from(m) >= u32::from(start)),
        "в окне"
    );
    Ok(DaySchedule { minutes })
}

/// Моменты срабатывания на день: (минута от полуночи) → ISO-метка времени.
///
/// Дата даётся строкой `YYYY-MM-DD` (локальной); результат — метки
/// `YYYY-MM-DDTHH:MM:00` в локальном времени приложения (UTC-конверсия —
/// на стороне записи в БД, здесь чистая функция от даты и минут).
pub fn day_markers(schedule: &DaySchedule, local_date: &str) -> Vec<String> {
    schedule
        .minutes
        .iter()
        .map(|&m| {
            let h = m / 60;
            let min = m % 60;
            format!("{local_date}T{h:02}:{min:02}:00")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::settings::ProbeWindow;

    fn settings(count: u8, gap: u16, start: (u8, u8), end: (u8, u8)) -> ProbeSettings {
        ProbeSettings {
            enabled: true,
            window: ProbeWindow {
                start_h: start.0,
                start_m: start.1,
                end_h: end.0,
                end_m: end.1,
            },
            count,
            min_gap_minutes: gap,
        }
    }

    #[test]
    fn default_config_produces_4_probes_in_window() {
        let s = settings(4, 90, (9, 0), (23, 0));
        let d = plan_day(&s, 42).unwrap();
        assert_eq!(d.minutes.len(), 4);
        assert!(
            d.minutes.iter().all(|&m| (540..1380).contains(&m)),
            "в окне 09:00–23:00"
        );
        assert!(
            d.minutes.windows(2).all(|w| w[1] - w[0] >= 90),
            "интервалы ≥ 90 мин: {:?}",
            d.minutes
        );
    }

    #[test]
    fn deterministic_by_seed() {
        let s = settings(5, 60, (8, 0), (22, 0));
        let a = plan_day(&s, 1234).unwrap();
        let b = plan_day(&s, 1234).unwrap();
        let c = plan_day(&s, 1235).unwrap();
        assert_eq!(a, b, "один сид — одно расписание");
        assert_ne!(a.minutes, c.minutes, "разные сиды — разные расписания");
    }

    #[test]
    fn rejects_window_too_short() {
        // 3 зонда × 60 мин = 120 мин нужно, окно 60 мин
        let s = settings(3, 60, (10, 0), (11, 0));
        assert_eq!(
            plan_day(&s, 1),
            Err(ScheduleError::WindowTooShort {
                window_minutes: 60,
                count: 3,
                min_gap: 60
            })
        );
    }

    #[test]
    fn rejects_zero_count_and_bad_window() {
        let mut s = settings(0, 30, (9, 0), (23, 0));
        assert_eq!(plan_day(&s, 1), Err(ScheduleError::ZeroCount));
        s = settings(2, 30, (23, 0), (9, 0));
        assert!(matches!(
            plan_day(&s, 1),
            Err(ScheduleError::WindowTooShort { .. })
        ));
    }

    #[test]
    fn tight_window_rejected_and_near_tight_repaired() {
        // Окно ровно (N-1)·min_gap (120 == 120): зонд упёрся бы в конец —
        // ошибка конфигурации.
        let s = settings(3, 60, (10, 0), (12, 0));
        assert!(matches!(
            plan_day(&s, 777),
            Err(ScheduleError::WindowTooShort { .. })
        ));

        // Окно на минуту больше: раскладка существует, ремонт не вылетает.
        let s = settings(3, 60, (10, 0), (12, 1));
        let d = plan_day(&s, 777).unwrap();
        assert_eq!(d.minutes.len(), 3);
        assert!(d.minutes.windows(2).all(|w| w[1] - w[0] >= 60));
        assert!(
            d.minutes.iter().all(|&m| (600..721).contains(&m)),
            "в окне {:?}",
            d.minutes
        );
    }

    #[test]
    fn markers_format_local_time() {
        let s = settings(1, 0, (9, 30), (11, 0));
        let d = plan_day(&s, 5).unwrap();
        let markers = day_markers(&d, "2026-11-18");
        assert_eq!(markers.len(), 1);
        assert!(markers[0].starts_with("2026-11-18T"));
        assert!(markers[0].ends_with(":00"));
        // минута внутри окна
        let hhmm: u16 = markers[0][11..16]
            .split(':')
            .fold(0u16, |acc, part| acc * 60 + part.parse::<u16>().unwrap());
        assert!((570..=660).contains(&hhmm));
    }
}
