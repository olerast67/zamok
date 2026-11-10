//! `zamok-shell.dll`: расширение Проводника. Сейчас в нём одно действие: не дать перенести
//! зашифрованную папку на другой диск. Проводник при таком переносе скопировал бы туда файлы
//! открытой папки без шифрования и удалил их из хранилища (проверено опытом).
//!
//! Код работает внутри Проводника, поэтому: ни диска, ни сети, ни долгих операций, список
//! папок берётся из общей памяти, которую публикует `zamok-core`. Паника здесь уронила бы
//! Проводник (`panic = "abort"`), поэтому в коде нет `unwrap` и индексации.
#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::OnceLock;

use windows::core::{w, Ref, Result, BOOL, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, HWND, S_FALSE,
};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
};
use windows::Win32::UI::Shell::{ICopyHookW, ICopyHookW_Impl};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
use windows_core::{implement, IUnknown, Interface};

/// Номер класса; тот же записывает в реестр `zamok-core`.
pub const CLSID: GUID = GUID::from_u128(0x3f7a9c12_8e4b_4d61_a5c7_19e2b60d4f83);

// Устройство общей памяти (то же в zamok-core, shellhook.rs): счётчик записи u32 (нечётный,
// пока идёт запись), длина u32 в знаках UTF-16, затем пути по одному в строке.
const SIZE: usize = 64 * 1024;
const HEADER: usize = 8;

const FO_MOVE: u32 = 1;
const FOF_NOERRORUI: u32 = 0x400;
const IDYES: u32 = 6;
const IDNO: u32 = 7;

fn normalize(path: &str) -> String {
    path.trim_end_matches('\\').to_lowercase()
}

fn drive(path: &str) -> Option<char> {
    let mut c = path.chars();
    let d = c.next()?;
    (c.next() == Some(':')).then(|| d.to_ascii_lowercase())
}

/// Какая папка Zamok мешает переносу `src` в `dest`. Нужна проверка и родительских папок:
/// вместе с ними Проводник унёс бы и зашифрованную папку внутри. `dest` без буквы диска
/// (сетевая папка) считается другим диском.
pub fn blocked_by<'a>(src: &str, dest: &str, folders: &'a str) -> Option<&'a str> {
    let from = drive(src)?;
    if drive(dest) == Some(from) {
        return None;
    }
    let src = normalize(src);
    let inside = format!("{src}\\");
    folders.lines().find(|f| {
        let f = normalize(f);
        !f.is_empty() && (f == src || f.starts_with(&inside))
    })
}

/// Имя общей памяти. `ZAMOK_SHELL_MAP` задаёт своё имя для теста DLL (`tests/shell`), чтобы он
/// не трогал список работающего core. Переменная читается из памяти процесса один раз.
fn map_name() -> &'static HSTRING {
    static NAME: OnceLock<HSTRING> = OnceLock::new();
    NAME.get_or_init(|| match std::env::var_os("ZAMOK_SHELL_MAP") {
        Some(n) if !n.is_empty() => HSTRING::from(n.as_os_str()),
        _ => HSTRING::from("Local\\ZamokFolders"),
    })
}

/// Прочитать список из общей памяти. Нет памяти (core не запущен): пустой список.
fn read_folders() -> String {
    let Ok(map) = (unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, map_name()) }) else {
        return String::new();
    };
    let view = unsafe { MapViewOfFile(map, FILE_MAP_READ, 0, 0, SIZE) };
    let base = view.Value as *const u8;
    let mut text = String::new();
    if !base.is_null() {
        // core мог писать в этот момент: тогда счётчик нечётный или изменился
        for _ in 0..5 {
            unsafe {
                let seq = (base as *const u32).read_volatile();
                let len =
                    ((base.add(4) as *const u32).read_volatile() as usize).min((SIZE - HEADER) / 2);
                let mut units = Vec::with_capacity(len);
                let data = base.add(HEADER) as *const u16;
                for i in 0..len {
                    units.push(data.add(i).read_volatile());
                }
                if seq.is_multiple_of(2) && (base as *const u32).read_volatile() == seq {
                    text = String::from_utf16_lossy(&units);
                    break;
                }
            }
            std::thread::yield_now();
        }
        unsafe {
            let _ = UnmapViewOfFile(view);
        }
    }
    unsafe {
        let _ = CloseHandle(map);
    }
    text
}

