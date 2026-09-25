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
//!
//! A field that shows a caret and a selection (the prompt) edits through
//! `edit_at` instead, with a `Cursor`: ← → Home End move it, Shift
//! extends, ⌥/Ctrl go by word, ⌘/Ctrl A selects the line, and typing,
//! paste, cut and erase act on the selection when there is one.

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

/// A caret and a selection on one line, in characters. `caret: None` is
/// the end of the line, where `edit` keeps it; `anchor` is where a
/// selection started, the caret its other end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub caret: Option<usize>,
    pub anchor: Option<usize>,
}

impl Cursor {
    /// The caret, as a character index into `line`.
    pub fn at(&self, line: &str) -> usize {
        let n = line.chars().count();
        self.caret.unwrap_or(n).min(n)
    }
    /// The selection, start..end, when it holds anything.
    pub fn range(&self, line: &str) -> Option<(usize, usize)> {
        let n = line.chars().count();
        let (a, b) = (self.anchor?.min(n), self.at(line));
        (a != b).then(|| (a.min(b), a.max(b)))
    }
    pub fn select_all(&mut self, line: &str) {
        self.anchor = Some(0);
        self.caret = Some(line.chars().count());
    }
    /// Put the caret at `to`; `extend` keeps (or starts) a selection.
    pub fn move_to(&mut self, line: &str, to: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.at(line));
        } else {
            self.anchor = None;
        }
        let n = line.chars().count();
        self.caret = (to < n).then_some(to).or(if self.anchor.is_some() { Some(n) } else { None });
    }
    /// Select the word around `at` (a double click).
    pub fn select_word(&mut self, line: &str, at: usize) {
        let chars: Vec<char> = line.chars().collect();
        let at = at.min(chars.len());
        let word = |c: char| !c.is_whitespace();
        let mut a = at;
        while a > 0 && word(chars[a - 1]) {
            a -= 1;
        }
        let mut b = at;
        while b < chars.len() && word(chars[b]) {
            b += 1;
        }
        self.anchor = Some(a);
        self.caret = Some(b);
    }
}

/// Byte offset of character `i`.
pub fn byte_at(line: &str, i: usize) -> usize {
    line.char_indices().nth(i).map(|(b, _)| b).unwrap_or(line.len())
}

/// The next word boundary from `at`, left or right, as ⌥←/⌥→ go.
fn word_step(line: &str, at: usize, right: bool) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut i = at.min(chars.len());
    if right {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
    } else {
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
    }
    i
}

