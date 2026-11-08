//! zamok-mount: подключить зашифрованную папку на путь (для разработки и тестов).
//!
//! zamok-mount <хранилище> <путь подключения (не должен существовать)> [имя папки]
//!
//! Пароль берётся из переменной ZAMOK_PASSWORD или спрашивается. Затем со стандартного
//! входа читаются команды по строке: `lock`, `unlock <пароль>`, `quit`.
//! В настоящем Zamok этим управляет фоновый процесс zamok-core.

use std::io::BufRead;
use std::path::PathBuf;

use zamok_fs::mount::{init_winfsp, Mounted};

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

    let password = match std::env::var("ZAMOK_PASSWORD") {
        Ok(p) => p,
        Err(_) => rpassword::prompt_password("Password: ")?,
    };
    init_winfsp()?;
    let m = Mounted::mount(&vault, &mount, &name)?;
    m.control()
        .unlock_with_password(&password)
        .map_err(|e| anyhow::anyhow!("cannot unlock: {e}"))?;
    drop(password);
    println!("mounted");

    for line in std::io::stdin().lock().lines() {
        let line = line?;
        // PowerShell с кодировкой UTF-8 ставит в начало ввода невидимый знак BOM
        let line = line.trim().trim_start_matches('\u{feff}');
        if line == "lock" {
            // Ответить «заперто» только после того, как ядро забыло сведения о файлах
            m.lock();
            println!("locked");
        } else if let Some(pw) = line.strip_prefix("unlock ") {
            match m.unlock_with_password(pw) {
                Ok(()) => println!("unlocked"),
                Err(e) => println!("unlock failed: {e}"),
            }
        } else if line == "quit" {
            break;
        } else if !line.is_empty() {
            println!("unknown command");
        }
    }
    drop(m);
    println!("unmounted");
    Ok(())
}
