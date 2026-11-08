//! Значок в трее (DESIGN.md, раздел 2.5): замок закрытый, если все папки заперты, и с открытой
//! дужкой, если хоть одна открыта. Клик показывает меню: папки с состоянием, «Запереть всё»,
//! «Выйти из Zamok».
//!
//! Значки берутся из `zamok.ico` и `zamok-open.ico` рядом с программой. Переменная
//! `ZAMOK_NO_TRAY` отключает значок (тесты запускают много экземпляров core).

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics, LoadIconW,
    LoadImageW, PostMessageW, RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, HICON,
    IDI_APPLICATION, IMAGE_ICON, LR_LOADFROMFILE, MF_GRAYED, MF_SEPARATOR, MF_STRING, SM_CXSMICON,
    SM_CYSMICON, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
};

use crate::core::Core;
use crate::protocol::FolderState;

/// Сообщение от значка в трее окну core.
pub const WM_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const CMD_LOCK_ALL: usize = 1;
const CMD_QUIT: usize = 2;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Загруженные значки (закрытый, открытый): грузятся один раз.
static ICONS: [AtomicIsize; 2] = [AtomicIsize::new(0), AtomicIsize::new(0)];
/// Что сейчас показано: есть ли открытые папки и подсказка.
static SHOWN: Mutex<Option<(bool, String)>> = Mutex::new(None);

fn russian() -> bool {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    (unsafe { GetUserDefaultUILanguage() } & 0x3FF) == 0x19
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
    let name = if open { "zamok-open.ico" } else { "zamok.ico" };
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
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
        format!("Zamok: {} folders, {} open", folders.len(), open)
    };
    (open > 0, tip)
}

pub fn install(hwnd: HWND, core: &Core) {
    if std::env::var_os("ZAMOK_NO_TRAY").is_some() {
        return;
    }
    ENABLED.store(true, Ordering::SeqCst);
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
                let core = core.clone();
                let h = hwnd.0 as isize;
                std::thread::spawn(move || {
                    core.lock_all("tray");
                    refresh(HWND(h as *mut _), &core);
                });
            }
            CMD_QUIT => crate::autolock::post_quit(),
            _ => {}
        }
    }
}
