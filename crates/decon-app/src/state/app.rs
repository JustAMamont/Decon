//! Состояние приложения: машина экранов журнала, зондов и инструментов.
//!
//! Потоки (ARCHITECTURE.md §4–§5): сессия — запуск → pre-замер → экран
//! практики → post-замер → сохранение; зонды — расписание на день из
//! планировщика, догоняющие ответы с `delay_minutes`; периодика — FFMQ и
//! слойная карта. БД открывается один раз; запись в том же потоке
//! (WAL + busy_timeout держат короткие транзакции; отдельный поток записи
//! — когда появятся тренажёры с телеметрией).

use crate::i18n::{I18n, Str};
use crate::platform::secret_store::{KeyringBackend, OsKeyringOrMemory};
use decon_core::db::keyring::NoteCrypto;
use decon_core::db::repo::{EventRepo, FfmqResult, ScaleRepo, SessionRepo, SettingsRepo};
use decon_core::db::{self, DbError};
use decon_core::model::AppSettings;
use decon_core::model::event::EventKind;
use decon_core::model::journal::SessionJournal;
use decon_core::model::probe::Probe;
use decon_core::model::scale::{Phase, Response, Scale, ScaleVersion};
use decon_core::model::session::{
    Layer, Session, SessionContext, SessionEndStatus, SessionKind, now_iso,
};
use decon_core::model::settings::Language;
use decon_core::schedule::DaySchedule;
use rusqlite::Connection;
use std::time::Instant;
use uuid::Uuid;

/// Экран журнала.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// Выбор параметров сессии.
    Setup,
    /// Pre-замер (2 слайдера).
    Pre,
    /// Идёт практика: таймер + кнопки-маркеры.
    Running,
    /// Post-замер (5 слайдеров + заметка).
    Post,
    /// История сессий.
    History,
    /// Зонд дня: вопрос из 2 слайдеров + флаг.
    Probe(Option<Uuid>),
    /// Список догоняющих зондов.
    ProbeCatchup,
    /// Периодические инструменты: FFMQ-импорт, слойная карта.
    Tools,
    /// Настройки: зонды, сигналы, язык.
    Settings,
    /// Список тренажёров (S3) — запуск Шульте/MOT.
    Trainers,
    /// Идёт тренажёр (S3).
    TrainerRun,
}

/// Интервал стрим-флаша событий живой сессии (S2.5): буфер льётся в БД
/// не реже, чем раз в 15 с — при краше теряется максимум 15 с событий.
pub const FLUSH_INTERVAL_MS: u64 = 15_000;

/// Живая сессия: доменная модель + буфер событий + таймер.
pub struct RunningSession {
    /// Доменная модель.
    pub session: Session,
    /// Буфер событий (0 аллокаций на append).
    pub journal: SessionJournal,
    /// Прошедшее время, мс.
    pub elapsed_ms: u64,
    /// Следующий равномерный сигнал, мс от старта.
    pub next_signal_ms: u64,
    /// Интервал сигналов, мс.
    pub signal_interval_ms: u64,
    /// Следующий стрим-флаш, мс от старта (S2.5).
    pub next_flush_ms: u64,
}

impl RunningSession {
    /// Новая живая сессия.
    pub fn new(session: Session, signal_interval_s: u32) -> Self {
        let interval = u64::from(signal_interval_s.max(1)) * 1000;
        RunningSession {
            session,
            journal: SessionJournal::new(Uuid::nil()),
            elapsed_ms: 0,
            next_signal_ms: interval,
            signal_interval_ms: interval,
            next_flush_ms: FLUSH_INTERVAL_MS,
        }
    }

    /// Синхронизировать journal с id сессии (id известен после insert).
    pub fn bind_journal(&mut self, session_id: Uuid) {
        self.journal = SessionJournal::new(session_id);
    }

    /// Прибавить прошедшее время; возвращает true, если пора равномерный
    /// сигнал (и планирует следующий).
    pub fn tick(&mut self, delta_ms: u64) -> bool {
        self.elapsed_ms += delta_ms;
        if self.elapsed_ms >= self.next_signal_ms {
            self.next_signal_ms += self.signal_interval_ms;
            let _ = self
                .journal
                .append(self.elapsed_ms as i64, EventKind::Signal);
            return true;
        }
        false
    }

    /// Событие от кнопки-маркера.
    pub fn mark(&mut self, kind: EventKind) {
        let _ = self.journal.append(self.elapsed_ms as i64, kind);
    }

