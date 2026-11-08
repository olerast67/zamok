//! Состояние всех папок и операции над ними: подключить, открыть, запереть, добавить, убрать.
//!
//! Блокировки короткие: долгие действия (проверка пароля через scrypt около секунды,
//! ожидание сброса кэша ядра) идут без общего замка, поэтому `status` и запирание
//! других папок не ждут, пока кто-то вводит пароль.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use zamok_fs::fs::{Control, UnlockError};
use zamok_fs::mount::{notify_shell, settle_lock, Mounted};

use crate::config::{self, Config, FolderConfig, DEFAULT_IDLE_LOCK_SECS};
use crate::encrypt::Progress;
use crate::mountpoint::{self, PathState};
use crate::protocol::{ErrorCode, FolderInfo, FolderState, JobInfo};

pub struct Entry {
    cfg: Mutex<FolderConfig>,
    /// Подключённый том. `None`: не подключена (см. `error`).
    mounted: Mutex<Option<Mounted>>,
    /// Копия управления томом, чтобы открывать и запирать, не держа `mounted`.
    control: Mutex<Option<Arc<Control>>>,
    error: Mutex<Option<ErrorCode>>,
    /// Растёт при каждом запирании этой папки. Открытие читает его в самом начале (ещё до
    /// подключения тома) и ставит ключ, только если число не изменилось: запирание,
    /// пришедшее посреди открытия, всегда его отменяет.
    lock_epoch: AtomicU64,
    /// Уже сказали, что запирание ждёт закрытия файла (чтобы не повторять каждые 2 секунды).
    waiting_told: AtomicBool,
}

/// Что произошло при запирании по бездействию (для уведомления в трее).
pub enum IdleEvent {
    Locked { name: String, minutes: u64 },
    Waiting { name: String, minutes: u64 },
}

impl Entry {
    fn new(cfg: FolderConfig) -> Arc<Self> {
        Arc::new(Entry {
            cfg: Mutex::new(cfg),
            mounted: Mutex::new(None),
            control: Mutex::new(None),
            error: Mutex::new(None),
            lock_epoch: AtomicU64::new(0),
            waiting_told: AtomicBool::new(false),
        })
    }

    fn cfg(&self) -> FolderConfig {
        self.cfg.lock().unwrap().clone()
    }

    fn control(&self) -> Option<Arc<Control>> {
        self.control.lock().unwrap().clone()
    }

    fn info(&self) -> FolderInfo {
        let cfg = self.cfg();
        let state = match self.control() {
            None => FolderState::Unavailable,
            Some(c) if c.is_locked() => FolderState::Locked,
            Some(_) => FolderState::Unlocked,
        };
        FolderInfo {
            id: cfg.id.clone(),
            name: cfg.name(),
            path: cfg.path.to_string_lossy().into_owned(),
            state,
            idle_lock_secs: cfg.idle_lock_secs,
            error: *self.error.lock().unwrap(),
        }
    }

    /// Подключить запертой, если ещё не подключена.
    fn ensure_mounted(&self) -> Result<Arc<Control>, ErrorCode> {
        let mut mounted = self.mounted.lock().unwrap();
        if let Some(m) = mounted.as_ref() {
            return Ok(m.control().clone());
        }
        let cfg = self.cfg();
        let result = mount_folder(&cfg);
        *self.error.lock().unwrap() = result.as_ref().err().copied();
        let m = result?;
        let control = m.control().clone();
        *self.control.lock().unwrap() = Some(control.clone());
        *mounted = Some(m);
        tracing::info!(folder = %cfg.id, "mounted (locked)");
        Ok(control)
    }

    /// Запереть и отменить идущие открытия. `true`, если папка была открыта.
    fn lock_now(&self) -> Option<Arc<Control>> {
        self.lock_epoch.fetch_add(1, Ordering::SeqCst);
        let c = self.control()?;
        c.lock().then_some(c)
    }

    fn unmount(&self) {
        *self.control.lock().unwrap() = None;
        // Drop стирает ключи и отключает том
        drop(self.mounted.lock().unwrap().take());
    }
}

