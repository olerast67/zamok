//! Проверки перед шифрованием папки на месте (DESIGN.md, раздел 2.1) и опись её содержимого.
//!
//! Проверки ничего не меняют на диске. Отказ возвращает код и до 20 примеров (относительные
//! пути проблемных файлов), чтобы окно показало человеку, что мешает. В журнал примеры
//! не пишутся.

use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard, GetDiskFreeSpaceExW,
    GetFileInformationByHandle, GetVolumeInformationW, GetVolumePathNameW,
    BY_HANDLE_FILE_INFORMATION, WIN32_FIND_STREAM_DATA,
};

use crate::config::normalize;
use crate::protocol::ErrorCode;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;
/// Глубже не пускает обход хранилища в zamok-crypto (256), оставляем запас.
const MAX_DEPTH: usize = 200;
const MAX_EXAMPLES: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File { size: u64, mtime: u64 },
}

/// Время и атрибуты, которые переносятся в зашифрованную копию: файловая система Zamok
/// показывает их по файлу шифротекста (формат Cryptomator сам их не хранит).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub created: u64,
    pub written: u64,
    /// Только чтение, скрытый, системный, архивный.
    pub attrs: u32,
}

/// Элемент описи. `rel` через `\`, родители всегда идут раньше детей.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub rel: String,
    pub kind: Kind,
    pub meta: Meta,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub items: Vec<Item>,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug)]
pub struct Refusal {
    pub code: ErrorCode,
    pub examples: Vec<String>,
}

impl Refusal {
    fn new(code: ErrorCode) -> Self {
        Refusal {
            code,
            examples: Vec::new(),
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn env_path(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| normalize(Path::new(&v)))
}

fn inside(path: &str, base: &str) -> bool {
    path == base || path.starts_with(&format!("{base}\\"))
}

/// Системные и особые папки, которые шифровать нельзя: сама папка, всё внутри неё
/// (для системных) и все её родители.
fn system_refusal(path: &str, data_dir: &Path) -> bool {
    let mut no_inside = Vec::new();
    for v in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(p) = env_path(v) {
            no_inside.push(p);
        }
    }
    no_inside.push(normalize(data_dir));
    let mut exact = Vec::new();
    if let Some(profile) = env_path("USERPROFILE") {
        // Корень профиля и известные папки: их пути знают Windows и программы
        for k in [
            "",
            "Desktop",
            "Documents",
            "Downloads",
            "Pictures",
            "Music",
            "Videos",
        ] {
            exact.push(if k.is_empty() {
                profile.clone()
            } else {
                format!("{profile}\\{}", k.to_lowercase())
            });
        }
    }
    no_inside.iter().any(|b| inside(path, b) || inside(b, path))
        || exact.iter().any(|b| b == path || inside(b, path))
}

/// Папки облачных клиентов: пока прототип P6 не проверил их, не шифруем (DESIGN.md, раздел 11).
fn cloud_folder(path: &str) -> bool {
    for v in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(p) = env_path(v) {
            if inside(path, &p) {
                return true;
            }
        }
    }
    path.split('\\').any(|part| {
        matches!(
            part,
            "dropbox" | "yandexdisk" | "яндекс.диск" | "google drive"
        ) || part.starts_with("onedrive")
    })
}

fn volume_root(path: &Path) -> Option<Vec<u16>> {
    let w = wide(&path.to_string_lossy());
    let mut buf = vec![0u16; 1024];
    unsafe { GetVolumePathNameW(PCWSTR(w.as_ptr()), &mut buf) }.ok()?;
    let len = buf.iter().position(|&c| c == 0)?;
    buf.truncate(len + 1);
    Some(buf)
}

fn is_ntfs(path: &Path) -> bool {
    let Some(root) = volume_root(path) else {
        return false;
    };
    let mut fs_name = [0u16; 64];
    let ok = unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_name),
        )
    }
    .is_ok();
    let len = fs_name.iter().position(|&c| c == 0).unwrap_or(0);
    ok && String::from_utf16_lossy(&fs_name[..len]) == "NTFS"
}

