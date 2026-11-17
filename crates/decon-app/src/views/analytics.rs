//! Экран аналитики (S4): тренды, корреляции, A/B-периоды, экспорт.
//!
//! Тонкий слой (правило №7.4): расчёты — `decon_core::analysis`,
//! здесь только сбор данных (SQL) и отображение. Паттерн как у
//! истории: снимок собирается при переключении экрана
//! (`App::reload_analytics` из навигации) и по кнопке «Обновить»;
//! экран только читает `app.analytics`. Паник нет — ошибка сборки
//! деградирует в пустые состояния (правило №1.5).
//!
//! Тренды — текстовая sparkline (блоки Unicode); полноценная отрисовка
//! графиков — после пилота.

use crate::i18n::{I18n, Str};
use crate::views::AppState;
use decon_core::analysis::correlation::{CorrKind, RankBuf, correlation_missing};
use decon_core::analysis::delta::{Delta, delta_summary, session_delta};
use decon_core::analysis::export::{export_csv, export_json, to_json_string};
use decon_core::analysis::n_of_1::DEFAULT_PERMUTATIONS;
use decon_core::analysis::trends::{sma, split_by_version};
use decon_core::db::repo::scale_repo::SeriesPoint;
use dioxus::prelude::*;
use rusqlite::Connection;

/// Один «предрасчёт» аналитики: собирается одним проходом по БД при
/// входе на экран, не на каждый рендер.
#[derive(Clone, Default)]
pub struct AnalyticsSnapshot {
    /// Тренд post.uniformity: SMA-7 значений (для sparkline).
    pub uniformity_sma: Vec<f64>,
    /// Число точек в тренде.
    pub uniformity_n: usize,
    /// Дельта post − pre по сессиям с парой замеров.
    pub delta: Vec<Delta>,
    /// Сон (ч) и post.uniformity по сессиям (пары без пропусков).
    pub sleep_vs_quality: (Vec<Option<f64>>, Vec<Option<f64>>),
    /// Кофеин и post.uniformity.
    pub caffeine_vs_quality: (Vec<Option<f64>>, Vec<Option<f64>>),
    /// Значения post.uniformity по хронологии (A/B режется пополам ряда).
    pub quality_series: Vec<f64>,
}