fn vault_looks_valid(vault: &Path) -> bool {
    vault.join("masterkey.cryptomator").is_file() && vault.join("vault.cryptomator").is_file()
}

fn mount_folder(cfg: &FolderConfig) -> Result<Mounted, ErrorCode> {
    if !vault_looks_valid(&cfg.vault) {
        tracing::warn!(folder = %cfg.id, "vault files are missing");
        return Err(ErrorCode::BadVault);
    }
    match mountpoint::prepare(&cfg.path) {
        Ok(()) => {}
        Err(PathState::StaleMount) => {
            tracing::warn!(folder = %cfg.id, "stale mount point could not be removed");
            return Err(ErrorCode::MountFailed);
        }
        Err(state) => {
            tracing::warn!(folder = %cfg.id, ?state, "mount path is occupied");
            return Err(ErrorCode::PathOccupied);
        }
    }
    // Хранилище не должно мешать в Проводнике: скрытое и системное (и у старых папок)
    crate::encrypt::set_hidden(&cfg.vault);
    Mounted::mount(&cfg.vault, &cfg.path, &cfg.name()).map_err(|e| {
        // Текст ошибки WinFsp без путей: код NTSTATUS
        tracing::warn!(folder = %cfg.id, error = %e, "mount failed");
        ErrorCode::MountFailed
    })
}

pub struct Core {
    data_dir: PathBuf,
    /// Общие настройки; список папок в нём не используется (он в `entries`).
    settings: Mutex<Config>,
    entries: Mutex<Vec<Arc<Entry>>>,
    /// Работы шифрования этого запуска (для `job_status`).
    jobs: Mutex<Vec<Arc<Progress>>>,
    /// Папки, которые сейчас расшифровываются навсегда: открывать их нельзя, иначе
    /// изменения во время копирования потерялись бы.
    busy: Mutex<std::collections::HashSet<String>>,
    quit: AtomicBool,
}

impl Core {
    /// Прочитать список и подключить все папки запертыми. Папки, которые не подключились
    /// (диск не вставлен, путь занят), остаются в списке с ошибкой; при открытии core
    /// попробует подключить их ещё раз.
    pub fn start(data_dir: &Path) -> anyhow::Result<Arc<Self>> {
        let mut cfg = Config::load(data_dir)?;
        let folders = std::mem::take(&mut cfg.folders);
        let core = Arc::new(Core {
            data_dir: data_dir.to_path_buf(),
            settings: Mutex::new(cfg),
            entries: Mutex::new(folders.into_iter().map(Entry::new).collect()),
            jobs: Mutex::new(Vec::new()),
            busy: Mutex::new(std::collections::HashSet::new()),
            quit: AtomicBool::new(false),
        });
        let entries = core.entries.lock().unwrap().clone();
        for e in entries {
            let _ = e.ensure_mounted();
        }
        // Шифрования, прерванные сбоем: довести до конца или откатить
        crate::encrypt::recover(&core, data_dir);
        crate::decrypt::recover(&core, data_dir);
        Ok(core)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Пути всех зашифрованных папок и их хранилищ (шифровать внутри них нельзя).
    pub fn taken_paths(&self) -> Vec<PathBuf> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .flat_map(|e| {
                let c = e.cfg();
                [c.path, c.vault]
            })
            .collect()
    }

    /// Добавить папку, зашифрованную на месте. Уже в списке: только подключить.
    pub fn register_encrypted(
        &self,
        path: &Path,
        vault: &Path,
        idle_lock_secs: u64,
    ) -> Result<(), ErrorCode> {
        let p = path.to_string_lossy();
        if let Ok(e) = self.find(&p) {
            return e.ensure_mounted().map(|_| ());
        }
        self.add(&p, &vault.to_string_lossy(), Some(idle_lock_secs))
            .map(|_| ())
    }

    pub fn add_job(&self, p: Arc<Progress>) {
        self.jobs.lock().unwrap().push(p);
    }

