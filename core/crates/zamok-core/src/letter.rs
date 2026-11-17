//! Папки на флешках и дисках exFAT и FAT32: подключение с буквой диска (DESIGN.md, раздел 3.6).
//!
//! WinFsp ставит точку подключения на путь папки только на NTFS (точка подключения это
//! метка reparse point, которой у FAT нет). Поэтому на таких дисках папка остаётся обычной
//! папкой с одним файлом `Открыть <Имя>.zamok` (его пишет Zamok, а не файловая система),
//! а открытая папка подключается на свободную букву диска, от Z: вниз, с меткой тома по
//! имени папки. Хранилище по-прежнему рядом: `.Имя.zamok`.
//!
//! Флешку узнаём по серийному номеру тома и пути хранилища от корня тома: буква между
//! вставками может меняться. Пока флешки нет, папка остаётся в списке «не подключена».

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetLogicalDrives, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::UI::WindowsAndMessaging::{
    RegisterDeviceNotificationW, UnregisterDeviceNotification, DBT_DEVTYP_HANDLE,
    DEVICE_NOTIFY_WINDOW_HANDLE, DEV_BROADCAST_HANDLE, HDEVNOTIFY,
};

use crate::protocol::ErrorCode;

/// Тестовый переключатель: `1` считает любой том флешкой exFAT, `fat32` флешкой FAT32 (с пределом
/// размера файла). Так весь путь проверяется на NTFS машины разработки, где exFAT нет.
pub const FORCE_ENV: &str = "ZAMOK_FORCE_LETTER_MODE";

/// Как подключается папка на этом томе.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// NTFS: точка подключения на пути папки.
    Path,
    /// exFAT, FAT32: буква диска. `fat32`: файл больше 4 ГиБ без 1 байта не записать.
    Letter { fat32: bool },
}

/// Режим по имени файловой системы тома. `None`: такой том не поддерживается (сетевой диск,
/// ReFS, CD), проверка откажет с `not_ntfs`.
pub fn mode_of_fs(fs_name: &str, force: Option<&str>) -> Option<Mode> {
    match force.map(str::trim) {
        Some("1") => return Some(Mode::Letter { fat32: false }),
        Some(f) if f.eq_ignore_ascii_case("fat32") => return Some(Mode::Letter { fat32: true }),
        _ => {}
    }
    match fs_name.to_ascii_uppercase().as_str() {
        "NTFS" => Some(Mode::Path),
        "EXFAT" => Some(Mode::Letter { fat32: false }),
        "FAT32" | "FAT" | "FAT16" | "FAT12" => Some(Mode::Letter { fat32: true }),
        _ => None,
    }
}

/// Режим для папки по её тому.
pub fn mode_for(path: &Path) -> Option<Mode> {
    let fs = crate::preflight::volume_fs(path)?;
    mode_of_fs(&fs, std::env::var(FORCE_ENV).ok().as_deref())
}

/// Самый большой файл на FAT32.
pub const FAT32_MAX_FILE: u64 = 0xFFFF_FFFF;

/// Размер файла шифротекста по размеру открытого текста (формат Cryptomator 8: заголовок 68
/// байт, каждый кусок до 32 КиБ получает 12 байт номера и 16 байт метки).
pub fn ciphertext_size(cleartext: u64) -> u64 {
    use zamok_crypto::content::{HEADER_LEN, NONCE_LEN, PAYLOAD_LEN, TAG_LEN};
    let chunk_extra = (NONCE_LEN + TAG_LEN) as u64;
    let full = cleartext / PAYLOAD_LEN as u64;
    let rest = cleartext % PAYLOAD_LEN as u64;
    HEADER_LEN as u64
        + full * (PAYLOAD_LEN as u64 + chunk_extra)
        + if rest > 0 { rest + chunk_extra } else { 0 }
}

/// Файл не поместится на FAT32 в зашифрованном виде.
pub fn too_big_for_fat32(cleartext: u64) -> bool {
    ciphertext_size(cleartext) > FAT32_MAX_FILE
}

/// Свободные буквы для подключения: от Z: вниз до D:. A:, B: и C: не берём никогда.
pub fn free_letters(used_mask: u32) -> Vec<char> {
    (b'D'..=b'Z')
        .rev()
        .filter(|l| used_mask & (1 << (l - b'A')) == 0)
        .map(char::from)
        .collect()
}

