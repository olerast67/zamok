//! Файловая система WinFsp поверх хранилища Cryptomator формата 8.
//!
//! Устройство:
//! - путь `\a\b\c` разбирается по частям: у каждого каталога есть ID, имя части
//!   шифруется с ID родителя, на диске ищется узел `<имя>.c9r` или `<хеш>.c9s`;
//!   если точного совпадения нет, каталог перечисляется и имя ищется без учёта
//!   регистра (Windows ждёт этого от файловой системы);
//! - содержимое читается и пишется через [`CipherFile`] кусками по 32 КиБ;
//! - состояние «заперто»: мастер-ключ и ключи всех открытых файлов стёрты, в корне
//!   видны только `Открыть <Имя>.zamok` и `desktop.ini`, а файлы, открытые до
//!   запирания, получают отказ навсегда (номер поколения, проверено в прототипе P3).
//!
//! В журнал (tracing) имена и пути не пишутся.

use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

use windows::Win32::Foundation::{
    STATUS_ACCESS_DENIED, STATUS_DIRECTORY_NOT_EMPTY, STATUS_FILE_IS_A_DIRECTORY,
    STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_INVALID,
    STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND,
};
use winfsp::constants::FspCleanupFlags::FspCleanupDelete;
use winfsp::filesystem::{
    DirBuffer, DirInfo, DirMarker, FileInfo, FileSecurity, FileSystemContext,
    ModificationDescriptor, OpenFileInfo, VolumeInfo, WideNameInfo,
};
use winfsp::{FspError, U16CStr};
use zamok_crypto::masterkey::MasterkeyFile;
use zamok_crypto::names::{self, C9R, C9S, CONTENTS_FILE, DIRID_BACKUP_FILE, DIR_FILE, NAME_FILE};
use zamok_crypto::vault::VaultConfig;
use zamok_crypto::MasterKey;

use crate::cipherfile::{CipherError, CipherFile};
use crate::security;

type FsResult<T> = winfsp::Result<T>;

const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const FILE_ATTRIBUTE_NOT_CONTENT_INDEXED: u32 = 0x2000;
const KEEP_ATTRIBUTES: u32 = FILE_ATTRIBUTE_READONLY
    | FILE_ATTRIBUTE_HIDDEN
    | FILE_ATTRIBUTE_SYSTEM
    | FILE_ATTRIBUTE_ARCHIVE;
const FILE_DIRECTORY_FILE: u32 = 0x1;

fn cerr(e: CipherError) -> FspError {
    match e {
        CipherError::Io(e) => e.into(),
        CipherError::Sealed => STATUS_ACCESS_DENIED.into(),
        CipherError::Crypto(_) => STATUS_ACCESS_DENIED.into(),
    }
}

fn ioerr(e: std::io::Error) -> FspError {
    e.into()
}

/// Узел на диске для расшифрованного пути.
#[derive(Clone, Debug)]
struct Resolved {
    /// Путь в открытом виде с настоящим регистром частей (`\Папка\файл.txt`).
    clear: String,
    /// ID каталога-родителя (для корня пустой).
    parent_id: String,
    /// Узел: файл `<имя>.c9r`, каталог `<имя>.c9r\` или `<хеш>.c9s\`. Для корня `None`.
    node: Option<PathBuf>,
    kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Dir { id: String },
    File { content: PathBuf },
}

/// Виртуальные файлы запертой папки.
#[derive(Clone, Debug, PartialEq)]
enum Virtual {
    Root,
    OpenItem,
    DesktopIni,
}

/// Почему папка не открылась.
#[derive(Debug, thiserror::Error)]
pub enum UnlockError {
    #[error(transparent)]
    Crypto(#[from] zamok_crypto::Error),
    /// Папку заперли, пока проверялся пароль: открытие отменено.
    #[error("unlock cancelled: the folder was locked meanwhile")]
    Cancelled,
}

impl From<std::io::Error> for UnlockError {
    fn from(e: std::io::Error) -> Self {
        UnlockError::Crypto(e.into())
    }
}

/// Значок запертой папки для `desktop.ini` (путь к .ico). Не задан: замок из imageres.dll.
static FOLDER_ICON: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Задать значок запертой папки один раз при старте программы.
pub fn set_folder_icon(path: &str) {
    let _ = FOLDER_ICON.set(path.to_string());
}

pub struct ZFile {
    path: Mutex<String>,
    kind: Mutex<Option<Kind>>,
    virt: Option<Virtual>,
    /// Открывается при первом чтении или записи: получение сведений о файле заголовок не трогает.
    cipher: Mutex<Option<Arc<Mutex<CipherFile>>>>,
    dir_buffer: DirBuffer,
    generation: u64,
    delete_on_close: AtomicBool,
}

type Listing = HashMap<String, (String, PathBuf)>;

struct Unlocked {
    key: MasterKey,
    config: VaultConfig,
}

/// Состояние, общее для файловой системы и программы, которая её запирает и открывает.
pub struct Control {
    root: PathBuf,
    state: RwLock<Option<Unlocked>>,
    generation: AtomicU64,
    /// Открытые зашифрованные файлы по пути содержимого на диске: один объект на файл,
    /// чтобы все дескрипторы видели один заголовок.
    open_files: Mutex<HashMap<PathBuf, Weak<Mutex<CipherFile>>>>,
    /// ID каталогов по пути в нижнем регистре.
    dir_cache: Mutex<HashMap<String, (String, String)>>,
    /// Расшифрованное содержимое каталогов по ID: имя в нижнем регистре -> (имя, узел).
    /// Сбрасывается при каждом изменении в каталоге.
    listings: Mutex<HashMap<String, Listing>>,
    /// Растёт при каждом изменении каталогов: кэш, собранный во время изменения, не сохраняется.
    listing_epoch: AtomicU64,
    /// Пути, о которых ядро WinFsp могло закэшировать сведения, пока папка открыта.
    seen: Mutex<std::collections::HashSet<String>>,
    /// Пути, по которым при запирании нужно сбросить кэш ядра (уведомлением).
    pending_invalidate: Mutex<Vec<String>>,
    /// Какой-то путь не удалось сбросить уведомлением (переполнение или слишком длинный путь):
    /// перед ответом «заперто» нужно дождаться, пока кэш ядра истечёт сам.
    fallback_wait: AtomicBool,
    /// Отсчёт для [`Control::idle_for`].
    started: std::time::Instant,
    /// Когда в папке последний раз читали, писали, создавали, переименовывали или смотрели
    /// содержимое каталога (миллисекунды от `started`).
    last_activity_ms: AtomicU64,
}

pub struct ZamokFs {
    root: PathBuf,
    display_name: String,
    ctl: Arc<Control>,
    security: Vec<u8>,
}

fn split(path: &str) -> Vec<&str> {
    path.split('\\').filter(|p| !p.is_empty()).collect()
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() || parent == "\\" {
        format!("\\{name}")
    } else {
        format!("{parent}\\{name}")
    }
}

fn parent_and_name(path: &str) -> FsResult<(String, String)> {
    let parts = split(path);
    let Some((name, parents)) = parts.split_last() else {
        return Err(STATUS_OBJECT_NAME_INVALID.into());
    };
    Ok((format!("\\{}", parents.join("\\")), name.to_string()))
}

fn valid_name(name: &str) -> bool {
    !(name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|c| {
            matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 32
        }))
}

