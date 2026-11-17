//! Автозапирание: скрытое окно получает сообщения Windows о блокировке экрана, сне,
//! выходе из системы и выключении, а таймер раз в 2 секунды запирает забытые папки.
//!
//! Окно обычное (не message-only), но не показывается: сообщения о питании и о завершении
//! сеанса Windows рассылает только окнам верхнего уровня.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::RegisterSuspendResumeNotification;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW,
    PostQuitMessage, RegisterClassW, SetTimer, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, MSG,
    PBT_APMSUSPEND, WINDOW_EX_STYLE, WM_APP, WM_CLIPBOARDUPDATE, WM_ENDSESSION, WM_POWERBROADCAST,
    WM_QUERYENDSESSION, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_OVERLAPPED,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
};

use crate::core::Core;

/// Класс окна: по нему тесты находят окно core и шлют ему те же сообщения, что и Windows.
pub const WINDOW_CLASS: PCWSTR = w!("ZamokCoreWindow");
/// Попросить цикл сообщений завершиться (из потока канала по команде `quit`).
pub const WM_ZAMOK_QUIT: u32 = WM_APP + 1;
/// Ошибка «устройство отключено» при чтении хранилища: проверить флешки сейчас.
/// (`WM_APP + 2` занят значком в трее.)
const WM_ZAMOK_DEVICE: u32 = WM_APP + 3;
const WM_DEVICECHANGE: u32 = 0x0219;
const DBT_DEVICEARRIVAL: usize = 0x8000;
const DBT_DEVICEQUERYREMOVE: usize = 0x8001;
const DBT_DEVICEREMOVEPENDING: usize = 0x8003;
const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;
const DBT_DEVTYP_HANDLE: u32 = 6;

const WTS_CONSOLE_DISCONNECT: usize = 0x2;
const WTS_REMOTE_DISCONNECT: usize = 0x4;
const WTS_SESSION_LOGOFF: usize = 0x6;
const WTS_SESSION_LOCK: usize = 0x7;
const IDLE_TIMER: usize = 1;
const IDLE_TICK_MS: u32 = 2000;
const WM_HOTKEY: u32 = 0x0312;
const CLIP_TIMER: usize = 2;
const CLIP_DELAY_MS: u32 = 150;

static CORE: OnceLock<Arc<Core>> = OnceLock::new();
static HWND_VALUE: AtomicIsize = AtomicIsize::new(0);
static IDLE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);
static DEVICE_CHECK_POSTED: AtomicBool = AtomicBool::new(false);

/// Окно core (0, пока не создано): ему Windows шлёт «устройство сейчас извлекут».
pub fn window() -> isize {
    HWND_VALUE.load(Ordering::SeqCst)
}

/// Файловая система получила ошибку «устройство отключено»: попросить окно проверить флешки.
/// Одно сообщение на пачку ошибок.
fn post_device_check() {
    let h = window();
    if h != 0 && !DEVICE_CHECK_POSTED.swap(true, Ordering::SeqCst) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(h as *mut _)),
                WM_ZAMOK_DEVICE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Дескриптор из `DEV_BROADCAST_HANDLE`, если сообщение о нём. `lparam` от Windows указывает
/// на заголовок `DEV_BROADCAST_HDR`; ноль (так шлёт тест) значит «без подробностей».
fn broadcast_handle(lparam: LPARAM) -> Option<isize> {
    use windows::Win32::UI::WindowsAndMessaging::{DEV_BROADCAST_HANDLE, DEV_BROADCAST_HDR};
    if lparam.0 == 0 {
        return None;
    }
    // SAFETY: для WM_DEVICECHANGE с этими кодами Windows передаёт указатель на заголовок
    unsafe {
        let hdr = &*(lparam.0 as *const DEV_BROADCAST_HDR);
        if hdr.dbch_devicetype.0 != DBT_DEVTYP_HANDLE {
            return None;
        }
        let h = &*(lparam.0 as *const DEV_BROADCAST_HANDLE);
        Some(h.dbch_handle.0 as isize)
    }
}

