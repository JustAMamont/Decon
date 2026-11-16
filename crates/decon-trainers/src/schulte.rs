//! Таблица Шульте (S3).
//!
//! Классический прибор для объёма внимания и периферийного поиска:
//! найти числа 1..N по порядку в сетке. Три режима дКВ-протокола Decon
//! (SPEC §4 `metrics_json.schulte.mode`):
//!
//! - `classic` — просто поиск по порядку (базовая линия);
//! - `field` — фиксация взгляда в центре, поиск периферией (расширение
//!   полезного поля зрения);
//! - `dkv` — расфокус («пустой» взгляд), режим минимальной фокусировки.
//!
//! Движок — чистая логика без рендера: UI рисует сетку из `cells`,
//! клики передаёт в [`Schulte::tap`], тики не нужны (время считает UI
//! по своим часам и передаёт в `tap`). 0 аллокаций на `tap`.

#![deny(missing_docs)]

use serde::{Deserialize, Serialize};

use crate::rng::Pcg32;

/// Режим прохождения (протокол дКВ).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchulteMode {
    /// Поиск по порядку без ограничений взгляда.
    Classic,
    /// Фиксация в центре, периферийный поиск.
    Field,
    /// Расфокусированный взгляд.
    Dkv,
}

impl SchulteMode {
    /// Код для БД/UI.
    pub const fn as_code(self) -> &'static str {
        match self {
            SchulteMode::Classic => "classic",
            SchulteMode::Field => "field",
            SchulteMode::Dkv => "dkv",
        }
    }
}

/// Набор символов сетки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchulteSymbols {
    /// Цифры 1..N.
    Digits,
    /// Буквы A.. (26 максимум).
    Letters,
    /// Двуцветные ячейки: ищем «красные по возрастанию» (S5-прибор,
    /// в S3 — раскладка теми же правилами).
    Bicolor,
}

impl SchulteSymbols {
    /// Код для БД/UI.
    pub const fn as_code(self) -> &'static str {
        match self {
            SchulteSymbols::Digits => "digits",
            SchulteSymbols::Letters => "letters",
            SchulteSymbols::Bicolor => "bicolor",
        }
    }

    /// Максимум ячеек для набора (букв — 26).
    pub const fn max_cells(self) -> u16 {
        match self {
            SchulteSymbols::Digits | SchulteSymbols::Bicolor => 100,
            SchulteSymbols::Letters => 26,
        }
    }
}

/// Конфигурация прогона Шульте (`config_json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchulteConfig {
    /// Режим.
    pub mode: SchulteMode,
    /// Сторона сетки: 3..=10 (квадрат).
    pub grid: u8,
    /// Набор символов.
    pub symbols: SchulteSymbols,
}

impl SchulteConfig {
    /// Валидация: сетка 3..=10, букв не больше 26.
    pub fn validate(&self) -> Result<(), SchulteError> {
        if !(3..=10).contains(&self.grid) {
            return Err(SchulteError::InvalidGrid(self.grid));
        }
        let cells = u16::from(self.grid) * u16::from(self.grid);
        if cells > self.symbols.max_cells() {
            return Err(SchulteError::TooManyCells);
        }
        Ok(())
    }

    /// Число ячеек.
    pub const fn cell_count(&self) -> usize {
        self.grid as usize * self.grid as usize
    }

    /// Строка вида "5x5" для метрик (контракт SPEC §4).
    pub fn grid_label(&self) -> String {
        format!("{}x{}", self.grid, self.grid)
    }
}

/// Ошибки тренажёра.
#[derive(Debug, thiserror::Error)]
pub enum SchulteError {
    /// Сетка вне 3..=10.
    #[error("grid side must be 3..=10, got {0}")]
    InvalidGrid(u8),
    /// Ячеек больше, чем символов в наборе.
    #[error("too many cells for symbol set")]
    TooManyCells,
    /// Прогон не активен (не начат или завершён).
    #[error("run not active")]
    NotActive,
    /// Тап по ячейке вне сетки.
    #[error("cell index out of range: {0}")]
    CellOutOfRange(usize),
}

/// Метрики прогона Шульте (`metrics_json`, контракт SPEC §4: ключи
/// фиксированы, serde `deny_unknown_fields` — неизвестные отклоняются).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchulteMetrics {
    /// Режим прохождения.
    pub mode: SchulteMode,
    /// Сетка, "5x5".
    pub grid: String,
    /// Набор символов.
    pub symbols: SchulteSymbols,
    /// Полное время прогона, мс.
    pub duration_ms: u64,
    /// Число ошибок (тапы не по порядку).
    pub errors: u32,
    /// Времена реакции на каждую ячейку, мс (по порядку находок).
    #[serde(default)]
    pub cell_rt_ms: Vec<u64>,
}

/// Одна ячейка сетки (значение + цвет для двуцветного набора).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Порядковый номер ячейки (искомое значение), 1-based.
    pub value: u16,
    /// Красная ли (только `Bicolor`; иначе false).
    pub red: bool,
}

