//! Ключ восстановления: мастер-ключ, записанный 44 словами, совместимо с Cryptomator.
//!
//! 64 байта мастер-ключа + 2 байта контрольной суммы = 66 байт. Каждые 3 байта
//! дают два слова по 12 бит из списка в 4096 английских слов. Контрольная сумма:
//! первые два байта CRC32 мастер-ключа в порядке little-endian (так их выдаёт
//! `HashCode.asBytes()` из Guava в Cryptomator). Проверено на ключе, который
//! создал Cryptomator 1.19.6 для эталонного хранилища.
//!
//! Список слов `words-en.txt` взят из Cryptomator
//! (src/main/resources/i18n/4096words_en.txt, GPLv3, (c) Skymatic GmbH).

use std::collections::HashMap;
use std::sync::OnceLock;

use zeroize::Zeroizing;

use crate::locked::Locked;
use crate::{Error, MasterKey, Result};

const WORDS_FILE: &str = include_str!("words-en.txt");
pub const WORD_COUNT: usize = 4096;
pub const KEY_WORDS: usize = 44;

fn words() -> &'static (Vec<&'static str>, HashMap<&'static str, u16>) {
    static W: OnceLock<(Vec<&'static str>, HashMap<&'static str, u16>)> = OnceLock::new();
    W.get_or_init(|| {
        let list: Vec<&str> = WORDS_FILE
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(WORD_COUNT)
            .collect();
        assert_eq!(list.len(), WORD_COUNT, "word list must have 4096 words");
        let idx = list
            .iter()
            .enumerate()
            .map(|(i, w)| (*w, i as u16))
            .collect();
        (list, idx)
    })
}

/// Список слов (для подсказок при вводе).
pub fn dictionary() -> &'static [&'static str] {
    &words().0
}

fn checksum(raw: &[u8; 64]) -> [u8; 2] {
    let crc = crc32fast::hash(raw).to_le_bytes();
    [crc[0], crc[1]]
}

/// Записать мастер-ключ словами.
pub fn encode(key: &MasterKey) -> Zeroizing<String> {
    let raw = key.to_raw();
    let mut padded = Locked::<66>::new();
    padded[..64].copy_from_slice(&raw[..]);
    padded[64..].copy_from_slice(&checksum(&raw));
    let list = &words().0;
    let mut out = Zeroizing::new(String::with_capacity(KEY_WORDS * 8));
    for (i, g) in padded.chunks(3).enumerate() {
        let w1 = ((g[0] as usize) << 4) | ((g[1] as usize) >> 4);
        let w2 = (((g[1] as usize) & 0x0F) << 8) | g[2] as usize;
        if i > 0 {
            out.push(' ');
        }
        out.push_str(list[w1]);
        out.push(' ');
        out.push_str(list[w2]);
    }
    out
}

/// Прочитать ключ из слов. Лишние пробелы и регистр не важны.
/// Неизвестное слово, неверное число слов или неверная контрольная сумма дают ошибку.
pub fn decode(text: &str) -> Result<MasterKey> {
    let idx = &words().1;
    let lower = Zeroizing::new(text.to_lowercase());
    let parts: Vec<&str> = lower.split_whitespace().collect();
    if parts.len() != KEY_WORDS {
        return Err(Error::BadRecoveryKey);
    }
    let mut padded = Locked::<66>::new();
    for (i, pair) in parts.chunks(2).enumerate() {
        let a = *idx.get(pair[0]).ok_or(Error::BadRecoveryKey)? as usize;
        let b = *idx.get(pair[1]).ok_or(Error::BadRecoveryKey)? as usize;
        padded[i * 3] = (a >> 4) as u8;
        padded[i * 3 + 1] = (((a & 0x0F) << 4) | (b >> 8)) as u8;
        padded[i * 3 + 2] = (b & 0xFF) as u8;
    }
    let mut raw = Locked::<64>::new();
    raw.copy_from_slice(&padded[..64]);
    if checksum(&raw) != [padded[64], padded[65]] {
        return Err(Error::BadRecoveryKey);
    }
    Ok(MasterKey::from_raw(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let k = MasterKey::generate().unwrap();
        let words = encode(&k);
        assert_eq!(words.split(' ').count(), KEY_WORDS);
        assert_eq!(*decode(&words).unwrap().to_raw(), *k.to_raw());
        assert_eq!(
            *decode(&words.to_uppercase()).unwrap().to_raw(),
            *k.to_raw()
        );
    }

    #[test]
    fn typo_is_detected() {
        let k = MasterKey::generate().unwrap();
        let words = encode(&k);
        let mut parts: Vec<String> = words.split(' ').map(String::from).collect();
        // заменить одно слово на другое из словаря: контрольная сумма должна не сойтись
        let other = dictionary().iter().find(|w| **w != parts[5]).unwrap();
        parts[5] = other.to_string();
        assert!(decode(&parts.join(" ")).is_err());
        assert!(decode("not enough words").is_err());
    }
}
