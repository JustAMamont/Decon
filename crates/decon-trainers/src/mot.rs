//! Multiple Object Tracking (S3).
//!
//! Задача Пирьяна-Уллмана: N целей среди дистракторов, фаза индикации
//! (подсветка целей) → фаза движения (все объекты, случайные блуждания
//! с непересечением) → фаза ответа (клик по целям). Измеряет способность
//! удерживать распределённое внимание по полю — прямой прокси-прибор
//! для дКВ.
//!
//! Движок синхронный, детерминированный по сиду; `tick` — 0 аллокаций
//! (SoA-массивы фиксированной ёмкости, RULE-1 бюджет из CONVENTIONS).
//! UI вызывает `tick(dt_ms)` из своего таймера и перерисовывает по
//! `positions()`.

#![deny(missing_docs)]

use serde::{Deserialize, Serialize};

use crate::rng::Pcg32;

/// Максимум объектов (цели + дистракторы); ёмкость SoA-буферов.
pub const MAX_OBJECTS: usize = 32;

/// Фаза прогона MOT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotPhase {
    /// Показ целей (подсвечены).
    Cue,
    /// Движение без подсветки.
    Motion,
    /// Сбор ответов.
    Answer,
    /// Прогон завершён.
    Done,
}

/// Конфигурация прогона MOT (`config_json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotConfig {
    /// Число целей.
    pub targets: u8,
    /// Число дистракторов.
    pub distractors: u8,
    /// Скорость, доля поля в секунду (1.0 — экранные поля в секунду).
    pub speed: f32,
    /// Длительность фазы движения, мс.
    pub motion_ms: u32,
}

impl MotConfig {
    /// Валидация: 1..=10 целей, сумма ≤ 32, скорость > 0.
    pub fn validate(&self) -> Result<(), MotError> {
        if self.targets == 0 || self.targets > 10 {
            return Err(MotError::InvalidTargets(self.targets));
        }
        if self.distractors > 22 {
            return Err(MotError::TooManyObjects);
        }
        if usize::from(self.targets) + usize::from(self.distractors) > MAX_OBJECTS {
            return Err(MotError::TooManyObjects);
        }
        if !self.speed.is_finite() || self.speed <= 0.0 {
            return Err(MotError::InvalidSpeed(self.speed));
        }
        if self.motion_ms == 0 {
            return Err(MotError::InvalidMotionMs);
        }
        Ok(())
    }

    /// Полное число объектов.
    pub const fn total(&self) -> usize {
        self.targets as usize + self.distractors as usize
    }
}

/// Ошибки MOT.
#[derive(Debug, thiserror::Error)]
pub enum MotError {
    /// Цели вне 1..=10.
    #[error("targets must be 1..=10, got {0}")]
    InvalidTargets(u8),
    /// Сумма объектов > MAX_OBJECTS.
    #[error("too many objects (max {MAX_OBJECTS})")]
    TooManyObjects,
    /// Скорость ≤ 0 или NaN.
    #[error("invalid speed: {0}")]
    InvalidSpeed(f32),
    /// Нулевая длительность движения.
    #[error("motion_ms must be > 0")]
    InvalidMotionMs,
    /// Конфигурация менялась между вызовами.
    #[error("config changed mid-run")]
    ConfigChanged,
    /// Ответ по индексу вне диапазона объектов.
    #[error("object index out of range: {0}")]
    ObjectOutOfRange(usize),
}

/// Метрики прогона MOT (`metrics_json`, контракт SPEC §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotMetrics {
    /// Число целей.
    pub targets: u8,
    /// Число дистракторов.
    pub distractors: u8,
    /// Скорость.
    pub speed: f32,
    /// Полная длительность прогона (до конца фазы ответа), мс.
    pub trial_ms: u64,
    /// Процент корректных ответов (верные цели из N).
    pub correct_pct: f32,
    /// Латентности ответов, мс от начала фазы ответа.
    #[serde(default)]
    pub answer_rt_ms: Vec<u64>,
}

/// Объект в SoA-раскладке: координаты и скорости в плоских массивах.
///
/// Поле — единичный квадрат [0,1]²; UI масштабирует под свои пиксели.
struct Objects {
    n: usize,
    x: [f32; MAX_OBJECTS],
    y: [f32; MAX_OBJECTS],
    vx: [f32; MAX_OBJECTS],
    vy: [f32; MAX_OBJECTS],
}

