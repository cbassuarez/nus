//! Keys to a page: what winit gives, as the events CEF's own client would
//! send on this platform. Chromium reads a key three ways, and each has
//! to be right or the page misses it:
//!
//!   · `windows_key_code` — the Windows VK, on every platform: it is what
//!     Blink's editing table looks up (VK_BACK → delete backward, VK_TAB →
//!     the next field) and what `keyCode` says to scripts;
//!   · `native_key_code` — the platform's own code for the physical key:
//!     the scan code on Windows, the X keycode on Linux, and on macOS the
//!     virtual keycode CEF builds its NSEvent from. Sent the VK there,
//!     Backspace (0x08) is the C key and Tab (0x09) the V key — which is
//!     why they did nothing in a field;
//!   · `character` — the key's text, on a CHAR event where it is typed,
//!     and on macOS on the down and up too: an event with no character
//!     there is taken for a modifier changing.
//!
//! A press is RAWKEYDOWN, then a CHAR per UTF-16 unit of its text (as
//! WM_CHAR would follow WM_KEYDOWN); a release is KEYUP.

use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::platform::scancode::PhysicalKeyExtScancode;

use crate::app::{cef_mods, KeyIn};

/// The events for one winit key, in the order to send them.
pub fn events(ev: &KeyIn, mods: ModifiersState) -> Vec<cef::KeyEvent> {
    let pressed = ev.state == ElementState::Pressed;
    let (ctrl, alt) = (mods.control_key(), mods.alt_key());
    let vk = vk_code(&ev.physical_key, &ev.logical_key);
    let native = native_code(&ev.physical_key);
    // The key's own character, for the platform that wants it on every event.
    let own = if cfg!(target_os = "macos") { key_character(&ev.logical_key, ev.text.as_deref()) } else { 0 };
    let base = cef::KeyEvent {
        windows_key_code: vk,
        native_key_code: native,
        modifiers: cef_mods(mods),
        // Alt chords are WM_SYSKEY* on Windows; Chromium wants to know.
        is_system_key: (cfg!(windows) && alt && !ctrl) as i32,
        character: own,
        unmodified_character: own,
        focus_on_editable_field: 0,
        ..Default::default()
    };
    let mut out = Vec::with_capacity(3);
    if pressed {
        out.push(cef::KeyEvent { type_: cef::KeyEventType::RAWKEYDOWN, ..base.clone() });
        // Typed text follows, unless Ctrl makes it a chord (AltGr is Ctrl+Alt
        // and does type).
        if let Some(text) = ev.text.as_deref() {
            if !ctrl || alt {
                for ch in text.encode_utf16() {
                    out.push(cef::KeyEvent {
                        type_: cef::KeyEventType::CHAR,
                        character: ch,
                        unmodified_character: ch,
                        windows_key_code: ch as i32,
                        ..base.clone()
                    });
                }
            }
        }
    } else {
        out.push(cef::KeyEvent { type_: cef::KeyEventType::KEYUP, ..base });
    }
    out
}

/// What the platform calls this physical key.
fn native_code(phys: &PhysicalKey) -> i32 {
    // winit maps a wholly unidentified key to KEY_UNKNOWN (240) on Linux.
    // That is not a real physical key and must not become X keycode 248.
    if matches!(phys, PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified)) { return 0; }
    let Some(code) = phys.to_scancode() else { return 0 };
    // X keycodes are the kernel's plus eight; winit hands back the kernel's.
    if cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd")) {
        code as i32 + 8
    } else {
        code as i32
    }
}

