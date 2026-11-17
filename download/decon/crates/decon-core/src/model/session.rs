//! Сессия практики дКВ — центральная сущность журнала.
//!
//! DDL — `sessions` в docs/SPEC-data-model.md §3. Сессия с `kind = trainer`
//! создаётся тренажёрами (S3) и пишется в общий журнал наравне с практикой.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Тип записи журнала.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Практика дКВ (медитативная сессия).
    Practice,
    /// Прохождение тренажёра (прокси-метрики).
    Trainer,
}

/// Чем закончилась сессия (данные честности, SPEC §3 `sessions.end_status`).
///
/// `None`/NULL — сессия ещё идёт или завершена до появления колонки (S2.5);
/// завершённая сессия всегда имеет статус.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndStatus {
    /// Завершена штатно (кнопка «Стоп» → post-замер → сохранение).
    Finished,
    /// Прервана внешними силами: сигнал ОС, выход из приложения, сбой
    /// питания/краш (финализирована recovery-проходом при старте).
    Interrupted,
    /// Прервана пользователем вручную («Прервать сессию»: срочно нужно
    /// бежать, плохое самочувствие и т.п.).
    Aborted,
}

impl SessionEndStatus {
    /// Код для колонки `end_status`.
    pub const fn as_code(self) -> &'static str {
        match self {
            SessionEndStatus::Finished => "finished",
            SessionEndStatus::Interrupted => "interrupted",
            SessionEndStatus::Aborted => "aborted",
        }
    }

    /// Из кода БД.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "finished" => Some(SessionEndStatus::Finished),
            "interrupted" => Some(SessionEndStatus::Interrupted),
            "aborted" => Some(SessionEndStatus::Aborted),
            _ => None,
        }
    }
}

/// Слой дКВ, в котором велась работа (ARCHITECTURE.md §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Зрительный слой.
    Visual,
    /// Слуховой слой.
    Audio,
    /// Витальный (телесный) слой.
    Vital,
    /// Семантический слой.
    Semantic,
    /// Полимодальный фон.
    Polymodal,
}

impl Layer {
    /// Строковый код слоя для колонки `layer` (устойчив к переименованиям enum).
    pub const fn as_code(self) -> &'static str {
        match self {
            Layer::Visual => "visual",
            Layer::Audio => "audio",
            Layer::Vital => "vital",
            Layer::Semantic => "semantic",
            Layer::Polymodal => "polymodal",
        }
    }

    /// Обратное преобразование из кода колонки `layer`.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "visual" => Some(Layer::Visual),
            "audio" => Some(Layer::Audio),
            "vital" => Some(Layer::Vital),
            "semantic" => Some(Layer::Semantic),
            "polymodal" => Some(Layer::Polymodal),
            _ => None,
        }
    }

    /// Человекочитаемое имя слоя для UI.
    pub const fn display_name(self) -> &'static str {
        match self {
            Layer::Visual => "Зрительный",
            Layer::Audio => "Слуховой",
            Layer::Vital => "Витальный",
            Layer::Semantic => "Семантический",
            Layer::Polymodal => "Полимодальный",
        }
    }
}

/// Локальное время дня сессии — ковариата аналитики (SPEC §3 `sessions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalTimeBucket {
    /// Утро.
    Morning,
    /// День.
    Day,
    /// Вечер.
    Evening,
    /// Ночь.
    Night,
}

impl LocalTimeBucket {
    /// Код для колонки `local_time_bucket`.
    pub const fn as_code(self) -> &'static str {
        match self {
            LocalTimeBucket::Morning => "morning",
            LocalTimeBucket::Day => "day",
            LocalTimeBucket::Evening => "evening",
            LocalTimeBucket::Night => "night",
        }
    }

    /// Обратное преобразование из кода колонки.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "morning" => Some(LocalTimeBucket::Morning),
            "day" => Some(LocalTimeBucket::Day),
            "evening" => Some(LocalTimeBucket::Evening),
            "night" => Some(LocalTimeBucket::Night),
            _ => None,
        }
    }

    /// Ковертация локального часа суток в корзину времени дня.
    pub const fn from_hour(hour: u32) -> Self {
        match hour {
            5..=11 => LocalTimeBucket::Morning,
            12..=17 => LocalTimeBucket::Day,
            18..=22 => LocalTimeBucket::Evening,
            _ => LocalTimeBucket::Night,
        }
    }
}

/// Контекст условий сессии — ковариаты для n-of-1 аналитики (S4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionContext {
    /// Часов сна за прошедшую ночь.
    pub sleep_hours: Option<f64>,
    /// Доз кофеина за сутки до сессии (чашки/эквиваленты).
    pub caffeine: Option<i64>,
    /// Поза практики (свободный текст, не enum: пользовательская феноменология).
    pub posture: Option<String>,
}

