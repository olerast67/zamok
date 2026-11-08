//! Содержимое файлов.
//!
//! Файл = заголовок 68 байт + куски.
//! - Заголовок: nonce (12) ‖ AES-GCM(ключ шифрования мастер-ключа, nonce, 8 байт 0xFF ‖ ключ файла 32 байта) с тегом (16).
//! - Кусок: nonce (12) ‖ AES-GCM(ключ файла, nonce, до 32 КиБ открытого текста, AAD = номер куска u64 BE ‖ nonce заголовка) ‖ тег (16).
//!   Номер в AAD не даёт переставлять куски, nonce заголовка не даёт переносить куски между файлами.

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use zeroize::Zeroizing;

use crate::{random_bytes, Error, MasterKey, Result};

pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;
pub const HEADER_LEN: usize = NONCE_LEN + 8 + 32 + TAG_LEN; // 68
pub const PAYLOAD_LEN: usize = 32 * 1024;
pub const CHUNK_LEN: usize = NONCE_LEN + PAYLOAD_LEN + TAG_LEN;

/// Расшифрованный заголовок файла.
pub struct FileHeader {
    pub nonce: [u8; NONCE_LEN],
    content_key: Zeroizing<[u8; 32]>,
}

impl FileHeader {
    pub fn generate() -> Result<Self> {
        let mut nonce = [0u8; NONCE_LEN];
        random_bytes(&mut nonce)?;
        let mut content_key = Zeroizing::new([0u8; 32]);
        random_bytes(content_key.as_mut())?;
        Ok(FileHeader { nonce, content_key })
    }

    pub fn encrypt(&self, key: &MasterKey) -> Result<[u8; HEADER_LEN]> {
        let mut payload = Zeroizing::new([0xFFu8; 40]);
        payload[8..].copy_from_slice(self.content_key.as_ref());
        let cipher = Aes256Gcm::new_from_slice(key.enc_key()).expect("32-byte key");
        let ct = cipher
            .encrypt(Nonce::from_slice(&self.nonce), payload.as_ref())
            .map_err(|_| Error::Authentication)?;
        let mut out = [0u8; HEADER_LEN];
        out[..NONCE_LEN].copy_from_slice(&self.nonce);
        out[NONCE_LEN..].copy_from_slice(&ct);
        Ok(out)
    }

    pub fn decrypt(key: &MasterKey, header: &[u8]) -> Result<Self> {
        if header.len() < HEADER_LEN {
            return Err(Error::BadLength);
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&header[..NONCE_LEN]);
        let cipher = Aes256Gcm::new_from_slice(key.enc_key()).expect("32-byte key");
        let pt = Zeroizing::new(
            cipher
                .decrypt(Nonce::from_slice(&nonce), &header[NONCE_LEN..HEADER_LEN])
                .map_err(|_| Error::Authentication)?,
        );
        let mut content_key = Zeroizing::new([0u8; 32]);
        content_key.copy_from_slice(&pt[8..40]);
        Ok(FileHeader { nonce, content_key })
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(self.content_key.as_ref()).expect("32-byte key")
    }

    fn aad(&self, chunk: u64) -> [u8; 8 + NONCE_LEN] {
        let mut aad = [0u8; 8 + NONCE_LEN];
        aad[..8].copy_from_slice(&chunk.to_be_bytes());
        aad[8..].copy_from_slice(&self.nonce);
        aad
    }

