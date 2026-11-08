//! zamok-recover: выгрузить расшифрованную копию зашифрованной папки.
//!
//! zamok-recover <хранилище .Имя.zamok> <новая папка для файлов> [--recovery-key]
//!
//! Пароль (или с `--recovery-key` ключ восстановления из 44 слов) спрашивается
//! без отображения на экране. Работает без установленного
//! Zamok и без WinFsp, подходит и для хранилищ Cryptomator формата 8.
use std::path::Path;
use std::process::ExitCode;

use zamok_crypto::tree::Vault;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let use_recovery = args.len() == 4 && args[3] == "--recovery-key";
    if args.len() != 3 && !use_recovery {
        eprintln!("usage: zamok-recover <vault folder> <new output folder> [--recovery-key]");
        return ExitCode::from(2);
    }
    let (vault, out) = (Path::new(&args[1]), Path::new(&args[2]));
    if out.exists()
        && out
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        eprintln!("output folder must not exist or must be empty");
        return ExitCode::from(2);
    }
    let prompt = if use_recovery {
        "Recovery key (44 words): "
    } else {
        "Password: "
    };
    let secret = match rpassword::prompt_password(prompt) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot read input: {e}");
            return ExitCode::from(2);
        }
    };
    let opened = if use_recovery {
        zamok_crypto::recovery::decode(&secret).and_then(|key| Vault::open_with_key(vault, key))
    } else {
        Vault::open_with_password(vault, &secret)
    };
    let v = match opened {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot open vault: {e}");
            return ExitCode::from(1);
        }
    };
    // Последнее средство: одна плохая запись не должна лишать остальных файлов
    match v.export_tolerant(out) {
        Ok(r) if r.skipped.is_empty() => {
            println!("done: {} files", r.files);
            ExitCode::SUCCESS
        }
        Ok(r) => {
            println!(
                "done: {} files, {} entries could not be read",
                r.files,
                r.skipped.len()
            );
            for s in &r.skipped {
                eprintln!("skipped {s}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("export failed: {e}");
            ExitCode::from(1)
        }
    }
}
