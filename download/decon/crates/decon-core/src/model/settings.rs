//! Настройки приложения — таблица `settings` (ключ → JSON-значение).
//!
//! Правило №6: пользовательских конфиг-файлов нет, все настройки живут в БД
//! и редактируются в UI. Ключи — SPEC-data-model.md §5.

use serde::{Deserialize, Serialize};

/// Окно активности зондов дня (локальное время, `HH:MM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeWindow {
    /// Начало окна, часы 0..=23.
    pub start_h: u8,
    /// Начало окна, минуты 0..=59.
    pub start_m: u8,
    /// Конец окна, часы 0..=23.
    pub end_h: u8,
    /// Конец окна, минуты 0..=59.
    pub end_m: u8,
}

impl ProbeWindow {
    /// Окно в минутах от полуночи: `(начало, конец)`.
    pub const fn minutes(self) -> (u16, u16) {
        let start = self.start_h as u16 * 60 + self.start_m as u16;
        let end = self.end_h as u16 * 60 + self.end_m as u16;
        (start, end)
    }
}

/// Зонды дня (ESM): расписание случайных срезов состояния.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProbeSettings {
    /// Зонды включены.
    pub enabled: bool,
    /// Окно активности.
    pub window: ProbeWindow,
    /// Количество зондов в день (SCALES.md §5: 3–5).
    pub count: u8,
    /// Минимальный интервал между зондами, мин.
    pub min_gap_minutes: u16,
}

impl Default for ProbeSettings {
    fn default() -> Self {
        ProbeSettings {
            enabled: false,
            window: ProbeWindow {
                start_h: 9,
                start_m: 0,
                end_h: 23,
                end_m: 0,
            },
            count: 4,
            min_gap_minutes: 90,
        }
    }
}

/// Параметры сессии практики.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SessionSettings {
    /// Интервал равномерных фоновых сигналов, с (SPEC §5).
    pub signal_interval_s: u32,
}

impl Default for SessionSettings {
    fn default() -> Self {
        SessionSettings {
            signal_interval_s: 300,
        }
    }
}

/// Звук оповещения (зонды, сигналы, напоминания) — S2.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundChoice {
    /// Без звука (только визуальное уведомление).
    Off,
    /// Мягкий перезвон chime.
    Chime,
    /// Колокол bell.
    Bell,
}

impl SoundChoice {
    /// Код для `settings` (`ui.sound`).
    pub const fn as_code(self) -> &'static str {
        match self {
            SoundChoice::Off => "off",
            SoundChoice::Chime => "chime",
            SoundChoice::Bell => "bell",
        }
    }

    /// Из кода настройки.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "off" => Some(SoundChoice::Off),
            "chime" => Some(SoundChoice::Chime),
            "bell" => Some(SoundChoice::Bell),
            _ => None,
        }
    }

    /// Звук включён (не Off).
    pub const fn is_on(self) -> bool {
        !matches!(self, SoundChoice::Off)
    }
}

/// Резидентный режим (S2.5): трей, автозапуск, звуки, повторы зондов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResidencySettings {
    /// Запускаться вместе с системой (XDG autostart / HKCU Run).
    pub autostart: bool,
    /// Звук оповещений.
    pub sound: SoundChoice,
    /// Интервал повтора-«будильника» неотвеченного зонда, мин (0 — выкл).
    pub reminder_interval_min: u16,
    /// Показывать окно при запуске приложения (false — старт сразу в трей).
    pub show_window_on_start: bool,
}

impl Default for ResidencySettings {
    fn default() -> Self {
        ResidencySettings {
            autostart: false,
            sound: SoundChoice::Chime,
            reminder_interval_min: 15,
            show_window_on_start: true,
        }
    }
}

/// Режим хранения мастер-ключа шифрования (SPEC §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoMode {
    /// Ключ в OS keyring, ноль действий пользователя (по умолчанию).
    Auto,
    /// Ключ = argon2id(passphrase); passphrase не хранится.
    Passphrase,
}

