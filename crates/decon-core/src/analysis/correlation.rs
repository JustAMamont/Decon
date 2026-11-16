//! Корреляции условий и качества (S4).
//!
//! Условия практики (сон, кофеин, время суток) ↔ качество состояния
//! (шкалы post-замеров). Шкалы Decon — порядковые (0..=10 слайдер),
//! поэтому основной коэффициент — Спирмен (по рангам); Пирсон оставлен
//! для непрерывных величин (сон в часах).
//!
//! Правило трезвости: функция возвращает только числа, каузальную
//! интерпретацию UI не предлагает (текст — «связь», не «влияние»).
//!
//! Бюджет (ARCHITECTURE.md §7): корреляция — 0 аллокаций на вызов,
//! ранжирование пары рядов — на месте, во внешнем буфере рангов,
//! который передаёт вызывающий (`&mut [RankBuf]`). Холодный проход по
//! 10k сессий < 50 мс — бенч `benches/analysis.rs`.

use serde::{Deserialize, Serialize};

/// Порог, ниже которого связь трактуется как «не отличима от шума»
/// (|ρ| < 0.1 при n ≥ 30 — консервативная планка для n-of-1 данных).
pub const NOISE_THRESHOLD: f64 = 0.1;

/// Минимум пар для вычисления корреляции: с меньшим числом любая
/// оценка неустойчива (для двух точек |ρ| ≡ 1).
pub const MIN_PAIRS: usize = 5;

/// Результат корреляции: коэффициент + число пар.
///
/// `rho` — Спирмен (порядковые шкалы) или Пирсон (непрерывные).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Correlation {
    /// Коэффициент −1..=1 (NaN — недостаточно пар или нулевая
    /// дисперсия хотя бы одного ряда).
    pub rho: f64,
    /// Число пар, по которым считали (после фильтра None).
    pub n: usize,
}

impl Correlation {
    /// Есть ли связь, отличимая от шума.
    pub const fn is_meaningful(&self) -> bool {
        self.n >= MIN_PAIRS && self.rho.abs() >= NOISE_THRESHOLD
    }

    /// Категория силы связи для UI (без каузальных слов).
    pub fn strength_label(&self) -> &'static str {
        if self.n < MIN_PAIRS {
            "insufficient"
        } else {
            let a = self.rho.abs();
            if a < NOISE_THRESHOLD {
                "none"
            } else if a < 0.3 {
                "weak"
            } else if a < 0.6 {
                "moderate"
            } else {
                "strong"
            }
        }
    }
}

/// Буфер рангов для пары рядов: переиспользуется между вызовами,
/// чтобы ранжирование не аллоцировало (правило №1).
///
/// `ORDER_CAP` покрывает холодный проход по всем сессиям (10k),
/// большее число пар ранжируется чанками — на практике не встречается.
pub const RANK_CAP: usize = 10_000;

/// Переиспользуемые индексы ранжирования пары рядов.
pub struct RankBuf {
    /// Индексы ряда x (сортируются по значению).
    idx: Vec<u32>,
    /// Прямой порядок → ранг для x.
    rank_x: Vec<f64>,
    /// Ранг для y.
    rank_y: Vec<f64>,
    /// Плотный (без None) ряд x для парной корреляции.
    dense_x: Vec<f64>,
    /// Плотный ряд y.
    dense_y: Vec<f64>,
}

impl RankBuf {
    /// Новый буфер (одна аллокация на всё время жизни аналитики).
    pub fn new() -> Self {
        RankBuf {
            idx: Vec::with_capacity(RANK_CAP),
            rank_x: Vec::with_capacity(RANK_CAP),
            rank_y: Vec::with_capacity(RANK_CAP),
            dense_x: Vec::with_capacity(RANK_CAP),
            dense_y: Vec::with_capacity(RANK_CAP),
        }
    }

    /// Текущая ёмкость (диагностика).
    pub const fn capacity(&self) -> usize {
        RANK_CAP
    }

