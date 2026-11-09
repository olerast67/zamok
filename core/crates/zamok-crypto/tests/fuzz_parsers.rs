//! Случайные и испорченные данные для всех разборщиков файлов хранилища: нет паники
//! (в release при `panic=abort` она убила бы процесс со всеми открытыми папками), нет
//! огромных выделений памяти по длинам из файла, и испорченное не выдаётся за настоящее.

#[path = "common/mutate.rs"]
mod mutate;

use std::fs;
use std::path::Path;

use mutate::{assert_no_huge_alloc, iters, mutate as spoil, Counting, Rng};
use zamok_crypto::content::{self, FileHeader, HEADER_LEN};
use zamok_crypto::masterkey::{self, MasterkeyFile};
use zamok_crypto::tree::{self, Vault};
use zamok_crypto::vault::VaultConfig;
use zamok_crypto::{names, recovery, MasterKey};

#[global_allocator]
static ALLOC: Counting = Counting;

/// В проверках дешёвый scrypt: 1 МиБ памяти и больше здесь только зря тратит время.
const FAST: u32 = 1 << 4;
const SCRYPT_PROBE_MAX: u64 = 1024 * 1024;

#[test]
fn masterkey_file() {
    let key = MasterKey::generate().unwrap();
    let json = MasterkeyFile::lock_with(&key, "pw", FAST)
        .unwrap()
        .to_json();
    let mut rng = Rng::new(1);
    for i in 0..iters(3000) {
        let bytes = spoil(&mut rng, json.as_bytes());
        let Ok(f) = MasterkeyFile::from_json(&String::from_utf8_lossy(&bytes)) else {
            continue;
        };
        // Параметры в пределах 256 МиБ законны, но проверять их тут слишком долго
        let memory = (f.scrypt_cost_param as u64)
            .saturating_mul(f.scrypt_block_size as u64)
            .saturating_mul(128);
        if memory > SCRYPT_PROBE_MAX && memory <= 256 * 1024 * 1024 {
            continue;
        }
        if let Ok(k) = f.unlock("pw") {
            assert_eq!(k.to_raw(), key.to_raw(), "iteration {i}");
        }
    }
    assert_no_huge_alloc();
}

#[test]
fn scrypt_parameters_from_the_file() {
    let key = MasterKey::generate().unwrap();
    let base = MasterkeyFile::lock_with(&key, "pw", FAST).unwrap();
    for cost in [0, 1, 2, 3, 1 << 15, 1 << 25, 1 << 31, u32::MAX] {
        for block in [0, 1, 8, 1 << 20, 1 << 28, u32::MAX] {
            let memory = (cost as u64)
                .saturating_mul(block as u64)
                .saturating_mul(128);
            if memory > SCRYPT_PROBE_MAX && memory <= 256 * 1024 * 1024 {
                continue;
            }
            let mut f = base.clone();
            f.scrypt_cost_param = cost;
            f.scrypt_block_size = block;
            f.scrypt_salt = Vec::new();
            assert!(f.unlock("pw").is_err(), "cost {cost} block {block}");
        }
    }
    assert_no_huge_alloc();
}

#[test]
fn vault_jwt() {
    let key = MasterKey::generate().unwrap();
    let cfg = VaultConfig::new().unwrap();
    let jwt = cfg.to_jwt(&key).unwrap();
    let mut rng = Rng::new(2);
    for i in 0..iters(20_000) {
        let bytes = spoil(&mut rng, jwt.as_bytes());
        let text = String::from_utf8_lossy(&bytes);
        let _ = VaultConfig::key_id(&text);
        if let Ok(c) = VaultConfig::from_jwt(&text, &key) {
            assert_eq!(c.jti, cfg.jti, "iteration {i}");
        }
    }
    assert_no_huge_alloc();
}

