//! The editor pane: a full editor on ropey — many buffers in a strip,
//! find/replace, undo, tree-sitter colour — with the language server
//! (crates/lsp) for hover, completion, diagnostics, definition and
//! format-on-save. Drawn in the terminal's monospace at the terminal's
//! size, ruled like everything else; nothing hand-rolled where a crate is
//! the real thing (ropey for the text, the lsp crate for the protocol).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use nus_lsp::lsp_types::{CompletionItem, Diagnostic, DiagnosticSeverity, Url};
use nus_render::text::Style;
use nus_render::{Rect, Scene};
use ropey::Rope;
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, NamedKey};

use crate::app::{fade, App, Pane};
use nus_render::theme::metric as m;

/// One undo step: the whole text (ropey clones are cheap) and where the
/// caret was.
#[derive(Clone)]
struct Snap {
    text: Rope,
    cursor: usize,
    anchor: Option<usize>,
}

pub struct Buffer {
    pub path: Option<PathBuf>,
    pub uri: Option<Url>,
    pub text: Rope,
    /// Caret, as a char index. `anchor` is the other end of a selection.
    pub cursor: usize,
    pub anchor: Option<usize>,
    /// Column the caret wants when moving vertically through short lines.
    pub want_col: Option<usize>,
    /// The protocol's language id (`rust`), and the grammar's name for
    /// tree-sitter (`rust` too, mostly).
    pub language: &'static str,
    pub dirty: bool,
    /// First visible line.
    pub scroll: usize,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_edit: Instant,
    /// Diagnostics from the server, by the text they were published for.
    pub diags: Vec<Diagnostic>,
    /// Spans per line, cached by a hash of the text.
    spans: Option<(u64, Vec<Vec<(usize, usize, crate::predict::Tok)>>)>,
    /// The server has this document open.
    pub in_lsp: bool,
    /// A save waits on the formatter's answer (sent at this instant).
    pub save_pending: Option<Instant>,
}

impl Buffer {
    pub fn from_path(path: &Path) -> anyhow::Result<Buffer> {
        let s = std::fs::read_to_string(path)?;
        let s = s.replace("\r\n", "\n");
        let language = nus_lsp::registry::language_for(path);
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let abs = PathBuf::from(abs.to_string_lossy().trim_start_matches(r"\\?\"));
        Ok(Buffer {
            uri: Url::from_file_path(&abs).ok(),
            path: Some(abs),
            text: Rope::from_str(&s),
            cursor: 0,
            anchor: None,
            want_col: None,
            language,
            dirty: false,
            scroll: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: Instant::now(),
            diags: Vec::new(),
            spans: None,
            in_lsp: false,
            save_pending: None,
        })
    }

    pub fn empty() -> Buffer {
        Buffer {
            path: None,
            uri: None,
            text: Rope::new(),
            cursor: 0,
            anchor: None,
            want_col: None,
            language: "plaintext",
            dirty: false,
            scroll: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: Instant::now(),
            diags: Vec::new(),
            spans: None,
            in_lsp: false,
            save_pending: None,
        }
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into())
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    pub fn line_of(&self, c: usize) -> usize {
        self.text.char_to_line(c.min(self.len_chars()))
    }

    pub fn col_of(&self, c: usize) -> usize {
        let c = c.min(self.len_chars());
        c - self.text.line_to_char(self.line_of(c))
    }

    /// Char index of (line, col), clamped to the line.
    pub fn at(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.text.len_lines().saturating_sub(1));
        let start = self.text.line_to_char(line);
        let len = self.line_len(line);
        start + col.min(len)
    }

    /// Length of a line without its newline.
    pub fn line_len(&self, line: usize) -> usize {
        let l = self.text.line(line);
        let n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n - 1
        } else {
            n
        }
    }