/// Состояние прогона Шульте.
pub struct Schulte {
    config: SchulteConfig,
    cells: Vec<Cell>,
    /// Индекс в cells следующей искомой (value = next_expected).
    next_expected: u16,
    /// Тапы-ошибки.
    errors: u32,
    /// Времена находок, мс от старта.
    found_rt: Vec<u64>,
    /// Прогон начат (первый тап = старт часов UI).
    started: bool,
    /// Прогон завершён (все ячейки найдены).
    finished: bool,
}

impl Schulte {
    /// Новая раскладка по сиду: перестановка 1..N (Фишер—Йетс), для
    /// `Bicolor` — каждая ячейка независимо красная с p=0.5 (поток seq=1,
    /// отдельный от раскладки seq=0).
    pub fn new(config: SchulteConfig, seed: u64) -> Result<Self, SchulteError> {
        config.validate()?;
        let n = config.cell_count() as u16;
        let mut values: Vec<u16> = (1..=n).collect();
        let mut rng = Pcg32::new(seed, 0);
        rng.shuffle(&mut values);
        let mut rng_color = Pcg32::new(seed, 1);
        let cells: Vec<Cell> = values
            .into_iter()
            .map(|value| Cell {
                value,
                red: config.symbols == SchulteSymbols::Bicolor && rng_color.next_u32() & 1 == 0,
            })
            .collect();
        Ok(Self {
            config,
            cells,
            next_expected: 1,
            errors: 0,
            found_rt: Vec::with_capacity(n as usize),
            started: false,
            finished: false,
        })
    }

    /// Конфигурация прогона.
    pub const fn config(&self) -> &SchulteConfig {
        &self.config
    }

    /// Ячейки для рендера (порядок — раскладка).
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Какое число ищем сейчас (1-based; после завершения — N).
    pub const fn next_expected(&self) -> u16 {
        self.next_expected
    }

    /// Число ошибок в текущем прогоне.
    pub const fn errors(&self) -> u32 {
        self.errors
    }

    /// Начат ли прогон (был тап).
    pub const fn is_started(&self) -> bool {
        self.started
    }

    /// Завершён ли прогон (все ячейки найдены).
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// Тап по ячейке `index` в момент `t_ms` от старта прогона.
    /// Первый корректный тап запускает счёт времени (`started`), поэтому
    /// UI может передавать время от первого тапа.
    ///
    /// Возвращает `Ok(true)`, если тап был верным (ячейка найдена),
    /// `Ok(false)` — ошибка (не по порядку), Err — тап вне сетки/не активен.
    /// 0 аллокаций (запись в preallocated `found_rt`).
    pub fn tap(&mut self, index: usize, t_ms: u64) -> Result<bool, SchulteError> {
        if self.finished {
            return Err(SchulteError::NotActive);
        }
        let Some(cell) = self.cells.get(index) else {
            return Err(SchulteError::CellOutOfRange(index));
        };
        if cell.value != self.next_expected {
            self.errors += 1;
            return Ok(false);
        }
        if !self.started {
            self.started = true;
        }
        self.found_rt.push(t_ms);
        if self.next_expected == self.cells.len() as u16 {
            self.finished = true;
        } else {
            self.next_expected += 1;
        }
        Ok(true)
    }

