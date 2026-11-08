//! Файл `vault.cryptomator`: настройки хранилища в виде JWT.
//!
//! Заголовок: `kid` = `masterkeyfile:masterkey.cryptomator`, `alg` = HS256 (бывают HS384, HS512).
//! Полезная нагрузка: `format` = 8, `shorteningThreshold` = 220, `jti`, `cipherCombo` = `SIV_GCM`.
//! Подпись HMAC ключом «ключ шифрования + ключ MAC» (64 байта).

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Sha384, Sha512};

use crate::{Error, MasterKey, Result};

pub const FORMAT: u32 = 8;
pub const CIPHER_COMBO: &str = "SIV_GCM";
pub const DEFAULT_SHORTENING_THRESHOLD: usize = 220;
pub const KEY_ID: &str = "masterkeyfile:masterkey.cryptomator";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    pub jti: String,
    pub format: u32,
    #[serde(rename = "cipherCombo")]
    pub cipher_combo: String,
    #[serde(rename = "shorteningThreshold")]
    pub shortening_threshold: usize,
}

#[derive(Serialize, Deserialize)]
struct Header {
    kid: String,
    typ: Option<String>,
    alg: String,
}

fn sign(alg: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    macro_rules! mac {
        ($h:ty) => {{
            let mut m = <Hmac<$h> as Mac>::new_from_slice(key).expect("any key length");
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    Ok(match alg {
        "HS256" => mac!(Sha256),
        "HS384" => mac!(Sha384),
        "HS512" => mac!(Sha512),
        _ => return Err(Error::BadVaultConfig),
    })
}

impl VaultConfig {
    /// Настройки нового хранилища.
    pub fn new() -> Result<Self> {
        let jti = crate::uuid_v4()?;
        Ok(VaultConfig {
            jti,
            format: FORMAT,
            cipher_combo: CIPHER_COMBO.into(),
            shortening_threshold: DEFAULT_SHORTENING_THRESHOLD,
        })
    }

    /// Подписанный JWT для записи в vault.cryptomator.
    pub fn to_jwt(&self, key: &MasterKey) -> Result<String> {
        let header = Header {
            kid: KEY_ID.into(),
            typ: Some("JWT".into()),
            alg: "HS256".into(),
        };
        let h = B64URL.encode(serde_json::to_vec(&header).expect("serializable"));
        let p = B64URL.encode(serde_json::to_vec(self).expect("serializable"));
        let signing_input = format!("{h}.{p}");
        let sig = sign("HS256", key.jwt_key().as_ref(), signing_input.as_bytes())?;
        Ok(format!("{signing_input}.{}", B64URL.encode(sig)))
    }

    /// Прочитать `kid` без проверки подписи: по нему понятно, каким способом загружать ключ.
    pub fn key_id(jwt: &str) -> Result<String> {
        let h = jwt.split('.').next().ok_or(Error::BadVaultConfig)?;
        let header: Header = serde_json::from_slice(
            &B64URL
                .decode(h.trim_end_matches('='))
                .map_err(|_| Error::BadVaultConfig)?,
        )
        .map_err(|_| Error::BadVaultConfig)?;
        Ok(header.kid)
    }

    /// Проверить подпись и прочитать настройки. Поддерживается только формат 8 с SIV_GCM.
    pub fn from_jwt(jwt: &str, key: &MasterKey) -> Result<Self> {
        let jwt = jwt.trim();
        let mut parts = jwt.split('.');
        let (Some(h), Some(p), Some(s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(Error::BadVaultConfig);
        };
        let dec = |x: &str| {
            B64URL
                .decode(x.trim_end_matches('='))
                .map_err(|_| Error::BadVaultConfig)
        };
        let header: Header = serde_json::from_slice(&dec(h)?).map_err(|_| Error::BadVaultConfig)?;
        let expected = sign(
            &header.alg,
            key.jwt_key().as_ref(),
            format!("{h}.{p}").as_bytes(),
        )?;
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            expected.as_slice(),
            dec(s)?.as_slice(),
        )) {
            return Err(Error::BadVaultConfig);
        }
        let cfg: VaultConfig =
            serde_json::from_slice(&dec(p)?).map_err(|_| Error::BadVaultConfig)?;
        if cfg.format != FORMAT || cfg.cipher_combo != CIPHER_COMBO || cfg.shortening_threshold < 36
        {
            return Err(Error::UnsupportedFormat);
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_roundtrip_and_tamper() {
        let k = MasterKey::generate().unwrap();
        let cfg = VaultConfig::new().unwrap();
        let jwt = cfg.to_jwt(&k).unwrap();
        assert_eq!(VaultConfig::key_id(&jwt).unwrap(), KEY_ID);
        let back = VaultConfig::from_jwt(&jwt, &k).unwrap();
        assert_eq!(back.jti, cfg.jti);
        let other = MasterKey::generate().unwrap();
        assert!(VaultConfig::from_jwt(&jwt, &other).is_err());
    }
}