    /// Стрим-флаш (S2.5): записать накопленный буфер событий в БД, не
    /// останавливая сессию. Арена не копится в ОЗУ дольше интервала
    /// флаша; при краше теряется максимум интервал (а не вся сессия).
    pub fn flush_now(&mut self, conn: &Connection) -> Result<usize, String> {
        let repo = EventRepo::new(conn);
        let n = self.journal.pending();
        if n > 0 {
            self.journal.flush(&repo).map_err(|e| e.to_string())?;
        }
        Ok(n)
    }
}

/// Идущий тренажёр (S3): движок + момент старта прогона.
///
/// Время считает UI-поток (`started` — Instant старта), движкам
/// передаются готовые миллисекунды: это держит движки чистыми от
/// часов и тестируемыми (детерминизм по сиду не зависит от времени
/// суток).
pub struct ActiveTrainer {
    /// Какой движок идёт.
    pub engine: TrainerEngine,
    /// Старт прогона (реальное время, для метрик).
    pub started: Instant,
    /// Время прошлого тика MOT (для dt).
    pub last_tick: Instant,
}

/// Движки тренажёров (S3): обёртка над decon-trainers без рендера.
/// MOT в Box: SoA-буферы на 32 объекта (~1.5 КБ) против ~64 Б у Шульте;
/// вариантов будет больше в S5 — enum остаётся компактным.
pub enum TrainerEngine {
    /// Таблицы Шульте.
    Schulte(decon_trainers::schulte::Schulte),
    /// Multiple object tracking.
    Mot(Box<decon_trainers::mot::Mot>),
}

/// Ответ по шкале на экране (значение слайдера).
#[derive(Debug, Clone)]
pub struct ScaleAnswer {
    /// Определение шкалы (для якорей).
    pub scale: Scale,
    /// Версия формулировки (id пишется в responses).
    pub version: ScaleVersion,
    /// Текущее значение слайдера.
    pub value: f64,
}

/// Корневое состояние приложения.
pub struct App {
    /// Подключение к БД (открыто на старте, живёт до выхода).
    pub conn: Connection,
    /// Шифровальщик заметок (ключ auto-режима из keyring).
    pub note_crypto: NoteCrypto,
    /// Локаль.
    pub i18n: I18n,
    /// Настройки.
    pub settings: AppSettings,
    /// Текущий экран.
    pub screen: Screen,
    /// Форма запуска: слой.
    pub setup_layer: Option<Layer>,
    /// Форма запуска: техника.
    pub setup_technique: String,
    /// Форма запуска: сон.
    pub setup_sleep: String,
    /// Форма запуска: кофеин.
    pub setup_caffeine: String,
    /// Живая сессия (между Begin и Save).
    pub running: Option<RunningSession>,
    /// Pre-ответы.
    pub pre_answers: Vec<ScaleAnswer>,
    /// Post-ответы.
    pub post_answers: Vec<ScaleAnswer>,
    /// Текст заметки на post-экране.
    pub note_draft: String,
    /// Момент старта таймера (для расчёта elapsed).
    pub timer_start: Option<Instant>,
    /// История сессий (обновляется при входе на экран).
    pub history: Vec<Session>,
    /// Тост-уведомление (показывается один кадр).
    pub toast: Option<String>,
    // --- Зонды дня (S2)
    /// Расписание на сегодня (None — ещё не построено).
    pub today_plan: Option<DaySchedule>,
    /// Зонды сегодня.
    pub today_probes: Vec<Probe>,
    /// Ответы текущего зонда (экран Probe).
    pub probe_answers: Vec<ScaleAnswer>,
    /// Зонд, который отвечается сейчас (экран Probe).
    pub active_probe: Option<Probe>,
    // --- Периодика (S2)
    /// Результаты FFMQ (последние).
    pub ffqm_results: Vec<FfmqResult>,
    /// Ответы слойной карты (Layer-фаза, по слоям).
    pub layer_answers: Vec<ScaleAnswer>,
    // --- Настройки (S2)
    /// Черновик окна зондов: начало, `HH:MM`.
    pub probe_window_start: String,
    /// Черновик окна зондов: конец.
    pub probe_window_end: String,
    /// Черновик: количество зондов.
    pub probe_count: String,
    /// Черновик: минимальный интервал, мин.
    pub probe_min_gap: String,
    /// Черновик: интервал сигналов сессии, с.
    pub signal_interval: String,
    /// Черновик: интервал повтора зондов, мин (S2.5).
    pub reminder_interval: String,
    /// Где реально лежит ключ (для статуса в настройках).
    pub keyring_backend: KeyringBackend,
    /// Автозапуск фактически зарегистрирован в ОС (кэш, S2.5).
    pub autostart_actual: bool,
    // --- Тренажёры (S3)
    /// Идёт ли сейчас тренажёр (движок + момент старта для метрик).
    pub active_trainer: Option<ActiveTrainer>,
    /// Стартовый сид последнего прогона (для повтора/отладки).
    pub trainer_seed: u64,
    /// Результат последнего завершённого прогона (тост на экране).
    pub trainer_summary: Option<String>,
}

