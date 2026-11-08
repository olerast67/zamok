//! Подключение зашифрованной папки на путь и запирание с ожиданием сброса кэша ядра.
//!
//! Общий код для `zamok-mount` (разработка и тесты) и фонового процесса `zamok-core`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winfsp::host::{FileSystemHost, FileSystemParams, VolumeParams};

use crate::fs::{Control, UnlockError, ZamokFs};

/// Подключённая папка. Пока значение живо, том висит на пути; при удалении он отключается,
/// а ключи стираются.
pub struct Mounted {
    host: FileSystemHost<ZamokFs>,
    control: Arc<Control>,
    path: PathBuf,
}

/// Проверить, что WinFsp установлен (загружается его DLL). Вызывать один раз до подключений.
pub fn init_winfsp() -> anyhow::Result<()> {
    winfsp::winfsp_init().map_err(|e| anyhow::anyhow!("WinFsp is not installed: {e:?}"))?;
    Ok(())
}

impl Mounted {
    /// Подключить хранилище `vault` на путь `path` (пути не должно существовать) в запертом
    /// состоянии. `name` показывается в запертой папке: `Открыть <name>.zamok`.
    pub fn mount(vault: &Path, path: &Path, name: &str) -> anyhow::Result<Self> {
        let fs = ZamokFs::new(vault, name)?;
        let control = fs.control();
        let mut vp = VolumeParams::new();
        vp.filesystem_name("Zamok")
            .sector_size(4096)
            .sectors_per_allocation_unit(1)
            .max_component_length(255)
            .case_sensitive_search(false)
            .case_preserved_names(true)
            .unicode_on_disk(true)
            .persistent_acls(true)
            .post_cleanup_when_modified_only(true)
            .flush_and_purge_on_cleanup(true)
            // Кэш сведений о файлах на 1 секунду. При запирании он сбрасывается уведомлениями
            // по всем путям, которые ядро могло закэшировать (находка прототипа P3).
            .file_info_timeout(1000);
        let params = FileSystemParams {
            use_dir_info_by_name: false,
            volume_params: vp,
            debug_mode: Default::default(),
        };
        let mut host: FileSystemHost<ZamokFs> =
            FileSystemHost::new_with_timer::<Vec<String>, 50>(params, fs)
                .map_err(|e| anyhow::anyhow!("host: {e:?}"))?;
        host.mount(path.as_os_str())
            .map_err(|e| anyhow::anyhow!("mount: {e:?}"))?;
        if let Err(e) = host.start() {
            host.unmount();
            anyhow::bail!("start: {e:?}");
        }
        // Атрибут «системный» на точке подключения: Проводник читает desktop.ini (иконка замка).
        // WinFsp создаёт точку заново при каждом подключении, поэтому ставим каждый раз.
        if let Ok(m) = std::fs::metadata(path) {
            use std::os::windows::fs::MetadataExt;
            let _ = crate::attrs::set_attributes(path, (m.file_attributes() | 0x4) & 0x7);
        }
        Ok(Mounted {
            host,
            control,
            path: path.to_path_buf(),
        })
    }

    pub fn control(&self) -> &Arc<Control> {
        &self.control
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Открыть паролем и попросить Проводник обновить окно.
    pub fn unlock_with_password(&self, password: &str) -> Result<(), UnlockError> {
        self.control.unlock_with_password(password)?;
        notify_shell(&self.path);
        Ok(())
    }

    /// Открыть ключом (ключ восстановления) и попросить Проводник обновить окно.
    pub fn unlock_with_key(&self, key: zamok_crypto::MasterKey) -> Result<(), UnlockError> {
        self.control.unlock_with_key(key)?;
        notify_shell(&self.path);
        Ok(())
    }

    /// Запереть и дождаться, пока ядро WinFsp забудет сведения о файлах.
    pub fn lock(&self) {
        self.control.lock();
        self.finish_lock();
    }

    /// Вторая половина запирания: см. [`settle_lock`].
    pub fn finish_lock(&self) {
        settle_lock(&self.control, &self.path);
    }
}

/// Вторая половина запирания: ключи уже стёрты [`Control::lock`], ждём, пока ядро WinFsp
/// забудет сведения о файлах, и просим Проводник обновить окно. Отдельно от `lock`, чтобы
/// при «запереть всё» сначала стереть ключи всех папок, а потом уже ждать.
pub fn settle_lock(control: &Control, path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while control.invalidation_pending() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    // Не всё удалось сбросить уведомлениями (или не успели): ждём, пока кэш ядра
    // (FileInfoTimeout = 1 с) истечёт сам.
    if control.take_fallback_wait() || control.invalidation_pending() {
        std::thread::sleep(Duration::from_millis(1200));
    }
    notify_shell(path);
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.control.lock();
        self.host.stop();
        self.host.unmount();
    }
}

/// Попросить Проводник перечитать папку (после открытия или запирания).
pub fn notify_shell(path: &Path) {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_UPDATEDIR, SHCNF_PATHW};
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe { SHChangeNotify(SHCNE_UPDATEDIR, SHCNF_PATHW, Some(wide.as_ptr() as _), None) };
}