fn tell(hwnd: HWND, folder: &str) {
    let name = folder.rsplit('\\').next().unwrap_or(folder);
    let russian = unsafe { GetUserDefaultUILanguage() } & 0x3FF == 0x19;
    let text = if russian {
        format!("Zamok не даёт перенести папку на другой диск: файлы зашифрованной папки «{name}» оказались бы там без шифрования.\n\nЧтобы перенести её, расшифруйте папку навсегда, перенесите и зашифруйте снова.")
    } else {
        format!("Zamok does not let you move this folder to another drive: the files of the encrypted folder \"{name}\" would end up there unencrypted.\n\nTo move it, decrypt the folder permanently, move it and encrypt it again.")
    };
    unsafe {
        MessageBoxW(
            Some(hwnd),
            &HSTRING::from(text),
            w!("Zamok"),
            MB_OK | MB_ICONWARNING,
        );
    }
}

#[implement(ICopyHookW)]
struct Hook;

impl ICopyHookW_Impl for Hook_Impl {
    fn CopyCallback(
        &self,
        hwnd: HWND,
        wfunc: u32,
        wflags: u32,
        src: &PCWSTR,
        _src_attrs: u32,
        dest: &PCWSTR,
        _dest_attrs: u32,
    ) -> u32 {
        if wfunc != FO_MOVE || src.is_null() || dest.is_null() {
            return IDYES;
        }
        let (Ok(src), Ok(dest)) = (unsafe { src.to_string() }, unsafe { dest.to_string() }) else {
            return IDYES;
        };
        // Перенос внутри диска безопасен, в общую память за ним ходить незачем
        if drive(&dest).is_some() && drive(&dest) == drive(&src) {
            return IDYES;
        }
        let folders = read_folders();
        match blocked_by(&src, &dest, &folders) {
            Some(folder) => {
                if wflags & FOF_NOERRORUI == 0 {
                    tell(hwnd, folder);
                }
                IDNO
            }
            None => IDYES,
        }
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let hook: IUnknown = Hook.into();
        unsafe { hook.query(iid, object).ok() }
    }
    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

/// # Safety
/// Вызывает COM: указатели действительны по договору `DllGetClassObject`.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(riid, ppv)
    }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "D:\\Новая папка\nC:\\Работа\\Клиенты";

    #[test]
    fn folder_to_another_drive_is_refused() {
        assert_eq!(
            blocked_by(r"d:\новая ПАПКА", r"E:\Новая папка", LIST),
            Some(r"D:\Новая папка")
        );
    }

    #[test]
    fn parent_of_a_folder_is_refused_too() {
        assert_eq!(
            blocked_by(r"C:\Работа", r"E:\Работа", LIST),
            Some(r"C:\Работа\Клиенты")
        );
    }

    #[test]
    fn same_drive_and_other_folders_pass() {
        assert_eq!(
            blocked_by(r"C:\Работа\Клиенты", r"C:\Архив\Клиенты", LIST),
            None
        );
        assert_eq!(blocked_by(r"C:\Работа\Прочее", r"E:\Прочее", LIST), None);
        assert_eq!(
            blocked_by(r"C:\Работа\Клиенты2", r"E:\Клиенты2", LIST),
            None
        );
    }

    #[test]
    fn network_destination_counts_as_another_drive() {
        assert!(blocked_by(r"D:\Новая папка", r"\\srv\share\x", LIST).is_some());
    }

    #[test]
    fn empty_list_blocks_nothing() {
        assert_eq!(blocked_by(r"C:\", r"E:\x", ""), None);
    }
}