/// Серийный номер тома, на котором лежит `path`.
pub fn volume_serial(path: &Path) -> Option<u32> {
    let root = volume_root(path)?;
    let mut serial = 0u32;
    unsafe {
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            Some(&mut serial),
            None,
            None,
            None,
        )
    }
    .ok()?;
    Some(serial)
}

pub fn free_space(dir: &Path) -> Option<u64> {
    let w = wide(&dir.to_string_lossy());
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

fn file_info(f: &std::fs::File) -> Option<BY_HANDLE_FILE_INFORMATION> {
    use std::os::windows::io::AsRawHandle;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut info) }.ok()?;
    Some(info)
}

fn open_attrs(path: &Path) -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .access_mode(0x80) // FILE_READ_ATTRIBUTES
        .share_mode(0x7)
        .open(path)
        .ok()
}

/// Число жёстких ссылок на файл. Больше одной: открытый текст остался бы по другой
/// ссылке после удаления оригинала.
fn link_count(path: &Path) -> Option<u32> {
    Some(file_info(&open_attrs(path)?)?.nNumberOfLinks)
}

/// Номер файла NTFS по открытому файлу: другой файл с тем же именем, размером и временем
/// (подмена копированием с сохранением даты) получит другой номер.
pub fn file_id_of(f: &std::fs::File) -> Option<u64> {
    let info = file_info(f)?;
    Some(((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64)
}

/// Номер файла NTFS по пути.
pub fn file_id(path: &Path) -> Option<u64> {
    file_id_of(&open_attrs(path)?)
}

/// Есть ли у файла альтернативные потоки NTFS с данными (кроме пометок «скачано из
/// интернета», которые не жалко потерять). Формат хранилища их не хранит.
fn has_extra_streams(path: &Path) -> bool {
    let w = wide(&path.to_string_lossy());
    let mut data = WIN32_FIND_STREAM_DATA::default();
    let Ok(h) = (unsafe {
        FindFirstStreamW(
            PCWSTR(w.as_ptr()),
            FindStreamInfoStandard,
            &mut data as *mut _ as *mut _,
            None,
        )
    }) else {
        return false; // у файла нет потоков (или не NTFS): лишних нет
    };
    let mut extra = false;
    loop {
        let len = data.cStreamName.iter().position(|&c| c == 0).unwrap_or(0);
        let name = String::from_utf16_lossy(&data.cStreamName[..len]).to_lowercase();
        if !matches!(
            name.as_str(),
            "::$data" | ":zone.identifier:$data" | ":smartscreen:$data"
        ) {
            extra = true;
            break;
        }
        if unsafe { FindNextStreamW(h, &mut data as *mut _ as *mut _) }.is_err() {
            break;
        }
    }
    unsafe {
        let _ = FindClose(h);
    }
    extra
}

fn mtime(m: &std::fs::Metadata) -> u64 {
    m.last_write_time()
}

/// Обойти папку: опись в порядке «родитель раньше детей». Ссылки, облачные заглушки,
/// неподходящие имена и слишком глубокие пути попадают в `bad`.
pub fn walk(root: &Path, bad: &mut Vec<String>) -> std::io::Result<Plan> {
    let mut plan = Plan::default();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        let mut subdirs = Vec::new();
        for e in entries {
            let name = e.file_name();
            let shown = name.to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() {
                shown.clone()
            } else {
                format!("{rel}\\{shown}")
            };
            // Метаданные без перехода по ссылкам
            let m = e.metadata()?;
            let attrs = m.file_attributes();
            let good_name = name
                .to_str()
                .is_some_and(|n| zamok_crypto::tree::safe_name(n).is_ok());
            if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || attrs
                    & (FILE_ATTRIBUTE_OFFLINE
                        | FILE_ATTRIBUTE_RECALL_ON_OPEN
                        | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
                    != 0
                || !good_name
                || depth + 1 > MAX_DEPTH
                || (!m.is_dir() && link_count(&e.path()).is_none_or(|n| n > 1))
                || has_extra_streams(&e.path())
            {
                bad.push(child_rel);
                continue;
            }
            let meta = Meta {
                created: m.creation_time(),
                written: m.last_write_time(),
                attrs: attrs & 0x27,
            };
            if m.is_dir() {
                plan.items.push(Item {
                    rel: child_rel.clone(),
                    kind: Kind::Dir,
                    meta,
                });
                subdirs.push((e.path(), child_rel, depth + 1));
            } else {
                plan.files += 1;
                plan.bytes += m.len();
                plan.items.push(Item {
                    rel: child_rel,
                    kind: Kind::File {
                        size: m.len(),
                        mtime: mtime(&m),
                    },
                    meta,
                });
            }
        }
        // В стек в обратном порядке: обход идёт по алфавиту
        stack.extend(subdirs.into_iter().rev());
    }
    Ok(plan)
}

/// Файлы, которые сейчас держат открытыми другие программы (открыть без совместного
/// доступа не вышло), и файлы, которые нельзя прочитать.
fn files_in_use(root: &Path, plan: &Plan) -> (Vec<String>, Vec<String>) {
    let mut busy = Vec::new();
    let mut unreadable = Vec::new();
    for item in &plan.items {
        if let Kind::File { .. } = item.kind {
            let r = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(root.join(&item.rel));
            match r {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.raw_os_error(),
                        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)
                    ) =>
                {
                    busy.push(item.rel.clone())
                }
                Err(_) => unreadable.push(item.rel.clone()),
            }
            if busy.len() + unreadable.len() >= MAX_EXAMPLES {
                break;
            }
        }
    }
    (busy, unreadable)
}

