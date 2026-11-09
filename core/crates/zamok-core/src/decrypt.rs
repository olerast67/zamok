//! «Расшифровать навсегда» (DESIGN.md, раздел 5): шифрование на месте в обратную сторону.
//!
//! | Шаг | Действие | Хранилище | Расшифрованная копия |
//! |---|---|---|---|
//! | `copying` | каждый файл расшифровывается в `.Имя.zamok-decrypted.tmp` и сверяется повторным чтением | подключено | неполная |
//! | `copied` | копия сверена и сброшена на диск | подключено | полная |
//! | `unmounted` | папка заперта, отключена и убрана из списка | на месте | полная |
//! | `placed` | копия стоит на месте папки | на месте | папка |
//! | (готово) | хранилище удалено, журнал удалён | нет | папка |
//!
//! Сбой до `unmounted` откатывается: копия удаляется, папка остаётся зашифрованной (если её
//! успели убрать из списка, она возвращается). После `unmounted` работа доводится до конца.
//! Хранилище удаляется только после того, как копия стоит на месте, и только если отпечаток
//! его `masterkey.cryptomator` совпадает с записанным в журнале.

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zamok_crypto::tree::{Node, Vault};

use crate::core::Core;
use crate::encrypt::{crash_point, flush_tree, masterkey_digest, rename_retry, Phase, Progress};
use crate::protocol::ErrorCode;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Copying,
    Copied,
    Unmounted,
    Placed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Journal {
    pub version: u32,
    pub id: String,
    pub step: Step,
    pub path: PathBuf,
    pub vault: PathBuf,
    pub tmp: PathBuf,
    pub idle_lock_secs: u64,
    pub masterkey_sha256: String,
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("decrypt-jobs")
}

impl Journal {
    fn file(&self, data_dir: &Path) -> PathBuf {
        dir(data_dir).join(format!("{}.json", self.id))
    }

    fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir(data_dir))?;
        let tmp = dir(data_dir).join(format!("{}.json.tmp", self.id));
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(self)?)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, self.file(data_dir))
    }

    fn set(&mut self, step: Step, data_dir: &Path) -> Result<(), ErrorCode> {
        self.step = step;
        self.save(data_dir).map_err(|_| ErrorCode::Internal)
    }

    fn remove(&self, data_dir: &Path) {
        let _ = std::fs::remove_file(self.file(data_dir));
    }

    pub fn load_all(data_dir: &Path) -> Vec<Journal> {
        let Ok(rd) = std::fs::read_dir(dir(data_dir)) else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
            .collect()
    }

    fn vault_is_ours(&self) -> bool {
        masterkey_digest(&self.vault).as_deref() == Some(self.masterkey_sha256.as_str())
    }
}