/// Собрать снимок данных аналитики из БД. Ошибки SQL возвращаются
/// вызывающему (`App::reload_analytics`), пустая база — валидный
/// пустой снимок, а не ошибка.
pub fn collect_snapshot(conn: &Connection) -> Result<AnalyticsSnapshot, String> {
    let mut snap = AnalyticsSnapshot::default();

    // Ряд post.uniformity с контекстом сессий
    let mut stmt = conn
        .prepare(
            "SELECT r.value, s.sleep_hours, s.caffeine
             FROM responses r
             JOIN scale_versions sv ON sv.id = r.scale_version_id
             JOIN scales sc ON sc.id = sv.scale_id
             LEFT JOIN sessions s ON s.id = r.session_id
             WHERE sc.id = 'post.uniformity' AND r.deleted_at IS NULL
             ORDER BY r.answered_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (value, sleep, caffeine) = row.map_err(|e| e.to_string())?;
        snap.quality_series.push(value);
        snap.sleep_vs_quality.0.push(sleep);
        snap.sleep_vs_quality.1.push(Some(value));
        snap.caffeine_vs_quality.0.push(caffeine.map(|c| c as f64));
        snap.caffeine_vs_quality.1.push(Some(value));
    }

    // Pre/post пары по сессиям (pre.noise → post.uniformity)
    let mut stmt = conn
        .prepare(
            "SELECT s.id,
               MAX(CASE WHEN sc.id = 'pre.noise' THEN r.value END) AS pre_v,
               MAX(CASE WHEN sc.id = 'post.uniformity' THEN r.value END) AS post_v
             FROM responses r
             JOIN scale_versions sv ON sv.id = r.scale_version_id
             JOIN scales sc ON sc.id = sv.scale_id
             JOIN sessions s ON s.id = r.session_id
             WHERE sc.id IN ('pre.noise','post.uniformity') AND r.deleted_at IS NULL
             GROUP BY s.id
             ORDER BY s.started_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, Option<f64>>(1)?, row.get::<_, Option<f64>>(2)?))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (pre, post) = row.map_err(|e| e.to_string())?;
        if let Some(d) = session_delta(pre, post) {
            snap.delta.push(d);
        }
    }

    // Ряд для SMA (с версиями формулировок — сегментация)
    let series: Vec<SeriesPoint> = {
        let mut stmt = conn
            .prepare(
                "SELECT r.answered_at, r.value, r.delay_minutes, sv.version
                 FROM responses r
                 JOIN scale_versions sv ON sv.id = r.scale_version_id
                 JOIN scales sc ON sc.id = sv.scale_id
                 WHERE sc.id = 'post.uniformity' AND r.deleted_at IS NULL
                 ORDER BY r.answered_at",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SeriesPoint {
                    answered_at: row.get(0)?,
                    value: row.get(1)?,
                    delay_minutes: row.get(2)?,
                    version: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    snap.uniformity_n = series.len();
    // SMA по последнему сегменту (текущая формулировка)
    if let Some(last) = split_by_version(&series).last() {
        let mut out = Vec::new();
        sma(&last.points, 7, &mut out);
        snap.uniformity_sma = out.into_iter().map(|t| t.value).collect();
    }

    Ok(snap)
}

/// Текстовая sparkline: Unicode-блоки ▁▂▃▄▅▆▇█ по бинам 0..10.
/// Чистая функция — тестируется без UI.
pub fn sparkline(values: &[f64], out: &mut String) {
    out.clear();
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let step = (values.len() / 40).max(1);
    for chunk in values.chunks(step) {
        let v = chunk.iter().sum::<f64>() / chunk.len() as f64;
        let idx = ((v / 10.0) * 7.0).round().clamp(0.0, 7.0) as usize;
        out.push(BLOCKS[idx]);
    }
}

/// Экран аналитики: тренды, корреляции, A/B, экспорт.
///
/// Данные читаются из `app.analytics` (снимок собирает навигация при
/// входе на экран и кнопка «Обновить») — экран сам в БД не ходит.
pub fn analytics_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    // Снимок: None при первом входе до сборки или после ошибки сборки —
    // показываем пустые состояния, не паникуем.
    let snap = state.read().app.analytics.clone().unwrap_or_default();

    // --- Корреляции (Спирмен)
    let mut buf = RankBuf::new();
    let corr_sleep = correlation_missing(
        &snap.sleep_vs_quality.0,
        &snap.sleep_vs_quality.1,
        &mut buf,
        CorrKind::Spearman,
    );
    let corr_caffeine = correlation_missing(
        &snap.caffeine_vs_quality.0,
        &snap.caffeine_vs_quality.1,
        &mut buf,
        CorrKind::Spearman,
    );

    // --- A/B: разбивка хронологии пополам (первая половина A, вторая B)
    let ab = decon_core::analysis::n_of_1::compare_periods(
        &snap.quality_series[..snap.quality_series.len() / 2],
        &snap.quality_series[snap.quality_series.len() / 2..],
        42,
        DEFAULT_PERMUTATIONS,
    );

    // --- Дельты
    let dsum = delta_summary(&snap.delta);

    // Sparkline и форматирование — до rsx (внутри только элементы)
    let mut spark = String::new();
    sparkline(&snap.uniformity_sma, &mut spark);

    rsx! {
        h2 { {i18n.get(Str::Analytics)} }

        // --- Тренды
        h3 { {i18n.get(Str::TrendsTitle)} }
        if snap.uniformity_n == 0 {
            p { style: "color:#7d8590;", {i18n.get(Str::HistoryEmpty)} }
        } else {
            div {
                style: "font-family: ui-monospace, monospace; font-size: 20px; letter-spacing: 1px; color: #9ec49e; background:#1a1d21; padding: 10px 12px; border-radius: 8px; display: inline-block;",
                {spark}
            }
            p { style: "color:#7d8590; font-size: 13px;",
                "post.uniformity · n = {snap.uniformity_n} · SMA-7"
            }
        }

        // --- Корреляции
        h3 { style: "margin-top: 18px;", {i18n.get(Str::CorrelationsTitle)} }
        corr_row { lang, label: i18n.get(Str::CorrSleep), corr: corr_sleep }
        corr_row { lang, label: i18n.get(Str::CorrCaffeine), corr: corr_caffeine }

        // --- Дельты
        h3 { style: "margin-top: 18px;", {i18n.get(Str::PostTitle)} }
        if dsum.n > 0 {
            p {
                {i18n.get(Str::DeltaMean).replace("{d}", &format!("{:+.1}", dsum.mean_change))}
                " · "
                {i18n.get(Str::DeltaN).replace("{n}", &dsum.n.to_string())}
            }
        } else {
            p { style: "color:#7d8590;", {i18n.get(Str::HistoryEmpty)} }
        }

        // --- A/B периоды
        h3 { style: "margin-top: 18px;", {i18n.get(Str::AbTitle)} }
        if let Some(r) = ab {
            if r.low_sample {
                // Критерий 4: предупреждение «мало данных» при n < 30
                div {
                    style: "background:#4a3a20; border:1px solid #6b542e; color:#e8c97a; padding:10px 12px; border-radius:8px; margin: 6px 0;",
                    {i18n.get(Str::AbLowSample)}
                }
            }
            p {
                {i18n.get(Str::AbMeans)
                    .replace("{ma}", &format!("{:.1}", r.mean_a))
                    .replace("{na}", &r.n_a.to_string())
                    .replace("{mb}", &format!("{:.1}", r.mean_b))
                    .replace("{nb}", &r.n_b.to_string())}
            }
            p { {i18n.get(Str::AbPValue).replace("{p}", &format!("{:.3}", r.p_value))} }
        } else {
            p { style: "color:#7d8590;", {i18n.get(Str::HistoryEmpty)} }
        }

        // --- Экспорт
        h3 { style: "margin-top: 18px;", {i18n.get(Str::ExportTitle)} }
        div { class: "row",
            button {
                onclick: move |_| {
                    // Результат считаем в отдельном скоупе, read-лок
                    // не переживает write (borrow checker)
                    let outcome: Result<String, String> = {
                        let s = state.read();
                        export_json(&s.app.conn)
                            .and_then(|t| to_json_string(&t))
                            .map_err(|e| e.to_string())
                    };
                    match outcome {
                        Ok(json) => {
                            let dir = std::env::temp_dir().join("decon-export");
                            let _ = std::fs::create_dir_all(&dir);
                            let path = dir.join("decon-export.json");
                            let msg = match std::fs::write(&path, json) {
                                Ok(()) => format!(
                                    "{}: {}",
                                    I18n::new(lang).get(Str::Exported),
                                    path.display()
                                ),
                                Err(e) => e.to_string(),
                            };
                            state.write().app.toast = Some(msg);
                        }
                        Err(e) => {
                            state.write().error = Some(e);
                        }
                    }
                },
                {i18n.get(Str::ExportJson)}
            }
            button {
                onclick: move |_| {
                    let outcome: Result<Vec<(String, String)>, String> = {
                        let s = state.read();
                        export_csv(&s.app.conn, false)
                            .map(|files| {
                                files
                                    .into_iter()
                                    .map(|(n, c)| (n.to_string(), c))
                                    .collect::<Vec<_>>()
                            })
                            .map_err(|e| e.to_string())
                    };
                    match outcome {
                        Ok(files) => {
                            let dir = std::env::temp_dir().join("decon-export");
                            let _ = std::fs::create_dir_all(&dir);
                            let mut last = String::new();
                            for (name, csv) in &files {
                                let path = dir.join(format!("{name}.csv"));
                                if std::fs::write(&path, csv).is_ok() {
                                    last = path.display().to_string();
                                }
                            }
                            let msg = format!(
                                "{}: {}",
                                I18n::new(lang).get(Str::Exported),
                                last
                            );
                            state.write().app.toast = Some(msg);
                        }
                        Err(e) => {
                            state.write().error = Some(e);
                        }
                    }
                },
                {i18n.get(Str::ExportCsv)}
            }
            button {
                onclick: move |_| {
                    // Пересбор снимка по кнопке (ошибка — в тост, не паника)
                    let outcome = {
                        let mut s = state.write();
                        s.app.reload_analytics()
                    };
                    if let Err(e) = outcome {
                        state.write().error = Some(e);
                    }
                },
                {i18n.get(Str::TrainerDone)}
            }
        }
    }
}

