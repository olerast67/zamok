//! Путь папки вне индекса поиска Windows (DESIGN.md, раздел 6).
//!
//! Все файлы открытой папки и так отдаются с атрибутом «не индексировать», но с ним поиск
//! Windows всё равно запоминает имена и свойства файлов, если путь входит в область
//! индексирования (по умолчанию это профиль пользователя). Поэтому core добавляет правило
//! исключения `file:///<путь>\*` через `ISearchCrawlScopeManager`, как это делает кнопка
//! «Изменить» в «Параметрах индексирования».
//!
//! Правило добавляется, только если путь сейчас входит в область индексирования: для папок
//! вне профиля ничего не меняется. Отказ (например, политика или нет службы поиска) не мешает
//! работе: остаётся атрибут. В журнал идут только коды ошибок, без путей.
//!
//! `ZAMOK_SEARCH_DRY_RUN`: только проверить, правило не добавлять и не удалять (тесты не
//! должны менять настройки поиска на компьютере, где их запускают).

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Search::{CSearchManager, ISearchCrawlScopeManager, ISearchManager};

fn dry_run() -> bool {
    std::env::var_os("ZAMOK_SEARCH_DRY_RUN").is_some()
}

/// Адрес папки для проверки: `file:///C:\a\b\`.
fn folder_url(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("file:///{}\\", s.trim_end_matches('\\'))
}

/// Правило исключения папки и всего внутри: `file:///C:\a\b\*`.
fn exclusion_rule(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("file:///{}\\*", s.trim_end_matches('\\'))
}

/// Работа с менеджером области индексирования в своём потоке COM.
fn with_scope<T>(
    f: impl FnOnce(&ISearchCrawlScopeManager) -> windows::core::Result<T>,
) -> windows::core::Result<T> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| {
            let manager: ISearchManager =
                CoCreateInstance(&CSearchManager, None, CLSCTX_LOCAL_SERVER)?;
            let catalog = manager.GetCatalog(&HSTRING::from("SystemIndex"))?;
            let scope = catalog.GetCrawlScopeManager()?;
            f(&scope)
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// Входит ли папка сейчас в область индексирования.
pub fn included(path: &Path) -> windows::core::Result<bool> {
    let url = HSTRING::from(folder_url(path));
    with_scope(|s| unsafe { s.IncludedInCrawlScope(PCWSTR(url.as_ptr())) }.map(|b| b.as_bool()))
}

/// Убрать папку из области индексирования, если она в неё входит. `Ok(true)`: правило добавлено.
pub fn exclude(path: &Path) -> windows::core::Result<bool> {
    let url = HSTRING::from(folder_url(path));
    let rule = HSTRING::from(exclusion_rule(path));
    let dry = dry_run();
    with_scope(|s| unsafe {
        if !s.IncludedInCrawlScope(PCWSTR(url.as_ptr()))?.as_bool() || dry {
            return Ok(false);
        }
        s.AddUserScopeRule(PCWSTR(rule.as_ptr()), false, false, 0)?;
        s.SaveAll()?;
        Ok(true)
    })
}

/// Папка больше не зашифрована (убрана из списка или расшифрована навсегда): снять правило,
/// если папку исключает именно правило на ней, а родитель в область входит. Иначе ничего.
pub fn forget(path: &Path) -> windows::core::Result<bool> {
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    let url = HSTRING::from(folder_url(path));
    let parent_url = HSTRING::from(folder_url(parent));
    let rule = HSTRING::from(exclusion_rule(path));
    let dry = dry_run();
    with_scope(|s| unsafe {
        let ours = !s.IncludedInCrawlScope(PCWSTR(url.as_ptr()))?.as_bool()
            && s.IncludedInCrawlScope(PCWSTR(parent_url.as_ptr()))?
                .as_bool();
        if !ours || dry {
            return Ok(false);
        }
        s.RemoveScopeRule(PCWSTR(rule.as_ptr()))?;
        s.SaveAll()?;
        Ok(true)
    })
}

/// [`exclude`] в отдельном потоке: служба поиска может отвечать долго.
pub fn exclude_later(id: String, path: PathBuf) {
    std::thread::spawn(move || match exclude(&path) {
        Ok(changed) => tracing::info!(folder = %id, changed, "search index scope checked"),
        Err(e) => tracing::warn!(folder = %id, code = %e.code(), "search index scope not changed"),
    });
}

/// [`forget`] в отдельном потоке.
pub fn forget_later(id: String, path: PathBuf) {
    std::thread::spawn(move || match forget(&path) {
        Ok(changed) => tracing::info!(folder = %id, changed, "search index rule checked"),
        Err(e) => tracing::warn!(folder = %id, code = %e.code(), "search index rule not removed"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_rules() {
        assert_eq!(folder_url(Path::new(r"C:\a\b")), r"file:///C:\a\b\");
        assert_eq!(folder_url(Path::new(r"C:\a\b\")), r"file:///C:\a\b\");
        assert_eq!(exclusion_rule(Path::new(r"C:\a\b")), r"file:///C:\a\b\*");
    }

    /// Только чтение: проверка не меняет настройки поиска. Без службы поиска пропускается.
    #[test]
    fn scope_can_be_read() {
        let d = tempfile::tempdir().unwrap();
        if let Err(e) = included(d.path()) {
            eprintln!("search service not available: {e}");
        }
    }
}