/// `edit` with a caret and a selection (see the module note).
pub fn edit_at(line: &mut String, cur: &mut Cursor, ev: &KeyIn, mods: ModifiersState, room: usize) -> Took {
    if ev.state != ElementState::Pressed {
        return Took::No;
    }
    let cmd = command(mods);
    let shift = mods.shift_key();
    let n = line.chars().count();
    let at = cur.at(line);
    let range = cur.range(line);
    // Moving: the caret, and with Shift the selection.
    let moved = match &ev.logical_key {
        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight) => {
            let right = matches!(ev.logical_key, Key::Named(NamedKey::ArrowRight));
            Some(if cmd {
                if right { n } else { 0 }
            } else if by_word(mods) {
                word_step(line, at, right)
            } else if let (Some((a, b)), false) = (range, shift) {
                if right { b } else { a }
            } else if right {
                (at + 1).min(n)
            } else {
                at.saturating_sub(1)
            })
        }
        Key::Named(NamedKey::Home) => Some(0),
        Key::Named(NamedKey::End) => Some(n),
        _ => None,
    };
    if let Some(to) = moved {
        cur.move_to(line, to, shift);
        return Took::Same;
    }
    if cmd && !shift && matches!(&ev.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("a")) {
        cur.select_all(line);
        return Took::Same;
    }
    let chord = |k: &str| cmd && matches!(&ev.logical_key, Key::Character(c) if c.eq_ignore_ascii_case(k));
    // With a selection: copy and cut take just it; erasing and typing
    // replace it.
    if let Some((a, b)) = range {
        let (ba, bb) = (byte_at(line, a), byte_at(line, b));
        if chord("c") {
            set_clipboard(&line[ba..bb]);
            return Took::Same;
        }
        let erase = matches!(ev.logical_key, Key::Named(NamedKey::Backspace) | Key::Named(NamedKey::Delete));
        let typing = !erase && !chord("x") && {
            // Would this key type or paste? Try it on a scratch line.
            let mut probe = String::new();
            edit(&mut probe, ev, mods, usize::MAX).changed()
        };
        if !(erase || typing || chord("x")) {
            return edit_tail(line, cur, ev, mods, room);
        }
        if chord("x") {
            set_clipboard(&line[ba..bb]);
        }
        line.replace_range(ba..bb, "");
        cur.move_to(line, a, false);
        if typing {
            edit_tail(line, cur, ev, mods, room);
        }
        return Took::Changed;
    }
    if matches!(ev.logical_key, Key::Named(NamedKey::Delete)) {
        if at >= n {
            return Took::Same;
        }
        let b = byte_at(line, at);
        line.replace_range(b..byte_at(line, at + 1), "");
        cur.move_to(line, at, false);
        return Took::Changed;
    }
    // Copy and cut with nothing selected take the whole line, as before.
    if chord("c") || chord("x") {
        return edit(line, ev, mods, room);
    }
    edit_tail(line, cur, ev, mods, room)
}

/// `edit` on the part of the line before the caret, the rest kept after it.
fn edit_tail(line: &mut String, cur: &mut Cursor, ev: &KeyIn, mods: ModifiersState, room: usize) -> Took {
    let at = cur.at(line);
    let split = byte_at(line, at);
    let tail = line[split..].to_string();
    let mut head = line[..split].to_string();
    let took = edit(&mut head, ev, mods, room.saturating_sub(tail.chars().count()));
    if took.changed() {
        let caret = head.chars().count();
        *line = head + &tail;
        cur.move_to(line, caret, false);
    }
    took
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

    #[test]
    fn a_caret_and_a_selection() {
        let cmd = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { CTRL };
        let mut l = "hello world".to_string();
        let mut c = Cursor::default();
        // Select all, then typing replaces it.
        assert_eq!(edit_at(&mut l, &mut c, &ch("a"), cmd, 40), Took::Same);
        assert_eq!(c.range(&l), Some((0, 11)));
        assert_eq!(edit_at(&mut l, &mut c, &ch("y"), NONE, 40), Took::Changed);
        assert_eq!(l, "y");
        assert_eq!(c.range(&l), None);
        // Typing in the middle.
        l = "abcd".into();
        c = Cursor::default();
        edit_at(&mut l, &mut c, &named(NamedKey::ArrowLeft), NONE, 40);
        edit_at(&mut l, &mut c, &named(NamedKey::ArrowLeft), NONE, 40);
        edit_at(&mut l, &mut c, &ch("X"), NONE, 40);
        assert_eq!(l, "abXcd");
        assert_eq!(c.at(&l), 3);
        edit_at(&mut l, &mut c, &named(NamedKey::Backspace), NONE, 40);
        assert_eq!(l, "abcd");
        edit_at(&mut l, &mut c, &named(NamedKey::Delete), NONE, 40);
        assert_eq!(l, "abd");
        // Shift extends; Backspace takes the selection.
        edit_at(&mut l, &mut c, &named(NamedKey::ArrowRight), ModifiersState::SHIFT, 40);
        assert_eq!(c.range(&l), Some((2, 3)));
        edit_at(&mut l, &mut c, &named(NamedKey::Backspace), NONE, 40);
        assert_eq!(l, "ab");
        assert_eq!(c.at(&l), 2);
        // A double click's word.
        l = "git commit -m".into();
        c.select_word(&l, 6);
        assert_eq!(c.range(&l), Some((4, 10)));
    }
}
