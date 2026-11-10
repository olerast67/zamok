//! Значок в трее (DESIGN.md, раздел 2.5): замок закрытый, если все папки заперты, и с открытой
//! дужкой, если хоть одна открыта. Клик показывает меню: папки с состоянием, «Запереть всё»,
//! «Выйти из Zamok».
//!
//! Значки берутся из `zamok.ico` и `zamok-open.ico` рядом с программой. Переменная
//! `ZAMOK_NO_TRAY` отключает значок (тесты запускают много экземпляров core).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics, LoadIconW,
    LoadImageW, MessageBoxW, PostMessageW, RegisterWindowMessageW, SetForegroundWindow,
    TrackPopupMenu, HICON, IDI_APPLICATION, IDOK, IMAGE_ICON, LR_LOADFROMFILE, MB_DEFBUTTON2,
    MB_ICONWARNING, MB_OKCANCEL, MB_SETFOREGROUND, MB_TOPMOST, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    SM_CXSMICON, SM_CYSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
};

use crate::core::Core;
use crate::protocol::{ErrorCode, FolderState, JobInfo, JobState};

/// Сообщение от значка в трее окну core.
pub const WM_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const CMD_LOCK_ALL: usize = 1;
const CMD_QUIT: usize = 2;
const CMD_SETTINGS: usize = 3;
/// Окно core, к значку которого привязаны уведомления.
static HWND_FOR_NOTIFY: AtomicIsize = AtomicIsize::new(0);

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Загруженные значки (закрытый, открытый): грузятся один раз.
static ICONS: [AtomicIsize; 2] = [AtomicIsize::new(0), AtomicIsize::new(0)];
/// Что сейчас показано: есть ли открытые папки и подсказка.
static SHOWN: Mutex<Option<(bool, String)>> = Mutex::new(None);
/// Работы, которые при прошлой проверке ещё шли: их конец надо показать уведомлением.
static RUNNING_JOBS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn russian() -> bool {
    crate::howto::user_is_russian()
}

fn t(ru: &'static str, en: &'static str) -> &'static str {
    if russian() {
        ru
    } else {
        en
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn load_icon(open: bool) -> HICON {
    let slot = &ICONS[open as usize];
    let cached = slot.load(Ordering::SeqCst);
    if cached != 0 {
        return HICON(cached as *mut _);
    }
    let h = load_icon_file(open);
    slot.store(h.0 as isize, Ordering::SeqCst);
    h
}

fn load_icon_file(open: bool) -> HICON {
    // Значок программы: замок закрыт или с поднятой дужкой; системные собранные запасные
    let names = if open {
        ["zamok-open.ico", "tray-open.ico"]
    } else {
        ["zamok.ico", "tray-locked.ico"]
    };
    let dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.to_path_buf()));
    for path in dir
        .iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .filter(|p| p.is_file())
    {
        let w = wide(&path.to_string_lossy());
        let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
        if let Ok(h) = unsafe {
            LoadImageW(
                None,
                PCWSTR(w.as_ptr()),
                IMAGE_ICON,
                cx,
                cy,
                LR_LOADFROMFILE,
            )
        } {
            return HICON(h.0);
        }
    }
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

fn data(hwnd: HWND, open: bool, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: load_icon(open),
        ..Default::default()
    };
    for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *dst = src;
    }
    nid
}

fn summary(core: &Core) -> (bool, String) {
    let folders = core.status();
    let open = folders
        .iter()
        .filter(|f| f.state == FolderState::Unlocked)
        .count();
    let tip = if russian() {
        format!("Zamok: папок {}, открыто {}", folders.len(), open)
    } else {
        format!("Zamok: folders {}, open {}", folders.len(), open)
    };
    (open > 0, tip)
}

pub fn install(hwnd: HWND, core: &Core) {
    if std::env::var_os("ZAMOK_NO_TRAY").is_some() {
        return;
    }
    ENABLED.store(true, Ordering::SeqCst);
    HWND_FOR_NOTIFY.store(hwnd.0 as isize, Ordering::SeqCst);
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        Ordering::SeqCst,
    );
    add(hwnd, core);
}