impl Control {
    /// Открыть паролем. Неверный пароль оставляет папку запертой.
    pub fn unlock_with_password(&self, password: &str) -> Result<(), UnlockError> {
        self.unlock_with_password_if(password, &|| true)
    }

    /// Открыть паролем, если к моменту установки ключа открытие всё ещё нужно.
    ///
    /// Проверка пароля (scrypt) занимает заметное время. Если за это время папку заперли
    /// ([`Control::lock`], например по Win+L) или `still_wanted` вернул `false`, ключ
    /// выбрасывается и возвращается [`UnlockError::Cancelled`]. `still_wanted` вызывается
    /// под тем же замком, под которым ставится ключ.
    pub fn unlock_with_password_if(
        &self,
        password: &str,
        still_wanted: &dyn Fn() -> bool,
    ) -> Result<(), UnlockError> {
        let started = self.generation.load(Ordering::SeqCst);
        let mk = MasterkeyFile::from_json(&fs::read_to_string(
            self.root.join("masterkey.cryptomator"),
        )?)?;
        let key = mk.unlock(password)?;
        self.commit_unlock(key, started, still_wanted)
    }

    /// Открыть ключом (ключ восстановления).
    pub fn unlock_with_key(&self, key: MasterKey) -> Result<(), UnlockError> {
        self.unlock_with_key_if(key, &|| true)
    }

    /// Открыть ключом с той же отменой, что у [`Control::unlock_with_password_if`].
    pub fn unlock_with_key_if(
        &self,
        key: MasterKey,
        still_wanted: &dyn Fn() -> bool,
    ) -> Result<(), UnlockError> {
        let started = self.generation.load(Ordering::SeqCst);
        self.commit_unlock(key, started, still_wanted)
    }

    fn commit_unlock(
        &self,
        key: MasterKey,
        started: u64,
        still_wanted: &dyn Fn() -> bool,
    ) -> Result<(), UnlockError> {
        let jwt = fs::read_to_string(self.root.join("vault.cryptomator"))?;
        let config = VaultConfig::from_jwt(&jwt, &key)?;
        let mut st = self.state.write().unwrap();
        // lock меняет поколение под этим же замком
        if self.generation.load(Ordering::SeqCst) != started || !still_wanted() {
            return Err(UnlockError::Cancelled); // key стирается при удалении
        }
        *st = Some(Unlocked { key, config });
        drop(st);
        self.touch();
        Ok(())
    }

    /// Запереть: стереть мастер-ключ и ключи всех открытых файлов. Поколение меняется
    /// всегда, и у запертой папки тоже: так отменяются открытия, которые сейчас проверяют
    /// пароль. Проверка состояния и запирание идут под одним замком.
    /// Возвращает `true`, если папка была открыта (тогда нужен [`crate::mount::settle_lock`]).
    pub fn lock(&self) -> bool {
        let mut st = self.state.write().unwrap();
        self.generation.fetch_add(1, Ordering::SeqCst);
        if st.is_none() {
            return false;
        }
        for weak in self.open_files.lock().unwrap().drain().map(|(_, w)| w) {
            if let Some(f) = weak.upgrade() {
                f.lock().unwrap().seal();
            }
        }
        self.dir_cache.lock().unwrap().clear();
        self.listings.lock().unwrap().clear();
        let mut seen: Vec<String> = self.seen.lock().unwrap().drain().collect();
        seen.push("\\".to_string());
        self.pending_invalidate.lock().unwrap().extend(seen);
        *st = None; // MasterKey стирается при удалении (ZeroizeOnDrop)
        true
    }

    pub fn is_locked(&self) -> bool {
        self.state.read().unwrap().is_none()
    }

    /// Нужно ли перед ответом «заперто» подождать, пока кэш ядра истечёт сам. Сбрасывает флаг.
    pub fn take_fallback_wait(&self) -> bool {
        self.fallback_wait.swap(false, Ordering::SeqCst)
    }

    /// Ещё не все уведомления о запирании отправлены ядру.
    pub fn invalidation_pending(&self) -> bool {
        !self.pending_invalidate.lock().unwrap().is_empty()
    }

    /// Отметить работу с папкой (для автозапирания по бездействию).
    pub fn touch(&self) {
        let ms = self.started.elapsed().as_millis() as u64;
        self.last_activity_ms.fetch_max(ms, Ordering::Relaxed);
    }

    /// Сколько времени с папкой ничего не делали (с момента открытия или последнего обращения).
    pub fn idle_for(&self) -> std::time::Duration {
        let now = self.started.elapsed().as_millis() as u64;
        let last = self.last_activity_ms.load(Ordering::Relaxed);
        std::time::Duration::from_millis(now.saturating_sub(last))
    }

