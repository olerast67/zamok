//! Скорость чтения зашифрованного файла без WinFsp: последовательные запросы по 1 МиБ,
//! как у `copy /b`. Показывает, сколько из времени чтения тратит сам Zamok.
//! Запуск: cargo run --release -p zamok-fs --example read_speed -- <папка> [МБ] [повторы]

use std::time::Instant;

use zamok_crypto::MasterKey;
use zamok_fs::cipherfile::CipherFile;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("folder"));
    let mb: usize = args.next().map_or(1024, |s| s.parse().unwrap());
    let rounds: usize = args.next().map_or(5, |s| s.parse().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("read-speed.c9r");
    let _ = std::fs::remove_file(&path);
    let key = MasterKey::generate().unwrap();
    let mut f = CipherFile::create(&path, &key).unwrap();
    let block: Vec<u8> = (0..1 << 20).map(|i| (i % 251) as u8).collect();
    for i in 0..mb {
        f.write_at((i << 20) as u64, &block).unwrap();
    }
    drop(f);
    let f = CipherFile::open(&path, &key).unwrap();
    let mut buf = vec![0u8; 1 << 20];
    for _ in 0..rounds {
        let t = Instant::now();
        let mut off = 0u64;
        loop {
            let n = f.read_at(off, &mut buf).unwrap();
            if n == 0 {
                break;
            }
            off += n as u64;
        }
        let s = t.elapsed().as_secs_f64();
        println!("{mb} MiB in {s:.3} s, {:.0} MiB/s", mb as f64 / s);
    }
    drop(f);
    std::fs::remove_file(&path).unwrap();
}
