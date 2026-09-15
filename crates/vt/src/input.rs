//! Key events → bytes for the PTY. Legacy xterm encoding, upgraded per the
//! Kitty keyboard protocol flags the application has requested.

use bitflags::bitflags;
use vte::ansi::KeyboardModes;

use crate::term::Modes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A key that produces text. Carries the text as typed (shifted).
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1–F12.
    F(u8),
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Mods: u8 {
        const SHIFT = 1;
        const ALT   = 2;
        const CTRL  = 4;
        const SUPER = 8;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Repeat,
    Release,
}

/// Encode a key event. Returns an empty vec when nothing should be sent
/// (e.g. a release without `REPORT_EVENT_TYPES`).
pub fn encode(
    key: Key,
    mods: Mods,
    action: KeyAction,
    modes: Modes,
    kitty: KeyboardModes,
) -> Vec<u8> {
    let report_events = kitty.contains(KeyboardModes::REPORT_EVENT_TYPES);
    if action == KeyAction::Release && !report_events {
        return Vec::new();
    }
    let disambiguate = kitty.contains(KeyboardModes::DISAMBIGUATE_ESC_CODES);
    let all_as_esc = kitty.contains(KeyboardModes::REPORT_ALL_KEYS_AS_ESC);

    // Modifier param: 1 + bits, omitted when 1 unless an event type follows.
    let mod_param = 1 + mods.bits() as u32;
    let event_suffix = match (report_events, action) {
        (true, KeyAction::Repeat) => ":2",
        (true, KeyAction::Release) => ":3",
        _ => "",
    };
    let mods_field = |always: bool| -> String {
        if mod_param == 1 && event_suffix.is_empty() && !always {
            String::new()
        } else {
            format!(";{}{}", mod_param, event_suffix)
        }
    };
    let csi_u =
        |code: u32| -> Vec<u8> { format!("\x1b[{}{}u", code, mods_field(false)).into_bytes() };

    let ctrl_or_alt_or_super = mods.intersects(Mods::CTRL | Mods::ALT | Mods::SUPER);

    match key {
        Key::Char(c) => {
            if all_as_esc || (disambiguate && (ctrl_or_alt_or_super || !event_suffix.is_empty())) {
                // Key code is the unshifted key: best effort from the text.
                let code = c.to_lowercase().next().unwrap_or(c) as u32;
                return csi_u(code);
            }
            legacy_char(c, mods)
        }
        Key::Enter | Key::Tab | Key::Backspace | Key::Escape => {
            let (code, legacy): (u32, &[u8]) = match key {
                Key::Enter => (13, b"\r"),
                Key::Tab => (9, b"\t"),
                Key::Backspace => (127, b"\x7f"),
                _ => (27, b"\x1b"),
            };
            let needs_csi = all_as_esc
                || (disambiguate
                    && (key == Key::Escape || mods != Mods::empty() || !event_suffix.is_empty()));
            if needs_csi {
                return csi_u(code);
            }
            if key == Key::Tab && mods == Mods::SHIFT {
                return b"\x1b[Z".to_vec();
            }
            if key == Key::Backspace && mods.contains(Mods::CTRL) {
                return prefix_alt(b"\x08", mods);
            }
            prefix_alt(legacy, mods)
        }
        Key::Up | Key::Down | Key::Left | Key::Right | Key::Home | Key::End | Key::F(1..=4) => {
            let final_byte = match key {
                Key::Up => 'A',
                Key::Down => 'B',
                Key::Right => 'C',
                Key::Left => 'D',
                Key::Home => 'H',
                Key::End => 'F',
                Key::F(1) => 'P',
                Key::F(2) => 'Q',
                Key::F(3) => 'R',
                _ => 'S',
            };
            let field = mods_field(false);
            if field.is_empty() {
                let app = modes.contains(Modes::APP_CURSOR) && !matches!(key, Key::F(_));
                let ss3 = matches!(key, Key::F(_)) || app;
                if ss3 && !all_as_esc {
                    return format!("\x1bO{}", final_byte).into_bytes();
                }
                return format!("\x1b[{}", final_byte).into_bytes();
            }
            format!("\x1b[1{}{}", field, final_byte).into_bytes()
        }
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown | Key::F(_) => {
            let num = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                Key::PageDown => 6,
                Key::F(5) => 15,
                Key::F(6) => 17,
                Key::F(7) => 18,
                Key::F(8) => 19,
                Key::F(9) => 20,
                Key::F(10) => 21,
                Key::F(11) => 23,
                Key::F(12) => 24,
                Key::F(_) => return Vec::new(),
                _ => unreachable!(),
            };
            format!("\x1b[{}{}~", num, mods_field(false)).into_bytes()
        }
    }
}

