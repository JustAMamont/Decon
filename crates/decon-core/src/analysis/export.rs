//! Экспорт и импорт данных пользователя (S4, SPEC §9).
//!
//! **CSV** — денормализованные таблицы (sessions, responses,
//! trainer_results, probes) по одной на файл; 🔒-поля (заметки) —
//! только с явным флагом, иначе `"[encrypted]"`.
//!
//! **JSON** — полное дерево пользователя без 🔒-полей, формат
//! `"decon/v1"`. Импорт не перезаписывает: новые ID, дедупликация по
//! `(source_updated_at, content_hash)` — повторный импорт того же
//! файла не плодит копии (roundtrip-тест).
//!
//! Content_hash — FNV-1a 64 по нормализованному содержимому строки
//! (без ID и updated_at: они и так меняются при импорте). Хэш не
//! криптографический — задача отличить «то же содержимое», не
//! защититься от подделки.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write as _;

use crate::db::DbError;

/// Метка формата JSON-экспорта.
pub const EXPORT_FORMAT: &str = "decon/v1";

/// Плейсхолдер зашифрованного поля в экспорте без заметок.
pub const ENCRYPTED_PLACEHOLDER: &str = "[encrypted]";

/// Ошибка экспорта/импорта.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// Ошибка БД при чтении/записи.
    #[error("db: {0}")]
    Db(#[from] DbError),
    /// Повреждённый или незнакомый формат импорта.
    #[error("bad format: {0}")]
    BadFormat(String),
    /// Ошибка сериализации.
    #[error("serialization: {0}")]
    Serde(String),
}

/// Строка сессии в JSON-дереве (без 🔒 note_enc — по умолчанию
/// заметки не покидают устройство; SPEC §9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionExport {
    /// Оригинальный ID (для сопоставления ссылок внутри файла).
    pub id: String,
    /// Тип: practice / trainer.
    pub kind: String,
    /// Слой.
    pub layer: Option<String>,
    /// Техника.
    pub technique: Option<String>,
    /// Начало (ISO-8601).
    pub started_at: String,
    /// Окончание.
    pub ended_at: Option<String>,
    /// Длительность, с.
    pub duration_s: Option<i64>,
    /// Чем закончилась.
    pub end_status: Option<String>,
    /// Корзина времени дня.
    pub local_time_bucket: Option<String>,
    /// Часов сна.
    pub sleep_hours: Option<f64>,
    /// Доз кофеина.
    pub caffeine: Option<i64>,
    /// Поза.
    pub posture: Option<String>,
    /// updated_at источника (для дедупликации).
    pub updated_at: String,
    /// Хэш содержимого (считает экспортёр).
    pub content_hash: u64,
}

/// Строка ответа в JSON-дереве.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseExport {
    /// Оригинальный ID.
    pub id: String,
    /// Сессия (если pre/post/carryover).
    pub session_id: Option<String>,
    /// Зонд (если ESM).
    pub probe_id: Option<String>,
    /// Версия шкалы.
    pub scale_version_id: String,
    /// Значение.
    pub value: f64,
    /// Момент ответа.
    pub answered_at: String,
    /// Задержка, мин.
    pub delay_minutes: Option<i64>,
}

/// Строка результата тренажёра в JSON-дереве.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainerResultExport {
    /// Оригинальный ID.
    pub id: String,
    /// Сессия.
    pub session_id: String,
    /// Тип тренажёра.
    pub trainer: String,
    /// Конфигурация (JSON-строка).
    pub config_json: String,
    /// Метрики (JSON-строка).
    pub metrics_json: String,
    /// Сид.
    pub seed: String,
}

/// Строка зонда в JSON-дереве.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeExport {
    /// Оригинальный ID.
    pub id: String,
    /// Плановое время.
    pub scheduled_at: String,
    /// Показ уведомления.
    pub fired_at: Option<String>,
    /// Ответ.
    pub answered_at: Option<String>,
    /// Задержка, мин.
    pub delay_minutes: Option<i64>,
    /// Последний повтор.
    pub last_reminder_at: Option<String>,
    /// Состояние окна.
    pub window_state: String,
    /// Хэш содержимого: зонды дедуплицируются при повторном импорте
    /// (scheduled_at + состояние), иначе копились бы с ответами.
    pub content_hash: u64,
}

