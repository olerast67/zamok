//! Шифрование существующей папки на месте с журналом (DESIGN.md, раздел 5).
//!
//! Шаги и что лежит на диске после каждого (журнал пишется после шага):
//!
//! | Шаг | Действие | Оригинал | Зашифрованная копия |
//! |---|---|---|---|
//! | `copying` | каждый файл шифруется в `.Имя.zamok.tmp` и сразу сверяется расшифровкой | на месте | неполная |
//! | `verified` | вся копия сверена, папка не менялась, всё сброшено на диск | на месте | полная, `.tmp` |
//! | `vault_placed` | `.tmp` переименована в `.Имя.zamok` | на месте | полная |
//! | `original_aside` | оригинал переименован в `.Имя.zamok-original` и сверен с описью; не совпал: откат | рядом | полная |
//! | `registered` | папка в списке и подключена на старый путь | рядом | подключена |
//! | (готово) | `.zamok-original` удалена, журнал удалён | нет | подключена |
//!
//! Сбой на любом шаге оставляет либо нетронутый оригинал, либо полную проверенную копию.
//! Рекурсивно удаляются только две папки: `.Имя.zamok.tmp` (наша копия, при откате) и
//! `.Имя.zamok-original` (оригинал, только после шага `registered`).

use std::collections::HashMap;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zamok_crypto::tree::Vault;
use zeroize::Zeroizing;

use crate::core::Core;
use crate::preflight::{self, Kind, Plan};
use crate::protocol::{ErrorCode, JobInfo, JobState};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Copying,
    Verified,
    VaultPlaced,
    OriginalAside,
    Registered,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Copying => "copying",
            Phase::Verified => "verified",
            Phase::VaultPlaced => "vault_placed",
            Phase::OriginalAside => "original_aside",
            Phase::Registered => "registered",
        }
    }
}

/// Журнал работы: `%LOCALAPPDATA%\Zamok\jobs\<id>.json`. Лежит вне шифруемой папки
/// и вне хранилища.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Journal {
    pub version: u32,
    pub id: String,
    pub phase: Phase,
    pub path: PathBuf,
    pub tmp: PathBuf,
    pub vault: PathBuf,
    pub aside: PathBuf,
    pub idle_lock_secs: u64,
    /// SHA-256 файла `masterkey.cryptomator` копии этой работы. Любое переименование и
    /// удаление после сбоя идёт, только если отпечаток совпал: журнал никогда не тронет
    /// чужое хранилище с тем же именем.
    #[serde(default)]
    pub masterkey_sha256: Option<String>,
    /// SHA-256 файла `vault.cryptomator`: подписан мастер-ключом и, в отличие от
    /// `masterkey.cryptomator`, не меняется при смене пароля.
    #[serde(default)]
    pub vault_config_sha256: Option<String>,
    /// SHA-256 описи папки (имена, виды, размеры, время файлов). С ней сверяется оригинал
    /// после переименования в `.zamok-original`. В журналах старых версий поля нет.
    #[serde(default)]
    pub plan_sha256: Option<String>,
}

fn jobs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("jobs")
}

impl Journal {
    fn file(&self, data_dir: &Path) -> PathBuf {
        jobs_dir(data_dir).join(format!("{}.json", self.id))
    }

    /// Записать атомарно и сбросить на диск.
    fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        let dir = jobs_dir(data_dir);
        std::fs::create_dir_all(&dir)?;
        let file = self.file(data_dir);
        let tmp = dir.join(format!("{}.json.tmp", self.id));
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(self)?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &file)
    }

    fn set(&mut self, phase: Phase, data_dir: &Path) -> std::io::Result<()> {
        self.phase = phase;
        self.save(data_dir)
    }

    fn remove(&self, data_dir: &Path) {
        let _ = std::fs::remove_file(self.file(data_dir));
    }

    /// Остались ли журналы (в том числе нечитаемые).
    pub fn any_left(data_dir: &Path) -> bool {
        std::fs::read_dir(jobs_dir(data_dir))
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().ends_with(".json"))
            })
            .unwrap_or(false)
    }

    pub fn load_all(data_dir: &Path) -> Vec<Journal> {
        let Ok(rd) = std::fs::read_dir(jobs_dir(data_dir)) else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .filter_map(|e| {
                let parsed = std::fs::read(e.path())
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Journal>(&b).ok());
                if parsed.is_none() {
                    tracing::error!("unreadable job journal left in place");
                }
                parsed
            })
            .collect()
    }
}

/// Ход работы для окна. Общий для потока работы и канала.
pub struct Progress {
    pub id: String,
    pub path: PathBuf,
    pub(crate) files_done: AtomicU64,
    files_total: AtomicU64,
    pub(crate) bytes_done: AtomicU64,
    bytes_total: AtomicU64,
    pub(crate) phase: Mutex<Phase>,
    state: Mutex<JobState>,
    error: Mutex<Option<ErrorCode>>,
    pub(crate) cancel: AtomicBool,
}

impl Progress {
    pub fn new(id: &str, path: &Path, plan: &Plan) -> Arc<Self> {
        Self::with_totals(id, path, plan.files, plan.bytes)
    }

    pub fn with_totals(id: &str, path: &Path, files: u64, bytes: u64) -> Arc<Self> {
        Arc::new(Progress {
            id: id.to_string(),
            path: path.to_path_buf(),
            files_done: AtomicU64::new(0),
            files_total: AtomicU64::new(files),
            bytes_done: AtomicU64::new(0),
            bytes_total: AtomicU64::new(bytes),
            phase: Mutex::new(Phase::Copying),
            state: Mutex::new(JobState::Running),
            error: Mutex::new(None),
            cancel: AtomicBool::new(false),
        })
    }

