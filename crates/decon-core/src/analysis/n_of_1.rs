//! N-of-1 сравнение периодов A/B (S4).
//!
//! Классическая ловушка самонаблюдения: «на новой технике стало
//! лучше». Проверка — сравнение двух периодов одного пользователя
//! (baseline A ↔ вмешательство B) с permutation-тестом: если
//! перемешивать метки периодов, насколько часто разница средних
//! получается не меньше наблюдаемой? Частота — это и есть
//! «видимая значимость» (p-value точного пермутационного теста,
//! без нормальности и прочих предположений).
//!
//! Трезвость (ROADMAP S4, критерий 4): при n < 30 в периоде любой
//! результат обязан выводиться с предупреждением «мало данных» —
//! `n_of_1` возвращает флаг `low_sample`, UI показывает плашку.
//! Статистика не обещает каузальность: периоды отличаются не только
//! техникой (сезон, сон, настроение).
//!
//! RNG — тот же `StdRng::seed_from_u64`, что в планировщике зондов:
//! фиксированный сид даёт воспроизводимый p-value.

use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

/// Порог малой выборки: ниже — предупреждение «мало данных»
/// (ROADMAP S4, критерий 4: n < 30 в периоде).
pub const LOW_SAMPLE_THRESHOLD: usize = 30;

/// Дефолт числа перестановок: 10_000 даёт p с шагом 1e-4 — за глаза
/// для n-of-1; больше — жечь CPU впустую.
pub const DEFAULT_PERMUTATIONS: usize = 10_000;

/// Результат сравнения периодов A/B.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbResult {
    /// Число наблюдений в периоде A.
    pub n_a: usize,
    /// Число наблюдений в периоде B.
    pub n_b: usize,
    /// Среднее A.
    pub mean_a: f64,
    /// Среднее B.
    pub mean_b: f64,
    /// Разница (B − A).
    pub mean_diff: f64,
    /// Двусторонний p-value permutation-теста (NaN при пустом периоде).
    pub p_value: f64,
    /// Мало данных (n < 30 хотя бы в одном периоде) — UI обязан
    /// показать предупреждение.
    pub low_sample: bool,
    /// Направление для UI: `improved`/`worsened`/`flat`/`insufficient`.
    pub direction: &'static str,
}

/// Сравнить периоды A и B: значения шкалы по сессиям каждого периода.
///
/// `seed` — сид перестановок: фиксированный сид даёт воспроизводимый
/// p-value (свойство теста S4 №1: «шум с сидом → воспроизводимость»).
/// Возвращает None при пустом периоде.
pub fn compare_periods(a: &[f64], b: &[f64], seed: u64, permutations: usize) -> Option<AbResult> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let permutations = permutations.max(100);
    let mean_a = a.iter().sum::<f64>() / a.len() as f64;
    let mean_b = b.iter().sum::<f64>() / b.len() as f64;
    let observed = mean_b - mean_a;

    // Объединённый пул для перестановок: кладём в один буфер, чтобы
    // шафлить без аллокаций на итерацию (одна аллокация на вызов).
    let mut pool: Vec<f64> = Vec::with_capacity(a.len() + b.len());
    pool.extend_from_slice(a);
    pool.extend_from_slice(b);
    let n_a = a.len();
    let total = pool.len();

    let mut rng = StdRng::seed_from_u64(seed);
    let mut extreme = 0usize;
    for _ in 0..permutations {
        // Fisher–Yates по пулу
        pool.shuffle(&mut rng);
        // Средние «периодов» после перемешивания меток
        let mut sa = 0.0;
        for v in &pool[..n_a] {
            sa += v;
        }
        let mut sb = 0.0;
        for v in &pool[n_a..] {
            sb += v;
        }
        let diff = sb / (total - n_a) as f64 - sa / n_a as f64;
        if diff.abs() >= observed.abs() - f64::EPSILON {
            extreme += 1;
        }
    }
    let p_value = extreme as f64 / permutations as f64;
    let low_sample = a.len() < LOW_SAMPLE_THRESHOLD || b.len() < LOW_SAMPLE_THRESHOLD;
    let direction = if low_sample {
        "insufficient"
    } else if p_value > 0.05 {
        "flat"
    } else if observed > 0.0 {
        "improved"
    } else {
        "worsened"
    };
    Some(AbResult {
        n_a: a.len(),
        n_b: b.len(),
        mean_a,
        mean_b,
        mean_diff: observed,
        p_value,
        low_sample,
        direction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separated_distributions_significant() {
        // A: 3..4, B: 7..8 — раздвинутые распределения
        let a: Vec<f64> = (0..40).map(|i| 3.0 + (i % 10) as f64 / 10.0).collect();
        let b: Vec<f64> = (0..40).map(|i| 7.0 + (i % 10) as f64 / 10.0).collect();
        let r = compare_periods(&a, &b, 42, 5000).unwrap();
        assert!(!r.low_sample);
        assert!(r.p_value < 0.001, "p={}", r.p_value);
        assert_eq!(r.direction, "improved");
        assert!((r.mean_diff - 4.0).abs() < 0.11);
    }

    #[test]
    fn same_distribution_flat() {
        // Одинаковые ряды — p близок к 1 (разница 0 всегда «не меньше»)
        let a: Vec<f64> = (0..40).map(|i| (i % 10) as f64).collect();
        let r = compare_periods(&a, &a.clone(), 42, 2000).unwrap();
        assert_eq!(r.direction, "flat");
        assert!(r.p_value > 0.9);
        assert!(r.mean_diff.abs() < 1e-12);
    }

    #[test]
    fn low_sample_flag() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![7.0, 8.0, 9.0];
        let r = compare_periods(&a, &b, 1, 500).unwrap();
        assert!(r.low_sample);
        assert_eq!(r.direction, "insufficient");
        assert_eq!(r.n_a, 3);
        assert_eq!(r.n_b, 3);
    }

    #[test]
    fn empty_period_none() {
        assert!(compare_periods(&[], &[1.0], 1, 100).is_none());
        assert!(compare_periods(&[1.0], &[], 1, 100).is_none());
        assert!(compare_periods(&[], &[], 1, 100).is_none());
    }

    #[test]
    fn deterministic_p_value() {
        // Критерий №1: один сид → одинаковый p (шум с сидом).
        let a: Vec<f64> = (0..30).map(|i| (i * 3 % 11) as f64).collect();
        let b: Vec<f64> = (0..30).map(|i| (i * 5 % 11 + 2) as f64).collect();
        let r1 = compare_periods(&a, &b, 777, 2000).unwrap();
        let r2 = compare_periods(&a, &b, 777, 2000).unwrap();
        assert_eq!(r1.p_value, r2.p_value);
        // Другой сид — может отличаться в пределах шага 1/perm
        let r3 = compare_periods(&a, &b, 778, 2000).unwrap();
        assert!((r3.p_value - r1.p_value).abs() < 0.05);
    }

    #[test]
    fn worsened_direction() {
        // B хуже A и значимо
        let a: Vec<f64> = (0..40).map(|i| 8.0 + (i % 5) as f64 / 10.0).collect();
        let b: Vec<f64> = (0..40).map(|i| 2.0 + (i % 5) as f64 / 10.0).collect();
        let r = compare_periods(&a, &b, 9, 3000).unwrap();
        assert_eq!(r.direction, "worsened");
        assert!(r.p_value < 0.001);
    }
}
