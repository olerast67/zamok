//! zamok-core: фоновый процесс Zamok. Запускается при входе в Windows.
//!
//! Переменные для разработки и тестов:
//! - `ZAMOK_DATA_DIR`: папка со списком и журналом вместо `%LOCALAPPDATA%\Zamok`;
//! - `ZAMOK_PIPE`: имя канала вместо `\\.\pipe\zamok-<SID>`.
//!
//! `zamok-core --watch`: сторож для автозапуска. Запускает core и перезапускает его, если тот
//! упал. Обычный выход (из трея, по команде `quit`) и «уже запущен» сторож уважает.

// В сборке для людей без окна консоли
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use zamok_core::{autolock, core::Core, pipe};

/// Журнал больше этого при старте начинается заново.
const LOG_LIMIT: u64 = 1024 * 1024;

fn data_dir() -> anyhow::Result<PathBuf> {
    if let Some(d) = std::env::var_os("ZAMOK_DATA_DIR") {
        return Ok(PathBuf::from(d));
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA is not set"))?;
    Ok(PathBuf::from(base).join("Zamok"))
}

fn init_log(dir: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join("core.log");
    let too_big = std::fs::metadata(&file)
        .map(|m| m.len() > LOG_LIMIT)
        .unwrap_or(false);
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(&file)?;
    tracing_subscriber::fmt()
        .with_writer(Mutex::new(f))
        .with_target(false)
        .init();
    Ok(())
}

/// Код выхода «core уже запущен»: сторож не перезапускает.
const EXIT_ALREADY_RUNNING: i32 = 3;

/// Сторож: перезапускать core после падения, не чаще 5 раз за минуту.
fn watch() -> ! {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => std::process::exit(1),
    };
    let mut recent: Vec<std::time::Instant> = Vec::new();
    loop {
        let status = std::process::Command::new(&exe).status();
        match status.map(|s| s.code()) {
            Ok(Some(0)) | Ok(Some(EXIT_ALREADY_RUNNING)) => std::process::exit(0),
            _ => {}
        }
        let now = std::time::Instant::now();
        recent.retain(|t| now.duration_since(*t).as_secs() < 60);
        recent.push(now);
        let pause = if recent.len() > 5 { 60 } else { 2 };
        std::thread::sleep(std::time::Duration::from_secs(pause));
    }
}

/// `zamok-core --quit`: попросить запущенный core запереть папки и выйти (установщик вызывает
/// перед удалением и обновлением). Core не запущен: ничего не делать.
fn quit_running() -> ! {
    use std::io::{BufRead, Write};
    let name = std::env::var("ZAMOK_PIPE")
        .ok()
        .or_else(|| pipe::default_name().ok());
    if let Some(name) = name {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
        {
            let _ = f.write_all(b"{\"cmd\":\"quit\"}\n");
            let mut line = String::new();
            let _ = std::io::BufReader::new(f).read_line(&mut line);
            // Дождаться, пока канал исчезнет: значит, папки отключены
            for _ in 0..100 {
                if std::fs::metadata(&name).is_err() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
    std::process::exit(0);
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--watch") => watch(),
        Some("--quit") => quit_running(),
        _ => {}
    }
    if let Err(e) = run() {
        tracing::error!(error = %e, "zamok-core stopped");
        eprintln!("zamok-core: {e}");
        let code = if e.to_string().contains("already running") {
            EXIT_ALREADY_RUNNING
        } else {
            1
        };
        std::process::exit(code);
    }
}

fn run() -> anyhow::Result<()> {
    let dir = data_dir()?;
    init_log(&dir)?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "zamok-core starting");

    // Канал первым: если core уже запущен, второй экземпляр выходит, ничего не тронув
    let name = match std::env::var("ZAMOK_PIPE") {
        Ok(n) => n,
        Err(_) => pipe::default_name()?,
    };
    let listener = pipe::Listener::bind(&name)?;

    zamok_fs::mount::init_winfsp()?;
    // Значок запертой папки: собран при установке из значков этой Windows
    if let Some(icon) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("folder-locked.ico")))
        .filter(|p| p.is_file())
    {
        zamok_fs::fs::set_folder_icon(&icon.to_string_lossy());
    }
    if let Some(icon) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("folder-open.ico")))
        .filter(|p| p.is_file())
    {
        zamok_fs::fs::set_open_folder_icon(&icon.to_string_lossy());
    }
    let core = Core::start(&dir)?;
    autolock::install(core.clone())?;

    let pipe_core = core.clone();
    std::thread::spawn(move || pipe::serve(listener, pipe_core, autolock::post_quit));
    tracing::info!("ready");

    autolock::run_message_loop();
    core.shutdown();
    tracing::info!("zamok-core stopped");
    Ok(())
}