#[test]
fn signed_config_with_extreme_values() {
    let key = MasterKey::generate().unwrap();
    for threshold in [0, 35, 36, 37, 220, usize::MAX] {
        for format in [0, 7, 8, u32::MAX] {
            let mut cfg = VaultConfig::new().unwrap();
            cfg.shortening_threshold = threshold;
            cfg.format = format;
            let jwt = cfg.to_jwt(&key).unwrap();
            match VaultConfig::from_jwt(&jwt, &key) {
                Ok(c) => {
                    assert!(format == 8 && threshold >= 36);
                    let _ = names::node_name(&key, &"я".repeat(255), "", c.shortening_threshold);
                }
                Err(_) => assert!(format != 8 || threshold < 36),
            }
        }
    }
}

#[test]
fn file_header_and_chunks() {
    let key = MasterKey::generate().unwrap();
    let header = FileHeader::generate().unwrap();
    let hbytes = header.encrypt(&key).unwrap();
    let chunk = header.encrypt_chunk(3, &[0x5A; 1000]).unwrap();
    let mut rng = Rng::new(3);
    for i in 0..iters(20_000) {
        let h = spoil(&mut rng, &hbytes);
        if FileHeader::decrypt(&key, &h).is_ok() {
            assert_eq!(&h[..HEADER_LEN], &hbytes[..], "iteration {i}");
        }
        let c = spoil(&mut rng, &chunk);
        if let Ok(pt) = header.decrypt_chunk(3, &c) {
            assert_eq!(c, chunk, "iteration {i}");
            assert_eq!(&pt[..], &[0x5A; 1000]);
        }
        assert!(header.decrypt_chunk(4, &chunk).is_err());
        let _ = content::cleartext_size(rng.next());
    }
    assert_no_huge_alloc();
}

#[test]
fn whole_files_and_dirid_backup() {
    let key = MasterKey::generate().unwrap();
    let body = vec![0x42u8; content::PAYLOAD_LEN * 2 + 77];
    let files = [
        content::encrypt_file(&key, &body).unwrap(),
        // Запасная копия ID каталога (`dirid.c9r`) тоже обычный зашифрованный файл
        content::encrypt_file(&key, b"11111111-2222-4333-8444-555555555555").unwrap(),
        content::encrypt_file(&key, b"").unwrap(),
    ];
    let plain: [&[u8]; 3] = [&body, b"11111111-2222-4333-8444-555555555555", b""];
    let mut rng = Rng::new(4);
    for i in 0..iters(2000) {
        let which = rng.below(files.len());
        let bytes = spoil(&mut rng, &files[which]);
        // Формат не защищает длину: отрезать целые куски с конца можно, менять нельзя
        if let Ok(pt) = content::decrypt_file(&key, &bytes) {
            assert!(plain[which].starts_with(&pt), "iteration {i}");
        }
        let mut out = Vec::new();
        let streamed = content::decrypt_stream(&key, &mut &bytes[..], &mut out);
        let digest = content::decrypt_stream_digest(&key, &mut &bytes[..]);
        assert_eq!(streamed.is_ok(), digest.is_ok(), "iteration {i}");
        if streamed.is_ok() {
            assert!(plain[which].starts_with(&out), "iteration {i}");
        }
    }
    assert_no_huge_alloc();
}

#[test]
fn encrypted_names() {
    let key = MasterKey::generate().unwrap();
    let enc = names::encrypt_name(&key, "Отчёт.docx", "parent");
    let mut rng = Rng::new(5);
    for i in 0..iters(20_000) {
        let bytes = spoil(&mut rng, enc.as_bytes());
        let text = String::from_utf8_lossy(&bytes);
        if let Ok(n) = names::decrypt_name(&key, &text, "parent") {
            assert_eq!(n, "Отчёт.docx", "iteration {i}");
        }
        let parent = String::from_utf8_lossy(&rng.bytes(64)).into_owned();
        assert!(parent == "parent" || names::decrypt_name(&key, &enc, &parent).is_err());
        let _ = names::dir_path(&key, &parent);
        let raw = rng.bytes(64);
        let _ = tree::safe_name(&String::from_utf8_lossy(&raw));
    }
    assert_no_huge_alloc();
}

