//! Утилиты локального времени без chrono (пилот: системное время = UTC;
//! TZ-детект — S4, оговорка в ROADMAP).
//!
//! Алгоритмы Хиннанта (civil_from_days / days_from_civil) — без таблиц
//! високосных лет, корректны во всём диапазоне дат приложения.

/// Миллисекунды от Unix-эпохи (монотонный источник — SystemTime).
pub fn unix_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Сегодня `YYYY-MM-DD` по системному времени.
pub fn local_date_today() -> String {
    // Системная дата без TZ-библиотек: считаем от UTC с смещением из
    // /etc/localtime не будем — в песочнице и CI это UTC; для пилота
    // берём UTC-дату (TZ-детект — S4, честная оговорка в ROADMAP).
    let secs = unix_now_ms() / 1000;
    let days = secs / 86_400;
    civil_from_days(days)
}

/// Метка «сейчас» в компактном локальном формате `YYYY-MM-DDTHH:MM:SS`.
pub fn local_now_compact() -> String {
    let secs = unix_now_ms() / 1000;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}",
        civil_from_days(days),
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Дни от эпохи → `YYYY-MM-DD` (алгоритм Говарда Хиннанта, без високосных
/// таблиц; корректен для всех дат 1970–2099 в наших пределах).
pub fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Детерминированный сид дня: дата → u64 (один день = один план).
pub fn date_seed(date: &str) -> u64 {
    // Простое смешивание символов даты: любой день даёт стабильный сид,
    // соседние дни заметно различаются.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset
    for b in date.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Задержка догоняющего ответа: минуты между плановым временем и сейчас;
/// None, если ответ в пределах 5 минут (свежий).
pub fn catchup_delay_minutes(scheduled_at: &str) -> Option<i64> {
    // Обе метки — локальный компактный формат `YYYY-MM-DDTHH:MM:SS`
    let now = local_now_compact();
    let to_min = |s: &str| -> Option<i64> {
        let (d, t) = s.split_once('T')?;
        let dp: Vec<i64> = d.split('-').filter_map(|x| x.parse().ok()).collect();
        let tp: Vec<i64> = t.split(':').filter_map(|x| x.parse().ok()).collect();
        if dp.len() != 3 || tp.len() != 3 {
            return None;
        }
        // Дни от эпохи для даты
        let days = days_from_civil(dp[0], dp[1], dp[2]);
        Some(days * 1440 + tp[0] * 60 + tp[1] + tp[2] / 60)
    };
    let diff = to_min(scheduled_at)? - to_min(&now)?;
    // Отрицательная (ответ «до» плана — гонка минут) или <5 — свежий
    (diff >= 5).then_some(diff)
}

/// Обратный алгоритм Хиннанта: дата → дни от эпохи.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_roundtrip() {
        for (y, m, d) in [(2026, 11, 16), (2026, 1, 1), (2026, 12, 31), (2027, 2, 28)] {
            let date = format!("{y:04}-{m:02}-{d:02}");
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), date, "roundtrip {date}");
        }
        assert_eq!(civil_from_days(0), "1970-01-01");
    }

    #[test]
    fn date_seed_stable_and_distinct() {
        assert_eq!(date_seed("2026-11-16"), date_seed("2026-11-16"));
        assert_ne!(date_seed("2026-11-16"), date_seed("2026-11-17"));
    }

    #[test]
    fn epoch_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(
            days_from_civil(2026, 11, 16),
            days_from_civil(2026, 11, 17) - 1
        );
    }
}