/// Удалить нашу расшифрованную копию: только папку `.….zamok-decrypted.tmp` из журнала.
fn remove_tmp(j: &Journal) -> std::io::Result<()> {
    let ok = j
        .tmp
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .is_some_and(|n| n.starts_with('.') && n.ends_with(".zamok-decrypted.tmp"));
    if !ok {
        return Err(std::io::Error::other(
            "refusing to delete: not a decrypted copy",
        ));
    }
    match std::fs::remove_dir_all(&j.tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Откат до подмены: копия удаляется, папка остаётся (или снова становится) зашифрованной.
/// Копию не удалось удалить: журнал остаётся, core попробует ещё раз. Шаг сначала
/// возвращается на `copying`: наполовину удалённая копия не должна встать на место папки.
fn rollback(core: &Core, j: &Journal, data_dir: &Path) {
    let mut j = j.clone();
    if j.step != Step::Copying && j.set(Step::Copying, data_dir).is_err() {
        tracing::error!(job = %j.id, "rollback: cannot update the journal; copy kept");
        return;
    }
    let tmp_removed = remove_tmp(&j);
    if core
        .register_encrypted(&j.path, &j.vault, j.idle_lock_secs)
        .is_err()
    {
        tracing::error!(job = %j.id, "rollback: cannot mount the folder again; journal kept");
        return;
    }
    if let Err(e) = tmp_removed {
        tracing::error!(job = %j.id, error = %e, "rollback: cannot delete the decrypted copy; journal kept");
        return;
    }
    j.remove(data_dir);
    tracing::info!(job = %j.id, "decryption rolled back, folder stays encrypted");
}

/// Расшифровать всё дерево в `tmp`, сверяя каждый файл повторным чтением. Время изменения
/// и создания, атрибуты берутся с файлов шифротекста (так их показывала папка Zamok).
fn copy_out(vault: &Vault, tmp: &Path, progress: &Progress) -> Result<(), ErrorCode> {
    std::fs::create_dir(tmp).map_err(|_| ErrorCode::CopyFailed)?;
    let nodes = vault.walk().map_err(|_| ErrorCode::VaultDamaged)?;
    let mut attrs_later = Vec::new();
    for node in nodes {
        if progress.cancel.load(Ordering::SeqCst) {
            return Err(ErrorCode::Cancelled);
        }
        match node {
            Node::Dir { path } => {
                std::fs::create_dir_all(tmp.join(path.replace('/', "\\")))
                    .map_err(|_| ErrorCode::CopyFailed)?;
            }
            Node::File { path, ciphertext } => {
                let target = tmp.join(path.replace('/', "\\"));
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| ErrorCode::CopyFailed)?;
                }
                let digest = {
                    let mut input = std::io::BufReader::with_capacity(
                        256 * 1024,
                        std::fs::File::open(&ciphertext).map_err(|_| ErrorCode::BadVault)?,
                    );
                    let out = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .map_err(|_| ErrorCode::CopyFailed)?;
                    let mut out = std::io::BufWriter::with_capacity(256 * 1024, out);
                    let d =
                        zamok_crypto::content::decrypt_stream(vault.key(), &mut input, &mut out)
                            .map_err(|_| ErrorCode::VerifyFailed)?;
                    use std::io::Write;
                    out.flush().map_err(|_| ErrorCode::CopyFailed)?;
                    d
                };
                // Сверка: прочитать записанный файл и сравнить отпечаток
                let back = {
                    use sha2::{Digest, Sha256};
                    use std::io::Read;
                    let mut f =
                        std::fs::File::open(&target).map_err(|_| ErrorCode::VerifyFailed)?;
                    let mut h = Sha256::new();
                    let mut buf = vec![0u8; 256 * 1024];
                    let mut len = 0u64;
                    loop {
                        let n = f.read(&mut buf).map_err(|_| ErrorCode::VerifyFailed)?;
                        if n == 0 {
                            break;
                        }
                        h.update(&buf[..n]);
                        len += n as u64;
                    }
                    (len, <[u8; 32]>::from(h.finalize()))
                };
                if back != digest {
                    return Err(ErrorCode::VerifyFailed);
                }
                if let Ok(m) = std::fs::metadata(&ciphertext) {
                    let _ = zamok_fs::attrs::set_times(
                        &target,
                        m.creation_time(),
                        0,
                        m.last_write_time(),
                    );
                    if m.file_attributes() & 0x7 != 0 {
                        attrs_later.push((target.clone(), m.file_attributes() & 0x27));
                    }
                }
                progress.bytes_done.fetch_add(digest.0, Ordering::Relaxed);
                progress.files_done.fetch_add(1, Ordering::Relaxed);
            }
            Node::Symlink { .. } => {}
        }
    }
    flush_tree(tmp).map_err(|_| ErrorCode::CopyFailed)?;
    // «Только чтение» после сброса: иначе сброс не открыл бы файл на запись
    for (p, a) in attrs_later {
        let _ = zamok_fs::attrs::set_attributes(&p, a);
    }
    Ok(())
}

/// Довести работу с текущего шага до конца (живая работа и восстановление при старте).
fn finish(core: &Core, j: &mut Journal, data_dir: &Path) -> Result<(), ErrorCode> {
    if j.step == Step::Copied {
        // Отключить и убрать из списка: на месте папки становится пусто
        let _ = core.lock(&j.path.to_string_lossy());
        if core.remove(&j.path.to_string_lossy()).is_err()
            && core.taken_paths().iter().any(|p| p == &j.path)
        {
            rollback(core, j, data_dir);
            return Err(ErrorCode::Internal);
        }
        crash_point("decrypt_unmounted_before_journal");
        j.set(Step::Unmounted, data_dir)?;
        crash_point("decrypt_unmounted");
    }
    if j.step == Step::Unmounted {
        if std::fs::symlink_metadata(&j.tmp).is_ok() {
            if crate::mountpoint::prepare(&j.path).is_err()
                || rename_retry(&j.tmp, &j.path, 20).is_err()
            {
                tracing::error!(job = %j.id, "cannot put the decrypted copy in place, rolling back");
                rollback(core, j, data_dir);
                return Err(ErrorCode::FolderInUse);
            }
        } else if std::fs::symlink_metadata(&j.path).is_err() {
            // Ни копии, ни папки: копия потеряна, хранилище цело; вернуть папку
            rollback(core, j, data_dir);
            return Err(ErrorCode::CopyFailed);
        }
        crash_point("decrypt_placed_before_journal");
        j.set(Step::Placed, data_dir)?;
        crash_point("decrypt_placed");
    }
    if j.step == Step::Placed {
        if j.vault_is_ours() {
            if let Err(e) = std::fs::remove_dir_all(&j.vault) {
                tracing::error!(job = %j.id, error = %e, "cannot delete the vault yet; journal kept");
                return Ok(());
            }
        }
        j.remove(data_dir);
        tracing::info!(job = %j.id, "decryption finished");
    }
    Ok(())
}