/// Сессия практики дКВ — строка таблицы `sessions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// UUID v7.
    pub id: Uuid,
    /// Тип: практика или тренажёр.
    kind: SessionKind,
    /// Слой дКВ (`None` для тренажёров без привязки к слою).
    layer: Option<Layer>,
    /// Название техники (свободное; дефолты — SCALES.md).
    technique: Option<String>,
    /// Начало сессии (UTC, ISO-8601).
    started_at: String,
    /// Окончание (UTC, ISO-8601); `None` — идёт сейчас.
    ended_at: Option<String>,
    /// Длительность, с (заполняется при завершении).
    duration_s: Option<i64>,
    /// Чем закончилась сессия (S2.5); `None` — идёт или завершена до S2.5.
    end_status: Option<SessionEndStatus>,
    /// Локальная корзина времени дня (аналитика «время дня ↔ качество»).
    local_time_bucket: Option<LocalTimeBucket>,
    /// Контекст условий.
    pub context: SessionContext,
    /// Открытый текст заметки; шифруется при записи в БД (🔒 note_enc).
    pub note: Option<String>,
    /// Момент последнего обновления строки (sync).
    updated_at: String,
    /// Soft-delete (None = жива).
    pub deleted_at: Option<String>,
}

impl Session {
    /// Создать новую сессию: ID и `updated_at` проставляются здесь.
    pub fn new(kind: SessionKind, started_at: String, local_hour: u32) -> Self {
        let id = Uuid::now_v7();
        let bucket = LocalTimeBucket::from_hour(local_hour);
        Session {
            id,
            kind,
            layer: None,
            technique: None,
            started_at,
            ended_at: None,
            duration_s: None,
            end_status: None,
            local_time_bucket: Some(bucket),
            context: SessionContext::default(),
            note: None,
            updated_at: now_iso(),
            deleted_at: None,
        }
    }

    /// Тип сессии.
    pub const fn kind(&self) -> SessionKind {
        self.kind
    }

    /// Слой дКВ.
    pub const fn layer(&self) -> Option<Layer> {
        self.layer
    }

    /// Привязать сессию к слою.
    pub fn set_layer(&mut self, layer: Option<Layer>) {
        self.layer = layer;
        self.touch();
    }

    /// Название техники.
    pub fn technique(&self) -> Option<&str> {
        self.technique.as_deref()
    }

    /// Установить технику (свободное название).
    pub fn set_technique(&mut self, technique: Option<String>) {
        self.technique = technique;
        self.touch();
    }

    /// Начало сессии (ISO-8601 UTC).
    pub fn started_at(&self) -> &str {
        &self.started_at
    }

    /// Окончание сессии (ISO-8601 UTC), `None` — идёт.
    pub fn ended_at(&self) -> Option<&str> {
        self.ended_at.as_deref()
    }

    /// Длительность в секундах, если сессия завершена.
    pub const fn duration_s(&self) -> Option<i64> {
        self.duration_s
    }

    /// Локальная корзина времени дня.
    pub const fn local_time_bucket(&self) -> Option<LocalTimeBucket> {
        self.local_time_bucket
    }

    /// Завершить сессию: проставляет `ended_at` и `duration_s`.
    ///
    /// Повторный вызов возвращает `false` и ничего не меняет — защита от
    /// двойного нажатия «стоп» в UI.
    pub fn finish(&mut self, ended_at: String, duration_s: i64) -> bool {
        if self.ended_at.is_some() {
            return false;
        }
        self.ended_at = Some(ended_at);
        self.duration_s = Some(duration_s);
        self.touch();
        true
    }

    /// Финализировать сессию с указанным статусом завершения (S2.5).
    ///
    /// Один путь для всех сценариев: штатный стоп (`Finished`), сигнал ОС /
    /// выход из приложения (`Interrupted`), ручное прерывание
    /// (`Aborted`). Повторный вызов — no-op (idempotent), уже
    /// финализированная сессия не перезаписывается.
    pub fn finalize(
        &mut self,
        status: SessionEndStatus,
        ended_at: String,
        duration_s: i64,
    ) -> bool {
        if self.ended_at.is_some() {
            return false;
        }
        self.ended_at = Some(ended_at);
        self.duration_s = Some(duration_s);
        self.end_status = Some(status);
        self.touch();
        true
    }

    /// Статус завершения, если сессия финализирована.
    pub const fn end_status(&self) -> Option<SessionEndStatus> {
        self.end_status
    }

    /// Момент последнего изменения (sync-поле).
    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    /// Soft-delete отметка.
    pub fn deleted_at(&self) -> Option<&str> {
        self.deleted_at.as_deref()
    }

