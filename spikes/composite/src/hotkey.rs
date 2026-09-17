//! The global hotkey that summons the hatch: registered with the OS so it
//! works whether or not nus is in front. Windows: `RegisterHotKey` on its
//! own thread with a message loop, `WM_HOTKEY` → a `UserEvent::Hatch` on
//! the event loop. macOS needs the Accessibility permission and a Carbon
//! hot key; Linux the portal's GlobalShortcuts — both are stubs that say
//! so for now, so the chord inside nus still works everywhere.

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
            let _ = proxy;
            let why = if cfg!(target_os = "macos") {
                "global hotkeys on macOS wait on the Accessibility permission (coming)"
            } else {
                "global hotkeys on Linux wait on the portal (coming)"
            };
            Hotkey { chord, status: why.into() }
        }
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
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
    #[cfg(not(windows))]
    {
        None
    }
}
