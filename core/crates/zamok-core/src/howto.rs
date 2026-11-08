//! Памятка при удалении программы. Без Zamok зашифрованная папка исчезает из вида: остаётся
//! только скрытое хранилище `.Имя.zamok`, и человек решит, что данные пропали. Установщик
//! вызывает `zamok-core --uninstall-notes`, и рядом с каждым хранилищем появляется текстовый
//! файл. Существующий файл не перезаписывается.

use std::path::{Path, PathBuf};

use crate::config::{Config, FolderConfig};

fn note_path(folder: &FolderConfig, russian: bool) -> Option<PathBuf> {
    let parent = folder.vault.parent()?;
    let name = folder.name();
    Some(parent.join(if russian {
        format!("КАК ОТКРЫТЬ {name}.txt")
    } else {
        format!("HOW TO OPEN {name}.txt")
    }))
}

fn note_text(folder: &FolderConfig, russian: bool) -> String {
    let name = folder.name();
    let vault = folder.vault.display();
    if russian {
        format!(
            "Программа Zamok удалена, но папка «{name}» не пропала. Она зашифрована и лежит здесь:\r\n\
             {vault}\r\n\r\n\
             Это скрытая папка. Чтобы её увидеть, включите в Проводнике показ скрытых и системных файлов.\r\n\r\n\
             Как открыть:\r\n\
             1. Установите Zamok снова: папка вернётся на своё место.\r\n\
             2. Или установите Cryptomator (cryptomator.org) и откройте в нём файл vault.cryptomator из этой папки. Понадобится ваш пароль или ключ восстановления.\r\n\r\n\
             Не удаляйте эту папку: без неё данные не восстановить.\r\n"
        )
    } else {
        format!(
            "Zamok was uninstalled, but the folder \"{name}\" is not lost. It is encrypted and stored here:\r\n\
             {vault}\r\n\r\n\
             This folder is hidden. To see it, turn on hidden and system files in File Explorer.\r\n\r\n\
             How to open it:\r\n\
             1. Install Zamok again: the folder comes back to its place.\r\n\
             2. Or install Cryptomator (cryptomator.org) and open the vault.cryptomator file from this folder. You will need your password or recovery key.\r\n\r\n\
             Do not delete this folder: your data cannot be recovered without it.\r\n"
        )
    }
}

/// Записать памятки для всех папок, хранилище которых на месте. Возвращает, сколько записано.
pub fn write_notes(data_dir: &Path, russian: bool) -> usize {
    let Ok(cfg) = Config::load(data_dir) else {
        return 0;
    };
    let mut written = 0;
    for folder in &cfg.folders {
        if !folder.vault.is_dir() {
            continue;
        }
        let Some(path) = note_path(folder, russian) else {
            continue;
        };
        // BOM нужен, чтобы старый Блокнот не ошибся с кодировкой
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(note_text(folder, russian).as_bytes());
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path);
        if let Ok(mut f) = file {
            if f.write_all(&bytes).is_ok() {
                written += 1;
            }
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_appears_next_to_the_vault_and_is_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let vault = d.path().join(".Клиенты.zamok");
        std::fs::create_dir(&vault).unwrap();
        let mut cfg = Config::default();
        cfg.folders.push(FolderConfig {
            id: "a".into(),
            path: d.path().join("Клиенты"),
            vault: vault.clone(),
            vault_prev: None,
            idle_lock_secs: 0,
        });
        cfg.folders.push(FolderConfig {
            id: "b".into(),
            path: d.path().join("Нет"),
            vault: d.path().join(".Нет.zamok"),
            vault_prev: None,
            idle_lock_secs: 0,
        });
        cfg.save(d.path()).unwrap();

        assert_eq!(write_notes(d.path(), true), 1);
        let note = d.path().join("КАК ОТКРЫТЬ Клиенты.txt");
        let text = std::fs::read(&note).unwrap();
        assert!(String::from_utf8_lossy(&text).contains(&vault.display().to_string()));

        std::fs::write(&note, b"mine").unwrap();
        assert_eq!(write_notes(d.path(), true), 0);
        assert_eq!(std::fs::read(&note).unwrap(), b"mine");
        assert!(!d.path().join("КАК ОТКРЫТЬ Нет.txt").exists());
    }
}