/// Стереть ключи сразу, а ожидание сброса кэша ядра увести в отдельный поток,
/// чтобы не держать Windows (перед сном на всё около двух секунд).
fn lock_all_fast(core: &Arc<Core>, reason: &'static str) {
    let locked = core.lock_all_now(reason);
    if !locked.is_empty() {
        std::thread::spawn(move || Core::settle(&locked));
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(core) = CORE.get() else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    match msg {
        WM_HOTKEY => {
            lock_all_fast(core, "hotkey");
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            // Не сразу: программа, которая кладёт данные, ещё работает с буфером обмена,
            // а открыть его может только кто-то один. Человек за это время вставить не успеет.
            unsafe { SetTimer(Some(hwnd), CLIP_TIMER, CLIP_DELAY_MS, None) };
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CLIP_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), CLIP_TIMER);
            }
            // Папки на флешках не в счёт: на их пути только файл открытия, перенос ничего не
            // расшифровывает (DESIGN.md, раздел 3.6)
            let folders: Vec<String> = core
                .status()
                .into_iter()
                .filter(|f| !f.letter)
                .map(|f| f.path)
                .collect();
            if let Some(path) = crate::clipguard::on_update(hwnd, &folders) {
                tracing::info!("cut of a zamok folder cancelled");
                crate::tray::notify_cut(&crate::clipguard::display_name(&path));
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 {
                WTS_SESSION_LOCK if core.lock_on_screen_lock() => {
                    lock_all_fast(core, "screen locked")
                }
                WTS_SESSION_LOGOFF => lock_all_fast(core, "logoff"),
                // Смена пользователя, отключение удалённого рабочего стола: за компьютером
                // может оказаться другой человек, запираем всегда, как при выходе
                WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                    lock_all_fast(core, "session disconnected")
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 == PBT_APMSUSPEND as usize {
                lock_all_fast(core, "suspend");
            }
            LRESULT(1)
        }
        WM_QUERYENDSESSION => {
            lock_all_fast(core, "end session");
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                // Сеанс точно завершается: отключить всё и выйти, пока Windows ждёт
                crate::tray::remove(hwnd);
                core.shutdown();
                std::process::exit(0);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == IDLE_TIMER => {
            crate::tray::refresh(hwnd, core);
            let folders = core.status();
            crate::shellhook::publish(&folders);
            crate::shellverbs::update(&folders);
            if !IDLE_CHECK_RUNNING.swap(true, Ordering::SeqCst) {
                let core = core.clone();
                std::thread::spawn(move || {
                    core.check_folders();
                    core.retry_leftovers();
                    for ev in core.lock_idle() {
                        crate::tray::notify_idle(&ev);
                    }
                    IDLE_CHECK_RUNNING.store(false, Ordering::SeqCst);
                });
            }
            LRESULT(0)
        }
        WM_DEVICECHANGE => {
            match wparam.0 {
                // «Извлечь»: Windows ждёт ответа, поэтому запираем здесь же. Дескриптор папки
                // хранилища закрывается вместе с подпиской, и извлечение проходит
                DBT_DEVICEQUERYREMOVE => core.drive_query_remove(broadcast_handle(lparam)),
                DBT_DEVICEREMOVEPENDING | DBT_DEVICEREMOVECOMPLETE => {
                    let core = core.clone();
                    std::thread::spawn(move || core.drives_changed(false));
                }
                DBT_DEVICEARRIVAL => {
                    let core = core.clone();
                    std::thread::spawn(move || core.drives_changed(true));
                }
                _ => {}
            }
            LRESULT(1)
        }
        WM_ZAMOK_DEVICE => {
            DEVICE_CHECK_POSTED.store(false, Ordering::SeqCst);
            let core = core.clone();
            std::thread::spawn(move || core.drives_changed(false));
            LRESULT(0)
        }
        WM_ZAMOK_QUIT => {
            crate::tray::remove(hwnd);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        crate::tray::WM_TRAY => {
            crate::tray::on_message(hwnd, lparam, core);
            LRESULT(0)
        }
        m if crate::tray::is_taskbar_created(m) => {
            crate::tray::on_taskbar_created(hwnd, core);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Создать скрытое окно и подписаться на события. Вызывать из потока, который потом
/// крутит [`run_message_loop`].
pub fn install(core: Arc<Core>) -> anyhow::Result<()> {
    let _ = CORE.set(core);
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            anyhow::bail!("RegisterClassW failed");
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WINDOW_CLASS,
            w!("Zamok"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        HWND_VALUE.store(hwnd.0 as isize, Ordering::SeqCst);
        zamok_fs::fs::set_device_gone_hook(post_device_check);
        // Блокировка экрана и выход из учётной записи
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        crate::clipguard::register(hwnd);
        // Ctrl+Alt+Shift+L: запереть всё. Сочетание занято другой программой: живём без него.
        // В тестах не занимаем, сообщение им шлёт сам тест.
        if std::env::var_os("ZAMOK_DATA_DIR").is_none() {
            let _ = RegisterHotKey(
                Some(hwnd),
                1,
                MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                u32::from(b'L'),
            );
        }
        // Сон, в том числе на ноутбуках с современным режимом ожидания
        let _ = RegisterSuspendResumeNotification(HANDLE(hwnd.0), DEVICE_NOTIFY_WINDOW_HANDLE);
        if SetTimer(Some(hwnd), IDLE_TIMER, IDLE_TICK_MS, None) == 0 {
            anyhow::bail!("SetTimer failed");
        }
        crate::tray::install(hwnd, CORE.get().unwrap());
    }
    Ok(())
}

/// Попросить цикл сообщений завершиться (из любого потока).
pub fn post_quit() {
    let h = HWND_VALUE.load(Ordering::SeqCst);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_ZAMOK_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn run_message_loop() {
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
