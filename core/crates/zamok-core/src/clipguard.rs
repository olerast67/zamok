//! Отмена «Вырезать» на папке Zamok.
//!
//! Проводник переносит папку Zamok копированием и удалением: открытая папка потеряла бы
//! шифрование (DESIGN.md, раздел 11). Перехватить сам перенос Windows не даёт, но «Вырезать»
//! сначала кладёт папку в буфер обмена с пометкой «перенос». Core замечает это и очищает
//! буфер: вставлять нечего. Обычное копирование не трогается. В журнал пути не пишутся.

use std::path::Path;

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

const CF_HDROP: u32 = 15;
const DROPEFFECT_MOVE: u32 = 2;

pub fn register(hwnd: HWND) {
    unsafe {
        let _ = AddClipboardFormatListener(hwnd);
    }
}

fn normalize(path: &str) -> String {
    path.trim_end_matches('\\').to_lowercase()
}

/// Какая папка Zamok оказалась среди вырезанного.
pub fn find_cut<'a>(cut: &[String], folders: &'a [String]) -> Option<&'a String> {
    folders
        .iter()
        .find(|f| cut.iter().any(|c| normalize(c) == normalize(f)))
}

fn is_move(format: u32) -> bool {
    let Ok(data) = (unsafe { GetClipboardData(format) }) else {
        return false;
    };
    let mem = HGLOBAL(data.0);
    if unsafe { GlobalSize(mem) } < 4 {
        return false;
    }
    let p = unsafe { GlobalLock(mem) } as *const u32;
    if p.is_null() {
        return false;
    }
    let effect = unsafe { p.read_unaligned() };
    unsafe {
        let _ = GlobalUnlock(mem);
    }
    effect & DROPEFFECT_MOVE != 0
}

fn dropped_paths(data: HANDLE) -> Vec<String> {
    let drop = HDROP(data.0);
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    let mut buf = vec![0u16; 32 * 1024];
    (0..count)
        .map(|i| {
            let n = unsafe { DragQueryFileW(drop, i, Some(&mut buf)) } as usize;
            String::from_utf16_lossy(&buf[..n.min(buf.len())])
        })
        .collect()
}

/// Открыть буфер: его может на миг держать другая программа.
fn open(hwnd: HWND) -> bool {
    for _ in 0..5 {
        if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// Буфер обмена изменился. Если в нём вырезанная папка Zamok, очистить его и вернуть её путь.
pub fn on_update(hwnd: HWND, folders: &[String]) -> Option<String> {
    if folders.is_empty() || unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_err() {
        return None;
    }
    let effect = unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) };
    if !open(hwnd) {
        return None;
    }
    let mut found = None;
    if is_move(effect) {
        if let Ok(data) = unsafe { GetClipboardData(CF_HDROP) } {
            found = find_cut(&dropped_paths(data), folders).cloned();
        }
    }
    unsafe {
        if found.is_some() {
            let _ = EmptyClipboard();
        }
        let _ = CloseClipboard();
    }
    found
}

/// Имя папки для уведомления.
pub fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_zamok_folder_itself_counts() {
        let folders = vec![r"D:\Новая папка".to_string()];
        let cut = |p: &str| vec![p.to_string()];
        assert!(find_cut(&cut(r"d:\НОВАЯ папка\"), &folders).is_some());
        assert!(find_cut(&cut(r"D:\Новая папка\файл.txt"), &folders).is_none());
        assert!(find_cut(&cut(r"D:\"), &folders).is_none());
    }

    #[test]
    fn name_for_the_notification() {
        assert_eq!(display_name(r"D:\Работа\Клиенты"), "Клиенты");
    }
}
