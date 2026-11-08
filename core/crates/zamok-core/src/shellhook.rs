//! Связь с `zamok-shell.dll`, которая работает внутри Проводника.
//!
//! - Список путей папок публикуется в общей памяти: DLL не должна читать диск.
//! - DLL кладётся в `%LOCALAPPDATA%\Zamok\shell` под именем с отпечатком содержимого и
//!   регистрируется в HKCU. Загруженную Проводником DLL нельзя заменить, поэтому у новой
//!   версии всегда новое имя, а старые удаляются, когда их уже никто не держит.
//!
//! Только в настоящей установке: при `ZAMOK_DATA_DIR` (тесты) не делается ничего.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};
use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

use crate::protocol::FolderInfo;

// То же устройство в zamok-shell: счётчик записи u32 (нечётный, пока идёт запись),
// длина u32 в знаках UTF-16, затем пути по одному в строке.
const SIZE: usize = 64 * 1024;
const HEADER: usize = 8;

const CLSID: &str = "{3F7A9C12-8E4B-4D61-A5C7-19E2B60D4F83}";

static VIEW: OnceLock<Option<usize>> = OnceLock::new();
static LAST: Mutex<Option<String>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_reg(key: &str, name: Option<&str>, value: &str) -> bool {
    let key = wide(key);
    let name = name.map(wide);
    let data = wide(value);
    let bytes = (data.len() * 2) as u32;
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
            REG_SZ.0,
            Some(data.as_ptr() as *const _),
            bytes,
        )
    }
    .is_ok()
}

/// Имя файла по содержимому: другая версия DLL всегда ляжет рядом, а не поверх занятой.
fn versioned_name(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let hex: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("zamok-shell-{hex}.dll")
}

/// Скопировать DLL из папки программы и зарегистрировать. Нет DLL рядом с core: запуск из
/// папки сборки, ничего не делать.
fn install_dll(exe_dir: &Path, shell_dir: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(exe_dir.join("zamok-shell.dll")).ok()?;
    let name = versioned_name(&bytes);
    let target = shell_dir.join(&name);
    if !target.is_file() {
        std::fs::create_dir_all(shell_dir).ok()?;
        // Через временное имя: DLL не окажется записанной наполовину
        let tmp = shell_dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, &bytes).ok()?;
        std::fs::rename(&tmp, &target).ok()?;
    }
    // Старые версии: те, что держит Проводник, не удалятся, до них дойдёт очередь потом
    for old in std::fs::read_dir(shell_dir).ok()?.flatten() {
        let n = old.file_name().to_string_lossy().into_owned();
        if n != name && n.starts_with("zamok-shell-") {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Some(target)
}

fn register(dll: &Path) -> bool {
    let server = format!(r"Software\Classes\CLSID\{CLSID}\InprocServer32");
    set_reg(&server, None, &dll.to_string_lossy())
        && set_reg(&server, Some("ThreadingModel"), "Apartment")
        && set_reg(
            r"Software\Classes\Directory\shellex\CopyHookHandlers\Zamok",
            None,
            CLSID,
        )
}

fn setup() -> Option<usize> {
    let exe = std::env::current_exe().ok()?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    let shell_dir = PathBuf::from(local).join("Zamok").join("shell");
    let dll = install_dll(exe.parent()?, &shell_dir)?;
    if !register(&dll) {
        tracing::warn!("explorer extension could not be registered");
    }
    let map = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            None,
            PAGE_READWRITE,
            0,
            SIZE as u32,
            w!("Local\\ZamokFolders"),
        )
    }
    .ok()?;
    // Память живёт, пока жив процесс: и описатель, и вид намеренно не закрываются
    let view = unsafe { MapViewOfFile(map, FILE_MAP_ALL_ACCESS, 0, 0, SIZE) };
    (!view.Value.is_null()).then_some(view.Value as usize)
}

fn write_list(base: *mut u8, text: &str) {
    let units: Vec<u16> = text.encode_utf16().take((SIZE - HEADER) / 2).collect();
    // SAFETY: base указывает на вид размером SIZE, запись идёт только из одного потока
    unsafe {
        let seq = &*(base as *const AtomicU32);
        seq.fetch_add(1, Ordering::SeqCst);
        (base.add(4) as *mut u32).write_volatile(units.len() as u32);
        let data = base.add(HEADER) as *mut u16;
        for (i, u) in units.iter().enumerate() {
            data.add(i).write_volatile(*u);
        }
        seq.fetch_add(1, Ordering::SeqCst);
    }
}

/// Опубликовать пути папок, если список изменился.
pub fn publish(folders: &[FolderInfo]) {
    if std::env::var_os("ZAMOK_DATA_DIR").is_some() {
        return;
    }
    let Some(base) = *VIEW.get_or_init(setup) else {
        return;
    };
    let text = folders
        .iter()
        .map(|f| f.path.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut last = LAST.lock().unwrap();
    if last.as_deref() != Some(text.as_str()) {
        write_list(base as *mut u8, &text);
        *last = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_version_gets_a_new_name_and_old_ones_are_removed() {
        let exe = tempfile::tempdir().unwrap();
        let shell = tempfile::tempdir().unwrap();
        assert!(install_dll(exe.path(), shell.path()).is_none());

        std::fs::write(exe.path().join("zamok-shell.dll"), b"one").unwrap();
        let first = install_dll(exe.path(), shell.path()).unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), b"one");

        std::fs::write(exe.path().join("zamok-shell.dll"), b"two").unwrap();
        let second = install_dll(exe.path(), shell.path()).unwrap();
        assert_ne!(first, second);
        assert!(second.is_file() && !first.exists());
    }

    #[test]
    fn list_layout_matches_the_extension() {
        let mut mem = vec![0u64; SIZE / 8];
        let base = mem.as_mut_ptr() as *mut u8;
        write_list(base, "D:\\А\nE:\\Б");
        unsafe {
            assert_eq!((base as *const u32).read(), 2);
            let len = (base.add(4) as *const u32).read() as usize;
            let s = std::slice::from_raw_parts(base.add(HEADER) as *const u16, len);
            assert_eq!(String::from_utf16_lossy(s), "D:\\А\nE:\\Б");
        }
    }
}
