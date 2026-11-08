//! Зашифрованный файл с произвольным доступом: чтение и запись по любому смещению,
//! изменение размера. Внутри формат Cryptomator: заголовок 68 байт и куски
//! по 32 КиБ открытого текста, каждый со своим nonce и тегом.
//!
//! Запись в середину куска = расшифровать кусок, поменять байты, зашифровать
//! заново с новым nonce. Ключ файла лежит в заголовке и стирается при [`CipherFile::seal`].

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::path::Path;

use zamok_crypto::content::{FileHeader, CHUNK_LEN, HEADER_LEN, NONCE_LEN, PAYLOAD_LEN, TAG_LEN};
use zamok_crypto::MasterKey;

/// Ошибки работы с зашифрованным файлом.
#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("crypto: {0}")]
    Crypto(#[from] zamok_crypto::Error),
    #[error("file is sealed (vault locked)")]
    Sealed,
}

pub type Result<T> = std::result::Result<T, CipherError>;

pub struct CipherFile {
    file: File,
    header: Option<FileHeader>,
}

/// Открыть файл так, чтобы его можно было переименовывать и удалять, пока он открыт.
pub fn open_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.read(true).write(true);
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
    o.share_mode(0x1 | 0x2 | 0x4);
    o
}

fn read_exact_at(file: &File, buf: &mut [u8], mut pos: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let n = file.seek_read(&mut buf[done..], pos)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        done += n;
        pos += n as u64;
    }
    Ok(())
}

fn write_all_at(file: &File, buf: &[u8], mut pos: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let n = file.seek_write(&buf[done..], pos)?;
        done += n;
        pos += n as u64;
    }
    Ok(())
}

impl CipherFile {
    /// Создать новый пустой зашифрованный файл (только заголовок).
    pub fn create(path: &Path, key: &MasterKey) -> Result<Self> {
        let file = open_options().create_new(true).open(path)?;
        let header = FileHeader::generate()?;
        write_all_at(&file, &header.encrypt(key)?, 0)?;
        Ok(CipherFile {
            file,
            header: Some(header),
        })
    }