/// Имена рядом с папкой: `.Имя.zamok.tmp`, `.Имя.zamok`, `.Имя.zamok-original`.
pub struct Siblings {
    pub tmp: PathBuf,
    pub vault: PathBuf,
    pub aside: PathBuf,
}

pub fn siblings(path: &Path) -> Option<Siblings> {
    let parent = path.parent()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(Siblings {
        tmp: parent.join(format!(".{name}.zamok.tmp")),
        vault: parent.join(format!(".{name}.zamok")),
        aside: parent.join(format!(".{name}.zamok-original")),
    })
}

/// Все проверки. `taken`: пути уже зашифрованных папок и их хранилищ.
pub fn check(path: &Path, taken: &[PathBuf], data_dir: &Path) -> Result<Plan, Refusal> {
    check_inner(path, taken, data_dir, None)
}

/// Проверки перед продолжением прерванного шифрования: рядом может лежать наша
/// `.Имя.zamok.tmp` (что она наша, проверяет вызывающий), а `kept` байт уже в ней и второй
/// раз места не займут.
pub fn check_resume(
    path: &Path,
    taken: &[PathBuf],
    data_dir: &Path,
    kept: u64,
) -> Result<Plan, Refusal> {
    check_inner(path, taken, data_dir, Some(kept))
}