    pub fn line_text(&self, line: usize) -> String {
        let l = self.text.line(line);
        let s: String = l.chars().collect();
        s.trim_end_matches('\n').to_string()
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.cursor {
            return None;
        }
        Some((a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> String {
        match self.selection() {
            Some((a, b)) => self.text.slice(a..b).to_string(),
            None => String::new(),
        }
    }

    /// Before a change: remember, and drop the redo stack. Typing runs
    /// within 400ms merge into one step.
    fn remember(&mut self, merge: bool) {
        let now = Instant::now();
        if merge && !self.undo.is_empty() && now.duration_since(self.last_edit).as_millis() < 400 {
            self.last_edit = now;
            return;
        }
        self.undo.push(Snap {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        if self.undo.len() > 400 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_edit = now;
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.spans = None;
        self.want_col = None;
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snap {
                text: self.text.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.text = s.text;
            self.cursor = s.cursor.min(self.len_chars());
            self.anchor = s.anchor;
            self.changed();
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snap {
                text: self.text.clone(),
                cursor: self.cursor,
                anchor: self.anchor,
            });
            self.text = s.text;
            self.cursor = s.cursor.min(self.len_chars());
            self.anchor = s.anchor;
            self.changed();
        }
    }

    /// Replace the selection (or insert at the caret) with `s`.
    pub fn insert(&mut self, s: &str, merge: bool) {
        self.remember(merge);
        if let Some((a, b)) = self.selection() {
            self.text.remove(a..b);
            self.cursor = a;
        }
        self.anchor = None;
        self.text.insert(self.cursor, s);
        self.cursor += s.chars().count();
        self.changed();
    }

    /// Delete a range; the caret lands at its start.
    pub fn delete(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        self.remember(true);
        self.text.remove(a..b.min(self.len_chars()));
        self.cursor = a;
        self.anchor = None;
        self.changed();
    }

    pub fn backspace(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.delete(a, b);
        } else if self.cursor > 0 {
            // Backspace at the start of a soft indent eats a tab's worth.
            let line = self.line_of(self.cursor);
            let col = self.col_of(self.cursor);
            let lt = self.line_text(line);
            let all_space = lt.chars().take(col).all(|c| c == ' ') && col > 0;
            let n = if all_space { ((col - 1) % 4) + 1 } else { 1 };
            self.delete(self.cursor - n, self.cursor);
        }
    }

    pub fn delete_forward(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.delete(a, b);
        } else if self.cursor < self.len_chars() {
            self.delete(self.cursor, self.cursor + 1);
        }
    }

    /// Enter: newline plus the current line's leading whitespace.
    pub fn newline(&mut self) {
        let line = self.line_of(self.cursor);
        let lt = self.line_text(line);
        let indent: String = lt.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let indent: String = indent.chars().take(self.col_of(self.cursor)).collect();
        let extra = if lt.trim_end().ends_with(['{', '(', '[', ':']) {
            "    "
        } else {
            ""
        };
        self.insert(&format!("\n{indent}{extra}"), false);
    }

    /// Tab: indent the selected lines, or insert spaces to the next stop.
    pub fn indent(&mut self, out: bool) {
        match self.selection() {
            Some((a, b)) if self.line_of(a) != self.line_of(b.saturating_sub(1)) || out => {
                self.remember(false);
                let (l0, l1) = (self.line_of(a), self.line_of(b.saturating_sub(1).max(a)));
                for l in (l0..=l1).rev() {
                    let start = self.text.line_to_char(l);
                    if out {
                        let lt = self.line_text(l);
                        let n = lt.chars().take(4).take_while(|c| *c == ' ').count();
                        self.text.remove(start..start + n);
                    } else {
                        self.text.insert(start, "    ");
                    }
                }
                let s0 = self.text.line_to_char(l0);
                let e1 = self.text.line_to_char(l1) + self.line_len(l1);
                self.anchor = Some(s0);
                self.cursor = e1;
                self.changed();
            }
            _ if out => {
                self.remember(false);
                let l = self.line_of(self.cursor);
                let start = self.text.line_to_char(l);
                let lt = self.line_text(l);
                let n = lt.chars().take(4).take_while(|c| *c == ' ').count();
                self.text.remove(start..start + n);
                self.cursor = self.cursor.saturating_sub(n).max(start);
                self.changed();
            }
            _ => {
                let col = self.col_of(self.cursor);
                let n = 4 - (col % 4);
                self.insert(&" ".repeat(n), true);
            }
        }
    }

    /// Replace the whole text (a formatter's answer), keeping the caret's
    /// line and column where it can.
    pub fn replace_all(&mut self, s: &str) {
        let (line, col) = (self.line_of(self.cursor), self.col_of(self.cursor));
        self.remember(false);
        self.text = Rope::from_str(s);
        self.cursor = self.at(line, col);
        self.anchor = None;
        self.changed();
    }

    /// Word bounds around a char index.
    pub fn word_at(&self, c: usize) -> (usize, usize) {
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
        let n = self.len_chars();
        let mut a = c.min(n);
        let mut b = a;
        while a > 0 && is_word(self.text.char(a - 1)) {
            a -= 1;
        }
        while b < n && is_word(self.text.char(b)) {
            b += 1;
        }
        (a, b)
    }

    /// Next word boundary from `c`, for Ctrl+arrows.
    pub fn word_step(&self, c: usize, forward: bool) -> usize {
        let n = self.len_chars();
        let class = |ch: char| {
            if ch.is_alphanumeric() || ch == '_' {
                1
            } else if ch.is_whitespace() {
                0
            } else {
                2
            }
        };
        if forward {
            let mut i = c;
            if i >= n {
                return n;
            }
            let k = class(self.text.char(i));
            while i < n && class(self.text.char(i)) == k {
                i += 1;
            }
            while i < n && class(self.text.char(i)) == 0 && self.text.char(i) != '\n' {
                i += 1;
            }
            i
        } else {
            let mut i = c;
            while i > 0 && class(self.text.char(i - 1)) == 0 && self.text.char(i - 1) != '\n' {
                i -= 1;
            }
            if i == 0 {
                return 0;
            }
            let k = class(self.text.char(i - 1));
            while i > 0 && class(self.text.char(i - 1)) == k {
                i -= 1;
            }
            i
        }
    }

    pub fn spans_for(&mut self, line: usize) -> Vec<(usize, usize, crate::predict::Tok)> {
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.text.len_bytes().hash(&mut h);
            for chunk in self.text.chunks() {
                chunk.hash(&mut h);
            }
            h.finish()
        };
        if self.spans.as_ref().map(|(h, _)| *h) != Some(hash) {
            let grammar = grammar_for(self.language);
            let all = if crate::syntax::has(grammar) {
                let text = self.text.to_string();
                crate::syntax::spans(grammar, &text).unwrap_or_default()
            } else {
                Vec::new()
            };
            // Split the file-wide spans by line.
            let mut per: Vec<Vec<(usize, usize, crate::predict::Tok)>> =
                vec![Vec::new(); self.text.len_lines()];
            for (a, len, class) in all {
                let l0 = self.text.char_to_line(a.min(self.len_chars()));
                let l1 = self.text.char_to_line((a + len).min(self.len_chars()));
                for l in l0..=l1.min(per.len().saturating_sub(1)) {
                    let ls = self.text.line_to_char(l);
                    let le = ls + self.line_len(l);
                    let (s0, s1) = (a.max(ls), (a + len).min(le));
                    if s1 > s0 {
                        per[l].push((s0 - ls, s1 - s0, class));
                    }
                }
            }
            self.spans = Some((hash, per));
        }
        self.spans
            .as_ref()
            .and_then(|(_, p)| p.get(line).cloned())
            .unwrap_or_default()
    }
}

/// The tree-sitter grammar folder for a language id.
pub fn grammar_for(language: &str) -> &str {
    match language {
        "shellscript" => "bash",
        "javascript" => "javascript",
        "typescript" => "typescript",
        "cpp" => "cpp",
        other => other,
    }
}

/// What a request was for, so its answer lands in the right place.
#[derive(Clone, Debug)]
pub enum Pending {
    Hover { uri: Url, at: usize },
    Completion { uri: Url },
    Definition { uri: Url },
    Format { uri: Url, then_save: bool },
    /// A completion for a shell's prompt line.
    PromptCompletion { uri: Url },
}

pub struct Find {
    pub query: String,
    pub replace: String,
    /// Which field has the keys.
    pub in_replace: bool,
    pub with_replace: bool,
    pub matches: Vec<(usize, usize)>,
    pub current: usize,
}

pub struct Completion {
    pub items: Vec<CompletionItem>,
    pub sel: usize,
    /// Where the word being completed starts.
    pub at: usize,
    pub scroll: usize,
}

pub struct HoverBox {
    pub text: String,
    pub at: usize,
}

pub struct EditorPane {
    pub rect: Rect,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub find: Option<Find>,
    pub completion: Option<Completion>,
    pub hover: Option<HoverBox>,
    /// Goto-line input, when open.
    pub goto: Option<String>,
    /// Layout from the last draw: text origin, cell size, visible rows,
    /// and the strip's hit rects (buffer index, close?).
    pub origin: (f32, f32),
    pub cell: (f32, f32),
    pub rows: usize,
    pub strip_hits: Vec<(Rect, usize, bool)>,
    pub dragging: bool,
    /// Pointer rest for hover: where and since when.
    pub rest: Option<((f32, f32), Instant)>,
    pub hover_sent_at: Option<usize>,
    /// The server's status line for this buffer's language.
    pub status: String,
    /// A one-line notice (saved, formatted, error), and when.
    pub notice: Option<(String, Instant)>,
}

impl EditorPane {
    pub fn new(rect: Rect) -> EditorPane {
        EditorPane {
            rect,
            buffers: Vec::new(),
            active: 0,
            find: None,
            completion: None,
            hover: None,
            goto: None,
            origin: (0.0, 0.0),
            cell: (8.0, 16.0),
            rows: 1,
            strip_hits: Vec::new(),
            dragging: false,
            rest: None,
            hover_sent_at: None,
            status: String::new(),
            notice: None,
        }
    }

    pub fn buf(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    pub fn buf_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.active)
    }

    pub fn title(&self) -> String {
        self.buf()
            .map(|b| b.name())
            .unwrap_or_else(|| "editor".into())
    }

