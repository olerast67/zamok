//! Зашифрованный файл с произвольным доступом: чтение и запись по любому смещению,
//! изменение размера. Внутри формат Cryptomator: заголовок 68 байт и куски
//! по 32 КиБ открытого текста, каждый со своим nonce и тегом.
//!
//! Запись в середину куска = расшифровать кусок, поменять байты, зашифровать
//! заново с новым nonce. Ключ файла лежит в заголовке и стирается при [`CipherFile::seal`].
//!
//! Последовательное чтение большими запросами идёт с упреждением: пока ядро отдаёт программе
//! ответ на один запрос, пул потоков уже читает и расшифровывает следующий.

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use rayon::prelude::*;

use zamok_crypto::content::{FileHeader, CHUNK_LEN, HEADER_LEN, NONCE_LEN, PAYLOAD_LEN, TAG_LEN};
use zamok_crypto::MasterKey;
use zeroize::Zeroizing;

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

/// С какого числа кусков в одном запросе чтение идёт в нескольких потоках.
const PARALLEL_FROM_CHUNKS: u64 = 8;
const MAX_CRYPTO_THREADS: usize = 4;

thread_local! {
    /// Буфер шифротекста для больших чтений, свой у каждого потока WinFsp.
    static CT_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Общий пул потоков расшифровки. Создавать потоки на каждый запрос дорого: запрос в 1 МиБ
/// приходит примерно тысячу раз на гигабайт.
fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_CRYPTO_THREADS);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("zamok-decrypt-{i}"))
            .build()
            .expect("decrypt thread pool")
    })
}

pub struct CipherFile {
    file: Arc<File>,
    header: Option<Arc<FileHeader>>,
    /// Растёт при каждой записи и изменении размера: упреждение, прочитанное до этого, не годится.
    generation: u64,
    /// Где кончился прошлый большой запрос чтения: так видно, что чтение последовательное.
    last_end: AtomicU64,
    ahead: Arc<Ahead>,
}

/// Упреждающее чтение: один следующий запрос, расшифрованный заранее.
#[derive(Default)]
struct Ahead {
    slot: Mutex<Option<Prefetch>>,
    done: Condvar,
    /// Отданный буфер упреждения для следующего раза: новый буфер в 1 МиБ стоит ошибок страниц.
    spare: Mutex<Option<Zeroizing<Vec<u8>>>>,
}

struct Prefetch {
    offset: u64,
    len: usize,
    generation: u64,
    /// `None`, пока пул ещё работает.
    state: Option<Fetched>,
}

