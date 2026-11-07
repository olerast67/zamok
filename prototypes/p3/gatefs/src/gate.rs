//! P3: два состояния файловой системы без отключения.
//!
//! Заперто: в корне пусто, любые пути «не найдены», файлы, открытые до запирания,
//! получают отказ в доступе. Открыто: обычный passthrough.
//! Переключение: файл управления (путь в переменной P3_CONTROL) со словом
//! `locked` или `unlocked`. При запирании рядом пишется `<control>.open.txt`
//! со списком процессов, которые держали файлы открытыми.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use widestring::U16CStr;

pub static LOCKED: AtomicBool = AtomicBool::new(false);
pub static GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OPEN: Mutex<Option<HashMap<u64, (u32, String)>>> = Mutex::new(None);

pub fn locked() -> bool {
    LOCKED.load(Ordering::SeqCst)
}

pub fn is_root(name: &U16CStr) -> bool {
    let s = name.to_string_lossy();
    s.is_empty() || s == "\\"
}

fn current_pid() -> u32 {
    // Вне контекста операции WinFsp (асинхронные задачи) номера процесса нет: возвращаем 0
    unsafe {
        if winfsp_sys::FspFileSystemGetOperationContext().is_null() {
            return 0;
        }
        winfsp_sys::FspFileSystemOperationProcessIdF()
    }
}

/// Запомнить открытый файл: кто открыл и что. Возвращает номер записи.
pub fn register(name: &U16CStr) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let pid = current_pid();
    let mut g = OPEN.lock().unwrap();
    g.get_or_insert_with(HashMap::new)
        .insert(id, (pid, name.to_string_lossy()));
    id
}

/// P2-B: журнал обращений к корню (переменная P3_TRACE), чтобы отличить вход в папку от подглядывания.
pub fn trace(what: &str) {
    if let Ok(p) = std::env::var("P3_TRACE") {
        use std::io::Write;
        let pid = current_pid();
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{ms}	{}	{pid}	{what}", process_name(pid));
        }
    }
}

pub fn unregister(id: u64) {
    if let Some(m) = OPEN.lock().unwrap().as_mut() {
        m.remove(&id);
    }
}

fn process_name(pid: u32) -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return format!("pid {pid}");
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit('\\').next().unwrap_or(&full).to_string()
        } else {
            format!("pid {pid}")
        }
    }
}

fn open_report() -> String {
    let g = OPEN.lock().unwrap();
    let mut lines: Vec<String> = g
        .as_ref()
        .map(|m| {
            m.values()
                .filter(|(_, n)| n != "\\" && !n.is_empty())
                .map(|(pid, n)| format!("{}\t{}\t{}", process_name(*pid), pid, n))
                .collect()
        })
        .unwrap_or_default();
    lines.sort();
    lines.dedup();
    lines.join("\r\n")
}

fn notify_shell(mount: &str) {
    use windows::Win32::UI::Shell::{SHCNE_UPDATEDIR, SHCNF_PATHW, SHChangeNotify};
    let wide: Vec<u16> = mount.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { SHChangeNotify(SHCNE_UPDATEDIR, SHCNF_PATHW, Some(wide.as_ptr() as _), None) };
}

/// Поток, который следит за файлом управления.
pub fn spawn_watcher() {
    let Ok(control) = std::env::var("P3_CONTROL") else { return };
    let mount = std::env::var("P3_MOUNT").unwrap_or_default();
    let control = PathBuf::from(control);
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            let want = std::fs::read_to_string(&control).unwrap_or_default().trim().to_string();
            if want != last && (want == "locked" || want == "unlocked") {
                if want == "locked" {
                    let report = open_report();
                    LOCKED.store(true, Ordering::SeqCst);
                    GENERATION.fetch_add(1, Ordering::SeqCst);
                    let _ = std::fs::write(control.with_extension("open.txt"), report);
                } else {
                    LOCKED.store(false, Ordering::SeqCst);
                }
                if !mount.is_empty() {
                    notify_shell(&mount);
                }
                let _ = std::fs::write(control.with_extension("ack.txt"), &want);
                last = want;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}
