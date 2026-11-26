//! Windows Explorer COM DropTarget local server (`--shell-drop-target`).
//!
//! Port of `ShellDropTargetServer.cs`. Windows Explorer passes multi-selected
//! files to a single `IDropTarget::Drop` call via `IDataObject` (`CF_HDROP`),
//! avoiding per-file `%1` process storms and single-file truncation.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::{
    E_FAIL, E_NOINTERFACE, E_POINTER, HGLOBAL, S_FALSE, S_OK,
};
use windows_sys::Win32::UI::Shell::DragQueryFileW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, SetTimer, TranslateMessage, MSG, WM_TIMER,
};

pub const PLAY_CLSID_STR: &str = "{A3B79A2E-C313-45F7-BB02-F9F4B0E4C101}";
pub const QUEUE_CLSID_STR: &str = "{A3B79A2E-C313-45F7-BB02-F9F4B0E4C102}";
pub const PLAYLIST_CLSID_STR: &str = "{A3B79A2E-C313-45F7-BB02-F9F4B0E4C103}";

const PLAY_CLSID: GUID = GUID::from_u128(0xA3B79A2E_C313_45F7_BB02_F9F4B0E4C101);
const QUEUE_CLSID: GUID = GUID::from_u128(0xA3B79A2E_C313_45F7_BB02_F9F4B0E4C102);
const PLAYLIST_CLSID: GUID = GUID::from_u128(0xA3B79A2E_C313_45F7_BB02_F9F4B0E4C103);

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_C000_000000000046);
const IID_ICLASS_FACTORY: GUID = GUID::from_u128(0x00000001_0000_0000_C000_000000000046);
const IID_IDROP_TARGET: GUID = GUID::from_u128(0x00000122_0000_0000_C000_000000000046);

const CLSCTX_LOCAL_SERVER: u32 = 4;
const REGCLS_MULTIPLEUSE: u32 = 1;
const CLASS_E_NOAGGREGATION: i32 = -2147221232; // 0x80040110
const DROPEFFECT_COPY: u32 = 1;
const DROPEFFECT_NONE: u32 = 0;

static LAST_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);
static SERVER_LOCKS: AtomicI32 = AtomicI32::new(0);
static TOKEN_SEQ: AtomicU64 = AtomicU64::new(0);

#[link(name = "ole32")]
extern "system" {
    fn OleInitialize(pvReserved: *const c_void) -> i32;
    fn OleUninitialize();
    fn CoRegisterClassObject(
        rclsid: *const GUID,
        pUnk: *mut c_void,
        dwClsContext: u32,
        flags: u32,
        lpdwRegister: *mut u32,
    ) -> i32;
    fn CoRevokeClassObject(dwRegister: u32) -> i32;
    fn ReleaseStgMedium(pmedium: *mut StgMedium);
}

#[repr(C)]
#[derive(Copy, Clone)]
struct PointL {
    x: i32,
    y: i32,
}

#[repr(C)]
struct FormatEtc {
    cf_format: u16,
    ptd: *mut c_void,
    dw_aspect: u32,
    lindex: i32,
    tymed: u32,
}

#[repr(C)]
struct StgMedium {
    tymed: u32,
    union_member: *mut c_void,
    p_unk_for_release: *mut c_void,
}

#[repr(C)]
struct IDataObjectVtbl {
    query_interface: usize,
    add_ref: usize,
    release: usize,
    get_data: unsafe extern "system" fn(
        this: *mut c_void,
        pformatetc_in: *const FormatEtc,
        pmedium: *mut StgMedium,
    ) -> i32,
}

#[repr(C)]
struct IDataObjectRaw {
    vtbl: *const IDataObjectVtbl,
}

#[repr(C)]
struct IDropTargetVtbl {
    query_interface:
        unsafe extern "system" fn(*mut DropTargetObj, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut DropTargetObj) -> u32,
    release: unsafe extern "system" fn(*mut DropTargetObj) -> u32,
    drag_enter: unsafe extern "system" fn(
        *mut DropTargetObj,
        *mut c_void,
        u32,
        PointL,
        *mut u32,
    ) -> i32,
    drag_over: unsafe extern "system" fn(*mut DropTargetObj, u32, PointL, *mut u32) -> i32,
    drag_leave: unsafe extern "system" fn(*mut DropTargetObj) -> i32,
    drop: unsafe extern "system" fn(
        *mut DropTargetObj,
        *mut c_void,
        u32,
        PointL,
        *mut u32,
    ) -> i32,
}

#[repr(C)]
struct DropTargetObj {
    vtbl: *const IDropTargetVtbl,
    refs: AtomicU32,
    verb: &'static str,
}