    /// Зашифровать один кусок (до 32 КиБ).
    pub fn encrypt_chunk(&self, chunk: u64, cleartext: &[u8]) -> Result<Vec<u8>> {
        if cleartext.len() > PAYLOAD_LEN {
            return Err(Error::BadLength);
        }
        let mut nonce = [0u8; NONCE_LEN];
        random_bytes(&mut nonce)?;
        let aad = self.aad(chunk);
        let ct = self
            .cipher()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: cleartext,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Authentication)?;
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Расшифровать один кусок.
    pub fn decrypt_chunk(&self, chunk: u64, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if ciphertext.len() < NONCE_LEN + TAG_LEN || ciphertext.len() > CHUNK_LEN {
            return Err(Error::BadLength);
        }
        let aad = self.aad(chunk);
        let pt = self
            .cipher()
            .decrypt(
                Nonce::from_slice(&ciphertext[..NONCE_LEN]),
                Payload {
                    msg: &ciphertext[NONCE_LEN..],
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Authentication)?;
        Ok(Zeroizing::new(pt))
    }
}

/// Размер открытого текста по размеру зашифрованного файла.
pub fn cleartext_size(ciphertext_size: u64) -> Result<u64> {
    if ciphertext_size < HEADER_LEN as u64 {
        return Err(Error::BadLength);
    }
    let body = ciphertext_size - HEADER_LEN as u64;
    let full = body / CHUNK_LEN as u64;
    let rest = body % CHUNK_LEN as u64;
    if rest != 0 && rest < (NONCE_LEN + TAG_LEN) as u64 {
        return Err(Error::BadLength);
    }
    let rest_pt = if rest == 0 {
        0
    } else {
        rest - (NONCE_LEN + TAG_LEN) as u64
    };
    Ok(full * PAYLOAD_LEN as u64 + rest_pt)
}

/// Зашифровать файл целиком в памяти.
pub fn encrypt_file(key: &MasterKey, cleartext: &[u8]) -> Result<Vec<u8>> {
    let header = FileHeader::generate()?;
    let mut out = header.encrypt(key)?.to_vec();
    for (i, chunk) in cleartext.chunks(PAYLOAD_LEN).enumerate() {
        out.extend_from_slice(&header.encrypt_chunk(i as u64, chunk)?);
    }
    Ok(out)
}

/// Расшифровать файл целиком в памяти.
pub fn decrypt_file(key: &MasterKey, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let header = FileHeader::decrypt(key, ciphertext)?;
    let total = cleartext_size(ciphertext.len() as u64)? as usize;
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    for (i, chunk) in ciphertext[HEADER_LEN..].chunks(CHUNK_LEN).enumerate() {
        out.extend_from_slice(&header.decrypt_chunk(i as u64, chunk)?);
    }
    Ok(out)
}

/// Прочитать сколько получится до заполнения `buf` или конца потока.
fn read_full(input: &mut impl std::io::Read, buf: &mut [u8]) -> Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match input.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(got)
}

/// Длина и отпечаток SHA-256 открытого текста.
pub type Digest = (u64, [u8; 32]);

