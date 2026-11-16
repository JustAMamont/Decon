//! Шкалы самоотчёта: определение, версии, ответы.
//!
//! DDL — `scales`, `scale_versions`, `responses` (SPEC §3). Дефолтный набор
//! v1 «Baseline-2026» — SCALES.md §2–§5; сидинг идемпотентен (перезапись
//! только если формулировка изменилась — тогда это новая версия).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Тип шкалы (колонка `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleKind {
    /// Слайдер 0–10 с якорями на концах.
    Slider,
    /// Флаг да/нет (0|1).
    Flag,
    /// Факторный балл (внешний инструмент, напр. FFMQ).
    Factor,
}

impl ScaleKind {
    /// Код для колонки `kind`.
    pub const fn as_code(self) -> &'static str {
        match self {
            ScaleKind::Slider => "slider",
            ScaleKind::Flag => "flag",
            ScaleKind::Factor => "factor",
        }
    }

    /// Из кода БД.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "slider" => Some(ScaleKind::Slider),
            "flag" => Some(ScaleKind::Flag),
            "factor" => Some(ScaleKind::Factor),
            _ => None,
        }
    }
}

/// Фаза замера: где шкала используется (SCALES.md §2–§5).
///
/// Маска: `scale.openness` живёт сразу в Pre и Probe (одна шкала — один
/// ряд данных, ROADMAP S2 критерий 4), поэтому фаз у шкалы может быть
/// несколько. `Phases` — набор фаз.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// До сессии (состояние входа).
    Pre,
    /// Сразу после сессии (качество состояния).
    Post,
    /// Хвост сессии (30–60 мин спустя, опционально).
    Carryover,
    /// Зонд дня (ESM-срез).
    Probe,
    /// Слойная карта (раз в 2–4 недели).
    Layer,
}

impl Phase {
    /// Код фазы.
    pub const fn as_code(self) -> &'static str {
        match self {
            Phase::Pre => "pre",
            Phase::Post => "post",
            Phase::Carryover => "carryover",
            Phase::Probe => "probe",
            Phase::Layer => "layer",
        }
    }

    /// Бит маски `Phases`.
    const fn bit(self) -> u8 {
        match self {
            Phase::Pre => 1 << 0,
            Phase::Post => 1 << 1,
            Phase::Carryover => 1 << 2,
            Phase::Probe => 1 << 3,
            Phase::Layer => 1 << 4,
        }
    }
}

/// Набор фаз применения шкалы (битовая маска фаз `Phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phases(u8);

impl Phases {
    /// Пустой набор (внутренняя утилита конструктора).
    pub const NONE: Phases = Phases(0);
    /// Одна фаза.
    pub const fn single(phase: Phase) -> Self {
        Phases(phase.bit())
    }

    /// Добавить фазу (builder).
    #[must_use]
    pub const fn with(self, phase: Phase) -> Self {
        Phases(self.0 | phase.bit())
    }

    /// Фаза входит в набор?
    pub const fn contains(self, phase: Phase) -> bool {
        self.0 & phase.bit() != 0
    }

    /// Хотя бы одна фаза?
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Определение шкалы — строка `scales`.
///
/// `id` стабилен («post.uniformity»), формулировки живут в версиях.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    /// Стабильный идентификатор (доменное имя, не UUID).
    pub id: String,
    /// Короткое название («Равномерность»).
    pub title: String,
    /// Якорь низа (дублирует текущую версию — нужен в списке UI).
    pub anchor_low: String,
    /// Якорь верха.
    pub anchor_high: String,
    /// Тип шкалы.
    pub kind: ScaleKind,
    /// Фазы применения (набор: `scale.openness` живёт в pre и probe).
    pub phases: Phases,
    /// Активна (деактивация скрывает из UI, история остаётся).
    pub active: bool,
}

/// Версия формулировки шкалы — строка `scale_versions`.
///
/// Изменение вопроса/якорей = новая версия; тренды не смешиваются
/// (SCALES.md §8.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScaleVersion {
    /// UUID v7.
    pub id: Uuid,
    /// Владелец-шкала.
    pub scale_id: String,
    /// Номер версии (растёт на 1).
    pub version: i64,
    /// Текст вопроса.
    pub question: String,
    /// Якорь низа.
    pub anchor_low: String,
    /// Якорь верха.
    pub anchor_high: String,
}

/// Ответ по шкале — строка `responses`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// UUID v7.
    pub id: Uuid,
    /// Сессия (для pre/post/carryover).
    pub session_id: Option<Uuid>,
    /// Зонд (для ESM-ответов).
    pub probe_id: Option<Uuid>,
    /// Версия шкалы (фиксирует формулировку на момент ответа).
    pub scale_version_id: Uuid,
    /// Значение: 0..=10 (slider) / 0|1 (flag).
    pub value: f64,
    /// Момент ответа (ISO-8601 UTC).
    pub answered_at: String,
    /// Задержка ответа, мин (догоняющие зонды; >0).
    pub delay_minutes: Option<i64>,
}