    fn remember(&self, path: &str) {
        let mut seen = self.seen.lock().unwrap();
        if seen.len() < 500_000 {
            seen.insert(path.to_string());
        } else {
            self.fallback_wait.store(true, Ordering::SeqCst);
        }
    }
}

impl ZamokFs {
    /// Файловая система для хранилища `root`, пока запертая.
    pub fn new(root: &Path, display_name: &str) -> anyhow::Result<Self> {
        let ctl = Arc::new(Control {
            root: root.to_path_buf(),
            state: RwLock::new(None),
            generation: AtomicU64::new(1),
            open_files: Mutex::new(HashMap::new()),
            dir_cache: Mutex::new(HashMap::new()),
            listings: Mutex::new(HashMap::new()),
            listing_epoch: AtomicU64::new(0),
            seen: Mutex::new(std::collections::HashSet::new()),
            pending_invalidate: Mutex::new(Vec::new()),
            fallback_wait: AtomicBool::new(false),
            started: std::time::Instant::now(),
            last_activity_ms: AtomicU64::new(0),
        });
        Ok(ZamokFs {
            root: root.to_path_buf(),
            display_name: display_name.to_string(),
            ctl,
            security: security::owner_only_descriptor()?,
        })
    }

    /// Управление состоянием (запереть, открыть) из другого потока.
    pub fn control(&self) -> Arc<Control> {
        self.ctl.clone()
    }

    fn is_locked(&self) -> bool {
        self.ctl.is_locked()
    }

    fn open_item_name(&self) -> String {
        format!("Открыть {}.zamok", self.display_name)
    }

    fn with_key<T>(&self, f: impl FnOnce(&Unlocked) -> FsResult<T>) -> FsResult<T> {
        let st = self.ctl.state.read().unwrap();
        match st.as_ref() {
            Some(u) => f(u),
            None => Err(STATUS_ACCESS_DENIED.into()),
        }
    }

    fn dir_physical(&self, u: &Unlocked, dir_id: &str) -> PathBuf {
        self.root.join(names::dir_path(&u.key, dir_id))
    }

    /// Узел имени `name` в каталоге `dir_id`: точное совпадение или без учёта регистра.
    fn find_child(
        &self,
        u: &Unlocked,
        dir_id: &str,
        name: &str,
    ) -> FsResult<Option<(String, PathBuf)>> {
        let lower = name.to_lowercase();
        self.with_listing(u, dir_id, |l| l.get(&lower).cloned())
    }

    /// Выполнить `f` над содержимым каталога (загрузить с диска, если его нет в кэше).
    fn with_listing<T>(
        &self,
        u: &Unlocked,
        dir_id: &str,
        f: impl FnOnce(&mut Listing) -> T,
    ) -> FsResult<T> {
        {
            let mut all = self.ctl.listings.lock().unwrap();
            if let Some(l) = all.get_mut(dir_id) {
                return Ok(f(l));
            }
        }
        let epoch = self.ctl.listing_epoch.load(Ordering::SeqCst);
        let mut map = Listing::new();
        for (clear, node) in self.list_physical(u, dir_id)? {
            map.insert(clear.to_lowercase(), (clear, node));
        }
        let mut all = self.ctl.listings.lock().unwrap();
        if self.ctl.listing_epoch.load(Ordering::SeqCst) != epoch {
            // Пока читали диск, каталоги менялись: ответ годится один раз, в кэш не кладём
            return Ok(f(&mut map));
        }
        let l = all.entry(dir_id.to_string()).or_insert(map);
        Ok(f(l))
    }

    /// Обновить кэш каталога, только если он уже загружен.
    fn listing_update(&self, dir_id: &str, f: impl FnOnce(&mut Listing)) {
        let mut all = self.ctl.listings.lock().unwrap();
        self.ctl.listing_epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(l) = all.get_mut(dir_id) {
            f(l);
        }
    }

    fn invalidate(&self, dir_id: &str) {
        let mut all = self.ctl.listings.lock().unwrap();
        self.ctl.listing_epoch.fetch_add(1, Ordering::SeqCst);
        all.remove(dir_id);
    }