/// Начать расшифровку: проверить пароль, место и открытые файлы, записать журнал.
pub fn start(
    core: Arc<Core>,
    data_dir: &Path,
    path: &str,
    password: &str,
) -> Result<Arc<Progress>, ErrorCode> {
    let (path, vault, idle) = core.folder_paths(path)?;
    let p = path.to_string_lossy().into_owned();
    let key = core.verify_password(&p, password)?;
    if core.open_files(&p) > 0 {
        return Err(ErrorCode::FilesInUse);
    }
    // Запереть и не давать открыть, пока идёт копия: изменения в папке потерялись бы
    let busy = core.busy_guard(&path);
    let _ = core.lock(&p);
    let v = Vault::open_with_key(&vault, key).map_err(|_| ErrorCode::BadVault)?;
    let nodes = v.walk().map_err(|_| ErrorCode::VaultDamaged)?;
    let (mut files, mut bytes) = (0u64, 0u64);
    for n in &nodes {
        if let Node::File { ciphertext, .. } = n {
            files += 1;
            let len = std::fs::metadata(ciphertext).map(|m| m.len()).unwrap_or(0);
            bytes += zamok_crypto::content::cleartext_size(len).unwrap_or(0);
        }
    }
    let parent = path.parent().ok_or(ErrorCode::BadRequest)?;
    let need = bytes + bytes / 10 + files * 4096 + 64 * 1024 * 1024;
    if crate::preflight::free_space(parent).is_none_or(|f| f < need) {
        return Err(ErrorCode::NotEnoughSpace);
    }
    let name = path
        .file_name()
        .ok_or(ErrorCode::BadRequest)?
        .to_string_lossy()
        .into_owned();
    let tmp = parent.join(format!(".{name}.zamok-decrypted.tmp"));
    if std::fs::symlink_metadata(&tmp).is_ok() {
        return Err(ErrorCode::VaultExists);
    }
    let id = crate::config::new_id().map_err(|_| ErrorCode::Internal)?;
    let mut j = Journal {
        version: 1,
        id: id.clone(),
        step: Step::Copying,
        path: path.clone(),
        vault: vault.clone(),
        tmp,
        idle_lock_secs: idle,
        masterkey_sha256: masterkey_digest(&vault).ok_or(ErrorCode::BadVault)?,
    };
    j.save(data_dir).map_err(|_| ErrorCode::Internal)?;
    let progress = Progress::with_totals(&id, &path, files, bytes);
    tracing::info!(job = %id, files, bytes, "decryption started");

    let pr = progress.clone();
    let dir = data_dir.to_path_buf();
    std::thread::spawn(move || {
        crash_point("decrypt_copying");
        let result = match copy_out(&v, &j.tmp, &pr) {
            Err(code) => {
                rollback(&core, &j, &dir);
                Err(code)
            }
            Ok(()) => {
                crash_point("decrypt_copied_before_journal");
                *pr.phase.lock().unwrap() = Phase::Verified;
                match j.set(Step::Copied, &dir) {
                    Err(code) => {
                        rollback(&core, &j, &dir);
                        Err(code)
                    }
                    Ok(()) => {
                        crash_point("decrypt_copied");
                        finish(&core, &mut j, &dir)
                    }
                }
            }
        };
        if let Err(code) = result {
            tracing::warn!(job = %j.id, ?code, "decryption stopped");
        }
        drop(busy);
        pr.finish(result);
    });
    Ok(progress)
}

/// Ещё раз попробовать убрать расшифрованную копию, которая не удалилась при откате.
pub fn retry_rollback(core: &Core, data_dir: &Path) {
    // Занято на всю уборку: журнал новой работы не примется за брошенный
    let Some(_starting) = core.begin_job() else {
        return;
    };
    for j in Journal::load_all(data_dir) {
        if j.step == Step::Copying {
            rollback(core, &j, data_dir);
        }
    }
}

/// При старте core: доделать или откатить расшифровки, прерванные сбоем.
pub fn recover(core: &Core, data_dir: &Path) {
    for mut j in Journal::load_all(data_dir) {
        tracing::info!(job = %j.id, "recovering interrupted decryption");
        match j.step {
            Step::Copying => rollback(core, &j, data_dir),
            _ => {
                let _ = finish(core, &mut j, data_dir);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_tmp_only_touches_our_copy() {
        let d = tempfile::tempdir().unwrap();
        let other = d.path().join("Клиенты");
        std::fs::create_dir(&other).unwrap();
        let mut j = Journal {
            version: 1,
            id: "d1".into(),
            step: Step::Copying,
            path: other.clone(),
            vault: d.path().join(".Клиенты.zamok"),
            tmp: other.clone(),
            idle_lock_secs: 0,
            masterkey_sha256: "ab".into(),
        };
        // Чужое имя: отказ, папка цела
        assert!(remove_tmp(&j).is_err());
        assert!(other.exists());
        // Своя копия удаляется, отсутствие копии не ошибка
        j.tmp = d.path().join(".Клиенты.zamok-decrypted.tmp");
        std::fs::create_dir(&j.tmp).unwrap();
        std::fs::write(j.tmp.join("a.txt"), b"x").unwrap();
        assert!(remove_tmp(&j).is_ok());
        assert!(!j.tmp.exists());
        assert!(remove_tmp(&j).is_ok());
    }
}
