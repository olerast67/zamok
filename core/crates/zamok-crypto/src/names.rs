//! Имена файлов и пути каталогов.
//!
//! - Имя: NFC → AES-SIV (связанные данные = ID родительского каталога) → base64url с `=` → `+ ".c9r"`.
//! - Если зашифрованное имя с `.c9r` длиннее порога (обычно 220), на диске лежит каталог
//!   `base64url(SHA-1(имя.c9r)) + ".c9s"`, а полное имя хранится в нём в файле `name.c9s`.
//! - Каталог с ID `id` лежит в `d/XX/YYYY…`, где `XXYYYY…` = base32(SHA-1(AES-SIV(id))).
//!   У корня ID пустой.

use aes_siv::siv::Aes256Siv;
use aes_siv::KeyInit;
use base64::engine::general_purpose::URL_SAFE as B64URL;
use base64::Engine;
use sha1::{Digest, Sha1};
use unicode_normalization::UnicodeNormalization;

use crate::{Error, MasterKey, Result};

pub const C9R: &str = ".c9r";
pub const C9S: &str = ".c9s";
pub const DIR_FILE: &str = "dir.c9r";
pub const CONTENTS_FILE: &str = "contents.c9r";
pub const NAME_FILE: &str = "name.c9s";
pub const SYMLINK_FILE: &str = "symlink.c9r";
/// Запасная копия ID каталога, которую пишет Cryptomator 1.19+ (зашифрованный файл).
pub const DIRID_BACKUP_FILE: &str = "dirid.c9r";

fn siv(key: &MasterKey) -> Aes256Siv {
    Aes256Siv::new_from_slice(key.siv_key().as_ref()).expect("64-byte key")
}

/// Зашифровать имя (без `.c9r`).
pub fn encrypt_name(key: &MasterKey, cleartext: &str, parent_dir_id: &str) -> String {
    let name: String = cleartext.nfc().collect();
    let ct = siv(key)
        .encrypt([parent_dir_id.as_bytes()], name.as_bytes())
        .expect("SIV encryption does not fail");
    B64URL.encode(ct)
}

/// Расшифровать имя (без `.c9r`).
pub fn decrypt_name(key: &MasterKey, ciphertext: &str, parent_dir_id: &str) -> Result<String> {
    let ct = B64URL
        .decode(ciphertext.as_bytes())
        .map_err(|_| Error::Authentication)?;
    let pt = siv(key)
        .decrypt([parent_dir_id.as_bytes()], &ct)
        .map_err(|_| Error::Authentication)?;
    String::from_utf8(pt).map_err(|_| Error::Authentication)
}

/// Имя на диске для узла: `<enc>.c9r` или `<hash>.c9s`, если длинное.
/// Второе значение: полное зашифрованное имя для `name.c9s`, если имя сокращено.
pub fn node_name(
    key: &MasterKey,
    cleartext: &str,
    parent_dir_id: &str,
    threshold: usize,
) -> (String, Option<String>) {
    let full = format!("{}{}", encrypt_name(key, cleartext, parent_dir_id), C9R);
    if full.len() > threshold {
        let hash = Sha1::digest(full.as_bytes());
        (format!("{}{}", B64URL.encode(hash), C9S), Some(full))
    } else {
        (full, None)
    }
}

/// Путь каталога относительно корня хранилища: `d/XX/YYYY…`.
pub fn dir_path(key: &MasterKey, dir_id: &str) -> String {
    let ct = siv(key)
        .encrypt::<[&[u8]; 0], &[u8]>([], dir_id.as_bytes())
        .expect("SIV encryption does not fail");
    let hash = Sha1::digest(&ct);
    let b32 = data_encoding::BASE32.encode(&hash);
    format!("d/{}/{}", &b32[..2], &b32[2..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_parent() {
        let k = MasterKey::generate().unwrap();
        let e = encrypt_name(&k, "Отчёт.docx", "parent");
        assert_eq!(decrypt_name(&k, &e, "parent").unwrap(), "Отчёт.docx");
        assert!(decrypt_name(&k, &e, "other").is_err());
        assert_eq!(
            e,
            encrypt_name(&k, "Отчёт.docx", "parent"),
            "SIV is deterministic"
        );
    }

    #[test]
    fn long_names_are_shortened() {
        let k = MasterKey::generate().unwrap();
        let (n, full) = node_name(&k, &"я".repeat(200), "", 220);
        assert!(n.ends_with(C9S) && full.unwrap().ends_with(C9R));
        let (n, full) = node_name(&k, "short", "", 220);
        assert!(n.ends_with(C9R) && full.is_none());
    }

    #[test]
    fn dir_path_shape() {
        let k = MasterKey::generate().unwrap();
        let p = dir_path(&k, "");
        assert_eq!(p.len(), 2 + 2 + 1 + 30);
    }
}
