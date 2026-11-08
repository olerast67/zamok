//! Проверка на эталоне: хранилище tests/fixtures/vault1 создано библиотеками
//! Cryptomator 1.19.6 (cryptofs 2.11.1, cryptolib 2.4.0) программой
//! tests/fixtures-tool/MakeVault.java из файлов tests/fixtures-src/vault1.
//! Пароль тестовый: zamok-test-1.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use zamok_crypto::masterkey::MasterkeyFile;
use zamok_crypto::tree::Vault;
use zamok_crypto::MasterKey;

fn repo() -> PathBuf {
    let r = repo_root();
    // Git не хранит пустые папки, а в эталоне она есть
    fs::create_dir_all(r.join("tests/fixtures-src/vault1/Пустая папка")).unwrap();
    r
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

fn read_tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut m = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if e.file_type().unwrap().is_dir() {
                m.insert(rel, None);
                stack.push(e.path());
            } else {
                m.insert(rel, Some(fs::read(e.path()).unwrap()));
            }
        }
    }
    m
}

#[test]
fn password_unlocks_cryptomator_masterkey() {
    let r = repo();
    let mk = MasterkeyFile::from_json(
        &fs::read_to_string(r.join("tests/fixtures/vault1/masterkey.cryptomator")).unwrap(),
    )
    .unwrap();
    let key = mk.unlock("zamok-test-1").unwrap();
    let hex = fs::read_to_string(r.join("tests/fixtures/vault1-masterkey-hex.txt")).unwrap();
    let expected: Vec<u8> = (0..128)
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex.trim()[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(key.to_raw().as_slice(), expected.as_slice());
    assert!(mk.unlock("wrong").is_err());
}

#[test]
fn export_matches_source_files() {
    let r = repo();
    let vault =
        Vault::open_with_password(&r.join("tests/fixtures/vault1"), "zamok-test-1").unwrap();
    assert_eq!(vault.config.format, 8);
    let out = std::env::temp_dir().join(format!("zamok-fixture-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out);
    vault.export(&out).unwrap();
    let got = read_tree(&out);
    let want = read_tree(&r.join("tests/fixtures-src/vault1"));
    let _ = fs::remove_dir_all(&out);
    assert_eq!(
        got.keys().collect::<Vec<_>>(),
        want.keys().collect::<Vec<_>>()
    );
    assert_eq!(got, want);
}

#[test]
fn our_vault_config_and_names_match_cryptomator() {
    // Наш код должен получать те же зашифрованные имена и пути каталогов, что и Cryptomator.
    let r = repo();
    let root = r.join("tests/fixtures/vault1");
    let vault = Vault::open_with_password(&root, "zamok-test-1").unwrap();
    let key: &MasterKey = vault.key();
    let root_dir = root.join(zamok_crypto::names::dir_path(key, ""));
    assert!(root_dir.is_dir(), "root dir path must match Cryptomator's");
    let (node, _) =
        zamok_crypto::names::node_name(key, "Привет.txt", "", vault.config.shortening_threshold);
    assert!(
        root_dir.join(node).is_file(),
        "file name must match Cryptomator's"
    );
    let long = fs::read_dir(r.join("tests/fixtures-src/vault1"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("Длинное"))
        .unwrap();
    let (node, full) =
        zamok_crypto::names::node_name(key, &long, "", vault.config.shortening_threshold);
    assert!(full.is_some());
    assert!(
        root_dir.join(node).join("name.c9s").is_file(),
        "shortened name must match Cryptomator's"
    );
}

#[test]
fn our_vault_roundtrips_through_our_reader() {
    let r = repo();
    let dst = std::env::temp_dir().join(format!("zamok-created-{}", std::process::id()));
    let out = std::env::temp_dir().join(format!("zamok-created-out-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dst);
    let _ = fs::remove_dir_all(&out);
    zamok_crypto::tree::create_from_dir(&r.join("tests/fixtures-src/vault1"), &dst, "pw", 1 << 4)
        .unwrap();
    Vault::open_with_password(&dst, "pw")
        .unwrap()
        .export(&out)
        .unwrap();
    let ok = read_tree(&out) == read_tree(&r.join("tests/fixtures-src/vault1"));
    let _ = fs::remove_dir_all(&dst);
    let _ = fs::remove_dir_all(&out);
    assert!(ok);
}

#[test]
fn directory_cycle_is_rejected() {
    // Испорченное хранилище: папка ссылается на корень. Обход должен остановиться с ошибкой.
    let r = repo();
    let dst = std::env::temp_dir().join(format!("zamok-cycle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dst);
    zamok_crypto::tree::create_from_dir(&r.join("tests/fixtures-src/vault1"), &dst, "pw", 1 << 4)
        .unwrap();
    let mut dir_files = Vec::new();
    let mut stack = vec![dst.join("d")];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().unwrap() == "dir.c9r" {
                dir_files.push(p);
            }
        }
    }
    fs::write(&dir_files[0], "").unwrap();
    let res = Vault::open_with_password(&dst, "pw").unwrap().walk();
    let _ = fs::remove_dir_all(&dst);
    assert!(res.is_err());
}

#[test]
fn cryptomator_recovery_key_matches() {
    // Ключ восстановления создан кодом Cryptomator 1.19.6 (RecoveryKeyFactory) для vault1
    let r = repo();
    let words = fs::read_to_string(r.join("tests/fixtures/vault1-recovery.txt")).unwrap();
    let key = zamok_crypto::recovery::decode(&words).unwrap();
    let hex = fs::read_to_string(r.join("tests/fixtures/vault1-masterkey-hex.txt")).unwrap();
    let expected: Vec<u8> = (0..128)
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex.trim()[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(key.to_raw().as_slice(), expected.as_slice());
    // и в обратную сторону: наш код записывает тот же ключ теми же словами
    assert_eq!(zamok_crypto::recovery::encode(&key).as_str(), words.trim());
    // ключом восстановления открывается всё хранилище
    let vault = Vault::open_with_key(&r.join("tests/fixtures/vault1"), key).unwrap();
    assert!(!vault.walk().unwrap().is_empty());
}

#[test]
fn dirid_backup_file_holds_encrypted_dir_id() {
    // Cryptomator 1.19 кладёт в каждый каталог d/XX/... файл dirid.c9r:
    // зашифрованное как обычный файл содержимое = ID этого каталога.
    let r = repo();
    let root = r.join("tests/fixtures/vault1");
    let vault = Vault::open_with_password(&root, "zamok-test-1").unwrap();
    let key = vault.key();
    let mut checked = 0;
    let mut ids = vec![String::new()];
    // ID всех каталогов: корень и содержимое каждого dir.c9r
    let mut stack = vec![root.join("d")];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().unwrap() == "dir.c9r" {
                ids.push(fs::read_to_string(&p).unwrap());
            }
        }
    }
    for id in ids {
        let backup = root
            .join(zamok_crypto::names::dir_path(key, &id))
            .join("dirid.c9r");
        let data = zamok_crypto::content::decrypt_file(key, &fs::read(&backup).unwrap()).unwrap();
        assert_eq!(String::from_utf8(data.to_vec()).unwrap(), id);
        checked += 1;
    }
    assert!(checked >= 4);
}