/// Настройки шифрования.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoSettings {
    /// Режим ключа.
    pub mode: CryptoMode,
}

impl Default for CryptoSettings {
    fn default() -> Self {
        CryptoSettings {
            mode: CryptoMode::Auto,
        }
    }
}

/// Язык интерфейса (i18n ru/en с первого релиза).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Русский (язык первого сообщества).
    Ru,
    /// Английский.
    En,
}

impl Language {
    /// Код для `settings` (`i18n.lang`).
    pub const fn as_code(self) -> &'static str {
        match self {
            Language::Ru => "ru",
            Language::En => "en",
        }
    }

    /// Из кода настройки.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "ru" => Some(Language::Ru),
            "en" => Some(Language::En),
            _ => None,
        }
    }
}

/// Корневой набор настроек приложения: дефолты при первом запуске,
/// сериализация в `settings` (ключи — SPEC §5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Зонды дня.
    pub probe: ProbeSettings,
    /// Параметры сессии.
    pub session: SessionSettings,
    /// Резидентный режим (S2.5).
    pub residency: ResidencySettings,
    /// Шифрование.
    pub crypto: CryptoSettings,
    /// Язык интерфейса.
    pub language: Language,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            probe: ProbeSettings::default(),
            session: SessionSettings::default(),
            residency: ResidencySettings::default(),
            crypto: CryptoSettings::default(),
            language: Language::Ru,
        }
    }
}

/// Ключи таблицы `settings` (справочник SPEC §5, исчерпывающий для S1).
pub mod keys {
    /// Зонды: включены.
    pub const PROBE_ENABLED: &str = "probe.enabled";
    /// Зонды: окно, начало (`HH:MM`).
    pub const PROBE_WINDOW_START: &str = "probe.window_start";
    /// Зонды: окно, конец (`HH:MM`).
    pub const PROBE_WINDOW_END: &str = "probe.window_end";
    /// Зонды: количество в день.
    pub const PROBE_COUNT: &str = "probe.count";
    /// Зонды: минимальный интервал, мин.
    pub const PROBE_MIN_GAP: &str = "probe.min_gap_minutes";
    /// Сессия: интервал равномерных сигналов, с.
    pub const SESSION_SIGNAL_INTERVAL: &str = "session.signal_interval_s";
    /// Резидентный режим: автозапуск с системой.
    pub const RESIDENCY_AUTOSTART: &str = "residency.autostart";
    /// Резидентный режим: звук оповещений (`off`/`chime`/`bell`).
    pub const RESIDENCY_SOUND: &str = "residency.sound";
    /// Резидентный режим: интервал повтора зондов, мин (0 — выкл).
    pub const RESIDENCY_REMINDER: &str = "residency.reminder_interval_min";
    /// Резидентный режим: показывать окно при старте.
    pub const RESIDENCY_SHOW_WINDOW: &str = "residency.show_window_on_start";
    /// Шифрование: режим ключа.
    pub const CRYPTO_MODE: &str = "crypto.mode";
    /// Язык интерфейса.
    pub const I18N_LANG: &str = "i18n.lang";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = AppSettings::default();
        // SPEC §5: окно 09:00–23:00, 4 зонда, интервал 90 мин, сигнал 300 с
        assert_eq!(s.probe.window.minutes(), (9 * 60, 23 * 60));
        assert_eq!(s.probe.count, 4);
        assert_eq!(s.probe.min_gap_minutes, 90);
        assert_eq!(s.session.signal_interval_s, 300);
        assert_eq!(s.language, Language::Ru);
        assert_eq!(s.crypto.mode, CryptoMode::Auto);
    }

    #[test]
    fn settings_json_roundtrip() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn language_codes() {
        assert_eq!(Language::Ru.as_code(), "ru");
        assert_eq!(Language::from_code("ru"), Some(Language::Ru));
        assert_eq!(Language::from_code("en"), Some(Language::En));
        assert_eq!(Language::from_code("de"), None);
    }
}