impl Objects {
    fn new() -> Self {
        Objects {
            n: 0,
            x: [0.0; MAX_OBJECTS],
            y: [0.0; MAX_OBJECTS],
            vx: [0.0; MAX_OBJECTS],
            vy: [0.0; MAX_OBJECTS],
        }
    }
}

/// Состояние прогона MOT.
pub struct Mot {
    config: MotConfig,
    phase: MotPhase,
    /// Сид прогона (воспроизводимость, пишется в trainer_results.seed).
    seed: u64,
    objects: Objects,
    /// Цели: индексы объектов (первая `targets` после init-перемешивания
    /// не хранится отдельно — цельность проверяется маской).
    is_target: [bool; MAX_OBJECTS],
    /// Движок для направлений движения (поток seq=2).
    rng: Pcg32,
    /// Прошло мс в текущей фазе.
    phase_ms: u64,
    /// Суммарное время прогона.
    total_ms: u64,
    /// Ответы: индексы выбранных объектов, по порядку.
    answers: Vec<usize>,
    /// Латентности ответов, мс от старта фазы ответа.
    answer_rt: Vec<u64>,
}

impl Mot {
    /// Новый прогон: раскладка без пересечений (rejection sampling,
    /// минимальная дистанция 2·радиус), случайные направления.
    pub fn new(config: MotConfig, seed: u64) -> Result<Self, MotError> {
        config.validate()?;
        let total = config.total();
        let mut objects = Objects::new();
        objects.n = total;
        // Раскладка: поток seq=0. Мин. дистанция — 1/8 поля для старта.
        let mut rng = Pcg32::new(seed, 0);
        let min_dist = 1.0 / 8.0;
        let mut placed = 0usize;
        while placed < total {
            let x = rng.next_f32();
            let y = rng.next_f32();
            let ok = (0..placed).all(|i| {
                let dx = objects.x[i] - x;
                let dy = objects.y[i] - y;
                dx * dx + dy * dy >= min_dist * min_dist
            });
            if ok {
                objects.x[placed] = x;
                objects.y[placed] = y;
                placed += 1;
            }
        }
        // Скорости: поток seq=1. Единичный вектор * speed_per_ms.
        let mut rng_v = Pcg32::new(seed, 1);
        let speed_per_ms = config.speed / 1000.0;
        for i in 0..total {
            let angle = rng_v.next_f32() * core::f32::consts::TAU;
            objects.vx[i] = angle.cos() * speed_per_ms;
            objects.vy[i] = angle.sin() * speed_per_ms;
        }
        // Цели: поток seq=2 — Фишер—Йетс по индексам, первые targets.
        let mut idx: [usize; MAX_OBJECTS] = [0; MAX_OBJECTS];
        for (i, slot) in idx.iter_mut().enumerate() {
            *slot = i;
        }
        Pcg32::new(seed, 2).shuffle(&mut idx[..total]);
        let mut is_target = [false; MAX_OBJECTS];
        for &i in &idx[..config.targets as usize] {
            is_target[i] = true;
        }
        Ok(Self {
            phase: MotPhase::Cue,
            seed,
            objects,
            is_target,
            rng: Pcg32::new(seed, 3),
            phase_ms: 0,
            total_ms: 0,
            answers: Vec::with_capacity(total),
            answer_rt: Vec::with_capacity(total),
            config,
        })
    }

    /// Конфигурация.
    pub const fn config(&self) -> &MotConfig {
        &self.config
    }

    /// Сид прогона.
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// Текущая фаза.
    pub const fn phase(&self) -> MotPhase {
        self.phase
    }

    /// Время в текущей фазе, мс.
    pub const fn phase_ms(&self) -> u64 {
        self.phase_ms
    }

    /// Полное время прогона, мс.
    pub const fn total_ms(&self) -> u64 {
        self.total_ms
    }

    /// Число объектов.
    pub const fn object_count(&self) -> usize {
        self.objects.n
    }

    /// Позиция объекта (единицы поля [0,1]²); паника вне диапазона —
    /// программная ошибка UI, не пользовательские данные.
    pub fn position(&self, i: usize) -> (f32, f32) {
        assert!(i < self.objects.n, "MOT: object index out of range");
        (self.objects.x[i], self.objects.y[i])
    }