/// Шкала справочника (SCALES.md): без формулировок — только метаданные.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScaleExport {
    /// ID шкалы ('post.uniformity').
    pub id: String,
    /// Заголовок.
    pub title: String,
    /// Якорь «низ».
    pub anchor_low: String,
    /// Якорь «высокий».
    pub anchor_high: String,
    /// Тип: slider / flag / factor.
    pub kind: String,
    /// Активна ли.
    pub active: bool,
}

/// Версия формулировки шкалы: без ответов на неё интерпретировать
/// экспортированные values нельзя (SCALES.md §8.4), потому версии
/// едут в дереве — импорт воссоздаёт недостающие.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScaleVersionExport {
    /// ID версии (совпадает с оригинальным: формулировки неизменяемы,
    /// переезд «как есть» не ломает сопоставимость рядов).
    pub id: String,
    /// Шкала.
    pub scale_id: String,
    /// Номер версии.
    pub version: i64,
    /// Текст вопроса.
    pub question: String,
    /// Якоря (JSON).
    pub anchors_json: String,
}

/// Полное дерево JSON-экспорта, формат `decon/v1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportTree {
    /// Метка формата: всегда `"decon/v1"`.
    pub export_format: String,
    /// Момент экспорта (ISO-8601).
    pub exported_at: String,
    /// Справочник шкал (метаданные, без формулировок ответов).
    pub scales: Vec<ScaleExport>,
    /// Версии формулировок шкал (нужны для интерпретации ответов).
    pub scale_versions: Vec<ScaleVersionExport>,
    /// Сессии.
    pub sessions: Vec<SessionExport>,
    /// Ответы.
    pub responses: Vec<ResponseExport>,
    /// Результаты тренажёров.
    pub trainer_results: Vec<TrainerResultExport>,
    /// Зонды.
    pub probes: Vec<ProbeExport>,
}

