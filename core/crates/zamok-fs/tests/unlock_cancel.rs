//! Запирание во время проверки пароля отменяет открытие (иначе Win+L, нажатый пока
//! scrypt считает ключ, не запер бы папку).

use std::sync::Arc;

use zamok_fs::fs::{UnlockError, ZamokFs};

fn vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("a.txt"), b"a").unwrap();
    let dst = d.path().join("vault");
    // Настоящая стоимость scrypt: проверка пароля идёт заметное время
    zamok_crypto::tree::create_from_dir(&src, &dst, "pw", zamok_crypto::masterkey::SCRYPT_COST)
        .unwrap();
    (d, dst)
}

#[test]
fn cancel_during_password_check_keeps_folder_locked() {
    let (_d, v) = vault();
    let fs = ZamokFs::new(&v, "T").unwrap();
    let c = fs.control();
    let c2 = Arc::clone(&c);
    let t = std::thread::spawn(move || c2.unlock_with_password("pw"));
    std::thread::sleep(std::time::Duration::from_millis(100));
    // Запирание запертой папки отменяет идущее открытие
    assert!(!c.lock());
    assert!(matches!(t.join().unwrap(), Err(UnlockError::Cancelled)));
    assert!(c.is_locked());
    // Следующая попытка без помех открывает
    c.unlock_with_password("pw").unwrap();
    assert!(!c.is_locked());
}

#[test]
fn wrong_password_is_reported_as_such() {
    let (_d, v) = vault();
    let fs = ZamokFs::new(&v, "T").unwrap();
    assert!(matches!(
        fs.control().unlock_with_password("nope"),
        Err(UnlockError::Crypto(zamok_crypto::Error::WrongPassword))
    ));
}

#[test]
fn still_wanted_false_cancels_and_true_unlocks() {
    let (_d, v) = vault();
    let fs = ZamokFs::new(&v, "T").unwrap();
    let c = fs.control();
    assert!(matches!(
        c.unlock_with_password_if("pw", &|| false),
        Err(UnlockError::Cancelled)
    ));
    assert!(c.is_locked());
    c.unlock_with_password_if("pw", &|| true).unwrap();
    assert!(c.lock());
    assert!(c.is_locked());
}