    /// Open or switch to a file. Returns the buffer index.
    pub fn open(&mut self, path: &Path) -> anyhow::Result<usize> {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let abs = PathBuf::from(abs.to_string_lossy().trim_start_matches(r"\\?\"));
        if let Some(i) = self
            .buffers
            .iter()
            .position(|b| b.path.as_deref() == Some(abs.as_path()))
        {
            self.active = i;
            return Ok(i);
        }
        let b = Buffer::from_path(&abs)?;
        self.buffers.push(b);
        self.active = self.buffers.len() - 1;
        self.find = None;
        self.completion = None;
        self.hover = None;
        Ok(self.active)
    }

    /// The (line, col) under a point, or None outside the text.
    pub fn cell_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let b = self.buf()?;
        let (cw, ch) = self.cell;
        if y < self.origin.1 || x < self.origin.0 - cw * 0.5 {
            return None;
        }
        let row = ((y - self.origin.1) / ch).floor() as usize;
        let col = ((x - self.origin.0) / cw + 0.5).floor().max(0.0) as usize;
        let line = b.scroll + row;
        if line >= b.text.len_lines() {
            return Some((b.text.len_lines().saturating_sub(1), usize::MAX));
        }
        Some((line, col))
    }

    /// Keep the caret on screen.
    pub fn reveal(&mut self) {
        let rows = self.rows.max(1);
        if let Some(b) = self.buf_mut() {
            let line = b.line_of(b.cursor);
            if line < b.scroll {
                b.scroll = line;
            } else if line >= b.scroll + rows {
                b.scroll = line + 1 - rows;
            }
        }
    }

    pub fn scroll_by(&mut self, lines: i64) {
        let rows = self.rows.max(1);
        if let Some(b) = self.buf_mut() {
            let max = b
                .text
                .len_lines()
                .saturating_sub(rows.min(b.text.len_lines()));
            let to = (b.scroll as i64 - lines).clamp(0, max as i64);
            b.scroll = to as usize;
        }
    }

    /// Recompute find matches for the current query.
    pub fn refind(&mut self) {
        let Some(f) = self.find.as_mut() else { return };
        let Some(b) = self.buffers.get(self.active) else {
            return;
        };
        f.matches.clear();
        if f.query.is_empty() {
            return;
        }
        let text = b.text.to_string();
        let q = f.query.to_lowercase();
        let lower = text.to_lowercase();
        // Byte offsets in the lowered text line up with the original only
        // for ASCII; walk chars to be safe.
        let qlen = q.chars().count();
        let chars: Vec<char> = lower.chars().collect();
        let qc: Vec<char> = q.chars().collect();
        let mut i = 0;
        while i + qlen <= chars.len() {
            if chars[i..i + qlen] == qc[..] {
                f.matches.push((i, i + qlen));
                i += qlen.max(1);
            } else {
                i += 1;
            }
        }
        // Current: the first match at or after the caret.
        let cur = b.cursor;
        f.current = f.matches.iter().position(|&(a, _)| a >= cur).unwrap_or(0);
    }

    pub fn find_step(&mut self, forward: bool) {
        let Some(f) = self.find.as_mut() else { return };
        if f.matches.is_empty() {
            return;
        }
        let n = f.matches.len();
        f.current = if forward {
            (f.current + 1) % n
        } else {
            (f.current + n - 1) % n
        };
        let (a, b2) = f.matches[f.current];
        if let Some(b) = self.buffers.get_mut(self.active) {
            b.anchor = Some(a);
            b.cursor = b2;
        }
        self.reveal();
    }

    /// Select the current match without stepping.
    pub fn find_select(&mut self) {
        let Some(f) = self.find.as_ref() else { return };
        let Some(&(a, b2)) = f.matches.get(f.current) else {
            return;
        };
        if let Some(b) = self.buffers.get_mut(self.active) {
            b.anchor = Some(a);
            b.cursor = b2;
        }
        self.reveal();
    }

    pub fn replace_one(&mut self) {
        let Some(f) = self.find.as_ref() else { return };
        let Some(&(a, b2)) = f.matches.get(f.current) else {
            return;
        };
        let rep = f.replace.clone();
        if let Some(b) = self.buffers.get_mut(self.active) {
            b.anchor = Some(a);
            b.cursor = b2;
            b.insert(&rep, false);
        }
        self.refind();
        self.find_select();
    }

    pub fn replace_all(&mut self) {
        let Some(f) = self.find.as_ref() else { return };
        if f.matches.is_empty() {
            return;
        }
        let rep = f.replace.clone();
        let matches = f.matches.clone();
        if let Some(b) = self.buffers.get_mut(self.active) {
            b.remember(false);
            for &(a, b2) in matches.iter().rev() {
                b.text.remove(a..b2);
                b.text.insert(a, &rep);
            }
            b.cursor = b.cursor.min(b.len_chars());
            b.anchor = None;
            b.changed();
        }
        self.refind();
    }
}

impl App {
    /// The editor pane in focus, if any.
    pub(crate) fn focused_editor(&mut self) -> Option<&mut EditorPane> {
        let tab = self.tabs.get_mut(self.active)?;
        match tab.focused() {
            Pane::Editor(e) => Some(e),
            _ => None,
        }
    }

