use windows::Win32::Foundation::HANDLE;
use winfsp::filesystem::DirBuffer;

use winfsp::util::{AtomicHandle, NtHandleDrop, NtSafeHandle};

/// A file context in the passthrough file system.
#[derive(Debug)]
pub struct NtPassthroughFile {
    handle: AtomicHandle<NtHandleDrop>,
    is_directory: bool,
    dir_buffer: DirBuffer,
    file_size_hint: u64,
    gate_id: u64,
    generation: u64,
    is_root: bool,
}

impl NtPassthroughFile {
    /// Create a new entry from an NT handle.
    pub fn new(handle: NtSafeHandle, file_size_hint: u64, is_directory: bool) -> Self {
        Self {
            handle: handle.into(),
            file_size_hint,
            is_directory,
            dir_buffer: DirBuffer::new(),
            gate_id: 0,
            generation: crate::gate::GENERATION.load(std::sync::atomic::Ordering::SeqCst),
            is_root: false,
        }
    }

    /// P3: файл с учётом состояния «заперто» и списка открытых файлов.
    pub fn new_gated(handle: NtSafeHandle, file_size_hint: u64, is_directory: bool, name: &winfsp::U16CStr) -> Self {
        let mut f = Self::new(handle, file_size_hint, is_directory);
        f.is_root = crate::gate::is_root(name);
        f.gate_id = crate::gate::register(name);
        f
    }

    /// P3: отказ в доступе, если папка заперта или файл открыт до последнего запирания.
    pub fn check(&self) -> winfsp::Result<()> {
        use std::sync::atomic::Ordering;
        if self.is_root {
            return Ok(());
        }
        if crate::gate::locked() || self.generation != crate::gate::GENERATION.load(Ordering::SeqCst) {
            return Err(windows::Win32::Foundation::STATUS_ACCESS_DENIED.into());
        }
        Ok(())
    }

    /// Get a HANDLE to this file entry.
    pub fn handle(&self) -> HANDLE {
        HANDLE(self.handle.handle())
    }

    pub fn handle_ref(&self) -> &AtomicHandle<NtHandleDrop> {
        &self.handle
    }

    /// Invalidate the underlying handle for this file entry.
    pub fn invalidate(&self) {
        self.handle.invalidate()
    }

    pub fn is_root(&self) -> bool {
        self.is_root
    }

    /// Whether or not this entry is a directory.
    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    /// The size of the file in bytes.
    pub fn size(&self) -> u32 {
        self.file_size_hint as u32
    }

    /// Get a reference to the directory buffer for this entry.
    pub fn dir_buffer(&self) -> &DirBuffer {
        &self.dir_buffer
    }

    /// Explicitly invalidate the handle before drop.
    pub fn close(self) {
        crate::gate::unregister(self.gate_id);
        self.invalidate();
        drop(self)
    }
}
