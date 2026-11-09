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
//! - [`recovery`]: ключ восстановления из 44 слов, как у Cryptomator.
//! - [`tree`]: обход хранилища целиком (для восстановления и тестов).
//! - [`locked`]: память для ключей, которую Windows не выгружает на диск.

pub mod content;
pub mod error;
pub mod locked;
pub mod masterkey;
pub mod names;
pub mod recovery;
pub mod tree;
pub mod vault;

pub use error::{Error, Result};
pub use locked::Locked;
pub use masterkey::MasterKey;

/// Предел размера служебных файлов хранилища. Настоящие занимают от десятков байт до пары
/// килобайт; огромный подложенный файл не должен читаться в память целиком.
pub const META_FILE_MAX: u64 = 1024 * 1024;

/// Прочитать файл, если он не длиннее `max` байт.
pub fn read_capped(path: &std::path::Path, max: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file is too large",
        ));
    }
    Ok(data)
}

/// Служебный файл хранилища (`masterkey.cryptomator`, `dir.c9r`, `name.c9s` и другие).
pub fn read_meta(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    read_capped(path, META_FILE_MAX)
}

/// Служебный файл хранилища как текст UTF-8.
pub fn read_meta_text(path: &std::path::Path) -> std::io::Result<String> {
    String::from_utf8(read_meta(path)?)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "not UTF-8"))
}

/// Заполнить буфер случайными байтами от операционной системы.
pub(crate) fn random_bytes(buf: &mut [u8]) -> Result<()> {
    getrandom::getrandom(buf).map_err(|_| Error::Random)
}

/// Случайный UUID версии 4 в текстовой форме (так Cryptomator называет каталоги и хранилища).
pub fn uuid_v4() -> Result<String> {
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