    /// Soft-delete: пометить удалённой.
    pub fn soft_delete(&mut self, at: String) {
        self.deleted_at = Some(at);
        self.touch();
    }

    /// Копия контекста условий сессии.
    pub fn context(&self) -> SessionContext {
        self.context.clone()
    }

    /// Сборка из готовых частей — путь репозитория при чтении из БД.
    ///
    /// В отличие от `new`, не генерирует ID/время и не трогает `updated_at`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        id: Uuid,
        kind: SessionKind,
        layer: Option<Layer>,
        technique: Option<String>,
        started_at: String,
        ended_at: Option<String>,
        duration_s: Option<i64>,
        end_status: Option<SessionEndStatus>,
        local_time_bucket: Option<LocalTimeBucket>,
        context: SessionContext,
        note: Option<String>,
        updated_at: String,
        deleted_at: Option<String>,
    ) -> Self {
        Session {
            id,
            kind,
            layer,
            technique,
            started_at,
            ended_at,
            duration_s,
            end_status,
            local_time_bucket,
            context,
            note,
            updated_at,
            deleted_at,
        }
    }

    /// Восстановить `updated_at` из БД без каскада `touch()` (репозиторий).
    pub fn sync_updated_at(&mut self, updated_at: String) {
        self.updated_at = updated_at;
    }

    /// Обновить `updated_at` (вызывается каждым сеттером).
    fn touch(&mut self) {
        self.updated_at = now_iso();
    }
}