impl Response {
    /// Новый ответ на шкалу сессии.
    pub fn for_session(session_id: Uuid, scale_version_id: Uuid, value: f64) -> Self {
        Response {
            id: Uuid::now_v7(),
            session_id: Some(session_id),
            probe_id: None,
            scale_version_id,
            value,
            answered_at: crate::model::session::now_iso(),
            delay_minutes: None,
        }
    }

    /// Новый ответ на шкалу зонда (ESM). `delay_minutes` — задержка
    /// догоняющего ответа (>0), свежий ответ — None.
    pub fn for_probe(
        probe_id: Uuid,
        scale_version_id: Uuid,
        value: f64,
        delay_minutes: Option<i64>,
    ) -> Self {
        Response {
            id: Uuid::now_v7(),
            session_id: None,
            probe_id: Some(probe_id),
            scale_version_id,
            value,
            answered_at: crate::model::session::now_iso(),
            delay_minutes,
        }
    }

    /// Валидация значения по типу шкалы (0..=10 или 0|1).
    pub fn is_valid_value(value: f64, kind: ScaleKind) -> bool {
        match kind {
            ScaleKind::Slider | ScaleKind::Factor => {
                (0.0..=10.0).contains(&value) && value.is_finite()
            }
            ScaleKind::Flag => value == 0.0 || value == 1.0,
        }
    }
}

/// Дефолтный набор шкал v1 «Baseline-2026» (SCALES.md §2–§5).
///
/// Формулировки зафиксированы документом; сидинг пишет их при первом
/// запуске. Якорь `scale.openness` один на фазы pre/probe (единый ряд
/// данных — ROADMAP S2, критерий 4).
pub fn default_scales_v1() -> Vec<Scale> {
    let s = |id: &str, title: &str, low: &str, high: &str, phases: Phases| Scale {
        id: id.to_string(),
        title: title.to_string(),
        anchor_low: low.to_string(),
        anchor_high: high.to_string(),
        kind: ScaleKind::Slider,
        phases,
        active: true,
    };
    vec![
        // --- Pre (SCALES.md §2)
        s(
            "pre.noise",
            "Шум в голове",
            "Полная внутренняя тишина",
            "Мысли захлёстывают, поток не останавливается",
            Phases::single(Phase::Pre),
        ),
        // Одна шкала в двух контекстах (ROADMAP S2, критерий 4):
        // pre-замер и зонды дня дают единый ряд по scale_id.
        Scale {
            id: "scale.openness".to_string(),
            title: "Открытость поля".to_string(),
            anchor_low: "Полностью сужено на одном объекте".to_string(),
            anchor_high: "Ровно открытое, разлитое поле".to_string(),
            kind: ScaleKind::Slider,
            phases: Phases::NONE.with(Phase::Pre).with(Phase::Probe),
            active: true,
        },
        // --- Post (SCALES.md §3)
        s(
            "post.uniformity",
            "Равномерность",
            "Всё стянуто в одну-две точки",
            "Идеально ровно по всему полю",
            Phases::single(Phase::Post),
        ),
        s(
            "post.background",
            "Осознаваемый фон",
            "Фон полностью исчез из сознания, видел только объекты",
            "Фон воспринимался весь, целиком, не хуже объектов",
            Phases::single(Phase::Post),
        ),
        s(
            "post.depth",
            "Глубина",
            "Лишь скользнул по поверхности",
            "Максимальная глубина за всю мою практику",
            Phases::single(Phase::Post),
        ),
        s(
            "post.stability",
            "Стабильность",
            "Рассыпалось почти сразу",
            "Держалось всю сессию без провалов",
            Phases::single(Phase::Post),
        ),
        s(
            "post.dissolution",
            "Растворение фигур",
            "Фигуры резко ярче фона, фон не виден",
            "Всё слилось в единое равномерное поле",
            Phases::single(Phase::Post),
        ),
        // --- Carryover (SCALES.md §4)
        s(
            "post.carryover",
            "Остаточный фон",
            "Полностью растворилось",
            "Держится в полную силу",
            Phases::single(Phase::Carryover),
        ),
        // --- Probe (SCALES.md §5): openness уже выше (единая шкала),
        // остаётся спонтанность + флаг практики
        s(
            "probe.spontaneity",
            "Спонтанность",
            "Только через усилие",
            "Само собой, привычно",
            Phases::single(Phase::Probe),
        ),
        Scale {
            id: "probe.practiced_today".to_string(),
            title: "Практика сегодня".to_string(),
            anchor_low: "нет".to_string(),
            anchor_high: "да".to_string(),
            kind: ScaleKind::Flag,
            phases: Phases::single(Phase::Probe),
            active: true,
        },
        // --- Layer map (SCALES.md §6.2)
        s(
            "layer.availability",
            "Доступность слоя",
            "Ни разу не получалось",
            "Вхожу легко, по желанию",
            Phases::single(Phase::Layer),
        ),
        s(
            "layer.holding",
            "Удержание слоя",
            "Рассыпается за секунды",
            "Держится многие минуты без усилия",
            Phases::single(Phase::Layer),
        ),
    ]
}