    pub fn job(&self, id: &str) -> Option<Arc<Progress>> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.id == id)
            .cloned()
    }

    pub fn job_running(&self) -> bool {
        self.jobs.lock().unwrap().iter().any(|j| j.is_running())
    }

    pub fn jobs(&self) -> Vec<JobInfo> {
        self.jobs.lock().unwrap().iter().map(|j| j.info()).collect()
    }

    pub fn lock_on_screen_lock(&self) -> bool {
        self.settings.lock().unwrap().lock_on_screen_lock
    }

    pub fn should_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    pub fn request_quit(&self) {
        self.quit.store(true, Ordering::SeqCst);
    }

    fn save(&self, entries: &[Arc<Entry>]) -> Result<(), ErrorCode> {
        let mut cfg = self.settings.lock().unwrap().clone();
        cfg.folders = entries.iter().map(|e| e.cfg()).collect();
        cfg.save(&self.data_dir).map_err(|e| {
            tracing::error!(error = %e, "cannot save folder list");
            ErrorCode::Internal
        })
    }

    fn find(&self, path: &str) -> Result<Arc<Entry>, ErrorCode> {
        let entries = self.entries.lock().unwrap();
        let probe = Config {
            folders: entries.iter().map(|e| e.cfg()).collect(),
            ..Default::default()
        };
        probe
            .find(Path::new(path))
            .map(|i| entries[i].clone())
            .ok_or(ErrorCode::NotFound)
    }

    pub fn status(&self) -> Vec<FolderInfo> {
        let entries = self.entries.lock().unwrap().clone();
        entries.iter().map(|e| e.info()).collect()
    }

    pub fn unlock(&self, path: &str, password: &str) -> Result<FolderInfo, ErrorCode> {
        let e = self.find(path)?;
        if self.is_busy(&e) {
            return Err(ErrorCode::JobRunning);
        }
        let epoch = e.lock_epoch.load(Ordering::SeqCst);
        let control = e.ensure_mounted()?;
        let id = e.cfg().id;
        let still_wanted = || e.lock_epoch.load(Ordering::SeqCst) == epoch;
        match control.unlock_with_password_if(password, &still_wanted) {
            Ok(()) => {
                notify_shell(&e.cfg().path);
                tracing::info!(folder = %id, "unlocked with password");
                Ok(e.info())
            }
            Err(UnlockError::Cancelled) => {
                tracing::info!(folder = %id, "unlock cancelled by a lock");
                Err(ErrorCode::UnlockCancelled)
            }
            Err(UnlockError::Crypto(zamok_crypto::Error::WrongPassword)) => {
                tracing::info!(folder = %id, "wrong password");
                Err(ErrorCode::WrongPassword)
            }
            Err(err) => {
                tracing::warn!(folder = %id, error = %err, "unlock failed");
                Err(ErrorCode::BadVault)
            }
        }
    }

    pub fn unlock_recovery(&self, path: &str, words: &str) -> Result<FolderInfo, ErrorCode> {
        let e = self.find(path)?;
        if self.is_busy(&e) {
            return Err(ErrorCode::JobRunning);
        }
        let epoch = e.lock_epoch.load(Ordering::SeqCst);
        let key = zamok_crypto::recovery::decode(words).map_err(|_| ErrorCode::WrongRecoveryKey)?;
        let control = e.ensure_mounted()?;
        let id = e.cfg().id;
        let still_wanted = || e.lock_epoch.load(Ordering::SeqCst) == epoch;
        match control.unlock_with_key_if(key, &still_wanted) {
            Ok(()) => {
                notify_shell(&e.cfg().path);
                tracing::info!(folder = %id, "unlocked with recovery key");
                Ok(e.info())
            }
            // Подпись настроек хранилища не сошлась: ключ от другой папки
            Err(UnlockError::Crypto(zamok_crypto::Error::BadVaultConfig)) => {
                Err(ErrorCode::WrongRecoveryKey)
            }
            Err(UnlockError::Cancelled) => Err(ErrorCode::UnlockCancelled),
            Err(err) => {
                tracing::warn!(folder = %id, error = %err, "unlock with recovery key failed");
                Err(ErrorCode::BadVault)
            }
        }
    }

    /// Путь, хранилище и время автозапирания зарегистрированной папки.
    pub fn folder_paths(&self, path: &str) -> Result<(PathBuf, PathBuf, u64), ErrorCode> {
        let c = self.find(path)?.cfg();
        Ok((c.path, c.vault, c.idle_lock_secs))
    }

    /// Проверить пароль и получить мастер-ключ (без подключения папки).
    pub fn verify_password(
        &self,
        path: &str,
        password: &str,
    ) -> Result<zamok_crypto::MasterKey, ErrorCode> {
        self.master_key(path, password).map(|(_, k)| k)
    }

    /// Сколько файлов папки сейчас открыто программами.
    pub fn open_files(&self, path: &str) -> usize {
        self.find(path)
            .ok()
            .and_then(|e| e.control())
            .map(|c| c.open_files())
            .unwrap_or(0)
    }

    pub fn set_busy(&self, path: &Path, busy: bool) {
        let key = config::normalize(path);
        let mut b = self.busy.lock().unwrap();
        if busy {
            b.insert(key);
        } else {
            b.remove(&key);
        }
    }

    fn is_busy(&self, e: &Entry) -> bool {
        self.busy
            .lock()
            .unwrap()
            .contains(&config::normalize(&e.cfg().path))
    }

    /// Мастер-ключ папки по паролю (из `masterkey.cryptomator`, без подключения).
    fn master_key(
        &self,
        path: &str,
        password: &str,
    ) -> Result<(Arc<Entry>, zamok_crypto::MasterKey), ErrorCode> {
        let e = self.find(path)?;
        let file = e.cfg().vault.join("masterkey.cryptomator");
        let json = std::fs::read_to_string(&file).map_err(|_| ErrorCode::BadVault)?;
        let mk = zamok_crypto::masterkey::MasterkeyFile::from_json(&json)
            .map_err(|_| ErrorCode::BadVault)?;
        match mk.unlock(password) {
            Ok(key) => Ok((e, key)),
            Err(zamok_crypto::Error::WrongPassword) => Err(ErrorCode::WrongPassword),
            Err(_) => Err(ErrorCode::BadVault),
        }
    }

    /// Сменить пароль: тот же мастер-ключ под новым паролем. Файл заменяется атомарно,
    /// старый не сохраняется: пароль меняют, когда старый мог утечь.
    pub fn change_password(
        &self,
        path: &str,
        old: &str,
        new: &str,
    ) -> Result<FolderInfo, ErrorCode> {
        if new.is_empty() {
            return Err(ErrorCode::BadRequest);
        }
        let (e, key) = self.master_key(path, old)?;
        let vault = e.cfg().vault;
        let json = zamok_crypto::masterkey::MasterkeyFile::lock_with(
            &key,
            new,
            zamok_crypto::masterkey::SCRYPT_COST,
        )
        .map_err(|_| ErrorCode::Internal)?
        .to_json();
        let tmp = vault.join("masterkey.cryptomator.tmp");
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(json.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, vault.join("masterkey.cryptomator"))
        };
        if write().is_err() {
            let _ = std::fs::remove_file(&tmp);
            return Err(ErrorCode::Internal);
        }
        tracing::info!(folder = %e.cfg().id, "password changed");
        Ok(e.info())
    }

    /// Ключ восстановления (44 слова) по паролю.
    pub fn recovery_key(
        &self,
        path: &str,
        password: &str,
    ) -> Result<zeroize::Zeroizing<String>, ErrorCode> {
        let (e, key) = self.master_key(path, password)?;
        tracing::info!(folder = %e.cfg().id, "recovery key shown");
        Ok(zamok_crypto::recovery::encode(&key))
    }

    pub fn lock(&self, path: &str) -> Result<FolderInfo, ErrorCode> {
        let e = self.find(path)?;
        if let Some(c) = e.lock_now() {
            settle_lock(&c, &e.cfg().path);
            tracing::info!(folder = %e.cfg().id, "locked by request");
        }
        Ok(e.info())
    }

    /// Сразу стереть ключи всех открытых папок. Быстро: годится перед сном, когда
    /// на всё есть пара секунд. Возвращает, что ещё надо дождаться ([`Core::settle`]).
    pub fn lock_all_now(&self, reason: &str) -> Vec<(Arc<Control>, PathBuf)> {
        let entries = self.entries.lock().unwrap().clone();
        let mut locked = Vec::new();
        for e in entries {
            // Всем папкам, и запертым тоже: к ним сейчас может проверяться пароль
            if let Some(c) = e.lock_now() {
                locked.push((c, e.cfg().path));
            }
        }
        if !locked.is_empty() {
            tracing::info!(count = locked.len(), reason, "locked all folders");
        }
        locked
    }

    /// Дождаться сброса кэша ядра у только что запертых папок (параллельно).
    pub fn settle(locked: &[(Arc<Control>, PathBuf)]) {
        std::thread::scope(|s| {
            for (c, p) in locked {
                s.spawn(move || settle_lock(c, p));
            }
        });
    }

    pub fn lock_all(&self, reason: &str) {
        let locked = self.lock_all_now(reason);
        Self::settle(&locked);
    }

    /// Папки, которые человек удалил в Проводнике (DESIGN.md, раздел 5.1):
    /// - удалено хранилище, а его родительская папка на месте: данных больше нет, папка
    ///   убирается из списка, имя снова свободно. Если нет и родителя (диск не подключён),
    ///   папка остаётся в списке;
    /// - удалена только точка подключения, хранилище цело: папка возвращается на место
    ///   запертой (по умолчанию вернуть: данные целы).
    pub fn check_folders(&self) {
        let entries = self.entries.lock().unwrap().clone();
        let mut gone = Vec::new();
        for e in entries {
            let cfg = e.cfg();
            if std::fs::symlink_metadata(&cfg.vault).is_err() {
                if cfg.vault.parent().is_some_and(|p| p.is_dir()) {
                    gone.push(e);
                }
                continue;
            }
            if e.control().is_some() && !mountpoint::exists(&cfg.path) {
                e.unmount();
                let restored = e.ensure_mounted().is_ok();
                tracing::info!(folder = %cfg.id, restored, "mount point was deleted, folder restored locked");
            }
        }
        if gone.is_empty() {
            return;
        }
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|e| !gone.iter().any(|g| Arc::ptr_eq(e, g)));
        let _ = self.save(&entries);
        drop(entries);
        for e in gone {
            e.unmount();
            tracing::info!(folder = %e.cfg().id, "vault was deleted, folder removed from the list");
        }
    }

    /// Запереть папки, с которыми долго ничего не делали. Возвращает события для уведомлений:
    /// папка заперта, или пора запирать, но программа держит файл (о каждой папке один раз).
    pub fn lock_idle(&self) -> Vec<IdleEvent> {
        let entries = self.entries.lock().unwrap().clone();
        let mut events = Vec::new();
        for e in entries {
            let cfg = e.cfg();
            let secs = cfg.idle_lock_secs;
            if secs == 0 {
                continue;
            }
            let Some(c) = e.control() else { continue };
            if c.is_locked() || c.idle_for().as_secs() < secs {
                e.waiting_told.store(false, Ordering::SeqCst);
                continue;
            }
            // Пока программа держит файл из папки, по бездействию не запираем: иначе
            // открытый документ потом не сохранить. Win+L и сон запирают сразу.
            if c.open_files() > 0 {
                if !e.waiting_told.swap(true, Ordering::SeqCst) {
                    events.push(IdleEvent::Waiting {
                        name: cfg.name(),
                        minutes: secs / 60,
                    });
                }
                continue;
            }
            if c.lock() {
                settle_lock(&c, &cfg.path);
                e.waiting_told.store(false, Ordering::SeqCst);
                tracing::info!(folder = %cfg.id, "locked after idle timeout");
                events.push(IdleEvent::Locked {
                    name: cfg.name(),
                    minutes: secs / 60,
                });
            }
        }
        events
    }

    /// Общие настройки для окна настроек.
    pub fn settings(&self) -> crate::protocol::Settings {
        crate::protocol::Settings {
            lock_on_screen_lock: self.lock_on_screen_lock(),
        }
    }

    pub fn set_lock_on_screen_lock(&self, on: bool) -> Result<(), ErrorCode> {
        let old = std::mem::replace(&mut self.settings.lock().unwrap().lock_on_screen_lock, on);
        let entries = self.entries.lock().unwrap();
        if let Err(code) = self.save(&entries) {
            self.settings.lock().unwrap().lock_on_screen_lock = old;
            return Err(code);
        }
        Ok(())
    }

    pub fn add(
        &self,
        path: &str,
        vault: &str,
        idle_lock_secs: Option<u64>,
    ) -> Result<FolderInfo, ErrorCode> {
        let path = PathBuf::from(path);
        let vault = PathBuf::from(vault);
        if !path.is_absolute() || !vault.is_absolute() || path.parent().is_none() {
            return Err(ErrorCode::BadRequest);
        }
        // Хранилище внутри самой папки подключилось бы само в себя
        let (np, nv) = (config::normalize(&path), config::normalize(&vault));
        if nv == np || nv.starts_with(&format!("{np}\\")) {
            return Err(ErrorCode::BadRequest);
        }
        if !vault_looks_valid(&vault) {
            return Err(ErrorCode::BadVault);
        }
        let mut entries = self.entries.lock().unwrap();
        for e in entries.iter() {
            let c = e.cfg();
            let (ep, ev) = (config::normalize(&c.path), config::normalize(&c.vault));
            let nested = |a: &str, b: &str| a == b || a.starts_with(&format!("{b}\\"));
            if nested(&np, &ep) || nested(&ep, &np) || ev == nv {
                return Err(ErrorCode::AlreadyAdded);
            }
        }
        let cfg = FolderConfig {
            id: config::new_id().map_err(|_| ErrorCode::Internal)?,
            path,
            vault,
            idle_lock_secs: idle_lock_secs.unwrap_or(DEFAULT_IDLE_LOCK_SECS),
        };
        let entry = Entry::new(cfg);
        entry.ensure_mounted()?;
        entries.push(entry.clone());
        if let Err(code) = self.save(&entries) {
            entries.pop();
            entry.unmount();
            return Err(code);
        }
        tracing::info!(folder = %entry.cfg().id, "folder added");
        Ok(entry.info())
    }

    pub fn remove(&self, path: &str) -> Result<(), ErrorCode> {
        let target = self.find(path)?;
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| !Arc::ptr_eq(e, &target));
        if entries.len() == before {
            return Err(ErrorCode::NotFound);
        }
        if let Err(code) = self.save(&entries) {
            entries.push(target);
            return Err(code);
        }
        drop(entries);
        target.unmount();
        tracing::info!(folder = %target.cfg().id, "folder removed from the list");
        Ok(())
    }

    pub fn set_idle_lock(&self, path: &str, secs: u64) -> Result<FolderInfo, ErrorCode> {
        let target = self.find(path)?;
        let entries = self.entries.lock().unwrap();
        let old = std::mem::replace(&mut target.cfg.lock().unwrap().idle_lock_secs, secs);
        if let Err(code) = self.save(&entries) {
            target.cfg.lock().unwrap().idle_lock_secs = old;
            return Err(code);
        }
        Ok(target.info())
    }

    /// Запереть всё и отключить все папки.
    pub fn shutdown(&self) {
        let _ = self.lock_all_now("shutdown");
        let entries = self.entries.lock().unwrap().clone();
        for e in entries {
            e.unmount();
        }
        tracing::info!("all folders unmounted");
    }
}
