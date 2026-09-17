//! The taskbar button follows the shell's OSC 9;4 progress, the way
//! Windows Terminal's does: a green fill, red for an error, yellow for a
//! warning, a marquee for indeterminate. Windows only; the other desks
//! have no equivalent worth faking.

use std::sync::Arc;

use winit::window::Window;

/// Show `progress` — `(state, percent)` as OSC 9;4 says it — on the
/// window's taskbar button; `None` clears it.
pub fn set_progress(window: &Arc<Window>, progress: Option<(u8, u8)>) {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let hwnd = h.hwnd.get() as isize;
        win::set(hwnd, progress);
    }
    #[cfg(not(windows))]
    {
        let _ = (window, progress);
    }
}

#[cfg(windows)]
mod win {
    //! A hand-rolled ITaskbarList3 call: CoCreateInstance, then the
    //! vtable slots for SetProgressState (10) and SetProgressValue (9).
    //! Small enough not to want the `windows` crate's COM machinery.

    use std::ffi::c_void;
    use std::sync::Mutex;

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);

    // CLSID_TaskbarList {56FDF344-FD6D-11d0-958A-006097C9A090}
    const CLSID_TASKBAR_LIST: Guid = Guid(0x56FDF344, 0xFD6D, 0x11d0, [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90]);
    // IID_ITaskbarList3 {EA1AFB91-9E28-4B86-90E9-9E9F8A5EEFAF}
    const IID_TASKBAR_LIST3: Guid = Guid(0xEA1AFB91, 0x9E28, 0x4B86, [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF]);

    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const TBPF_NOPROGRESS: u32 = 0;
    const TBPF_INDETERMINATE: u32 = 0x1;
    const TBPF_NORMAL: u32 = 0x2;
    const TBPF_ERROR: u32 = 0x4;
    const TBPF_PAUSED: u32 = 0x8;

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoCreateInstance(clsid: *const Guid, outer: *mut c_void, ctx: u32, iid: *const Guid, out: *mut *mut c_void) -> i32;
    }

    type Slot = unsafe extern "system" fn(*mut c_void) -> i32;
    type SetState = unsafe extern "system" fn(*mut c_void, isize, u32) -> i32;
    type SetValue = unsafe extern "system" fn(*mut c_void, isize, u64, u64) -> i32;

    struct Taskbar(*mut c_void);
    // The pointer is only ever used from the app's thread; the mutex is
    // for the lazy init.
    unsafe impl Send for Taskbar {}

    static TASKBAR: Mutex<Option<Taskbar>> = Mutex::new(None);

    fn get() -> Option<*mut c_void> {
        let mut g = TASKBAR.lock().ok()?;
        if let Some(t) = g.as_ref() {
            return Some(t.0);
        }
        unsafe {
            // COINIT_APARTMENTTHREADED; already-initialised is fine.
            let _ = CoInitializeEx(std::ptr::null_mut(), 0x2);
            let mut p: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(&CLSID_TASKBAR_LIST, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_TASKBAR_LIST3, &mut p);
            if hr < 0 || p.is_null() {
                return None;
            }
            // HrInit is slot 3.
            let vt = *(p as *const *const *const c_void);
            let hr_init: Slot = std::mem::transmute::<*const c_void, Slot>(*vt.add(3));
            if hr_init(p) < 0 {
                return None;
            }
            *g = Some(Taskbar(p));
            Some(p)
        }
    }

    pub fn set(hwnd: isize, progress: Option<(u8, u8)>) {
        let Some(p) = get() else { return };
        unsafe {
            let vt = *(p as *const *const *const c_void);
            let set_value: SetValue = std::mem::transmute::<*const c_void, SetValue>(*vt.add(9));
            let set_state: SetState = std::mem::transmute::<*const c_void, SetState>(*vt.add(10));
            match progress {
                None | Some((0, _)) => {
                    set_state(p, hwnd, TBPF_NOPROGRESS);
                }
                Some((3, _)) => {
                    set_state(p, hwnd, TBPF_INDETERMINATE);
                }
                Some((state, pct)) => {
                    let flag = match state {
                        2 => TBPF_ERROR,
                        4 => TBPF_PAUSED,
                        _ => TBPF_NORMAL,
                    };
                    set_state(p, hwnd, flag);
                    set_value(p, hwnd, pct.min(100) as u64, 100);
                }
            }
        }
    }
}