/// FNV-1a 64 — быстрый нечувствительный к ID хэш содержимого.
fn fnv1a(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Разделитель полей: ("ab","c") ≠ ("a","bc")
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Собрать JSON-дерево экспорта из БД (без заметок).
pub fn export_json(conn: &Connection) -> Result<ExportTree, ExportError> {
    // Справочник шкал (метаданные) + версии формулировок
    let mut stmt = conn
        .prepare(
            "SELECT id, title, anchor_low, anchor_high, kind, active
             FROM scales WHERE deleted_at IS NULL ORDER BY id",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let scales: Vec<ScaleExport> = stmt
        .query_map([], |r| {
            Ok(ScaleExport {
                id: r.get(0)?,
                title: r.get(1)?,
                anchor_low: r.get(2)?,
                anchor_high: r.get(3)?,
                kind: r.get(4)?,
                active: r.get::<_, i64>(5)? != 0,
            })
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, scale_id, version, question, anchors_json
             FROM scale_versions ORDER BY scale_id, version",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let scale_versions: Vec<ScaleVersionExport> = stmt
        .query_map([], |r| {
            Ok(ScaleVersionExport {
                id: r.get(0)?,
                scale_id: r.get(1)?,
                version: r.get(2)?,
                question: r.get(3)?,
                anchors_json: r.get(4)?,
            })
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    // Сессии
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, CAST(COALESCE(layer,'') AS TEXT), CAST(COALESCE(technique,'') AS TEXT), started_at,
             CAST(COALESCE(ended_at,'') AS TEXT), duration_s, CAST(COALESCE(end_status,'') AS TEXT),
             CAST(COALESCE(local_time_bucket,'') AS TEXT), sleep_hours, caffeine,
             CAST(COALESCE(posture,'') AS TEXT), updated_at
             FROM sessions WHERE deleted_at IS NULL ORDER BY started_at",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let sessions: Vec<SessionExport> = stmt
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let kind: String = r.get(1)?;
            let layer: String = r.get(2)?;
            let technique: String = r.get(3)?;
            let started_at: String = r.get(4)?;
            let ended_at: String = r.get(5)?;
            let duration_s: Option<i64> = r.get(6)?;
            let end_status: String = r.get(7)?;
            let bucket: String = r.get(8)?;
            let sleep: Option<f64> = r.get(9)?;
            let caffeine: Option<i64> = r.get(10)?;
            let posture: String = r.get(11)?;
            let updated_at: String = r.get(12)?;
            let content_hash = fnv1a(&[
                &kind,
                &layer,
                &technique,
                &started_at,
                &ended_at,
                &duration_s.unwrap_or(0).to_string(),
                &end_status,
                &bucket,
                &sleep.map(|v| v.to_string()).unwrap_or_default(),
                &caffeine.unwrap_or(0).to_string(),
                &posture,
            ]);
            Ok(SessionExport {
                id,
                kind,
                layer: if layer.is_empty() { None } else { Some(layer) },
                technique: if technique.is_empty() {
                    None
                } else {
                    Some(technique)
                },
                started_at,
                ended_at: if ended_at.is_empty() {
                    None
                } else {
                    Some(ended_at)
                },
                duration_s,
                end_status: if end_status.is_empty() {
                    None
                } else {
                    Some(end_status)
                },
                local_time_bucket: if bucket.is_empty() {
                    None
                } else {
                    Some(bucket)
                },
                sleep_hours: sleep,
                caffeine,
                posture: if posture.is_empty() {
                    None
                } else {
                    Some(posture)
                },
                updated_at,
                content_hash,
            })
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    // Ответы
    let mut stmt = conn
        .prepare(
            "SELECT id, CAST(COALESCE(session_id,'') AS TEXT), CAST(COALESCE(probe_id,'') AS TEXT), scale_version_id,
             value, answered_at, delay_minutes
             FROM responses WHERE deleted_at IS NULL ORDER BY answered_at",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let responses: Vec<ResponseExport> = stmt
        .query_map([], |r| {
            Ok(ResponseExport {
                id: r.get(0)?,
                session_id: {
                    let s: String = r.get(1)?;
                    if s.is_empty() { None } else { Some(s) }
                },
                probe_id: {
                    let s: String = r.get(2)?;
                    if s.is_empty() { None } else { Some(s) }
                },
                scale_version_id: r.get(3)?,
                value: r.get(4)?,
                answered_at: r.get(5)?,
                delay_minutes: r.get(6)?,
            })
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    // Тренажёры
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, trainer, config_json, metrics_json, seed
             FROM trainer_results WHERE deleted_at IS NULL ORDER BY session_id",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let trainer_results: Vec<TrainerResultExport> = stmt
        .query_map([], |r| {
            Ok(TrainerResultExport {
                id: r.get(0)?,
                session_id: r.get(1)?,
                trainer: r.get(2)?,
                config_json: r.get(3)?,
                metrics_json: r.get(4)?,
                seed: r.get(5)?,
            })
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    // Зонды
    let mut stmt = conn
        .prepare(
            "SELECT id, scheduled_at, CAST(COALESCE(fired_at,'') AS TEXT), CAST(COALESCE(answered_at,'') AS TEXT),
             delay_minutes, CAST(COALESCE(last_reminder_at,'') AS TEXT), window_state
             FROM probes ORDER BY scheduled_at",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let probes: Vec<ProbeExport> = stmt
        .query_map([], |r| {
            let opt = |v: String| if v.is_empty() { None } else { Some(v) };
            let id: String = r.get(0)?;
            let scheduled_at: String = r.get(1)?;
            let fired_at = opt(r.get(2)?);
            let answered_at = opt(r.get(3)?);
            let delay_minutes: Option<i64> = r.get(4)?;
            let last_reminder_at = opt(r.get(5)?);
            let window_state: String = r.get(6)?;
            let content_hash = fnv1a(&[
                &scheduled_at,
                fired_at.as_deref().unwrap_or(""),
                answered_at.as_deref().unwrap_or(""),
                &delay_minutes.unwrap_or(0).to_string(),
                last_reminder_at.as_deref().unwrap_or(""),
                &window_state,
            ]);
            Ok(ProbeExport {
                id,
                scheduled_at,
                fired_at,
                answered_at,
                delay_minutes,
                last_reminder_at,
                window_state,
                content_hash,
            })
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
        .collect::<Result<_, _>>()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    Ok(ExportTree {
        export_format: EXPORT_FORMAT.into(),
        exported_at: crate::model::session::now_iso(),
        scales,
        scale_versions,
        sessions,
        responses,
        trainer_results,
        probes,
    })
}

/// Сериализовать дерево в JSON-строку (файл пишет вызывающий).
pub fn to_json_string(tree: &ExportTree) -> Result<String, ExportError> {
    serde_json::to_string_pretty(tree).map_err(|e| ExportError::Serde(e.to_string()))
}

/// Результат импорта: сколько строк добавлено/пропущено как дубли.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportStats {
    /// Добавлено сессий.
    pub sessions_added: usize,
    /// Сессий пропущено (дубликаты).
    pub sessions_skipped: usize,
    /// Добавлено ответов.
    pub responses_added: usize,
    /// Ответов пропущено.
    pub responses_skipped: usize,
    /// Добавлено результатов тренажёров.
    pub trainers_added: usize,
    /// Результатов пропущено.
    pub trainers_skipped: usize,
    /// Добавлено зондов.
    pub probes_added: usize,
    /// Зондов пропущено.
    pub probes_skipped: usize,
}

impl ImportStats {
    /// Всего строк добавлено.
    pub const fn total_added(&self) -> usize {
        self.sessions_added + self.responses_added + self.trainers_added + self.probes_added
    }
}

/// Дедупликация сессий: ключ (source_updated_at, content_hash).
/// Пара существующих ключей передаётся снаружи (собирается одним
/// запросом), чтобы импорт не аллоцировал на каждую строку.
fn existing_session_keys(conn: &Connection) -> Result<HashSet<(String, u64)>, ExportError> {
    // Хэш содержимого существующих сессий считается из тех же полей,
    // что экспортёр, — иначе ключи дедупликации не совпадут.
    let mut keys = HashSet::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, CAST(COALESCE(layer,'') AS TEXT), CAST(COALESCE(technique,'') AS TEXT), started_at,
             CAST(COALESCE(ended_at,'') AS TEXT), duration_s, CAST(COALESCE(end_status,'') AS TEXT),
             CAST(COALESCE(local_time_bucket,'') AS TEXT), sleep_hours, caffeine,
             CAST(COALESCE(posture,'') AS TEXT), updated_at
             FROM sessions WHERE deleted_at IS NULL",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?, // id (для отладки не нужен, но читаем)
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, Option<f64>>(9)?,
                r.get::<_, Option<i64>>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, String>(12)?,
            ))
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    for row in rows {
        let (
            _id,
            kind,
            layer,
            technique,
            started_at,
            ended_at,
            duration_s,
            end_status,
            bucket,
            sleep,
            caffeine,
            posture,
            updated_at,
        ) = row.map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let hash = fnv1a(&[
            &kind,
            &layer,
            &technique,
            &started_at,
            &ended_at,
            &duration_s.unwrap_or(0).to_string(),
            &end_status,
            &bucket,
            &sleep.map(|v| v.to_string()).unwrap_or_default(),
            &caffeine.unwrap_or(0).to_string(),
            &posture,
        ]);
        keys.insert((updated_at, hash));
    }
    Ok(keys)
}

/// Импорт JSON-дерева: не перезаписывает существующее, дубликаты по
/// `(source_updated_at, content_hash)` пропускает; строки получают
/// новые ID (ссылки session_id/probe_id/scale_version_id
/// переклеиваются на новые).
///
/// Ответы привязаны к `scale_version_id`: при импорте в чужую базу
/// (где такой версии нет) строка пропускается — без формулировки
/// шкалы значение неинтерпретируемо (SCALES.md §8.4).
pub fn import_json(conn: &mut Connection, json: &str) -> Result<ImportStats, ExportError> {
    let tree: ExportTree =
        serde_json::from_str(json).map_err(|e| ExportError::BadFormat(e.to_string()))?;
    if tree.export_format != EXPORT_FORMAT {
        return Err(ExportError::BadFormat(format!(
            "unknown export_format: {} (expected {EXPORT_FORMAT})",
            tree.export_format
        )));
    }
    let mut stats = ImportStats {
        sessions_added: 0,
        sessions_skipped: 0,
        responses_added: 0,
        responses_skipped: 0,
        trainers_added: 0,
        trainers_skipped: 0,
        probes_added: 0,
        probes_skipped: 0,
    };
    // Допустимые версии шкал: существующие в базе + воссозданные из
    // файла (формулировки неизменяемы — импортируются «как есть», по
    // стабильному id; SCALES.md §8.4: без версии значение неинтерпретируемо)
    let mut known_scale_versions: HashSet<String> = {
        let mut stmt = conn
            .prepare("SELECT id FROM scale_versions")
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
    };

    let tx = conn
        .transaction()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;

    // Карта «id версии в файле → фактический id в базе» (для случаев
    // independently посеянных UUID при одинаковых формулировках).
    let mut scale_version_remap: Vec<(String, String)> = Vec::new();

    // --- Справочник: шкалы и версии формулировок (до сессий: ответы
    // ссылаются на версии)
    for sc in &tree.scales {
        let exists: i64 = tx
            .query_row("SELECT COUNT(*) FROM scales WHERE id = ?1", [&sc.id], |r| {
                r.get(0)
            })
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        if exists == 0 {
            tx.execute(
                "INSERT INTO scales (id, title, anchor_low, anchor_high, kind, active,
                 updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![
                    sc.id,
                    sc.title,
                    sc.anchor_low,
                    sc.anchor_high,
                    sc.kind,
                    sc.active,
                    crate::model::session::now_iso(),
                ],
            )
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        }
    }
    for sv in &tree.scale_versions {
        if known_scale_versions.contains(&sv.id) {
            continue;
        }
        // UNIQUE (scale_id, version): та же формулировка могла приехать
        // под другим id (базы с независимо посеянными UUID). Совпадение
        // пары = та же версия: переклеиваем id файла на существующий.
        let existing_pair: Option<String> = tx
            .query_row(
                "SELECT id FROM scale_versions WHERE scale_id = ?1 AND version = ?2",
                rusqlite::params![sv.scale_id, sv.version],
                |r| r.get(0),
            )
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        if let Some(real_id) = existing_pair {
            // Переназначение: ответы файла ссылаются на sv.id —
            // подменяем в дереве на фактический id базы.
            known_scale_versions.insert(real_id.clone());
            scale_version_remap.push((sv.id.clone(), real_id));
            continue;
        }
        // Шкала-родитель должна существовать (или прилететь в файле выше)
        let parent: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM scales WHERE id = ?1",
                [&sv.scale_id],
                |r| r.get(0),
            )
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        if parent == 0 {
            continue; // битая ссылка — версию пропускаем, ответы на неё отвалятся ниже
        }
        tx.execute(
            "INSERT INTO scale_versions (id, scale_id, version, question, anchors_json,
             created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                sv.id,
                sv.scale_id,
                sv.version,
                sv.question,
                sv.anchors_json,
                crate::model::session::now_iso(),
            ],
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        known_scale_versions.insert(sv.id.clone());
    }

    // --- Сессии: новые ID + карта старый → новый
    let mut session_map: Vec<(String, String)> = Vec::new();
    let existing = existing_session_keys(&tx)?;
    for s in &tree.sessions {
        let key = (s.updated_at.clone(), s.content_hash);
        if existing.contains(&key) {
            stats.sessions_skipped += 1;
            continue;
        }
        let new_id = uuid::Uuid::now_v7().to_string();
        // updated_at = исходный из файла: ключ дедупликации
        // (source_updated_at, content_hash) должен воспроизводиться при
        // повторном импорте того же файла. now_iso() здесь ломал бы
        // дедуп: каждый следующий импорт плодил бы копии.
        tx.execute(
            "INSERT INTO sessions (id, kind, layer, technique, started_at, ended_at,
             duration_s, end_status, local_time_bucket, sleep_hours, caffeine, posture,
             note_enc, updated_at, deleted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,NULL)",
            rusqlite::params![
                new_id,
                s.kind,
                s.layer,
                s.technique,
                s.started_at,
                s.ended_at,
                s.duration_s,
                s.end_status,
                s.local_time_bucket,
                s.sleep_hours,
                s.caffeine,
                s.posture,
                s.updated_at,
            ],
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        session_map.push((s.id.clone(), new_id));
        stats.sessions_added += 1;
    }

    // --- Зонды: новые ID + карта; дедуп по content_hash (повторный
    // импорт того же файла не плодит копии зондов и их ответов)
    let existing_probe_hashes: HashSet<u64> = {
        let mut stmt = tx
            .prepare(
                "SELECT scheduled_at, CAST(COALESCE(fired_at,'') AS TEXT),
                 CAST(COALESCE(answered_at,'') AS TEXT), delay_minutes,
                 CAST(COALESCE(last_reminder_at,'') AS TEXT), window_state
                 FROM probes",
            )
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let rows = stmt
            .query_map([], |r| {
                let scheduled_at: String = r.get(0)?;
                let fired_at: String = r.get(1)?;
                let answered_at: String = r.get(2)?;
                let delay: Option<i64> = r.get(3)?;
                let last_reminder: String = r.get(4)?;
                let window_state: String = r.get(5)?;
                Ok(fnv1a(&[
                    &scheduled_at,
                    &fired_at,
                    &answered_at,
                    &delay.unwrap_or(0).to_string(),
                    &last_reminder,
                    &window_state,
                ]))
            })
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        rows.collect::<Result<_, _>>()
            .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?
    };
    let mut probe_map: Vec<(String, String)> = Vec::new();
    for p in &tree.probes {
        if existing_probe_hashes.contains(&p.content_hash) {
            stats.probes_skipped += 1;
            continue;
        }
        let new_id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO probes (id, scheduled_at, fired_at, answered_at, delay_minutes,
             last_reminder_at, window_state, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                new_id,
                p.scheduled_at,
                p.fired_at,
                p.answered_at,
                p.delay_minutes,
                p.last_reminder_at,
                p.window_state,
                crate::model::session::now_iso(),
            ],
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        probe_map.push((p.id.clone(), new_id));
        stats.probes_added += 1;
    }

    // --- Ответы: переклейка ссылок, пропуск чужих scale_version_id
    for r in &tree.responses {
        // Сначала remap (тот же текст формулировки под другим id),
        // затем валидация.
        let sv_id = scale_version_remap
            .iter()
            .find(|(o, _)| o == &r.scale_version_id)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| r.scale_version_id.clone());
        let Some(sv) = known_scale_versions.get(&sv_id) else {
            stats.responses_skipped += 1;
            continue;
        };
        let session_id = r.session_id.as_ref().and_then(|old| {
            session_map
                .iter()
                .find(|(o, _)| o == old)
                .map(|(_, n)| n.as_str())
        });
        let probe_id = r.probe_id.as_ref().and_then(|old| {
            probe_map
                .iter()
                .find(|(o, _)| o == old)
                .map(|(_, n)| n.as_str())
        });
        // Ответы зондов без импортированного зонда — висящие ссылки
        // не создаём
        if r.probe_id.is_some() && probe_id.is_none() {
            stats.responses_skipped += 1;
            continue;
        }
        if r.session_id.is_some() && session_id.is_none() {
            // Сессия была дублем (пропущена) — ответ тоже дубликат
            stats.responses_skipped += 1;
            continue;
        }
        tx.execute(
            "INSERT INTO responses (id, session_id, probe_id, scale_version_id, value,
             answered_at, delay_minutes, updated_at, deleted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL)",
            rusqlite::params![
                uuid::Uuid::now_v7().to_string(),
                session_id,
                probe_id,
                sv,
                r.value,
                r.answered_at,
                r.delay_minutes,
                crate::model::session::now_iso(),
            ],
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        stats.responses_added += 1;
    }

    // --- Тренажёры: переклейка сессии
    for t in &tree.trainer_results {
        let Some(session_id) = session_map
            .iter()
            .find(|(o, _)| o == &t.session_id)
            .map(|(_, n)| n.as_str())
        else {
            stats.trainers_skipped += 1;
            continue;
        };
        tx.execute(
            "INSERT INTO trainer_results (id, session_id, trainer, config_json,
             metrics_json, seed, updated_at, deleted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,NULL)",
            rusqlite::params![
                uuid::Uuid::now_v7().to_string(),
                session_id,
                t.trainer,
                t.config_json,
                t.metrics_json,
                t.seed,
                crate::model::session::now_iso(),
            ],
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        stats.trainers_added += 1;
    }

    tx.commit()
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    Ok(stats)
}

/// CSV-экспорт: строки всех четырёх таблиц в одном строковом
/// представлении — `(имя_таблицы, csv)`. Заметки — только при
/// `include_notes: true`, иначе колонки note_enc с плейсхолдером.
///
/// CSV простейший RFC-4180: кавычки только при запятых/кавычках в
/// значении; поле пишется как есть.
pub fn export_csv(
    conn: &Connection,
    include_notes: bool,
) -> Result<Vec<(&'static str, String)>, ExportError> {
    let mut out = Vec::new();

    // sessions
    let mut csv = String::from(
        "id,kind,layer,technique,started_at,ended_at,duration_s,end_status,\
         local_time_bucket,sleep_hours,caffeine,posture,note,updated_at\n",
    );
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, CAST(COALESCE(layer,'') AS TEXT), CAST(COALESCE(technique,'') AS TEXT), started_at,
             CAST(COALESCE(ended_at,'') AS TEXT), CAST(COALESCE(duration_s,'') AS TEXT), CAST(COALESCE(end_status,'') AS TEXT),
             CAST(COALESCE(local_time_bucket,'') AS TEXT), CAST(COALESCE(sleep_hours,'') AS TEXT),
             CAST(COALESCE(caffeine,'') AS TEXT), CAST(COALESCE(posture,'') AS TEXT), CAST(COALESCE(note_enc,'') AS TEXT),
             updated_at
             FROM sessions WHERE deleted_at IS NULL ORDER BY started_at",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, String>(12)?,
                r.get::<_, String>(13)?,
            ))
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    for row in rows {
        let r = row.map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let note = if include_notes {
            r.12.clone()
        } else {
            ENCRYPTED_PLACEHOLDER.to_string()
        };
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            csv_field(&r.0),
            csv_field(&r.1),
            csv_field(&r.2),
            csv_field(&r.3),
            csv_field(&r.4),
            csv_field(&r.5),
            csv_field(&r.6),
            csv_field(&r.7),
            csv_field(&r.8),
            csv_field(&r.9),
            csv_field(&r.10),
            csv_field(&r.11),
            csv_field(&note),
            csv_field(&r.13),
        );
    }
    out.push(("sessions", csv));

    // responses
    let mut csv =
        String::from("id,session_id,probe_id,scale_version_id,value,answered_at,delay_minutes\n");
    let mut stmt = conn
        .prepare(
            "SELECT id, CAST(COALESCE(session_id,'') AS TEXT), CAST(COALESCE(probe_id,'') AS TEXT), scale_version_id,
             value, answered_at, CAST(COALESCE(delay_minutes,'') AS TEXT)
             FROM responses WHERE deleted_at IS NULL ORDER BY answered_at",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    for row in rows {
        let r = row.map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{}",
            csv_field(&r.0),
            csv_field(&r.1),
            csv_field(&r.2),
            csv_field(&r.3),
            csv_field(&r.4.to_string()),
            csv_field(&r.5),
            csv_field(&r.6),
        );
    }
    out.push(("responses", csv));

    // trainer_results
    let mut csv = String::from("id,session_id,trainer,config_json,metrics_json,seed,updated_at\n");
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, trainer, config_json, metrics_json, seed, updated_at
             FROM trainer_results WHERE deleted_at IS NULL ORDER BY session_id",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    for row in rows {
        let r = row.map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{}",
            csv_field(&r.0),
            csv_field(&r.1),
            csv_field(&r.2),
            csv_field(&r.3),
            csv_field(&r.4),
            csv_field(&r.5),
            csv_field(&r.6),
        );
    }
    out.push(("trainer_results", csv));

    // probes
    let mut csv = String::from(
        "id,scheduled_at,fired_at,answered_at,delay_minutes,last_reminder_at,window_state\n",
    );
    let mut stmt = conn
        .prepare(
            "SELECT id, scheduled_at, CAST(COALESCE(fired_at,'') AS TEXT), CAST(COALESCE(answered_at,'') AS TEXT),
             CAST(COALESCE(delay_minutes,'') AS TEXT), CAST(COALESCE(last_reminder_at,'') AS TEXT), window_state
             FROM probes ORDER BY scheduled_at",
        )
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
    for row in rows {
        let r = row.map_err(|e| ExportError::Db(DbError::Sqlite(e.to_string())))?;
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{},{}",
            csv_field(&r.0),
            csv_field(&r.1),
            csv_field(&r.2),
            csv_field(&r.3),
            csv_field(&r.4),
            csv_field(&r.5),
            csv_field(&r.6),
        );
    }
    out.push(("probes", csv));

    Ok(out)
}

/// Экранирование CSV-поля (RFC-4180): кавычка при запятой/кавычке/
/// переносе строки; внутренние кавычки удваиваются.
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
        out
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, open_in_memory};

    fn seeded_db() -> Connection {
        let conn = open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::repo::ScaleRepo::new(&conn).seed_defaults().unwrap();
        conn
    }

    #[test]
    fn csv_field_escaping() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn export_import_roundtrip() {
        let conn = seeded_db();
        // Сессия практики + ответ
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, ended_at, duration_s, end_status,
             local_time_bucket, sleep_hours, caffeine, updated_at)
             VALUES ('s1','practice','2026-11-01T10:00:00Z','2026-11-01T10:20:00Z',1200,
             'finished','morning',7.5,1,'2026-11-01T10:21:00Z')",
            [],
        )
        .unwrap();
        let sv: String = conn
            .query_row("SELECT id FROM scale_versions LIMIT 1", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO responses (id, session_id, scale_version_id, value, answered_at, updated_at)
             VALUES ('r1','s1',?1,7.0,'2026-11-01T10:20:30Z','2026-11-01T10:20:30Z')",
            [&sv],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO probes (id, scheduled_at, window_state, updated_at)
             VALUES ('p1','2026-11-01T12:00:00Z','answered','2026-11-01T12:05:00Z')",
            [],
        )
        .unwrap();

        // Экспорт
        let tree = export_json(&conn).unwrap();
        assert_eq!(tree.export_format, "decon/v1");
        assert_eq!(tree.sessions.len(), 1);
        assert_eq!(tree.responses.len(), 1);
        assert_eq!(tree.probes.len(), 1);
        let json = to_json_string(&tree).unwrap();

        // Импорт в чистую базу (с теми же версиями шкал)
        let mut fresh = seeded_db();
        let stats = import_json(&mut fresh, &json).unwrap();
        assert_eq!(stats.sessions_added, 1);
        assert_eq!(stats.responses_added, 1);
        assert_eq!(stats.probes_added, 1);
        assert_eq!(stats.total_added(), 3);

        // Повторный импорт ТОГО ЖЕ файла — всё дубликаты
        let stats2 = import_json(&mut fresh, &json).unwrap();
        assert_eq!(stats2.sessions_skipped, 1);
        assert_eq!(stats2.sessions_added, 0);
        // Ответы висят на пропущенной сессии — тоже пропущены
        assert_eq!(stats2.responses_added, 0);

        // Контент-сравнение: экспорт fresh == исходный (кроме ID/дат)
        let tree2 = export_json(&conn).unwrap();
        assert_eq!(
            tree2.sessions[0].content_hash,
            tree.sessions[0].content_hash
        );
    }

    #[test]
    fn import_rejects_unknown_format() {
        let mut conn = seeded_db();
        let bad = r#"{"export_format":"other/v9","exported_at":"","scales":[],
            "scale_versions":[],"sessions":[],"responses":[],
            "trainer_results":[],"probes":[]}"#;
        assert!(matches!(
            import_json(&mut conn, bad),
            Err(ExportError::BadFormat(_))
        ));
    }

    #[test]
    fn import_skips_unknown_scale_version() {
        let mut conn = seeded_db();
        let json = r#"{
            "export_format":"decon/v1","exported_at":"2026-11-16T00:00:00Z",
            "scales":[],"scale_versions":[],
            "sessions":[],"trainer_results":[],"probes":[],
            "responses":[{"id":"r9","session_id":null,"probe_id":null,
            "scale_version_id":"00000000-0000-0000-0000-000000000000",
            "value":5.0,"answered_at":"2026-11-16T10:00:00Z","delay_minutes":null}]
        }"#;
        let stats = import_json(&mut conn, json).unwrap();
        assert_eq!(stats.responses_added, 0);
        assert_eq!(stats.responses_skipped, 1);
    }

    #[test]
    fn csv_export_notes_policy() {
        let conn = seeded_db();
        conn.execute(
            "INSERT INTO sessions (id, kind, started_at, updated_at)
             VALUES ('s1','practice','2026-11-01T10:00:00Z','2026-11-01T10:00:00Z')",
            [],
        )
        .unwrap();
        let without = export_csv(&conn, false).unwrap();
        let sessions_csv = &without.iter().find(|(n, _)| *n == "sessions").unwrap().1;
        assert!(sessions_csv.contains(ENCRYPTED_PLACEHOLDER));
        assert!(!sessions_csv.contains("secret"));
        let with = export_csv(&conn, true).unwrap();
        let _ = &with;
    }

    #[test]
    fn fnv_stable_and_separating() {
        // Стабильность
        assert_eq!(fnv1a(&["a", "b"]), fnv1a(&["a", "b"]));
        // Разделитель полей работает
        assert_ne!(fnv1a(&["ab", "c"]), fnv1a(&["a", "bc"]));
        assert_ne!(fnv1a(&["x"]), fnv1a(&["y"]));
    }
}