/// Вопросы дефолтных шкал v1 (id → вопрос, SCALES.md §2–§6).
///
/// Вынесено отдельно от `default_scales_v1`, потому что в `scales`
/// хранятся только якоря, а вопрос — в `scale_versions`.
pub fn default_questions_v1() -> std::collections::HashMap<String, String> {
    let pairs: [(&str, &str); 12] = [
        (
            "pre.noise",
            "Насколько сейчас активен внутренний диалог / умственный шум?",
        ),
        (
            "scale.openness",
            "Насколько ваше внимание прямо сейчас расширено, а не сужено на чём-то одном?",
        ),
        (
            "post.uniformity",
            "Внимание было распределено по всему полю равномерно?",
        ),
        (
            "post.background",
            "Фон восприятия был именно осознаваемым (виден целиком), а не «выключенным»?",
        ),
        ("post.depth", "Как глубоко вы погрузились в состояние?"),
        (
            "post.stability",
            "Какую долю времени сессии состояние удерживалось без срывов?",
        ),
        (
            "post.dissolution",
            "Отдельные объекты теряли свою выделенность, сливались с фоном?",
        ),
        (
            "post.carryover",
            "Состояние (открытость, равномерность) всё ещё присутствует само собой?",
        ),
        (
            "probe.spontaneity",
            "Если внимание открытое — оно возникло само, без усилия?",
        ),
        ("probe.practiced_today", "Сегодня уже практиковали?"),
        ("layer.availability", "Удавалось ли входить в этот слой?"),
        ("layer.holding", "Как долго удавалось удерживать слой?"),
    ];
    pairs
        .into_iter()
        .map(|(id, q)| (id.to_string(), q.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_set_matches_scales_md() {
        // pre 2 + post 5 + carryover 1 + probe 3 + layer 2, но openness —
        // одна шкала на два контекста → 12 определений (ROADMAP S2, кр. 4)
        let scales = default_scales_v1();
        assert_eq!(scales.len(), 12);
        assert_eq!(
            scales
                .iter()
                .filter(|s| s.phases.contains(Phase::Pre))
                .count(),
            2,
            "pre: noise, openness"
        );
        assert_eq!(
            scales
                .iter()
                .filter(|s| s.phases.contains(Phase::Post))
                .count(),
            5,
            "post: uniformity, background, depth, stability, dissolution"
        );
        assert_eq!(
            scales
                .iter()
                .filter(|s| s.phases.contains(Phase::Carryover))
                .count(),
            1
        );
        assert_eq!(
            scales
                .iter()
                .filter(|s| s.phases.contains(Phase::Probe))
                .count(),
            3,
            "probe: openness (общая), spontaneity, practiced_today"
        );
        assert_eq!(
            scales
                .iter()
                .filter(|s| s.phases.contains(Phase::Layer))
                .count(),
            2
        );

        // Критерий №4 ROADMAP S2: openness — один scale_id в двух фазах
        let openness = scales
            .iter()
            .find(|s| s.id == "scale.openness")
            .expect("scale.openness существует");
        assert!(openness.phases.contains(Phase::Pre));
        assert!(openness.phases.contains(Phase::Probe));
        assert!(!scales.iter().any(|s| s.id == "pre.openness"));
        assert!(!scales.iter().any(|s| s.id == "probe.openness"));

        // ID уникальны
        let ids: std::collections::HashSet<&String> = scales.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), scales.len());

        // Один флаг, остальные слайдеры
        assert_eq!(
            scales.iter().filter(|s| s.kind == ScaleKind::Flag).count(),
            1
        );
        assert_eq!(
            scales
                .iter()
                .filter(|s| s.kind == ScaleKind::Slider)
                .count(),
            11
        );

        // У каждой шкалы есть вопрос
        let questions = default_questions_v1();
        for s in &scales {
            assert!(questions.contains_key(&s.id), "нет вопроса для {}", s.id);
        }
    }

    #[test]
    fn value_validation() {
        assert!(Response::is_valid_value(0.0, ScaleKind::Slider));
        assert!(Response::is_valid_value(10.0, ScaleKind::Slider));
        assert!(Response::is_valid_value(5.5, ScaleKind::Slider));
        assert!(!Response::is_valid_value(-0.1, ScaleKind::Slider));
        assert!(!Response::is_valid_value(10.1, ScaleKind::Slider));
        assert!(!Response::is_valid_value(f64::NAN, ScaleKind::Slider));
        assert!(Response::is_valid_value(0.0, ScaleKind::Flag));
        assert!(Response::is_valid_value(1.0, ScaleKind::Flag));
        assert!(!Response::is_valid_value(0.5, ScaleKind::Flag));
    }

    #[test]
    fn scale_kind_codes() {
        assert_eq!(ScaleKind::Slider.as_code(), "slider");
        assert_eq!(ScaleKind::from_code("flag"), Some(ScaleKind::Flag));
        assert_eq!(ScaleKind::from_code("bogus"), None);
    }
}