    /// Все дети каталога: (имя в открытом виде, узел на диске).
    fn list_physical(&self, u: &Unlocked, dir_id: &str) -> FsResult<Vec<(String, PathBuf)>> {
        let dir = self.dir_physical(u, dir_id);
        let mut out = Vec::new();
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(ioerr(e)),
        };
        for entry in rd.flatten() {
            let fname = entry.file_name().to_string_lossy().into_owned();
            if fname == DIRID_BACKUP_FILE {
                continue;
            }
            let enc = if let Some(stem) = fname.strip_suffix(C9R) {
                stem.to_string()
            } else if fname.ends_with(C9S) {
                match fs::read_to_string(entry.path().join(NAME_FILE)) {
                    Ok(full) => match full.trim().strip_suffix(C9R) {
                        Some(s) => s.to_string(),
                        None => continue,
                    },
                    Err(_) => continue,
                }
            } else {
                continue;
            };
            if let Ok(clear) = names::decrypt_name(&u.key, &enc, dir_id) {
                if valid_name(&clear) {
                    out.push((clear, entry.path()));
                }
            }
        }
        Ok(out)
    }

    fn kind_of(node: &Path) -> FsResult<Kind> {
        if node.is_dir() {
            if let Ok(id) = fs::read_to_string(node.join(DIR_FILE)) {
                return Ok(Kind::Dir {
                    id: id.trim().to_string(),
                });
            }
            let content = node.join(CONTENTS_FILE);
            if content.is_file() {
                return Ok(Kind::File { content });
            }
            return Err(STATUS_OBJECT_NAME_NOT_FOUND.into());
        }
        Ok(Kind::File {
            content: node.to_path_buf(),
        })
    }

    fn resolve(&self, u: &Unlocked, path: &str) -> FsResult<Resolved> {
        let parts = split(path);
        if parts.is_empty() {
            return Ok(Resolved {
                clear: "\\".into(),
                parent_id: String::new(),
                node: None,
                kind: Kind::Dir { id: String::new() },
            });
        }
        let mut dir_id = String::new();
        let mut clear = String::new();
        for (i, part) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            let key = format!("{}\\{}", clear.to_lowercase(), part.to_lowercase());
            if !last {
                if let Some((id, real)) = self.ctl.dir_cache.lock().unwrap().get(&key).cloned() {
                    dir_id = id;
                    clear = real;
                    continue;
                }
            }
            let Some((real_name, node)) = self.find_child(u, &dir_id, part)? else {
                return Err(if last {
                    STATUS_OBJECT_NAME_NOT_FOUND.into()
                } else {
                    STATUS_OBJECT_PATH_NOT_FOUND.into()
                });
            };
            let kind = Self::kind_of(&node)?;
            let parent_id = dir_id.clone();
            clear = join(&clear, &real_name);
            if last {
                return Ok(Resolved {
                    clear,
                    parent_id,
                    node: Some(node),
                    kind,
                });
            }
            match kind {
                Kind::Dir { id } => {
                    self.ctl
                        .dir_cache
                        .lock()
                        .unwrap()
                        .insert(key, (id.clone(), clear.clone()));
                    dir_id = id;
                }
                Kind::File { .. } => return Err(STATUS_OBJECT_PATH_NOT_FOUND.into()),
            }
        }
        unreachable!()
    }

    fn virtual_of(&self, path: &str) -> Option<Virtual> {
        let parts = split(path);
        match parts.as_slice() {
            [] => Some(Virtual::Root),
            [n] if n.eq_ignore_ascii_case("desktop.ini") => Some(Virtual::DesktopIni),
            [n] if n.to_lowercase() == self.open_item_name().to_lowercase() => {
                Some(Virtual::OpenItem)
            }
            _ => None,
        }
    }

    fn virtual_content(&self, v: &Virtual) -> Vec<u8> {
        match v {
            Virtual::Root => Vec::new(),
            Virtual::OpenItem => {
                "Zamok: эта папка заперта. Дважды щёлкните по этому файлу, чтобы ввести пароль.\r\n"
                    .as_bytes()
                    .to_vec()
            }
            Virtual::DesktopIni => {
                // UTF-16LE с BOM: так desktop.ini понимает Проводник
                let icon = FOLDER_ICON
                    .get()
                    .map(|p| format!("{p},0"))
                    .unwrap_or_else(|| r"%SystemRoot%\System32\imageres.dll,54".to_string());
                let text = format!("[.ShellClassInfo]\r\nIconResource={icon}\r\nInfoTip=Zamok\r\n");
                let mut v = vec![0xFF, 0xFE];
                for u in text.encode_utf16() {
                    v.extend_from_slice(&u.to_le_bytes());
                }
                v
            }
        }
    }

    fn root_meta(&self) -> Option<fs::Metadata> {
        fs::metadata(&self.root).ok()
    }

    fn fill_virtual(&self, v: &Virtual, info: &mut FileInfo) {
        let m = self.root_meta();
        let t = m.as_ref().map(|m| m.last_write_time()).unwrap_or(0);
        info.file_attributes = match v {
            Virtual::Root => {
                FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_READONLY
            }
            Virtual::DesktopIni => FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM,
            Virtual::OpenItem => FILE_ATTRIBUTE_READONLY,
        } | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
        info.file_size = self.virtual_content(v).len() as u64;
        info.allocation_size = info.file_size.div_ceil(4096) * 4096;
        info.creation_time = m.as_ref().map(|m| m.creation_time()).unwrap_or(0);
        info.last_access_time = t;
        info.last_write_time = t;
        info.change_time = t;
        info.index_number = 0;
        info.hard_links = 0;
        info.reparse_tag = 0;
        info.ea_size = 0;
    }

    fn fill_info(&self, r: &Resolved, info: &mut FileInfo) -> FsResult<()> {
        let (meta_path, is_dir) = match (&r.kind, &r.node) {
            (Kind::Dir { .. }, None) => (self.root.clone(), true),
            (Kind::Dir { .. }, Some(n)) => (n.clone(), true),
            (Kind::File { content }, _) => (content.clone(), false),
        };
        let m = fs::metadata(&meta_path).map_err(ioerr)?;
        let mut attrs = m.file_attributes() & KEEP_ATTRIBUTES;
        if is_dir {
            attrs |= FILE_ATTRIBUTE_DIRECTORY;
            attrs &= !FILE_ATTRIBUTE_ARCHIVE;
        } else if attrs == 0 {
            attrs = FILE_ATTRIBUTE_NORMAL;
        }
        info.file_attributes = attrs | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
        info.file_size = if is_dir || m.len() == 0 {
            0
        } else {
            zamok_crypto::content::cleartext_size(m.len()).unwrap_or(0)
        };
        info.allocation_size = info.file_size.div_ceil(4096) * 4096;
        info.creation_time = m.creation_time();
        info.last_access_time = m.last_access_time();
        info.last_write_time = m.last_write_time();
        info.change_time = m.last_write_time();
        info.index_number = 0;
        info.hard_links = 0;
        info.reparse_tag = 0;
        info.ea_size = 0;
        Ok(())
    }

    fn cipher_for(
        &self,
        u: &Unlocked,
        content: &Path,
        create: bool,
    ) -> FsResult<Arc<Mutex<CipherFile>>> {
        let mut table = self.ctl.open_files.lock().unwrap();
        if let Some(f) = table.get(content).and_then(Weak::upgrade) {
            return Ok(f);
        }
        let cf = if create {
            CipherFile::create(content, &u.key)
        } else {
            CipherFile::open(content, &u.key)
        }
        .map_err(cerr)?;
        let arc = Arc::new(Mutex::new(cf));
        table.insert(content.to_path_buf(), Arc::downgrade(&arc));
        Ok(arc)
    }

    fn new_context(
        &self,
        path: String,
        kind: Option<Kind>,
        virt: Option<Virtual>,
        cipher: Option<Arc<Mutex<CipherFile>>>,
    ) -> ZFile {
        ZFile {
            path: Mutex::new(path),
            kind: Mutex::new(kind),
            virt,
            cipher: Mutex::new(cipher),
            dir_buffer: DirBuffer::new(),
            generation: self.ctl.generation.load(Ordering::SeqCst),
            delete_on_close: AtomicBool::new(false),
        }
    }

    /// Зашифрованный файл контекста: открыть при первом обращении.
    fn cipher_of(&self, ctx: &ZFile) -> FsResult<Arc<Mutex<CipherFile>>> {
        let mut slot = ctx.cipher.lock().unwrap();
        if let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
        let Some(Kind::File { content }) = ctx.kind.lock().unwrap().clone() else {
            return Err(STATUS_FILE_IS_A_DIRECTORY.into());
        };
        let c = self.with_key(|u| self.cipher_for(u, &content, false))?;
        *slot = Some(c.clone());
        Ok(c)
    }

    /// Файл, открытый до последнего запирания, больше не работает.
    fn check(&self, ctx: &ZFile) -> FsResult<()> {
        if ctx.virt.is_some() {
            return Ok(());
        }
        if self.is_locked() || ctx.generation != self.ctl.generation.load(Ordering::SeqCst) {
            return Err(STATUS_ACCESS_DENIED.into());
        }
        Ok(())
    }

    fn info_for_ctx(&self, ctx: &ZFile, info: &mut FileInfo) -> FsResult<()> {
        if let Some(v) = &ctx.virt {
            if !self.is_locked() && *v == Virtual::Root {
                return self.with_key(|u| {
                    let r = self.resolve(u, "\\")?;
                    self.fill_info(&r, info)
                });
            }
            self.fill_virtual(v, info);
            return Ok(());
        }
        let path = ctx.path.lock().unwrap().clone();
        self.with_key(|u| {
            let r = self.resolve(u, &path)?;
            self.fill_info(&r, info)
        })
    }

    fn security_copy(&self, buf: Option<&mut [c_void]>) -> u64 {
        let len = self.security.len();
        if let Some(b) = buf {
            if b.len() >= len {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.security.as_ptr(),
                        b.as_mut_ptr() as *mut u8,
                        len,
                    )
                };
            }
        }
        len as u64
    }

    /// Создать узел (файл или каталог) с именем `name` в каталоге `dir_id`.
    fn make_node(&self, u: &Unlocked, dir_id: &str, name: &str, is_dir: bool) -> FsResult<Kind> {
        let dir = self.dir_physical(u, dir_id);
        fs::create_dir_all(&dir).map_err(ioerr)?;
        let (node, full) = names::node_name(&u.key, name, dir_id, u.config.shortening_threshold);
        let node_path = dir.join(&node);
        if node_path.exists() {
            return Err(STATUS_OBJECT_NAME_COLLISION.into());
        }
        if is_dir || full.is_some() {
            fs::create_dir(&node_path).map_err(ioerr)?;
        }
        if let Some(full) = &full {
            fs::write(node_path.join(NAME_FILE), full).map_err(ioerr)?;
        }
        let entry = (name.to_string(), node_path.clone());
        self.listing_update(dir_id, |l| {
            l.insert(name.to_lowercase(), entry);
        });
        if is_dir {
            let id = zamok_crypto::uuid_v4().map_err(|_| FspError::from(STATUS_ACCESS_DENIED))?;
            fs::write(node_path.join(DIR_FILE), &id).map_err(ioerr)?;
            zamok_crypto::tree::write_dirid_backup(&self.root, &u.key, &id).map_err(
                |e| match e {
                    zamok_crypto::Error::Io(e) => ioerr(e),
                    _ => FspError::from(STATUS_ACCESS_DENIED),
                },
            )?;
            Ok(Kind::Dir { id })
        } else {
            let content = if full.is_some() {
                node_path.join(CONTENTS_FILE)
            } else {
                node_path
            };
            Ok(Kind::File { content })
        }
    }

    fn dir_is_empty(&self, u: &Unlocked, id: &str) -> FsResult<bool> {
        // Проверка перед удалением читает диск, а не кэш
        Ok(self.list_physical(u, id)?.is_empty())
    }

    fn remove_node(&self, u: &Unlocked, r: &Resolved) -> FsResult<()> {
        if let Some(name) = r.clear.rsplit('\\').next() {
            let lower = name.to_lowercase();
            self.listing_update(&r.parent_id, |l| {
                l.remove(&lower);
            });
        }
        let Some(node) = &r.node else {
            return Err(STATUS_ACCESS_DENIED.into());
        };
        match &r.kind {
            Kind::Dir { id } => {
                // Без рекурсивного удаления: если в каталоге что-то появилось,
                // remove_dir откажет и данные останутся целы
                let phys = self.dir_physical(u, id);
                if phys.exists() {
                    let _ = fs::remove_file(phys.join(DIRID_BACKUP_FILE));
                    fs::remove_dir(&phys)
                        .map_err(|_| FspError::from(STATUS_DIRECTORY_NOT_EMPTY))?;
                }
                let _ = fs::remove_file(node.join(DIR_FILE));
                let _ = fs::remove_file(node.join(NAME_FILE));
                fs::remove_dir(node).map_err(ioerr)?;
                self.ctl.dir_cache.lock().unwrap().clear();
            }
            Kind::File { .. } => {
                if node.is_dir() {
                    fs::remove_dir_all(node).map_err(ioerr)?;
                } else {
                    fs::remove_file(node).map_err(ioerr)?;
                }
            }
        }
        Ok(())
    }
}