static DROP_TARGET_VTBL: IDropTargetVtbl = IDropTargetVtbl {
    query_interface: dt_query_interface,
    add_ref: dt_add_ref,
    release: dt_release,
    drag_enter: dt_drag_enter,
    drag_over: dt_drag_over,
    drag_leave: dt_drag_leave,
    drop: dt_drop,
};

#[repr(C)]
struct IClassFactoryVtbl {
    query_interface:
        unsafe extern "system" fn(*mut ClassFactoryObj, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut ClassFactoryObj) -> u32,
    release: unsafe extern "system" fn(*mut ClassFactoryObj) -> u32,
    create_instance: unsafe extern "system" fn(
        *mut ClassFactoryObj,
        *mut c_void,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    lock_server: unsafe extern "system" fn(*mut ClassFactoryObj, i32) -> i32,
}

#[repr(C)]
struct ClassFactoryObj {
    vtbl: *const IClassFactoryVtbl,
    refs: AtomicU32,
    verb: &'static str,
}

static CLASS_FACTORY_VTBL: IClassFactoryVtbl = IClassFactoryVtbl {
    query_interface: cf_query_interface,
    add_ref: cf_add_ref,
    release: cf_release,
    create_instance: cf_create_instance,
    lock_server: cf_lock_server,
};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn touch_activity() {
    LAST_ACTIVITY_MS.store(now_ms(), Ordering::Relaxed);
}

unsafe extern "system" fn dt_query_interface(
    this: *mut DropTargetObj,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let iid = *riid;
    if iid == IID_IUNKNOWN || iid == IID_IDROP_TARGET {
        dt_add_ref(this);
        *ppv = this.cast();
        return S_OK;
    }
    *ppv = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn dt_add_ref(this: *mut DropTargetObj) -> u32 {
    (*this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn dt_release(this: *mut DropTargetObj) -> u32 {
    let prev = (*this).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this));
    }
    prev.saturating_sub(1)
}

unsafe extern "system" fn dt_drag_enter(
    _this: *mut DropTargetObj,
    _data: *mut c_void,
    _keys: u32,
    _pt: PointL,
    effect: *mut u32,
) -> i32 {
    touch_activity();
    if !effect.is_null() {
        *effect = DROPEFFECT_COPY;
    }
    S_OK
}

unsafe extern "system" fn dt_drag_over(
    _this: *mut DropTargetObj,
    _keys: u32,
    _pt: PointL,
    effect: *mut u32,
) -> i32 {
    touch_activity();
    if !effect.is_null() {
        *effect = DROPEFFECT_COPY;
    }
    S_OK
}

unsafe extern "system" fn dt_drag_leave(_this: *mut DropTargetObj) -> i32 {
    touch_activity();
    S_OK
}

unsafe extern "system" fn dt_drop(
    this: *mut DropTargetObj,
    data: *mut c_void,
    _keys: u32,
    _pt: PointL,
    effect: *mut u32,
) -> i32 {
    touch_activity();
    if !effect.is_null() {
        *effect = DROPEFFECT_NONE;
    }
    if data.is_null() {
        return E_POINTER;
    }

    let format = FormatEtc {
        cf_format: 15, // CF_HDROP
        ptd: std::ptr::null_mut(),
        dw_aspect: 1, // DVASPECT_CONTENT
        lindex: -1,
        tymed: 1, // TYMED_HGLOBAL
    };
    let mut medium = StgMedium {
        tymed: 0,
        union_member: std::ptr::null_mut(),
        p_unk_for_release: std::ptr::null_mut(),
    };
    let raw_data = &*(data as *const IDataObjectRaw);
    let hr = ((*raw_data.vtbl).get_data)(data, &format, &mut medium);
    if hr < 0 {
        return hr;
    }

    let mut paths = Vec::new();
    let hdrop = medium.union_member as HGLOBAL;
    if !hdrop.is_null() {
        let count = DragQueryFileW(hdrop as _, u32::MAX, std::ptr::null_mut(), 0);
        for index in 0..count {
            let len = DragQueryFileW(hdrop as _, index, std::ptr::null_mut(), 0);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let copied = DragQueryFileW(hdrop as _, index, buf.as_mut_ptr(), buf.len() as u32);
            if copied > 0 {
                buf.truncate(copied as usize);
                paths.push(String::from_utf16_lossy(&buf));
            }
        }
    }
    ReleaseStgMedium(&mut medium);

    if paths.is_empty() {
        return S_OK;
    }

    let verb = (*this).verb;
    if dispatch_file_request(verb, &paths).is_err() {
        return E_FAIL;
    }
    if !effect.is_null() {
        *effect = DROPEFFECT_COPY;
    }
    S_OK
}

unsafe extern "system" fn cf_query_interface(
    this: *mut ClassFactoryObj,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let iid = *riid;
    if iid == IID_IUNKNOWN || iid == IID_ICLASS_FACTORY {
        cf_add_ref(this);
        *ppv = this.cast();
        return S_OK;
    }
    *ppv = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn cf_add_ref(this: *mut ClassFactoryObj) -> u32 {
    (*this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn cf_release(this: *mut ClassFactoryObj) -> u32 {
    let prev = (*this).refs.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this));
    }
    prev.saturating_sub(1)
}

unsafe extern "system" fn cf_create_instance(
    this: *mut ClassFactoryObj,
    outer: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> i32 {
    touch_activity();
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    let target = Box::into_raw(Box::new(DropTargetObj {
        vtbl: &DROP_TARGET_VTBL,
        refs: AtomicU32::new(1),
        verb: (*this).verb,
    }));
    let hr = dt_query_interface(target, riid, ppv);
    dt_release(target);
    hr
}

unsafe extern "system" fn cf_lock_server(_this: *mut ClassFactoryObj, lock: i32) -> i32 {
    touch_activity();
    if lock != 0 {
        SERVER_LOCKS.fetch_add(1, Ordering::Relaxed);
    } else {
        let _ = SERVER_LOCKS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            Some(v.saturating_sub(1))
        });
    }
    S_OK
}

