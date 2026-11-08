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
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetTimer, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, MSG, PBT_APMSUSPEND,
    WINDOW_EX_STYLE, WM_APP, WM_ENDSESSION, WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_TIMER,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_OVERLAPPED,
};

use crate::core::Core;

/// Класс окна: по нему тесты находят окно core и шлют ему те же сообщения, что и Windows.
pub const WINDOW_CLASS: PCWSTR = w!("ZamokCoreWindow");
/// Попросить цикл сообщений завершиться (из потока канала по команде `quit`).
pub const WM_ZAMOK_QUIT: u32 = WM_APP + 1;

const WTS_CONSOLE_DISCONNECT: usize = 0x2;
const WTS_REMOTE_DISCONNECT: usize = 0x4;
const WTS_SESSION_LOGOFF: usize = 0x6;
const WTS_SESSION_LOCK: usize = 0x7;
const IDLE_TIMER: usize = 1;
const IDLE_TICK_MS: u32 = 2000;

static CORE: OnceLock<Arc<Core>> = OnceLock::new();
static HWND_VALUE: AtomicIsize = AtomicIsize::new(0);
static IDLE_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

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
                core.shutdown();
                std::process::exit(0);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == IDLE_TIMER => {
            if !IDLE_CHECK_RUNNING.swap(true, Ordering::SeqCst) {
                let core = core.clone();
                std::thread::spawn(move || {
                    core.lock_idle();
                    IDLE_CHECK_RUNNING.store(false, Ordering::SeqCst);
                });
            }
            LRESULT(0)
        }
        WM_ZAMOK_QUIT => {
            unsafe { PostQuitMessage(0) };
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
        // Блокировка экрана и выход из учётной записи
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        // Сон, в том числе на ноутбуках с современным режимом ожидания
        let _ = RegisterSuspendResumeNotification(HANDLE(hwnd.0), DEVICE_NOTIFY_WINDOW_HANDLE);
        if SetTimer(Some(hwnd), IDLE_TIMER, IDLE_TICK_MS, None) == 0 {
            anyhow::bail!("SetTimer failed");
        }
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