impl FileSystemContext for ZamokFs {
    type FileContext = ZFile;

    fn get_security_by_name(
        &self,
        file_name: &U16CStr,
        security_descriptor: Option<&mut [c_void]>,
        _resolver: impl FnOnce(&U16CStr) -> Option<FileSecurity>,
    ) -> FsResult<FileSecurity> {
        let path = file_name.to_string_lossy();
        let mut info = FileInfo::default();
        if self.is_locked() {
            let Some(v) = self.virtual_of(&path) else {
                return Err(STATUS_OBJECT_NAME_NOT_FOUND.into());
            };
            self.fill_virtual(&v, &mut info);
        } else {
            self.with_key(|u| {
                let r = self.resolve(u, &path)?;
                self.ctl.remember(&r.clear);
                self.fill_info(&r, &mut info)
            })?;
        }
        let sz = self.security_copy(security_descriptor);
        Ok(FileSecurity {
            reparse: false,
            sz_security_descriptor: sz,
            attributes: info.file_attributes,
        })
    }

    fn open(
        &self,
        file_name: &U16CStr,
        create_options: u32,
        _granted_access: u32,
        file_info: &mut OpenFileInfo,
    ) -> FsResult<Self::FileContext> {
        let path = file_name.to_string_lossy();
        if self.is_locked() {
            let Some(v) = self.virtual_of(&path) else {
                return Err(STATUS_OBJECT_NAME_NOT_FOUND.into());
            };
            self.fill_virtual(&v, file_info.as_mut());
            return Ok(self.new_context(path, None, Some(v), None));
        }
        self.with_key(|u| {
            let r = self.resolve(u, &path)?;
            if create_options & FILE_DIRECTORY_FILE != 0 && matches!(r.kind, Kind::File { .. }) {
                return Err(STATUS_NOT_A_DIRECTORY.into());
            }
            self.ctl.remember(&r.clear);
            self.fill_info(&r, file_info.as_mut())?;
            let cipher = match &r.kind {
                Kind::File { .. } => None,
                Kind::Dir { .. } => None,
            };
            let virt = if r.node.is_none() {
                Some(Virtual::Root)
            } else {
                None
            };
            Ok(self.new_context(r.clear.clone(), Some(r.kind.clone()), virt, cipher))
        })
    }

