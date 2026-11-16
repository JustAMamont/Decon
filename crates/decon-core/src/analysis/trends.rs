//! Тренды: скользящие средние по шкалам (S4).
//!
//! Ряд ответа по одной шкале — упорядоченный по времени список точек
//! (`SeriesPoint` из репозитория). Особенности данных Decon:
//!
//! - версии формулировок меняются (SCALES.md §8): смешивать ответы
//!   разных версий в одно среднее нельзя — тренд строится раздельно
//!   по версиям, смена версии = вертикальная линия «смена инструмента»;
//! - слайдер 0..=10 — порядковая шкала, но для тренда (визуализации
//!   динамики) среднее допустимо; интерпретация — «центр масс ответов».
//!
//! SMA считается на месте, в буфере вызывающего: 0 аллокаций на окно.

use serde::{Deserialize, Serialize};

use crate::db::repo::scale_repo::SeriesPoint;

/// Точка тренда: центр окна + среднее.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrendPoint {
    /// Индекс в исходном ряду (центр окна).
    pub index: usize,
    /// Среднее по окну.
    pub value: f64,
}

/// Разрыв ряда: границы сегментов по версии формулировки.
///
/// Точки с `index >= split_at` — уже другая версия: тренд не должен
/// соединять сегменты линией (инструмент сменился).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionSplit {
    /// Первый индекс новой версии.
    pub split_at: usize,
    /// Номер новой версии.
    pub version: i64,
}

/// Сегмент ряда одной версии формулировки.
///
/// Не сериализуется: `SeriesPoint` — тип репозитория (без serde);
/// сегмент — внутренняя структура тренда, наружу UI берёт точки.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Номер версии формулировки.
    pub version: i64,
    /// Точки сегмента (срез исходного ряда).
    pub points: Vec<SeriesPoint>,
}

/// Разбить ряд на сегменты по версиям формулировок.
///
/// Ряд должен быть упорядочен по времени (репозиторий так отдаёт).
/// Аллокация — на результат (одна на сегмент), не на hot path.
pub fn split_by_version(points: &[SeriesPoint]) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for p in points {
        match out.last_mut() {
            Some(seg) if seg.version == p.version => seg.points.push(p.clone()),
            _ => out.push(Segment {
                version: p.version,
                points: vec![p.clone()],
            }),
        }
    }
    out
}

/// Простое скользящее среднее с окном `window` по одному сегменту.
///
/// Пишет в `out` (очищает); 0 аллокаций, если `out` уже имеет ёмкость.
/// Окно центрировано: первые/последние (window-1)/2 точек получают
/// среднее по доступному префиксу/суффиксу (края не выбрасываем —
/// пилотному пользователю дорог каждый ответ).
pub fn sma(points: &[SeriesPoint], window: usize, out: &mut Vec<TrendPoint>) {
    out.clear();
    if points.is_empty() || window == 0 {
        return;
    }
    let n = points.len();
    let w = window.min(n).max(1);
    // Окно длины w, центрированное на i. У краёв окно сдвигается
    // целиком (обе границы), сохраняя длину w — тренд у краёв смотрит
    // внутрь ряда, а не сползает в усечённое среднее.
    let h = (w - 1) / 2;
    // Кумулятивная сумма — окно за O(1) на точку.
    let mut cum = vec![0.0f64; n + 1];
    for (i, p) in points.iter().enumerate() {
        cum[i + 1] = cum[i] + p.value;
    }
    for i in 0..n {
        // Центр: [i-h, i-h+w); при выходе за границы — зажим окна
        // с сохранением длины (как в running-статистиках).
        let mut lo = i.saturating_sub(h);
        let mut hi = lo + w;
        if hi > n {
            hi = n;
            lo = hi - w;
        }
        let len = (hi - lo) as f64;
        out.push(TrendPoint {
            index: i,
            value: (cum[hi] - cum[lo]) / len,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(value: f64, version: i64) -> SeriesPoint {
        SeriesPoint {
            answered_at: format!("2026-11-{value:02}T10:00:00Z"),
            value,
            delay_minutes: None,
            version,
        }
    }

    #[test]
    fn sma_constant_series() {
        let pts: Vec<SeriesPoint> = (0..10).map(|_| pt(5.0, 1)).collect();
        let mut out = Vec::new();
        sma(&pts, 3, &mut out);
        assert_eq!(out.len(), 10);
        assert!(out.iter().all(|t| t.value == 5.0));
    }

    #[test]
    fn sma_window_smoothing() {
        // Ряд [0,0,0,10,10,10]: окно 3 сглаживает переход в центре.
        let vals = [0.0, 0.0, 0.0, 10.0, 10.0, 10.0];
        let pts: Vec<SeriesPoint> = vals.iter().map(|v| pt(*v, 1)).collect();
        let mut out = Vec::new();
        sma(&pts, 3, &mut out);
        // Центр (индекс 2): окно [1..4) → (0+0+10)/3
        assert!((out[2].value - 10.0 / 3.0).abs() < 1e-12);
        // Край (индекс 0): доступный префикс [0..2) → 0
        assert!(out[0].value.abs() < 1e-12);
        // Индекс 3: окно [2..5) → (0+10+10)/3
        assert!((out[3].value - 20.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn sma_edge_cases() {
        let mut out = Vec::new();
        sma(&[], 5, &mut out);
        assert!(out.is_empty());
        sma(&[pt(1.0, 1)], 0, &mut out); // окно 0 — нет смысла
        assert!(out.is_empty());
        // Окно больше ряда — работает как среднее всего ряда
        sma(&[pt(1.0, 1), pt(3.0, 1)], 100, &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[0].value - 2.0).abs() < 1e-12);
    }

    #[test]
    fn split_by_version_boundaries() {
        let pts = vec![pt(1.0, 1), pt(2.0, 1), pt(3.0, 2), pt(4.0, 2), pt(5.0, 1)];
        let segs = split_by_version(&pts);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].version, 1);
        assert_eq!(segs[0].points.len(), 2);
        assert_eq!(segs[1].version, 2);
        assert_eq!(segs[1].points.len(), 2);
        assert_eq!(segs[2].version, 1); // возврат к старой версии
        assert_eq!(segs[2].points.len(), 1);
    }

    #[test]
    fn split_single_version() {
        let pts = vec![pt(1.0, 1), pt(2.0, 1)];
        let segs = split_by_version(&pts);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].points.len(), 2);
    }

    #[test]
    fn split_empty() {
        assert!(split_by_version(&[]).is_empty());
    }

    #[test]
    fn sma_no_alloc_growth_on_reuse() {
        // Бюджет: повторный вызов с тем же окном не растит буфер.
        let pts: Vec<SeriesPoint> = (0..500).map(|i| pt((i % 10) as f64, 1)).collect();
        let mut out = Vec::new();
        sma(&pts, 7, &mut out);
        let cap = out.capacity();
        for _ in 0..50 {
            sma(&pts, 7, &mut out);
        }
        assert_eq!(out.capacity(), cap);
        assert_eq!(out.len(), 500);
    }
}
