//! Какие программы держат файлы папки открытыми: окна пишут «Открыто в: WINWORD.EXE».
//!
//! Номера процессов даёт файловая система (WinFsp сообщает процесс при открытии файла),
//! здесь они превращаются в имена. Только имя файла программы, без пути: путь к программе
//! может содержать имя пользователя или папки, а в окне он не нужен. В журнал имена не пишутся.

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Больше имён окну не нужно: длинный список всё равно не прочитать.
pub const MAX_NAMES: usize = 5;

/// Имена программ по номерам процессов. Процесс уже завершился или закрыт для этого
/// пользователя: его нет в списке.
pub fn names(pids: &[u32]) -> Vec<String> {
    tidy(pids.iter().filter_map(|&pid| image_path(pid)).collect())
}

fn image_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: буфер живёт до конца вызова, длина передаётся вместе с ним, дескриптор закрывается
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Из полных путей: только имена файлов, без повторов (без учёта регистра), по алфавиту,
/// не больше [`MAX_NAMES`].
fn tidy(paths: Vec<String>) -> Vec<String> {
    let mut names: Vec<String> = paths
        .iter()
        .filter_map(|p| p.rsplit(['\\', '/']).next())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names.truncate(MAX_NAMES);
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_file_names_without_repeats() {
        let got = tidy(vec![
            r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE".into(),
            r"C:\Windows\explorer.exe".into(),
            r"C:\Program Files\Microsoft Office\root\Office16\winword.exe".into(),
            r"C:\Users\Имя\AppData\Local\Programs\app\Редактор.exe".into(),
            String::new(),
        ]);
        assert_eq!(got, vec!["explorer.exe", "WINWORD.EXE", "Редактор.exe"]);
        assert!(got.iter().all(|n| !n.contains('\\')));
    }

    #[test]
    fn list_is_capped() {
        let many = (0..20).map(|i| format!(r"C:\p\app{i:02}.exe")).collect();
        assert_eq!(tidy(many).len(), MAX_NAMES);
    }

    #[test]
    fn own_process_has_a_name_and_gone_ones_do_not() {
        let me = names(&[std::process::id(), 0]);
        assert_eq!(me.len(), 1);
        assert!(me[0].to_lowercase().ends_with(".exe"), "{me:?}");
        assert!(!me[0].contains('\\'));
    }
}
