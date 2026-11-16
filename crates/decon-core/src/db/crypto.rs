//! Шифрование чувствительных полей (SPEC-data-model.md §7).
//!
//! fernet (AES-128-CBC + HMAC-SHA256) на чистом Rust (фича `rustcrypto` —
//! без OpenSSL, дружелюбно к Windows). Ключ: авто-режим — случайные 32
//! байта в OS keyring; режим passphrase — argon2id. Крейт предоставляет
//! только криптографию; хранение ключа — платформенный слой decon-app
//! (`platform/keyring`), чтобы core остался без системных зависимостей.

use fernet::{Fernet, MultiFernet};
use thiserror::Error;

/// Ошибки криптослоя.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// Неверный пароль: расшифровка не прошла.
    #[error("decryption failed: wrong key or corrupted data")]
    Decryption,
    /// Ошибка конфигурации ключа.
    #[error("invalid key material: {0}")]
    Key(String),
}

/// Шифровальщик заметок: один активный ключ, опционально — предыдущие
/// (ротация ключа читает старые токены, пишет новые).
pub struct FieldCryptor {
    fernet: MultiFernet,
}

impl FieldCryptor {
    /// Создать шифровальщик из одного ключа (base64, 32 байта).
    pub fn new(key_b64: &str) -> Result<Self, CryptoError> {
        let f = Fernet::new(key_b64).ok_or_else(|| CryptoError::Key("bad key".into()))?;
        Ok(FieldCryptor {
            fernet: MultiFernet::new(vec![f]),
        })
    }

    /// Зашифровать открытый текст → fernet-токен.
    pub fn encrypt(&self, plaintext: &str) -> String {
        self.fernet.encrypt(plaintext.as_bytes())
    }

    /// Расшифровать токен. Ошибка — типизированная, не паника.
    pub fn decrypt(&self, token: &str) -> Result<String, CryptoError> {
        self.fernet
            .decrypt(token)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .map_err(|_| CryptoError::Decryption)
    }
}

/// Сгенерировать новый случайный ключ fernet (base64, 32 байта).
pub fn generate_key() -> String {
    Fernet::generate_key()
}

/// Вывести ключ fernet из passphrase через argon2id (режим passphrase).
///
/// Параметры консервативны (m=64 МБ, t=3, p=1) — desktop-таргет.
/// Salt обязан быть уникальным на пользователя; хранится в `settings`
/// открыто (не секретен).
pub fn derive_key_passphrase(passphrase: &str, salt_b64: &str) -> Result<String, CryptoError> {
    use argon2::Argon2;
    use base64::Engine;

    // argon2id v19, m=65536 КБ (64 МБ), t=3, p=1 — базовый OWASP-профиль.
    let params = argon2::Params::new(65_536, 3, 1, Some(32))
        .map_err(|e| CryptoError::Key(format!("argon2 params: {e}")))?;
    let ctx = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    // Salt — стандартный base64 (как хранится в settings); ключ — URL-safe
    // без padding: таков формат fernet-ключей (см. b64_decode_url в крейте).
    let salt_bytes = base64::engine::general_purpose::STANDARD
        .decode(salt_b64)
        .map_err(|e| CryptoError::Key(format!("salt decode: {e}")))?;
    // Массив u8 длиной 32 из хэша argon2id.
    let mut out = [0u8; 32];
    ctx.hash_password_into(passphrase.as_bytes(), &salt_bytes, &mut out)
        .map_err(|e| CryptoError::Key(format!("argon2: {e}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = generate_key();
        let c = FieldCryptor::new(&key).unwrap();
        let secret = "Заметка о срыве в концентрацию — русский текст, emoji 🧘";
        let token = c.encrypt(secret);
        assert_ne!(token, secret);
        assert_eq!(c.decrypt(&token).unwrap(), secret);
    }

    #[test]
    fn wrong_key_fails_typed() {
        let c1 = FieldCryptor::new(&generate_key()).unwrap();
        let c2 = FieldCryptor::new(&generate_key()).unwrap();
        let token = c1.encrypt("secret");
        assert!(matches!(c2.decrypt(&token), Err(CryptoError::Decryption)));
    }

    #[test]
    fn garbage_token_fails() {
        let c = FieldCryptor::new(&generate_key()).unwrap();
        assert!(c.decrypt("not-a-fernet-token").is_err());
        assert!(c.decrypt("").is_err());
    }

    #[test]
    fn passphrase_derivation_is_deterministic_per_salt() {
        let salt = "c2FsdC1zYWx0LXNhbHQtc2FsdA=="; // 16 байт "salt-salt-salt"
        let k1 = derive_key_passphrase("правильная парольная фраза", salt).unwrap();
        let k2 = derive_key_passphrase("правильная парольная фраза", salt).unwrap();
        assert_eq!(k1, k2, "тот же passphrase+salt → тот же ключ");

        let k3 = derive_key_passphrase("другая фраза", salt).unwrap();
        assert_ne!(k1, k3, "другой passphrase → другой ключ");

        let salt2 = "c2FsdC1vdGhlci1zYWx0LXZhbHVl"; // "salt-other-saltvalue"
        let k4 = derive_key_passphrase("правильная парольная фраза", salt2).unwrap();
        assert_ne!(k1, k4, "другой salt → другой ключ");

        // Ключ — валидный fernet-ключ: шифровальщик из него работает
        let c = FieldCryptor::new(&k1).unwrap();
        assert_eq!(c.decrypt(&c.encrypt("x")).unwrap(), "x");
    }

    #[test]
    fn passphrase_key_len_32() {
        let salt = "c2FsdC1zYWx0LXNhbHQtc2FsdA==";
        let key = derive_key_passphrase("p", salt).unwrap();
        use base64::Engine;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&key)
            .unwrap();
        assert_eq!(raw.len(), 32);
        // Fernet принимает ключ этого формата (как generate_key())
        assert!(FieldCryptor::new(&key).is_ok());
    }

    #[test]
    fn tokens_are_unique_per_encryption() {
        // Случайный IV fernet: два шифрования одного текста ≠
        let c = FieldCryptor::new(&generate_key()).unwrap();
        let t1 = c.encrypt("same");
        let t2 = c.encrypt("same");
        assert_ne!(t1, t2);
    }
}