/// Текущий момент UTC в ISO-8601 (`YYYY-MM-DDTHH:MM:SS.sssZ`).
///
/// Формат фиксирован SPEC-data-model.md §1.3. Реализация без внешних
/// крейтов: время из `std`, форматирование вручную (миллисекунды из
/// SystemTime как fraction Unix-эпохи).
pub fn now_iso() -> String {
    iso_from_unix_ms(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
}

/// Преобразовать Unix-миллисекунды в ISO-8601 UTC.
///
/// Годы в диапазоне 1970..=9999. Формула — григорианская арифметика
/// days-from-civil (Howard Hinnant), без таблиц.
pub fn iso_from_unix_ms(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let ms = unix_ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z")
}

/// Обратное преобразование ISO-8601 UTC → Unix-миллисекунды.
///
/// Принимает каноничный формат SPEC §1.3 (с `.sss` и `Z`). Возвращает
/// `None` на мусоре — парсинг в домене не паникует.
pub fn unix_ms_from_iso(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() != 24 || bytes[10] != b'T' || bytes[19] != b'.' || bytes[23] != b'Z' {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = s.get(range)?;
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse::<i64>().ok()
    };
    let year = num(0..4)?;
    let month = num(5..7)?;
    let day = num(8..10)?;
    let hour = num(11..13)?;
    let minute = num(14..16)?;
    let second = num(17..19)?;
    let ms = num(20..23)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(((days * 86_400 + hour * 3600 + minute * 60 + second) * 1000) + ms)
}

/// Дни от 1970-01-01 до год-месяц-день (Howard Hinnant, days_from_civil).
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Обратная к `days_from_civil`: (год, месяц, день) из дней от эпохи.
const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_roundtrip_known_moments() {
        // Эталон сверен с datetime UTC (Python): 2025-02-14T21:40:00Z и
        // 2026-11-20T18:40:00Z. Дополнительно — границы эпохи.
        let cases: [(i64, &str); 4] = [
            (0, "1970-01-01T00:00:00.000Z"),
            (86_400_000, "1970-01-02T00:00:00.000Z"),
            (1_739_569_200_000, "2025-02-14T21:40:00.000Z"),
            (1_795_200_000_000, "2026-11-20T18:40:00.000Z"),
        ];
        for (unix_ms, iso) in cases {
            assert_eq!(iso_from_unix_ms(unix_ms), iso, "unix={unix_ms}");
            assert_eq!(unix_ms_from_iso(iso), Some(unix_ms), "iso={iso}");
        }
    }

    #[test]
    fn iso_rejects_garbage() {
        assert_eq!(unix_ms_from_iso(""), None);
        assert_eq!(unix_ms_from_iso("2026-11-16"), None);
        assert_eq!(unix_ms_from_iso("2026-13-16T13:30:23.123Z"), None);
        assert_eq!(unix_ms_from_iso("2026-11-32T13:30:23.123Z"), None);
        assert_eq!(unix_ms_from_iso("not-a-date!"), None);
    }

    #[test]
    fn iso_all_days_of_era_roundtrip() {
        // Плотный smoke: каждый 37-й день 400-летней эры туда-обратно.
        let mut day = -10_000i64;
        while day < 150_000 {
            let (y, m, d) = civil_from_days(day);
            let iso = iso_from_unix_ms(day * 86_400_000);
            assert_eq!(
                unix_ms_from_iso(&iso),
                Some(day * 86_400_000),
                "day={day} iso={iso}"
            );
            assert_eq!(days_from_civil(y, m, d), day, "civil={y}-{m}-{d}");
            day += 37;
        }
    }

    #[test]
    fn session_lifecycle() {
        let mut s = Session::new(SessionKind::Practice, "2026-11-16T13:00:00.000Z".into(), 13);
        assert_eq!(s.kind(), SessionKind::Practice);
        assert_eq!(s.local_time_bucket(), Some(LocalTimeBucket::Day));
        assert!(s.ended_at().is_none());
        assert!(s.end_status().is_none(), "новая сессия без статуса");

        s.set_layer(Some(Layer::Visual));
        assert_eq!(s.layer(), Some(Layer::Visual));
        s.set_technique(Some("дКВ 1.0 визуальная".into()));
        assert_eq!(s.technique(), Some("дКВ 1.0 визуальная"));

        assert!(s.finish("2026-11-16T13:21:00.000Z".into(), 1260));
        assert_eq!(s.duration_s(), Some(1260));
        // Повторный finish — no-op (защита от двойного «стоп»)
        assert!(!s.finish("2026-11-16T13:25:00.000Z".into(), 1500));
        assert_eq!(s.duration_s(), Some(1260));

        s.soft_delete("2026-11-16T14:00:00.000Z".into());
        assert!(s.deleted_at().is_some());
    }

    #[test]
    fn finalize_sets_status_and_is_idempotent() {
        // Все три пути завершения пишут свой статус; повтор — no-op.
        let mk = || Session::new(SessionKind::Practice, "2026-11-16T10:00:00.000Z".into(), 10);
        let mut a = mk();
        assert!(a.finalize(
            SessionEndStatus::Interrupted,
            "2026-11-16T10:12:00.000Z".into(),
            720
        ));
        assert_eq!(a.end_status(), Some(SessionEndStatus::Interrupted));
        assert_eq!(a.duration_s(), Some(720));
        // Повторная финализация (двойной SIGTERM) не перезаписывает
        assert!(!a.finalize(
            SessionEndStatus::Aborted,
            "2026-11-16T10:13:00.000Z".into(),
            780
        ));
        assert_eq!(a.end_status(), Some(SessionEndStatus::Interrupted));

        let mut b = mk();
        assert!(b.finalize(
            SessionEndStatus::Aborted,
            "2026-11-16T10:05:00.000Z".into(),
            300
        ));
        assert_eq!(b.end_status(), Some(SessionEndStatus::Aborted));

        let mut c = mk();
        assert!(c.finalize(
            SessionEndStatus::Finished,
            "2026-11-16T10:20:00.000Z".into(),
            1200
        ));
        assert_eq!(c.end_status(), Some(SessionEndStatus::Finished));
        // Коды roundtrip
        for st in [
            SessionEndStatus::Finished,
            SessionEndStatus::Interrupted,
            SessionEndStatus::Aborted,
        ] {
            assert_eq!(SessionEndStatus::from_code(st.as_code()), Some(st));
        }
        assert_eq!(SessionEndStatus::from_code("nope"), None);
    }

    #[test]
    fn time_bucket_boundaries() {
        assert_eq!(LocalTimeBucket::from_hour(4), LocalTimeBucket::Night);
        assert_eq!(LocalTimeBucket::from_hour(5), LocalTimeBucket::Morning);
        assert_eq!(LocalTimeBucket::from_hour(11), LocalTimeBucket::Morning);
        assert_eq!(LocalTimeBucket::from_hour(12), LocalTimeBucket::Day);
        assert_eq!(LocalTimeBucket::from_hour(17), LocalTimeBucket::Day);
        assert_eq!(LocalTimeBucket::from_hour(18), LocalTimeBucket::Evening);
        assert_eq!(LocalTimeBucket::from_hour(22), LocalTimeBucket::Evening);
        assert_eq!(LocalTimeBucket::from_hour(23), LocalTimeBucket::Night);
        assert_eq!(LocalTimeBucket::from_hour(0), LocalTimeBucket::Night);
    }

    #[test]
    fn layer_codes_roundtrip() {
        for layer in [
            Layer::Visual,
            Layer::Audio,
            Layer::Vital,
            Layer::Semantic,
            Layer::Polymodal,
        ] {
            assert_eq!(Layer::from_code(layer.as_code()), Some(layer));
        }
        assert_eq!(Layer::from_code("nope"), None);
    }
}