/// Подключить хранилище на свободную букву. Буква может оказаться занятой между проверкой
/// и подключением: тогда пробуем следующую.
pub fn mount(vault: &Path, name: &str) -> anyhow::Result<(zamok_fs::mount::Mounted, char)> {
    let used = unsafe { GetLogicalDrives() };
    let mut last = anyhow::anyhow!("no free drive letter");
    for letter in free_letters(used).into_iter().take(4) {
        match zamok_fs::mount::Mounted::mount_letter(vault, letter, name) {
            Ok(m) => return Ok((m, letter)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Корень тома и путь от него (`E:\`, `Фото\Отпуск`).
pub fn split_volume(path: &Path) -> Option<(PathBuf, String)> {
    let root = crate::preflight::volume_root_path(path)?;
    let full = path.to_string_lossy();
    let r = root.to_string_lossy();
    let rel = full.get(r.len()..)?.trim_start_matches('\\').to_string();
    if !full.to_lowercase().starts_with(&r.to_lowercase()) || rel.is_empty() {
        return None;
    }
    Some((root, rel))
}

/// Найти том с этим серийным номером, на котором есть хранилище `rel_vault`: флешку вставили
/// под другой буквой. Сетевые диски и CD не смотрим (опрос может надолго зависнуть или
/// раскрутить диск). Подходящих несколько: берём первую по алфавиту.
pub fn find_volume(serial: u32, rel_vault: &str) -> Option<PathBuf> {
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    let used = unsafe { GetLogicalDrives() };
    for l in b'A'..=b'Z' {
        if used & (1 << (l - b'A')) == 0 {
            continue;
        }
        let root = format!("{}:\\", char::from(l));
        let w: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) };
        if kind != DRIVE_REMOVABLE && kind != DRIVE_FIXED {
            continue;
        }
        let root = PathBuf::from(root);
        if crate::preflight::volume_serial(&root) != Some(serial) {
            continue;
        }
        let vault = root.join(rel_vault);
        if crate::core::vault_looks_valid(&vault) {
            return Some(root);
        }
    }
    None
}

/// Имя файла, по которому дважды щёлкают в папке.
pub fn opener_name(folder_name: &str) -> String {
    zamok_fs::fs::open_item_name(folder_name)
}

const DESKTOP_INI: &str = "desktop.ini";
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Больше файл открытия не бывает: чужой большой файл с тем же именем не наш.
const MAX_OPENER: u64 = 64 * 1024;

/// Текст файла открытия. Он нужен и там, где Zamok нет: человек откроет файл в Блокноте и
/// узнает, где лежат данные и чем их открыть.
fn opener_text(folder_name: &str) -> String {
    // Оба языка сразу: файл читают и на чужом компьютере, язык которого неизвестен
    [
        format!(
            "Zamok: папка «{folder_name}» зашифрована.\r\n\
             Дважды щёлкните по этому файлу, чтобы ввести пароль: папка откроется как отдельный диск.\r\n\
             Зашифрованные файлы лежат рядом в скрытой папке «.{folder_name}.zamok». Переносите и \
             копируйте её вместе с этой папкой. Без Zamok её открывает Cryptomator (формат vault 8) \
             или zamok-recover: https://github.com/olerast67/zamok\r\n\r\n"
        ),
        format!(
            "Zamok: the folder \"{folder_name}\" is encrypted.\r\n\
             Double-click this file to enter the password: the folder opens as a separate drive.\r\n\
             The encrypted files are next to it, in the hidden folder \".{folder_name}.zamok\". Move \
             and copy it together with this folder. Without Zamok, Cryptomator (vault format 8) or \
             zamok-recover opens it: https://github.com/olerast67/zamok\r\n"
        ),
    ]
    .concat()
}

/// Значок папки для `desktop.ini`. Программа стоит в обычном месте: путь через
/// `%LOCALAPPDATA%`, тогда значок виден и на другом компьютере с Zamok.
fn icon_resource() -> String {
    let Some(icon) = zamok_fs::fs::folder_icon() else {
        return r"%SystemRoot%\System32\imageres.dll,54".to_string();
    };
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = local.to_string_lossy().into_owned();
        if icon
            .to_lowercase()
            .starts_with(&format!("{}\\", local.to_lowercase()))
        {
            return format!("%LOCALAPPDATA%{},0", &icon[local.len()..]);
        }
    }
    format!("{icon},0")
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Папка на месте зашифрованной: обычная папка с файлом открытия и `desktop.ini` со значком
/// замка. Чего нет, то создаётся; что уже есть, не трогается. На пути файл или ссылка:
/// `PathOccupied`, ничего не меняется.
pub fn ensure_placeholder(path: &Path, folder_name: &str) -> Result<(), ErrorCode> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 => {}
        Ok(_) => return Err(ErrorCode::PathOccupied),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(|_| ErrorCode::PathOccupied)?;
        }
        Err(_) => return Err(ErrorCode::PathOccupied),
    }
    let opener = path.join(opener_name(folder_name));
    if std::fs::symlink_metadata(&opener).is_err() {
        // С BOM: Блокнот на старых Windows иначе показал бы кириллицу кракозябрами
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(opener_text(folder_name).as_bytes());
        write_new(&opener, &bytes).map_err(|_| ErrorCode::PathOccupied)?;
    }
    let ini = path.join(DESKTOP_INI);
    if std::fs::symlink_metadata(&ini).is_err() {
        // UTF-16LE с BOM: так desktop.ini понимает Проводник
        let text = format!(
            "[.ShellClassInfo]\r\nIconResource={}\r\nInfoTip=Zamok\r\n",
            icon_resource()
        );
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        // Значок не главное: не вышло, папка всё равно работает
        if write_new(&ini, &bytes).is_ok() {
            let _ = zamok_fs::attrs::set_attributes(
                &ini,
                FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM,
            );
        }
    }
    // Проводник читает desktop.ini только у папки с атрибутом «системный» или «только чтение»
    if let Ok(m) = std::fs::metadata(path) {
        let attrs = m.file_attributes() & 0x27;
        if attrs & FILE_ATTRIBUTE_SYSTEM == 0 {
            let _ = zamok_fs::attrs::set_attributes(path, attrs | FILE_ATTRIBUTE_SYSTEM);
        }
    }
    Ok(())
}

/// Что лежит в папке на пути: только наш файл открытия и `desktop.ini` (или ничего).
fn our_items(path: &Path) -> Option<Vec<PathBuf>> {
    let m = std::fs::symlink_metadata(path).ok()?;
    if !m.is_dir() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let opener = opener_name(&name).to_lowercase();
    let mut items = Vec::new();
    for e in std::fs::read_dir(path).ok()? {
        let e = e.ok()?;
        let n = e.file_name().to_string_lossy().to_lowercase();
        let m = e.metadata().ok()?;
        if m.is_dir() || m.len() > MAX_OPENER || (n != opener && n != DESKTOP_INI) {
            return None;
        }
        items.push(e.path());
    }
    Some(items)
}

/// На пути стоит папка-заглушка Zamok (и ничего чужого в ней нет).
pub fn is_placeholder(path: &Path) -> bool {
    our_items(path).is_some_and(|items| !items.is_empty())
}

/// Убрать папку-заглушку: только если в ней лишь наш файл открытия и `desktop.ini`.
/// Нужно, чтобы на её место вернуть оригинал (откат шифрования) или расшифрованную копию.
pub fn remove_placeholder(path: &Path) -> bool {
    let Some(items) = our_items(path) else {
        return false;
    };
    for item in items {
        let _ = zamok_fs::attrs::set_attributes(&item, 0);
        if std::fs::remove_file(&item).is_err() {
            return false;
        }
    }
    let _ = zamok_fs::attrs::set_attributes(path, 0);
    std::fs::remove_dir(path).is_ok()
}

/// Освободить путь папки: свободен, мёртвая точка подключения снята или убрана наша
/// заглушка. Обычная папка с чужими файлами не трогается никогда.
pub fn free_path(path: &Path) -> bool {
    if crate::mountpoint::prepare(path).is_ok() {
        return true;
    }
    remove_placeholder(path) && crate::mountpoint::state(path) == crate::mountpoint::PathState::Free
}

/// Подписка на «устройство сейчас извлекут» для тома с хранилищем открытой папки. Пока она
/// жива, открыт дескриптор папки хранилища. На `DBT_DEVICEQUERYREMOVE` core запирает папку,
/// отключает букву и закрывает дескриптор, иначе Windows сказала бы «устройство занято».
pub struct Watch {
    handle: isize,
    notify: isize,
}

impl Watch {
    pub fn new(hwnd: isize, dir: &Path) -> Option<Self> {
        if hwnd == 0 {
            return None;
        }
        let w: Vec<u16> = dir
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                PCWSTR(w.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .ok()?;
        let filter = DEV_BROADCAST_HANDLE {
            dbch_size: std::mem::size_of::<DEV_BROADCAST_HANDLE>() as u32,
            dbch_devicetype: DBT_DEVTYP_HANDLE.0,
            dbch_handle: handle,
            ..Default::default()
        };
        let notify = unsafe {
            RegisterDeviceNotificationW(
                HANDLE(HWND(hwnd as *mut _).0),
                &filter as *const _ as *const _,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        };
        match notify {
            Ok(n) => Some(Watch {
                handle: handle.0 as isize,
                notify: n.0 as isize,
            }),
            Err(_) => {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                None
            }
        }
    }

    /// Дескриптор, который Windows назовёт в `DEV_BROADCAST_HANDLE`.
    pub fn handle(&self) -> isize {
        self.handle
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterDeviceNotification(HDEVNOTIFY(self.notify as *mut _));
            let _ = CloseHandle(HANDLE(self.handle as *mut _));
        }
    }
}

/// Что с папкой на флешке по итогам проверки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Хранилище на месте, том тот же.
    Connected,
    /// Флешки нет, вставлена другая или хранилище временно недоступно: папка остаётся в
    /// списке «не подключена».
    Absent,
    /// Флешка на месте, а хранилища нет: его удалили (правило `certainly_deleted`).
    Deleted,
}

/// Решение по одной проверке. Пропавший диск никогда не значит «удалено»: для этого нужен
/// живой родитель хранилища на томе с тем же серийным номером несколько проверок подряд.
pub fn presence(
    found: Result<(), std::io::ErrorKind>,
    parent_exists: bool,
    stored_serial: Option<u32>,
    current_serial: Option<u32>,
    ticks: u32,
) -> Presence {
    match found {
        Ok(()) if stored_serial.is_none() || stored_serial == current_serial => Presence::Connected,
        Ok(()) => Presence::Absent,
        Err(kind) => {
            if crate::core::certainly_deleted(
                kind,
                parent_exists,
                stored_serial,
                current_serial,
                ticks,
            ) {
                Presence::Deleted
            } else {
                Presence::Absent
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind::{NotFound, PermissionDenied};

    #[test]
    fn mode_follows_the_file_system_and_the_test_switch() {
        assert_eq!(mode_of_fs("NTFS", None), Some(Mode::Path));
        assert_eq!(
            mode_of_fs("exFAT", None),
            Some(Mode::Letter { fat32: false })
        );
        assert_eq!(
            mode_of_fs("FAT32", None),
            Some(Mode::Letter { fat32: true })
        );
        assert_eq!(mode_of_fs("FAT", None), Some(Mode::Letter { fat32: true }));
        assert_eq!(mode_of_fs("ReFS", None), None);
        assert_eq!(mode_of_fs("", None), None);
        assert_eq!(
            mode_of_fs("NTFS", Some("1")),
            Some(Mode::Letter { fat32: false })
        );
        assert_eq!(
            mode_of_fs("NTFS", Some("FAT32")),
            Some(Mode::Letter { fat32: true })
        );
        // Другое значение переключателя ничего не меняет
        assert_eq!(mode_of_fs("NTFS", Some("0")), Some(Mode::Path));
    }

    #[test]
    fn ciphertext_size_matches_the_format() {
        use zamok_crypto::content::cleartext_size;
        assert_eq!(ciphertext_size(0), 68);
        assert_eq!(ciphertext_size(1), 68 + 1 + 28);
        assert_eq!(ciphertext_size(32 * 1024), 68 + 32 * 1024 + 28);
        for n in [
            0u64,
            1,
            100,
            32767,
            32768,
            32769,
            65536,
            10_000_000,
            4_000_000_000,
        ] {
            assert_eq!(cleartext_size(ciphertext_size(n)).unwrap(), n);
        }
    }

    #[test]
    fn fat32_refuses_files_whose_ciphertext_exceeds_4_gib() {
        // Самый большой открытый текст, шифротекст которого ещё помещается
        let mut lo = 0u64;
        let mut hi = FAT32_MAX_FILE;
        while lo < hi {
            let mid = lo + (hi - lo).div_ceil(2);
            if ciphertext_size(mid) <= FAT32_MAX_FILE {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        assert!(!too_big_for_fat32(lo));
        assert!(too_big_for_fat32(lo + 1));
        // Около 3,5 МБ уходит на заголовки кусков: файл ровно 4 ГиБ без байта уже не помещается
        assert!(too_big_for_fat32(FAT32_MAX_FILE));
        assert!(lo < FAT32_MAX_FILE - 3_000_000 && lo > FAT32_MAX_FILE - 4_000_000);
        assert!(!too_big_for_fat32(1 << 30));
        // На exFAT предела нет: проверка зовётся только для FAT32
        assert!(too_big_for_fat32(5 * (1 << 30)));
    }

    #[test]
    fn free_letters_go_from_z_down_and_skip_a_b_c() {
        assert_eq!(free_letters(0).first(), Some(&'Z'));
        assert_eq!(free_letters(0).last(), Some(&'D'));
        assert_eq!(free_letters(0).len(), 23);
        let z = 1 << 25;
        let y = 1 << 24;
        assert_eq!(free_letters(z | y).first(), Some(&'X'));
        assert!(free_letters(u32::MAX).is_empty());
    }

    #[test]
    fn absent_drive_is_never_deleted() {
        let t = 100;
        // Флешки нет: родителя нет, серийного номера нет
        assert_eq!(
            presence(Err(NotFound), false, Some(7), None, t),
            Presence::Absent
        );
        // Под той же буквой другая флешка
        assert_eq!(
            presence(Err(NotFound), true, Some(7), Some(9), t),
            Presence::Absent
        );
        // На другой флешке лежит хранилище с тем же именем: не наше
        assert_eq!(
            presence(Ok(()), true, Some(7), Some(9), t),
            Presence::Absent
        );
        // Нет доступа
        assert_eq!(
            presence(Err(PermissionDenied), true, Some(7), Some(7), t),
            Presence::Absent
        );
        // Флешка на месте, хранилища нет, но проверок ещё мало
        assert_eq!(
            presence(Err(NotFound), true, Some(7), Some(7), 1),
            Presence::Absent
        );
        // Флешка на месте, хранилище удалили
        assert_eq!(
            presence(Err(NotFound), true, Some(7), Some(7), t),
            Presence::Deleted
        );
        assert_eq!(
            presence(Ok(()), true, Some(7), Some(7), 0),
            Presence::Connected
        );
    }

    fn tempdir() -> tempfile::TempDir {
        crate::preflight::tests::tempdir()
    }

    #[test]
    fn placeholder_is_created_once_and_removed_only_when_it_holds_our_files() {
        let d = tempdir();
        let p = d.path().join("Фото");
        ensure_placeholder(&p, "Фото").unwrap();
        let opener = p.join("Открыть Фото.zamok");
        let text = std::fs::read(&opener).unwrap();
        assert!(String::from_utf8_lossy(&text).contains(".Фото.zamok"));
        assert!(p.join("desktop.ini").is_file());
        assert!(is_placeholder(&p));
        // Повторно ничего не переписывается
        std::fs::write(&opener, b"Zamok").unwrap();
        ensure_placeholder(&p, "Фото").unwrap();
        assert_eq!(std::fs::read(&opener).unwrap(), b"Zamok");
        // Чужой файл внутри: не заглушка, не удаляется
        std::fs::write(p.join("мой.txt"), b"x").unwrap();
        assert!(!is_placeholder(&p));
        assert!(!remove_placeholder(&p));
        assert!(!free_path(&p));
        assert!(p.join("мой.txt").is_file());
        std::fs::remove_file(p.join("мой.txt")).unwrap();
        assert!(free_path(&p));
        assert!(!p.exists());
        // Файл на пути: занято
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(ensure_placeholder(&p, "Фото"), Err(ErrorCode::PathOccupied));
        assert!(!remove_placeholder(&p));
    }

    #[test]
    fn split_volume_gives_root_and_relative_path() {
        let (root, rel) = split_volume(Path::new(r"C:\Работа\Фото")).unwrap();
        assert_eq!(root, Path::new(r"C:\"));
        assert_eq!(rel, r"Работа\Фото");
        assert!(split_volume(Path::new(r"C:\")).is_none());
    }
}