enum Fetched {
    Data(Zeroizing<Vec<u8>>),
    /// Чтение не удалось, например шифротекст меняли во время чтения. Запрос читается заново
    /// обычным путём, и настоящая ошибка, если она есть, придёт оттуда.
    Failed,
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

fn chunk_pos(chunk: u64) -> u64 {
    HEADER_LEN as u64 + chunk * CHUNK_LEN as u64
}

/// Прочитать и расшифровать открытый текст `[offset, end)` файла длины `len` в `buf`.
/// Шифротекст читается одним обращением к диску, куски расшифровываются в общем пуле потоков
/// прямо в `buf`. Запрос в 1 МиБ это 32 куска.
fn decrypt_range(
    file: &File,
    header: &FileHeader,
    offset: u64,
    end: u64,
    len: u64,
    buf: &mut [u8],
) -> Result<()> {
    let payload = PAYLOAD_LEN as u64;
    let (first, last) = (offset / payload, (end - 1) / payload);
    let last_pt = (len - last * payload).min(payload) as usize;
    let total = (last - first) as usize * CHUNK_LEN + NONCE_LEN + last_pt + TAG_LEN;
    // Буфер шифротекста живёт в потоке и не выделяется на каждый запрос: свежие страницы
    // большого буфера стоят по ошибке страницы на каждые 4 КиБ. Буфер забирается из потока на
    // время работы, а не берётся взаймы: пока поток пула ждёт свои куски в `install`, он
    // выполняет чужие задачи, и упреждение другого файла зайдёт сюда же в этом потоке. Взаймы
    // это паника «RefCell already borrowed», а паника в задаче `spawn` роняет весь процесс.
    let mut owned = CT_BUF.with_borrow_mut(std::mem::take);
    let r = (|| -> Result<()> {
        if owned.len() < total {
            owned.resize(total, 0);
        }
        let ct = &mut owned[..total];
        read_exact_at(file, ct, chunk_pos(first))?;

        // Для каждого куска: номер, шифротекст, место в ответе и смещение внутри куска
        type Part<'a> = (u64, &'a [u8], &'a mut [u8], usize);
        let mut parts: Vec<Part> = Vec::with_capacity((last - first + 1) as usize);
        let mut rest = &mut buf[..(end - offset) as usize];
        for (i, piece) in ct.chunks(CHUNK_LEN).enumerate() {
            let chunk = first + i as u64;
            let start = chunk * payload;
            let skip = offset.saturating_sub(start) as usize;
            let take = (end.min(start + payload) - start) as usize - skip;
            let (dst, tail) = std::mem::take(&mut rest).split_at_mut(take);
            rest = tail;
            parts.push((chunk, piece, dst, skip));
        }
        pool().install(|| {
            parts
                .par_iter_mut()
                .try_for_each(|(chunk, piece, dst, skip)| -> Result<()> {
                    if *skip == 0 && dst.len() == piece.len() - NONCE_LEN - TAG_LEN {
                        // Кусок нужен целиком: расшифровка сразу на место, без копий
                        header.decrypt_chunk_into(*chunk, piece, dst)?;
                    } else {
                        let pt = header.decrypt_chunk(*chunk, piece)?;
                        dst.copy_from_slice(&pt[*skip..*skip + dst.len()]);
                    }
                    Ok(())
                })
        })
    })();
    // Вернуть буфер потоку, если там пусто или лежит меньший (его взяла вложенная задача)
    CT_BUF.with_borrow_mut(|b| {
        if b.capacity() < owned.capacity() {
            *b = owned;
        }
    });
    r
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
        // Папка родителя есть почти всегда: создаётся только после отказа, без лишних обращений
        let file = match open_options().create_new(true).open(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                open_options().create_new(true).open(path)?
            }
            r => r?,
        };
        let header = FileHeader::generate()?;
        write_all_at(&file, &header.encrypt(key)?, 0)?;
        Ok(Self::new(file, header))
    }

    fn new(file: File, header: FileHeader) -> Self {
        CipherFile {
            file: Arc::new(file),
            header: Some(Arc::new(header)),
            generation: 0,
            last_end: AtomicU64::new(0),
            ahead: Arc::default(),
        }
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
        Ok(Self::new(file, header))
    }

    /// Стереть ключ файла из памяти: дальнейшие операции дают [`CipherError::Sealed`].
    /// Упреждение стирается вместе с ним; задача пула, если она ещё идёт, держит свою ссылку
    /// на заголовок до конца и свой результат уже никуда не положит.
    pub fn seal(&mut self) {
        self.header = None;
        self.changed();
        *self.ahead.spare.lock().unwrap() = None;
    }

    fn header(&self) -> Result<&FileHeader> {
        self.header.as_deref().ok_or(CipherError::Sealed)
    }

    /// Содержимое меняется: упреждение больше не годится.
    fn changed(&mut self) {
        self.generation += 1;
        *self.ahead.slot.lock().unwrap() = None;
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

    /// Расшифровать кусок целиком (пустой вектор, если куска нет).
    fn read_chunk(&self, chunk: u64, len: u64) -> Result<Vec<u8>> {
        let start = chunk * PAYLOAD_LEN as u64;
        if start >= len {
            return Ok(Vec::new());
        }
        let pt_len = (len - start).min(PAYLOAD_LEN as u64) as usize;
        let mut ct = vec![0u8; NONCE_LEN + pt_len + TAG_LEN];
        read_exact_at(&self.file, &mut ct, chunk_pos(chunk))?;
        let pt = self.header()?.decrypt_chunk(chunk, &ct)?;
        Ok(pt.to_vec())
    }

    fn write_chunk(&self, chunk: u64, cleartext: &[u8]) -> Result<()> {
        let ct = self.header()?.encrypt_chunk(chunk, cleartext)?;
        write_all_at(&self.file, &ct, chunk_pos(chunk))?;
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
        let (first, last) = (offset / PAYLOAD_LEN as u64, (end - 1) / PAYLOAD_LEN as u64);
        if last - first + 1 >= PARALLEL_FROM_CHUNKS {
            self.read_large(offset, end, len, buf)?;
            return Ok((end - offset) as usize);
        }
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

    /// Большой запрос: берётся из упреждения, если оно есть, иначе читается сразу. При
    /// последовательном чтении следующий запрос такого же размера заказывается пулу заранее.
    fn read_large(&self, offset: u64, end: u64, len: u64, buf: &mut [u8]) -> Result<()> {
        let header = self.header()?;
        let want = (end - offset) as usize;
        let sequential = offset == 0 || offset == self.last_end.load(Ordering::Relaxed);
        match self.take_ahead(offset, want) {
            Some(data) => {
                buf[..want].copy_from_slice(&data[..want]);
                *self.ahead.spare.lock().unwrap() = Some(data);
            }
            None => decrypt_range(&self.file, header, offset, end, len, buf)?,
        }
        self.last_end.store(end, Ordering::Relaxed);
        if sequential && end < len {
            self.schedule_ahead(end, buf.len().min((len - end) as usize), len);
        }
        Ok(())
    }

    /// Забрать упреждение для `[offset, offset + want)`, если оно заказано на это место и
    /// файл с тех пор не менялся. Если пул ещё работает, подождать его.
    fn take_ahead(&self, offset: u64, want: usize) -> Option<Zeroizing<Vec<u8>>> {
        let mut slot = self.ahead.slot.lock().unwrap();
        match slot.as_ref() {
            Some(p) if p.offset == offset && p.len >= want && p.generation == self.generation => {}
            _ => return None,
        }
        while slot.as_ref().is_some_and(|p| p.state.is_none()) {
            slot = self.ahead.done.wait(slot).unwrap();
        }
        match slot.take()?.state? {
            Fetched::Data(data) => Some(data),
            Fetched::Failed => None,
        }
    }

    /// Заказать пулу чтение `[start, start + n)` про запас. Заказ заменяет прежний.
    fn schedule_ahead(&self, start: u64, n: usize, len: u64) {
        let Some(header) = self.header.clone() else {
            return;
        };
        let generation = self.generation;
        *self.ahead.slot.lock().unwrap() = Some(Prefetch {
            offset: start,
            len: n,
            generation,
            state: None,
        });
        let file = Arc::clone(&self.file);
        let ahead = Arc::clone(&self.ahead);
        pool().spawn(move || {
            let spare = ahead.spare.lock().unwrap().take();
            let mut data = spare
                .filter(|d| d.len() >= n)
                .unwrap_or_else(|| Zeroizing::new(vec![0u8; n]));
            let end = start + n as u64;
            // Паника в задаче `spawn` роняет весь процесс: здесь она становится неудачным
            // упреждением, и запрос читается обычным путём
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                decrypt_range(&file, &header, start, end, len, &mut data[..n])
            }));
            let state = match read {
                Ok(Ok(())) => Fetched::Data(data),
                _ => Fetched::Failed,
            };
            drop(header);
            let mut slot = ahead.slot.lock().unwrap();
            // Пока шло чтение, заказ могли заменить или отменить записью в файл
            if let Some(p) = slot.as_mut() {
                if p.offset == start && p.generation == generation && p.state.is_none() {
                    p.state = Some(state);
                }
            }
            drop(slot);
            ahead.done.notify_all();
        });
    }

    /// Записать `data` с позиции `offset`. Дыра между концом файла и `offset` заполняется нулями.
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.header()?;
        self.changed();
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
            let in_chunk = (pos - chunk_start) as usize;
            let take = ((end - pos) as usize).min(PAYLOAD_LEN - in_chunk);
            // Кусок перезаписан целиком: старое содержимое читать незачем
            let whole = in_chunk == 0 && (take == PAYLOAD_LEN || chunk_start + take as u64 >= len);
            let mut pt = if whole {
                Vec::new()
            } else {
                self.read_chunk(chunk, len)?
            };
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
        self.changed();
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
            .set_len(chunk_pos(last) + (NONCE_LEN + keep + TAG_LEN) as u64)?;
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
    fn large_reads_match_and_reject_damage() {
        let k = MasterKey::generate().unwrap();
        let p = tmp("parallel");
        let mut f = CipherFile::create(&p, &k).unwrap();
        let data: Vec<u8> = (0..PAYLOAD_LEN * 20 + 77)
            .map(|i| (i % 253) as u8)
            .collect();
        f.write_at(0, &data).unwrap();
        for (off, n) in [
            (0, data.len()),
            (10, PAYLOAD_LEN * 19),
            (PAYLOAD_LEN - 1, PAYLOAD_LEN * 8 + 2),
            (PAYLOAD_LEN * 3, data.len()),
        ] {
            let mut buf = vec![0u8; n];
            let got = f.read_at(off as u64, &mut buf).unwrap();
            let want = &data[off..(off + n).min(data.len())];
            assert_eq!(got, want.len());
            assert_eq!(&buf[..got], want);
        }
        // Испорченный кусок в середине: чтение возвращает ошибку
        let mut raw = std::fs::read(&p).unwrap();
        raw[HEADER_LEN + CHUNK_LEN * 5 + 40] ^= 1;
        drop(f);
        std::fs::write(&p, &raw).unwrap();
        let f = CipherFile::open(&p, &k).unwrap();
        let mut buf = vec![0u8; PAYLOAD_LEN * 10];
        assert!(f.read_at(0, &mut buf).is_err());
        drop(f);
        std::fs::remove_file(&p).unwrap();
    }

    /// Последовательное чтение с упреждением: данные те же, а запись и изменение размера между
    /// запросами не дают отдать устаревшее упреждение.
    #[test]
    fn read_ahead_follows_writes() {
        let k = MasterKey::generate().unwrap();
        let p = tmp("ahead");
        let mut f = CipherFile::create(&p, &k).unwrap();
        let req = PAYLOAD_LEN * 10;
        let mut model: Vec<u8> = (0..req * 6 + 333).map(|i| (i % 249) as u8).collect();
        f.write_at(0, &model).unwrap();
        let mut buf = vec![0u8; req];
        let mut off = 0usize;
        let mut step = 0;
        while off < model.len() {
            let got = f.read_at(off as u64, &mut buf).unwrap();
            let want = &model[off..(off + req).min(model.len())];
            assert_eq!(&buf[..got], want, "offset {off}");
            off += got;
            step += 1;
            // Упреждение уже заказано на следующий запрос: меняем именно эти байты
            if step == 2 {
                let patch = vec![0xAB; 100];
                f.write_at(off as u64 + 5, &patch).unwrap();
                model[off + 5..off + 105].copy_from_slice(&patch);
            }
            if step == 4 {
                let cut = off + req / 2;
                f.set_len(cut as u64).unwrap();
                model.truncate(cut);
            }
        }
        assert_eq!(off, model.len());
        // Чтение не с начала и не подряд: упреждение не используется, ответ верный
        let got = f.read_at(7, &mut buf).unwrap();
        assert_eq!(&buf[..got], &model[7..7 + got]);
        f.seal();
        assert!(matches!(f.read_at(0, &mut buf), Err(CipherError::Sealed)));
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

    /// Много файлов читаются большими запросами одновременно: задачи упреждения разных файлов
    /// встречаются в одном потоке пула (пока поток ждёт свои куски, он берёт чужую задачу).
    /// Паника в задаче `spawn` роняет весь процесс, поэтому этот тест либо проходит, либо
    /// процесс тестов падает с 0xc0000409.
    #[test]
    fn parallel_large_reads_of_many_files_do_not_crash() {
        let k = MasterKey::generate().unwrap();
        let size = 6 * 1024 * 1024;
        let data: Vec<u8> = (0..size).map(|i| (i * 7 % 251) as u8).collect();
        let paths: Vec<_> = (0..8).map(|i| tmp(&format!("par{i}"))).collect();
        for p in &paths {
            let mut f = CipherFile::create(p, &k).unwrap();
            f.write_at(0, &data).unwrap();
        }
        std::thread::scope(|s| {
            for p in &paths {
                let (k, data) = (&k, &data);
                s.spawn(move || {
                    for _ in 0..6 {
                        let f = CipherFile::open(p, k).unwrap();
                        let mut buf = vec![0u8; 1024 * 1024];
                        let mut off = 0;
                        while off < data.len() {
                            let n = f.read_at(off as u64, &mut buf).unwrap();
                            assert_eq!(&buf[..n], &data[off..off + n]);
                            off += n;
                        }
                    }
                });
            }
        });
        for p in &paths {
            std::fs::remove_file(p).unwrap();
        }
    }
}