/// The key's character as macOS reports it: the typed text, or for a
/// named key the character its NSEvent carries (0x7F for Backspace, the
/// function-key range for arrows and the rest). Modifiers have none.
fn key_character(logical: &Key, text: Option<&str>) -> u16 {
    if let Some(t) = text.and_then(|t| t.encode_utf16().next()) {
        return t;
    }
    match logical {
        Key::Character(s) => s.encode_utf16().next().unwrap_or(0),
        Key::Named(n) => match n {
            NamedKey::Backspace => 0x7F,
            NamedKey::Tab => 0x09,
            NamedKey::Enter => 0x0D,
            NamedKey::Escape => 0x1B,
            NamedKey::Space => 0x20,
            NamedKey::ArrowUp => 0xF700,
            NamedKey::ArrowDown => 0xF701,
            NamedKey::ArrowLeft => 0xF702,
            NamedKey::ArrowRight => 0xF703,
            NamedKey::F1 => 0xF704,
            NamedKey::F2 => 0xF705,
            NamedKey::F3 => 0xF706,
            NamedKey::F4 => 0xF707,
            NamedKey::F5 => 0xF708,
            NamedKey::F6 => 0xF709,
            NamedKey::F7 => 0xF70A,
            NamedKey::F8 => 0xF70B,
            NamedKey::F9 => 0xF70C,
            NamedKey::F10 => 0xF70D,
            NamedKey::F11 => 0xF70E,
            NamedKey::F12 => 0xF70F,
            NamedKey::Insert => 0xF727,
            NamedKey::Delete => 0xF728,
            NamedKey::Home => 0xF729,
            NamedKey::End => 0xF72B,
            NamedKey::PageUp => 0xF72C,
            NamedKey::PageDown => 0xF72D,
            _ => 0,
        },
        _ => 0,
    }
}

