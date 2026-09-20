//! One line of text, edited the way every field in nus edits it: the
//! prompt, the palette, the ask panel, the atlas, the find bands, a
//! rename. The caret lives at the end. A key goes in and the line changes:
//!
//!   Backspace                 a character; Ctrl (⌥ on macOS) takes a word
//!   Ctrl/⌘ V                  the clipboard's first line; Ctrl+Shift+V too
//!   Ctrl/⌘ C · Ctrl/⌘ X       the line to the clipboard; X clears it
//!   Ctrl+W · Ctrl+U           a word, the line — the shell's own
//!   a character, Space        typed in, up to `room`
//!
//! Enter, Escape, Tab and the arrows are not editing keys: the field's
//! owner decides what they do. `Took` says whether the line changed, so a
//! list under it can drop back to its first row.

use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::app::KeyIn;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Took {
    /// Not an editing key.
    No,
    /// Taken, the line the same (a copy, a backspace on nothing).
    Same,
    /// Taken, the line changed.
    Changed,
}

impl Took {
    pub fn taken(self) -> bool {
        self != Took::No
    }
    pub fn changed(self) -> bool {
        self == Took::Changed
    }
}

/// The chord that copies and pastes here: ⌘ on macOS, Ctrl elsewhere.
pub fn command(mods: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        mods.super_key()
    } else {
        mods.control_key()
    }
}

/// The modifier that makes Backspace take a word.
fn by_word(mods: ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        mods.alt_key()
    } else {
        mods.control_key()
    }
}

/// The clipboard's first line, trimmed of its ends.
pub fn clipboard_line() -> Option<String> {
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?;
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim().to_string();
    (!line.is_empty()).then_some(line)
}

pub fn set_clipboard(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

/// Take the last word off the line: trailing spaces, then the word.
pub fn pop_word(line: &mut String) {
    let trimmed = line.trim_end();
    let cut = trimmed.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
    line.truncate(cut);
}

/// Push what was typed, as far as there is room.
fn push(line: &mut String, text: &str, room: usize) {
    let have = line.chars().count();
    if have >= room {
        return;
    }
    line.extend(text.chars().filter(|c| !c.is_control()).take(room - have));
}

/// One key on the line. `room` is the most characters it holds.
pub fn edit(line: &mut String, ev: &KeyIn, mods: ModifiersState, room: usize) -> Took {
    if ev.state != ElementState::Pressed {
        return Took::No;
    }
    let cmd = command(mods);
    match &ev.logical_key {
        Key::Named(NamedKey::Backspace) => {
            if line.is_empty() {
                return Took::Same;
            }
            if by_word(mods) || (cmd && mods.shift_key()) {
                pop_word(line);
            } else {
                line.pop();
            }
            Took::Changed
        }
        Key::Named(NamedKey::Space) if !cmd => {
            push(line, " ", room);
            Took::Changed
        }
        Key::Character(c) if cmd => match c.to_lowercase().as_str() {
            "v" => match clipboard_line() {
                Some(text) => {
                    push(line, &text, room);
                    Took::Changed
                }
                None => Took::Same,
            },
            "c" => {
                if !line.is_empty() {
                    set_clipboard(line);
                }
                Took::Same
            }
            "x" => {
                if line.is_empty() {
                    return Took::Same;
                }
                set_clipboard(line);
                line.clear();
                Took::Changed
            }
            // The shell's own: a word, the line.
            "w" if !cfg!(target_os = "macos") && !mods.shift_key() => {
                if line.is_empty() {
                    return Took::Same;
                }
                pop_word(line);
                Took::Changed
            }
            "u" if !mods.shift_key() => {
                if line.is_empty() {
                    return Took::Same;
                }
                line.clear();
                Took::Changed
            }
            _ => Took::No,
        },
        // Typed text. AltGr arrives as Ctrl+Alt on Windows with the
        // character it makes, so the text decides, not the modifiers.
        Key::Character(c) if !mods.super_key() && (!mods.control_key() || mods.alt_key()) => {
            let text = ev.text.as_deref().unwrap_or(c);
            if text.chars().any(char::is_control) {
                return Took::No;
            }
            push(line, text, room);
            Took::Changed
        }
        _ => Took::No,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{KeyCode, NativeKeyCode, PhysicalKey, SmolStr};

    fn key(k: Key, text: Option<&str>) -> KeyIn {
        KeyIn {
            physical_key: PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
            logical_key: k,
            text: text.map(SmolStr::new),
            state: ElementState::Pressed,
            repeat: false,
        }
    }
    fn ch(c: &str) -> KeyIn {
        key(Key::Character(SmolStr::new(c)), Some(c))
    }
    fn named(n: NamedKey) -> KeyIn {
        key(Key::Named(n), None)
    }
    const CTRL: ModifiersState = ModifiersState::CONTROL;
    const ALT: ModifiersState = ModifiersState::ALT;
    const NONE: ModifiersState = ModifiersState::empty();

    #[test]
    fn types_and_erases() {
        let mut l = String::new();
        assert_eq!(edit(&mut l, &ch("a"), NONE, 40), Took::Changed);
        assert_eq!(edit(&mut l, &named(NamedKey::Space), NONE, 40), Took::Changed);
        assert_eq!(edit(&mut l, &ch("b"), NONE, 40), Took::Changed);
        assert_eq!(l, "a b");
        assert_eq!(edit(&mut l, &named(NamedKey::Backspace), NONE, 40), Took::Changed);
        assert_eq!(l, "a ");
        // A word at a time, then nothing left to take.
        l = "git commit -m".into();
        let word = if cfg!(target_os = "macos") { ALT } else { CTRL };
        assert_eq!(edit(&mut l, &named(NamedKey::Backspace), word, 40), Took::Changed);
        assert_eq!(l, "git commit ");
        assert_eq!(edit(&mut l, &named(NamedKey::Backspace), word, 40), Took::Changed);
        assert_eq!(l, "git ");
        l.clear();
        assert_eq!(edit(&mut l, &named(NamedKey::Backspace), NONE, 40), Took::Same);
    }

    #[test]
    fn room_and_control_characters() {
        let mut l = String::new();
        for _ in 0..5 {
            edit(&mut l, &ch("x"), NONE, 3);
        }
        assert_eq!(l, "xxx");
        assert_eq!(edit(&mut l, &ch("\u{8}"), NONE, 40), Took::No);
        let _ = PhysicalKey::Code(KeyCode::KeyA);
    }

    #[test]
    fn the_owner_keeps_its_keys() {
        let mut l = "abc".to_string();
        for n in [NamedKey::Enter, NamedKey::Escape, NamedKey::Tab, NamedKey::ArrowDown, NamedKey::ArrowUp] {
            assert_eq!(edit(&mut l, &named(n), NONE, 40), Took::No);
        }
        // An app chord is not text.
        let cmd = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { CTRL };
        assert_eq!(edit(&mut l, &ch("k"), cmd, 40), Took::No);
        assert_eq!(l, "abc");
    }

    #[test]
    fn a_word_and_the_line() {
        let mut l = "cargo test --all".to_string();
        assert_eq!(edit(&mut l, &ch("u"), if cfg!(target_os = "macos") { ModifiersState::SUPER } else { CTRL }, 40), Took::Changed);
        assert!(l.is_empty());
        pop_word(&mut String::new());
        let mut w = "a  ".to_string();
        pop_word(&mut w);
        assert!(w.is_empty());
    }
}
