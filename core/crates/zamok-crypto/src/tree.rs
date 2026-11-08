//! Обход хранилища целиком: открыть по паролю или по мастер-ключу и выгрузить
//! расшифрованную копию. Это основа `zamok-recover` и проверка на эталонах.
//!
//! Расшифрованные имена проверяются: в них не может быть разделителей пути,
//! `.` и `..` и запрещённых в Windows символов. Иначе испорченное или чужое
//! хранилище могло бы записать файл за пределы папки выгрузки.

use std::fs;
use std::path::{Path, PathBuf};

use crate::masterkey::MasterkeyFile;
use crate::names::{self, C9R, C9S, CONTENTS_FILE, DIR_FILE, NAME_FILE, SYMLINK_FILE};
use crate::vault::VaultConfig;
use crate::{content, Error, MasterKey, Result};

pub struct Vault {
    root: PathBuf,
    key: MasterKey,
    pub config: VaultConfig,
}

/// Узел расшифрованного дерева.
#[derive(Debug)]
pub enum Node {
    Dir { path: String },
    File { path: String, ciphertext: PathBuf },
    Symlink { path: String },
}

fn safe_name(name: &str) -> Result<()> {
    let bad_char = |c: char| {
        matches!(
            c,
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        ) || (c as u32) < 32
    };
    // Зарезервированные имена устройств Windows: CON, NUL, COM1 и т. п., в том числе с расширением
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.ends_with(' ')
        || reserved
        || name.chars().any(bad_char);
    if bad {
        return Err(Error::Authentication);
    }
    Ok(())
}

/// Пределы обхода: защищают от испорченного хранилища, где каталоги ссылаются друг на друга.
const MAX_DEPTH: usize = 256;
const MAX_NODES: usize = 10_000_000;

impl Vault {
    pub fn open_with_password(root: &Path, password: &str) -> Result<Self> {
        let mk =
            MasterkeyFile::from_json(&fs::read_to_string(root.join("masterkey.cryptomator"))?)?;
        let key = mk.unlock(password)?;
        Self::open_with_key(root, key)
    }

    pub fn open_with_key(root: &Path, key: MasterKey) -> Result<Self> {
        let jwt = fs::read_to_string(root.join("vault.cryptomator"))?;
        let config = VaultConfig::from_jwt(&jwt, &key)?;
        Ok(Vault {
            root: root.to_path_buf(),
            key,
            config,
        })
    }

    pub fn key(&self) -> &MasterKey {
        &self.key
    }