    /// Является ли объект целью — только в фазах Cue/Answer (в Motion
    /// UI не должен это спрашивать; движок не скрывает, но протокол
    /// требует рисовать одинаково).
    pub const fn is_target(&self, i: usize) -> bool {
        self.is_target[i]
    }

    /// Выбранные пользователем объекты (фаза ответа).
    pub fn answers(&self) -> &[usize] {
        &self.answers
    }

    /// Тик движка: `dt_ms` с прошлого тика. Двигает объекты в Motion,
    /// переключает фазы по таймеру. 0 аллокаций.
    ///
    /// Возвращает `true`, если фаза сменилась (UI может пере-рендер).
    pub fn tick(&mut self, dt_ms: u32) -> bool {
        if self.phase == MotPhase::Done {
            return false;
        }
        self.total_ms += u64::from(dt_ms);
        match self.phase {
            MotPhase::Cue => {
                // Фаза индикации — фикс 1500 мс.
                self.phase_ms += u64::from(dt_ms);
                if self.phase_ms >= 1500 {
                    self.phase = MotPhase::Motion;
                    self.phase_ms = 0;
                    return true;
                }
            }
            MotPhase::Motion => {
                self.phase_ms += u64::from(dt_ms);
                self.integrate(dt_ms);
                if self.phase_ms >= u64::from(self.config.motion_ms) {
                    self.phase = MotPhase::Answer;
                    self.phase_ms = 0;
                    return true;
                }
            }
            MotPhase::Answer => {
                self.phase_ms += u64::from(dt_ms);
            }
            MotPhase::Done => unreachable!(),
        }
        false
    }

