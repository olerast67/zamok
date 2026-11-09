//! Мастер-ключ и файл `masterkey.cryptomator`.
//!
//! Мастер-ключ состоит из двух случайных 256-битных ключей: ключа шифрования
//! и ключа MAC. В файле они лежат завёрнутыми (AES-KW, RFC 3394) ключом,
//! который получается из пароля через scrypt. Пароль перед этим приводится
//! к форме Unicode NFC, как это делает Cryptomator.

use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::{ZeroizeOnDrop, Zeroizing};

use crate::locked::Locked;
use crate::{random_bytes, Error, Result};

/// Номер версии, который Cryptomator пишет в masterkey.cryptomator с формата 8.
/// Настоящая версия формата хранится в vault.cryptomator.
pub const LEGACY_VERSION: u32 = 999;
/// Параметры scrypt по умолчанию у Cryptomator: N = 2^15, r = 8, p = 1.
pub const SCRYPT_COST: u32 = 1 << 15;
pub const SCRYPT_BLOCK_SIZE: u32 = 8;
const SCRYPT_SALT_LEN: usize = 8;
/// Верхняя граница памяти для scrypt из чужого файла (как в cryptolib): 256 МиБ.
const SCRYPT_MAX_MEMORY: u64 = 256 * 1024 * 1024;

/// Мастер-ключ. Лежит в запертой странице, при удалении стирается.
///
/// Внутри 128 байт: ключ шифрования, ключ MAC, затем те же ключи в порядке AES-SIV. Так ключ
/// для имён файлов не нужно собирать во временной копии при каждой операции.
#[derive(Clone)]
pub struct MasterKey {
    keys: Locked<128>,
}

impl ZeroizeOnDrop for MasterKey {}

impl MasterKey {
    /// Новый случайный мастер-ключ.
    pub fn generate() -> Result<Self> {
        let mut raw = Locked::<64>::new();
        random_bytes(&mut raw[..])?;
        Ok(Self::from_raw(&raw))
    }

    /// Из 64 байт: ключ шифрования, затем ключ MAC (так Cryptomator хранит
    /// сырой мастер-ключ, в том числе в ключе восстановления).
    pub fn from_raw(raw: &[u8; 64]) -> Self {
        let mut keys = Locked::<128>::new();
        keys[..64].copy_from_slice(raw);
        keys[64..96].copy_from_slice(&raw[32..]);
        keys[96..].copy_from_slice(&raw[..32]);
        MasterKey { keys }
    }

    pub fn to_raw(&self) -> Locked<64> {
        let mut raw = Locked::<64>::new();
        *raw = *self.jwt_key();
        raw
    }

    pub(crate) fn enc_key(&self) -> &[u8; 32] {
        self.keys.first_chunk().expect("32 < 128")
    }

    pub(crate) fn mac_key(&self) -> &[u8; 32] {
        self.keys[32..].first_chunk().expect("32 < 96")
    }

    /// Ключ для AES-SIV по RFC 5297: сначала ключ MAC (для S2V), затем ключ шифрования (для CTR).
    pub(crate) fn siv_key(&self) -> &[u8; 64] {
        self.keys[64..].first_chunk().expect("64 == 64")
    }

    /// Ключ подписи vault.cryptomator: ключ шифрования, затем ключ MAC.
    pub(crate) fn jwt_key(&self) -> &[u8; 64] {
        self.keys.first_chunk().expect("64 < 128")
    }
}

/// AES-KW из ключа обёртки по ссылке: лишней копии ключа на стеке не остаётся.
fn key_wrap(kek: &[u8; 32]) -> aes_kw::KekAes256 {
    aes_kw::KekAes256::new(kek.into())
}

/// Завернуть мастер-ключ 32-байтовым ключом обёртки (AES-KW, RFC 3394), как в файле
/// `masterkey.cryptomator`, только без scrypt: ключ обёртки уже случайный (вход по Windows Hello).
pub fn wrap_with(key: &MasterKey, kek: &[u8; 32]) -> Result<Vec<u8>> {
    key_wrap(kek)
        .wrap_vec(key.jwt_key())
        .map_err(|_| Error::MalformedMasterkey)
}

