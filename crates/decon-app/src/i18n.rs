//! Локализация UI: ru/en с первого релиза (CONVENTIONS, «Язык проекта»).
//!
//! Никаких файлов переводов (правило №6: ноль конфигов) — строки в коде,
//! выбор — через `settings.i18n.lang` из БД.

use decon_core::model::settings::Language;

/// Перевод всех строк UI.
pub struct I18n {
    lang: Language,
}

/// Ключи строк UI (типобезопасно: забыть перевод невозможно — компилятор
/// потребует все ветки).
///
/// Часть ключей не используется в S1 (пауза/резюм, отмена, секунды,
/// техника) — они под уже спроектированные экраны S2+ (зонды, настройки,
/// редактор техник), чтобы не менять enum на каждом скоупе.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Str {
    /// «Decon — журнал практики»
    AppTitle,
    /// «Начать сессию»
    StartSession,
    /// «Слой»
    Layer,
    /// «Техника»
    Technique,
    /// «Техника (необязательно)»
    TechniqueOptional,
    /// «Часов сна»
    SleepHours,
    /// «Кофеин (доз за сутки)»
    CaffeineDoses,
    /// «Дальше»
    Next,
    /// «Назад»
    Back,
    /// «Начать»
    Begin,
    /// «Стоп»
    Stop,
    /// «Отменить сессию»
    CancelSession,
    /// «Заметка (шифруется)»
    NoteEncrypted,
    /// «Сохранить»
    Save,
    /// «Сессия идёт»
    SessionRunning,
    /// «Срыв в концентрацию»
    EvCollapse,
    /// «Уход в мысли»
    EvSemantic,
    /// «Уход в тело»
    EvBody,
    /// «Вспышка ясности»
    EvClarity,
    /// «Пауза»
    EvPause,
    /// «Продолжить»
    EvResume,
    /// «История»
    History,
    /// «Пусто: сессий ещё нет»
    HistoryEmpty,
    /// «Завершить сессию?»
    ConfirmFinish,
    /// «Оцените состояние до практики»
    PreTitle,
    /// «Оцените качество состояния»
    PostTitle,
    /// «Сессия сохранена»
    SessionSaved,
    /// «Мин»
    MinutesShort,
    /// «Сек»
    SecondsShort,
    /// «Ошибка»
    Error,
}

impl I18n {
    /// Локаль для языка из настроек.
    pub const fn new(lang: Language) -> Self {
        I18n { lang }
    }

    /// Строка по ключу.
    pub fn get(&self, key: Str) -> &'static str {
        match self.lang {
            Language::Ru => self.ru(key),
            Language::En => self.en(key),
        }
    }

    fn ru(&self, key: Str) -> &'static str {
        match key {
            Str::AppTitle => "Decon — журнал практики",
            Str::StartSession => "Начать сессию",
            Str::Layer => "Слой",
            Str::Technique => "Техника",
            Str::TechniqueOptional => "Техника (необязательно)",
            Str::SleepHours => "Часов сна",
            Str::CaffeineDoses => "Кофеин (доз за сутки)",
            Str::Next => "Дальше",
            Str::Back => "Назад",
            Str::Begin => "Начать",
            Str::Stop => "Стоп",
            Str::CancelSession => "Отменить сессию",
            Str::NoteEncrypted => "Заметка (шифруется)",
            Str::Save => "Сохранить",
            Str::SessionRunning => "Сессия идёт",
            Str::EvCollapse => "Срыв в концентрацию",
            Str::EvSemantic => "Уход в мысли",
            Str::EvBody => "Уход в тело",
            Str::EvClarity => "Вспышка ясности",
            Str::EvPause => "Пауза",
            Str::EvResume => "Продолжить",
            Str::History => "История",
            Str::HistoryEmpty => "Пусто: сессий ещё нет",
            Str::ConfirmFinish => "Завершить сессию?",
            Str::PreTitle => "Оцените состояние до практики",
            Str::PostTitle => "Оцените качество состояния",
            Str::SessionSaved => "Сессия сохранена",
            Str::MinutesShort => "мин",
            Str::SecondsShort => "сек",
            Str::Error => "Ошибка",
        }
    }

    fn en(&self, key: Str) -> &'static str {
        match key {
            Str::AppTitle => "Decon — practice journal",
            Str::StartSession => "Start session",
            Str::Layer => "Layer",
            Str::Technique => "Technique",
            Str::TechniqueOptional => "Technique (optional)",
            Str::SleepHours => "Hours of sleep",
            Str::CaffeineDoses => "Caffeine (doses in 24h)",
            Str::Next => "Next",
            Str::Back => "Back",
            Str::Begin => "Begin",
            Str::Stop => "Stop",
            Str::CancelSession => "Cancel session",
            Str::NoteEncrypted => "Note (encrypted)",
            Str::Save => "Save",
            Str::SessionRunning => "Session running",
            Str::EvCollapse => "Collapse into focus",
            Str::EvSemantic => "Drift into thoughts",
            Str::EvBody => "Drift into body",
            Str::EvClarity => "Clarity flash",
            Str::EvPause => "Pause",
            Str::EvResume => "Resume",
            Str::History => "History",
            Str::HistoryEmpty => "Empty: no sessions yet",
            Str::ConfirmFinish => "Finish session?",
            Str::PreTitle => "Rate your state before practice",
            Str::PostTitle => "Rate the quality of the state",
            Str::SessionSaved => "Session saved",
            Str::MinutesShort => "min",
            Str::SecondsShort => "sec",
            Str::Error => "Error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keys_translated_both_langs() {
        // Перебор всех ключей: непокрытая ветка = ошибка компиляции match,
        // здесь проверяем непустоту и различие языков хотя бы на одном ключе.
        let ru = I18n::new(Language::Ru);
        let en = I18n::new(Language::En);
        let keys = [
            Str::AppTitle,
            Str::StartSession,
            Str::Layer,
            Str::Technique,
            Str::TechniqueOptional,
            Str::SleepHours,
            Str::CaffeineDoses,
            Str::Next,
            Str::Back,
            Str::Begin,
            Str::Stop,
            Str::CancelSession,
            Str::NoteEncrypted,
            Str::Save,
            Str::SessionRunning,
            Str::EvCollapse,
            Str::EvSemantic,
            Str::EvBody,
            Str::EvClarity,
            Str::EvPause,
            Str::EvResume,
            Str::History,
            Str::HistoryEmpty,
            Str::ConfirmFinish,
            Str::PreTitle,
            Str::PostTitle,
            Str::SessionSaved,
            Str::MinutesShort,
            Str::SecondsShort,
            Str::Error,
        ];
        for k in keys {
            assert!(!ru.get(k).is_empty(), "ru пуст: {k:?}");
            assert!(!en.get(k).is_empty(), "en пуст: {k:?}");
        }
        assert_ne!(ru.get(Str::StartSession), en.get(Str::StartSession));
    }
}
