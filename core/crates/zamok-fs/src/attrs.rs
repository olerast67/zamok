//! Атрибуты и время файлов на диске, свободное место.
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, SetFileAttributesW, SetFileTime, FILE_FLAGS_AND_ATTRIBUTES,
};

pub fn set_attributes(path: &Path, attrs: u32) -> io::Result<()> {
    let attrs = if attrs == 0 { 0x80 } else { attrs }; // FILE_ATTRIBUTE_NORMAL
    unsafe {
        SetFileAttributesW(
            &HSTRING::from(path.as_os_str()),
            FILE_FLAGS_AND_ATTRIBUTES(attrs),
        )
    }
    .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
}

fn ft(v: u64) -> FILETIME {
    FILETIME {
        dwLowDateTime: v as u32,
        dwHighDateTime: (v >> 32) as u32,
    }
}

/// Время в формате FILETIME (100 нс с 1601 года); 0 значит «не менять».
pub fn set_times(path: &Path, creation: u64, access: u64, write: u64) -> io::Result<()> {
    if creation == 0 && access == 0 && write == 0 {
        return Ok(());
    }
    let f = std::fs::OpenOptions::new()
        .access_mode(0x100) // FILE_WRITE_ATTRIBUTES
        .share_mode(0x7)
        .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS: открывает и каталоги
        .open(path)?;
    let (c, a, w) = (ft(creation), ft(access), ft(write));
    unsafe {
        SetFileTime(
            HANDLE(f.as_raw_handle()),
            if creation != 0 { Some(&c) } else { None },
            if access != 0 { Some(&a) } else { None },
            if write != 0 { Some(&w) } else { None },
        )
    }
    .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
}

/// (всего байт, свободно байт) на диске, где лежит `path`.
pub fn disk_space(path: &Path) -> io::Result<(u64, u64)> {
    let mut free = 0u64;
    let mut total = 0u64;
    unsafe {
        GetDiskFreeSpaceExW(
            &HSTRING::from(path.as_os_str()),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
    Ok((total, free))
}
