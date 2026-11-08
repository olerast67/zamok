//! Список зашифрованных папок и их настройки: `%LOCALAPPDATA%\Zamok\folders.json`.
//!
//! Файл пишется атомарно: новый текст во временный файл, сброс на диск, замена старого.
//! Выключение питания посреди записи оставляет либо старый список, либо новый.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Автозапирание по умолчанию: 30 минут без работы с папкой (DESIGN.md, раздел 11).
pub const DEFAULT_IDLE_LOCK_SECS: u64 = 30 * 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FolderConfig {
    /// Короткий номер для журнала и окон (в журнал путь не пишем).
    pub id: String,
    /// Где человек видит папку: на этот путь подключается файловая система.
    pub path: PathBuf,
    /// Скрытое хранилище с шифротекстом (`.Имя.zamok`).
    pub vault: PathBuf,
    /// Старое место хранилища, пока оно переносится следом за папкой: если core упал
    /// посреди переноса, при запуске видно, где хранилище на самом деле.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_prev: Option<PathBuf>,
    /// Запереть после стольких секунд без работы с папкой; 0 = не запирать по времени.
    #[serde(default = "default_idle")]
    pub idle_lock_secs: u64,
}

fn default_idle() -> u64 {
    DEFAULT_IDLE_LOCK_SECS
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    /// Запирать все папки при блокировке экрана (Win+L).
    #[serde(default = "default_true")]
    pub lock_on_screen_lock: bool,
    #[serde(default)]
    pub folders: Vec<FolderConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: 1,
            lock_on_screen_lock: true,
            folders: Vec::new(),
        }
    }
}

impl FolderConfig {
    /// Имя, которое видит человек: последняя часть пути.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Zamok".into())
    }

    /// Разобраться с переносом хранилища, прерванным сбоем. `true`, если запись изменилась.
    /// Новое место пусто, а старое занято: переименование не успело, хранилище на старом месте.
    pub fn recover_vault_move(&mut self) -> bool {
        let Some(prev) = self.vault_prev.take() else {
            return false;
        };
        if std::fs::symlink_metadata(&self.vault).is_err()
            && std::fs::symlink_metadata(&prev).is_ok()
        {
            self.vault = prev;
        }
        true
    }
}

impl Config {
    pub fn file(data_dir: &Path) -> PathBuf {
        data_dir.join("folders.json")
    }

    /// Прочитать список. Нет файла: пустой список. Испорченный файл: ошибка, а не пустой
    /// список, чтобы следующая запись не стёрла папки, о которых core просто не смог прочитать.
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let file = Self::file(data_dir);
        match std::fs::read(&file) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let file = Self::file(data_dir);
        let tmp = data_dir.join("folders.json.tmp");
        let text = serde_json::to_vec_pretty(self)?;
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&text)?;
            f.sync_all()?;
        }
        // Замена существующего файла одним действием (MoveFileEx с REPLACE_EXISTING)
        std::fs::rename(&tmp, &file)?;
        Ok(())
    }

    /// Папка, к которой относится путь: сама папка или что-то внутри неё
    /// (например `…\Клиенты\Открыть Клиенты.zamok`).
    pub fn find(&self, path: &Path) -> Option<usize> {
        let want = normalize(path);
        self.folders.iter().position(|f| {
            let base = normalize(&f.path);
            want == base || want.starts_with(&format!("{base}\\"))
        })
    }

    pub fn find_id(&self, id: &str) -> Option<usize> {
        self.folders.iter().position(|f| f.id == id)
    }
}

/// Путь для сравнения: обратные косые черты, без хвостовой черты, без `\\?\`, нижний регистр.
/// В Windows имена в папках сравниваются без учёта регистра.
pub fn normalize(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s.to_lowercase()
}

/// Новый короткий номер папки: 8 шестнадцатеричных знаков из UUID.
pub fn new_id() -> anyhow::Result<String> {
    let u = zamok_crypto::uuid_v4().map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(u.replace('-', "")[..8].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str) -> FolderConfig {
        FolderConfig {
            id: "a1".into(),
            path: path.into(),
            vault: format!("{path}.zamok").into(),
            vault_prev: None,
            idle_lock_secs: DEFAULT_IDLE_LOCK_SECS,
        }
    }

    #[test]
    fn interrupted_vault_move_finds_the_real_place() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("old");
        let new = d.path().join("new");
        let moving = |vault: &Path| FolderConfig {
            vault: vault.into(),
            vault_prev: Some(old.clone()),
            ..folder("x")
        };
        // Переименование не успело: хранилище на старом месте
        std::fs::create_dir(&old).unwrap();
        let mut f = moving(&new);
        assert!(f.recover_vault_move());
        assert_eq!((f.vault.as_path(), f.vault_prev), (old.as_path(), None));
        // Успело: хранилище уже на новом
        std::fs::rename(&old, &new).unwrap();
        let mut f = moving(&new);
        assert!(f.recover_vault_move());
        assert_eq!((f.vault.as_path(), f.vault_prev), (new.as_path(), None));
        // Переноса не было
        let mut f = folder("x");
        assert!(!f.recover_vault_move());
    }

    #[test]
    fn missing_file_is_empty_list() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(d.path()).unwrap(), Config::default());
    }

    #[test]
    fn roundtrip_and_no_tmp_left() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.folders.push(folder(r"C:\Работа\Клиенты"));
        c.save(d.path()).unwrap();
        c.folders[0].idle_lock_secs = 60;
        c.save(d.path()).unwrap();
        assert_eq!(Config::load(d.path()).unwrap(), c);
        assert!(!d.path().join("folders.json.tmp").exists());
    }

    #[test]
    fn damaged_file_is_an_error_not_an_empty_list() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(Config::file(d.path()), b"{\"version\":1,\"folders\":[").unwrap();
        assert!(Config::load(d.path()).is_err());
    }

    #[test]
    fn old_file_without_new_fields_gets_defaults() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            Config::file(d.path()),
            br#"{"version":1,"folders":[{"id":"x","path":"C:\\a","vault":"C:\\.a.zamok"}]}"#,
        )
        .unwrap();
        let c = Config::load(d.path()).unwrap();
        assert!(c.lock_on_screen_lock);
        assert_eq!(c.folders[0].idle_lock_secs, DEFAULT_IDLE_LOCK_SECS);
    }

    #[test]
    fn find_matches_folder_and_items_inside_ignoring_case() {
        let mut c = Config::default();
        c.folders.push(folder(r"C:\Работа\Клиенты"));
        c.folders.push(folder(r"C:\Работа\Клиенты2"));
        assert_eq!(c.find(Path::new(r"c:\работа\клиенты\")), Some(0));
        assert_eq!(
            c.find(Path::new(r"C:\Работа\Клиенты\Открыть Клиенты.zamok")),
            Some(0)
        );
        assert_eq!(c.find(Path::new(r"\\?\C:\Работа\Клиенты2")), Some(1));
        assert_eq!(c.find(Path::new(r"C:\Работа\Клиенты3")), None);
        assert_eq!(c.find(Path::new(r"C:\Работа")), None);
    }

    #[test]
    fn name_is_last_path_part() {
        assert_eq!(folder(r"C:\Работа\Клиенты").name(), "Клиенты");
    }
}