    pub fn info(&self) -> JobInfo {
        JobInfo {
            id: self.id.clone(),
            path: self.path.to_string_lossy().into_owned(),
            state: *self.state.lock().unwrap(),
            phase: self.phase.lock().unwrap().name().to_string(),
            files_done: self.files_done.load(Ordering::Relaxed),
            files_total: self.files_total.load(Ordering::Relaxed),
            bytes_done: self.bytes_done.load(Ordering::Relaxed),
            bytes_total: self.bytes_total.load(Ordering::Relaxed),
            error: *self.error.lock().unwrap(),
        }
    }

    pub fn is_running(&self) -> bool {
        *self.state.lock().unwrap() == JobState::Running
    }

    /// Отмена возможна только до подмены папки (шаги copying и verified).
    pub fn request_cancel(&self) -> bool {
        let phase = *self.phase.lock().unwrap();
        if matches!(phase, Phase::Copying | Phase::Verified) && self.is_running() {
            self.cancel.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub(crate) fn finish(&self, result: Result<(), ErrorCode>) {
        *self.error.lock().unwrap() = result.err();
        *self.state.lock().unwrap() = if result.is_ok() {
            JobState::Done
        } else {
            JobState::Failed
        };
    }
}

/// Точки аварийного завершения для теста «выключение на каждом шаге»: переменная
/// `ZAMOK_TEST_CRASH` с именем точки. Процесс снимается так же грубо, как при сбое.
pub(crate) fn crash_point(name: &str) {
    if std::env::var("ZAMOK_TEST_CRASH").is_ok_and(|v| v == name) {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        unsafe {
            let _ = TerminateProcess(GetCurrentProcess(), 99);
        }
        // TerminateProcess только начинает завершение: дальше этот поток идти не должен
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Точка ожидания для теста «файл появился, пока идёт сброс на диск»: переменная
/// `ZAMOK_TEST_PAUSE` вида `точка|файл`. Core создаёт `файл.reached` и ждёт, пока тест
/// создаст `файл`.
fn pause_point(name: &str) {
    let Ok(value) = std::env::var("ZAMOK_TEST_PAUSE") else {
        return;
    };
    let Some((point, gate)) = value.split_once('|') else {
        return;
    };
    if point != name {
        return;
    }
    let _ = std::fs::write(format!("{gate}.reached"), b"");
    for _ in 0..2400 {
        if Path::new(gate).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Переименование с повторами: антивирус и индексатор ненадолго держат свежие папки.
pub(crate) fn rename_retry(from: &Path, to: &Path, attempts: u32) -> std::io::Result<()> {
    let mut last = None;
    for i in 0..attempts {
        if std::fs::symlink_metadata(to).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "rename target exists",
            ));
        }
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        if i + 1 < attempts {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    Err(last.unwrap())
}

/// Скрытая и системная: Проводник не показывает такую папку даже с включённым показом
/// скрытых файлов (только если снять «Скрывать защищённые системные файлы»).
pub fn set_hidden(path: &Path) {
    use std::os::windows::fs::MetadataExt;
    if let Ok(m) = std::fs::metadata(path) {
        let want = (m.file_attributes() | 0x2 | 0x4) & 0x7;
        if m.file_attributes() & 0x7 != want {
            let _ = zamok_fs::attrs::set_attributes(path, want);
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn file_digest(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    Some(hex(&Sha256::digest(std::fs::read(path).ok()?)))
}

/// SHA-256 файла `masterkey.cryptomator` в папке хранилища.
pub(crate) fn masterkey_digest(dir: &Path) -> Option<String> {
    file_digest(&dir.join("masterkey.cryptomator"))
}

/// SHA-256 файла `vault.cryptomator` в папке хранилища.
pub(crate) fn vault_config_digest(dir: &Path) -> Option<String> {
    file_digest(&dir.join("vault.cryptomator"))
}

/// Хранилище в `dir` создано этой работой: совпал отпечаток мастер-ключа или настроек
/// хранилища. Настройки подписаны тем же мастер-ключом и не меняются при смене пароля:
/// иначе после смены пароля оригинал остался бы на диске навсегда.
fn is_ours(j: &Journal, dir: &Path) -> bool {
    (j.masterkey_sha256.is_some() && masterkey_digest(dir) == j.masterkey_sha256)
        || (j.vault_config_sha256.is_some() && vault_config_digest(dir) == j.vault_config_sha256)
}

/// Отпечаток описи: длины имён входят в него, поэтому разные описи не склеятся.
fn plan_digest(plan: &Plan) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update((plan.items.len() as u64).to_le_bytes());
    for item in &plan.items {
        h.update((item.rel.len() as u64).to_le_bytes());
        h.update(item.rel.as_bytes());
        match item.kind {
            Kind::Dir => h.update([0u8]),
            Kind::File { size, mtime } => {
                h.update([1u8]);
                h.update(size.to_le_bytes());
                h.update(mtime.to_le_bytes());
            }
        }
    }
    hex(&h.finalize())
}

/// Совпадает ли папка `dir` с описью из журнала. `None`: журнал старой версии, описи в нём
/// нет. Прочитать папку не вышло: считается, что не совпадает.
fn matches_plan(j: &Journal, dir: &Path) -> Option<bool> {
    let want = j.plan_sha256.as_ref()?;
    let mut bad = Vec::new();
    Some(match preflight::walk(dir, &mut bad) {
        Ok(now) => bad.is_empty() && plan_digest(&now) == *want,
        Err(_) => false,
    })
}

/// Сверить оригинал с описью перед подключением копии. Скрытое имя уже закрыло папку для
/// записи, поэтому эта сверка последняя и видит всё, что появилось за время сброса на диск.
fn check_aside(core: &Core, j: &Journal, data_dir: &Path) -> Result<(), ErrorCode> {
    if std::fs::symlink_metadata(&j.aside).is_ok() {
        if matches_plan(j, &j.aside) == Some(false) {
            tracing::warn!(job = %j.id, "the folder changed after the last check, rolling back");
            // Сбой мог случиться уже после подключения: снять папку из списка, хранилище цело
            let _ = core.remove(&j.path.to_string_lossy());
            rollback_aside(j, data_dir);
            return Err(ErrorCode::ChangedDuringCopy);
        }
    } else if matches_plan(j, &j.path) == Some(true) {
        // Прошлый откат вернул оригинал на место и прервался
        rollback_copy(j, data_dir);
        return Err(ErrorCode::ChangedDuringCopy);
    }
    Ok(())
}

/// Удалить нашу временную копию. Только папку с именем `.….zamok.tmp` из журнала и
/// только нашу: с нашим мастер-ключом, или (если отпечаток ещё не записан) созданную
/// после журнала (раньше проверки не пускают, пока на этом месте что-то есть).
fn remove_tmp(j: &Journal) -> std::io::Result<()> {
    let name = j.tmp.file_name().map(|n| n.to_string_lossy().into_owned());
    if !name.is_some_and(|n| n.starts_with('.') && n.ends_with(".zamok.tmp")) {
        return Err(std::io::Error::other("refusing to delete: not a tmp vault"));
    }
    if std::fs::symlink_metadata(&j.tmp).is_err() {
        return Ok(());
    }
    if j.masterkey_sha256.is_some() && !is_ours(j, &j.tmp) {
        return Err(std::io::Error::other(
            "refusing to delete: tmp vault is not from this job",
        ));
    }
    match std::fs::remove_dir_all(&j.tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub(crate) fn file_mtime(m: &std::fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    m.last_write_time()
}

/// Скопировать всё в хранилище, сверяя каждый файл расшифровкой с отпечатком оригинала.
/// В конце убедиться, что в папке за это время ничего не изменилось.
/// Возвращает узлы с атрибутами, которые надо поставить после сброса на диск
/// («только чтение» не дал бы открыть файл для сброса).
fn copy_and_verify(
    vault: &Vault,
    src: &Path,
    plan: &Plan,
    progress: &Progress,
) -> Result<Vec<(PathBuf, u32)>, ErrorCode> {
    let mut attrs_later = Vec::new();
    let mut dir_ids: HashMap<String, String> = HashMap::new();
    dir_ids.insert(String::new(), String::new());
    let crash_after: Option<u64> = std::env::var("ZAMOK_TEST_CRASH")
        .ok()
        .and_then(|v| v.strip_prefix("copy:").and_then(|n| n.parse().ok()));
    for item in &plan.items {
        if progress.cancel.load(Ordering::SeqCst) {
            return Err(ErrorCode::Cancelled);
        }
        let (parent, name) = item.rel.rsplit_once('\\').unwrap_or(("", &item.rel));
        let parent_id = dir_ids.get(parent).cloned().ok_or(ErrorCode::CopyFailed)?;
        match &item.kind {
            Kind::Dir => {
                let (id, node) = vault
                    .add_dir(&parent_id, name)
                    .map_err(|_| ErrorCode::CopyFailed)?;
                keep_meta(&node, &item.meta, &mut attrs_later)?;
                dir_ids.insert(item.rel.clone(), id);
            }
            Kind::File { size, mtime } => {
                let path = src.join(&item.rel);
                // Читаем, разрешая другим только чтение: писать в файл, пока мы его
                // копируем, никто не сможет
                let mut f = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1) // FILE_SHARE_READ
                    .open(&path)
                    .map_err(|_| ErrorCode::ChangedDuringCopy)?;
                let before = f.metadata().map_err(|_| ErrorCode::CopyFailed)?;
                if before.len() != *size || file_mtime(&before) != *mtime {
                    return Err(ErrorCode::ChangedDuringCopy);
                }
                let mut reader = std::io::BufReader::with_capacity(256 * 1024, &mut f);
                let (ct, digest) = vault
                    .add_file(&parent_id, name, &mut reader)
                    .map_err(|_| ErrorCode::CopyFailed)?;
                drop(reader);
                if digest.0 != *size {
                    return Err(ErrorCode::ChangedDuringCopy);
                }
                // Сверка: расшифровать записанное и сравнить отпечаток с прочитанным
                let mut ctf = std::io::BufReader::with_capacity(
                    256 * 1024,
                    std::fs::File::open(&ct).map_err(|_| ErrorCode::VerifyFailed)?,
                );
                let back = zamok_crypto::content::decrypt_stream_digest(vault.key(), &mut ctf)
                    .map_err(|_| ErrorCode::VerifyFailed)?;
                if back != digest {
                    return Err(ErrorCode::VerifyFailed);
                }
                let after = std::fs::metadata(&path).map_err(|_| ErrorCode::ChangedDuringCopy)?;
                if after.len() != *size || file_mtime(&after) != *mtime {
                    return Err(ErrorCode::ChangedDuringCopy);
                }
                keep_meta(&ct, &item.meta, &mut attrs_later)?;
                progress.bytes_done.fetch_add(*size, Ordering::Relaxed);
                let done = progress.files_done.fetch_add(1, Ordering::Relaxed) + 1;
                if crash_after == Some(done) {
                    crash_point(&format!("copy:{done}"));
                }
            }
        }
    }
    // Пока копировали, в папке ничего не появилось, не исчезло и не изменилось
    let mut bad = Vec::new();
    let now = preflight::walk(src, &mut bad).map_err(|_| ErrorCode::ChangedDuringCopy)?;
    // Сравниваем имена, виды, размеры и время файлов. Время папок NTFS обновляет с
    // задержкой после изменений внутри, а изменения файлов видны и так.
    if !bad.is_empty() || plan_digest(&now) != plan_digest(plan) {
        return Err(ErrorCode::ChangedDuringCopy);
    }
    Ok(attrs_later)
}

/// Перенести время создания и изменения на узел шифротекста, атрибуты отложить.
fn keep_meta(
    node: &Path,
    meta: &preflight::Meta,
    attrs_later: &mut Vec<(PathBuf, u32)>,
) -> Result<(), ErrorCode> {
    zamok_fs::attrs::set_times(node, meta.created, 0, meta.written)
        .map_err(|_| ErrorCode::CopyFailed)?;
    // 0x20 (архивный) ставит сама Windows, его не переносим отдельно
    if meta.attrs & 0x7 != 0 {
        attrs_later.push((node.to_path_buf(), meta.attrs));
    }
    Ok(())
}

/// Сбросить все файлы хранилища на диск (FlushFileBuffers).
pub(crate) fn flush_tree(root: &Path) -> std::io::Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            if e.file_type()?.is_dir() {
                stack.push(e.path());
            } else {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(e.path())?
                    .sync_all()?;
            }
        }
    }
    Ok(())
}

/// Откат до подмены папки: удалить временную копию и журнал. Оригинал не тронут.
fn rollback_copy(j: &Journal, data_dir: &Path) {
    // Если копия уже переименована в итоговое имя, вернуть ей имя .tmp перед удалением.
    // Только нашу: хранилище с тем же именем, но другим ключом не трогаем.
    if std::fs::symlink_metadata(&j.tmp).is_err() && is_ours(j, &j.vault) {
        if let Err(e) = rename_retry(&j.vault, &j.tmp, 20) {
            tracing::error!(job = %j.id, error = %e, "rollback: cannot rename vault back; journal kept");
            return;
        }
    }
    if let Err(e) = remove_tmp(j) {
        tracing::error!(job = %j.id, error = %e, "rollback: cannot delete the tmp copy; journal kept");
        return;
    }
    j.remove(data_dir);
    tracing::info!(job = %j.id, "rolled back, original untouched");
}

/// Откат после переименования оригинала: вернуть оригинал на место, затем как
/// [`rollback_copy`]. Если путь занят, ничего не трогаем и оставляем журнал. Если оригинала
/// нет ни рядом, ни на месте, хранилище осталось единственной копией: его не трогаем.
fn rollback_aside(j: &Journal, data_dir: &Path) {
    if std::fs::symlink_metadata(&j.aside).is_ok() {
        if crate::mountpoint::prepare(&j.path).is_err() {
            tracing::error!(job = %j.id, "rollback: folder path is occupied; journal kept");
            return;
        }
        if let Err(e) = rename_retry(&j.aside, &j.path, 20) {
            tracing::error!(job = %j.id, error = %e, "rollback: cannot move the original back; journal kept");
            return;
        }
    } else if matches_plan(j, &j.path) != Some(true) {
        tracing::error!(job = %j.id, "rollback: the original is gone; vault and journal kept");
        return;
    }
    rollback_copy(j, data_dir);
}

/// Удалить оригинал после того, как зашифрованная папка зарегистрирована и подключена.
fn delete_aside(j: &Journal) -> std::io::Result<()> {
    let name = j
        .aside
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    anyhow_guard(
        name.is_some_and(|n| n.starts_with('.') && n.ends_with(".zamok-original"))
            && is_ours(j, &j.vault),
    )?;
    match std::fs::remove_dir_all(&j.aside) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn anyhow_guard(ok: bool) -> std::io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(std::io::Error::other("refusing to delete the original"))
    }
}

/// Довести работу с шага `from` до конца: подмена, подключение, удаление оригинала.
/// Используется и живой работой, и восстановлением при старте.
fn finish_from(
    core: &Core,
    j: &mut Journal,
    data_dir: &Path,
    progress: Option<&Progress>,
) -> Result<(), ErrorCode> {
    let set_phase = |j: &mut Journal, p: Phase| -> Result<(), ErrorCode> {
        j.set(p, data_dir).map_err(|_| ErrorCode::Internal)?;
        if let Some(pr) = progress {
            *pr.phase.lock().unwrap() = p;
        }
        Ok(())
    };
    // Дальше двигается оригинал: только если копия на месте и она наша
    if !(is_ours(j, &j.tmp) || is_ours(j, &j.vault)) {
        tracing::error!(job = %j.id, "encrypted copy is missing or not from this job; journal kept");
        return Err(ErrorCode::Internal);
    }
    if j.phase == Phase::Verified {
        if std::fs::symlink_metadata(&j.tmp).is_ok() {
            if rename_retry(&j.tmp, &j.vault, 20).is_err() {
                rollback_copy(j, data_dir);
                return Err(ErrorCode::CopyFailed);
            }
        } else if std::fs::symlink_metadata(&j.vault).is_err() {
            // Ни копии, ни хранилища: копия потеряна, оригинал на месте
            j.remove(data_dir);
            return Err(ErrorCode::CopyFailed);
        }
        crash_point("vault_placed_before_journal");
        set_phase(j, Phase::VaultPlaced)?;
        crash_point("vault_placed");
    }
    if j.phase == Phase::VaultPlaced {
        let original_here = std::fs::symlink_metadata(&j.path).is_ok_and(|m| m.is_dir());
        if std::fs::symlink_metadata(&j.aside).is_err() {
            if !original_here {
                tracing::error!(job = %j.id, "original folder is missing; journal kept");
                return Err(ErrorCode::Internal);
            }
            // Папку с открытыми в программах файлами Windows переименовать не даст
            if rename_retry(&j.path, &j.aside, 8).is_err() {
                rollback_copy(j, data_dir);
                return Err(ErrorCode::FolderInUse);
            }
            set_hidden(&j.aside);
        }
        crash_point("original_aside_before_journal");
        set_phase(j, Phase::OriginalAside)?;
        crash_point("original_aside");
    }
    if j.phase == Phase::OriginalAside {
        check_aside(core, j, data_dir)?;
        crash_point("aside_checked");
        if let Err(code) = core.register_encrypted(&j.path, &j.vault, j.idle_lock_secs) {
            tracing::error!(job = %j.id, ?code, "cannot mount the encrypted folder, rolling back");
            rollback_aside(j, data_dir);
            return Err(code);
        }
        crash_point("registered_before_journal");
        set_phase(j, Phase::Registered)?;
        crash_point("registered");
    }
    Ok(())
}

/// Последний шаг: удалить оригинал и журнал.
fn cleanup(j: &Journal, data_dir: &Path) {
    match delete_aside(j) {
        Ok(()) => {
            crash_point("aside_deleted");
            j.remove(data_dir);
            tracing::info!(job = %j.id, "encryption finished");
        }
        Err(e) => {
            // Журнал остаётся: core попробует удалить ещё раз (retry_cleanup и при старте)
            tracing::error!(job = %j.id, error = %e, "cannot delete the original yet");
        }
    }
}

/// Ещё раз попробовать удалить оригиналы, которые не удалились сразу: антивирус или
/// индексатор могли держать файл. Открытый текст не должен лежать до перезапуска core.
pub fn retry_cleanup(core: &Core, data_dir: &Path) {
    if core.job_running() {
        return;
    }
    for j in Journal::load_all(data_dir) {
        if j.phase == Phase::Registered {
            cleanup(&j, data_dir);
        }
    }
}

/// Сколько зашифрованных папок ждут удаления своего открытого оригинала.
pub fn originals_left(data_dir: &Path) -> usize {
    Journal::load_all(data_dir)
        .iter()
        .filter(|j| j.phase == Phase::Registered)
        .count()
}

/// Начать шифрование: проверки, журнал, пустое хранилище. Возвращает ход работы и ключ
/// восстановления; копирование идёт в отдельном потоке.
pub fn start(
    core: Arc<Core>,
    data_dir: &Path,
    path: &Path,
    password: Zeroizing<String>,
    idle_lock_secs: u64,
) -> Result<(Arc<Progress>, Zeroizing<String>), preflight::Refusal> {
    let refuse = |code| preflight::Refusal {
        code,
        examples: Vec::new(),
    };
    if password.is_empty() {
        return Err(refuse(ErrorCode::BadRequest));
    }
    // До любой подготовки: если с этого момента экран заблокировали, в конце папка не откроется
    let locks_at_start = core.lock_all_count();
    // Пока не разобран журнал прошлой работы, новую не начинаем
    if Journal::any_left(data_dir) {
        return Err(refuse(ErrorCode::RecoveryPending));
    }
    let plan = preflight::check(path, &core.taken_paths(), data_dir)?;
    let sib = preflight::siblings(path).ok_or_else(|| refuse(ErrorCode::BadRequest))?;
    let id = crate::config::new_id().map_err(|_| refuse(ErrorCode::Internal))?;
    let mut journal = Journal {
        version: 1,
        id: id.clone(),
        phase: Phase::Copying,
        path: path.to_path_buf(),
        tmp: sib.tmp,
        vault: sib.vault,
        aside: sib.aside,
        idle_lock_secs,
        masterkey_sha256: None,
        vault_config_sha256: None,
        plan_sha256: Some(plan_digest(&plan)),
    };
    // Журнал раньше копии: сбой во время создания хранилища откатится при старте
    journal
        .save(data_dir)
        .map_err(|_| refuse(ErrorCode::Internal))?;
    // Папка создаётся только здесь и только новая: вторая работа не перепишет ключ первой.
    // Чужую папку на этом месте не трогаем, поэтому без отката, только журнал долой.
    if std::fs::create_dir(&journal.tmp).is_err() {
        journal.remove(data_dir);
        return Err(refuse(ErrorCode::VaultExists));
    }
    let vault = match Vault::create_empty(
        &journal.tmp,
        &password,
        zamok_crypto::masterkey::SCRYPT_COST,
    ) {
        Ok(v) => v,
        Err(_) => {
            rollback_copy(&journal, data_dir);
            return Err(refuse(ErrorCode::CopyFailed));
        }
    };
    set_hidden(&journal.tmp);
    journal.masterkey_sha256 = masterkey_digest(&journal.tmp);
    journal.vault_config_sha256 = vault_config_digest(&journal.tmp);
    if journal.masterkey_sha256.is_none()
        || journal.vault_config_sha256.is_none()
        || journal.save(data_dir).is_err()
    {
        rollback_copy(&journal, data_dir);
        return Err(refuse(ErrorCode::CopyFailed));
    }
    let words = zamok_crypto::recovery::encode(vault.key());
    let progress = Progress::new(&id, path, &plan);
    tracing::info!(job = %id, files = plan.files, bytes = plan.bytes, "encryption started");

    let pr = progress.clone();
    let dir = data_dir.to_path_buf();
    std::thread::spawn(move || {
        let result = run(
            &core,
            &mut journal,
            &dir,
            &vault,
            &plan,
            &pr,
            &password,
            locks_at_start,
        );
        if let Err(code) = result {
            tracing::warn!(job = %journal.id, ?code, "encryption stopped");
        }
        pr.finish(result);
    });
    Ok((progress, words))
}

#[allow(clippy::too_many_arguments)]
fn run(
    core: &Core,
    j: &mut Journal,
    data_dir: &Path,
    vault: &Vault,
    plan: &Plan,
    progress: &Progress,
    password: &str,
    locks_at_start: u64,
) -> Result<(), ErrorCode> {
    crash_point("copying");
    let attrs_later = match copy_and_verify(vault, &j.path, plan, progress) {
        Ok(a) => a,
        Err(code) => {
            rollback_copy(j, data_dir);
            return Err(code);
        }
    };
    if flush_tree(&j.tmp).is_err() {
        rollback_copy(j, data_dir);
        return Err(ErrorCode::CopyFailed);
    }
    for (node, attrs) in &attrs_later {
        if zamok_fs::attrs::set_attributes(node, *attrs).is_err() {
            rollback_copy(j, data_dir);
            return Err(ErrorCode::CopyFailed);
        }
    }
    pause_point("flush_window");
    crash_point("flush_done");
    if progress.cancel.load(Ordering::SeqCst) {
        rollback_copy(j, data_dir);
        return Err(ErrorCode::Cancelled);
    }
    // Файлы проверены ключом из памяти; ключ и настройки хранилища на диске проверяем здесь
    if !opens_from_disk(&j.tmp, password, vault.key()) {
        tracing::error!(job = %j.id, "the vault on disk does not open with the password");
        rollback_copy(j, data_dir);
        return Err(ErrorCode::VerifyFailed);
    }
    crash_point("verified_before_journal");
    // Дальше отмены нет: с этого шага работа только доводится до конца
    *progress.phase.lock().unwrap() = Phase::Verified;
    if j.set(Phase::Verified, data_dir).is_err() {
        rollback_copy(j, data_dir);
        return Err(ErrorCode::Internal);
    }
    crash_point("verified");
    finish_from(core, j, data_dir, Some(progress))?;
    // В конце папка открыта (DESIGN.md, раздел 2.1), если её за это время не запирали
    if let Err(code) =
        core.unlock_if_no_lock_since(&j.path.to_string_lossy(), password, locks_at_start)
    {
        tracing::warn!(job = %j.id, ?code, "folder was not opened after encryption");
    }
    cleanup(j, data_dir);
    Ok(())
}

/// Открывается ли копия с диска паролем и подходит ли ключ из файла к тому, которым
/// зашифрованы файлы.
fn opens_from_disk(tmp: &Path, password: &str, key: &zamok_crypto::MasterKey) -> bool {
    Vault::open_with_password(tmp, password).is_ok_and(|v| v.key().to_raw() == key.to_raw())
}

/// При старте core: довести до конца или откатить работы, прерванные сбоем.
pub fn recover(core: &Core, data_dir: &Path) {
    for mut j in Journal::load_all(data_dir) {
        tracing::info!(job = %j.id, phase = j.phase.name(), "recovering interrupted encryption");
        match j.phase {
            Phase::Copying => rollback_copy(&j, data_dir),
            _ => {
                if finish_from(core, &mut j, data_dir, None).is_ok() {
                    cleanup(&j, data_dir);
                }
            }
        }
    }
}

/// Прочитать файл целиком (для сравнения в тестах).
#[cfg(test)]
fn read_all(p: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut v = Vec::new();
    std::fs::File::open(p).unwrap().read_to_end(&mut v).unwrap();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preflight::tests::tempdir;

    fn sample(root: &Path) {
        std::fs::create_dir_all(root.join("Папка").join("вложенная")).unwrap();
        std::fs::create_dir_all(root.join("пустая")).unwrap();
        std::fs::write(root.join("а.txt"), "привет").unwrap();
        std::fs::write(root.join("пусто.bin"), b"").unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(root.join("Папка").join("big.bin"), &big).unwrap();
        let long = format!("{}.txt", "д".repeat(120));
        std::fs::write(root.join("Папка").join("вложенная").join(long), b"long").unwrap();
    }

    fn progress_for(plan: &Plan) -> Arc<Progress> {
        Progress::new("t", Path::new("x"), plan)
    }

    #[test]
    fn copy_and_verify_makes_an_exact_vault() {
        let d = tempdir();
        let src = d.path().join("src");
        sample(&src);
        let mut bad = Vec::new();
        let plan = preflight::walk(&src, &mut bad).unwrap();
        let v = Vault::create_empty(&d.path().join("v"), "pw", 1 << 10).unwrap();
        let pr = progress_for(&plan);
        copy_and_verify(&v, &src, &plan, &pr).unwrap();
        assert_eq!(pr.info().files_done, 4);
        // Время изменения оригинала перенесено на шифротекст
        for n in v.walk().unwrap() {
            if let zamok_crypto::tree::Node::File { path, ciphertext } = n {
                use std::os::windows::fs::MetadataExt;
                let orig = std::fs::metadata(src.join(path.replace('/', "\\"))).unwrap();
                let ct = std::fs::metadata(&ciphertext).unwrap();
                assert_eq!(ct.last_write_time(), orig.last_write_time(), "{path}");
                assert_eq!(ct.creation_time(), orig.creation_time(), "{path}");
            }
        }
        let out = d.path().join("out");
        assert_eq!(v.export(&out).unwrap(), 4);
        let mut bad2 = Vec::new();
        let back = preflight::walk(&out, &mut bad2).unwrap();
        let shape = |p: &Plan| -> Vec<String> { p.items.iter().map(|i| i.rel.clone()).collect() };
        assert_eq!(shape(&back), shape(&plan));
        for i in &plan.items {
            if let Kind::File { .. } = i.kind {
                assert_eq!(read_all(&src.join(&i.rel)), read_all(&out.join(&i.rel)));
            }
        }
    }

    #[test]
    fn changed_file_is_detected() {
        let d = tempdir();
        let src = d.path().join("src");
        sample(&src);
        let mut bad = Vec::new();
        let plan = preflight::walk(&src, &mut bad).unwrap();
        std::fs::write(src.join("а.txt"), "другое содержимое").unwrap();
        let v = Vault::create_empty(&d.path().join("v"), "pw", 1 << 10).unwrap();
        assert_eq!(
            copy_and_verify(&v, &src, &plan, &progress_for(&plan)),
            Err(ErrorCode::ChangedDuringCopy)
        );
    }

    #[test]
    fn new_file_during_copy_is_detected() {
        let d = tempdir();
        let src = d.path().join("src");
        sample(&src);
        let mut bad = Vec::new();
        let plan = preflight::walk(&src, &mut bad).unwrap();
        std::fs::write(src.join("я-новый.txt"), "x").unwrap();
        let v = Vault::create_empty(&d.path().join("v"), "pw", 1 << 10).unwrap();
        assert_eq!(
            copy_and_verify(&v, &src, &plan, &progress_for(&plan)),
            Err(ErrorCode::ChangedDuringCopy)
        );
    }

    #[test]
    fn rollback_deletes_only_the_tmp_copy() {
        let d = tempdir();
        let src = d.path().join("Клиенты");
        sample(&src);
        let sib = preflight::siblings(&src).unwrap();
        std::fs::create_dir(&sib.tmp).unwrap();
        std::fs::write(sib.tmp.join("x"), b"x").unwrap();
        let j = Journal {
            version: 1,
            id: "r1".into(),
            phase: Phase::Copying,
            path: src.clone(),
            tmp: sib.tmp.clone(),
            vault: sib.vault.clone(),
            aside: sib.aside.clone(),
            idle_lock_secs: 0,
            masterkey_sha256: None,
            vault_config_sha256: None,
            plan_sha256: None,
        };
        let data = d.path().join("data");
        j.save(&data).unwrap();
        rollback_copy(&j, &data);
        assert!(!sib.tmp.exists());
        assert!(src.join("а.txt").exists());
        assert!(Journal::load_all(&data).is_empty());
        // Журнал с чужим путём вместо .zamok.tmp: удаление отказывает
        let evil = Journal {
            tmp: src.clone(),
            ..j
        };
        assert!(remove_tmp(&evil).is_err());
        assert!(src.join("а.txt").exists());
    }

    #[test]
    fn delete_aside_refuses_without_vault_or_wrong_name() {
        let d = tempdir();
        let src = d.path().join("Клиенты");
        sample(&src);
        let sib = preflight::siblings(&src).unwrap();
        let j = Journal {
            version: 1,
            id: "a1".into(),
            phase: Phase::Registered,
            path: src.clone(),
            tmp: sib.tmp,
            vault: sib.vault,
            aside: src.clone(),
            idle_lock_secs: 0,
            masterkey_sha256: None,
            vault_config_sha256: None,
            plan_sha256: None,
        };
        assert!(delete_aside(&j).is_err());
        assert!(src.join("а.txt").exists());
    }

    /// Журнал настоящей работы над `src` с описью её содержимого.
    fn journal_for(src: &Path, plan: &Plan, phase: Phase) -> Journal {
        let sib = preflight::siblings(src).unwrap();
        Journal {
            version: 1,
            id: "j1".into(),
            phase,
            path: src.to_path_buf(),
            tmp: sib.tmp,
            vault: sib.vault,
            aside: sib.aside,
            idle_lock_secs: 0,
            masterkey_sha256: None,
            vault_config_sha256: None,
            plan_sha256: Some(plan_digest(plan)),
        }
    }

    #[test]
    fn plan_check_sees_new_and_changed_files() {
        let d = tempdir();
        let src = d.path().join("src");
        sample(&src);
        let mut bad = Vec::new();
        let plan = preflight::walk(&src, &mut bad).unwrap();
        let j = journal_for(&src, &plan, Phase::OriginalAside);
        assert_eq!(matches_plan(&j, &src), Some(true));
        // Новый файл: именно он пропал бы при удалении оригинала
        std::fs::write(src.join("я-новый.txt"), "x").unwrap();
        assert_eq!(matches_plan(&j, &src), Some(false));
        std::fs::remove_file(src.join("я-новый.txt")).unwrap();
        assert_eq!(matches_plan(&j, &src), Some(true));
        std::fs::write(src.join("а.txt"), "другое содержимое").unwrap();
        assert_eq!(matches_plan(&j, &src), Some(false));
        // Папки нет: не совпало
        assert_eq!(matches_plan(&j, &d.path().join("нет")), Some(false));
        // В журнале старой версии описи нет
        let old = Journal {
            plan_sha256: None,
            ..j
        };
        assert_eq!(matches_plan(&old, &src), None);
    }

    #[test]
    fn journal_of_an_older_version_still_loads() {
        let d = tempdir();
        let dir = jobs_dir(d.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("old.json"),
            br#"{"version":1,"id":"old","phase":"verified","path":"C:\\a",
                "tmp":"C:\\.a.zamok.tmp","vault":"C:\\.a.zamok",
                "aside":"C:\\.a.zamok-original","idle_lock_secs":0,"masterkey_sha256":"ab"}"#,
        )
        .unwrap();
        let all = Journal::load_all(d.path());
        assert_eq!(all.len(), 1);
        assert!(all[0].plan_sha256.is_none() && all[0].vault_config_sha256.is_none());
    }

    #[test]
    fn rollback_keeps_the_vault_when_the_original_is_gone() {
        let d = tempdir();
        let src = d.path().join("Клиенты");
        sample(&src);
        let mut bad = Vec::new();
        let plan = preflight::walk(&src, &mut bad).unwrap();
        let mut j = journal_for(&src, &plan, Phase::OriginalAside);
        let v = Vault::create_empty(&j.vault, "pw", 1 << 10).unwrap();
        j.masterkey_sha256 = masterkey_digest(v.root());
        let data = d.path().join("data");
        j.save(&data).unwrap();
        // Оригинала нет ни на месте, ни рядом: хранилище единственная копия
        std::fs::remove_dir_all(&src).unwrap();
        rollback_aside(&j, &data);
        assert!(j.vault.join("masterkey.cryptomator").exists());
        assert_eq!(Journal::load_all(&data).len(), 1);
    }

    #[test]
    fn vault_is_recognised_after_a_password_change() {
        let d = tempdir();
        let src = d.path().join("Клиенты");
        sample(&src);
        let sib = preflight::siblings(&src).unwrap();
        let v = Vault::create_empty(&sib.vault, "pw", 1 << 10).unwrap();
        std::fs::create_dir(&sib.aside).unwrap();
        let j = Journal {
            version: 1,
            id: "p1".into(),
            phase: Phase::Registered,
            path: src,
            tmp: sib.tmp,
            vault: sib.vault.clone(),
            aside: sib.aside.clone(),
            idle_lock_secs: 0,
            masterkey_sha256: masterkey_digest(v.root()),
            vault_config_sha256: vault_config_digest(v.root()),
            plan_sha256: None,
        };
        // Смена пароля меняет masterkey.cryptomator, настройки хранилища остаются
        let json = zamok_crypto::masterkey::MasterkeyFile::lock_with(v.key(), "new", 1 << 10)
            .unwrap()
            .to_json();
        std::fs::write(sib.vault.join("masterkey.cryptomator"), json).unwrap();
        assert!(is_ours(&j, &sib.vault));
        assert!(delete_aside(&j).is_ok());
        assert!(!sib.aside.exists());
    }

    #[test]
    fn damaged_key_file_is_caught_before_the_original_moves() {
        let d = tempdir();
        let v = Vault::create_empty(&d.path().join("v"), "pw", 1 << 10).unwrap();
        assert!(opens_from_disk(v.root(), "pw", v.key()));
        assert!(!opens_from_disk(v.root(), "другой", v.key()));
        std::fs::write(v.root().join("masterkey.cryptomator"), b"{}").unwrap();
        assert!(!opens_from_disk(v.root(), "pw", v.key()));
    }

    #[test]
    fn stale_journal_never_touches_a_foreign_vault() {
        let d = tempdir();
        let src = d.path().join("Клиенты");
        sample(&src);
        let sib = preflight::siblings(&src).unwrap();
        // Наша копия когда-то была, журнал помнит её отпечаток
        let ours = Vault::create_empty(&sib.tmp, "pw", 1 << 10).unwrap();
        let digest = masterkey_digest(ours.root());
        std::fs::remove_dir_all(&sib.tmp).unwrap();
        // Теперь рядом лежит другое, настоящее хранилище с тем же именем
        Vault::create_empty(&sib.vault, "pw2", 1 << 10).unwrap();
        let data = d.path().join("data");
        let j = Journal {
            version: 1,
            id: "s1".into(),
            phase: Phase::Copying,
            path: src.clone(),
            tmp: sib.tmp.clone(),
            vault: sib.vault.clone(),
            aside: sib.aside.clone(),
            idle_lock_secs: 0,
            masterkey_sha256: digest,
            vault_config_sha256: None,
            plan_sha256: None,
        };
        j.save(&data).unwrap();
        rollback_copy(&j, &data);
        assert!(sib.vault.join("masterkey.cryptomator").exists());
        // Чужая папка .tmp с тем же именем тоже не удаляется
        Vault::create_empty(&sib.tmp, "pw3", 1 << 10).unwrap();
        assert!(remove_tmp(&j).is_err());
        assert!(sib.tmp.exists());
        // Оригинал не удаляется ради чужого хранилища
        std::fs::create_dir(&sib.aside).unwrap();
        let j2 = Journal {
            phase: Phase::Registered,
            ..j
        };
        assert!(delete_aside(&j2).is_err());
        assert!(sib.aside.exists());
    }
}
