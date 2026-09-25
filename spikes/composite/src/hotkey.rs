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
    /// Option+Space (Alt+Space): one reach, the launcher convention on macOS.
    AltSpace,
    /// Option+` (Alt+`).
    AltGrave,
    /// Recorded in settings: modifier bits (`M_*`) and a row of `KEYS`.
    Custom { mods: u8, key: u8 },
}

pub const M_CTRL: u8 = 1;
pub const M_SHIFT: u8 = 2;
pub const M_ALT: u8 = 4;
pub const M_SUPER: u8 = 8;

/// The keys a recorded hotkey may end on: winit's code, the Windows
/// virtual key, and how it reads. `OS_CODES` is the same rows for the
/// global-hotkey crate (not built on Windows).
type KeyRow = (winit::keyboard::KeyCode, u32, &'static str);
macro_rules! keys {
    ($(($k:ident, $vk:expr, $label:expr)),* $(,)?) => {
        pub(crate) const KEYS: &[KeyRow] = &[$((winit::keyboard::KeyCode::$k, $vk, $label)),*];
        #[cfg(not(windows))]
        const OS_CODES: &[global_hotkey::hotkey::Code] = &[$(global_hotkey::hotkey::Code::$k),*];
    };
}
keys![
    (KeyA, 0x41, "A"), (KeyB, 0x42, "B"), (KeyC, 0x43, "C"), (KeyD, 0x44, "D"), (KeyE, 0x45, "E"),
    (KeyF, 0x46, "F"), (KeyG, 0x47, "G"), (KeyH, 0x48, "H"), (KeyI, 0x49, "I"), (KeyJ, 0x4A, "J"),
    (KeyK, 0x4B, "K"), (KeyL, 0x4C, "L"), (KeyM, 0x4D, "M"), (KeyN, 0x4E, "N"), (KeyO, 0x4F, "O"),
    (KeyP, 0x50, "P"), (KeyQ, 0x51, "Q"), (KeyR, 0x52, "R"), (KeyS, 0x53, "S"), (KeyT, 0x54, "T"),
    (KeyU, 0x55, "U"), (KeyV, 0x56, "V"), (KeyW, 0x57, "W"), (KeyX, 0x58, "X"), (KeyY, 0x59, "Y"),
    (KeyZ, 0x5A, "Z"),
    (Digit0, 0x30, "0"), (Digit1, 0x31, "1"), (Digit2, 0x32, "2"), (Digit3, 0x33, "3"), (Digit4, 0x34, "4"),
    (Digit5, 0x35, "5"), (Digit6, 0x36, "6"), (Digit7, 0x37, "7"), (Digit8, 0x38, "8"), (Digit9, 0x39, "9"),
    (Space, 0x20, "SPACE"), (Backquote, 0xC0, "`"), (Minus, 0xBD, "-"), (Equal, 0xBB, "="),
    (BracketLeft, 0xDB, "["), (BracketRight, 0xDD, "]"), (Backslash, 0xDC, "\\"), (Semicolon, 0xBA, ";"),
    (Quote, 0xDE, "'"), (Comma, 0xBC, ","), (Period, 0xBE, "."), (Slash, 0xBF, "/"),
    (Enter, 0x0D, "RETURN"), (Tab, 0x09, "TAB"),
    (F1, 0x70, "F1"), (F2, 0x71, "F2"), (F3, 0x72, "F3"), (F4, 0x73, "F4"), (F5, 0x74, "F5"), (F6, 0x75, "F6"),
    (F7, 0x76, "F7"), (F8, 0x77, "F8"), (F9, 0x78, "F9"), (F10, 0x79, "F10"), (F11, 0x7A, "F11"), (F12, 0x7B, "F12"),
];

impl Chord {
    /// A chord from a key press in the recorder. A function key may stand
    /// alone; anything else needs Ctrl, Option/Alt or Cmd/Win, so typing is
    /// never taken.
    pub fn record(code: winit::keyboard::KeyCode, mods: winit::keyboard::ModifiersState) -> Result<Chord, &'static str> {
        let key = KEYS.iter().position(|k| k.0 == code).ok_or("That key can't be a hotkey. Try a letter, number, Space, ` or F1–F12.")?;
        let m = (if mods.control_key() { M_CTRL } else { 0 }) | (if mods.shift_key() { M_SHIFT } else { 0 }) | (if mods.alt_key() { M_ALT } else { 0 }) | (if mods.super_key() { M_SUPER } else { 0 });
        let function = KEYS[key].2.starts_with('F') && KEYS[key].2.len() > 1;
        if m & (M_CTRL | M_ALT | M_SUPER) == 0 && !function {
            return Err("Add Ctrl, Option or Cmd so the hotkey doesn't take ordinary typing.");
        }
        Ok(Chord::Custom { mods: m, key: key as u8 })
    }

    /// (modifier bits, row of KEYS) for any chord.
    fn parts(self) -> (u8, usize) {
        let row = |c: winit::keyboard::KeyCode| KEYS.iter().position(|k| k.0 == c).unwrap_or(0);
        use winit::keyboard::KeyCode::{Backquote, Space};
        match self {
            Chord::CtrlGrave => (M_CTRL, row(Backquote)),
            Chord::SuperGrave => (M_SUPER, row(Backquote)),
            Chord::CtrlShiftSpace => (M_CTRL | M_SHIFT, row(Space)),
            Chord::AltSpace => (M_ALT, row(Space)),
            Chord::AltGrave => (M_ALT, row(Backquote)),
            Chord::Custom { mods, key } => (mods, (key as usize).min(KEYS.len() - 1)),
        }
    }

    /// A recorded chord's name, built once and kept for the process.
    fn custom_label(mods: u8, key: usize) -> &'static str {
        use std::sync::{Mutex, OnceLock};
        static NAMES: OnceLock<Mutex<std::collections::HashMap<(u8, usize), &'static str>>> = OnceLock::new();
        let mut names = NAMES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        names.entry((mods, key)).or_insert_with(|| {
            let mac = cfg!(target_os = "macos");
            let mut s = String::new();
            for (bit, glyph, word) in [(M_CTRL, "⌃", "CTRL+"), (M_ALT, "⌥", "ALT+"), (M_SHIFT, "⇧", "SHIFT+"), (M_SUPER, "⌘", "WIN+")] {
                if mods & bit != 0 { s.push_str(if mac { glyph } else { word }); }
            }
            s.push_str(KEYS[key].2);
            Box::leak(s.into_boxed_str())
        })
    }
}