    /// Спирмен по собственным dense-рядам: метод владеет всеми
    /// массивами, borrow-конфликт «читаю dense_x / пишу через buf»
    /// исчезает — ранжирование разнесено по полям внутри метода.
    fn spearman_dense(&mut self) -> f64 {
        let n = self.dense_x.len().min(self.dense_y.len());
        if !(2..=RANK_CAP).contains(&n) {
            return f64::NAN;
        }
        // Ранги x: порядок индексов по значениям dense_x
        self.idx.clear();
        self.idx.extend(0..n as u32);
        self.idx.sort_unstable_by(|&a, &b| {
            self.dense_x[a as usize]
                .partial_cmp(&self.dense_x[b as usize])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranks_from_sorted(&self.idx, &self.dense_x, &mut self.rank_x);
        // Ранги y
        self.idx.sort_unstable_by(|&a, &b| {
            self.dense_y[a as usize]
                .partial_cmp(&self.dense_y[b as usize])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranks_from_sorted(&self.idx, &self.dense_y, &mut self.rank_y);
        pearson(&self.rank_x, &self.rank_y)
    }
}

impl Default for RankBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// Корреляция Пирсона двух полных рядов (без None). 0 аллокаций.
///
/// Возвращает NaN при нулевой дисперсии или n < 2.
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return f64::NAN;
    }
    let mut sx = 0.0;
    let mut sy = 0.0;
    for i in 0..n {
        sx += x[i];
        sy += y[i];
    }
    let mx = sx / n as f64;
    let my = sy / n as f64;
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..n {
        let dx = x[i] - mx;
        let dy = y[i] - my;
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx <= 0.0 || vy <= 0.0 {
        return f64::NAN;
    }
    cov / (vx * vy).sqrt()
}

/// Корреляция Спирмена двух полных рядов: ранги (с усреднением
/// связок/ties) → Пирсон по рангам. 0 аллокаций — сортировка
/// индексов в переиспользуемом буфере `buf`.
///
/// Возвращает NaN при n < 2 или нулевой дисперсии рангов.
pub fn spearman_buf(x: &[f64], y: &[f64], buf: &mut RankBuf) -> f64 {
    let n = x.len().min(y.len());
    if !(2..=RANK_CAP).contains(&n) {
        return f64::NAN;
    }
    // Ранги x: сортировка индексов по значению, затем усреднение ties.
    buf.idx.clear();
    buf.idx.extend(0..n as u32);
    buf.idx.sort_unstable_by(|&a, &b| {
        x[a as usize]
            .partial_cmp(&x[b as usize])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranks_from_sorted(&buf.idx, x, &mut buf.rank_x);
    buf.idx.sort_unstable_by(|&a, &b| {
        y[a as usize]
            .partial_cmp(&y[b as usize])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranks_from_sorted(&buf.idx, y, &mut buf.rank_y);
    pearson(&buf.rank_x, &buf.rank_y)
}

/// Присвоить ранги по отсортированному порядку индексов с усреднением
/// связок: равные значения получают средний ранг (требование Спирмена
/// для порядковых данных с повторами — слайдер 0..=10 даёт их массово).
fn ranks_from_sorted(order: &[u32], values: &[f64], out: &mut Vec<f64>) {
    out.clear();
    out.resize(order.len(), 0.0);
    let n = order.len();
    let mut i = 0;
    while i < n {
        // Группа равных значений [i..j)
        let vi = values[order[i] as usize];
        let mut j = i + 1;
        while j < n && values[order[j] as usize] == vi {
            j += 1;
        }
        // Средний ранг группы (ранги 1-based: позиции i+1..j)
        let avg = (i + 1 + j) as f64 / 2.0;
        for k in i..j {
            out[order[k] as usize] = avg;
        }
        i = j;
    }
}

/// Парная корреляция рядов с пропусками: пары, где хотя бы одно
/// значение None, выбрасываются (pairwise deletion). 0 аллокаций.
pub fn correlation_missing(
    x: &[Option<f64>],
    y: &[Option<f64>],
    buf: &mut RankBuf,
    kind: CorrKind,
) -> Correlation {
    let mut n = 0usize;
    // Плотные ряды (без None) — в отдельные dense-буферы: ранжирование
    // в spearman_buf пишет ранги в rank_x/rank_y, читая dense на входе
    // (разные массивы — borrow-конфликта и перезаписи входа нет).
    buf.dense_x.clear();
    buf.dense_y.clear();
    for i in 0..x.len().min(y.len()) {
        if let (Some(a), Some(b)) = (x[i], y[i]) {
            buf.dense_x.push(a);
            buf.dense_y.push(b);
            n += 1;
        }
    }
    if n < MIN_PAIRS {
        return Correlation { rho: f64::NAN, n };
    }
    let rho = match kind {
        CorrKind::Pearson => pearson(&buf.dense_x, &buf.dense_y),
        CorrKind::Spearman => buf.spearman_dense(),
    };
    Correlation { rho, n }
}

/// Какой коэффициент считать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrKind {
    /// Пирсон: непрерывные величины (часы сна).
    Pearson,
    /// Спирмен: порядковые шкалы (слайдеры 0..=10).
    Spearman,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pearson_perfect_linear() {
        let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|v| 2.0 * v + 1.0).collect();
        let r = pearson(&x, &y);
        assert!((r - 1.0).abs() < 1e-12);
    }

    #[test]
    fn pearson_inverse() {
        let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|v| -v).collect();
        let r = pearson(&x, &y);
        assert!((r + 1.0).abs() < 1e-12);
    }

    #[test]
    fn pearson_degenerate() {
        let x = [1.0, 2.0, 3.0];
        let y = [5.0, 5.0, 5.0]; // нулевая дисперсия
        assert!(pearson(&x, &y).is_nan());
        assert!(pearson(&[1.0], &[2.0]).is_nan());
        assert!(pearson(&[], &[]).is_nan());
    }

    #[test]
    fn spearman_monotonic_nonlinear() {
        // Монотонная нелинейная связь: Пирсон < 1, Спирмен = 1.
        let x: Vec<f64> = (1..=20).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|v| v * v).collect();
        let mut buf = RankBuf::new();
        let sp = spearman_buf(&x, &y, &mut buf);
        assert!((sp - 1.0).abs() < 1e-12, "spearman={sp}");
        let pe = pearson(&x, &y);
        assert!(pe < 1.0 && pe > 0.9, "pearson={pe}");
    }

    #[test]
    fn spearman_ties_average_ranks() {
        // Массовые ties (слайдер): y константен на двух уровнях.
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let y = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let mut buf = RankBuf::new();
        let r = spearman_buf(&x, &y, &mut buf);
        // Ранги y: три нуля → средний ранг (1+2+3)/3 = 2.0;
        // шесть единиц → (4+9)/2 = 6.5
        assert_eq!(buf.rank_y[0], 2.0);
        assert_eq!(buf.rank_y[3], 6.5);
        // Спирмен по рангам = 0.8215838362577491 (проверено независимо)
        assert!((r - 0.821_583_836_257_749_1).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn spearman_degenerate() {
        let mut buf = RankBuf::new();
        assert!(spearman_buf(&[1.0], &[2.0], &mut buf).is_nan());
        // Константный ряд — нулевая дисперсия рангов
        assert!(spearman_buf(&[3.0, 3.0, 3.0], &[1.0, 2.0, 3.0], &mut buf).is_nan());
    }

    #[test]
    fn missing_pairs_dropped() {
        // 8 позиций, 6 полных пар (порог MIN_PAIRS=5 пройден): None на
        // i=1 (x) и i=2 (y) — выпадают обе позиции
        let x = [
            Some(1.0),
            None,
            Some(3.0),
            Some(4.0),
            Some(5.0),
            Some(6.0),
            Some(7.0),
            Some(8.0),
        ];
        let y = [
            Some(2.0),
            Some(3.0),
            None,
            Some(5.0),
            Some(6.0),
            Some(7.0),
            Some(8.0),
            Some(9.0),
        ];
        let mut buf = RankBuf::new();
        let c = correlation_missing(&x, &y, &mut buf, CorrKind::Pearson);
        assert_eq!(c.n, 6);
        assert!((c.rho - 1.0).abs() < 1e-12);
        // Полностью пустое пересечение
        let a = [Some(1.0), None];
        let b = [None, Some(2.0)];
        let c2 = correlation_missing(&a, &b, &mut buf, CorrKind::Spearman);
        assert_eq!(c2.n, 0);
        assert!(c2.rho.is_nan());
        // Меньше MIN_PAIRS — NaN даже при идеальной связи
        let c3 = correlation_missing(
            &[Some(1.0), Some(2.0), Some(3.0), Some(4.0)],
            &[Some(2.0), Some(3.0), Some(4.0), Some(5.0)],
            &mut buf,
            CorrKind::Pearson,
        );
        assert_eq!(c3.n, 4);
        assert!(c3.rho.is_nan());
    }

    #[test]
    fn strength_labels() {
        let mut buf = RankBuf::new();
        let strong = Correlation { rho: 0.8, n: 50 };
        assert_eq!(strong.strength_label(), "strong");
        let weak = Correlation { rho: 0.15, n: 50 };
        assert_eq!(weak.strength_label(), "weak");
        let small = Correlation { rho: 0.9, n: 3 };
        assert_eq!(small.strength_label(), "insufficient");
        assert!(!small.is_meaningful());
        let none = Correlation { rho: 0.02, n: 50 };
        assert_eq!(none.strength_label(), "none");
        let _ = &mut buf;
    }

    #[test]
    fn no_allocations_on_calls() {
        // Бюджет: повторные вызовы на одном буфере не аллоцируют.
        // Проверяем через рост len ёмкостей: capacity стабилен после
        // первого вызова, push в пределах ёмкости не аллоцирует.
        let mut buf = RankBuf::new();
        let x: Vec<f64> = (0..1000).map(|i| (i % 11) as f64).collect();
        let y: Vec<f64> = (0..1000).map(|i| ((i * 7 + 3) % 11) as f64).collect();
        spearman_buf(&x, &y, &mut buf);
        let cap_after_first = (buf.idx.capacity(), buf.rank_x.capacity());
        for _ in 0..100 {
            spearman_buf(&x, &y, &mut buf);
        }
        assert_eq!(
            (buf.idx.capacity(), buf.rank_x.capacity()),
            cap_after_first,
            "capacity grew — allocation happened"
        );
    }
}