pub fn shell_requests_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("MusicPlayer").join("ShellRequests"))
        .unwrap_or_else(|| PathBuf::from("shell-requests"))
}

fn dispatch_file_request(verb: &str, paths: &[String]) -> std::io::Result<()> {
    let dir = shell_requests_dir();
    std::fs::create_dir_all(&dir)?;
    let token = format!(
        "{}_{}_{}",
        now_ms(),
        std::process::id(),
        TOKEN_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let path = dir.join(format!("{token}.json"));
    let payload = serde_json::json!({
        "verb": verb,
        "paths": paths,
    });
    std::fs::write(&path, payload.to_string())?;
    let exe = std::env::current_exe()?;
    match std::process::Command::new(exe)
        .arg("--file-request")
        .arg(&token)
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(path);
            Err(err)
        }
    }
}

pub fn read_file_request(token: &str) -> Option<crate::os::ActivationPayload> {
    if token.trim().is_empty()
        || token.contains(['/', '\\', '.'])
        || token.len() > 128
    {
        return None;
    }
    let path = shell_requests_dir().join(format!("{token}.json"));
    let content = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    let verb = val
        .get("verb")
        .and_then(|v| v.as_str())
        .unwrap_or("play")
        .to_string();
    let paths: Vec<String> = val
        .get("paths")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return None;
    }
    Some(crate::os::ActivationPayload { verb, paths })
}

pub fn run_drop_target_server() -> Result<(), String> {
    touch_activity();
    unsafe {
        let hr = OleInitialize(std::ptr::null());
        if hr != S_OK && hr != S_FALSE {
            return Err(format!("OleInitialize failed: {hr:#x}"));
        }
        let specs = [
            (PLAY_CLSID, "play"),
            (QUEUE_CLSID, "addtoqueue"),
            (PLAYLIST_CLSID, "createplaylist"),
        ];
        let mut cookies = Vec::new();
        let mut factories = Vec::new();
        for (clsid, verb) in specs {
            let factory = Box::into_raw(Box::new(ClassFactoryObj {
                vtbl: &CLASS_FACTORY_VTBL,
                refs: AtomicU32::new(1),
                verb,
            }));
            factories.push(factory);
            let mut cookie = 0u32;
            let reg_hr = CoRegisterClassObject(
                &clsid,
                factory.cast(),
                CLSCTX_LOCAL_SERVER,
                REGCLS_MULTIPLEUSE,
                &mut cookie,
            );
            if HRESULT(reg_hr).is_err() {
                for c in cookies {
                    let _ = CoRevokeClassObject(c);
                }
                for f in factories {
                    cf_release(f);
                }
                OleUninitialize();
                return Err(format!("CoRegisterClassObject failed: {reg_hr:#x}"));
            }
            cookies.push(cookie);
        }

        let timer_id = SetTimer(std::ptr::null_mut(), 0, 1000, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_TIMER
                && SERVER_LOCKS.load(Ordering::Relaxed) == 0
                && now_ms().saturating_sub(LAST_ACTIVITY_MS.load(Ordering::Relaxed)) > 5 * 60 * 1000
            {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if timer_id != 0 {
            KillTimer(std::ptr::null_mut(), timer_id);
        }
        for c in cookies {
            let _ = CoRevokeClassObject(c);
        }
        for f in factories {
            cf_release(f);
        }
        OleUninitialize();
    }
    Ok(())
}
