//! Хранение мастер-ключа шифрования (ARCHITECTURE.md §6).
//!
//! Core знает только контракт `SecretStore` — где именно лежит ключ,
//! решает платформенный слой приложения (OS keyring). Для тестов и
//! headless-прогона есть `MemoryStore`. Это держит core синхронным и
//! тестируемым без D-Bus/Windows-сервисов.

use crate::db::DbError;
use crate::db::crypto::FieldCryptor;

/// Контракт хранилища секрета мастер-ключа.
///
/// Реализации: OS keyring (app/platform), память (тесты). Ключ —
/// base64-токен fernet (32 байта после декодирования).
pub trait SecretStore {
    /// Прочитать ключ; None — ключа ещё нет (первый запуск).
    fn load(&self) -> Result<Option<String>, String>;

    /// Сохранить ключ (перезапись — при ротации).
    fn save(&self, key: &str) -> Result<(), String>;
}

/// Секрет в памяти (тесты, headless): живёт ровно столько, сколько
/// процесс. В проде не используется — см. `decon-app/src/platform`.
#[derive(Default)]
pub struct MemoryStore {
    slot: std::cell::RefCell<Option<String>>,
}

impl MemoryStore {
    /// Пустое хранилище.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self.slot.borrow().clone())
    }

    fn save(&self, key: &str) -> Result<(), String> {
        *self.slot.borrow_mut() = Some(key.to_string());
        Ok(())
    }
}

/// Имя секрета в keyring (константа контракта, SPEC §7.1).
pub const SECRET_NAME: &str = "decon.master-key";

/// Криптовальщик заметок, привязанный к хранилищу ключа.
///
/// Логика auto-режима (SPEC §7.1): при первом запуске ключа нет —
/// генерируем и сохраняем; далее — читаем один раз на запуск. Смена
/// ключа (ротация) — отдельная операция уровня приложения.
pub struct NoteCrypto {
    cryptor: FieldCryptor,
    pub(crate) _key_b64: String,
}

impl NoteCrypto {
    /// Собрать криптовальщик из хранилища: существующий ключ или новый.
    pub fn from_store(store: &dyn SecretStore) -> Result<Self, String> {
        let key_b64 = match store.load()? {
            Some(existing) => existing,
            None => {
                let fresh = crate::db::crypto::generate_key();
                store.save(&fresh)?;
                fresh
            }
        };
        let cryptor = FieldCryptor::new(&key_b64).map_err(|e| e.to_string())?;
        Ok(NoteCrypto {
            cryptor,
            _key_b64: key_b64,
        })
    }

    /// Ссылка на криптовальщик (шифрование/расшифровка полей).
    pub fn cryptor(&self) -> &FieldCryptor {
        &self.cryptor
    }
}

/// Ротация ключа: повторно зашифровать 🔒-поля сессий в одной транзакции.
///
/// Возвращает число перешифрованных заметок. При сбое на любом шаге —
/// полный откат (SPEC §7.3: никаких тихих потерь).
pub fn rotate_notes_key(
    conn: &rusqlite::Connection,
    old: &FieldCryptor,
    new: &FieldCryptor,
    store: &dyn SecretStore,
) -> Result<usize, DbError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| DbError::Sqlite(e.to_string()))?;

    // Идентификаторы и токены зашифрованных заметок
    let rows: Vec<(String, String)> = {
        let mut stmt = tx
            .prepare(
                "SELECT id, note_enc FROM sessions
                 WHERE note_enc IS NOT NULL AND deleted_at IS NULL",
            )
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        let mapped = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        mapped.filter_map(|r| r.ok()).collect()
    };

    let mut n = 0usize;
    {
        let mut stmt = tx
            .prepare("UPDATE sessions SET note_enc = ?2 WHERE id = ?1")
            .map_err(|e| DbError::Sqlite(e.to_string()))?;
        for (id, token) in &rows {
            let plain = old
                .decrypt(token)
                .map_err(|e| DbError::Crypto(e.to_string()))?;
            let re_enc = new.encrypt(&plain);
            stmt.execute(rusqlite::params![id, re_enc])
                .map_err(|e| DbError::Sqlite(e.to_string()))?;
            n += 1;
        }
    }

    tx.commit().map_err(|e| DbError::Sqlite(e.to_string()))?;

    // Ключ меняем в keyring только после успешного коммита БД.
    store
        .save(new.key_b64())
        .map_err(|e| DbError::Crypto(e.to_string()))?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::repo::SessionRepo;
    use crate::model::session::{Session, SessionKind};

    #[test]
    fn first_run_generates_and_persists_key() {
        let store = MemoryStore::new();
        assert!(store.load().unwrap().is_none(), "первый запуск — ключа нет");

        let a = NoteCrypto::from_store(&store).expect("init");
        assert!(store.load().unwrap().is_some(), "ключ сохранён");

        // Второй запуск — тот же ключ
        let b = NoteCrypto::from_store(&store).expect("re-init");
        assert_eq!(a._key_b64, b._key_b64, "ключ стабилен между запусками");
    }

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::new();
        store
            .save("dGVzdC1rZXktdGVzdC1rZXktdGVzdC1rZXktdGVzdA==")
            .unwrap();
        assert_eq!(
            store.load().unwrap().as_deref(),
            Some("dGVzdC1rZXktdGVzdC1rZXktdGVzdC1rZXktdGVzdA==")
        );
    }

    #[test]
    fn key_rotation_reencrypts_notes_atomically() {
        let conn = db::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();

        let old_store = MemoryStore::new();
        let old_crypto = NoteCrypto::from_store(&old_store).unwrap();
        let old = old_crypto.cryptor();

        // Сессия с заметкой
        let mut session =
            Session::new(SessionKind::Practice, "2026-11-18T10:00:00.000Z".into(), 10);
        session.note = Some("секретная заметка про практику".into());
        let repo = SessionRepo::new(&conn).with_cryptor(old);
        repo.insert(&session).unwrap();
        // Заметка шифруется при записи: положим токен напрямую
        let token = old.encrypt("секретная заметка про практику");
        conn.execute(
            "UPDATE sessions SET note_enc = ?2 WHERE id = ?1",
            rusqlite::params![session.id.to_string(), token],
        )
        .unwrap();

        // Ротация
        let new_key = db::crypto::generate_key();
        let new = FieldCryptor::new(&new_key).unwrap();
        let new_store = MemoryStore::new();
        let n = rotate_notes_key(&conn, old, &new, &new_store).unwrap();
        assert_eq!(n, 1, "одна заметка перешифрована");

        // Новый ключ сохранён, заметка читается новым ключом
        assert_eq!(new_store.load().unwrap().as_deref(), Some(new.key_b64()));
        let reloaded = SessionRepo::new(&conn)
            .with_cryptor(&new)
            .get(&session.id)
            .unwrap()
            .expect("exists");
        assert_eq!(
            reloaded.note.as_deref(),
            Some("секретная заметка про практику")
        );

        // Старым ключом — не читается (расшифровка честно падает)
        let stored_token: String = conn
            .query_row(
                "SELECT note_enc FROM sessions WHERE id = ?1",
                [&session.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(old.decrypt(&stored_token).is_err());
    }
}
