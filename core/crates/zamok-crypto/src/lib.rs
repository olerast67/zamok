//! Формат хранилища Cryptomator, версия 8 (vault format 8, cipher combo SIV_GCM).
//!
//! Своей криптографии здесь нет: только алгоритмы из RustCrypto, собранные
//! по спецификации Cryptomator (docs.cryptomator.org, Security Architecture)
//! и сверенные с хранилищами, которые создал настоящий Cryptomator.
//!
//! - [`masterkey`]: мастер-ключ, файл `masterkey.cryptomator` (scrypt + AES-KW).
//! - [`vault`]: файл `vault.cryptomator` (JWT с подписью HMAC-SHA256).
//! - [`names`]: имена файлов (AES-SIV) и пути каталогов.
//! - [`content`]: заголовок файла и содержимое кусками по 32 КиБ (AES-GCM).
//! - [`tree`]: обход хранилища целиком (для восстановления и тестов).

pub mod content;
pub mod error;
pub mod masterkey;
pub mod names;
pub mod tree;
pub mod vault;

pub use error::{Error, Result};
pub use masterkey::MasterKey;

/// Заполнить буфер случайными байтами от операционной системы.
pub(crate) fn random_bytes(buf: &mut [u8]) -> Result<()> {
    getrandom::getrandom(buf).map_err(|_| Error::Random)
}

/// Случайный UUID версии 4 в текстовой форме (так Cryptomator называет каталоги и хранилища).
pub(crate) fn uuid_v4() -> Result<String> {
    let mut id = [0u8; 16];
    random_bytes(&mut id)?;
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    let h: String = id.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}