/// Развернуть мастер-ключ. Чужой ключ обёртки или испорченные данные: [`Error::WrongPassword`].
pub fn unwrap_with(blob: &[u8], kek: &[u8; 32]) -> Result<MasterKey> {
    if blob.len() != 72 {
        return Err(Error::MalformedMasterkey);
    }
    let mut raw = Locked::<64>::new();
    key_wrap(kek)
        .unwrap(blob, &mut raw[..])
        .map_err(|_| Error::WrongPassword)?;
    Ok(MasterKey::from_raw(&raw))
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(..)")
    }
}

/// Содержимое masterkey.cryptomator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterkeyFile {
    pub version: u32,
    #[serde(rename = "scryptSalt", with = "b64")]
    pub scrypt_salt: Vec<u8>,
    #[serde(rename = "scryptCostParam")]
    pub scrypt_cost_param: u32,
    #[serde(rename = "scryptBlockSize")]
    pub scrypt_block_size: u32,
    #[serde(rename = "primaryMasterKey", with = "b64")]
    pub primary_master_key: Vec<u8>,
    #[serde(rename = "hmacMasterKey", with = "b64")]
    pub hmac_master_key: Vec<u8>,
    #[serde(rename = "versionMac", with = "b64")]
    pub version_mac: Vec<u8>,
}

mod b64 {
    use super::B64;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&B64.encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        B64.decode(s.as_bytes()).map_err(serde::de::Error::custom)
    }
}

fn normalize(passphrase: &str) -> Zeroizing<String> {
    Zeroizing::new(passphrase.nfc().collect())
}

fn derive_kek(passphrase: &str, salt: &[u8], cost: u32, block: u32) -> Result<Locked<32>> {
    if cost < 2 || !cost.is_power_of_two() || block == 0 {
        return Err(Error::UnsupportedScrypt);
    }
    // Оба числа из файла: насыщение, чтобы произведение не перевернулось и не обошло предел
    if (block as u64)
        .saturating_mul(cost as u64)
        .saturating_mul(128)
        > SCRYPT_MAX_MEMORY
    {
        return Err(Error::UnsupportedScrypt);
    }
    let log_n = cost.trailing_zeros() as u8;
    let params = scrypt::Params::new(log_n, block, 1, 32).map_err(|_| Error::UnsupportedScrypt)?;
    let pass = normalize(passphrase);
    let mut kek = Locked::<32>::new();
    scrypt::scrypt(pass.as_bytes(), salt, &params, &mut kek[..])
        .map_err(|_| Error::UnsupportedScrypt)?;
    Ok(kek)
}

fn version_mac(key: &MasterKey, version: u32) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key.mac_key()).expect("any key length");
    m.update(&(version as i32).to_be_bytes());
    m.finalize().into_bytes().to_vec()
}

impl MasterkeyFile {
    /// Завернуть мастер-ключ паролем (новая соль, параметры scrypt по умолчанию).
    pub fn lock(key: &MasterKey, passphrase: &str) -> Result<Self> {
        Self::lock_with(key, passphrase, SCRYPT_COST)
    }

    pub fn lock_with(key: &MasterKey, passphrase: &str, scrypt_cost: u32) -> Result<Self> {
        let mut salt = vec![0u8; SCRYPT_SALT_LEN];
        random_bytes(&mut salt)?;
        let kek = derive_kek(passphrase, &salt, scrypt_cost, SCRYPT_BLOCK_SIZE)?;
        let kw = key_wrap(&kek);
        let primary = kw
            .wrap_vec(key.enc_key())
            .map_err(|_| Error::MalformedMasterkey)?;
        let hmac_key = kw
            .wrap_vec(key.mac_key())
            .map_err(|_| Error::MalformedMasterkey)?;
        Ok(MasterkeyFile {
            version: LEGACY_VERSION,
            scrypt_salt: salt,
            scrypt_cost_param: scrypt_cost,
            scrypt_block_size: SCRYPT_BLOCK_SIZE,
            primary_master_key: primary,
            hmac_master_key: hmac_key,
            version_mac: version_mac(key, LEGACY_VERSION),
        })
    }