    /// Open a file in the editor: the focused editor pane, else the tab's
    /// other pane if it's one, else a new tab (or a split when `split`).
    pub(crate) fn open_file(&mut self, path: &Path, split: bool) {
        if !path.is_file() {
            self.notice(&format!("not a file · {}", path.display()));
            return;
        }
        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".nus.luau")) {
            self.open_layout(path);
            return;
        }
        let active = self.active;
        let existing = self.tabs.get(active).and_then(|t| {
            if matches!(t.left, Pane::Editor(_)) {
                Some(false)
            } else if matches!(t.right, Some(Pane::Editor(_))) {
                Some(true)
            } else {
                None
            }
        });
        let idx = match existing {
            Some(right) if !split || right => {
                let tab = &mut self.tabs[active];
                tab.focus_right = right;
                let pane = if right {
                    tab.right.as_mut().unwrap()
                } else {
                    &mut tab.left
                };
                let Pane::Editor(e) = pane else { return };
                match e.open(path) {
                    Ok(i) => Some((active, right, i)),
                    Err(err) => {
                        self.notice(&format!("{err}"));
                        None
                    }
                }
            }
            _ if split => {
                let mut e = EditorPane::new(Rect::new(0.0, 0.0, 1.0, 1.0));
                match e.open(path) {
                    Ok(i) => {
                        let tab = &mut self.tabs[active];
                        tab.right = Some(Pane::Editor(e));
                        tab.focus_right = true;
                        Some((active, true, i))
                    }
                    Err(err) => {
                        self.notice(&format!("{err}"));
                        None
                    }
                }
            }
            _ => {
                let mut e = EditorPane::new(Rect::new(0.0, 0.0, 1.0, 1.0));
                match e.open(path) {
                    Ok(i) => {
                        let tab = self.make_tab(Pane::Editor(e), None);
                        self.tabs.push(tab);
                        let n = self.tabs.len() - 1;
                        self.activate(n);
                        Some((n, false, i))
                    }
                    Err(err) => {
                        self.notice(&format!("{err}"));
                        None
                    }
                }
            }
        };
        if let Some((ti, right, bi)) = idx {
            self.lsp_open_buffer(ti, right, bi);
            self.files_root_from(path);
            self.apply_term_resizes(false);
        }
        self.dirty = true;
    }

    /// A short line in the editor's status row.
    pub(crate) fn notice(&mut self, s: &str) {
        if let Some(e) = self.focused_editor() {
            e.notice = Some((s.to_string(), Instant::now()));
        }
        self.dirty = true;
    }

    /// Keys for the focused editor. Returns true when consumed.
    pub(crate) fn editor_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        let pressed = ev.state == ElementState::Pressed;
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        let sup = self.mods.super_key();
        let cmd = if cfg!(target_os = "macos") { sup } else { ctrl };
        let Some(tab) = self.tabs.get(self.active) else {
            return false;
        };
        if !matches!(tab.focused_ref(), Pane::Editor(_)) {
            return false;
        }
        if !pressed {
            return true;
        }
        // App chords (Ctrl+Shift+letter) stay the app's.
        if ctrl
            && shift
            && matches!(&ev.logical_key, WKey::Character(_))
            && !matches!(ev.logical_key.to_text(), Some("Z") | Some("z"))
        {
            return false;
        }
        let key = ev.logical_key.clone();
        let text = ev.text.as_ref().map(|s| s.to_string());
        let mut request: Option<(&'static str, Pending)> = None;
        let mut save = false;
        let mut goto_def = false;
        let mut copy: Option<String> = None;
        let mut paste = false;
        {
            let Some(e) = self.focused_editor() else {
                return false;
            };
            // Goto line.
            if let Some(g) = e.goto.as_mut() {
                match &key {
                    WKey::Named(NamedKey::Escape) => e.goto = None,
                    WKey::Named(NamedKey::Enter) => {
                        let n: usize = g.trim().parse().unwrap_or(1);
                        e.goto = None;
                        if let Some(b) = e.buf_mut() {
                            b.cursor = b.at(n.saturating_sub(1), 0);
                            b.anchor = None;
                        }
                        e.reveal();
                    }
                    WKey::Named(NamedKey::Backspace) => {
                        g.pop();
                    }
                    WKey::Character(s) if s.chars().all(|c| c.is_ascii_digit()) => g.push_str(s),
                    _ => {}
                }
                self.dirty = true;
                return true;
            }
            // The find bar.
            if e.find.is_some() && !(cmd && matches!(key.to_text(), Some("f") | Some("h"))) {
                let f = e.find.as_mut().unwrap();
                match &key {
                    WKey::Named(NamedKey::Escape) => {
                        e.find = None;
                        if let Some(b) = e.buf_mut() {
                            b.anchor = None;
                        }
                    }
                    WKey::Named(NamedKey::Enter) if f.in_replace => {
                        if cmd {
                            e.replace_all();
                        } else {
                            e.replace_one();
                        }
                    }
                    WKey::Named(NamedKey::Enter) => e.find_step(!shift),
                    WKey::Named(NamedKey::Tab) if f.with_replace => f.in_replace = !f.in_replace,
                    WKey::Named(NamedKey::Backspace) => {
                        if f.in_replace {
                            f.replace.pop();
                        } else {
                            f.query.pop();
                            e.refind();
                            e.find_select();
                        }
                    }
                    WKey::Named(NamedKey::ArrowDown) => e.find_step(true),
                    WKey::Named(NamedKey::ArrowUp) => e.find_step(false),
                    _ => {
                        if let Some(t) =
                            text.as_deref().filter(|t| !t.chars().any(char::is_control))
                        {
                            if f.in_replace {
                                f.replace.push_str(t);
                            } else {
                                f.query.push_str(t);
                                e.refind();
                                e.find_select();
                            }
                        }
                    }
                }
                self.dirty = true;
                return true;
            }
            // The completion menu.
            if let Some(c) = e.completion.as_mut() {
                match &key {
                    WKey::Named(NamedKey::Escape) => {
                        e.completion = None;
                        self.dirty = true;
                        return true;
                    }
                    WKey::Named(NamedKey::ArrowDown) => {
                        c.sel = (c.sel + 1).min(c.items.len().saturating_sub(1));
                        self.dirty = true;
                        return true;
                    }
                    WKey::Named(NamedKey::ArrowUp) => {
                        c.sel = c.sel.saturating_sub(1);
                        self.dirty = true;
                        return true;
                    }
                    WKey::Named(NamedKey::Enter) | WKey::Named(NamedKey::Tab) => {
                        let at = c.at;
                        let item = c.items.get(c.sel).cloned();
                        e.completion = None;
                        if let (Some(item), Some(b)) = (item, e.buf_mut()) {
                            let ins = item
                                .insert_text
                                .clone()
                                .unwrap_or_else(|| item.label.clone());
                            // Text edits that replace a range win over the label.
                            let ins = match &item.text_edit {
                                Some(nus_lsp::lsp_types::CompletionTextEdit::Edit(te)) => {
                                    let text = b.text.to_string();
                                    let a = nus_lsp::offset_of(&text, te.range.start);
                                    b.anchor = Some(a);
                                    te.new_text.clone()
                                }
                                _ => {
                                    b.anchor = Some(at);
                                    ins
                                }
                            };
                            let ins = ins.split("$0").next().unwrap_or(&ins).to_string();
                            b.insert(&ins, false);
                        }
                        self.dirty = true;
                        return true;
                    }
                    _ => {}
                }
            }
            let page_rows = e.rows.max(1) as i64;
            let Some(b) = e.buf_mut() else { return true };
            let n = b.len_chars();
            let sel = |b: &mut Buffer, shift: bool| {
                if shift {
                    if b.anchor.is_none() {
                        b.anchor = Some(b.cursor);
                    }
                } else {
                    b.anchor = None;
                }
            };
            let mut moved = true;
            let mut scroll_lines: Option<i64> = None;
            match &key {
                WKey::Named(NamedKey::ArrowLeft) => {
                    sel(b, shift);
                    b.cursor = if ctrl {
                        b.word_step(b.cursor, false)
                    } else {
                        b.cursor.saturating_sub(1)
                    };
                    b.want_col = None;
                }
                WKey::Named(NamedKey::ArrowRight) => {
                    sel(b, shift);
                    b.cursor = if ctrl {
                        b.word_step(b.cursor, true)
                    } else {
                        (b.cursor + 1).min(n)
                    };
                    b.want_col = None;
                }
                WKey::Named(NamedKey::ArrowUp) | WKey::Named(NamedKey::ArrowDown) => {
                    let down = matches!(key, WKey::Named(NamedKey::ArrowDown));
                    if alt {
                        // Alt+Up/Down: move the line.
                        let l = b.line_of(b.cursor);
                        let target = if down { l + 1 } else { l.wrapping_sub(1) };
                        if target < b.text.len_lines() {
                            b.remember(false);
                            let col = b.col_of(b.cursor);
                            let a = b.line_text(l);
                            let t = b.line_text(target);
                            let (first, second) = if down { (l, target) } else { (target, l) };
                            let s0 = b.text.line_to_char(first);
                            let e1 = b.text.line_to_char(second) + b.line_len(second);
                            b.text.remove(s0..e1);
                            let joined = if down {
                                format!("{t}\n{a}")
                            } else {
                                format!("{a}\n{t}")
                            };
                            b.text.insert(s0, &joined);
                            b.cursor = b.at(target, col);
                            b.anchor = None;
                            b.changed();
                        }
                    } else if ctrl {
                        moved = false;
                        scroll_lines = Some(if down { -1 } else { 1 });
                    } else {
                        sel(b, shift);
                        let l = b.line_of(b.cursor);
                        let col = b.want_col.unwrap_or(b.col_of(b.cursor));
                        let target = if down { l + 1 } else { l.wrapping_sub(1) };
                        if target < b.text.len_lines() {
                            b.cursor = b.at(target, col);
                            b.want_col = Some(col);
                        } else if down {
                            b.cursor = n;
                        } else {
                            b.cursor = 0;
                        }
                    }
                }
                WKey::Named(NamedKey::Home) => {
                    sel(b, shift);
                    if ctrl {
                        b.cursor = 0;
                    } else {
                        let l = b.line_of(b.cursor);
                        let start = b.text.line_to_char(l);
                        let lt = b.line_text(l);
                        let first = start + lt.chars().take_while(|c| c.is_whitespace()).count();
                        b.cursor = if b.cursor == first { start } else { first };
                    }
                    b.want_col = None;
                }
                WKey::Named(NamedKey::End) => {
                    sel(b, shift);
                    if ctrl {
                        b.cursor = n;
                    } else {
                        let l = b.line_of(b.cursor);
                        b.cursor = b.text.line_to_char(l) + b.line_len(l);
                    }
                    b.want_col = None;
                }
                WKey::Named(NamedKey::PageUp) | WKey::Named(NamedKey::PageDown) => {
                    let down = matches!(key, WKey::Named(NamedKey::PageDown));
                    let rows = page_rows;
                    sel(b, shift);
                    let l = b.line_of(b.cursor) as i64;
                    let col = b.want_col.unwrap_or(b.col_of(b.cursor));
                    let target = (l + if down { rows } else { -rows })
                        .clamp(0, b.text.len_lines() as i64 - 1)
                        as usize;
                    b.cursor = b.at(target, col);
                    b.want_col = Some(col);
                    scroll_lines = Some(if down { -rows } else { rows });
                }
                WKey::Named(NamedKey::Backspace) => {
                    if ctrl && b.selection().is_none() {
                        let a = b.word_step(b.cursor, false);
                        b.delete(a, b.cursor);
                    } else {
                        b.backspace();
                    }
                }
                WKey::Named(NamedKey::Delete) => {
                    if ctrl && b.selection().is_none() {
                        let z = b.word_step(b.cursor, true);
                        b.delete(b.cursor, z);
                    } else {
                        b.delete_forward();
                    }
                }
                WKey::Named(NamedKey::Enter) => {
                    b.newline();
                }
                WKey::Named(NamedKey::Tab) => b.indent(shift),
                WKey::Named(NamedKey::Escape) => {
                    b.anchor = None;
                    e.hover = None;
                }
                WKey::Named(NamedKey::Space) if ctrl => {
                    moved = false;
                    if let Some(uri) = b.uri.clone() {
                        request = Some(("completion", Pending::Completion { uri }));
                    }
                }
                WKey::Named(NamedKey::Space) => b.insert(" ", true),
                WKey::Named(NamedKey::F12) => {
                    moved = false;
                    goto_def = true;
                }
                WKey::Named(NamedKey::F2) => {
                    moved = false;
                }
                WKey::Character(s) if cmd => {
                    moved = false;
                    match s.to_lowercase().as_str() {
                        "a" => {
                            b.anchor = Some(0);
                            b.cursor = n;
                        }
                        "z" if shift => b.redo(),
                        "z" => b.undo(),
                        "y" => b.redo(),
                        "c" => {
                            let t = b.selected_text();
                            copy = Some(if t.is_empty() {
                                b.line_text(b.line_of(b.cursor)) + "\n"
                            } else {
                                t
                            });
                        }
                        "x" => {
                            if let Some((a, z)) = b.selection() {
                                copy = Some(b.text.slice(a..z).to_string());
                                b.delete(a, z);
                            } else {
                                let l = b.line_of(b.cursor);
                                let s0 = b.text.line_to_char(l);
                                let e1 = (s0 + b.text.line(l).len_chars()).min(n);
                                copy = Some(b.text.slice(s0..e1).to_string());
                                b.delete(s0, e1);
                            }
                        }
                        "v" => paste = true,
                        "s" => save = true,
                        "f" => {
                            let q = b.selected_text();
                            let q = if q.contains('\n') { String::new() } else { q };
                            e.find = Some(Find {
                                query: q,
                                replace: String::new(),
                                in_replace: false,
                                with_replace: false,
                                matches: Vec::new(),
                                current: 0,
                            });
                            e.refind();
                            e.find_select();
                        }
                        "h" => {
                            let q = b.selected_text();
                            let q = if q.contains('\n') { String::new() } else { q };
                            let had = e.find.take();
                            e.find = Some(Find {
                                query: if q.is_empty() {
                                    had.map(|f| f.query).unwrap_or_default()
                                } else {
                                    q
                                },
                                replace: String::new(),
                                in_replace: false,
                                with_replace: true,
                                matches: Vec::new(),
                                current: 0,
                            });
                            e.refind();
                            e.find_select();
                        }
                        "g" => e.goto = Some(String::new()),
                        "d" => {
                            // Select the word, or the next match of the selection.
                            match b.selection() {
                                None => {
                                    let (a, z) = b.word_at(b.cursor);
                                    b.anchor = Some(a);
                                    b.cursor = z;
                                }
                                Some((a, z)) => {
                                    let q: String = b.text.slice(a..z).to_string();
                                    let text = b.text.to_string();
                                    let from = z;
                                    let chars: Vec<char> = text.chars().collect();
                                    let qc: Vec<char> = q.chars().collect();
                                    let mut i = from;
                                    while i + qc.len() <= chars.len() {
                                        if chars[i..i + qc.len()] == qc[..] {
                                            b.anchor = Some(i);
                                            b.cursor = i + qc.len();
                                            break;
                                        }
                                        i += 1;
                                    }
                                }
                            }
                            moved = true;
                        }
                        "/" => {
                            // Toggle a line comment on the selected lines.
                            let marker = comment_marker(b.language);
                            let (l0, l1) = match b.selection() {
                                Some((a, z)) => {
                                    (b.line_of(a), b.line_of(z.saturating_sub(1).max(a)))
                                }
                                None => {
                                    let l = b.line_of(b.cursor);
                                    (l, l)
                                }
                            };
                            b.remember(false);
                            let all_commented = (l0..=l1).all(|l| {
                                b.line_text(l).trim_start().starts_with(marker)
                                    || b.line_text(l).trim().is_empty()
                            });
                            for l in l0..=l1 {
                                let lt = b.line_text(l);
                                let start = b.text.line_to_char(l);
                                let ws = lt.chars().take_while(|c| c.is_whitespace()).count();
                                if all_commented {
                                    if let Some(rest) = lt[ws..].strip_prefix(marker) {
                                        let extra = if rest.starts_with(' ') { 1 } else { 0 };
                                        b.text.remove(
                                            start + ws..start + ws + marker.chars().count() + extra,
                                        );
                                    }
                                } else if !lt.trim().is_empty() {
                                    b.text.insert(start + ws, &format!("{marker} "));
                                }
                            }
                            b.cursor = b.cursor.min(b.len_chars());
                            b.changed();
                        }
                        "i" if shift => {
                            if let Some(uri) = b.uri.clone() {
                                request = Some((
                                    "format",
                                    Pending::Format {
                                        uri,
                                        then_save: false,
                                    },
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                WKey::Character(_) | WKey::Named(_) => {
                    if let Some(t) = text
                        .as_deref()
                        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                    {
                        if !ctrl && !alt {
                            b.insert(t, true);
                            // A trigger character asks the server as you type.
                            if let Some(uri) = b.uri.clone() {
                                if t == "."
                                    || t == ":"
                                    || t == ">"
                                    || t.chars().all(|c| c.is_alphanumeric() || c == '_')
                                {
                                    request = Some(("completion", Pending::Completion { uri }));
                                }
                            }
                        }
                    } else {
                        moved = false;
                    }
                }
                _ => moved = false,
            }
            if let Some(n) = scroll_lines {
                e.scroll_by(n);
            }
            if moved {
                e.hover = None;
                if !matches!(key, WKey::Character(_)) {
                    e.completion = None;
                }
                e.reveal();
            }
        }
        if let Some(text) = copy {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(text);
            }
        }
        if paste {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                if let Ok(text) = cb.get_text() {
                    let text = text.replace("\r\n", "\n");
                    if let Some(b) = self.focused_editor().and_then(|e| e.buf_mut()) {
                        b.insert(&text, false);
                    }
                    if let Some(e) = self.focused_editor() {
                        e.reveal();
                    }
                }
            }
        }
        if save {
            self.editor_save();
        }
        if goto_def {
            self.editor_request("definition");
        }
        if let Some((kind, _)) = request {
            self.editor_request(kind);
        }
        self.editor_synced();
        self.dirty = true;
        true
    }

    /// Mouse in the focused editor. Returns true when consumed.
    pub(crate) fn editor_mouse(
        &mut self,
        button: MouseButton,
        state: ElementState,
        x: f32,
        y: f32,
    ) -> bool {
        let pressed = state == ElementState::Pressed;
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let mut hit_editor = false;
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Editor(e) = p else { continue };
            if !pressed && button == MouseButton::Left {
                e.dragging = false;
                continue;
            }
            if !e.rect.contains(x, y) || !pressed {
                continue;
            }
            hit_editor = true;
            let mut start_drag = false;
            // The strip: switch or close a buffer.
            if let Some((_, i, close)) = e
                .strip_hits
                .iter()
                .find(|(r, _, _)| r.contains(x, y))
                .cloned()
            {
                if close || button == MouseButton::Middle {
                    e.buffers.remove(i);
                    if e.active >= e.buffers.len() {
                        e.active = e.buffers.len().saturating_sub(1);
                    }
                } else {
                    e.active = i;
                }
                e.find = None;
                e.completion = None;
                e.hover = None;
                self.dirty = true;
                return true;
            }
            if button == MouseButton::Left {
                if let Some((line, col)) = e.cell_at(x, y) {
                    e.completion = None;
                    e.hover = None;
                    let Some(b) = e.buf_mut() else { continue };
                    let c = if col == usize::MAX {
                        b.text.line_to_char(line) + b.line_len(line)
                    } else {
                        b.at(line, col)
                    };
                    if ctrl {
                        b.cursor = c;
                        b.anchor = None;
                        self.editor_request("definition");
                        return true;
                    }
                    if shift {
                        if b.anchor.is_none() {
                            b.anchor = Some(b.cursor);
                        }
                        b.cursor = c;
                    } else {
                        // Double click: the word. Triple: the line.
                        let now = Instant::now();
                        let count = match self.click_at {
                            Some((at, (px, py), n))
                                if now.duration_since(at).as_millis() < 400
                                    && (px - x).abs() < 4.0
                                    && (py - y).abs() < 4.0 =>
                            {
                                n + 1
                            }
                            _ => 1,
                        };
                        self.click_at = Some((now, (x, y), count));
                        match count {
                            1 => {
                                b.cursor = c;
                                b.anchor = None;
                                start_drag = true;
                            }
                            2 => {
                                let (a, z) = b.word_at(c);
                                b.anchor = Some(a);
                                b.cursor = z;
                            }
                            _ => {
                                let l = b.line_of(c);
                                let s0 = b.text.line_to_char(l);
                                b.anchor = Some(s0);
                                b.cursor = (s0 + b.text.line(l).len_chars()).min(b.len_chars());
                            }
                        }
                    }
                    b.want_col = None;
                    if start_drag {
                        e.dragging = true;
                    }
                }
            }
        }
        if hit_editor {
            self.dirty = true;
        }
        hit_editor
    }

    /// Pointer motion over editors: drag-select, and the hover timer.
    pub(crate) fn editor_motion(&mut self, x: f32, y: f32) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let mut dirty = false;
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Editor(e) = p else { continue };
            if e.dragging {
                if let Some((line, col)) = e.cell_at(x, y) {
                    if let Some(b) = e.buf_mut() {
                        let c = if col == usize::MAX {
                            b.text.line_to_char(line) + b.line_len(line)
                        } else {
                            b.at(line, col)
                        };
                        if b.anchor.is_none() {
                            b.anchor = Some(b.cursor);
                        }
                        if b.cursor != c {
                            b.cursor = c;
                            dirty = true;
                        }
                    }
                }
                continue;
            }
            if e.rect.contains(x, y) {
                let moved = e
                    .rest
                    .map(|((px, py), _)| (px - x).abs() > 2.0 || (py - y).abs() > 2.0)
                    .unwrap_or(true);
                if moved {
                    e.rest = Some(((x, y), Instant::now()));
                    e.hover_sent_at = None;
                    if e.hover.is_some() {
                        e.hover = None;
                        dirty = true;
                    }
                }
            } else {
                e.rest = None;
            }
        }
        if dirty {
            self.dirty = true;
        }
    }

    /// Once a loop: hover after the pointer rests, expire notices, and
    /// the save that waited on a formatter.
    pub(crate) fn editor_tick(&mut self) {
        let mut hover_req: Option<(usize, bool)> = None;
        let mut expired = false;
        for (ti, tab) in self.tabs.iter_mut().enumerate() {
            for (right, p) in
                std::iter::once((false, &mut tab.left)).chain(tab.right.as_mut().map(|r| (true, r)))
            {
                let Pane::Editor(e) = p else { continue };
                if let Some(((x, y), at)) = e.rest {
                    if at.elapsed().as_millis() > 450 && e.hover.is_none() {
                        if let Some((line, col)) = e.cell_at(x, y) {
                            if col != usize::MAX {
                                let c = e.buf().map(|b| b.at(line, col));
                                if let Some(c) = c {
                                    if e.hover_sent_at != Some(c) && ti == self.active {
                                        e.hover_sent_at = Some(c);
                                        hover_req = Some((c, right));
                                    }
                                }
                            }
                        }
                    }
                }
                if e.notice
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed().as_secs() > 4)
                {
                    e.notice = None;
                    expired = true;
                }
                if let Some(b) = e.buf_mut() {
                    if b.save_pending.is_some_and(|at| at.elapsed().as_secs() >= 3) {
                        b.save_pending = None;
                        expired = true;
                    }
                }
            }
        }
        if let Some((c, _)) = hover_req {
            self.editor_hover_at(c);
        }
        if expired {
            self.dirty = true;
        }
    }
}

