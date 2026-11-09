//! Случайные и испорченные данные для файлов core: список папок, журналы работ, команды канала.
//! Журналы только читаются: восстановление по случайным путям здесь запускать нельзя.

#[path = "../../zamok-crypto/tests/common/mutate.rs"]
mod mutate;

use std::fs;

use mutate::{assert_no_huge_alloc, iters, mutate as spoil, Counting, Rng};
use zamok_core::config::Config;
use zamok_core::{decrypt, encrypt, protocol};

#[global_allocator]
static ALLOC: Counting = Counting;

const FOLDERS: &str = r#"{"version":1,"lock_on_screen_lock":true,"folders":[{"id":"a1","path":"C:\\x","vault":"C:\\.x.zamok","idle_lock_secs":60,"volume_serial":1,"vault_file_id":2}]}"#;
const ENCRYPT_JOB: &str = r#"{"version":1,"id":"j1","phase":"copying","path":"C:\\x","tmp":"C:\\.x.tmp","vault":"C:\\.x.zamok","aside":"C:\\.x.zamok-original","idle_lock_secs":60,"masterkey_sha256":"ab","rolling_back":false}"#;
const DECRYPT_JOB: &str = r#"{"version":1,"id":"j2","step":"copying","path":"C:\\x","vault":"C:\\.x.zamok","tmp":"C:\\.x.zamok-decrypted.tmp","idle_lock_secs":60,"masterkey_sha256":"ab"}"#;
const REQUEST: &str = r#"{"cmd":"unlock","path":"C:\\x","password":"pw"}"#;

#[test]
fn folder_list() {
    let d = tempfile::tempdir().unwrap();
    fs::write(Config::file(d.path()), FOLDERS).unwrap();
    assert_eq!(Config::load(d.path()).unwrap().folders.len(), 1);
    let mut rng = Rng::new(11);
    for _ in 0..iters(5000) {
        fs::write(Config::file(d.path()), spoil(&mut rng, FOLDERS.as_bytes())).unwrap();
        let _ = Config::load(d.path());
    }
    fs::write(Config::file(d.path()), vec![b' '; 20 * 1024 * 1024]).unwrap();
    assert!(Config::load(d.path()).is_err());
    assert_no_huge_alloc();
}

#[test]
fn job_journals() {
    let d = tempfile::tempdir().unwrap();
    let (jobs, decrypts) = (d.path().join("jobs"), d.path().join("decrypt-jobs"));
    fs::create_dir_all(&jobs).unwrap();
    fs::create_dir_all(&decrypts).unwrap();
    fs::write(jobs.join("g.json"), ENCRYPT_JOB).unwrap();
    fs::write(decrypts.join("g.json"), DECRYPT_JOB).unwrap();
    assert_eq!(encrypt::Journal::load_all(d.path()).len(), 1);
    assert_eq!(decrypt::Journal::load_all(d.path()).len(), 1);
    let mut rng = Rng::new(12);
    for _ in 0..iters(5000) {
        fs::write(jobs.join("f.json"), spoil(&mut rng, ENCRYPT_JOB.as_bytes())).unwrap();
        fs::write(
            decrypts.join("f.json"),
            spoil(&mut rng, DECRYPT_JOB.as_bytes()),
        )
        .unwrap();
        let _ = encrypt::Journal::load_all(d.path());
        let _ = decrypt::Journal::load_all(d.path());
    }
    let huge = vec![b' '; 3 * 1024 * 1024];
    fs::write(jobs.join("f.json"), &huge).unwrap();
    fs::write(decrypts.join("f.json"), &huge).unwrap();
    assert_eq!(encrypt::Journal::load_all(d.path()).len(), 1);
    assert_eq!(decrypt::Journal::load_all(d.path()).len(), 1);
    assert_no_huge_alloc();
}

#[test]
fn pipe_requests() {
    let mut rng = Rng::new(13);
    for _ in 0..iters(50_000) {
        let _ = protocol::parse(&spoil(&mut rng, REQUEST.as_bytes()));
    }
    assert_no_huge_alloc();
}