    /// Достать мастер-ключ паролем. Неверный пароль даёт [`Error::WrongPassword`].
    pub fn unlock(&self, passphrase: &str) -> Result<MasterKey> {
        if self.primary_master_key.len() != 40 || self.hmac_master_key.len() != 40 {
            return Err(Error::MalformedMasterkey);
        }
        let kek = derive_kek(
            passphrase,
            &self.scrypt_salt,
            self.scrypt_cost_param,
            self.scrypt_block_size,
        )?;
        let kw = key_wrap(&kek);
        let mut raw = Locked::<64>::new();
        kw.unwrap(&self.primary_master_key, &mut raw[..32])
            .map_err(|_| Error::WrongPassword)?;
        kw.unwrap(&self.hmac_master_key, &mut raw[32..])
            .map_err(|_| Error::WrongPassword)?;
        let key = MasterKey::from_raw(&raw);
        let expected = version_mac(&key, self.version);
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            expected.as_slice(),
            self.version_mac.as_slice(),
        )) {
            return Err(Error::MalformedMasterkey);
        }
        Ok(key)
    }

    /// Сменить пароль: тот же мастер-ключ, новая обёртка. Файлы не перешифровываются.
    pub fn change_password(&self, old: &str, new: &str) -> Result<Self> {
        let key = self.unlock(old)?;
        Self::lock_with(&key, new, self.scrypt_cost_param)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serializable")
    }

    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|_| Error::MalformedMasterkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // В тестах дешёвый scrypt, чтобы не ждать секунду на каждый вызов.
    const FAST: u32 = 1 << 4;

    #[test]
    fn lock_unlock_roundtrip() {
        let k = MasterKey::generate().unwrap();
        let f = MasterkeyFile::lock_with(&k, "пароль", FAST).unwrap();
        let f2 = MasterkeyFile::from_json(&f.to_json()).unwrap();
        let k2 = f2.unlock("пароль").unwrap();
        assert_eq!(*k.to_raw(), *k2.to_raw());
    }

    #[test]
    fn wrong_password_fails() {
        let k = MasterKey::generate().unwrap();
        let f = MasterkeyFile::lock_with(&k, "right", FAST).unwrap();
        assert!(matches!(f.unlock("wrong"), Err(Error::WrongPassword)));
    }

    #[test]
    fn nfc_normalization() {
        // «й» одним символом и «и» + комбинируемая кратка дают один и тот же ключ
        let k = MasterKey::generate().unwrap();
        let f = MasterkeyFile::lock_with(&k, "\u{0439}", FAST).unwrap();
        assert!(f.unlock("\u{0438}\u{0306}").is_ok());
    }

    #[test]
    fn change_password_keeps_key() {
        let k = MasterKey::generate().unwrap();
        let f = MasterkeyFile::lock_with(&k, "old", FAST).unwrap();
        let g = f.change_password("old", "new").unwrap();
        assert_eq!(*g.unlock("new").unwrap().to_raw(), *k.to_raw());
        assert!(g.unlock("old").is_err());
    }

    #[test]
    fn rejects_huge_scrypt() {
        let k = MasterKey::generate().unwrap();
        let mut f = MasterkeyFile::lock_with(&k, "p", FAST).unwrap();
        f.scrypt_cost_param = 1 << 30;
        assert!(matches!(f.unlock("p"), Err(Error::UnsupportedScrypt)));
    }

    #[test]
    fn wrap_with_roundtrip_and_wrong_key() {
        let k = MasterKey::generate().unwrap();
        let kek = [7u8; 32];
        let blob = wrap_with(&k, &kek).unwrap();
        assert_eq!(blob.len(), 72);
        assert_eq!(&*unwrap_with(&blob, &kek).unwrap().to_raw(), &*k.to_raw());
        assert!(matches!(
            unwrap_with(&blob, &[8u8; 32]),
            Err(Error::WrongPassword)
        ));
    }
}