/// The line-comment marker for a language.
pub fn comment_marker(language: &str) -> &'static str {
    match language {
        "python" | "shellscript" | "powershell" | "yaml" | "toml" | "ruby" | "perl" => "#",
        "lua" => "--",
        "html" | "markdown" => "<!--",
        _ => "//",
    }
}

/// Severity → a colour index into the theme's ANSI ramp.
pub fn severity_ansi(d: &Diagnostic) -> usize {
    match d.severity {
        Some(DiagnosticSeverity::ERROR) => 1,
        Some(DiagnosticSeverity::WARNING) => 3,
        _ => 4,
    }
}

// --- drawing ---

impl App {
    pub(crate) fn draw_editor(
        &mut self,
        scene: &mut Scene,
        e: &mut EditorPane,
        r: Rect,
        focused: bool,
    ) {
        let t = self.theme.clone();
        let (ink, paper) = (t.ink, t.paper);
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style {
            color: t.dim,
            ..label
        };
        let term_px = 13.0 * self.scale * 96.0 / 72.0;
        let mono = Style {
            font: self.f.term,
            px: term_px,
            color: ink,
            tracking: 0.0,
        };
        let mono_dim = Style {
            color: t.dim,
            ..mono
        };
        let cw = self.fonts.measure(mono, "M").max(1.0);
        let metrics = self.fonts.metrics(self.f.term, term_px);
        let ch = metrics.line_height.max(term_px * 1.25);
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        let signal = self.surface.signal;
        let caret = self.caret_color();
        let (mx, my) = self.mouse;
        let strip_h = self.px(30.0);
        let status_h = self.px(26.0);
        let hair = self.px(m::HAIRLINE);
        let pad = self.px(10.0);
        let motion_reduced = self.motion.reduced();
        let _ = motion_reduced;

        let scale = self.scale;
        let px = |v: f32| (v * scale).round();
        let fonts = &mut self.fonts;
        let fit = |fonts: &nus_render::text::FontSystem,
                   style: Style,
                   text: &str,
                   max_w: f32|
         -> String {
            if fonts.measure(style, text) <= max_w {
                return text.to_string();
            }
            let chars: Vec<char> = text.chars().collect();
            let (mut lo, mut hi) = (0usize, chars.len());
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let s: String = chars[..mid].iter().collect();
                if fonts.measure(style, &format!("{s}…")) <= max_w {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            let s: String = chars[..lo].iter().collect();
            format!("{s}…")
        };
        e.rect = r;
        e.strip_hits.clear();
        scene.rect(r, paper);
        scene.layer(Some(r));

        // The strip: one cell per buffer, the active one underlined in signal.
        let mut x = r.x + pad;
        let strip_base = r.y + px(19.0);
        for i in 0..e.buffers.len() {
            let b = &e.buffers[i];
            let name = b.name();
            let active = i == e.active;
            let st = if active {
                Style {
                    color: ink,
                    ..strong
                }
            } else {
                dim
            };
            let w = fonts.measure(st, &name);
            let cell = Rect::new(x - px(6.0), r.y, w + px(30.0), strip_h);
            let hot = cell.contains(mx, my);
            if active {
                scene.rect(
                    Rect::new(cell.x, r.y + strip_h - px(2.0), cell.w, px(2.0)),
                    signal,
                );
            } else if hot {
                scene.rect(cell, crate::surface::mix(paper, ink, 0.05));
            }
            fonts.draw(scene, st, x, strip_base, &name);
            // Dirty dot, or a close × when hot.
            let dx = x + w + px(8.0);
            let close_r = Rect::new(dx - px(4.0), r.y + px(8.0), px(14.0), px(14.0));
            if hot {
                fonts.draw(
                    scene,
                    Style {
                        color: if close_r.contains(mx, my) {
                            signal
                        } else {
                            t.dim
                        },
                        ..label
                    },
                    dx,
                    strip_base,
                    "×",
                );
                e.strip_hits.push((close_r, i, true));
            } else if b.dirty {
                scene.rect(
                    Rect::new(dx, strip_base - px(7.0), px(6.0), px(6.0)),
                    signal,
                );
            }
            e.strip_hits.push((cell, i, false));
            x = cell.right() + px(2.0);
        }
        scene.hline(r.x, r.y + strip_h - hair, r.w, hair, ink);

        let Some(bi) = (e.active < e.buffers.len()).then_some(e.active) else {
            fonts.draw(
                scene,
                dim,
                r.x + pad,
                r.y + strip_h + px(40.0),
                "no file open · type a path at the prompt",
            );
            scene.layer(None);
            return;
        };

        // Geometry.
        let gutter_w = {
            let digits = e.buffers[bi]
                .text
                .len_lines()
                .max(1)
                .to_string()
                .len()
                .max(3);
            cw * (digits as f32 + 2.0)
        };
        let text_top = r.y + strip_h + px(6.0);
        let text_bottom = r.bottom() - status_h - if e.find.is_some() { status_h } else { 0.0 };
        let rows = ((text_bottom - text_top) / ch).floor().max(1.0) as usize;
        e.rows = rows;
        e.cell = (cw, ch);
        e.origin = (r.x + gutter_w + pad, text_top);
        let (ox, oy) = e.origin;
        let baseline_off = metrics.ascent + (ch - (metrics.ascent + metrics.descent)) * 0.5;

        let b = &mut e.buffers[bi];
        let max_scroll = b.text.len_lines().saturating_sub(1);
        b.scroll = b.scroll.min(max_scroll);
        let cur_line = b.line_of(b.cursor);
        let sel = b.selection();
        let n_lines = b.text.len_lines();
        let find_matches: Vec<(usize, usize)> = e
            .find
            .as_ref()
            .map(|f| f.matches.clone())
            .unwrap_or_default();
        let find_cur = e
            .find
            .as_ref()
            .and_then(|f| f.matches.get(f.current).copied());
        let diags: Vec<(usize, usize, usize, usize, usize)> = {
            let text = b.text.to_string();
            b.diags
                .iter()
                .map(|d| {
                    let a = nus_lsp::offset_of(&text, d.range.start);
                    let z = nus_lsp::offset_of(&text, d.range.end).max(a + 1);
                    (
                        a,
                        z,
                        severity_ansi(d),
                        d.range.start.line as usize,
                        d.range.end.line as usize,
                    )
                })
                .collect()
        };
        let wash = crate::surface::mix(paper, ink, 0.04);
        let sel_color = self.theme.selection;
        let match_color = fade(ansi(3), 0.25);

        for row in 0..rows {
            let line = b.scroll + row;
            if line >= n_lines {
                break;
            }
            let ly = oy + row as f32 * ch;
            let base = ly + baseline_off;
            let ls = b.text.line_to_char(line);
            let len = b.line_len(line);
            // Current line wash.
            if line == cur_line && focused {
                scene.rect(Rect::new(r.x, ly, r.w, ch), wash);
            }
            // Gutter number, a diagnostic mark beside it.
            let num = format!("{:>w$}", line + 1, w = (gutter_w / cw) as usize - 2);
            let ns = if line == cur_line {
                Style {
                    color: ink,
                    ..mono_dim
                }
            } else {
                mono_dim
            };
            fonts.draw(scene, ns, r.x + cw * 0.5, base, &num);
            if let Some(&(_, _, sev, _, _)) = diags
                .iter()
                .find(|&&(_, _, _, l0, l1)| line >= l0 && line <= l1)
            {
                scene.rect(
                    Rect::new(
                        r.x + gutter_w - cw * 0.9,
                        ly + ch * 0.35,
                        cw * 0.35,
                        ch * 0.3,
                    ),
                    ansi(sev),
                );
            }
            // Selection and matches under the text.
            if let Some((a, z)) = sel {
                let (s0, s1) = (a.max(ls), z.min(ls + len + 1));
                if s1 > s0 {
                    let w = if z > ls + len {
                        (s1 - s0) as f32 * cw + cw * 0.4
                    } else {
                        (s1 - s0) as f32 * cw
                    };
                    scene.rect(Rect::new(ox + (s0 - ls) as f32 * cw, ly, w, ch), sel_color);
                }
            }
            for &(a, z) in &find_matches {
                let (s0, s1) = (a.max(ls), z.min(ls + len));
                if s1 > s0 {
                    let c = if Some((a, z)) == find_cur {
                        fade(ansi(3), 0.5)
                    } else {
                        match_color
                    };
                    scene.rect(
                        Rect::new(ox + (s0 - ls) as f32 * cw, ly, (s1 - s0) as f32 * cw, ch),
                        c,
                    );
                }
            }
            // The text, as coloured runs.
            let lt = b.line_text(line);
            let spans = b.spans_for(line);
            let chars: Vec<char> = lt.chars().collect();
            let mut col = 0usize;
            let mut spans = spans;
            spans.sort_by_key(|s| s.0);
            let mut draw_run = |scene: &mut Scene,
                                fonts: &mut nus_render::text::FontSystem,
                                from: usize,
                                to: usize,
                                color: nus_render::Color| {
                if to <= from || from >= chars.len() {
                    return;
                }
                let run: String = chars[from..to.min(chars.len())].iter().collect();
                let run = run.replace('\t', "    ");
                fonts.draw(
                    scene,
                    Style { color, ..mono },
                    ox + from as f32 * cw,
                    base,
                    &run,
                );
            };
            for (a, l, class) in spans {
                if a > col {
                    draw_run(scene, fonts, col, a, ink);
                }
                let color = match class {
                    crate::predict::Tok::Command => ansi(4),
                    crate::predict::Tok::Flag => ansi(6),
                    crate::predict::Tok::Str => ansi(2),
                    crate::predict::Tok::Num => ansi(5),
                    crate::predict::Tok::Op => ansi(3),
                    crate::predict::Tok::Path => ansi(6),
                    crate::predict::Tok::Plain => ink,
                };
                draw_run(scene, fonts, a.max(col), a + l, color);
                col = (a + l).max(col);
            }
            draw_run(scene, fonts, col, chars.len(), ink);
            // Diagnostics: a dotted underline.
            for &(a, z, sev, _, _) in &diags {
                let (s0, s1) = (a.max(ls), z.min(ls + len.max(1)));
                if s1 > s0 {
                    let uy = ly + ch - px(2.5);
                    let mut ux = ox + (s0 - ls) as f32 * cw;
                    let end = ox + (s1 - ls) as f32 * cw;
                    while ux < end {
                        scene.rect(Rect::new(ux, uy, px(2.0), px(1.5)), ansi(sev));
                        ux += px(4.0);
                    }
                }
            }
            // The caret.
            if line == cur_line && focused && e.goto.is_none() && e.find.is_none() {
                let cx = ox + b.col_of(b.cursor) as f32 * cw;
                scene.rect(Rect::new(cx - px(0.5), ly, px(2.0), ch), caret);
            }
        }

        // Status row: path · Ln, Col · language · server, or the notice.
        let sy = r.bottom() - status_h;
        scene.hline(r.x, sy, r.w, hair, ink);
        let base = sy + px(17.0);
        let b = &e.buffers[bi];
        let left = match &e.notice {
            Some((s, _)) => s.clone(),
            None => {
                let p = b
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let n_err = b
                    .diags
                    .iter()
                    .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
                    .count();
                let n_warn = b
                    .diags
                    .iter()
                    .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
                    .count();
                let mut s = p;
                if n_err + n_warn > 0 {
                    s.push_str(&format!(" · {n_err} errors · {n_warn} warnings"));
                }
                s
            }
        };
        let right_text = {
            let mut s = format!(
                "Ln {}, Col {} · {}",
                cur_line + 1,
                b.col_of(b.cursor) + 1,
                b.language
            );
            if !e.status.is_empty() {
                s.push_str(" · ");
                s.push_str(&e.status);
            }
            if let Some(g) = &e.goto {
                s = format!("GO TO LINE {g}_");
            }
            s
        };
        let rw = fonts.measure(label, &right_text);
        let left = fit(fonts, dim, &left, r.w - rw - 3.0 * pad);
        fonts.draw(scene, dim, r.x + pad, base, &left);
        fonts.draw(
            scene,
            if e.goto.is_some() {
                Style {
                    color: signal,
                    ..strong
                }
            } else {
                dim
            },
            r.right() - pad - rw,
            base,
            &right_text,
        );

        // Find bar above the status row.
        if let Some(f) = &e.find {
            let fy = sy - status_h;
            scene.rect(Rect::new(r.x, fy, r.w, status_h), wash);
            scene.hline(r.x, fy, r.w, hair, ink);
            let base = fy + px(17.0);
            let mut x = r.x + pad;
            let head = if f.with_replace { "REPLACE" } else { "FIND" };
            x += fonts.draw(scene, strong, x, base, head) + pad;
            let q = format!("{}{}", f.query, if !f.in_replace { "_" } else { "" });
            x += fonts.draw(
                scene,
                Style {
                    color: if f.in_replace { t.dim } else { ink },
                    ..mono
                },
                x,
                base,
                &q,
            ) + pad * 2.0;
            if f.with_replace {
                x += fonts.draw(scene, dim, x, base, "WITH") + pad;
                let rp = format!("{}{}", f.replace, if f.in_replace { "_" } else { "" });
                x += fonts.draw(
                    scene,
                    Style {
                        color: if f.in_replace { ink } else { t.dim },
                        ..mono
                    },
                    x,
                    base,
                    &rp,
                ) + pad * 2.0;
            }
            let count = if f.matches.is_empty() {
                if f.query.is_empty() {
                    String::new()
                } else {
                    "NO MATCHES".into()
                }
            } else {
                format!("{} OF {}", f.current + 1, f.matches.len())
            };
            fonts.draw(scene, dim, x, base, &count);
            let hint = if f.with_replace {
                "ENTER REPLACES · CTRL+ENTER ALL · TAB SWITCHES · ESC"
            } else {
                "ENTER NEXT · SHIFT+ENTER BACK · ESC"
            };
            let hw = fonts.measure(dim, hint);
            fonts.draw(scene, dim, r.right() - pad - hw, base, hint);
        }

        // Completion menu under the caret.
        if let Some(c) = &e.completion {
            let b = &e.buffers[bi];
            let row = b.line_of(b.cursor).saturating_sub(b.scroll);
            let col = b.col_of(c.at.min(b.cursor));
            let cx = ox + col as f32 * cw;
            let cy = oy + (row + 1) as f32 * ch;
            let shown = c.items.len().min(8);
            let row_h = ch + px(4.0);
            let wmax = c
                .items
                .iter()
                .skip(c.scroll)
                .take(shown)
                .map(|i| {
                    fonts.measure(mono, &i.label)
                        + i.detail
                            .as_ref()
                            .map(|d| fonts.measure(mono_dim, d) + pad)
                            .unwrap_or(0.0)
                })
                .fold(0.0f32, f32::max)
                .min(r.w * 0.6);
            let bw = wmax + 2.0 * pad;
            let bh = shown as f32 * row_h + px(6.0);
            let bx = cx.min(r.right() - bw - pad).max(r.x);
            let by = if cy + bh > r.bottom() - status_h {
                cy - ch - bh
            } else {
                cy
            };
            let bx_r = Rect::new(bx, by, bw, bh);
            scene.rect(bx_r, paper);
            scene.outline(bx_r, hair, ink);
            let mut yy = by + px(3.0);
            for (k, item) in c.items.iter().enumerate().skip(c.scroll).take(shown) {
                let rr = Rect::new(bx, yy, bw, row_h);
                if k == c.sel {
                    scene.rect(rr, fade(signal, 0.18));
                }
                let base = yy + baseline_off + px(2.0);
                let lw = fonts.draw(
                    scene,
                    mono,
                    bx + pad,
                    base,
                    &fit(fonts, mono, &item.label, bw - 2.0 * pad),
                );
                if let Some(d) = &item.detail {
                    let dd = fit(fonts, mono_dim, d, bw - 3.0 * pad - lw);
                    let dw = fonts.measure(mono_dim, &dd);
                    if lw + dw + 3.0 * pad < bw {
                        fonts.draw(scene, mono_dim, bx + bw - pad - dw, base, &dd);
                    }
                }
                yy += row_h;
            }
        }

        // Hover box near the pointer's word.
        if let Some(h) = &e.hover {
            let b = &e.buffers[bi];
            let line = b.line_of(h.at);
            if line >= b.scroll && line < b.scroll + rows {
                let row = line - b.scroll;
                let col = b.col_of(h.at);
                let lines: Vec<String> = h
                    .text
                    .lines()
                    .take(14)
                    .map(|l| fit(fonts, mono, l, r.w * 0.7))
                    .collect();
                let wmax = lines
                    .iter()
                    .map(|l| fonts.measure(mono, l))
                    .fold(0.0f32, f32::max);
                let bw = wmax + 2.0 * pad;
                let bh = lines.len() as f32 * ch + 2.0 * px(6.0);
                let bx = (ox + col as f32 * cw).min(r.right() - bw - pad).max(r.x);
                let above = oy + row as f32 * ch;
                let by = if above - bh < r.y + strip_h {
                    above + ch
                } else {
                    above - bh
                };
                let hr = Rect::new(bx, by, bw, bh);
                scene.rect(hr, paper);
                scene.outline(hr, hair, ink);
                let mut yy = by + px(6.0) + baseline_off;
                for l in &lines {
                    fonts.draw(scene, mono, bx + pad, yy, l);
                    yy += ch;
                }
            }
        }
        scene.layer(None);
    }
}

/// Index of a buffer by uri across the editor panes of a tab.
pub fn buffer_with<'a>(
    tab: &'a mut crate::app::Tab,
    uri: &Url,
) -> Option<(&'a mut EditorPane, usize)> {
    // Servers spell file URIs their own way (`c%3A` for `C:`, case), so
    // match on the path, not the string.
    let want = uri.to_file_path().ok().map(|p| p.to_string_lossy().to_lowercase());
    for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
        if let Pane::Editor(e) = p {
            if let Some(i) = e.buffers.iter().position(|b| {
                b.uri.as_ref() == Some(uri)
                    || (want.is_some() && b.path.as_ref().map(|p| p.to_string_lossy().to_lowercase()) == want)
            }) {
                return Some((e, i));
            }
        }
    }
    None
}

pub type Diags = HashMap<Url, Vec<Diagnostic>>;