    /// Открыть существующий. Пустой файл (0 байт) получает новый заголовок.
    pub fn open(path: &Path, key: &MasterKey) -> Result<Self> {
        // Файл с атрибутом «только чтение» открывается без записи: запись в него вернёт ошибку доступа
        let file = match open_options().open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                let mut o = OpenOptions::new();
                o.read(true).share_mode(0x1 | 0x2 | 0x4);
                o.open(path)?
            }
            Err(e) => return Err(e.into()),
        };
        let len = file.metadata()?.len();
        let header = if len == 0 {
            let h = FileHeader::generate()?;
            write_all_at(&file, &h.encrypt(key)?, 0)?;
            h
        } else {
            let mut buf = [0u8; HEADER_LEN];
            read_exact_at(&file, &mut buf, 0)?;
            FileHeader::decrypt(key, &buf)?
        };
        Ok(CipherFile {
            file,
            header: Some(header),
        })
    }

    /// Стереть ключ файла из памяти: дальнейшие операции дают [`CipherError::Sealed`].
    pub fn seal(&mut self) {
        self.header = None;
    }

    fn header(&self) -> Result<&FileHeader> {
        self.header.as_ref().ok_or(CipherError::Sealed)
    }

    fn physical_len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// Размер открытого текста.
    pub fn len(&self) -> Result<u64> {
        let p = self.physical_len()?;
        if p == 0 {
            return Ok(0);
        }
        Ok(zamok_crypto::content::cleartext_size(p)?)
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    fn chunk_pos(chunk: u64) -> u64 {
        HEADER_LEN as u64 + chunk * CHUNK_LEN as u64
    }

    /// Расшифровать кусок целиком (пустой вектор, если куска нет).
    fn read_chunk(&self, chunk: u64, len: u64) -> Result<Vec<u8>> {
        let start = chunk * PAYLOAD_LEN as u64;
        if start >= len {
            return Ok(Vec::new());
        }
        let pt_len = (len - start).min(PAYLOAD_LEN as u64) as usize;
        let mut ct = vec![0u8; NONCE_LEN + pt_len + TAG_LEN];
        read_exact_at(&self.file, &mut ct, Self::chunk_pos(chunk))?;
        let pt = self.header()?.decrypt_chunk(chunk, &ct)?;
        Ok(pt.to_vec())
    }

    fn write_chunk(&self, chunk: u64, cleartext: &[u8]) -> Result<()> {
        let ct = self.header()?.encrypt_chunk(chunk, cleartext)?;
        write_all_at(&self.file, &ct, Self::chunk_pos(chunk))?;
        Ok(())
    }

    /// Прочитать до `buf.len()` байт с позиции `offset`. Возвращает число прочитанных байт.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.header()?;
        let len = self.len()?;
        if offset >= len || buf.is_empty() {
            return Ok(0);
        }
        let end = len.min(offset + buf.len() as u64);
        let mut pos = offset;
        while pos < end {
            let chunk = pos / PAYLOAD_LEN as u64;
            let pt = self.read_chunk(chunk, len)?;
            let in_chunk = (pos - chunk * PAYLOAD_LEN as u64) as usize;
            let take = ((end - pos) as usize).min(pt.len() - in_chunk);
            let dst = (pos - offset) as usize;
            buf[dst..dst + take].copy_from_slice(&pt[in_chunk..in_chunk + take]);
            pos += take as u64;
        }
        Ok((end - offset) as usize)
    }

    /// Записать `data` с позиции `offset`. Дыра между концом файла и `offset` заполняется нулями.
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.header()?;
        let len = self.len()?;
        if offset > len {
            self.extend_zeros(len, offset)?;
        }
        let len = self.len()?;
        let end = offset + data.len() as u64;
        let mut pos = offset;
        while pos < end {
            let chunk = pos / PAYLOAD_LEN as u64;
            let chunk_start = chunk * PAYLOAD_LEN as u64;
            let mut pt = self.read_chunk(chunk, len)?;
            let in_chunk = (pos - chunk_start) as usize;
            let take = ((end - pos) as usize).min(PAYLOAD_LEN - in_chunk);
            if pt.len() < in_chunk + take {
                pt.resize(in_chunk + take, 0);
            }
            let src = (pos - offset) as usize;
            pt[in_chunk..in_chunk + take].copy_from_slice(&data[src..src + take]);
            self.write_chunk(chunk, &pt)?;
            pos += take as u64;
        }
        Ok(())
    }

    fn extend_zeros(&mut self, from: u64, to: u64) -> Result<()> {
        let zeros = vec![0u8; PAYLOAD_LEN];
        let mut pos = from;
        while pos < to {
            // дописываем до границы куска, чтобы каждый кусок шифровался один раз
            let room = PAYLOAD_LEN as u64 - (pos % PAYLOAD_LEN as u64);
            let n = room.min(to - pos) as usize;
            self.write_at(pos, &zeros[..n])?;
            pos += n as u64;
        }
        Ok(())
    }

    /// Изменить размер открытого текста.
    pub fn set_len(&mut self, new_len: u64) -> Result<()> {
        self.header()?;
        let len = self.len()?;
        if new_len > len {
            return self.extend_zeros(len, new_len);
        }
        if new_len == len {
            return Ok(());
        }
        if new_len == 0 {
            self.file.set_len(HEADER_LEN as u64)?;
            return Ok(());
        }
        let last = (new_len - 1) / PAYLOAD_LEN as u64;
        let mut pt = self.read_chunk(last, len)?;
        let keep = (new_len - last * PAYLOAD_LEN as u64) as usize;
        pt.truncate(keep);
        self.write_chunk(last, &pt)?;
        self.file
            .set_len(Self::chunk_pos(last) + (NONCE_LEN + keep + TAG_LEN) as u64)?;
        Ok(())
    }

    pub fn sync(&self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    pub fn file(&self) -> &File {
        &self.file
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zamok_crypto::content;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("zamok-cf-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn model_check(ops: &[(u64, usize)], truncs: &[u64]) {
        let k = MasterKey::generate().unwrap();
        let p = tmp(&format!("{}", ops.len() * 31 + truncs.len()));
        let mut f = CipherFile::create(&p, &k).unwrap();
        let mut model: Vec<u8> = Vec::new();
        let mut seed = 7u8;
        for &(off, n) in ops {
            let data: Vec<u8> = (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(31).wrapping_add(17);
                    seed
                })
                .collect();
            f.write_at(off, &data).unwrap();
            if model.len() < off as usize + n {
                model.resize(off as usize + n, 0);
            }
            model[off as usize..off as usize + n].copy_from_slice(&data);
        }
        for &t in truncs {
            f.set_len(t).unwrap();
            model.resize(t as usize, 0);
        }
        assert_eq!(f.len().unwrap(), model.len() as u64);
        let mut buf = vec![0u8; model.len() + 10];
        let n = f.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf[..n], &model[..]);
        drop(f);
        // файл на диске расшифровывается целиком тем же криптоядром
        let whole = content::decrypt_file(&k, &std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(&*whole, &model[..]);
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn writes_across_chunks() {
        let p = PAYLOAD_LEN as u64;
        model_check(&[(0, 10)], &[]);
        model_check(&[(0, PAYLOAD_LEN * 2 + 5), (p - 3, 10)], &[]);
        model_check(&[(5, 3), (p * 3 + 1, 2)], &[]); // дыра заполняется нулями
        model_check(&[(0, PAYLOAD_LEN * 3)], &[p + 7, 0, 5]);
        model_check(&[(0, 100)], &[p * 2 + 3]); // увеличение размера
    }

    #[test]
    fn partial_reads() {
        let k = MasterKey::generate().unwrap();
        let p = tmp("reads");
        let mut f = CipherFile::create(&p, &k).unwrap();
        let data: Vec<u8> = (0..PAYLOAD_LEN * 2 + 100)
            .map(|i| (i % 251) as u8)
            .collect();
        f.write_at(0, &data).unwrap();
        for (off, n) in [
            (0usize, 1usize),
            (PAYLOAD_LEN - 1, 2),
            (PAYLOAD_LEN * 2 + 99, 5),
            (10, PAYLOAD_LEN * 2),
        ] {
            let mut buf = vec![0u8; n];
            let got = f.read_at(off as u64, &mut buf).unwrap();
            let want = &data[off..(off + n).min(data.len())];
            assert_eq!(&buf[..got], want);
        }
        let mut buf = [0u8; 4];
        assert_eq!(f.read_at(data.len() as u64 + 5, &mut buf).unwrap(), 0);
        drop(f);
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn sealed_file_refuses() {
        let k = MasterKey::generate().unwrap();
        let p = tmp("sealed");
        let mut f = CipherFile::create(&p, &k).unwrap();
        f.write_at(0, b"secret").unwrap();
        f.seal();
        let mut buf = [0u8; 6];
        assert!(matches!(f.read_at(0, &mut buf), Err(CipherError::Sealed)));
        assert!(f.write_at(0, b"x").is_err());
        drop(f);
        std::fs::remove_file(&p).unwrap();
    }
}
