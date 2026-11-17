//! Property-based тесты математики аналитики (S4, критерий 1).
//!
//! Синтетические ряды с известными свойствами:
//! - монотонная связь → Спирмен = 1 (независимо от формы);
//! - линейная связь → Пирсон ≈ ±1;
//! - шум по фиксированному сиду → воспроизводимость результата;
//! - SMA сохраняет среднее ряда (при окне ≥ длины) и монотонность
//!   сглаживания.

use decon_core::analysis::correlation::{
    CorrKind, RankBuf, correlation_missing, pearson, spearman_buf,
};
use decon_core::analysis::delta::{delta_summary, session_delta};
use decon_core::analysis::n_of_1::compare_periods;
use decon_core::analysis::trends::{Segment, sma};
use decon_core::db::repo::scale_repo::SeriesPoint;
use proptest::prelude::*;
// `vec` из прелюдии proptest конфликтует с макросом std::vec —
// берём генератор коллекций по полному пути.
use proptest::collection::vec as pvec;

fn pt(value: f64, version: i64) -> SeriesPoint {
    SeriesPoint {
        answered_at: String::new(),
        value,
        delay_minutes: None,
        version,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Монотонная (не обязательно линейная) связь → Спирмен = 1.
    #[test]
    fn spearman_monotonic_is_one(
        xs in pvec(0.0f64..10.0, 3..100),
        transform in 0u8..4,
    ) {
        // y = f(x): строго монотонные преобразования
        let mut x = xs.clone();
        x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Дедупликация: строгая монотонность требует различных x
        x.dedup();
        prop_assume!(x.len() >= 3);
        let y: Vec<f64> = x.iter().map(|v| match transform {
            0 => v * v,               // квадрат — выпуклая
            1 => v.sqrt(),            // корень — вогнутая
            2 => -v,                  // убывающая → ρ = -1
            _ => v.ln() + 1.0,        // логарифм
        }).collect();
        let mut buf = RankBuf::new();
        let r = spearman_buf(&x, &y, &mut buf);
        let expected = if transform == 2 { -1.0 } else { 1.0 };
        prop_assert!((r - expected).abs() < 1e-9, "r={r}");
    }

    /// Линейная связь + шум → Пирсон близок к 1 при слабом шуме,
    /// и строго убывает по силе шума? Проверяем скромнее: без шума = 1.
    #[test]
    fn pearson_exact_linear(
        base in 0.0f64..10.0,
        slope in -5.0f64..5.0,
        n in 3usize..60,
    ) {
        prop_assume!(slope.abs() > 1e-6);
        let x: Vec<f64> = (0..n).map(|i| base + i as f64).collect();
        let y: Vec<f64> = x.iter().map(|v| slope * v + 1.0).collect();
        let r = pearson(&x, &y);
        prop_assert!((r - slope.signum()).abs() < 1e-9, "r={r}");
    }

    /// Шум с фиксированным сидом → воспроизводимый коэффициент:
    /// два вызова на одинаковых данных дают бит-в-бит одинаковый r.
    #[test]
    fn noise_seed_reproducible(
        seed in any::<u64>(),
        n in 10usize..500,
    ) {
        // Детерминированный генератор шума: LCG
        let mut state = seed.wrapping_mul(0x9E3779B97F4A7C15).max(1);
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as f64 / (u64::MAX >> 33) as f64
        };
        let x: Vec<f64> = (0..n).map(|_| next() * 10.0).collect();
        let y: Vec<f64> = (0..n).map(|_| next() * 10.0).collect();
        let mut buf1 = RankBuf::new();
        let mut buf2 = RankBuf::new();
        let r1 = spearman_buf(&x, &y, &mut buf1);
        let r2 = spearman_buf(&x, &y, &mut buf2);
        prop_assert_eq!(r1, r2);
        // |r| в разумных пределах
        prop_assert!(r1.abs() <= 1.0);
        // n-of-1: тот же сид → тот же p-value
        let a: Vec<f64> = x.iter().take(n / 2).cloned().collect();
        let b: Vec<f64> = x.iter().skip(n / 2).cloned().collect();
        match (
            compare_periods(&a, &b, seed, 500),
            compare_periods(&a, &b, seed, 500),
        ) {
            (Some(r1), Some(r2)) => prop_assert_eq!(r1.p_value, r2.p_value),
            (None, None) => {}
            _ => prop_assert!(false, "determinism broken"),
        }
    }

    /// SMA с окном ≥ длины ряда → среднее по всему ряду в каждой точке.
    #[test]
    fn sma_window_ge_length_is_mean(
        values in pvec(0.0f64..10.0, 1..50),
    ) {
        let pts: Vec<SeriesPoint> = values.iter().map(|v| pt(*v, 1)).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let mut out = Vec::new();
        sma(&pts, values.len() + 10, &mut out);
        prop_assert_eq!(out.len(), values.len());
        for t in out {
            prop_assert!((t.value - mean).abs() < 1e-9);
        }
    }

    /// SMA не меняет сумму заметно: среднее SMA-кривой ≈ среднее ряда
    /// при любом окне (краевые эффекты малы для длинных рядов).
    #[test]
    fn sma_preserves_mean_approx(
        values in pvec(0.0f64..10.0, 60..200),
        window in 2usize..30,
    ) {
        let pts: Vec<SeriesPoint> = values.iter().map(|v| pt(*v, 1)).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let mut out = Vec::new();
        sma(&pts, window, &mut out);
        let sma_mean = out.iter().map(|t| t.value).sum::<f64>() / out.len() as f64;
        prop_assert!((sma_mean - mean).abs() < 0.5, "sma_mean={sma_mean} mean={mean}");
    }

    /// Дельты: средняя дельта = средняя post − средняя pre.
    #[test]
    fn delta_mean_identity(
        pres in pvec(0.0f64..10.0, 1..50),
        posts in pvec(0.0f64..10.0, 1..50),
    ) {
        let n = pres.len().min(posts.len());
        let deltas: Vec<_> = (0..n)
            .filter_map(|i| session_delta(Some(pres[i]), Some(posts[i])))
            .collect();
        let s = delta_summary(&deltas);
        let mean_post = posts[..n].iter().sum::<f64>() / n as f64;
        let mean_pre = pres[..n].iter().sum::<f64>() / n as f64;
        prop_assert!((s.mean_change - (mean_post - mean_pre)).abs() < 1e-9);
    }

    /// n-of-1: раздвинутые распределения → значимый результат
    /// (p < 0.01) при достаточных выборках.
    #[test]
    fn n_of_1_separated_significant(
        seed in any::<u64>(),
        n in 35usize..60,
        shift in 2.0f64..5.0,
    ) {
        let mut state = seed.max(1);
        let mut next = || {
            state = state.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
            (state >> 33) as f64 / (u64::MAX >> 33) as f64
        };
        let a: Vec<f64> = (0..n).map(|_| 3.0 + next() * 2.0).collect();
        let b: Vec<f64> = (0..n).map(|_| 3.0 + shift + next() * 2.0).collect();
        let r = compare_periods(&a, &b, seed, 2000).expect("non-empty");
        prop_assert!(!r.low_sample, "n={n} не должен быть малой выборкой");
        prop_assert!(r.p_value < 0.01, "p={} при shift={shift}", r.p_value);
        prop_assert_eq!(r.direction, "improved");
    }

    /// Correlation с пропусками: pair = ∩ полных позиций всегда.
    #[test]
    fn missing_pairs_count_consistent(
        x in pvec(proptest::option::of(0.0f64..10.0), 5..60),
        y in pvec(proptest::option::of(0.0f64..10.0), 5..60),
    ) {
        let mut buf = RankBuf::new();
        let c = correlation_missing(&x, &y, &mut buf, CorrKind::Spearman);
        let expected = x
            .iter()
            .zip(y.iter())
            .filter(|(a, b)| a.is_some() && b.is_some())
            .count();
        prop_assert_eq!(c.n, expected);
        if expected >= 5 {
            prop_assert!(c.rho.abs() <= 1.0);
        } else {
            prop_assert!(c.rho.is_nan());
        }
    }

    /// Сегментация по версиям: количество смен версий = числу границ,
    /// сегменты покрывают весь ряд без пропусков и дублирования.
    #[test]
    fn version_split_covers_all(
        values in pvec(0.0f64..10.0, 1..40),
        versions in pvec(1i64..4, 1..40),
    ) {
        let n = values.len().min(versions.len());
        let pts: Vec<SeriesPoint> = (0..n)
            .map(|i| pt(values[i], versions[i]))
            .collect();
        let segs: Vec<Segment> = decon_core::analysis::trends::split_by_version(&pts);
        prop_assert_eq!(segs.iter().map(|s| s.points.len()).sum::<usize>(), n);
        // Границы сегментов — именно смены версии
        for w in segs.windows(2) {
            prop_assert_ne!(w[0].version, w[1].version);
        }
    }
}