fn check_inner(
    path: &Path,
    taken: &[PathBuf],
    data_dir: &Path,
    resume: Option<u64>,
) -> Result<Plan, Refusal> {
    if !path.is_absolute() || path.parent().is_none() || path.file_name().is_none() {
        return Err(Refusal::new(ErrorCode::BadRequest));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| Refusal::new(ErrorCode::NotFound))?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        // Файл, ссылка, точка подключения (в том числе уже зашифрованная папка)
        return Err(Refusal::new(ErrorCode::NotAPlainFolder));
    }
    let n = normalize(path);
    // Корень диска: `c:`
    if n.len() <= 3 {
        return Err(Refusal::new(ErrorCode::SystemFolder));
    }
    if system_refusal(&n, data_dir) {
        return Err(Refusal::new(ErrorCode::SystemFolder));
    }
    if cloud_folder(&n) {
        return Err(Refusal::new(ErrorCode::CloudFolder));
    }
    if taken.iter().any(|t| {
        let t = normalize(t);
        inside(&n, &t) || inside(&t, &n)
    }) {
        return Err(Refusal::new(ErrorCode::AlreadyAdded));
    }
    if !is_ntfs(path) {
        return Err(Refusal::new(ErrorCode::NotNtfs));
    }
    let sib = siblings(path).ok_or_else(|| Refusal::new(ErrorCode::BadRequest))?;
    // При продолжении своя `.zamok.tmp` не мешает
    let existing: Vec<String> = [&sib.tmp, &sib.vault, &sib.aside]
        .iter()
        .skip(usize::from(resume.is_some()))
        .filter(|p| std::fs::symlink_metadata(p).is_ok())
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    if !existing.is_empty() {
        return Err(Refusal {
            code: ErrorCode::VaultExists,
            examples: existing,
        });
    }

    let mut bad = Vec::new();
    let plan = walk(path, &mut bad).map_err(|_| Refusal::new(ErrorCode::Unreadable))?;
    if !bad.is_empty() {
        bad.truncate(MAX_EXAMPLES);
        return Err(Refusal {
            code: ErrorCode::UnsupportedItems,
            examples: bad,
        });
    }
    let (busy, unreadable) = files_in_use(path, &plan);
    if !busy.is_empty() {
        return Err(Refusal {
            code: ErrorCode::FilesInUse,
            examples: busy,
        });
    }
    if !unreadable.is_empty() {
        return Err(Refusal {
            code: ErrorCode::Unreadable,
            examples: unreadable,
        });
    }
    // Вторая копия плюс 10%, по кластеру на файл и 64 МБ запаса
    let copy = plan.bytes.saturating_sub(resume.unwrap_or(0));
    let need = copy + copy / 10 + plan.items.len() as u64 * 4096 + 64 * 1024 * 1024;
    match free_space(path.parent().unwrap()) {
        Some(free) if free >= need => {}
        _ => return Err(Refusal::new(ErrorCode::NotEnoughSpace)),
    }
    Ok(plan)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Временная папка внутри target: %TEMP% лежит в AppData, а её шифровать запрещено.
    pub(crate) fn tempdir() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    fn tree() -> tempfile::TempDir {
        let d = tempdir();
        let r = d.path().join("Работа");
        std::fs::create_dir_all(r.join("б").join("в")).unwrap();
        std::fs::create_dir_all(r.join("пустая")).unwrap();
        std::fs::write(r.join("а.txt"), b"12345").unwrap();
        std::fs::write(r.join("б").join("в").join("г.bin"), vec![7u8; 100]).unwrap();
        d
    }

    #[test]
    fn walk_lists_parents_before_children() {
        let d = tree();
        let mut bad = Vec::new();
        let plan = walk(&d.path().join("Работа"), &mut bad).unwrap();
        assert!(bad.is_empty());
        let rels: Vec<&str> = plan.items.iter().map(|i| i.rel.as_str()).collect();
        assert_eq!(rels, ["а.txt", "б", "пустая", "б\\в", "б\\в\\г.bin"]);
        assert_eq!((plan.files, plan.bytes), (2, 105));
    }

    #[test]
    fn check_accepts_plain_folder_and_refuses_the_rest() {
        let d = tree();
        let data = d.path().join("data");
        let r = d.path().join("Работа");
        let plan = check(&r, &[], &data).unwrap();
        assert_eq!(plan.files, 2);
        // Файл вместо папки, нет такого пути, корень диска
        assert_eq!(
            check(&r.join("а.txt"), &[], &data).unwrap_err().code,
            ErrorCode::NotAPlainFolder
        );
        assert_eq!(
            check(&d.path().join("нет"), &[], &data).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            check(Path::new("C:\\"), &[], &data).unwrap_err().code,
            ErrorCode::BadRequest
        );
        // Уже зашифрована или внутри зашифрованной
        assert_eq!(
            check(&r, &[d.path().to_path_buf()], &data)
                .unwrap_err()
                .code,
            ErrorCode::AlreadyAdded
        );
        // Рядом уже лежит хранилище с таким именем
        std::fs::create_dir(d.path().join(".Работа.zamok")).unwrap();
        let e = check(&r, &[], &data).unwrap_err();
        assert_eq!(e.code, ErrorCode::VaultExists);
        assert_eq!(e.examples, [".Работа.zamok"]);
    }

    #[test]
    fn file_open_without_sharing_is_reported() {
        let d = tree();
        let r = d.path().join("Работа");
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(r.join("а.txt"))
            .unwrap();
        let e = check(&r, &[], &d.path().join("data")).unwrap_err();
        assert_eq!(e.code, ErrorCode::FilesInUse);
        assert_eq!(e.examples, ["а.txt"]);
    }

    #[test]
    fn hard_links_and_alternate_streams_are_refused() {
        let d = tree();
        let r = d.path().join("Работа");
        let data = d.path().join("data");
        // Пометка «скачано из интернета» не мешает
        std::fs::write(
            r.join("а.txt:Zone.Identifier"),
            b"[ZoneTransfer]\r\nZoneId=3",
        )
        .unwrap();
        assert!(check(&r, &[], &data).is_ok());
        // Вторая жёсткая ссылка снаружи папки
        std::fs::hard_link(r.join("а.txt"), d.path().join("снаружи.txt")).unwrap();
        let e = check(&r, &[], &data).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnsupportedItems);
        assert_eq!(e.examples, ["а.txt"]);
        std::fs::remove_file(d.path().join("снаружи.txt")).unwrap();
        // Данные в альтернативном потоке
        std::fs::write(r.join("б").join("в").join("г.bin:секрет"), b"hidden").unwrap();
        let e = check(&r, &[], &data).unwrap_err();
        assert_eq!(e.examples, ["б\\в\\г.bin"]);
    }

    #[test]
    fn system_folders_and_their_parents_are_refused() {
        let data = Path::new(r"C:\Users\x\AppData\Local\Zamok");
        let win = env_path("SystemRoot").unwrap();
        assert!(system_refusal(&win, data));
        assert!(system_refusal(&format!("{win}\\system32"), data));
        let profile = env_path("USERPROFILE").unwrap();
        assert!(system_refusal(&profile, data));
        assert!(system_refusal(&format!("{profile}\\documents"), data));
        assert!(!system_refusal(
            &format!("{profile}\\documents\\работа"),
            data
        ));
        // Родитель профиля (C:\Users) тоже нельзя
        let users = normalize(Path::new(&profile).parent().unwrap());
        assert!(system_refusal(&users, data));
    }

    #[test]
    fn cloud_folders_are_refused() {
        assert!(cloud_folder(r"c:\users\x\dropbox\работа"));
        assert!(cloud_folder(r"d:\яндекс.диск\a"));
        assert!(cloud_folder(r"c:\users\x\onedrive - company\a"));
        assert!(!cloud_folder(r"d:\работа\клиенты"));
    }

    #[test]
    fn siblings_are_hidden_names_next_to_the_folder() {
        let s = siblings(Path::new(r"D:\Работа\Клиенты")).unwrap();
        assert_eq!(s.tmp, Path::new(r"D:\Работа\.Клиенты.zamok.tmp"));
        assert_eq!(s.vault, Path::new(r"D:\Работа\.Клиенты.zamok"));
        assert_eq!(s.aside, Path::new(r"D:\Работа\.Клиенты.zamok-original"));
    }
}