/// Windows virtual-key code for a winit key: the physical key where it
/// names one, else the character, so a layout's own keys still say
/// something to `keyCode`.
pub fn vk_code(phys: &PhysicalKey, logical: &Key) -> i32 {
    if let PhysicalKey::Code(c) = phys {
        let v = match c {
            KeyCode::Enter | KeyCode::NumpadEnter => 0x0D,
            KeyCode::Tab => 0x09,
            KeyCode::Backspace => 0x08,
            KeyCode::Escape => 0x1B,
            KeyCode::Space => 0x20,
            KeyCode::ArrowLeft => 0x25,
            KeyCode::ArrowUp => 0x26,
            KeyCode::ArrowRight => 0x27,
            KeyCode::ArrowDown => 0x28,
            KeyCode::Home => 0x24,
            KeyCode::End => 0x23,
            KeyCode::PageUp => 0x21,
            KeyCode::PageDown => 0x22,
            KeyCode::Delete => 0x2E,
            KeyCode::Insert => 0x2D,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => 0x10,
            KeyCode::ControlLeft | KeyCode::ControlRight => 0x11,
            KeyCode::AltLeft | KeyCode::AltRight => 0x12,
            KeyCode::SuperLeft => 0x5B,
            KeyCode::SuperRight => 0x5C,
            KeyCode::ContextMenu => 0x5D,
            KeyCode::CapsLock => 0x14,
            KeyCode::NumLock => 0x90,
            KeyCode::ScrollLock => 0x91,
            KeyCode::Pause => 0x13,
            KeyCode::PrintScreen => 0x2C,
            KeyCode::F1 => 0x70,
            KeyCode::F2 => 0x71,
            KeyCode::F3 => 0x72,
            KeyCode::F4 => 0x73,
            KeyCode::F5 => 0x74,
            KeyCode::F6 => 0x75,
            KeyCode::F7 => 0x76,
            KeyCode::F8 => 0x77,
            KeyCode::F9 => 0x78,
            KeyCode::F10 => 0x79,
            KeyCode::F11 => 0x7A,
            KeyCode::F12 => 0x7B,
            KeyCode::Numpad0 => 0x60,
            KeyCode::Numpad1 => 0x61,
            KeyCode::Numpad2 => 0x62,
            KeyCode::Numpad3 => 0x63,
            KeyCode::Numpad4 => 0x64,
            KeyCode::Numpad5 => 0x65,
            KeyCode::Numpad6 => 0x66,
            KeyCode::Numpad7 => 0x67,
            KeyCode::Numpad8 => 0x68,
            KeyCode::Numpad9 => 0x69,
            KeyCode::NumpadMultiply => 0x6A,
            KeyCode::NumpadAdd => 0x6B,
            KeyCode::NumpadSubtract => 0x6D,
            KeyCode::NumpadDecimal => 0x6E,
            KeyCode::NumpadDivide => 0x6F,
            KeyCode::Semicolon => 0xBA,
            KeyCode::Equal => 0xBB,
            KeyCode::Comma => 0xBC,
            KeyCode::Minus => 0xBD,
            KeyCode::Period => 0xBE,
            KeyCode::Slash => 0xBF,
            KeyCode::Backquote => 0xC0,
            KeyCode::BracketLeft => 0xDB,
            KeyCode::Backslash => 0xDC,
            KeyCode::BracketRight => 0xDD,
            KeyCode::Quote => 0xDE,
            KeyCode::AudioVolumeMute => 0xAD,
            KeyCode::AudioVolumeDown => 0xAE,
            KeyCode::AudioVolumeUp => 0xAF,
            KeyCode::MediaTrackNext => 0xB0,
            KeyCode::MediaTrackPrevious => 0xB1,
            KeyCode::MediaStop => 0xB2,
            KeyCode::MediaPlayPause => 0xB3,
            KeyCode::BrowserBack => 0xA6,
            KeyCode::BrowserForward => 0xA7,
            _ => 0,
        };
        if v != 0 {
            return v;
        }
    }
    match logical {
        Key::Character(s) => {
            let c = s.chars().next().unwrap_or('\0').to_ascii_uppercase();
            if c.is_ascii_alphanumeric() {
                c as i32
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{NativeKeyCode, SmolStr};

    fn key(phys: PhysicalKey, logical: Key, text: Option<&str>, pressed: bool) -> KeyIn {
        KeyIn { physical_key: phys, logical_key: logical, text: text.map(SmolStr::new), state: if pressed { ElementState::Pressed } else { ElementState::Released }, repeat: false }
    }

    #[test]
    fn a_letter_is_a_down_a_char_and_an_up() {
        let down = key(PhysicalKey::Code(KeyCode::KeyA), Key::Character(SmolStr::new("a")), Some("a"), true);
        let evs = events(&down, ModifiersState::empty());
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].windows_key_code, 'A' as i32);
        assert_eq!(evs[1].character, 'a' as u16);
        assert!(matches!(evs[1].type_, cef::KeyEventType::CHAR));
        let up = key(PhysicalKey::Code(KeyCode::KeyA), Key::Character(SmolStr::new("a")), None, false);
        assert_eq!(events(&up, ModifiersState::empty()).len(), 1);
    }

    #[test]
    fn a_chord_types_nothing() {
        let down = key(PhysicalKey::Code(KeyCode::KeyV), Key::Character(SmolStr::new("v")), Some("v"), true);
        assert_eq!(events(&down, ModifiersState::CONTROL).len(), 1);
        // AltGr is Ctrl+Alt and does type.
        assert_eq!(events(&down, ModifiersState::CONTROL | ModifiersState::ALT).len(), 2);
    }

    #[test]
    fn named_keys_carry_their_vk_and_native_code() {
        let bs = key(PhysicalKey::Code(KeyCode::Backspace), Key::Named(NamedKey::Backspace), None, true);
        let evs = events(&bs, ModifiersState::empty());
        assert_eq!(evs[0].windows_key_code, 0x08);
        // The platform's own code for the key, never the VK.
        let native = evs[0].native_key_code;
        if cfg!(target_os = "macos") {
            assert_eq!(native, 0x33);
            assert_eq!(evs[0].character, 0x7F);
        } else if cfg!(windows) {
            assert_eq!(native, 0x0E);
            assert_eq!(evs[0].character, 0);
        }
        let tab = key(PhysicalKey::Code(KeyCode::Tab), Key::Named(NamedKey::Tab), None, true);
        assert_eq!(events(&tab, ModifiersState::empty())[0].windows_key_code, 0x09);
        let unknown = key(PhysicalKey::Unidentified(NativeKeyCode::Unidentified), Key::Character(SmolStr::new(".")), Some("."), true);
        assert_eq!(events(&unknown, ModifiersState::empty())[0].native_key_code, 0);
        assert_eq!(vk_code(&PhysicalKey::Code(KeyCode::Period), &Key::Character(SmolStr::new("."))), 0xBE);
    }

    #[test]
    fn a_modifier_alone_has_no_character() {
        let shift = key(PhysicalKey::Code(KeyCode::ShiftLeft), Key::Named(NamedKey::Shift), None, true);
        let evs = events(&shift, ModifiersState::SHIFT);
        assert_eq!(evs[0].windows_key_code, 0x10);
        assert_eq!(evs[0].character, 0);
    }
}
