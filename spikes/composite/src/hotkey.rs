//! The global hotkey that summons the hatch: registered with the OS so it
//! works whether or not nus is in front. Windows: `RegisterHotKey` on its
//! own thread with a message loop, `WM_HOTKEY` → a `UserEvent::Hatch` on
//! the event loop. macOS uses a registered Carbon shortcut (no input
//! monitoring). Linux uses X11; Wayland exposes the CLI shortcut fallback.

use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Chord {
    /// Ctrl+` — the drop-down classic.
    #[default]
    CtrlGrave,
    /// Win+` (Cmd+` on macOS).
    SuperGrave,
    /// Ctrl+Shift+Space.
    CtrlShiftSpace,
}

impl Chord {
    pub fn matches(self, event: &winit::event::KeyEvent, mods: winit::keyboard::ModifiersState) -> bool {
        use winit::keyboard::{KeyCode, PhysicalKey};
        let key = match event.physical_key { PhysicalKey::Code(key) => key, _ => return false };
        let ctrl = mods.control_key(); let shift = mods.shift_key(); let sup = mods.super_key();
        !mods.alt_key() && match self {
            Self::CtrlGrave => key == KeyCode::Backquote && ctrl && !shift && !sup,
            Self::SuperGrave => key == KeyCode::Backquote && sup && !shift && !ctrl,
            Self::CtrlShiftSpace => key == KeyCode::Space && ctrl && shift && !sup,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Chord::CtrlGrave => "CTRL+`",
            Chord::SuperGrave => {
                if cfg!(target_os = "macos") {
                    "CMD+`"
                } else {
                    "WIN+`"
                }
            }
            Chord::CtrlShiftSpace => "CTRL+SHIFT+SPACE",
        }
    }
}

/// A registered hotkey; dropping it unregisters.
pub struct Hotkey {
    #[cfg(windows)]
    thread: Option<u32>,
    #[cfg(not(windows))]
    registration: Option<(global_hotkey::GlobalHotKeyManager, global_hotkey::hotkey::HotKey)>,
    pub chord: Chord,
    /// What the OS said: empty when it took, else why not.
    pub status: String,
}

impl Hotkey {
    pub fn register(chord: Chord, proxy: EventLoopProxy<UserEvent>) -> Hotkey {
        #[cfg(windows)]
        {
            use std::sync::mpsc::channel;
            let (tx, rx) = channel::<Result<u32, String>>();
            std::thread::Builder::new()
                .name("hotkey".into())
                .spawn(move || unsafe {
                    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
                    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN};
                    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY, WM_QUIT};
                    let (mods, vk) = match chord {
                        Chord::CtrlGrave => (MOD_CONTROL, 0xC0u32),      // VK_OEM_3
                        Chord::SuperGrave => (MOD_WIN, 0xC0u32),
                        Chord::CtrlShiftSpace => (MOD_CONTROL | MOD_SHIFT, 0x20u32),
                    };
                    if RegisterHotKey(std::ptr::null_mut(), 1, mods | MOD_NOREPEAT, vk) == 0 {
                        let _ = tx.send(Err("the OS refused the hotkey (another app has it?)".into()));
                        return;
                    }
                    let _ = tx.send(Ok(GetCurrentThreadId()));
                    let mut msg: MSG = std::mem::zeroed();
                    loop {
                        let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                        if r == 0 || r == -1 || msg.message == WM_QUIT {
                            break;
                        }
                        if msg.message == WM_HOTKEY {
                            let _ = proxy.send_event(UserEvent::Hatch);
                        }
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    UnregisterHotKey(std::ptr::null_mut(), 1);
                })
                .ok();
            match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                Ok(Ok(tid)) => Hotkey { thread: Some(tid), chord, status: String::new() },
                Ok(Err(e)) => Hotkey { thread: None, chord, status: e },
                Err(_) => Hotkey { thread: None, chord, status: "the hotkey thread did not answer".into() },
            }
        }
        #[cfg(not(windows))]
        {
            use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::{HotKey, Modifiers, Code}};
            if crate::hatch_native::wayland() {
                return Hotkey { registration: None, chord, status: "Set a desktop shortcut to: nus hatch toggle (Wayland manages global shortcuts)".into() };
            }
            let (mods, code) = match chord {
                Chord::CtrlGrave => (Modifiers::CONTROL, Code::Backquote),
                Chord::SuperGrave => (Modifiers::SUPER, Code::Backquote),
                Chord::CtrlShiftSpace => (Modifiers::CONTROL | Modifiers::SHIFT, Code::Space),
            };
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                if event.state == HotKeyState::Pressed { let _ = proxy.send_event(UserEvent::Hatch); }
            }));
            let key = HotKey::new(Some(mods), code);
            match GlobalHotKeyManager::new().and_then(|manager| { manager.register(key)?; Ok(manager) }) {
                Ok(manager) => Hotkey { registration: Some((manager, key)), chord, status: String::new() },
                Err(e) => Hotkey { registration: None, chord, status: format!("Shortcut unavailable: {e}. Choose another shortcut.") },
            }
        }
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        #[cfg(not(windows))]
        if let Some((manager,key)) = self.registration.take() { let _ = manager.unregister(key); }
        #[cfg(windows)]
        if let Some(tid) = self.thread.take() {
            unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
        }
    }
}

/// Where the pointer is, in physical screen pixels (Windows); None elsewhere.
pub fn pointer() -> Option<(i32, i32)> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut p) != 0 {
            return Some((p.x, p.y));
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::c_void;
        #[repr(C)]
        struct Point { x:f64, y:f64 }
        #[link(name="CoreGraphics",kind="framework")]
        unsafe extern "C" {
            fn CGEventCreate(source:*const c_void)->*const c_void;
            fn CGEventGetLocation(event:*const c_void)->Point;
        }
        #[link(name="CoreFoundation",kind="framework")]
        unsafe extern "C" { fn CFRelease(value:*const c_void); }
        // SAFETY: a newly created event samples the pointer. The event is
        // released after copying its CGPoint; no event is posted or retained.
        unsafe {
            let event=CGEventCreate(std::ptr::null());
            if event.is_null() {return None;}
            let point=CGEventGetLocation(event);
            CFRelease(event);
            Some((point.x as i32,point.y as i32))
        }
    }
    #[cfg(not(any(windows,target_os="macos")))]
    { None }
}
