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

/// Итог обхода с пропусками: что удалось прочитать и что нет. В `skipped` путь и причина.
#[derive(Debug, Default)]
pub struct WalkReport {
    pub nodes: Vec<Node>,
    pub skipped: Vec<String>,
}

/// Итог выгрузки с пропусками.
#[derive(Debug, Default)]
pub struct ExportReport {
    pub files: usize,
    pub skipped: Vec<String>,
}

/// Узел, прочитанный из одной записи каталога.
enum Classified {
    Dir(String),
    Symlink,
    File(PathBuf),
    /// Папка узла без `dir.c9r`, ссылки и содержимого.
    Empty,
}

/// Имя годится для хранилища и для Windows: без разделителей, `.`, `..`, запрещённых знаков
/// и имён устройств (CON, NUL, COM1…).
pub fn safe_name(name: &str) -> Result<()> {
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
    let chars: Vec<char> = stem.chars().collect();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (chars.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(chars[3], '0'..='9' | '\u{00B9}' | '\u{00B2}' | '\u{00B3}'));
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

    /// Папка хранилища на диске.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Все узлы дерева, начиная с корня (ID корня пустой). Любая повреждённая запись: ошибка.
    /// Каталог без физической папки считается пустым (остаётся после сбоя между созданием
    /// `dir.c9r` и папки каталога), но только пока в `d/` нет непустых папок, до которых обход
    /// не дошёл: такая папка значит, что ссылка на каталог испорчена и файлы отрезаны от дерева.
    pub fn walk(&self) -> Result<Vec<Node>> {
        self.walk_inner(false).map(|r| r.nodes)
    }

    /// Как [`Vault::walk`], но повреждённые записи пропускаются и перечисляются в отчёте.
    pub fn walk_tolerant(&self) -> Result<WalkReport> {
        self.walk_inner(true)
    }

    fn walk_inner(&self, tolerant: bool) -> Result<WalkReport> {
        let mut report = WalkReport::default();
        let mut visited = std::collections::HashSet::new();
        visited.insert(String::new());
        let mut reached = std::collections::HashSet::new();
        let mut stack = vec![(String::new(), String::new(), 0usize)]; // (ID каталога, путь, глубина)
        while let Some((dir_id, path, depth)) = stack.pop() {
            let dir = self.root.join(names::dir_path(&self.key, &dir_id));
            reached.insert(dir.clone());
            let shown = if path.is_empty() { "/" } else { path.as_str() };
            let rd = match fs::read_dir(&dir) {
                Ok(rd) => rd,
                // Без папки корня хранилище не существует: это ошибка и при обходе с пропусками
                Err(e) if dir_id.is_empty() => return Err(e.into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if tolerant {
                        report
                            .skipped
                            .push(format!("{shown}: folder data is missing"));
                    }
                    continue;
                }
                Err(e) if tolerant => {
                    report.skipped.push(format!("{shown}: {e}"));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            for entry in rd {
                let read = entry
                    .map_err(Error::from)
                    .and_then(|e| self.read_entry(&e, &dir_id));
                let (name, classified) = match read {
                    Ok(Some(found)) => found,
                    Ok(None) => continue,
                    Err(e) if tolerant => {
                        report.skipped.push(format!("{shown}: {e}"));
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                if report.nodes.len() >= MAX_NODES {
                    return Err(Error::Authentication);
                }
                let child = if path.is_empty() {
                    name
                } else {
                    format!("{path}/{name}")
                };
                match classified {
                    Classified::Dir(child_id) => {
                        // Каталог, который уже встречался, или слишком глубокое дерево: хранилище испорчено
                        if !visited.insert(child_id.clone()) || depth + 1 > MAX_DEPTH {
                            if tolerant {
                                report.skipped.push(format!("{child}: damaged folder link"));
                                continue;
                            }
                            return Err(Error::Authentication);
                        }
                        report.nodes.push(Node::Dir {
                            path: child.clone(),
                        });
                        stack.push((child_id, child, depth + 1));
                    }
                    Classified::Empty => {
                        if tolerant {
                            report
                                .skipped
                                .push(format!("{child}: entry has no content"));
                        }
                    }
                    Classified::Symlink => report.nodes.push(Node::Symlink { path: child }),
                    Classified::File(ciphertext) => report.nodes.push(Node::File {
                        path: child,
                        ciphertext,
                    }),
                }
            }
        }
        let orphans = self.orphan_dirs(&reached)?;
        if !orphans.is_empty() {
            if !tolerant {
                return Err(Error::OrphanedData);
            }
            for dir in orphans {
                report.skipped.push(format!(
                    "{}: files are cut off from the tree",
                    dir.display()
                ));
            }
        }
        Ok(report)
    }

    /// Непустые папки каталогов в `d/`, до которых обход не дошёл (запасная копия ID не считается).
    fn orphan_dirs(&self, reached: &std::collections::HashSet<PathBuf>) -> Result<Vec<PathBuf>> {
        let mut orphans = Vec::new();
        for level1 in fs::read_dir(self.root.join("d"))? {
            let level1 = level1?;
            if !level1.file_type()?.is_dir() {
                continue;
            }
            for level2 in fs::read_dir(level1.path())? {
                let level2 = level2?;
                if !level2.file_type()?.is_dir() || reached.contains(&level2.path()) {
                    continue;
                }
                let mut entries = fs::read_dir(level2.path())?;
                if entries.any(|e| e.map_or(true, |e| e.file_name() != names::DIRID_BACKUP_FILE)) {
                    orphans.push(level2.path());
                }
            }
        }
        Ok(orphans)
    }

    /// Разобрать одну запись каталога: имя в открытом виде и что это. `None`: служебный файл.
    fn read_entry(
        &self,
        entry: &fs::DirEntry,
        dir_id: &str,
    ) -> Result<Option<(String, Classified)>> {
        let fname = entry.file_name().to_string_lossy().into_owned();
        if fname == names::DIRID_BACKUP_FILE {
            return Ok(None);
        }
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
            return Ok(None);
        };
        let name = names::decrypt_name(&self.key, &enc_name, dir_id)?;
        safe_name(&name)?;
        let is_dir_node = entry.file_type()?.is_dir();
        let classified = if is_dir_node && node_dir.join(DIR_FILE).is_file() {
            let child_id = fs::read_to_string(node_dir.join(DIR_FILE))?
                .trim()
                .to_string();
            Classified::Dir(child_id)
        } else if is_dir_node && node_dir.join(SYMLINK_FILE).is_file() {
            Classified::Symlink
        } else if is_dir_node && node_dir.join(CONTENTS_FILE).is_file() {
            Classified::File(node_dir.join(CONTENTS_FILE))
        } else if !is_dir_node {
            Classified::File(entry.path())
        } else {
            Classified::Empty
        };
        Ok(Some((name, classified)))
    }

    /// Выгрузить расшифрованную копию в папку `out` (должна быть пустой или новой).
    /// Ссылки (symlink) пропускаются. Возвращает число файлов.
    pub fn export(&self, out: &Path) -> Result<usize> {
        fs::create_dir_all(out)?;
        let mut files = 0;
        for node in self.walk()? {
            if self.export_node(out, &node)? {
                files += 1;
            }
        }
        Ok(files)
    }

    /// Выгрузить всё, что читается: последнее средство, которое не должно терять целую папку
    /// из-за одной плохой записи. Что не вышло, перечислено в отчёте.
    pub fn export_tolerant(&self, out: &Path) -> Result<ExportReport> {
        fs::create_dir_all(out)?;
        let walked = self.walk_tolerant()?;
        let mut report = ExportReport {
            files: 0,
            skipped: walked.skipped,
        };
        for node in walked.nodes {
            match self.export_node(out, &node) {
                Ok(true) => report.files += 1,
                Ok(false) => {}
                Err(e) => {
                    let (Node::Dir { path } | Node::File { path, .. } | Node::Symlink { path }) =
                        &node;
                    report.skipped.push(format!("{path}: {e}"));
                }
            }
        }
        Ok(report)
    }

    /// Выгрузить один узел. `true`, если записан файл.
    fn export_node(&self, out: &Path, node: &Node) -> Result<bool> {
        match node {
            Node::Dir { path } => fs::create_dir_all(out.join(path))?,
            Node::File { path, ciphertext } => {
                let target = out.join(path);
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
                return Ok(true);
            }
            Node::Symlink { .. } => {}
        }
        Ok(false)
    }
}

/// Создать новое хранилище в `dst` из содержимого папки `src` (для тестов совместимости).
/// `dst` не должна существовать или должна быть пустой.
pub fn create_from_dir(src: &Path, dst: &Path, password: &str, scrypt_cost: u32) -> Result<Vault> {
    let vault = Vault::create_empty(dst, password, scrypt_cost)?;
    vault.write_dir(src, "")?;
    Ok(vault)
}

/// Записать запасную копию ID каталога `d/XX/.../dirid.c9r`, как Cryptomator 1.19:
/// зашифрованный файл, содержимое = ID каталога. Помогает восстановить дерево,
/// если `dir.c9r` в родителе потерян.
pub fn write_dirid_backup(root: &Path, key: &MasterKey, dir_id: &str) -> Result<()> {
    let dir = root.join(names::dir_path(key, dir_id));
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join(names::DIRID_BACKUP_FILE),
        content::encrypt_file(key, dir_id.as_bytes())?,
    )?;
    Ok(())
}

impl Vault {
    /// Новое пустое хранилище в `dst` (папки не должно быть или она пустая) с новым
    /// мастер-ключом, защищённым паролем.
    pub fn create_empty(dst: &Path, password: &str, scrypt_cost: u32) -> Result<Vault> {
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
        write_dirid_backup(&vault.root, &vault.key, "")?;
        Ok(vault)
    }

    /// Узел для имени `name` в каталоге `parent_id`: путь на диске; если имя сокращено,
    /// создаёт папку `.c9s` с `name.c9s`. Ошибка, если такой узел уже есть.
    fn new_node(&self, parent_id: &str, name: &str, as_dir: bool) -> Result<(PathBuf, bool)> {
        safe_name(name)?;
        let dir = self.root.join(names::dir_path(&self.key, parent_id));
        let (node, full) =
            names::node_name(&self.key, name, parent_id, self.config.shortening_threshold);
        let node_path = dir.join(&node);
        if node_path.exists() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "node already exists",
            )));
        }
        if as_dir || full.is_some() {
            fs::create_dir(&node_path)?;
        }
        if let Some(full) = &full {
            fs::write(node_path.join(NAME_FILE), full)?;
        }
        Ok((node_path, full.is_some()))
    }

    /// Создать каталог `name` внутри каталога `parent_id` (корень: `""`). Возвращает ID
    /// нового каталога и папку его узла на диске (по ней видны время и атрибуты каталога).
    pub fn add_dir(&self, parent_id: &str, name: &str) -> Result<(String, PathBuf)> {
        let (node_path, _) = self.new_node(parent_id, name, true)?;
        let child_id = crate::uuid_v4()?;
        fs::write(node_path.join(DIR_FILE), &child_id)?;
        fs::create_dir_all(self.root.join(names::dir_path(&self.key, &child_id)))?;
        write_dirid_backup(&self.root, &self.key, &child_id)?;
        Ok((child_id, node_path))
    }

    /// Записать файл `name` в каталог `parent_id`, читая содержимое из потока кусками.
    /// Возвращает путь шифротекста на диске и длину с SHA-256 прочитанного открытого текста.
    pub fn add_file(
        &self,
        parent_id: &str,
        name: &str,
        input: &mut impl std::io::Read,
    ) -> Result<(PathBuf, content::Digest)> {
        let (node_path, shortened) = self.new_node(parent_id, name, false)?;
        let target = if shortened {
            node_path.join(CONTENTS_FILE)
        } else {
            node_path
        };
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let mut out = std::io::BufWriter::with_capacity(256 * 1024, file);
        let digest = content::encrypt_stream(&self.key, input, &mut out)?;
        use std::io::Write;
        out.flush()?;
        Ok((target, digest))
    }

    fn write_dir(&self, src: &Path, dir_id: &str) -> Result<()> {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Authentication)?;
            if entry.file_type()?.is_dir() {
                let (child_id, _) = self.add_dir(dir_id, &name)?;
                self.write_dir(&entry.path(), &child_id)?;
            } else {
                self.add_file(dir_id, &name, &mut fs::File::open(entry.path())?)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Хранилище: файл в корне, папка с файлом, пустая папка.
    fn sample(root: &Path) -> Vault {
        let v = Vault::create_empty(root, "pw", 1 << 4).unwrap();
        v.add_file("", "a.txt", &mut &b"aaa"[..]).unwrap();
        let (sub, _) = v.add_dir("", "sub").unwrap();
        v.add_file(&sub, "b.txt", &mut &b"bbb"[..]).unwrap();
        v.add_dir("", "empty").unwrap();
        v
    }

    fn file_count(nodes: &[Node]) -> usize {
        nodes
            .iter()
            .filter(|n| matches!(n, Node::File { .. }))
            .count()
    }

    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zamok-tree-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn missing_folder_of_a_directory_counts_as_empty() {
        let d = tempdir("missing");
        let v = Vault::create_empty(&d.join("v"), "pw", 1 << 4).unwrap();
        v.add_file("", "a.txt", &mut &b"aaa"[..]).unwrap();
        let (sub, _) = v.add_dir("", "sub").unwrap();
        fs::remove_dir_all(v.root.join(names::dir_path(&v.key, &sub))).unwrap();
        let nodes = v.walk().unwrap();
        assert_eq!((nodes.len(), file_count(&nodes)), (2, 1));
        // Без папки корня хранилища нет
        fs::remove_dir_all(v.root.join(names::dir_path(&v.key, ""))).unwrap();
        assert!(v.walk().is_err());
        assert!(v.walk_tolerant().is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn damaged_directory_link_is_not_an_empty_directory() {
        let d = tempdir("link");
        let v = sample(&d.join("v"));
        // Ссылка папки sub указывает в никуда, файлы остались в d/ без дерева
        let dir = v.root.join(names::dir_path(&v.key, ""));
        let mut changed = 0;
        for e in fs::read_dir(&dir).unwrap() {
            let link = e.unwrap().path().join(DIR_FILE);
            if link.is_file()
                && fs::read_dir(v.root.join(names::dir_path(
                    &v.key,
                    fs::read_to_string(&link).unwrap().trim(),
                )))
                .unwrap()
                .count()
                    > 1
            {
                fs::write(&link, "00000000-0000-0000-0000-000000000000").unwrap();
                changed += 1;
            }
        }
        assert_eq!(changed, 1);
        assert!(matches!(v.walk(), Err(Error::OrphanedData)));
        let out = d.join("out");
        let r = v.export_tolerant(&out).unwrap();
        assert!(
            r.skipped.iter().any(|s| s.contains("cut off")),
            "{:?}",
            r.skipped
        );
        assert_eq!(r.files, 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn one_bad_entry_fails_strict_walk_and_is_reported_by_tolerant() {
        let d = tempdir("bad");
        let v = sample(&d.join("v"));
        // Имя не расшифровывается
        let dir = v.root.join(names::dir_path(&v.key, ""));
        fs::write(dir.join("AAAAAAAAAAAAAAAA.c9r"), b"x").unwrap();
        // Сокращённое имя потеряло name.c9s
        fs::create_dir(dir.join("broken.c9s")).unwrap();
        assert!(v.walk().is_err());
        assert!(v.export(&d.join("strict")).is_err());
        let w = v.walk_tolerant().unwrap();
        assert_eq!(file_count(&w.nodes), 2);
        assert_eq!(w.skipped.len(), 2);
        let out = d.join("out");
        let r = v.export_tolerant(&out).unwrap();
        assert_eq!((r.files, r.skipped.len()), (2, 2));
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"aaa");
        assert_eq!(fs::read(out.join("sub").join("b.txt")).unwrap(), b"bbb");
        assert!(out.join("empty").is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn damaged_file_is_reported_and_the_rest_is_exported() {
        let d = tempdir("file");
        let v = sample(&d.join("v"));
        let (ct, _) = v.add_file("", "bad.bin", &mut &b"secret"[..]).unwrap();
        fs::write(&ct, b"not a ciphertext").unwrap();
        let out = d.join("out");
        let r = v.export_tolerant(&out).unwrap();
        assert_eq!(r.files, 2);
        assert_eq!(r.skipped.len(), 1);
        assert!(r.skipped[0].starts_with("bad.bin"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn loop_of_directories_is_skipped_not_followed() {
        let d = tempdir("loop");
        let v = sample(&d.join("v"));
        let (_, node) = v.add_dir("", "loop").unwrap();
        fs::write(node.join(DIR_FILE), "").unwrap();
        assert!(v.walk().is_err());
        let r = v.walk_tolerant().unwrap();
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(file_count(&r.nodes), 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_path_tricks() {
        for bad in [
            "..",
            ".",
            "a/b",
            "a\\b",
            "c:",
            "x\0y",
            "",
            "CON",
            "nul.txt",
            "com1.log",
            "COM¹",
            "lpt².txt",
            "conin$",
            "CONOUT$.x",
            "end.",
            "end ",
        ] {
            assert!(safe_name(bad).is_err(), "{bad:?}");
        }
        assert!(safe_name("Отчёт 2026.docx").is_ok());
    }
}
