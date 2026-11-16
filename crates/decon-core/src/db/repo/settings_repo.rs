//! Репозиторий настроек (`settings`: ключ → JSON).
//!
//! Правило №6: ноль конфиг-файлов, все настройки в БД, UI редактирует.

use crate::db::DbError;
use crate::model::settings::AppSettings;
use rusqlite::Connection;

/// Репозиторий `settings`.
pub struct SettingsRepo<'a> {
    conn: &'a Connection,
}

impl<'a> SettingsRepo<'a> {
    /// Репозиторий поверх подключения.
    pub fn new(conn: &'a Connection) -> Self {
        SettingsRepo { conn }
    }

    /// Загрузить полный набор настроек; отсутствующие ключи → дефолты
    /// (первый запуск: таблица пуста — это нормальный путь).
    pub fn load(&self) -> Result<AppSettings, DbError> {
        let mut raw: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings")
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        for r in rows {
            let (k, v) = r.map_err(|e| DbError::Sqlite(e.to_string()))?;
            raw.insert(k, v);
        }
        Ok(Self::parse(raw))
    }

    /// Сохранить набор настроек (ключи SPEC §5) одним UPSERT-батчем.
    pub fn save(&self, s: &AppSettings) -> Result<(), DbError> {
        use crate::model::settings::keys as k;
        let now = crate::model::session::now_iso();
        // Сериализация примитивов serde_json не падает; тем не менее по
        // правилу №1.5 — без unwrap: ошибка сериализации → DbError::Parse.
        fn json(v: &impl serde::Serialize) -> Result<String, DbError> {
            serde_json::to_string(v).map_err(|e| DbError::Parse(e.to_string()))
        }
        let pairs: [(&str, String); 8] = [
            (k::PROBE_ENABLED, json(&s.probe.enabled)?),
            (
                k::PROBE_WINDOW_START,
                format!(
                    "{:02}:{:02}",
                    s.probe.window.start_h, s.probe.window.start_m
                ),
            ),
            (
                k::PROBE_WINDOW_END,
                format!("{:02}:{:02}", s.probe.window.end_h, s.probe.window.end_m),
            ),
            (k::PROBE_COUNT, s.probe.count.to_string()),
            (k::PROBE_MIN_GAP, s.probe.min_gap_minutes.to_string()),
            (
                k::SESSION_SIGNAL_INTERVAL,
                s.session.signal_interval_s.to_string(),
            ),
            (k::CRYPTO_MODE, json(&s.crypto.mode)?),
            (k::I18N_LANG, json(&s.language)?),
        ];
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1,?2,?3)
                     ON CONFLICT(key) DO UPDATE SET value=?2, updated_at=?3",
                )
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            for (key, value) in pairs {
                stmt.execute(rusqlite::params![key, value, now])
                    .map_err(|e| DbError::Sqlite(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| DbError::Sqlite(e.to_string()))?;
        Ok(())
    }

    /// Разбор карты «ключ → JSON» в типизированные настройки.
    fn parse(raw: std::collections::HashMap<String, String>) -> AppSettings {
        use crate::model::settings::keys as k;
        let mut s = AppSettings::default();
        let get = |key: &str| -> Option<&String> { raw.get(key) };

        if let Some(v) = get(k::PROBE_ENABLED) {
            if let Ok(b) = serde_json::from_str::<bool>(v) {
                s.probe.enabled = b;
            }
        }
        if let Some(v) = get(k::PROBE_WINDOW_START) {
            if let Some((h, m)) = parse_hhmm(v) {
                s.probe.window.start_h = h;
                s.probe.window.start_m = m;
            }
        }
        if let Some(v) = get(k::PROBE_WINDOW_END) {
            if let Some((h, m)) = parse_hhmm(v) {
                s.probe.window.end_h = h;
                s.probe.window.end_m = m;
            }
        }
        if let Some(v) = get(k::PROBE_COUNT) {
            if let Ok(n) = v.parse::<u8>() {
                if (1..=12).contains(&n) {
                    s.probe.count = n;
                }
            }
        }
        if let Some(v) = get(k::PROBE_MIN_GAP) {
            if let Ok(n) = v.parse::<u16>() {
                s.probe.min_gap_minutes = n;
            }
        }
        if let Some(v) = get(k::SESSION_SIGNAL_INTERVAL) {
            if let Ok(n) = v.parse::<u32>() {
                if n > 0 {
                    s.session.signal_interval_s = n;
                }
            }
        }
        if let Some(v) = get(k::CRYPTO_MODE) {
            if let Ok(mode) = serde_json::from_str(v) {
                s.crypto.mode = mode;
            }
        }
        if let Some(v) = get(k::I18N_LANG) {
            if let Ok(lang) = serde_json::from_str(v) {
                s.language = lang;
            }
        }
        s
    }
}

/// Разбор `HH:MM` (окно зондов).
fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.split_once(':')?;
    let h: u8 = h.parse().ok()?;
    let m: u8 = m.parse().ok()?;
    if h < 24 && m < 60 { Some((h, m)) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn settings_roundtrip_defaults_and_custom() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let repo = SettingsRepo::new(&conn);

        // Первый запуск: пусто → дефолты
        assert_eq!(repo.load().unwrap(), AppSettings::default());

        // Сохраняем изменённые
        let mut s = AppSettings::default();
        s.probe.enabled = true;
        s.probe.count = 5;
        s.probe.min_gap_minutes = 120;
        s.probe.window.start_h = 10;
        s.session.signal_interval_s = 600;
        s.language = crate::model::settings::Language::En;
        repo.save(&s).unwrap();

        // Загружаем: значения совпали
        let back = repo.load().unwrap();
        assert_eq!(back, s);
        assert_eq!(back.probe.count, 5);
        assert_eq!(back.language.as_code(), "en");
    }

    #[test]
    fn garbage_values_fall_back_to_defaults() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES
             ('probe.count', 'banana', '2026-11-16T00:00:00.000Z'),
             ('probe.window_start', '99:99', '2026-11-16T00:00:00.000Z')",
            [],
        )
        .unwrap();
        let repo = SettingsRepo::new(&conn);
        let s = repo.load().unwrap();
        assert_eq!(s.probe.count, AppSettings::default().probe.count);
        assert_eq!(s.probe.window.start_h, 9, "дефолт 09:00 не затёрт мусором");
    }

    #[test]
    fn hhmm_parser() {
        assert_eq!(parse_hhmm("09:30"), Some((9, 30)));
        assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
        assert_eq!(
            parse_hhmm("9:5"),
            Some((9, 5)),
            "однозначные часы — валидны"
        );
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("noise"), None);
        assert_eq!(parse_hhmm("12-30"), None);
    }
}
