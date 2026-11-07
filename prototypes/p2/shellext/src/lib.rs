//! P2: обработчик меню правой кнопки, который делает «Открыть с паролем»
//! действием по умолчанию (двойной клик, Enter) только у запертых папок.
//!
//! Прототип: список запертых папок читается из HKCU\Software\ZamokP2, значение
//! `Locked` (пути через `|`), события пишутся в журнал (путь в значении `Log`).
//! Пароль не спрашивается: после «ввода» окно Проводника переходит внутрь папки.
//! В настоящем zamok-shell.dll не будет ни реестра, ни журнала на диске: список
//! придёт из общей памяти core (CLAUDE.md, правило про DLL в Проводнике).
#![allow(non_snake_case)]

use std::ffi::c_void;
use std::io::Write;
use std::sync::Mutex;

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;
use windows_core::implement;

pub const CLSID_P2: GUID = GUID::from_u128(0x5a3d6c1e_7b2f_4e8a_9c41_2f6b8d0e7a13);

fn reg_string(name: PCWSTR) -> Option<String> {
    let mut buf = vec![0u16; 8192];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\ZamokP2"),
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    if r.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn log(msg: &str) {
    if let Some(path) = reg_string(w!("Log")) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let pid = std::process::id();
            let _ = writeln!(f, "{pid}\t{msg}");
        }
    }
}

fn is_locked(path: &str) -> bool {
    let p = path.trim_end_matches('\\').to_lowercase();
    reg_string(w!("Locked"))
        .map(|l| l.split('|').any(|x| x.trim_end_matches('\\').to_lowercase() == p && !p.is_empty()))
        .unwrap_or(false)
}

#[implement(IShellExtInit, IContextMenu, IObjectWithSite)]
struct Menu {
    path: Mutex<Option<String>>,
    site: Mutex<Option<IUnknown>>,
}

impl IShellExtInit_Impl for Menu_Impl {
    fn Initialize(&self, _pidl: *const ITEMIDLIST, pdtobj: Ref<IDataObject>, _hkey: HKEY) -> Result<()> {
        let Some(obj) = pdtobj.as_ref() else { return Ok(()) };
        let fmt = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe {
            let mut medium = obj.GetData(&fmt)?;
            let hdrop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(hdrop, u32::MAX, None);
            if count == 1 {
                let mut buf = [0u16; 1024];
                let n = DragQueryFileW(hdrop, 0, Some(&mut buf)) as usize;
                *self.path.lock().unwrap() = Some(String::from_utf16_lossy(&buf[..n]));
            }
            ReleaseStgMedium(&mut medium);
        }
        Ok(())
    }
}

impl IContextMenu_Impl for Menu_Impl {
    fn QueryContextMenu(&self, hmenu: HMENU, indexmenu: u32, idcmdfirst: u32, _idcmdlast: u32, uflags: u32) -> HRESULT {
        let path = self.path.lock().unwrap().clone().unwrap_or_default();
        let locked = is_locked(&path);
        log(&format!("query flags=0x{uflags:x} locked={locked} path={path}"));
        if !locked {
            return HRESULT(0);
        }
        unsafe {
            if InsertMenuW(hmenu, indexmenu, MF_BYPOSITION | MF_STRING, idcmdfirst as usize, w!("Открыть с паролем (Zamok P2)")).is_err() {
                return HRESULT(0);
            }
            let _ = SetMenuDefaultItem(hmenu, idcmdfirst, 0);
        }
        HRESULT(1)
    }

    fn InvokeCommand(&self, pici: *const CMINVOKECOMMANDINFO) -> Result<()> {
        let verb = unsafe { (*pici).lpVerb.0 as usize };
        if verb >> 16 != 0 || verb != 0 {
            return Err(E_FAIL.into());
        }
        let path = self.path.lock().unwrap().clone().unwrap_or_default();
        let site = self.site.lock().unwrap().clone();
        log(&format!("invoke site={} path={path}", site.is_some()));
        // Здесь будет окно пароля. В прототипе считаем пароль верным и открываем папку.
        let wide = HSTRING::from(path.as_str());
        if let Some(site) = site {
            let browsed = (|| -> Result<()> {
                let sp: IServiceProvider = site.cast()?;
                let browser: IShellBrowser = unsafe { sp.QueryService(&SID_STopLevelBrowser)? };
                let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
                unsafe {
                    SHParseDisplayName(&wide, None, &mut pidl, 0, None)?;
                    let r = browser.BrowseObject(pidl, SBSP_SAMEBROWSER | SBSP_ABSOLUTE);
                    CoTaskMemFree(Some(pidl as *const c_void));
                    r
                }
            })();
            log(&format!("browse same window: {:?}", browsed.as_ref().map(|_| "ok")));
            if browsed.is_ok() {
                return Ok(());
            }
        }
        // Нет окна Проводника (рабочий стол, ShellExecute): новое окно без повторного разбора действий
        unsafe {
            ShellExecuteW(None, w!("open"), w!("explorer.exe"), &wide, None, SW_SHOWNORMAL);
        }
        log("opened new explorer window");
        Ok(())
    }

    fn GetCommandString(&self, _idcmd: usize, _utype: u32, _res: *const u32, _name: PSTR, _cch: u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}

impl IObjectWithSite_Impl for Menu_Impl {
    fn SetSite(&self, punksite: Ref<IUnknown>) -> Result<()> {
        *self.site.lock().unwrap() = punksite.cloned();
        Ok(())
    }
    fn GetSite(&self, _riid: *const GUID, ppvsite: *mut *mut c_void) -> Result<()> {
        unsafe { *ppvsite = std::ptr::null_mut() };
        Err(E_FAIL.into())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let menu: IUnknown = Menu { path: Mutex::new(None), site: Mutex::new(None) }.into();
        unsafe { menu.query(iid, object).ok() }
    }
    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

#[no_mangle]
extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    unsafe {
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID_P2 {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
    }
    let f: IClassFactory = Factory.into();
    unsafe { f.query(riid, ppv) }
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