    /// Все узлы дерева, начиная с корня (ID корня пустой).
    pub fn walk(&self) -> Result<Vec<Node>> {
        let mut out = Vec::new();
        let mut visited = std::collections::HashSet::new();
        visited.insert(String::new());
        let mut stack = vec![(String::new(), String::new(), 0usize)]; // (ID каталога, путь, глубина)
        while let Some((dir_id, path, depth)) = stack.pop() {
            let dir = self.root.join(names::dir_path(&self.key, &dir_id));
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let fname = entry.file_name().to_string_lossy().into_owned();
                let (enc_name, node_dir) = if let Some(stem) = fname.strip_suffix(C9R) {
                    (stem.to_string(), entry.path())
                } else if fname.ends_with(C9S) {
                    let full = fs::read_to_string(entry.path().join(NAME_FILE))?;
                    let stem = full
                        .trim()
                        .strip_suffix(C9R)
                        .ok_or(Error::Authentication)?
                        .to_string();
                    (stem, entry.path())
                } else {
                    continue;
                };
                if fname == names::DIRID_BACKUP_FILE {
                    continue;
                }
                if out.len() >= MAX_NODES {
                    return Err(Error::Authentication);
                }
                let name = names::decrypt_name(&self.key, &enc_name, &dir_id)?;
                safe_name(&name)?;
                let child = if path.is_empty() {
                    name
                } else {
                    format!("{path}/{name}")
                };
                let is_dir_node = entry.file_type()?.is_dir();
                if is_dir_node && node_dir.join(DIR_FILE).is_file() {
                    let child_id = fs::read_to_string(node_dir.join(DIR_FILE))?
                        .trim()
                        .to_string();
                    out.push(Node::Dir {
                        path: child.clone(),
                    });
                    // Каталог, который уже встречался, или слишком глубокое дерево: хранилище испорчено
                    if !visited.insert(child_id.clone()) || depth + 1 > MAX_DEPTH {
                        return Err(Error::Authentication);
                    }
                    stack.push((child_id, child, depth + 1));
                } else if is_dir_node && node_dir.join(SYMLINK_FILE).is_file() {
                    out.push(Node::Symlink { path: child });
                } else if is_dir_node && node_dir.join(CONTENTS_FILE).is_file() {
                    out.push(Node::File {
                        path: child,
                        ciphertext: node_dir.join(CONTENTS_FILE),
                    });
                } else if !is_dir_node {
                    out.push(Node::File {
                        path: child,
                        ciphertext: entry.path(),
                    });
                }
            }
        }
        Ok(out)
    }

    /// Выгрузить расшифрованную копию в папку `out` (должна быть пустой или новой).
    /// Ссылки (symlink) пропускаются. Возвращает число файлов.
    pub fn export(&self, out: &Path) -> Result<usize> {
        fs::create_dir_all(out)?;
        let mut files = 0;
        for node in self.walk()? {
            match node {
                Node::Dir { path } => fs::create_dir_all(out.join(&path))?,
                Node::File { path, ciphertext } => {
                    let target = out.join(&path);
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let data = content::decrypt_file(&self.key, &fs::read(ciphertext)?)?;
                    // create_new: два одинаковых имени (или разный регистр) не перезапишут друг друга
                    use std::io::Write;
                    let mut f = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)?;
                    f.write_all(&data)?;
                    files += 1;
                }
                Node::Symlink { .. } => {}
            }
        }
        Ok(files)
    }
}

/// Создать новое хранилище в `dst` из содержимого папки `src` (для тестов совместимости
/// и как основа шифрования на месте). `dst` не должна существовать или должна быть пустой.
pub fn create_from_dir(src: &Path, dst: &Path, password: &str, scrypt_cost: u32) -> Result<Vault> {
    let key = MasterKey::generate()?;
    fs::create_dir_all(dst)?;
    fs::write(
        dst.join("masterkey.cryptomator"),
        MasterkeyFile::lock_with(&key, password, scrypt_cost)?.to_json(),
    )?;
    let config = VaultConfig::new()?;
    fs::write(dst.join("vault.cryptomator"), config.to_jwt(&key)?)?;
    let vault = Vault {
        root: dst.to_path_buf(),
        key,
        config,
    };
    vault.write_dir(src, "")?;
    Ok(vault)
}

impl Vault {
    fn write_dir(&self, src: &Path, dir_id: &str) -> Result<()> {
        let dir = self.root.join(names::dir_path(&self.key, dir_id));
        fs::create_dir_all(&dir)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Authentication)?;
            let (node, full) =
                names::node_name(&self.key, &name, dir_id, self.config.shortening_threshold);
            let node_path = dir.join(&node);
            let is_dir = entry.file_type()?.is_dir();
            if is_dir || full.is_some() {
                fs::create_dir_all(&node_path)?;
            }
            if let Some(full) = &full {
                fs::write(node_path.join(NAME_FILE), full)?;
            }
            if is_dir {
                let child_id = crate::uuid_v4()?;
                fs::write(node_path.join(DIR_FILE), &child_id)?;
                self.write_dir(&entry.path(), &child_id)?;
            } else {
                let ct = content::encrypt_file(&self.key, &fs::read(entry.path())?)?;
                let target = if full.is_some() {
                    node_path.join(CONTENTS_FILE)
                } else {
                    node_path
                };
                fs::write(target, ct)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn rejects_path_tricks() {
        for bad in [
            "..", ".", "a/b", "a\\b", "c:", "x\0y", "", "CON", "nul.txt", "com1.log", "end.",
            "end ",
        ] {
            assert!(safe_name(bad).is_err(), "{bad:?}");
        }
        assert!(safe_name("Отчёт 2026.docx").is_ok());
    }
}
