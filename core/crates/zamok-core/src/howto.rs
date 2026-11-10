//! Памятка рядом с хранилищем. Без Zamok зашифрованная папка исчезает из вида: остаётся
//! только скрытое хранилище `.Имя.zamok`, и человек решит, что данные пропали. Памятка
//! появляется в двух случаях:
//! - установщик при удалении программы вызывает `zamok-core --uninstall-notes`;
//! - человек удалил папку в Проводнике и в окне выбрал «Убрать из Zamok».
//!
//! Существующий файл не перезаписывается.

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

/// Почему появилась памятка.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    /// Программу удалили.
    Uninstalled,
    /// Папку убрали из списка Zamok после удаления в Проводнике.
    Removed,
}

/// Язык интерфейса Windows русский: памятка, трей и уведомления по-русски, иначе по-английски.
/// То же правило в окнах (`Lang` в `WindowHelper.cs`). `ZAMOK_LANG=ru` или `en` задаёт язык
/// для тестов.
pub fn user_is_russian() -> bool {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    match std::env::var("ZAMOK_LANG").as_deref() {
        Ok("ru") => true,
        Ok("en") => false,
        _ => (unsafe { GetUserDefaultUILanguage() } & 0x3FF) == 0x19,
    }
}

fn note_text(folder: &FolderConfig, russian: bool, why: Why) -> String {
    let name = folder.name();
    let vault = folder.vault.display();
    if why == Why::Removed {
        return if russian {
            format!(
                "Папку «{name}» удалили в Проводнике и убрали из Zamok, но зашифрованные файлы не пропали. Они лежат здесь:\r\n\
                 {vault}\r\n\r\n\
                 Это скрытая папка. Чтобы её увидеть, включите в Проводнике показ скрытых и системных файлов.\r\n\r\n\
                 Как открыть: установите Cryptomator (cryptomator.org) и откройте в нём файл vault.cryptomator из этой папки. Понадобится ваш пароль или ключ восстановления.\r\n\r\n\
                 Если файлы больше не нужны, удалите эту скрытую папку и этот файл. Без скрытой папки данные не восстановить.\r\n"
            )
        } else {
            format!(
                "The folder \"{name}\" was deleted in File Explorer and removed from Zamok, but the encrypted files are not lost. They are stored here:\r\n\
                 {vault}\r\n\r\n\
                 This folder is hidden. To see it, turn on hidden and system files in File Explorer.\r\n\r\n\
                 How to open it: install Cryptomator (cryptomator.org) and open the vault.cryptomator file from this folder. You will need your password or recovery key.\r\n\r\n\
                 If you no longer need the files, delete this hidden folder and this file. Without the hidden folder the data cannot be recovered.\r\n"
            )
        };
    }
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
    cfg.folders
        .iter()
        .filter(|f| write_note(f, russian, Why::Uninstalled))
        .count()
}

/// Записать памятку для одной папки, если хранилище на месте и памятки ещё нет.
pub fn write_note(folder: &FolderConfig, russian: bool, why: Why) -> bool {
    if !folder.vault.is_dir() {
        return false;
    }
    let Some(path) = note_path(folder, russian) else {
        return false;
    };
    // BOM нужен, чтобы старый Блокнот не ошибся с кодировкой
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(note_text(folder, russian, why).as_bytes());
    use std::io::Write;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path);
    match file {
        Ok(mut f) => f.write_all(&bytes).is_ok(),
        Err(_) => false,
    }
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
            volume_serial: None,
            vault_file_id: None,
            idle_lock_secs: 0,
        });
        cfg.folders.push(FolderConfig {
            id: "b".into(),
            path: d.path().join("Нет"),
            vault: d.path().join(".Нет.zamok"),
            vault_prev: None,
            volume_serial: None,
            vault_file_id: None,
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

    #[test]
    fn note_after_removal_says_so_and_keeps_the_vault() {
        let d = tempfile::tempdir().unwrap();
        let vault = d.path().join(".Docs.zamok");
        std::fs::create_dir(&vault).unwrap();
        std::fs::write(vault.join("masterkey.cryptomator"), b"k").unwrap();
        let folder = FolderConfig {
            id: "a".into(),
            path: d.path().join("Docs"),
            vault: vault.clone(),
            vault_prev: None,
            volume_serial: None,
            vault_file_id: None,
            idle_lock_secs: 0,
        };
        assert!(write_note(&folder, false, Why::Removed));
        let text = std::fs::read_to_string(d.path().join("HOW TO OPEN Docs.txt")).unwrap();
        assert!(text.contains("removed from Zamok") && text.contains(&vault.display().to_string()));
        assert!(!write_note(&folder, false, Why::Removed));
        assert!(vault.join("masterkey.cryptomator").is_file());
    }
}