    /// Метрики завершённого прогона; `NotActive`, если не завершён.
    /// Аллокация — одна (сам вектор rt уже собран, тут сериализация).
    pub fn metrics(&self) -> Result<SchulteMetrics, SchulteError> {
        if !self.finished {
            return Err(SchulteError::NotActive);
        }
        let duration_ms = self.found_rt.last().copied().unwrap_or(0);
        // cell_rt_ms — межклеточные интервалы (rt на ячейку), первый —
        // от старта; контракт SPEC §4.
        let mut cell_rt = Vec::with_capacity(self.found_rt.len());
        let mut prev = 0u64;
        for &t in &self.found_rt {
            cell_rt.push(t.saturating_sub(prev));
            prev = t;
        }
        Ok(SchulteMetrics {
            mode: self.config.mode,
            grid: self.config.grid_label(),
            symbols: self.config.symbols,
            duration_ms,
            errors: self.errors,
            cell_rt_ms: cell_rt,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg5x5() -> SchulteConfig {
        SchulteConfig {
            mode: SchulteMode::Field,
            grid: 5,
            symbols: SchulteSymbols::Digits,
        }
    }

    #[test]
    fn config_validation_bounds() {
        assert!(cfg5x5().validate().is_ok());
        for bad in [2u8, 11] {
            assert!(
                SchulteConfig {
                    mode: SchulteMode::Classic,
                    grid: bad,
                    symbols: SchulteSymbols::Digits,
                }
                .validate()
                .is_err()
            );
        }
        // 6x6 букв = 36 > 26
        assert!(
            SchulteConfig {
                mode: SchulteMode::Classic,
                grid: 6,
                symbols: SchulteSymbols::Letters,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn layout_is_permutation_deterministic() {
        let a = Schulte::new(cfg5x5(), 42).expect("a");
        let b = Schulte::new(cfg5x5(), 42).expect("b");
        let mut vals: Vec<u16> = a.cells().iter().map(|c| c.value).collect();
        vals.sort_unstable();
        assert_eq!(vals, (1..=25).collect::<Vec<_>>());
        assert_eq!(
            a.cells().iter().map(|c| c.value).collect::<Vec<_>>(),
            b.cells().iter().map(|c| c.value).collect::<Vec<_>>()
        );
    }

    #[test]
    fn tap_flow_errors_and_finish() {
        let mut s = Schulte::new(cfg5x5(), 1).expect("s");
        // Ищем позицию «1» и «2»
        let i1 = s.cells().iter().position(|c| c.value == 1).unwrap();
        let i2 = s.cells().iter().position(|c| c.value == 2).unwrap();
        let i3 = s.cells().iter().position(|c| c.value == 3).unwrap();
        // Тап «2» первым — ошибка, счётчик растёт, прогресса нет
        assert!(!s.tap(i2, 0).unwrap());
        assert_eq!(s.errors(), 1);
        assert_eq!(s.next_expected(), 1);
        // Верный тап
        assert!(s.tap(i1, 100).unwrap());
        assert!(s.is_started());
        assert!(s.tap(i2, 500).unwrap());
        assert!(!s.is_finished());
        // Тап по уже найденной — ошибка (значение < expected)
        assert!(!s.tap(i1, 600).unwrap());
        assert_eq!(s.errors(), 2);
        assert!(s.tap(i3, 900).unwrap());
        // Вне сетки
        assert!(s.tap(25, 950).is_err());
    }

    #[test]
    fn metrics_after_full_run() {
        let mut s = Schulte::new(cfg5x5(), 3).expect("s");
        let mut t = 0u64;
        for value in 1..=25u16 {
            let idx = s.cells().iter().position(|c| c.value == value).unwrap();
            t += 100;
            assert!(s.tap(idx, t).unwrap());
        }
        assert!(s.is_finished());
        let m = s.metrics().expect("metrics");
        assert_eq!(m.errors, 0);
        assert_eq!(m.duration_ms, 2500);
        assert_eq!(m.cell_rt_ms, vec![100; 25]);
        assert_eq!(m.grid, "5x5");
        // После завершения тапы отклоняются
        assert!(matches!(s.tap(0, 0), Err(SchulteError::NotActive)));
    }

    #[test]
    fn metrics_before_finish_rejected() {
        let s = Schulte::new(cfg5x5(), 4).expect("s");
        assert!(matches!(s.metrics(), Err(SchulteError::NotActive)));
    }

    #[test]
    fn metrics_json_contract_roundtrip() {
        // Контракт SPEC §4: фиксированные ключи, неизвестные отклоняются.
        let m = SchulteMetrics {
            mode: SchulteMode::Dkv,
            grid: "5x5".into(),
            symbols: SchulteSymbols::Digits,
            duration_ms: 41_230,
            errors: 1,
            cell_rt_ms: vec![812, 655],
        };
        let json = serde_json::to_string(&m).expect("ser");
        assert!(json.contains("\"mode\":\"dkv\""));
        assert!(json.contains("\"grid\":\"5x5\""));
        let back: SchulteMetrics = serde_json::from_str(&json).expect("de");
        assert_eq!(back, m);
        // Неизвестный ключ отклоняется (контракт)
        let bad = json.replace("\"errors\":1", "\"errors\":1,\"surprise\":true");
        assert!(serde_json::from_str::<SchulteMetrics>(&bad).is_err());
        // Отсутствие cell_rt_ms допустимо (serde default) — собираем
        // объект без ключа через Value, без хрупких replace
        let mut v: serde_json::Value = serde_json::from_str(&json).expect("value");
        v.as_object_mut().expect("obj").remove("cell_rt_ms");
        let no_rt = serde_json::to_string(&v).expect("ser");
        assert!(serde_json::from_str::<SchulteMetrics>(&no_rt).is_ok());
    }

    #[test]
    fn bicolor_layout_colors_deterministic() {
        let cfg = SchulteConfig {
            mode: SchulteMode::Classic,
            grid: 3,
            symbols: SchulteSymbols::Bicolor,
        };
        let a = Schulte::new(cfg.clone(), 10).expect("a");
        let b = Schulte::new(cfg, 10).expect("b");
        let ra: Vec<bool> = a.cells().iter().map(|c| c.red).collect();
        let rb: Vec<bool> = b.cells().iter().map(|c| c.red).collect();
        assert_eq!(ra, rb);
        // В 9 ячейках с p=0.5 почти наверняка есть оба цвета
        assert!(ra.iter().any(|&r| r) && ra.iter().any(|&r| !r));
    }
}