impl Chord {
    pub fn matches(self, event: &crate::app::KeyIn, mods: winit::keyboard::ModifiersState) -> bool {
        use winit::keyboard::{KeyCode, PhysicalKey};
        let key: KeyCode = match event.physical_key { PhysicalKey::Code(key) => key, _ => return false };
        let (m, row) = self.parts();
        let held = (if mods.control_key() { M_CTRL } else { 0 }) | (if mods.shift_key() { M_SHIFT } else { 0 }) | (if mods.alt_key() { M_ALT } else { 0 }) | (if mods.super_key() { M_SUPER } else { 0 });
        KEYS[row].0 == key && held == m
    }

    pub fn label(self) -> &'static str {
        let mac = cfg!(target_os = "macos");
        match self {
            Chord::CtrlGrave => if mac { "⌃`" } else { "CTRL+`" },
            Chord::SuperGrave => if mac { "⌘`" } else { "WIN+`" },
            Chord::CtrlShiftSpace => if mac { "⌃⇧SPACE" } else { "CTRL+SHIFT+SPACE" },
            Chord::AltSpace => if mac { "⌥SPACE" } else { "ALT+SPACE" },
            Chord::AltGrave => if mac { "⌥`" } else { "ALT+`" },
            Chord::Custom { mods, key } => Chord::custom_label(mods, (key as usize).min(KEYS.len() - 1)),
        }
    }

    /// What this platform should offer. ⌘` is macOS's own window cycling and
    /// Alt+Space is the Windows window menu, so each is left out where it
    /// would fight the system (a saved choice still works).
    pub fn offered() -> &'static [Chord] {
        if cfg!(target_os = "macos") {
            &[Chord::AltSpace, Chord::AltGrave, Chord::CtrlGrave, Chord::CtrlShiftSpace]
        } else if cfg!(windows) {
            &[Chord::CtrlGrave, Chord::AltGrave, Chord::SuperGrave, Chord::CtrlShiftSpace]
        } else {
            &[Chord::CtrlGrave, Chord::AltSpace, Chord::AltGrave, Chord::CtrlShiftSpace]
        }
    }

    /// The first choice: ⌥Space on macOS, Ctrl+` elsewhere.
    pub fn platform_default() -> Chord {
        if cfg!(target_os = "macos") { Chord::AltSpace } else { Chord::CtrlGrave }
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
                    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN};
                    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY, WM_QUIT};
                    let (m, row) = chord.parts();
                    let mods = (if m & M_CTRL != 0 { MOD_CONTROL } else { 0 }) | (if m & M_SHIFT != 0 { MOD_SHIFT } else { 0 }) | (if m & M_ALT != 0 { MOD_ALT } else { 0 }) | (if m & M_SUPER != 0 { MOD_WIN } else { 0 });
                    let vk = KEYS[row].1;
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
            let (m, row) = chord.parts();
            let mut mods = Modifiers::empty();
            for (bit, flag) in [(M_CTRL, Modifiers::CONTROL), (M_SHIFT, Modifiers::SHIFT), (M_ALT, Modifiers::ALT), (M_SUPER, Modifiers::SUPER)] {
                if m & bit != 0 { mods |= flag; }
            }
            let code: Code = OS_CODES[row];
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
