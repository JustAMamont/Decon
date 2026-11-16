//! Экран аналитики (S4): тренды, корреляции, A/B-периоды, экспорт.
//!
//! Тонкий слой (правило №7.4): расчёты — `decon_core::analysis`,
//! здесь только сбор данных (SQL через стейт) и отображение.
//! Тренды — текстовая sparkline-строка (без web-компонентов), графики
//! SVG — следующий шаг после пилота.

use crate::i18n::{I18n, Str};
use crate::state::app::App;
use crate::views::AppState;
use decon_core::analysis::correlation::{CorrKind, RankBuf, correlation_missing};
use decon_core::analysis::delta::{Delta, delta_summary, session_delta};
use decon_core::analysis::export::{export_csv, export_json, to_json_string};
use decon_core::analysis::n_of_1::DEFAULT_PERMUTATIONS;
use decon_core::analysis::trends::{sma, split_by_version};
use dioxus::prelude::*;

/// Один «предрасчёт» аналитики: собирается одним проходом по БД при
/// входе на экран (кнопка «Обновить»), не на каждый рендер.
#[derive(Clone)]
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

/// Собрать снимок данных аналитики из БД (один проход SQL).
pub fn collect_snapshot(app: &App) -> AnalyticsSnapshot {
    let conn = &app.conn;
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
        .expect("sql: series");
    let mut quality_series = Vec::new();
    let mut sleep_vs_quality: (Vec<Option<f64>>, Vec<Option<f64>>) = (Vec::new(), Vec::new());
    let mut caffeine_vs_quality: (Vec<Option<f64>>, Vec<Option<f64>>) = (Vec::new(), Vec::new());
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .expect("sql: rows");
    for row in rows {
        let (value, sleep, caffeine) = row.expect("row");
        quality_series.push(value);
        sleep_vs_quality.0.push(sleep);
        sleep_vs_quality.1.push(Some(value));
        caffeine_vs_quality.0.push(caffeine.map(|c| c as f64));
        caffeine_vs_quality.1.push(Some(value));
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
        .expect("sql: deltas");
    let mut delta = Vec::new();
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, Option<f64>>(1)?, row.get::<_, Option<f64>>(2)?))
        })
        .expect("sql: delta rows");
    for row in rows {
        let (pre, post) = row.expect("row");
        if let Some(d) = session_delta(pre, post) {
            delta.push(d);
        }
    }

    // SMA по тренду
    let series_points: Vec<decon_core::db::repo::scale_repo::SeriesPoint> = conn
        .prepare(
            "SELECT r.answered_at, r.value, r.delay_minutes, sv.version
             FROM responses r
             JOIN scale_versions sv ON sv.id = r.scale_version_id
             JOIN scales sc ON sc.id = sv.scale_id
             WHERE sc.id = 'post.uniformity' AND r.deleted_at IS NULL
             ORDER BY r.answered_at",
        )
        .expect("sql: trend")
        .query_map([], |row| {
            Ok(decon_core::db::repo::scale_repo::SeriesPoint {
                answered_at: row.get(0)?,
                value: row.get(1)?,
                delay_minutes: row.get(2)?,
                version: row.get(3)?,
            })
        })
        .expect("sql: trend rows")
        .collect::<Result<_, _>>()
        .expect("trend collect");
    let segments = split_by_version(&series_points);
    let mut uniformity_sma = Vec::new();
    // SMA по последнему сегменту (текущая формулировка)
    if let Some(last) = segments.last() {
        let mut out = Vec::new();
        sma(&last.points, 7, &mut out);
        uniformity_sma = out.into_iter().map(|t| t.value).collect();
    }

    AnalyticsSnapshot {
        uniformity_sma,
        uniformity_n: series_points.len(),
        delta,
        sleep_vs_quality,
        caffeine_vs_quality,
        quality_series,
    }
}

/// Текстовая sparkline: Unicode-блоки ▁▂▃▄▅▆▇█ по бинам 0..10.
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
pub fn analytics_screen() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let lang = state.read().app.lang();
    let i18n = I18n::new(lang);

    // Снимок пересчитывается по кнопке (не на каждый рендер — правило
    // тонкого UI). Первый вход — сразу считаем.
    let mut snapshot = use_signal(|| Option::<AnalyticsSnapshot>::None);
    if snapshot.read().is_none() {
        let s = state.read();
        let snap = collect_snapshot(&s.app);
        snapshot.set(Some(snap));
    }

    let Some(snap) = snapshot.read().clone() else {
        return rsx! { div {} };
    };

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

    // Sparkline считается до rsx (в rsx нельзя объявлять let)
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
                        #[allow(clippy::type_complexity)]
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
                    snapshot.set(None); // пересбор на следующем рендере
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
    rsx! {
        p { style: "margin: 4px 0;",
            {label}
            ": ρ = "
            { if corr.rho.is_nan() { "—".to_string() } else { format!("{:+.2}", corr.rho) } }
            " ({strength}) · "
            {i18n.get(Str::CorrPairs).replace("{n}", &corr.n.to_string())}
        }
    }
}
