//! Чистка «Недавних файлов» при запирании. Windows кладёт в `Recent` ярлык на каждый открытый
//! файл, и в его имени видно имя файла из зашифрованной папки (DESIGN.md, раздел 6).
//!
//! Удаляются только файлы `.lnk` прямо в этой папке и только те, что ведут внутрь запертой
//! папки. В журнал пути и имена не пишутся.

use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// `ZAMOK_RECENT_DIR` задают тесты, чтобы не трогать настоящие ярлыки.
fn recent_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ZAMOK_RECENT_DIR") {
        return Some(d.into());
    }
    if std::env::var_os("ZAMOK_DATA_DIR").is_some() {
        return None;
    }
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join(r"Microsoft\Windows\Recent"))
}

/// Лежит ли `target` внутри `folder` (или это она сама).
pub fn is_inside(target: &str, folder: &Path) -> bool {
    let target = target.trim_end_matches('\\').to_lowercase();
    let folder = folder
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_lowercase();
    !folder.is_empty() && (target == folder || target.starts_with(&format!("{folder}\\")))
}

fn link_target(file: &Path) -> Option<String> {
    let mut buf = [0u16; 32 * 1024];
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        persist.Load(&HSTRING::from(file), STGM_READ).ok()?;
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
    }
    let n = buf.iter().position(|c| *c == 0)?;
    (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
}

/// Удалить ярлыки, которые ведут в `folder`. Возвращает, сколько удалено.
pub fn clean(folder: &Path) -> usize {
    let Some(dir) = recent_dir() else { return 0 };
    let Ok(items) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let mut removed = 0;
    for item in items.flatten() {
        let path = item.path();
        let is_link = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
        if is_link
            && link_target(&path).is_some_and(|t| is_inside(&t, folder))
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    if com {
        unsafe { CoUninitialize() };
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_means_the_folder_or_below_it() {
        let f = Path::new(r"D:\Новая папка");
        assert!(is_inside(r"d:\НОВАЯ папка\письмо.docx", f));
        assert!(is_inside(r"D:\Новая папка", f));
        assert!(!is_inside(r"D:\Новая папка 2\письмо.docx", f));
        assert!(!is_inside(r"D:\письмо.docx", f));
    }
}