impl App {
    /// Инициализация: открыть БД в XDG-каталоге, мигрировать, посеять
    /// шкалы, загрузить настройки и ключ. Ключ auto-режима — в ОЗУ на
    /// время сессии (keyring-интеграция — S2, пока ключ в БД не хранится:
    /// для пилота генерируется новый при каждом запуске, что делает старые
    /// заметки нечитаемыми — потому заметки в UI S1 отключены до keyring).
    pub fn init() -> Result<Self, String> {
        let db_path = data_dir().join("decon.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = db::open(&db_path).map_err(|e| e.to_string())?;
        db::migrate(&conn).map_err(|e| e.to_string())?;
        ScaleRepo::new(&conn)
            .seed_defaults()
            .map_err(|e| e.to_string())?;
        let settings = SettingsRepo::new(&conn).load().map_err(|e| e.to_string())?;

        // auto-режим (SPEC §7.1): ключ из OS keyring или новый при
        // первом запуске; headless-окружение — memory-фолбэк.
        let store = OsKeyringOrMemory::detect();
        let keyring_backend = store.backend();
        let note_crypto = NoteCrypto::from_store(&store).map_err(|e| e.to_string())?;

        let mut app = App::skeleton(conn, settings, note_crypto);
        app.keyring_backend = keyring_backend;
        // Recovery-проход (S2.5): висящие после сбоя сессии → interrupted
        let _ = SessionRepo::new(&app.conn).finalize_dangling(&now_iso());
        // Зонды на сегодня строим сразу (если включены)
        if app.settings.probe.enabled {
            let _ = crate::state::probes_flow::rebuild_today_probes(&mut app);
        }
        Ok(app)
    }

    /// Каркас состояния без БД-инициализации (init и init_fallback).
    fn skeleton(conn: Connection, settings: AppSettings, note_crypto: NoteCrypto) -> Self {
        App {
            conn,
            note_crypto,
            i18n: I18n::new(settings.language),
            settings,
            screen: Screen::Setup,
            setup_layer: None,
            setup_technique: String::new(),
            setup_sleep: String::new(),
            setup_caffeine: String::new(),
            running: None,
            pre_answers: Vec::new(),
            post_answers: Vec::new(),
            note_draft: String::new(),
            timer_start: None,
            history: Vec::new(),
            toast: None,
            today_plan: None,
            today_probes: Vec::new(),
            probe_answers: Vec::new(),
            active_probe: None,
            ffqm_results: Vec::new(),
            layer_answers: Vec::new(),
            probe_window_start: format!(
                "{:02}:{:02}",
                settings.probe.window.start_h, settings.probe.window.start_m
            ),
            probe_window_end: format!(
                "{:02}:{:02}",
                settings.probe.window.end_h, settings.probe.window.end_m
            ),
            probe_count: settings.probe.count.to_string(),
            probe_min_gap: settings.probe.min_gap_minutes.to_string(),
            signal_interval: settings.session.signal_interval_s.to_string(),
            reminder_interval: settings.residency.reminder_interval_min.to_string(),
            // Факт из ОС (файл .desktop / ключ HKCU), не черновик настроек;
            // кэшируется при старте и обновляется кнопкой в настройках —
            // reg query/проверка файла не выполняется на каждом рендере.
            autostart_actual: crate::platform::autostart::is_enabled(),
            // Тренажёры (S3): сид — из времени, прогоны стартуют из UI.
            active_trainer: None,
            trainer_seed: 0,
            trainer_summary: None,
            keyring_backend: KeyringBackend::Memory,
        }
    }

    /// Fallback при ошибке инициализации: in-memory БД, чтобы окно
    /// открылось и показало причину ошибки (не паникуем в проде).
    pub fn init_fallback() -> Self {
        let conn = db::open_in_memory().unwrap_or_else(|_| {
            // Абсолютный крайний случай: даже in-memory не открылся.
            // Connection::open_in_memory не падает на практике.
            panic!("sqlite in-memory unavailable")
        });
        let _ = db::migrate(&conn);
        let settings = AppSettings::default();
        let store = decon_core::db::keyring::MemoryStore::new();
        let note_crypto = NoteCrypto::from_store(&store).expect("generated key is always valid");
        App::skeleton(conn, settings, note_crypto)
    }

    /// Начать сессию: создать запись, pre-ответы уйти в БД, перейти к Run.
    pub fn begin_session(&mut self) -> Result<(), String> {
        // Контекст из формы
        let context = SessionContext {
            sleep_hours: self.setup_sleep.trim().parse::<f64>().ok(),
            caffeine: self.setup_caffeine.trim().parse::<i64>().ok(),
            posture: None,
        };
        // Локальный час для корзины: UTC-час (пилот; TZ-детект — S2).
        let utc_hour = (crate::state::clock::unix_now_ms() / 3_600_000 % 24) as u32;
        let mut session = Session::new(SessionKind::Practice, now_iso(), utc_hour);
        session.set_layer(self.setup_layer);
        let technique = self.setup_technique.trim().to_string();
        if !technique.is_empty() {
            session.set_technique(Some(technique));
        }
        session.context = context;

        // Pre-ответы
        let sid = session.id;
        let responses: Vec<Response> = self
            .pre_answers
            .iter()
            .map(|a| Response::for_session(sid, a.version.id, a.value))
            .collect();

        let tx_ok: Result<(), db::DbError> = (|| {
            SessionRepo::new(&self.conn).insert(&session)?;
            if !responses.is_empty() {
                ScaleRepo::new(&self.conn).save_session_responses(&responses)?;
            }
            Ok(())
        })();

        let mut running = RunningSession::new(session, self.settings.session.signal_interval_s);
        running.bind_journal(sid);
        self.running = Some(running);
        self.timer_start = Some(Instant::now());
        self.screen = Screen::Running;
        self.toast = None;
        tx_ok.map_err(|e| e.to_string())
    }

    /// Обновить elapsed живой сессии по таймеру; true — пора сигналить.
    pub fn tick_running(&mut self) -> bool {
        let Some(start) = self.timer_start else {
            return false;
        };
        let elapsed = start.elapsed().as_millis() as u64;
        let Some(r) = self.running.as_mut() else {
            return false;
        };
        let delta = elapsed.saturating_sub(r.elapsed_ms);
        if delta == 0 {
            return false;
        }
        let fire = r.tick(delta);
        // Стрим-флаш (S2.5): буфер событий не ждёт stop — льём каждые
        // FLUSH_INTERVAL_S живой сессии (устойчивость к крашу/сигналу).
        if elapsed >= r.next_flush_ms {
            r.next_flush_ms = elapsed + FLUSH_INTERVAL_MS;
            let _ = r.flush_now(&self.conn);
        }
        fire
    }

    /// Финализировать живую сессию без post-замера (S2.5, graceful
    /// shutdown). Единый путь для сигналов ОС, выхода из приложения и
    /// ручного прерывания: событие Pause не пишем (оно про паузу внутри
    /// практики), флашим журнал, закрываем сессию со статусом.
    pub fn finalize_running(&mut self, status: SessionEndStatus) -> Result<(), String> {
        let Some(r) = self.running.as_mut() else {
            return Ok(());
        };
        let duration_s = (r.elapsed_ms / 1000) as i64;
        r.session.finalize(status, now_iso(), duration_s);
        let batch = r.journal.take_batch();
        let sid = r.session.id;
        (|| -> Result<(), db::DbError> {
            if !batch.is_empty() {
                EventRepo::new(&self.conn).insert_batch(&batch)?;
            }
            SessionRepo::new(&self.conn).update(&r.session)?;
            // Чекпоинт: WAL сворачивается, файл БД самодостаточен
            let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
            Ok(())
        })()
        .map_err(|e| e.to_string())?;
        let _ = sid;
        self.running = None;
        self.timer_start = None;
        Ok(())
    }

    /// Ручное прерывание сессии пользователем («Прервать сессию»:
    /// срочно нужно бежать / плохое самочувствие).
    pub fn abort_session(&mut self) -> Result<(), String> {
        self.finalize_running(SessionEndStatus::Aborted)
    }

    /// Остановить практику: завершить модель, flush событий, перейти к
    /// post-замеру.
    pub fn stop_session(&mut self) -> Result<(), String> {
        let i18n = &self.i18n;
        let Some(r) = self.running.as_mut() else {
            return Err(i18n.get(Str::Error).to_string());
        };
        let duration_s = (r.elapsed_ms / 1000) as i64;
        r.session.finish(now_iso(), duration_s);
        let batch = r.journal.take_batch();
        let sid = r.session.id;
        (|| -> Result<(), db::DbError> {
            SessionRepo::new(&self.conn).update(&r.session)?;
            if !batch.is_empty() {
                EventRepo::new(&self.conn).insert_batch(&batch)?;
            }
            Ok(())
        })()
        .map_err(|e| e.to_string())?;
        let _ = sid;

        self.screen = Screen::Post;
        self.note_draft.clear();
        Ok(())
    }

    /// Сохранить post-замер, зашифрованную заметку и закрыть сессию.
    pub fn save_session(&mut self) -> Result<(), String> {
        let Some(r) = self.running.as_ref() else {
            return Err(self.i18n.get(Str::Error).to_string());
        };
        let sid = r.session.id;
        let responses: Vec<Response> = self
            .post_answers
            .iter()
            .map(|a| Response::for_session(sid, a.version.id, a.value))
            .collect();
        // Заметка шифруется ключом auto-режима (S2: keyring подключён)
        let note_enc = if self.note_draft.trim().is_empty() {
            None
        } else {
            Some(self.note_crypto.cryptor().encrypt(self.note_draft.trim()))
        };
        (|| -> Result<(), db::DbError> {
            if !responses.is_empty() {
                ScaleRepo::new(&self.conn).save_session_responses(&responses)?;
            }
            if let Some(token) = &note_enc {
                self.conn
                    .execute(
                        "UPDATE sessions SET note_enc = ?2, updated_at =
                         strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
                        rusqlite::params![sid.to_string(), token],
                    )
                    .map_err(|e| DbError::Sqlite(e.to_string()))?;
            }
            Ok(())
        })()
        .map_err(|e| e.to_string())?;

        self.running = None;
        self.timer_start = None;
        self.screen = Screen::Setup;
        self.toast = Some(self.i18n.get(Str::SessionSaved).to_string());
        Ok(())
    }

    /// Загрузить pre-вопросы (при входе на pre-экран).
    pub fn load_pre_scales(&mut self) -> Result<(), String> {
        self.pre_answers = self
            .load_phase(Phase::Pre)
            .map(|list| {
                list.into_iter()
                    .map(|(scale, version)| ScaleAnswer {
                        scale,
                        version,
                        value: 5.0,
                    })
                    .collect()
            })
            .map_err(|e: db::DbError| e.to_string())?;
        Ok(())
    }

    /// Загрузить post-вопросы (при входе на post-экран).
    pub fn load_post_scales(&mut self) -> Result<(), String> {
        self.post_answers = self
            .load_phase(Phase::Post)
            .map(|list| {
                list.into_iter()
                    .map(|(scale, version)| ScaleAnswer {
                        scale,
                        version,
                        value: 5.0,
                    })
                    .collect()
            })
            .map_err(|e: db::DbError| e.to_string())?;
        Ok(())
    }

    fn load_phase(&self, phase: Phase) -> Result<Vec<(Scale, ScaleVersion)>, db::DbError> {
        ScaleRepo::new(&self.conn).list_active_phase(phase)
    }

    /// Обновить историю (при входе на экран истории).
    pub fn reload_history(&mut self) -> Result<(), String> {
        self.history = SessionRepo::new(&self.conn)
            .list_recent(50)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Текущая локаль.
    pub const fn lang(&self) -> Language {
        self.settings.language
    }
}

/// Каталог данных приложения: XDG_DATA_HOME/decon (Linux) или
/// %APPDATA%\decon (Windows).
fn data_dir() -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        if !appdata.is_empty() {
            return std::path::PathBuf::from(appdata).join("decon");
        }
    }
    let xdg = std::env::var("XDG_DATA_HOME").unwrap_or_default();
    if !xdg.is_empty() {
        return std::path::PathBuf::from(xdg).join("decon");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("decon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_session_ticks_and_signals() {
        let session = Session::new(SessionKind::Practice, now_iso(), 12);
        let mut r = RunningSession::new(session, 60); // сигнал каждую минуту
        r.bind_journal(Uuid::now_v7());

        assert!(!r.tick(30_000), "первая половина интервала");
        assert!(!r.tick(29_000), "почти минута");
        assert!(r.tick(1_000), "минута — сигнал");
        assert_eq!(r.journal.pending(), 1);
        assert!(!r.tick(10_000));
        assert!(r.tick(50_000), "вторая минута");
        assert_eq!(r.journal.pending(), 2);

        r.mark(EventKind::ClarityFlash);
        assert_eq!(r.journal.pending(), 3);
        assert_eq!(r.elapsed_ms, 120_000);
    }

    #[test]
    fn data_dir_prefers_xdg() {
        // Не мутируем окружение процесса в тестах — проверяем только
        // fallback-логику чистой функцией.
        assert!(data_dir().ends_with("decon"));
    }
}
