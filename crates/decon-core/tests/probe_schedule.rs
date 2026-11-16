//! Property-based тесты планировщика зондов (ROADMAP S2, критерий 1).
//!
//! 1000 случайных конфигураций: все зонды в окне, интервалы ≥ минимума,
//! количество ровно N. Плюс детерминизм: один сид — одно расписание.

use decon_core::model::settings::{ProbeSettings, ProbeWindow};
use decon_core::schedule::{ScheduleError, plan_day};
use proptest::prelude::*;

/// Стратегия валидного окна: start < end, оба в сутках.
fn window_strategy() -> impl Strategy<Value = ProbeWindow> {
    (any::<u16>(), any::<u16>()).prop_map(|(a, b)| {
        let start = a % 1440;
        let mut end = b % 1440;
        if end <= start {
            end = start + 1 + (end % (1440 - start.min(1439)));
            if end > 1440 {
                end = 1440;
            }
        }
        ProbeWindow {
            start_h: (start / 60) as u8,
            start_m: (start % 60) as u8,
            end_h: (end / 60) as u8,
            end_m: (end % 60) as u8,
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// 1000 случайных конфигураций: инварианты расписания.
    #[test]
    fn schedule_invariants(
        count in 1u8..=8,
        min_gap in 0u16..=600,
        window in window_strategy(),
        seed in any::<u64>(),
    ) {
        let (start, end) = window.minutes();
        let settings = ProbeSettings {
            enabled: true,
            window,
            count,
            min_gap_minutes: min_gap,
        };

        match plan_day(&settings, seed) {
            Ok(day) => {
                // 1. Количество ровно N
                prop_assert_eq!(day.minutes.len(), count as usize, "ровно N зондов");

                // 2. Все в окне [start, end)
                prop_assert!(day.minutes.iter().all(|&m| m >= start && m < end),
                    "зонд вне окна: {:?} in [{}, {})", day.minutes, start, end);

                // 3. Соседние интервалы ≥ min_gap
                if min_gap > 0 {
                    prop_assert!(day.minutes.windows(2).all(|w| w[1] - w[0] >= min_gap),
                        "интервал < min_gap: {:?} (gap={})", day.minutes, min_gap);
                }

                // 4. Сортировка по возрастанию
                prop_assert!(day.minutes.windows(2).all(|w| w[1] > w[0] || min_gap == 0),
                    "не отсортировано: {:?}", day.minutes);

                // 5. Детерминизм: тот же сид — то же расписание
                let again = plan_day(&settings, seed).expect("повторный план");
                prop_assert_eq!(day, again, "недетерминизм по сиду");
            }
            Err(ScheduleError::WindowTooShort { .. }) => {
                // Допустимая ошибка только когда окно реально не вмещает
                let window_minutes = end - start;
                let needed = u32::from(min_gap) * u32::from(count - 1);
                prop_assert!(u32::from(window_minutes) <= needed,
                    "ложная WindowTooShort: окно {} > needed {}", window_minutes, needed);
            }
            Err(other) => panic!("неожная ошибка: {other:?}"),
        }
    }

    /// Разные сиды на одной конфигурации дают разные расписания
    /// (распределение точек живое, не вырожденное).
    #[test]
    fn seeds_spread_points(
        seed_a in any::<u64>(),
        seed_b in any::<u64>(),
    ) {
        let settings = ProbeSettings::default();
        if seed_a == seed_b { return Ok(()); }
        let a = plan_day(&settings, seed_a).expect("plan a");
        let b = plan_day(&settings, seed_b).expect("plan b");
        // Не обязаны различаться всегда, но на дефолт-конфиге (4 зонда,
        // 540-мин окно) совпадение всех точек крайне маловероятно.
        prop_assert_ne!(a, b, "два случайных сида дали одно расписание");
    }
}

/// Юнит-проверка: сама стратегия окна всегда даёт start < end ≤ 1440.
/// Используем run — ValueTree-трейт скрыт внутри proptest.
#[test]
fn window_strategy_produces_valid_windows() {
    use proptest::test_runner::TestRunner;
    use std::cell::Cell;
    let ok_count = Cell::new(0usize);
    let mut runner = TestRunner::deterministic();
    let result = runner.run(&window_strategy(), |w| {
        let (start, end) = w.minutes();
        prop_assert!(start < end, "start {start} >= end {end}");
        prop_assert!(end <= 1440);
        ok_count.set(ok_count.get() + 1);
        Ok(())
    });
    result.expect("стратегия валидна");
    assert!(ok_count.get() >= 100, "прогонено {} окон", ok_count.get());
}