    fn close(&self, _context: Self::FileContext) {}

    fn create(
        &self,
        file_name: &U16CStr,
        create_options: u32,
        _granted_access: u32,
        file_attributes: u32,
        _security_descriptor: Option<&[c_void]>,
        _allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        _extra_buffer_is_reparse_point: bool,
        file_info: &mut OpenFileInfo,
    ) -> FsResult<Self::FileContext> {
        self.ctl.touch();
        let path = file_name.to_string_lossy();
        self.with_key(|u| {
            let (parent, name) = parent_and_name(&path)?;
            if !valid_name(&name) {
                return Err(STATUS_OBJECT_NAME_INVALID.into());
            }
            let pr = self.resolve(u, &parent)?;
            let Kind::Dir { id } = &pr.kind else {
                return Err(STATUS_NOT_A_DIRECTORY.into());
            };
            if self.find_child(u, id, &name)?.is_some() {
                return Err(STATUS_OBJECT_NAME_COLLISION.into());
            }
            let is_dir = create_options & FILE_DIRECTORY_FILE != 0;
            let kind = self.make_node(u, id, &name, is_dir)?;
            let cipher = match &kind {
                Kind::File { content } => Some(self.cipher_for(u, content, true)?),
                Kind::Dir { .. } => None,
            };
            let clear = join(&pr.clear, &name);
            let attrs = file_attributes & KEEP_ATTRIBUTES;
            let r = self.resolve(u, &clear)?;
            if attrs != 0 {
                let target = match &r.kind {
                    Kind::File { content } => content.clone(),
                    Kind::Dir { .. } => r.node.clone().unwrap_or_default(),
                };
                let _ = crate::attrs::set_attributes(&target, attrs);
            }
            self.fill_info(&r, file_info.as_mut())?;
            Ok(self.new_context(clear, Some(kind), None, cipher))
        })
    }

    fn cleanup(&self, context: &Self::FileContext, _file_name: Option<&U16CStr>, flags: u32) {
        if flags & FspCleanupDelete as u32 == 0 && !context.delete_on_close.load(Ordering::SeqCst) {
            return;
        }
        if flags & FspCleanupDelete as u32 == 0 {
            return;
        }
        if self.check(context).is_err() || context.virt.is_some() {
            return;
        }
        let path = context.path.lock().unwrap().clone();
        let _ = self.with_key(|u| {
            let r = self.resolve(u, &path)?;
            if let Kind::File { content } = &r.kind {
                if let Some(f) = self
                    .ctl
                    .open_files
                    .lock()
                    .unwrap()
                    .remove(content)
                    .and_then(|w| w.upgrade())
                {
                    f.lock().unwrap().seal();
                }
            }
            self.remove_node(u, &r)
        });
    }

    fn flush(&self, context: Option<&Self::FileContext>, file_info: &mut FileInfo) -> FsResult<()> {
        let Some(ctx) = context else { return Ok(()) };
        self.check(ctx)?;
        if let Some(c) = ctx.cipher.lock().unwrap().clone() {
            c.lock().unwrap().sync().map_err(cerr)?;
        }
        self.info_for_ctx(ctx, file_info)
    }

    fn get_file_info(&self, context: &Self::FileContext, file_info: &mut FileInfo) -> FsResult<()> {
        self.check(context)?;
        self.info_for_ctx(context, file_info)
    }

    fn get_security(
        &self,
        context: &Self::FileContext,
        security_descriptor: Option<&mut [c_void]>,
    ) -> FsResult<u64> {
        self.check(context)?;
        Ok(self.security_copy(security_descriptor))
    }

    fn set_security(
        &self,
        context: &Self::FileContext,
        _info: u32,
        _desc: ModificationDescriptor,
    ) -> FsResult<()> {
        // Права у всех файлов одинаковые: только владелец. Изменения прав молча не применяются.
        self.check(context)
    }

    fn overwrite(
        &self,
        context: &Self::FileContext,
        file_attributes: u32,
        replace_file_attributes: bool,
        _allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        file_info: &mut FileInfo,
    ) -> FsResult<()> {
        self.ctl.touch();
        self.check(context)?;
        let c = self.cipher_of(context)?;
        c.lock().unwrap().set_len(0).map_err(cerr)?;
        if let Some(Kind::File { content }) = context.kind.lock().unwrap().clone() {
            let cur = fs::metadata(&content)
                .map(|m| m.file_attributes() & KEEP_ATTRIBUTES)
                .unwrap_or(0);
            let new = if replace_file_attributes {
                file_attributes & KEEP_ATTRIBUTES
            } else {
                cur | (file_attributes & KEEP_ATTRIBUTES)
            };
            let _ = crate::attrs::set_attributes(&content, new | FILE_ATTRIBUTE_ARCHIVE);
        }
        self.info_for_ctx(context, file_info)
    }

