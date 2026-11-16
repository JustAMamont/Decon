//! Pre/post дельты и carryover-кривая (S4).
//!
//! Дельта — разница post − pre по шкалам одной сессии (или паре
//! «зонд до / зонд после», если сессия была между ними). Carryover —
//! динамика остаточного фона (`post.carryover`) по зондам: сколько
//! минут после сессии состояние ещё держится.
//!
//! Все функции принимают готовые ряды значений (репозиторий собирает
//! их SQL-ем) и не ходят в БД — чистая математика, property-тесты
//! в `tests/analysis_props.rs`.

use serde::{Deserialize, Serialize};

/// Дельта одной сессии по одной шкале.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    /// Pre-значение (0..=10).
    pub pre: f64,
    /// Post-значение (0..=10).
    pub post: f64,
    /// post − pre (−10..=10).
    pub change: f64,
}

/// Сводка дельт по ряду сессий: средняя дельта + разброс.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DeltaSummary {
    /// Число сессий с обеими оценками.
    pub n: usize,
    /// Средняя дельта (NaN при n=0).
    pub mean_change: f64,
    /// Стандартное отклонение дельт (NaN при n<2; популяционное, σ²).
    pub sd_change: f64,
}

/// Дельта одной сессии; None, если нет одной из оценок.
pub const fn session_delta(pre: Option<f64>, post: Option<f64>) -> Option<Delta> {
    match (pre, post) {
        (Some(pre), Some(post)) => Some(Delta {
            pre,
            post,
            change: post - pre,
        }),
        _ => None,
    }
}

/// Сводка по списку дельт (None-ы уже отфильтрованы вызывающим).
///
/// 0 аллокаций — один проход по слайсу.
pub fn delta_summary(deltas: &[Delta]) -> DeltaSummary {
    let n = deltas.len();
    if n == 0 {
        return DeltaSummary {
            n: 0,
            mean_change: f64::NAN,
            sd_change: f64::NAN,
        };
    }
    let mut sum = 0.0;
    for d in deltas {
        sum += d.change;
    }
    let mean = sum / n as f64;
    if n < 2 {
        return DeltaSummary {
            n,
            mean_change: mean,
            sd_change: f64::NAN,
        };
    }
    let mut ss = 0.0;
    for d in deltas {
        let dx = d.change - mean;
        ss += dx * dx;
    }
    DeltaSummary {
        n,
        mean_change: mean,
        sd_change: (ss / n as f64).sqrt(),
    }
}

/// Точка carryover-кривой: «прошло X минут после сессии — фон ещё
/// такой-то».
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CarryoverPoint {
    /// Минут после сессии.
    pub minutes_after: f64,
    /// Значение остаточного фона (0..=10).
    pub value: f64,
}