/// Строка корреляции: метка + ρ + сила связи + число пар.
#[component]
fn corr_row(
    lang: decon_core::model::settings::Language,
    label: &'static str,
    corr: decon_core::analysis::correlation::Correlation,
) -> Element {
    let i18n = I18n::new(lang);
    let strength = match corr.strength_label() {
        "insufficient" => i18n.get(Str::CorrInsufficient),
        "weak" => i18n.get(Str::CorrStrengthWeak),
        "moderate" => i18n.get(Str::CorrStrengthModerate),
        "strong" => i18n.get(Str::CorrStrengthStrong),
        _ => i18n.get(Str::CorrStrengthNone),
    };
    let rho = if corr.rho.is_nan() {
        "—".to_string()
    } else {
        format!("{:+.2}", corr.rho)
    };
    let pairs = i18n.get(Str::CorrPairs).replace("{n}", &corr.n.to_string());
    rsx! {
        p { style: "margin: 4px 0;",
            {label}
            ": ρ = "
            {rho}
            " ({strength}) · "
            {pairs}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use decon_core::db::{self, open_in_memory};

    fn seeded() -> Connection {
        let conn = open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::repo::ScaleRepo::new(&conn).seed_defaults().unwrap();
        conn
    }

    /// Регрессия скриншота 2026-11-17: экран аналитики паниковал на
    /// пустой базе (`expect` в SQL-обвязке). Снимок обязан собираться
    /// без паник на пустой и наполненной базе.
    #[test]
    fn snapshot_empty_db_no_panic() {
        let conn = seeded();
        let snap = collect_snapshot(&conn).unwrap();
        assert_eq!(snap.uniformity_n, 0);
        assert!(snap.quality_series.is_empty());
        assert!(snap.delta.is_empty());
        assert!(snap.uniformity_sma.is_empty());
        // Пустые ряды → корреляции NaN, экран показывает «—»
        let mut buf = RankBuf::new();
        let c = correlation_missing(
            &snap.sleep_vs_quality.0,
            &snap.sleep_vs_quality.1,
            &mut buf,
            CorrKind::Spearman,
        );
        assert_eq!(c.n, 0);
        assert!(c.rho.is_nan());
        // sparkline на пустом ряду — пустая строка, не паника
        let mut out = String::new();
        sparkline(&snap.uniformity_sma, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn snapshot_populated_db() {
        let conn = seeded();
        // Сессия с pre/post парой
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at)
             VALUES ('s1','practice','2026-11-01T10:00:00Z','2026-11-01T10:00:00Z')",
            [],
        )
        .unwrap();
        let (sv_pre, sv_post): (String, String) = conn
            .query_row(
                "SELECT
                   (SELECT sv.id FROM scale_versions sv JOIN scales s ON s.id = sv.scale_id
                    WHERE s.id = 'pre.noise' LIMIT 1),
                 (SELECT sv.id FROM scale_versions sv JOIN scales s ON s.id = sv.scale_id
                    WHERE s.id = 'post.uniformity' LIMIT 1)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        for (id, sv, val) in [("r1", &sv_pre, 4.0), ("r2", &sv_post, 8.0)] {
            conn.execute(
                "INSERT INTO responses (id, session_id, scale_version_id, value, answered_at, updated_at)
                 VALUES (?1,'s1',?2,?3,'2026-11-01T10:20:00Z','2026-11-01T10:20:00Z')",
                rusqlite::params![id, sv, val],
            )
            .unwrap();
        }
        let snap = collect_snapshot(&conn).unwrap();
        assert_eq!(snap.uniformity_n, 1);
        assert_eq!(snap.quality_series, vec![8.0]);
        assert_eq!(snap.delta.len(), 1);
        assert_eq!(snap.delta[0].change, 4.0);
        assert_eq!(snap.uniformity_sma, vec![8.0]); // одно окно — само значение
    }

    #[test]
    fn sparkline_bins() {
        let mut out = String::new();
        sparkline(&[0.0], &mut out);
        assert_eq!(out, "▁");
        sparkline(&[10.0], &mut out);
        assert_eq!(out, "█");
        sparkline(&[5.0], &mut out);
        assert_eq!(out, "▅"); // 5/10*7=3.5 → round 4 → ▅
    }
}