fn prefix_alt(bytes: &[u8], mods: Mods) -> Vec<u8> {
    let mut v = Vec::with_capacity(bytes.len() + 1);
    if mods.contains(Mods::ALT) {
        v.push(0x1b);
    }
    v.extend_from_slice(bytes);
    v
}

fn legacy_char(c: char, mods: Mods) -> Vec<u8> {
    let mut out = Vec::new();
    if mods.contains(Mods::ALT) {
        out.push(0x1b);
    }
    if mods.contains(Mods::CTRL) {
        let byte = match c.to_ascii_lowercase() {
            l @ 'a'..='z' => Some(l as u8 & 0x1f),
            ' ' | '2' | '@' => Some(0x00),
            '[' | '3' => Some(0x1b),
            '\\' | '4' => Some(0x1c),
            ']' | '5' => Some(0x1d),
            '^' | '6' => Some(0x1e),
            '_' | '7' | '/' => Some(0x1f),
            '8' | '?' => Some(0x7f),
            _ => None,
        };
        match byte {
            Some(b) => out.push(b),
            None => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        return out;
    }
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(key: Key, mods: Mods) -> Vec<u8> {
        encode(
            key,
            mods,
            KeyAction::Press,
            Modes::empty(),
            KeyboardModes::NO_MODE,
        )
    }

    #[test]
    fn legacy_basics() {
        assert_eq!(enc(Key::Char('a'), Mods::empty()), b"a");
        assert_eq!(enc(Key::Char('c'), Mods::CTRL), b"\x03");
        assert_eq!(enc(Key::Char('x'), Mods::ALT), b"\x1bx");
        assert_eq!(enc(Key::Enter, Mods::empty()), b"\r");
        assert_eq!(enc(Key::Tab, Mods::SHIFT), b"\x1b[Z");
        assert_eq!(enc(Key::Up, Mods::empty()), b"\x1b[A");
        assert_eq!(enc(Key::Up, Mods::SHIFT), b"\x1b[1;2A");
        assert_eq!(enc(Key::Delete, Mods::CTRL), b"\x1b[3;5~");
        assert_eq!(enc(Key::F(1), Mods::empty()), b"\x1bOP");
        assert_eq!(enc(Key::F(5), Mods::empty()), b"\x1b[15~");
    }

    #[test]
    fn app_cursor_mode_uses_ss3() {
        let v = encode(
            Key::Left,
            Mods::empty(),
            KeyAction::Press,
            Modes::APP_CURSOR,
            KeyboardModes::NO_MODE,
        );
        assert_eq!(v, b"\x1bOD");
    }

    #[test]
    fn kitty_disambiguate() {
        let k = KeyboardModes::DISAMBIGUATE_ESC_CODES;
        let e = |key, mods| encode(key, mods, KeyAction::Press, Modes::empty(), k);
        assert_eq!(e(Key::Escape, Mods::empty()), b"\x1b[27u");
        assert_eq!(e(Key::Char('a'), Mods::CTRL), b"\x1b[97;5u");
        assert_eq!(e(Key::Char('a'), Mods::empty()), b"a");
        assert_eq!(e(Key::Enter, Mods::empty()), b"\r");
        assert_eq!(e(Key::Enter, Mods::SHIFT), b"\x1b[13;2u");
        assert_eq!(e(Key::Up, Mods::empty()), b"\x1b[A");
    }

    #[test]
    fn kitty_event_types() {
        let k = KeyboardModes::DISAMBIGUATE_ESC_CODES | KeyboardModes::REPORT_EVENT_TYPES;
        let rel = encode(
            Key::Char('a'),
            Mods::CTRL,
            KeyAction::Release,
            Modes::empty(),
            k,
        );
        assert_eq!(rel, b"\x1b[97;5:3u");
        let none = encode(
            Key::Char('a'),
            Mods::empty(),
            KeyAction::Release,
            Modes::empty(),
            KeyboardModes::NO_MODE,
        );
        assert!(none.is_empty());
    }
}
