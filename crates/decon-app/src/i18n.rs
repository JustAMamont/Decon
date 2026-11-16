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
    // --- S2: зонды
    /// «Зонд дня»
    ProbeTitle,
    /// «Зонды дня»
    ProbeList,
    /// «Догоняющий ответ: плановое время»
    ProbeCatchupHint,
    /// «Ответить сейчас»
    AnswerNow,
    /// «Завершён»
    ProbeDone,
    /// «Пропустить»
    Skip,
    // --- S2: инструменты
    /// «Инструменты»
    Tools,
    /// «Пройдите опросник и введите факторные баллы (0–100)»
    FfmqHint,
    /// «История FFMQ»
    FfmqHistory,
    /// «Наблюдение»
    FfmqObserving,
    /// «Описание»
    FfmqDescribing,
    /// «Действие с осознаванием»
    FfmqActing,
    /// «Нереактивность»
    FfmqNonreactivity,
    /// «Неосуждение»
    FfmqNonjudging,
    /// «Слойная карта»
    LayerMap,
    /// «Оцените доступность и удержание каждого слоя за 2 недели»
    LayerMapHint,
    // --- S2: настройки
    /// «Настройки»
    Settings,
    /// «Начало окна зондов (ЧЧ:ММ)»
    ProbeWindowStart,
    /// «Конец окна зондов (ЧЧ:ММ)»
    ProbeWindowEnd,
    /// «Зондов в день»
    ProbeCount,
    /// «Мин. интервал, мин»
    ProbeMinGap,
    /// «Интервал сигналов, с»
    SignalInterval,
    /// «Зонды включены»
    ProbesEnabled,
    /// «вкл»
    On,
    /// «выкл»
    Off,
    /// «Язык»
    Language,
    /// «Хранилище ключа»
    KeyringStatus,
    /// «системный keyring»
    KeyringOs,
    /// «память (нет сервиса)»
    KeyringMemory,
    // --- S2.5: резидентный режим
    /// «Резидентный режим»
    ResidencyTitle,
    /// «Запускаться вместе с системой»
    Autostart,
    /// «Звук оповещений»
    SoundSetting,
    /// «Без звука»
    SoundOff,
    /// «Перезвон»
    SoundChime,
    /// «Колокол»
    SoundBell,
    /// «Интервал повтора зондов, мин (0 — выкл)»
    ReminderInterval,
    /// «Показывать окно при запуске»
    ShowWindowOnStart,
    /// «Тест звука»
    TestSound,
    /// «Сессия прервана (данные сохранены)»
    SessionInterrupted,
    // --- S3: тренажёры
    /// «Тренажёры»
    Trainers,
    /// «Таблицы Шульте»
    SchulteTitle,
    /// «Multiple Object Tracking»
    MotTitle,
    /// «Прокси-приборы внимания: результат пишется в общий журнал»
    TrainersHint,
    /// «Режим»
    TrainerMode,
    /// «Классика»
    SchulteClassic,
    /// «Поле (фиксация в центре)»
    SchulteField,
    /// «дКВ (расфокус)»
    SchulteDkv,
    /// «Сетка»
    SchulteGrid,
    /// «Найдите числа по порядку»
    SchulteInstruction,
    /// «Ищите: {n}» — текущее число
    SchulteFindNext,
    /// «Тапайте по целям»
    MotInstruction,
    /// «Запомните подсвеченные объекты»
    MotCueHint,
    /// «Отслеживайте их движением глаз»
    MotMotionHint,
    /// «Выберите подсвеченные в начале»
    MotAnswerHint,
    /// «Целей»
    MotTargets,
    /// «Дистракторов»
    MotDistractors,
    /// «Скорость»
    MotSpeed,
    /// «Готово»
    TrainerDone,
    /// «Ошибок: {n}»
    TrainerErrors,
    /// «Время: {t}»
    TrainerTime,
    /// «Корректных: {p}%»
    TrainerCorrectPct,
    /// «Результат сохранён в журнал»
    TrainerSaved,
    /// «Символы»
    SchulteSymbols,
    /// «Цифры»
    SymbolsDigits,
    /// «Буквы»
    SymbolsLetters,
    /// «Двуцветные»
    SymbolsBicolor,
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
            Str::ProbeTitle => "Зонд дня",
            Str::ProbeList => "Зонды дня",
            Str::ProbeCatchupHint => "Догоняющий ответ — плановое время",
            Str::AnswerNow => "Ответить сейчас",
            Str::ProbeDone => "Завершён",
            Str::Skip => "Пропустить",
            Str::Tools => "Инструменты",
            Str::FfmqHint => "Пройдите опросник вне Decon и введите факторные баллы (0–100)",
            Str::FfmqHistory => "История FFMQ",
            Str::FfmqObserving => "Наблюдение",
            Str::FfmqDescribing => "Описание",
            Str::FfmqActing => "Действие с осознаванием",
            Str::FfmqNonreactivity => "Нереактивность",
            Str::FfmqNonjudging => "Неосуждение",
            Str::LayerMap => "Слойная карта",
            Str::LayerMapHint => {
                "Оцените доступность и удержание каждого слоя за последние 2 недели"
            }
            Str::Settings => "Настройки",
            Str::ProbeWindowStart => "Начало окна зондов (ЧЧ:ММ)",
            Str::ProbeWindowEnd => "Конец окна зондов (ЧЧ:ММ)",
            Str::ProbeCount => "Зондов в день",
            Str::ProbeMinGap => "Мин. интервал, мин",
            Str::SignalInterval => "Интервал сигналов, с",
            Str::ProbesEnabled => "Зонды",
            Str::On => "вкл",
            Str::Off => "выкл",
            Str::Language => "Язык",
            Str::KeyringStatus => "Хранилище ключа",
            Str::KeyringOs => "системный keyring",
            Str::KeyringMemory => "память (нет сервиса)",
            Str::ResidencyTitle => "Резидентный режим",
            Str::Autostart => "Запускаться вместе с системой",
            Str::SoundSetting => "Звук оповещений",
            Str::SoundOff => "Без звука",
            Str::SoundChime => "Перезвон",
            Str::SoundBell => "Колокол",
            Str::ReminderInterval => "Интервал повтора зондов, мин (0 — выкл)",
            Str::ShowWindowOnStart => "Показывать окно при запуске",
            Str::TestSound => "Тест звука",
            Str::SessionInterrupted => "Сессия прервана (данные сохранены)",
            Str::Trainers => "Тренажёры",
            Str::SchulteTitle => "Таблицы Шульте",
            Str::MotTitle => "Multiple Object Tracking",
            Str::TrainersHint => "Прокси-приборы внимания: результат пишется в общий журнал",
            Str::TrainerMode => "Режим",
            Str::SchulteClassic => "Классика",
            Str::SchulteField => "Поле (фиксация в центре)",
            Str::SchulteDkv => "дКВ (расфокус)",
            Str::SchulteGrid => "Сетка",
            Str::SchulteInstruction => "Найдите числа по порядку",
            Str::SchulteFindNext => "Ищите: {n}",
            Str::MotInstruction => "Тапайте по целям",
            Str::MotCueHint => "Запомните подсвеченные объекты",
            Str::MotMotionHint => "Отслеживайте их движением глаз",
            Str::MotAnswerHint => "Выберите подсвеченные в начале",
            Str::MotTargets => "Целей",
            Str::MotDistractors => "Дистракторов",
            Str::MotSpeed => "Скорость",
            Str::TrainerDone => "Готово",
            Str::TrainerErrors => "Ошибок: {n}",
            Str::TrainerTime => "Время: {t}",
            Str::TrainerCorrectPct => "Корректных: {p}%",
            Str::TrainerSaved => "Результат сохранён в журнал",
            Str::SchulteSymbols => "Символы",
            Str::SymbolsDigits => "Цифры",
            Str::SymbolsLetters => "Буквы",
            Str::SymbolsBicolor => "Двуцветные",
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
            Str::ProbeTitle => "Day probe",
            Str::ProbeList => "Day probes",
            Str::ProbeCatchupHint => "Catch-up answer — scheduled at",
            Str::AnswerNow => "Answer now",
            Str::ProbeDone => "Done",
            Str::Skip => "Skip",
            Str::Tools => "Tools",
            Str::FfmqHint => {
                "Take the questionnaire outside Decon, then enter factor scores (0–100)"
            }
            Str::FfmqHistory => "FFMQ history",
            Str::FfmqObserving => "Observing",
            Str::FfmqDescribing => "Describing",
            Str::FfmqActing => "Acting with awareness",
            Str::FfmqNonreactivity => "Non-reactivity",
            Str::FfmqNonjudging => "Non-judging",
            Str::LayerMap => "Layer map",
            Str::LayerMapHint => {
                "Rate availability and holding of each layer over the last 2 weeks"
            }
            Str::Settings => "Settings",
            Str::ProbeWindowStart => "Probe window start (HH:MM)",
            Str::ProbeWindowEnd => "Probe window end (HH:MM)",
            Str::ProbeCount => "Probes per day",
            Str::ProbeMinGap => "Min gap, minutes",
            Str::SignalInterval => "Signal interval, s",
            Str::ProbesEnabled => "Probes",
            Str::On => "on",
            Str::Off => "off",
            Str::Language => "Language",
            Str::KeyringStatus => "Key storage",
            Str::KeyringOs => "system keyring",
            Str::KeyringMemory => "memory (no service)",
            Str::ResidencyTitle => "Resident mode",
            Str::Autostart => "Start with the system",
            Str::SoundSetting => "Alert sound",
            Str::SoundOff => "Silent",
            Str::SoundChime => "Chime",
            Str::SoundBell => "Bell",
            Str::ReminderInterval => "Probe reminder interval, min (0 — off)",
            Str::ShowWindowOnStart => "Show window on launch",
            Str::TestSound => "Test sound",
            Str::SessionInterrupted => "Session interrupted (data saved)",
            Str::Trainers => "Trainers",
            Str::SchulteTitle => "Schulte tables",
            Str::MotTitle => "Multiple Object Tracking",
            Str::TrainersHint => "Attention proxy instruments: results go to the common journal",
            Str::TrainerMode => "Mode",
            Str::SchulteClassic => "Classic",
            Str::SchulteField => "Field (center fixation)",
            Str::SchulteDkv => "Open monitoring (defocus)",
            Str::SchulteGrid => "Grid",
            Str::SchulteInstruction => "Find the numbers in order",
            Str::SchulteFindNext => "Find: {n}",
            Str::MotInstruction => "Tap the targets",
            Str::MotCueHint => "Memorize the highlighted objects",
            Str::MotMotionHint => "Track them with your eyes",
            Str::MotAnswerHint => "Select the ones highlighted at the start",
            Str::MotTargets => "Targets",
            Str::MotDistractors => "Distractors",
            Str::MotSpeed => "Speed",
            Str::TrainerDone => "Done",
            Str::TrainerErrors => "Errors: {n}",
            Str::TrainerTime => "Time: {t}",
            Str::TrainerCorrectPct => "Correct: {p}%",
            Str::TrainerSaved => "Result saved to journal",
            Str::SchulteSymbols => "Symbols",
            Str::SymbolsDigits => "Digits",
            Str::SymbolsLetters => "Letters",
            Str::SymbolsBicolor => "Bicolor",
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
