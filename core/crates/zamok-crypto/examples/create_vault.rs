//! Создать хранилище Zamok из папки (для проверки совместимости с Cryptomator).
//! cargo run --example create_vault -- <папка с файлами> <новое хранилище> <пароль>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: create_vault <src> <dst> <password>");
        std::process::exit(2);
    }
    let v = zamok_crypto::tree::create_from_dir(
        a[1].as_ref(),
        a[2].as_ref(),
        &a[3],
        zamok_crypto::masterkey::SCRYPT_COST,
    )
    .expect("create vault");
    println!("ok, format {}", v.config.format);
}