    fn read_directory(
        &self,
        context: &Self::FileContext,
        _pattern: Option<&U16CStr>,
        marker: DirMarker,
        buffer: &mut [u8],
    ) -> FsResult<u32> {
        self.ctl.touch();
        self.check(context)?;
        let mut entries: Vec<(String, FileInfo)> = Vec::new();
        let path = context.path.lock().unwrap().clone();
        let is_root = split(&path).is_empty();
        if self.is_locked() {
            if is_root {
                for v in [Virtual::OpenItem, Virtual::DesktopIni] {
                    let mut fi = FileInfo::default();
                    self.fill_virtual(&v, &mut fi);
                    let name = if v == Virtual::OpenItem {
                        self.open_item_name()
                    } else {
                        "desktop.ini".into()
                    };
                    entries.push((name, fi));
                }
            }
        } else {
            self.with_key(|u| {
                let r = self.resolve(u, &path)?;
                let Kind::Dir { id } = &r.kind else {
                    return Err(STATUS_NOT_A_DIRECTORY.into());
                };
                if !is_root {
                    let mut fi = FileInfo::default();
                    self.fill_info(&r, &mut fi)?;
                    entries.push((".".into(), fi.clone()));
                    entries.push(("..".into(), fi));
                }
                let children: Vec<(String, PathBuf)> =
                    self.with_listing(u, id, |l| l.values().cloned().collect())?;
                for (clear, node) in children {
                    let Ok(kind) = Self::kind_of(&node) else {
                        continue;
                    };
                    let child = Resolved {
                        clear: join(&r.clear, &clear),
                        parent_id: id.clone(),
                        node: Some(node),
                        kind,
                    };
                    let mut fi = FileInfo::default();
                    self.ctl.remember(&child.clear);
                    if self.fill_info(&child, &mut fi).is_ok() {
                        entries.push((clear, fi));
                    }
                }
                Ok(())
            })?;
        }
        if let Ok(lock) = context.dir_buffer.acquire(marker.is_none(), None) {
            let mut di: DirInfo = DirInfo::new();
            for (name, fi) in entries {
                di.reset();
                *di.file_info_mut() = fi;
                di.set_name(&name)?;
                lock.write(&mut di)?;
            }
        }
        Ok(context.dir_buffer.read(marker, buffer))
    }

    fn rename(
        &self,
        context: &Self::FileContext,
        _file_name: &U16CStr,
        new_file_name: &U16CStr,
        replace_if_exists: bool,
    ) -> FsResult<()> {
        self.ctl.touch();
        self.check(context)?;
        let old_path = context.path.lock().unwrap().clone();
        let new_path = new_file_name.to_string_lossy();
        self.with_key(|u| {
            let src = self.resolve(u, &old_path)?;
            let Some(src_node) = src.node.clone() else {
                return Err(STATUS_ACCESS_DENIED.into());
            };
            let (new_parent, new_name) = parent_and_name(&new_path)?;
            if !valid_name(&new_name) {
                return Err(STATUS_OBJECT_NAME_INVALID.into());
            }
            let pr = self.resolve(u, &new_parent)?;
            let Kind::Dir { id: new_dir_id } = &pr.kind else {
                return Err(STATUS_NOT_A_DIRECTORY.into());
            };
            // Переименование только регистра букв того же узла
            let same_node = src.parent_id == *new_dir_id
                && src.clear.rsplit('\\').next().map(|n| n.to_lowercase())
                    == Some(new_name.to_lowercase());
            if !same_node {
                if let Some((_, existing)) = self.find_child(u, new_dir_id, &new_name)? {
                    if !replace_if_exists {
                        return Err(STATUS_OBJECT_NAME_COLLISION.into());
                    }
                    let target = Resolved {
                        clear: join(&pr.clear, &new_name),
                        parent_id: new_dir_id.clone(),
                        kind: Self::kind_of(&existing)?,
                        node: Some(existing),
                    };
                    if let Kind::Dir { id } = &target.kind {
                        if !self.dir_is_empty(u, id)? {
                            return Err(STATUS_ACCESS_DENIED.into());
                        }
                    }
                    self.remove_node(u, &target)?;
                }
            }
            let dir = self.dir_physical(u, new_dir_id);
            fs::create_dir_all(&dir).map_err(ioerr)?;
            let (node, full) =
                names::node_name(&u.key, &new_name, new_dir_id, u.config.shortening_threshold);
            let dst_node = dir.join(&node);
            let src_long = src_node.extension().map(|e| e == "c9s").unwrap_or(false);
            let dst_long = full.is_some();
            match &src.kind {
                Kind::Dir { .. } => {
                    // Узел каталога всегда папка: dir.c9r (+ name.c9s у длинного имени)
                    if src_node != dst_node {
                        fs::rename(&src_node, &dst_node).map_err(ioerr)?;
                    }
                    let name_file = dst_node.join(NAME_FILE);
                    match &full {
                        Some(f) => fs::write(&name_file, f).map_err(ioerr)?,
                        None => {
                            if name_file.exists() {
                                fs::remove_file(&name_file).map_err(ioerr)?;
                            }
                        }
                    }
                }
                Kind::File { content } => {
                    let new_content = if dst_long {
                        dst_node.join(CONTENTS_FILE)
                    } else {
                        dst_node.clone()
                    };
                    if dst_long && !dst_node.exists() {
                        fs::create_dir(&dst_node).map_err(ioerr)?;
                    }
                    if let Some(f) = &full {
                        fs::write(dst_node.join(NAME_FILE), f).map_err(ioerr)?;
                    }
                    if *content != new_content {
                        fs::rename(content, &new_content).map_err(ioerr)?;
                    }
                    if src_long && src_node != dst_node {
                        let _ = fs::remove_dir_all(&src_node);
                    }
                    // таблица открытых файлов: новый путь содержимого
                    let mut table = self.ctl.open_files.lock().unwrap();
                    if let Some(w) = table.remove(content) {
                        table.insert(new_content.clone(), w);
                    }
                    *context.kind.lock().unwrap() = Some(Kind::File {
                        content: new_content,
                    });
                }
            }
            self.invalidate(&src.parent_id);
            self.invalidate(new_dir_id);
            self.ctl.dir_cache.lock().unwrap().clear();
            *context.path.lock().unwrap() = join(&pr.clear, &new_name);
            Ok(())
        })
    }