#[test]
fn recovery_key_text() {
    let key = MasterKey::generate().unwrap();
    let words = recovery::encode(&key);
    let dict = recovery::dictionary();
    let mut rng = Rng::new(6);
    for i in 0..iters(20_000) {
        let bytes = spoil(&mut rng, words.as_bytes());
        if let Ok(k) = recovery::decode(&String::from_utf8_lossy(&bytes)) {
            // Контрольная сумма 16 бит: совпасть может и чужой текст, но он всегда канонический
            let again = recovery::encode(&k);
            assert_eq!(
                recovery::decode(&again).unwrap().to_raw(),
                k.to_raw(),
                "{i}"
            );
        }
        // Настоящие слова в нужном количестве доходят до проверки контрольной суммы
        let text: Vec<&str> = (0..44).map(|_| dict[rng.below(dict.len())]).collect();
        let _ = recovery::decode(&text.join(" "));
    }
    assert_no_huge_alloc();
}

#[test]
fn windows_hello_wrapped_key() {
    let key = MasterKey::generate().unwrap();
    let kek = [9u8; 32];
    let blob = masterkey::wrap_with(&key, &kek).unwrap();
    let mut rng = Rng::new(7);
    for i in 0..iters(20_000) {
        let bytes = spoil(&mut rng, &blob);
        if let Ok(k) = masterkey::unwrap_with(&bytes, &kek) {
            assert_eq!(k.to_raw(), key.to_raw(), "iteration {i}");
        }
    }
    assert_no_huge_alloc();
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            files_under(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

/// Хранилище целиком: испорченный файл или папка обходится без паники и не пишет за пределы
/// папки выгрузки.
#[test]
fn walking_and_exporting_a_damaged_vault() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("a.txt"), b"aaa").unwrap();
    fs::write(src.join("sub").join("b.txt"), vec![7u8; 40_000]).unwrap();
    fs::write(src.join("L".repeat(200)), b"long name").unwrap();
    let root = d.path().join("vault");
    tree::create_from_dir(&src, &root, "pw", FAST).unwrap();
    let mut files = Vec::new();
    files_under(&root, &mut files);
    assert!(files.iter().any(|p| p.ends_with("name.c9s")));
    assert!(files.iter().any(|p| p.ends_with("dir.c9r")));

    let mut rng = Rng::new(8);
    for i in 0..iters(400) {
        let target = &files[rng.below(files.len())];
        let original = fs::read(target).unwrap();
        fs::write(target, spoil(&mut rng, &original)).unwrap();
        if let Ok(v) = Vault::open_with_password(&root, "pw") {
            let _ = v.walk();
            let out = d.path().join(format!("out{i}"));
            if v.export_tolerant(&out).is_ok() {
                let mut written = Vec::new();
                files_under(&out, &mut written);
                assert!(written.iter().all(|p| p.starts_with(&out)), "iteration {i}");
            }
            let _ = fs::remove_dir_all(&out);
        }
        fs::write(target, &original).unwrap();
    }
    assert_no_huge_alloc();
}

/// Огромные служебные файлы читаются с пределом, а не целиком в память.
#[test]
fn oversized_metadata_files_are_refused() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("L".repeat(200)), b"x").unwrap();
    let root = d.path().join("vault");
    tree::create_from_dir(&src, &root, "pw", FAST).unwrap();
    let mut files = Vec::new();
    files_under(&root, &mut files);
    let huge = vec![b' '; 3 * 1024 * 1024];

    for name in ["dir.c9r", "name.c9s"] {
        let target = files.iter().find(|p| p.ends_with(name)).unwrap();
        let original = fs::read(target).unwrap();
        fs::write(target, &huge).unwrap();
        let v = Vault::open_with_password(&root, "pw").unwrap();
        assert!(v.walk().is_err(), "{name}");
        fs::write(target, &original).unwrap();
    }
    for name in ["masterkey.cryptomator", "vault.cryptomator"] {
        let target = root.join(name);
        let original = fs::read(&target).unwrap();
        fs::write(&target, &huge).unwrap();
        assert!(Vault::open_with_password(&root, "pw").is_err(), "{name}");
        fs::write(&target, &original).unwrap();
    }
    assert!(zamok_crypto::read_meta(&root.join("masterkey.cryptomator")).is_ok());
    assert_no_huge_alloc();
}