/// Зашифровать поток кусками по 32 КиБ, не держа файл в памяти. Результат тот же, что у
/// [`encrypt_file`]. Возвращает длину и SHA-256 прочитанного открытого текста.
pub fn encrypt_stream(
    key: &MasterKey,
    input: &mut impl std::io::Read,
    output: &mut impl std::io::Write,
) -> Result<Digest> {
    use sha2::{Digest as _, Sha256};
    let header = FileHeader::generate()?;
    output.write_all(&header.encrypt(key)?)?;
    let mut buf = Zeroizing::new(vec![0u8; PAYLOAD_LEN]);
    let mut hash = Sha256::new();
    let mut total = 0u64;
    for i in 0u64.. {
        let n = read_full(input, &mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        output.write_all(&header.encrypt_chunk(i, &buf[..n])?)?;
        total += n as u64;
        if n < PAYLOAD_LEN {
            break;
        }
    }
    Ok((total, hash.finalize().into()))
}

/// Расшифровать поток и посчитать длину и SHA-256 открытого текста, ничего не сохраняя.
/// Для проверки зашифрованной копии против оригинала.
pub fn decrypt_stream_digest(key: &MasterKey, input: &mut impl std::io::Read) -> Result<Digest> {
    use sha2::{Digest as _, Sha256};
    let mut hbuf = [0u8; HEADER_LEN];
    if read_full(input, &mut hbuf)? != HEADER_LEN {
        return Err(Error::BadLength);
    }
    let header = FileHeader::decrypt(key, &hbuf)?;
    let mut buf = vec![0u8; CHUNK_LEN];
    let mut hash = Sha256::new();
    let mut total = 0u64;
    for i in 0u64.. {
        let n = read_full(input, &mut buf)?;
        if n == 0 {
            break;
        }
        let pt = header.decrypt_chunk(i, &buf[..n])?;
        hash.update(&pt[..]);
        total += pt.len() as u64;
        if n < CHUNK_LEN {
            break;
        }
    }
    Ok((total, hash.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sizes() {
        let k = MasterKey::generate().unwrap();
        for n in [
            0usize,
            1,
            PAYLOAD_LEN - 1,
            PAYLOAD_LEN,
            PAYLOAD_LEN + 1,
            3 * PAYLOAD_LEN + 7,
        ] {
            let data: Vec<u8> = (0..n).map(|i| (i * 7) as u8).collect();
            let ct = encrypt_file(&k, &data).unwrap();
            assert_eq!(cleartext_size(ct.len() as u64).unwrap(), n as u64);
            assert_eq!(&*decrypt_file(&k, &ct).unwrap(), &data);
        }
    }

    #[test]
    fn stream_matches_whole_file_functions() {
        use sha2::{Digest as _, Sha256};
        let k = MasterKey::generate().unwrap();
        for n in [0usize, 1, PAYLOAD_LEN, PAYLOAD_LEN + 1, 3 * PAYLOAD_LEN + 7] {
            let data: Vec<u8> = (0..n).map(|i| (i * 13) as u8).collect();
            let mut ct = Vec::new();
            let (len, digest) = encrypt_stream(&k, &mut &data[..], &mut ct).unwrap();
            assert_eq!(len, n as u64);
            let want: [u8; 32] = Sha256::digest(&data).into();
            assert_eq!(digest, want);
            assert_eq!(ct.len(), encrypt_file(&k, &data).unwrap().len());
            assert_eq!(&*decrypt_file(&k, &ct).unwrap(), &data);
            assert_eq!(
                decrypt_stream_digest(&k, &mut &ct[..]).unwrap(),
                (len, digest)
            );
        }
    }

    #[test]
    fn stream_digest_detects_damage_and_truncation() {
        let k = MasterKey::generate().unwrap();
        let data = vec![5u8; 2 * PAYLOAD_LEN + 10];
        let mut ct = Vec::new();
        encrypt_stream(&k, &mut &data[..], &mut ct).unwrap();
        let mut bad = ct.clone();
        bad[HEADER_LEN + 20] ^= 1;
        assert!(decrypt_stream_digest(&k, &mut &bad[..]).is_err());
        // Срезан последний кусок целиком: длина другая
        let cut = &ct[..HEADER_LEN + 2 * CHUNK_LEN];
        let ok = decrypt_stream_digest(&k, &mut &cut[..]).unwrap();
        assert_ne!(ok.0, data.len() as u64);
        assert!(decrypt_stream_digest(&k, &mut &ct[..10]).is_err());
    }

    #[test]
    fn tamper_and_swap_are_detected() {
        let k = MasterKey::generate().unwrap();
        let data = vec![1u8; 2 * PAYLOAD_LEN];
        let mut ct = encrypt_file(&k, &data).unwrap();
        // перестановка кусков
        let a = ct[HEADER_LEN..HEADER_LEN + CHUNK_LEN].to_vec();
        let b = ct[HEADER_LEN + CHUNK_LEN..].to_vec();
        let mut swapped = ct[..HEADER_LEN].to_vec();
        swapped.extend_from_slice(&b);
        swapped.extend_from_slice(&a);
        assert!(decrypt_file(&k, &swapped).is_err());
        // изменение одного байта
        let last = ct.len() - 1;
        ct[last] ^= 1;
        assert!(decrypt_file(&k, &ct).is_err());
    }
}
