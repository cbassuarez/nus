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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    /// Motion with no button held (mode 1003 only).
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// The pointer moved with `button` held (or none, in mode 1003).
    Motion,
}

/// Encode a mouse event per the modes the application set. `cell` is
/// 0-based (column, row) in the viewport; `px` is the pointer's 0-based
/// pixel offset inside the text area, for mode 1016. Returns nothing when
/// no mouse mode is on, or the mode doesn't want this event.
pub fn encode_mouse(
    button: MouseButton,
    action: MouseAction,
    mods: Mods,
    cell: (usize, usize),
    px: (u32, u32),
    modes: Modes,
) -> Vec<u8> {
    if !modes.intersects(Modes::ANY_MOUSE) {
        return Vec::new();
    }
    if action == MouseAction::Motion {
        let wants = if button == MouseButton::None {
            modes.contains(Modes::MOUSE_ANY)
        } else {
            modes.intersects(Modes::MOUSE_MOTION | Modes::MOUSE_ANY)
        };
        if !wants {
            return Vec::new();
        }
    }
    let wheel = matches!(
        button,
        MouseButton::WheelUp
            | MouseButton::WheelDown
            | MouseButton::WheelLeft
            | MouseButton::WheelRight
    );
    // Wheels only press; X10 mode (1000) only reports presses.
    if wheel && action != MouseAction::Press {
        return Vec::new();
    }
    let mut cb: u32 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::None => 3,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
        MouseButton::WheelLeft => 66,
        MouseButton::WheelRight => 67,
    };
    if mods.contains(Mods::SHIFT) {
        cb += 4;
    }
    if mods.contains(Mods::ALT) {
        cb += 8;
    }
    if mods.contains(Mods::CTRL) {
        cb += 16;
    }
    if action == MouseAction::Motion {
        cb += 32;
    }
    if modes.contains(Modes::MOUSE_SGR) || modes.contains(Modes::MOUSE_SGR_PIXEL) {
        let (x, y) = if modes.contains(Modes::MOUSE_SGR_PIXEL) {
            (px.0 + 1, px.1 + 1)
        } else {
            (cell.0 as u32 + 1, cell.1 as u32 + 1)
        };
        let fin = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return format!("[<{cb};{x};{y}{fin}").into_bytes();
    }
    if action == MouseAction::Release {
        cb = (cb & !3) | 3;
    }
    let mut out = b"[M".to_vec();
    let coord = |v: usize, out: &mut Vec<u8>| {
        let v = v as u32 + 1 + 32;
        if modes.contains(Modes::MOUSE_UTF8) {
            if v > 2047 {
                return false;
            }
            let mut b = [0u8; 4];
            out.extend_from_slice(
                char::from_u32(v)
                    .unwrap_or(' ')
                    .encode_utf8(&mut b)
                    .as_bytes(),
            );
            true
        } else {
            if v > 255 {
                return false;
            }
            out.push(v as u8);
            true
        }
    };
    out.push((cb + 32) as u8);
    if !coord(cell.0, &mut out) || !coord(cell.1, &mut out) {
        // Out of the encoding's range: X10 can't say it, so say nothing.
        return Vec::new();
    }
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

    #[test]
    fn mouse_reports() {
        use MouseAction as A;
        use MouseButton as B;
        let m = Modes::MOUSE_CLICK;
        assert_eq!(
            encode_mouse(B::Left, A::Press, Mods::empty(), (0, 0), (0, 0), m),
            b"[M !!"
        );
        assert_eq!(
            encode_mouse(B::Left, A::Release, Mods::empty(), (0, 0), (0, 0), m),
            b"[M#!!"
        );
        assert!(encode_mouse(B::Left, A::Motion, Mods::empty(), (1, 1), (0, 0), m).is_empty());
        assert!(encode_mouse(
            B::Left,
            A::Press,
            Mods::empty(),
            (1, 1),
            (0, 0),
            Modes::empty()
        )
        .is_empty());
        let sgr = Modes::MOUSE_ANY | Modes::MOUSE_SGR;
        assert_eq!(
            encode_mouse(B::Right, A::Press, Mods::CTRL, (9, 4), (0, 0), sgr),
            b"[<18;10;5M"
        );
        assert_eq!(
            encode_mouse(B::Left, A::Release, Mods::empty(), (9, 4), (0, 0), sgr),
            b"[<0;10;5m"
        );
        assert_eq!(
            encode_mouse(B::None, A::Motion, Mods::empty(), (2, 3), (0, 0), sgr),
            b"[<35;3;4M"
        );
        assert_eq!(
            encode_mouse(B::WheelUp, A::Press, Mods::empty(), (0, 0), (0, 0), sgr),
            b"[<64;1;1M"
        );
        let pix = sgr | Modes::MOUSE_SGR_PIXEL;
        assert_eq!(
            encode_mouse(B::Left, A::Press, Mods::empty(), (0, 0), (17, 33), pix),
            b"[<0;18;34M"
        );
    }
}