    /// Интегрирование движения + отражение от стен + непересечение
    /// (простое расталкивание). 0 аллокаций, SoA-массивы на месте.
    fn integrate(&mut self, dt_ms: u32) {
        let dt = dt_ms as f32;
        let n = self.objects.n;
        // Радиус объекта в единицах поля (отражение от [0,1]).
        const R: f32 = 0.02;
        // Позиции
        for i in 0..n {
            self.objects.x[i] += self.objects.vx[i] * dt;
            self.objects.y[i] += self.objects.vy[i] * dt;
        }
        // Стены: отражение с зажимом
        for i in 0..n {
            if self.objects.x[i] < R {
                self.objects.x[i] = R + (R - self.objects.x[i]);
                self.objects.vx[i] = -self.objects.vx[i];
            } else if self.objects.x[i] > 1.0 - R {
                self.objects.x[i] = 2.0 * (1.0 - R) - self.objects.x[i];
                self.objects.vx[i] = -self.objects.vx[i];
            }
            if self.objects.y[i] < R {
                self.objects.y[i] = R + (R - self.objects.y[i]);
                self.objects.vy[i] = -self.objects.vy[i];
            } else if self.objects.y[i] > 1.0 - R {
                self.objects.y[i] = 2.0 * (1.0 - R) - self.objects.y[i];
                self.objects.vy[i] = -self.objects.vy[i];
            }
        }
        // Пересечения: одна итерация попарного расталкивания (визуально
        // достаточно; полный sol­ver не нужен, объекты мелкие).
        const MIN_D: f32 = 0.045;
        let min_d2 = MIN_D * MIN_D;
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = self.objects.x[j] - self.objects.x[i];
                let dy = self.objects.y[j] - self.objects.y[i];
                let d2 = dx * dx + dy * dy;
                if d2 < min_d2 && d2 > 1e-12 {
                    let d = d2.sqrt();
                    let push = (MIN_D - d) * 0.5;
                    let ux = dx / d;
                    let uy = dy / d;
                    self.objects.x[i] -= ux * push;
                    self.objects.y[i] -= uy * push;
                    self.objects.x[j] += ux * push;
                    self.objects.y[j] += uy * push;
                }
            }
        }
        // Случайные микро-повороты скоростей (живость траекторий):
        // ±15° с вероятностью ~1/8 на объект за тик.
        for i in 0..n {
            if self.rng.below(8) == 0 {
                let turn = (self.rng.next_f32() - 0.5) * 0.5;
                let (s, c) = turn.sin_cos();
                let vx = self.objects.vx[i];
                let vy = self.objects.vy[i];
                self.objects.vx[i] = vx * c - vy * s;
                self.objects.vy[i] = vx * s + vy * c;
            }
        }
    }

    /// Ответ пользователя: выбор объекта `index` в фазе Answer.
    /// Повторный выбор того же объекта игнорируется (Ok(false)).
    pub fn answer(&mut self, index: usize) -> Result<bool, MotError> {
        if self.phase != MotPhase::Answer {
            return Ok(false);
        }
        if index >= self.objects.n {
            return Err(MotError::ObjectOutOfRange(index));
        }
        if self.answers.contains(&index) {
            return Ok(false);
        }
        if self.answers.len() >= self.config.targets as usize {
            return Ok(false);
        }
        self.answers.push(index);
        self.answer_rt.push(self.phase_ms);
        if self.answers.len() == self.config.targets as usize {
            self.phase = MotPhase::Done;
        }
        Ok(true)
    }

    /// Завершить прогон досрочно (истекло время ответа; лимит — 15 с
    /// по протоколу, UI решает когда звать). Недостающие ответы —
    /// неверные.
    pub fn finish(&mut self) {
        if self.phase != MotPhase::Done {
            self.phase = MotPhase::Done;
        }
    }

    /// Метрики завершённого прогона. `Ok(None)`, если не завершён.
    pub fn metrics(&self) -> Result<Option<MotMetrics>, MotError> {
        if self.phase != MotPhase::Done {
            return Ok(None);
        }
        let correct = self.answers.iter().filter(|&&i| self.is_target[i]).count();
        let correct_pct = if self.config.targets == 0 {
            0.0
        } else {
            correct as f32 * 100.0 / f32::from(self.config.targets)
        };
        Ok(Some(MotMetrics {
            targets: self.config.targets,
            distractors: self.config.distractors,
            speed: self.config.speed,
            trial_ms: self.total_ms,
            correct_pct,
            answer_rt_ms: self.answer_rt.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MotConfig {
        MotConfig {
            targets: 4,
            distractors: 8,
            speed: 0.1,
            motion_ms: 2000,
        }
    }

    #[test]
    fn config_validation() {
        assert!(cfg().validate().is_ok());
        assert!(
            MotConfig {
                targets: 0,
                ..cfg()
            }
            .validate()
            .is_err()
        );
        assert!(
            MotConfig {
                targets: 11,
                ..cfg()
            }
            .validate()
            .is_err()
        );
        assert!(
            MotConfig {
                distractors: 23,
                ..cfg()
            }
            .validate()
            .is_err()
        );
        assert!(
            MotConfig {
                speed: 0.0,
                ..cfg()
            }
            .validate()
            .is_err()
        );
        assert!(
            MotConfig {
                motion_ms: 0,
                ..cfg()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn initial_layout_no_overlap_in_range() {
        let m = Mot::new(cfg(), 42).expect("mot");
        for i in 0..m.object_count() {
            let (x, y) = m.position(i);
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
            for j in (i + 1)..m.object_count() {
                let (x2, y2) = m.position(j);
                let dx = x - x2;
                let dy = y - y2;
                assert!(dx * dx + dy * dy >= 1e-4, "overlap {i}/{j}");
            }
        }
        // Ровно targets целей
        let t = (0..m.object_count()).filter(|&i| m.is_target(i)).count();
        assert_eq!(t, 4);
    }

    #[test]
    fn phase_transitions_by_time() {
        let mut m = Mot::new(cfg(), 7).expect("mot");
        assert_eq!(m.phase(), MotPhase::Cue);
        assert!(!m.tick(1400));
        assert_eq!(m.phase(), MotPhase::Cue);
        assert!(m.tick(200)); // 1600 ≥ 1500
        assert_eq!(m.phase(), MotPhase::Motion);
        assert!(!m.tick(1900));
        assert_eq!(m.phase(), MotPhase::Motion);
        assert!(m.tick(100)); // 2000 ≥ motion_ms
        assert_eq!(m.phase(), MotPhase::Answer);
        // В Answer тики просто копят время
        assert!(!m.tick(50));
        assert_eq!(m.phase(), MotPhase::Answer);
    }

    #[test]
    fn objects_stay_in_field_after_motion() {
        let mut m = Mot::new(
            MotConfig {
                targets: 5,
                distractors: 15,
                speed: 2.0,
                motion_ms: 5000,
            },
            99,
        )
        .expect("mot");
        // Быстро в Motion и гоняем 60 Гц × 10 с
        m.tick(1500);
        assert_eq!(m.phase(), MotPhase::Motion);
        for _ in 0..600 {
            m.tick(16);
        }
        for i in 0..m.object_count() {
            let (x, y) = m.position(i);
            assert!((-0.05..=1.05).contains(&x), "x out: {x}");
            assert!((-0.05..=1.05).contains(&y), "y out: {y}");
        }
    }

    #[test]
    fn answer_flow_and_metrics() {
        let mut m = Mot::new(cfg(), 11).expect("mot");
        m.tick(1500);
        m.tick(2000);
        assert_eq!(m.phase(), MotPhase::Answer);
        // Выбираем 2 верных и 2 неверных
        let targets: Vec<usize> = (0..m.object_count()).filter(|&i| m.is_target(i)).collect();
        let others: Vec<usize> = (0..m.object_count()).filter(|&i| !m.is_target(i)).collect();
        assert!(m.answer(targets[0]).unwrap());
        assert!(m.answer(others[0]).unwrap());
        m.tick(30);
        assert!(m.answer(targets[1]).unwrap());
        assert!(m.answer(others[1]).unwrap());
        assert_eq!(m.phase(), MotPhase::Done);
        let met = m.metrics().unwrap().expect("metrics");
        assert_eq!(met.correct_pct, 50.0);
        assert_eq!(met.answer_rt_ms.len(), 4);
        assert_eq!(met.answer_rt_ms[3], 30);
        // Повторный выбор после Done — no-op
        assert!(!m.answer(targets[2]).unwrap());
    }

    #[test]
    fn answer_out_of_range() {
        let mut m = Mot::new(cfg(), 12).expect("mot");
        m.tick(1500);
        m.tick(2000);
        assert!(matches!(m.answer(12), Err(MotError::ObjectOutOfRange(12))));
    }

    #[test]
    fn finish_early_counts_missing_as_wrong() {
        let mut m = Mot::new(cfg(), 13).expect("mot");
        m.tick(1500);
        m.tick(2000);
        let targets: Vec<usize> = (0..m.object_count()).filter(|&i| m.is_target(i)).collect();
        assert!(m.answer(targets[0]).unwrap());
        m.finish();
        let met = m.metrics().unwrap().expect("metrics");
        assert_eq!(met.correct_pct, 25.0);
        assert_eq!(met.answer_rt_ms.len(), 1);
    }

    #[test]
    fn determinism_same_seed_same_trajectory() {
        // Критерий приёмки S3 №2 (движковая часть): сид → траектория.
        let mut a = Mot::new(cfg(), 2024).expect("a");
        let mut b = Mot::new(cfg(), 2024).expect("b");
        for _ in 0..200 {
            a.tick(16);
            b.tick(16);
        }
        for i in 0..a.object_count() {
            assert_eq!(a.position(i), b.position(i));
        }
        // Разные сиды — разные траектории (не обязаны, но практически)
        let mut c = Mot::new(cfg(), 2025).expect("c");
        for _ in 0..200 {
            c.tick(16);
        }
        let mut any_diff = false;
        for i in 0..c.object_count() {
            if a.position(i) != c.position(i) {
                any_diff = true;
                break;
            }
        }
        assert!(any_diff, "different seeds gave identical trajectories");
    }

    #[test]
    fn metrics_json_contract() {
        let m = MotMetrics {
            targets: 4,
            distractors: 8,
            speed: 1.0,
            trial_ms: 8000,
            correct_pct: 62.5,
            answer_rt_ms: vec![1450, 1620],
        };
        let json = serde_json::to_string(&m).expect("ser");
        let back: MotMetrics = serde_json::from_str(&json).expect("de");
        assert_eq!(back, m);
        let bad = json.replace("\"speed\":1.0", "\"speed\":1.0,\"extra\":1");
        assert!(serde_json::from_str::<MotMetrics>(&bad).is_err());
    }
}
