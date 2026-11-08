//! Точки подключения на пути папки.
//!
//! После выключения питания на пути может остаться «мёртвая» точка подключения: папка
//! с меткой (reparse point), которая указывает на том WinFsp, которого больше нет.
//! Подключиться поверх неё нельзя, поэтому при старте core её снимает (прототип P4).
//!
//! Удаление здесь только нерекурсивное: снять метку и убрать пустую папку. Обычную папку
//! или файл на этом пути core не трогает никогда.

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FileIdType, GetFileAttributesW, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, OpenFileById, RemoveDirectoryW, BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING,
    VOLUME_NAME_DOS,
};
use windows::Win32::System::Ioctl::{FSCTL_DELETE_REPARSE_POINT, FSCTL_GET_REPARSE_POINT};
use windows::Win32::System::IO::DeviceIoControl;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

/// Что стоит на пути, куда надо подключить папку.
#[derive(Debug, PartialEq, Eq)]
pub enum PathState {
    /// Пути нет: можно подключать.
    Free,
    /// Живая точка подключения тома (папка уже подключена кем-то).
    LiveMount,
    /// Мёртвая точка подключения тома WinFsp.
    StaleMount,
    /// Обычная папка или файл: не трогаем.
    Occupied,
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open_reparse(path: &Path, write: bool) -> std::io::Result<Handle> {
    let w = wide(path);
    let access = if write {
        GENERIC_READ.0 | GENERIC_WRITE.0
    } else {
        FILE_READ_ATTRIBUTES.0
    };
    unsafe {
        CreateFileW(
            PCWSTR(w.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map(Handle)
    .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
}

/// Цель точки подключения тома (`\Device\Volume{…}\` или `\??\Volume{…}\`),
/// `None`, если это не точка подключения.
pub fn mount_target(path: &Path) -> Option<String> {
    let h = open_reparse(path, false).ok()?;
    let mut buf = vec![0u8; 16 * 1024];
    let mut got = 0u32;
    unsafe {
        DeviceIoControl(
            h.0,
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            Some(&mut got),
            None,
        )
    }
    .ok()?;
    let got = got as usize;
    if got < 16 || u32::from_le_bytes(buf[0..4].try_into().ok()?) != IO_REPARSE_TAG_MOUNT_POINT {
        return None;
    }
    // REPARSE_DATA_BUFFER.MountPointReparseBuffer: смещение и длина подставляемого имени
    let off = u16::from_le_bytes([buf[8], buf[9]]) as usize;
    let len = u16::from_le_bytes([buf[10], buf[11]]) as usize;
    let start = 16 + off;
    if start + len > got || !len.is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = buf[start..start + len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn is_volume_target(t: &str) -> bool {
    t.starts_with(r"\Device\Volume{") || t.starts_with(r"\??\Volume{")
}

/// Постоянный номер точки подключения в NTFS: при переименовании и переносе в пределах
/// диска он не меняется. Смотрит только саму точку, внутрь тома не заходит.
pub fn file_id(path: &Path) -> Option<i64> {
    let h = open_reparse(path, false).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(h.0, &mut info) }.ok()?;
    Some(((info.nFileIndexHigh as i64) << 32) | info.nFileIndexLow as i64)
}

/// Убрать приставку `\\?\`, которую добавляет `GetFinalPathNameByHandleW`.
fn strip_verbatim(path: &str) -> Option<&str> {
    let rest = path.strip_prefix(r"\\?\")?;
    // Сетевой путь вида `UNC\…` таким образом не упростить, а Zamok работает только с NTFS-дисками
    (!rest.starts_with(r"UNC\")).then_some(rest)
}

fn in_recycle_bin(path: &str) -> bool {
    path.split('\\')
        .any(|part| part.eq_ignore_ascii_case("$Recycle.Bin"))
}

/// Текущий путь точки подключения с номером `id` на том же диске, что и `on_volume`.
/// `None`: её нет, это уже другая папка (адрес тома не `target`) или она в Корзине,
/// то есть для человека удалена.
pub fn find_by_id(on_volume: &Path, id: i64, target: &str) -> Option<std::path::PathBuf> {
    let hint = open_reparse(on_volume, false).ok()?;
    let path = final_path_by_id(&hint, id)?;
    if in_recycle_bin(&path) {
        return None;
    }
    let path = std::path::PathBuf::from(path);
    (mount_target(&path).as_deref() == Some(target)).then_some(path)
}

/// Путь объекта с номером `id` на томе `hint`.
fn final_path_by_id(hint: &Handle, id: i64) -> Option<String> {
    let desc = FILE_ID_DESCRIPTOR {
        dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
        Type: FileIdType,
        Anonymous: FILE_ID_DESCRIPTOR_0 { FileId: id },
    };
    let h = Handle(
        unsafe {
            OpenFileById(
                hint.0,
                &desc,
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
        }
        .ok()?,
    );
    let mut buf = vec![0u16; 32 * 1024];
    let n = unsafe { GetFinalPathNameByHandleW(h.0, &mut buf, VOLUME_NAME_DOS) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    let full = String::from_utf16_lossy(&buf[..n]);
    strip_verbatim(&full).map(str::to_string)
}

/// Текущий путь папки с номером `id` на том же диске, что и `on_volume`.
/// `None`: её нет или она в Корзине, то есть для человека удалена.
pub fn find_dir_by_id(on_volume: &Path, id: i64) -> Option<std::path::PathBuf> {
    let hint = open_reparse(on_volume, false).ok()?;
    let path = final_path_by_id(&hint, id)?;
    (!in_recycle_bin(&path)).then(|| std::path::PathBuf::from(path))
}

/// Есть ли что-то на пути. Смотрит только атрибуты самой точки подключения и не заходит
/// внутрь подключённой папки (чтение внутри считалось бы работой с папкой и сбивало бы
/// автозапирание по бездействию).
pub fn exists(path: &Path) -> bool {
    let w = wide(path);
    let attrs = unsafe { GetFileAttributesW(PCWSTR(w.as_ptr())) };
    attrs != INVALID_FILE_ATTRIBUTES
}

pub fn state(path: &Path) -> PathState {
    let w = wide(path);
    let attrs = unsafe { GetFileAttributesW(PCWSTR(w.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        // Нет такого пути (или нет доступа к родителю: тогда подключение само скажет ошибку)
        return PathState::Free;
    }
    let reparse_dir =
        attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 && attrs & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    if !reparse_dir {
        return PathState::Occupied;
    }
    match mount_target(path) {
        Some(t) if is_volume_target(&t) => {
            if std::fs::read_dir(path).is_ok() {
                PathState::LiveMount
            } else {
                PathState::StaleMount
            }
        }
        // Чужая метка (ссылка OneDrive, символьная ссылка и т. п.): не наша, не трогаем
        _ => PathState::Occupied,
    }
}

/// Снять мёртвую точку подключения: удалить метку и пустую папку.
/// Перед удалением ещё раз проверяет, что это именно мёртвая точка подключения тома.
pub fn remove_stale(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        state(path) == PathState::StaleMount,
        "not a stale mount point"
    );
    {
        let h = open_reparse(path, true)?;
        // REPARSE_GUID_DATA_BUFFER без данных: тег, длина 0, резерв
        let mut hdr = [0u8; 8];
        hdr[0..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        unsafe {
            DeviceIoControl(
                h.0,
                FSCTL_DELETE_REPARSE_POINT,
                Some(hdr.as_ptr() as *const _),
                hdr.len() as u32,
                None,
                0,
                None,
                None,
            )
        }?;
    }
    // RemoveDirectoryW удаляет только пустую папку: если в ней вдруг что-то есть, откажет
    let w = wide(path);
    unsafe { RemoveDirectoryW(PCWSTR(w.as_ptr())) }?;
    Ok(())
}

/// Подготовить путь к подключению: свободен или мёртвая точка снята.
pub fn prepare(path: &Path) -> Result<(), PathState> {
    match state(path) {
        PathState::Free => Ok(()),
        PathState::StaleMount => {
            if remove_stale(path).is_ok() && state(path) == PathState::Free {
                Ok(())
            } else {
                Err(PathState::StaleMount)
            }
        }
        other => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycle_bin_counts_as_deleted() {
        assert!(in_recycle_bin(r"C:\$RECYCLE.BIN\S-1-5\$R1"));
        assert!(!in_recycle_bin(r"C:\Users\me\Recycle.Bin stuff\a"));
    }

    #[test]
    fn verbatim_prefix_is_removed_only_from_local_paths() {
        assert_eq!(strip_verbatim(r"\\?\D:\a\b"), Some(r"D:\a\b"));
        assert_eq!(strip_verbatim(r"\\?\UNC\srv\a"), None);
    }

    #[test]
    fn missing_path_is_free() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(state(&d.path().join("нет")), PathState::Free);
    }

    #[test]
    fn plain_folder_and_file_are_occupied_and_untouched() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("Папка");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("f.txt"), b"x").unwrap();
        assert_eq!(state(&dir), PathState::Occupied);
        assert_eq!(state(&dir.join("f.txt")), PathState::Occupied);
        assert!(remove_stale(&dir).is_err());
        assert_eq!(prepare(&dir), Err(PathState::Occupied));
        assert!(dir.join("f.txt").exists());
    }
}
