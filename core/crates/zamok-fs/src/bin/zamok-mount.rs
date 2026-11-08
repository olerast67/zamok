//! zamok-mount: подключить зашифрованную папку на путь (для разработки и тестов).
//!
//! zamok-mount <хранилище> <путь подключения (не должен существовать)> [имя папки]
//!
//! Пароль берётся из переменной ZAMOK_PASSWORD или спрашивается. Затем со стандартного
//! входа читаются команды по строке: `lock`, `unlock <пароль>`, `quit`.
//! В настоящем Zamok этим будет управлять фоновый процесс zamok-core.

use std::io::BufRead;
use std::path::PathBuf;

use winfsp::host::{FileSystemHost, FileSystemParams, VolumeParams};
use zamok_fs::fs::ZamokFs;

fn notify_shell(path: &std::path::Path) {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_UPDATEDIR, SHCNF_PATHW};
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe { SHChangeNotify(SHCNE_UPDATEDIR, SHCNF_PATHW, Some(wide.as_ptr() as _), None) };
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: zamok-mount <vault> <mount point> [display name]");
        std::process::exit(2);
    }
    let vault = PathBuf::from(&args[1]);
    let mount = PathBuf::from(&args[2]);
    let name = args.get(3).cloned().unwrap_or_else(|| {
        mount
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Zamok".into())
    });

    let fs = ZamokFs::new(&vault, &name)?;
    let control = fs.control();
    let password = match std::env::var("ZAMOK_PASSWORD") {
        Ok(p) => p,
        Err(_) => rpassword::prompt_password("Password: ")?,
    };
    control
        .unlock_with_password(&password)
        .map_err(|e| anyhow::anyhow!("cannot unlock: {e}"))?;
    drop(password);

    let init =
        winfsp::winfsp_init().map_err(|e| anyhow::anyhow!("WinFsp is not installed: {e:?}"))?;
    let _ = init;
    let mut vp = VolumeParams::new();
    vp.filesystem_name("Zamok")
        .sector_size(4096)
        .sectors_per_allocation_unit(1)
        .max_component_length(255)
        .case_sensitive_search(false)
        .case_preserved_names(true)
        .unicode_on_disk(true)
        .persistent_acls(true)
        .post_cleanup_when_modified_only(true)
        .flush_and_purge_on_cleanup(true)
        // Кэш сведений о файлах на 1 секунду. При запирании он сбрасывается уведомлениями
        // по всем путям, которые ядро могло закэшировать (находка прототипа P3).
        .file_info_timeout(1000);
    let params = FileSystemParams {
        use_dir_info_by_name: false,
        volume_params: vp,
        debug_mode: Default::default(),
    };
    let mut host: FileSystemHost<ZamokFs> =
        FileSystemHost::new_with_timer::<Vec<String>, 50>(params, fs)
            .map_err(|e| anyhow::anyhow!("host: {e:?}"))?;
    host.mount(mount.as_os_str())
        .map_err(|e| anyhow::anyhow!("mount: {e:?}"))?;
    host.start().map_err(|e| anyhow::anyhow!("start: {e:?}"))?;
    // Атрибут «системный» на точке подключения: Проводник читает desktop.ini (иконка замка)
    if let Ok(m) = std::fs::metadata(&mount) {
        use std::os::windows::fs::MetadataExt;
        let _ = zamok_fs::attrs::set_attributes(&mount, (m.file_attributes() | 0x4) & 0x7);
    }
    println!("mounted");

    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let line = line.trim();
        if line == "lock" {
            control.lock();
            // Ответить «заперто» только после того, как ядру ушли все уведомления
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while control.invalidation_pending() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            // Не всё удалось сбросить уведомлениями (или не успели): ждём, пока кэш ядра
            // (FileInfoTimeout = 1 с) истечёт сам
            if control.take_fallback_wait() || control.invalidation_pending() {
                std::thread::sleep(std::time::Duration::from_millis(1200));
            }
            notify_shell(&mount);
            println!("locked");
        } else if let Some(pw) = line.strip_prefix("unlock ") {
            match control.unlock_with_password(pw) {
                Ok(()) => {
                    notify_shell(&mount);
                    println!("unlocked");
                }
                Err(e) => println!("unlock failed: {e}"),
            }
        } else if line == "quit" {
            break;
        } else if !line.is_empty() {
            println!("unknown command");
        }
    }
    control.lock();
    host.stop();
    host.unmount();
    println!("unmounted");
    Ok(())
}