    fn set_basic_info(
        &self,
        context: &Self::FileContext,
        file_attributes: u32,
        creation_time: u64,
        last_access_time: u64,
        last_write_time: u64,
        _last_change_time: u64,
        file_info: &mut FileInfo,
    ) -> FsResult<()> {
        self.check(context)?;
        if context.virt.is_some() {
            return self.info_for_ctx(context, file_info);
        }
        let path = context.path.lock().unwrap().clone();
        self.with_key(|u| {
            let r = self.resolve(u, &path)?;
            let target = match &r.kind {
                Kind::File { content } => content.clone(),
                Kind::Dir { .. } => r.node.clone().ok_or(FspError::from(STATUS_ACCESS_DENIED))?,
            };
            if file_attributes != u32::MAX {
                crate::attrs::set_attributes(&target, file_attributes & KEEP_ATTRIBUTES)
                    .map_err(ioerr)?;
            }
            crate::attrs::set_times(&target, creation_time, last_access_time, last_write_time)
                .map_err(ioerr)?;
            self.fill_info(&r, file_info)
        })
    }

    fn set_delete(
        &self,
        context: &Self::FileContext,
        _file_name: &U16CStr,
        delete_file: bool,
    ) -> FsResult<()> {
        self.ctl.touch();
        self.check(context)?;
        if context.virt.is_some() {
            return Err(STATUS_ACCESS_DENIED.into());
        }
        if delete_file {
            if let Some(Kind::Dir { id }) = context.kind.lock().unwrap().clone() {
                let empty = self.with_key(|u| self.dir_is_empty(u, &id))?;
                if !empty {
                    return Err(STATUS_DIRECTORY_NOT_EMPTY.into());
                }
            }
        }
        context.delete_on_close.store(delete_file, Ordering::SeqCst);
        Ok(())
    }

    fn set_file_size(
        &self,
        context: &Self::FileContext,
        new_size: u64,
        set_allocation_size: bool,
        file_info: &mut FileInfo,
    ) -> FsResult<()> {
        self.ctl.touch();
        self.check(context)?;
        let c = self.cipher_of(context)?;
        {
            let mut f = c.lock().unwrap();
            let len = f.len().map_err(cerr)?;
            if !set_allocation_size || new_size < len {
                f.set_len(new_size).map_err(cerr)?;
            }
        }
        self.info_for_ctx(context, file_info)
    }

    fn read(&self, context: &Self::FileContext, buffer: &mut [u8], offset: u64) -> FsResult<u32> {
        self.ctl.touch();
        self.check(context)?;
        if let Some(v) = &context.virt {
            let data = self.virtual_content(v);
            if offset as usize >= data.len() {
                return Err(windows::Win32::Foundation::STATUS_END_OF_FILE.into());
            }
            let n = (data.len() - offset as usize).min(buffer.len());
            buffer[..n].copy_from_slice(&data[offset as usize..offset as usize + n]);
            return Ok(n as u32);
        }
        let c = self.cipher_of(context)?;
        let n = c.lock().unwrap().read_at(offset, buffer).map_err(cerr)?;
        if n == 0 && !buffer.is_empty() {
            return Err(windows::Win32::Foundation::STATUS_END_OF_FILE.into());
        }
        Ok(n as u32)
    }

    fn write(
        &self,
        context: &Self::FileContext,
        buffer: &[u8],
        offset: u64,
        write_to_eof: bool,
        constrained_io: bool,
        file_info: &mut FileInfo,
    ) -> FsResult<u32> {
        self.ctl.touch();
        self.check(context)?;
        let c = self.cipher_of(context)?;
        let written = {
            let mut f = c.lock().unwrap();
            let len = f.len().map_err(cerr)?;
            let offset = if write_to_eof { len } else { offset };
            let data = if constrained_io {
                if offset >= len {
                    &buffer[..0]
                } else {
                    &buffer[..buffer.len().min((len - offset) as usize)]
                }
            } else {
                buffer
            };
            f.write_at(offset, data).map_err(cerr)?;
            data.len()
        };
        self.info_for_ctx(context, file_info)?;
        Ok(written as u32)
    }

    fn get_volume_info(&self, out: &mut VolumeInfo) -> FsResult<()> {
        let (total, free) = crate::attrs::disk_space(&self.root).unwrap_or((0, 0));
        out.total_size = total;
        out.free_size = free;
        out.set_volume_label("Zamok");
        Ok(())
    }
}

impl winfsp::notify::NotifyingFileSystemContext<Vec<String>> for ZamokFs {
    fn should_notify(&self) -> Option<Vec<String>> {
        let mut p = self.ctl.pending_invalidate.lock().unwrap();
        if p.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut *p))
        }
    }

    /// Уведомление «удалён» по каждому пути: Проводник обновит окно, а ядро WinFsp
    /// сбросит закэшированные сведения о файле (иначе имя было бы видно до FileInfoTimeout).
    fn notify(&self, paths: Vec<String>, notifier: &winfsp::notify::Notifier) {
        const FILTER: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x10; // имя файла, имя каталога, атрибуты, размер, запись
        const ACTION_REMOVED: u32 = 2;
        for path in paths {
            let mut info: winfsp::notify::NotifyInfo<1024> = winfsp::notify::NotifyInfo::new();
            info.filter = FILTER;
            info.action = ACTION_REMOVED;
            if info.set_name(&path).is_ok() {
                notifier.notify(&info);
            } else {
                // Путь не помещается в уведомление: не пропускаем молча, а ждём истечения кэша
                self.ctl.fallback_wait.store(true, Ordering::SeqCst);
            }
        }
    }
}
