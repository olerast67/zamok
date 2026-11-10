//! Пункты меню правой кнопки, которые видны только на нужных папках (прототип P2):
//! «Запереть» на открытых папках Zamok, «Открыть паролем» на запертых, а «Зашифровать паролем»
//! не показывается на папках Zamok. У каждого пункта в HKCU есть условие `AppliesTo`
//! со списком путей; core переписывает его, когда меняется состояние папок.
//!
//! Это отдельные пункты меню, действие папок по умолчанию не меняется (в P2 подмена именно
//! его ломала открытие всех папок). Работает только в настоящей установке: если пунктов нет
//! в реестре или задан `ZAMOK_DATA_DIR` (тесты), core реестр не трогает.

use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ,
};

use crate::protocol::{FolderInfo, FolderState};

const BASE: &str = r"Software\Classes\Directory\shell\";
/// Условие, которому не подходит ни одна папка.
const NOTHING: &str = r#"System.ItemPathDisplay:="::zamok-none::""#;

static LAST: Mutex<Option<[Option<String>; 4]>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn any_of(paths: &[&str]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    Some(
        paths
            .iter()
            .map(|p| format!(r#"System.ItemPathDisplay:="{p}""#))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Условия пунктов: (Запереть, Открыть паролем, Зашифровать, подменю Zamok). `None` у
/// «Зашифровать» значит «без условия, на всех папках».
pub fn conditions(folders: &[FolderInfo]) -> [Option<String>; 4] {
    let pick = |st: FolderState| -> Vec<&str> {
        folders
            .iter()
            .filter(|f| f.state == st)
            .map(|f| f.path.as_str())
            .collect()
    };
    let all: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
    [
        Some(any_of(&pick(FolderState::Unlocked)).unwrap_or_else(|| NOTHING.into())),
        Some(any_of(&pick(FolderState::Locked)).unwrap_or_else(|| NOTHING.into())),
        any_of(&all).map(|c| format!("NOT ({c})")),
        Some(any_of(&all).unwrap_or_else(|| NOTHING.into())),
    ]
}

fn enabled() -> bool {
    std::env::var_os("ZAMOK_DATA_DIR").is_none()
}

/// Записать условие пункта. `None`: убрать условие. Пункта нет в реестре: ничего не делать.
fn set_applies_to(verb: &str, value: Option<&str>) -> bool {
    let path = wide(&format!("{BASE}{verb}"));
    let mut key = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if opened.is_err() {
        return false;
    }
    let name = wide("AppliesTo");
    let ok = match value {
        Some(v) => {
            let data: Vec<u8> = wide(v).iter().flat_map(|u| u.to_le_bytes()).collect();
            unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)) }.is_ok()
        }
        None => {
            let r = unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())) };
            r.is_ok() || r.0 == 2 // нет значения: уже без условия
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    ok
}

/// Обновить условия, если состояние папок изменилось с прошлого раза.
pub fn update(folders: &[FolderInfo]) {
    if !enabled() {
        return;
    }
    let want = conditions(folders);
    let mut last = LAST.lock().unwrap();
    if last.as_ref() == Some(&want) {
        return;
    }
    let ok = set_applies_to("ZamokLock", want[0].as_deref())
        & set_applies_to("ZamokUnlock", want[1].as_deref())
        & set_applies_to("ZamokEncrypt", want[2].as_deref())
        & set_applies_to("ZamokMore", want[3].as_deref());
    // Пункты не установлены (запуск из папки сборки): не пытаться каждые 2 секунды
    *last = Some(want);
    if !ok {
        tracing::debug!("shell verbs are not installed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, state: FolderState) -> FolderInfo {
        FolderInfo {
            id: "x".into(),
            name: "x".into(),
            path: path.into(),
            state,
            idle_lock_secs: 0,
            asking: false,
            open_files: 0,
            open_in: Vec::new(),
            idle_asking: false,
            hello: false,
            error: None,
        }
    }

    #[test]
    fn conditions_split_folders_by_state() {
        let c = conditions(&[
            f(r"D:\Новая папка", FolderState::Unlocked),
            f(r"D:\Клиенты", FolderState::Locked),
            f(r"E:\Архив", FolderState::Locked),
        ]);
        assert_eq!(
            c[0].as_deref(),
            Some(r#"System.ItemPathDisplay:="D:\Новая папка""#)
        );
        assert_eq!(
            c[1].as_deref(),
            Some(r#"System.ItemPathDisplay:="D:\Клиенты" OR System.ItemPathDisplay:="E:\Архив""#)
        );
        assert!(c[2]
            .as_deref()
            .unwrap()
            .starts_with("NOT (System.ItemPathDisplay:="));
        assert!(
            c[3].as_deref().unwrap().contains("Новая папка")
                && c[3].as_deref().unwrap().contains("Архив")
        );
    }

    #[test]
    fn no_folders_means_lock_and_unlock_nowhere_encrypt_everywhere() {
        let c = conditions(&[]);
        assert_eq!(c[0].as_deref(), Some(NOTHING));
        assert_eq!(c[1].as_deref(), Some(NOTHING));
        assert_eq!(c[2], None);
        assert_eq!(c[3].as_deref(), Some(NOTHING));
    }
}
