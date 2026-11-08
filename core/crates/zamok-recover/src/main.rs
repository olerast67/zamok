//! zamok-recover: выгрузить расшифрованную копию зашифрованной папки.
//!
//! zamok-recover <хранилище .Имя.zamok> <новая папка для файлов>
//!
//! Пароль спрашивается без отображения на экране. Работает без установленного
//! Zamok и без WinFsp, подходит и для хранилищ Cryptomator формата 8.
use std::path::Path;
use std::process::ExitCode;

use zamok_crypto::tree::Vault;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: zamok-recover <vault folder> <new output folder>");
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
    let password = match rpassword::prompt_password("Password: ") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot read password: {e}");
            return ExitCode::from(2);
        }
    };
    let v = match Vault::open_with_password(vault, &password) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot open vault: {e}");
            return ExitCode::from(1);
        }
    };
    match v.export(out) {
        Ok(n) => {
            println!("done: {n} files");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("export failed: {e}");
            ExitCode::from(1)
        }
    }
}