/// Carryover-кривая из пар (минуты, значение).
///
/// Зонды, отвеченные до сессии, отфильтровываются по `minutes_after >= 0`
/// (отрицательные — опечатки порядка, их отбрасываем молча: источник —
/// ручной ввод времени в пилоте). Ряд сортируется по времени: точки
/// кривой идут в хронологическом порядке.
///
/// Аллокация — только sort на месте переданного mutable slice.
pub fn carryover_curve(points: &mut Vec<CarryoverPoint>) {
    points.retain(|p| p.minutes_after.is_finite() && p.minutes_after >= 0.0);
    points.sort_by(|a, b| {
        a.minutes_after
            .partial_cmp(&b.minutes_after)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Период полураспада остаточного фона (экспоненциальная модель) —
/// грубая характеристика «сколько держится эффект». Метод: линейная
/// регрессия ln(value) по minutes_after, t½ = ln(2)/|k|.
///
/// None — если регрессия не строится (меньше 3 точек, нулевые
/// значения — логарифм не определён — или наклон ≥ 0: фон не угасает).
pub fn carryover_halflife(points: &[CarryoverPoint]) -> Option<f64> {
    // Логарифмируемые точки: value > 0
    let usable: Vec<(f64, f64)> = points
        .iter()
        .filter(|p| p.value > 0.0 && p.value <= 10.0)
        .map(|p| (p.minutes_after, p.value.ln()))
        .collect();
    if usable.len() < 3 {
        return None;
    }
    // OLS: k = cov(x, y) / var(x)
    let n = usable.len() as f64;
    let mut sx = 0.0;
    let mut sy = 0.0;
    for (x, y) in &usable {
        sx += x;
        sy += y;
    }
    let mx = sx / n;
    let my = sy / n;
    let mut cov = 0.0;
    let mut var = 0.0;
    for (x, y) in &usable {
        let dx = x - mx;
        cov += dx * (y - my);
        var += dx * dx;
    }
    if var <= 0.0 {
        return None;
    }
    let k = cov / var;
    if k >= 0.0 {
        return None; // не угасает — полураспада нет
    }
    Some(std::f64::consts::LN_2 / -k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_delta_cases() {
        let d = session_delta(Some(3.0), Some(8.0)).unwrap();
        assert_eq!(d.change, 5.0);
        assert!(session_delta(None, Some(8.0)).is_none());
        assert!(session_delta(Some(3.0), None).is_none());
        assert!(session_delta(None, None).is_none());
    }

    #[test]
    fn summary_empty_and_single() {
        let s = delta_summary(&[]);
        assert_eq!(s.n, 0);
        assert!(s.mean_change.is_nan());
        let s1 = delta_summary(&[session_delta(Some(1.0), Some(3.0)).unwrap()]);
        assert_eq!(s1.n, 1);
        assert!((s1.mean_change - 2.0).abs() < 1e-12);
        assert!(s1.sd_change.is_nan());
    }

    #[test]
    fn summary_stats() {
        let ds = [
            session_delta(Some(0.0), Some(2.0)).unwrap(), // +2
            session_delta(Some(0.0), Some(4.0)).unwrap(), // +4
            session_delta(Some(0.0), Some(6.0)).unwrap(), // +6
        ];
        let s = delta_summary(&ds);
        assert_eq!(s.n, 3);
        assert!((s.mean_change - 4.0).abs() < 1e-12);
        // Популяционное SD ряда [2,4,6] = sqrt(8/3)
        assert!((s.sd_change - (8.0f64 / 3.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn carryover_filters_and_sorts() {
        let mut pts = vec![
            CarryoverPoint {
                minutes_after: 45.0,
                value: 4.0,
            },
            CarryoverPoint {
                minutes_after: 5.0,
                value: 8.0,
            },
            CarryoverPoint {
                minutes_after: -10.0,
                value: 9.0,
            }, // до сессии
            CarryoverPoint {
                minutes_after: 20.0,
                value: 6.0,
            },
        ];
        carryover_curve(&mut pts);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[0].minutes_after, 5.0);
        assert_eq!(pts[1].minutes_after, 20.0);
        assert_eq!(pts[2].minutes_after, 45.0);
    }

    #[test]
    fn halflife_exponential_decay() {
        // Идеальная экспонента с t½ = 30 мин: value = 8 * exp(-ln2 * t/30)
        let pts: Vec<CarryoverPoint> = [0.0, 10.0, 20.0, 30.0, 60.0]
            .iter()
            .map(|&t| CarryoverPoint {
                minutes_after: t,
                value: 8.0 * (-std::f64::consts::LN_2 * t / 30.0).exp(),
            })
            .collect();
        let th = carryover_halflife(&pts).unwrap();
        assert!((th - 30.0).abs() < 1.0, "t½={th}");
    }

    #[test]
    fn halflife_degenerate() {
        // Меньше 3 точек
        let short = vec![
            CarryoverPoint {
                minutes_after: 0.0,
                value: 5.0,
            },
            CarryoverPoint {
                minutes_after: 10.0,
                value: 4.0,
            },
        ];
        assert!(carryover_halflife(&short).is_none());
        // Растущий фон — наклон положительный
        let growing = vec![
            CarryoverPoint {
                minutes_after: 0.0,
                value: 1.0,
            },
            CarryoverPoint {
                minutes_after: 10.0,
                value: 5.0,
            },
            CarryoverPoint {
                minutes_after: 20.0,
                value: 9.0,
            },
        ];
        assert!(carryover_halflife(&growing).is_none());
        // Нули (логарифм не определён) — берутся только положительные
        let zeros = vec![
            CarryoverPoint {
                minutes_after: 0.0,
                value: 0.0,
            },
            CarryoverPoint {
                minutes_after: 10.0,
                value: 8.0,
            },
            CarryoverPoint {
                minutes_after: 20.0,
                value: 4.0,
            },
        ];
        // остаётся 2 точки — None
        assert!(carryover_halflife(&zeros).is_none());
    }
}