fn add(hwnd: HWND, core: &Core) {
    let (open, tip) = summary(core);
    *SHOWN.lock().unwrap() = Some((open, tip.clone()));
    let nid = data(hwnd, open, &tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

/// Обновить значок и подсказку, если что-то изменилось (по таймеру и после действий).
pub fn refresh(hwnd: HWND, core: &Core) {
    if !ENABLED.load(Ordering::SeqCst) {
        return;
    }
    notify_finished_jobs(core);
    let (open, tip) = summary(core);
    {
        let mut shown = SHOWN.lock().unwrap();
        if shown.as_ref() == Some(&(open, tip.clone())) {
            return;
        }
        *shown = Some((open, tip.clone()));
    }
    let nid = data(hwnd, open, &tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn remove(hwnd: HWND) {
    if !ENABLED.load(Ordering::SeqCst) {
        return;
    }
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Уведомление у значка в трее (Windows 10 и 11 показывают его как обычное уведомление).
pub fn notify(title: &str, text: &str) {
    if !ENABLED.load(Ordering::SeqCst) {
        return;
    }
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(HWND_FOR_NOTIFY.load(Ordering::SeqCst) as *mut _),
        uID: TRAY_ID,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    for (dst, src) in nid
        .szInfoTitle
        .iter_mut()
        .zip(title.encode_utf16().take(63))
    {
        *dst = src;
    }
    for (dst, src) in nid.szInfo.iter_mut().zip(text.encode_utf16().take(255)) {
        *dst = src;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// Имя папки для заголовка: заголовок уведомления ограничен 63 знаками, и хвост
/// «заперта» не должен потеряться за длинным именем.
fn short(name: &str) -> String {
    const MAX: usize = 32;
    if name.chars().count() <= MAX {
        name.to_string()
    } else {
        name.chars()
            .take(MAX - 1)
            .chain(std::iter::once('…'))
            .collect()
    }
}

/// Сказать о конце шифрования или расшифровки: окно могли закрыть, а работа шла дальше.
fn notify_finished_jobs(core: &Core) {
    let jobs = core.jobs();
    let was = {
        let mut seen = RUNNING_JOBS.lock().unwrap();
        let running = jobs
            .iter()
            .filter(|j| j.state == JobState::Running)
            .map(|j| j.id.clone())
            .collect();
        seen.replace(running).unwrap_or_default()
    };
    let finished: Vec<&JobInfo> = jobs
        .iter()
        .filter(|j| j.state != JobState::Running && was.contains(&j.id))
        .collect();
    if finished.is_empty() {
        return;
    }
    let folders = core.status();
    for j in finished {
        // Шифрование оставляет папку в списке, расшифровка убирает
        let encrypted = folders.iter().any(|f| f.path.eq_ignore_ascii_case(&j.path));
        notify_job_end(j, encrypted);
    }
}

fn notify_job_end(job: &JobInfo, encrypted: bool) {
    let name = short(&crate::clipguard::display_name(&job.path));
    match (job.state, job.error) {
        (JobState::Done, _) if encrypted => notify(
            &if russian() { format!("Папка «{name}» зашифрована") } else { format!("\u{201c}{name}\u{201d} is encrypted") },
            t("Открыть и запереть её можно из меню папки или значка Zamok.", "Open and lock it from the folder menu or the Zamok icon."),
        ),
        (JobState::Done, _) => notify(
            &if russian() { format!("Папка «{name}» расшифрована") } else { format!("\u{201c}{name}\u{201d} is decrypted") },
            t("Она снова обычная.", "It is an ordinary folder again."),
        ),
        // Отмену человек нажал сам
        (_, Some(ErrorCode::Cancelled)) => {}
        _ => notify(
            &if russian() { format!("Папка «{name}»: работа не завершена") } else { format!("\u{201c}{name}\u{201d}: not finished") },
            t("Шифрование или расшифровка не закончились, папка осталась как была. Файлы на месте.", "Encryption or decryption did not finish; the folder stays as it was. The files are in place."),
        ),
    }
}

pub fn notify_cut(name: &str) {
    notify(
        &if russian() { format!("Папку «{}» нельзя вырезать", short(name)) } else { format!("\u{201c}{}\u{201d} cannot be cut", short(name)) },
        t(
            "Проводник перенёс бы её файлы без шифрования. Чтобы переместить папку, расшифруйте её, перенесите и зашифруйте снова.",
            "Explorer would move its files unencrypted. To move the folder, decrypt it, move it and encrypt it again.",
        ),
    );
}

pub fn notify_idle(ev: &crate::core::IdleEvent) {
    use crate::core::IdleEvent;
    match ev {
        IdleEvent::Locked { name, minutes } => notify(
            &if russian() { format!("Папка «{}» заперта", short(name)) } else { format!("\u{201c}{}\u{201d} is locked", short(name)) },
            &if russian() {
                format!("С ней не работали {minutes} мин. Чтобы открыть, дважды щёлкните значок замка в папке.")
            } else {
                format!("It was not used for {minutes} min. To open it, double-click the padlock inside the folder.")
            },
        ),
        IdleEvent::Waiting { name, .. } => notify(
            &if russian() { format!("Папка «{}» пока открыта", short(name)) } else { format!("\u{201c}{}\u{201d} stays open for now", short(name)) },
            t(
                "В ней открыт файл в другой программе. Zamok запрёт папку, когда файл закроют.",
                "A file from it is open in another program. Zamok locks the folder when the file is closed.",
            ),
        ),
    }
}

/// Проводник перезапустился: значки трея надо добавить заново.
pub fn is_taskbar_created(msg: u32) -> bool {
    let id = TASKBAR_CREATED.load(Ordering::SeqCst);
    id != 0 && msg == id
}

pub fn on_taskbar_created(hwnd: HWND, core: &Core) {
    if ENABLED.load(Ordering::SeqCst) {
        add(hwnd, core);
    }
}

/// Спросить в системном окне; `true`, если человек нажал ОК. Отмена выбрана заранее.
fn confirm(text: &str) -> bool {
    let body = wide(text);
    let title = wide("Zamok");
    let answer = unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    answer == IDOK
}

/// Окно настроек: Zamok.exe рядом с core.
fn open_settings() {
    if let Some(exe) = crate::core::ui_exe() {
        let _ = std::process::Command::new(exe).arg("--settings").spawn();
    }
}

/// Сообщение от значка: по щелчку любой кнопкой показать меню.
pub fn on_message(hwnd: HWND, lparam: LPARAM, core: &Arc<Core>) {
    use windows::Win32::UI::WindowsAndMessaging::{WM_LBUTTONUP, WM_RBUTTONUP};
    let event = (lparam.0 & 0xFFFF) as u32;
    if event != WM_LBUTTONUP && event != WM_RBUTTONUP {
        return;
    }
    let folders = core.status();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        if folders.is_empty() {
            let s = wide(t("Зашифрованных папок нет", "No encrypted folders"));
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(s.as_ptr()));
        }
        for f in &folders {
            let state = match f.state {
                FolderState::Unlocked => t("открыта", "open"),
                FolderState::Locked => t("заперта", "locked"),
                FolderState::Unavailable => t("недоступна", "unavailable"),
            };
            let s = wide(&format!("{}: {}", f.name, state));
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(s.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let any_open = folders.iter().any(|f| f.state == FolderState::Unlocked);
        let lock = wide(t("Запереть всё", "Lock all"));
        let flags = if any_open {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        let _ = AppendMenuW(menu, flags, CMD_LOCK_ALL, PCWSTR(lock.as_ptr()));
        let settings = wide(t("Настройки", "Settings"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, PCWSTR(settings.as_ptr()));
        let quit = wide(t(
            "Выйти из Zamok (запереть и отключить папки)",
            "Quit Zamok (lock and unmount folders)",
        ));
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, PCWSTR(quit.as_ptr()));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Без этого меню не закрывается щелчком мимо него
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            CMD_LOCK_ALL => {
                let open_files: usize = folders.iter().map(|f| core.open_files(&f.path)).sum();
                if open_files > 0
                    && !confirm(t(
                        "В папках есть открытые файлы. Несохранённые изменения пропадут: программа не сможет их записать. Запереть всё равно?",
                        "Some files in the folders are in use. Unsaved changes will be lost: the program will not be able to save them. Lock anyway?",
                    ))
                {
                    return;
                }
                let core = core.clone();
                let h = hwnd.0 as isize;
                std::thread::spawn(move || {
                    core.lock_all("tray");
                    refresh(HWND(h as *mut _), &core);
                });
            }
            CMD_QUIT => {
                if confirm(t(
                    "Все папки будут заперты и исчезнут из Проводника, пока Zamok снова не запустится. Он запустится при входе в Windows или когда вы откроете любое окно Zamok. Выйти?",
                    "All folders will be locked and disappear from Explorer until Zamok runs again. It starts at the next sign-in or when you open any Zamok window. Quit?",
                )) {
                    crate::autolock::post_quit();
                }
            }
            CMD_SETTINGS => open_settings(),
            _ => {}
        }
    }
}
