//! The editor pane: a full editor on ropey — many buffers in a strip,
//! find/replace, undo, tree-sitter colour — with the language server
//! (crates/lsp) for hover, completion, diagnostics, definition and
//! format-on-save. Drawn in the terminal's monospace at the terminal's
//! size, ruled like everything else; nothing hand-rolled where a crate is
//! the real thing (ropey for the text, the lsp crate for the protocol).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use crate::{editor_work as work, work::Task};

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
    /// Revision changes only on edits; painting never hashes the document.
    pub revision: u64,
    pub synced_revision: u64,
    pub lsp_key: Option<String>,
    spans: Option<(usize, usize, work::LineSpans)>,
    highlighting: Option<(usize, usize, Task<work::LineSpans>)>,
    loading: Option<Task<std::io::Result<Rope>>>,
    pub load_error: Option<String>,
    pub scroll_col: usize,
    pub pending_position: Option<nus_lsp::lsp_types::Position>,
    opened_at: Option<Instant>,
    /// The server has this document open.
    pub in_lsp: bool,
    /// A save waits on the formatter's answer (sent at this instant).
    pub save_pending: Option<Instant>,
}

impl Buffer {
    pub fn from_path(path: &Path) -> anyhow::Result<Buffer> {
        let mut b = Buffer::empty();
        b.opened_at = crate::perf::enabled().then(Instant::now);
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let abs = PathBuf::from(abs.to_string_lossy().trim_start_matches(r"\\?\"));
        b.uri = if crate::protected_state::is_private_path(&abs){None}else{Url::from_file_path(&abs).ok()};
        b.language = nus_lsp::registry::language_for(&abs);
        b.path = Some(abs.clone());
        b.loading = Some(Task::start(move |cancel| work::load(&abs, cancel))
            .ok_or_else(|| anyhow::anyhow!("File workers are busy. Try opening the file again."))?);
        Ok(b)
    }

    pub fn ready(&self) -> bool { self.loading.is_none() && self.load_error.is_none() }

    pub fn presented(&mut self) {
        if !self.ready() { return; }
        if let Some(at) = self.opened_at.take() {
            let name = if self.text.len_bytes() >= 100 * 1024 * 1024 { "file_100m_open_submit" }
                else if self.text.len_bytes() >= 10 * 1024 * 1024 { "file_10m_open_submit" } else { "file_open_submit" };
            crate::perf::record(name, at.elapsed().as_secs_f64()*1000.0);
        }
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(job) = &self.loading {
            match job.take() {
                Ok(Ok(text)) => {
                    self.text = text; self.loading = None; self.revision += 1; changed = true;
                    if let Some(pos) = self.pending_position.take() { self.cursor = work::offset(&self.text, pos); self.scroll = (pos.line as usize).saturating_sub(5); }
                }
                Ok(Err(e)) => { self.load_error = Some(e.to_string()); self.loading = None; changed = true; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.load_error = Some("File worker stopped. Close this buffer and open it again.".into());
                    self.loading = None; changed = true;
                }
                Err(_) => {}
            }
        }
        if let Some((start, end, job)) = &self.highlighting {
            match job.take() {
                Ok(spans) => { self.spans = Some((*start, *end, spans)); self.highlighting = None; changed = true; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.highlighting = None; }
                Err(_) => {}
            }
        }
        changed
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
            last_edit: crate::clock::now(),
            diags: Vec::new(),
            spans: None,
            revision: 0,
            synced_revision: 0,
            lsp_key: None,
            highlighting: None,
            loading: None,
            load_error: None,
            scroll_col: 0,
            pending_position: None,
            opened_at: None,
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
        let n = n - usize::from(n > 0 && l.char(n-1) == '\n');
        n - usize::from(n > 0 && l.char(n-1) == '\r')
    }

    pub fn line_text(&self, line: usize) -> String {
        let l = self.text.line(line);
        let s: String = l.chars().collect();
        s.trim_end_matches(['\r', '\n']).to_string()
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
        let now = crate::clock::now();
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
        self.highlighting = None;
        self.revision = self.revision.wrapping_add(1);
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
        if !self.ready() { return; }
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
        if !self.ready() { return; }
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
            let all_space = self.text.line(line).chars().take(col).all(|c| c == ' ') && col > 0;
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
        let lt: String = self.text.line(line).chars().take(8192).collect();
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
        if !self.ready() { return; }
        match self.selection() {
            Some((a, b)) if self.line_of(a) != self.line_of(b.saturating_sub(1)) || out => {
                self.remember(false);
                let (l0, l1) = (self.line_of(a), self.line_of(b.saturating_sub(1).max(a)));
                for l in (l0..=l1).rev() {
                    let start = self.text.line_to_char(l);
                    if out {
                        let n = self.text.line(l).chars().take(4).take_while(|c| *c == ' ').count();
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
                let n = self.text.line(l).chars().take(4).take_while(|c| *c == ' ').count();
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
        if !self.ready() { return; }
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

    fn prepare_spans(&mut self, first: usize, rows: usize, columns: usize) {
        if !self.ready() || self.language == "plaintext" { return; }
        let line = first.min(self.text.len_lines()-1);
        let start = self.text.line_to_char(line) + self.scroll_col.min(self.line_len(line));
        let last = (line + rows).min(self.text.len_lines()-1);
        let end = (self.text.line_to_char(last) + self.line_len(last).min(self.scroll_col + columns)).min(self.len_chars());
        // Small files keep full grammar context. Large files color a bounded
        // window with preceding context; neither copying nor parsing runs on
        // the event thread, and an edit/scroll cancels superseded work.
        let (a, z) = if self.text.len_bytes() <= 512 * 1024 {
            (0, self.len_chars())
        } else {
            let a = self.text.line_to_char(line.saturating_sub(32)).max(start.saturating_sub(4096));
            (a, (a + 16384).min(self.len_chars()))
        };
        let covers = |a: usize, z: usize| a <= start && z >= end.min(start + columns);
        if self.spans.as_ref().is_some_and(|(a,z,_)| covers(*a,*z))
            || self.highlighting.as_ref().is_some_and(|(a,z,_)| covers(*a,*z)) { return; }
        self.highlighting = None;
        let text = self.text.clone();
        let grammar = grammar_for(self.language);
        if let Some(job) = Task::start(move |cancel| work::highlight(&text, grammar, a, z, cancel)) {
            self.highlighting = Some((a,z,job));
        }
    }

    fn spans_for(&self, line: usize) -> &[(usize, usize, crate::predict::Tok)] {
        self.spans.as_ref().and_then(|(_,_,p)| p.get(&line)).map(Vec::as_slice).unwrap_or(&[])
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
    pub truncated: bool,
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
    pub zoom: u32,
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
    search: Option<(usize, u64, String, Task<work::Matches>)>,
    search_needed: bool,
    /// While the buffer is a note: the rails a whole tab shows (notes_ui.rs).
    pub notes: Option<crate::notes_ui::Rails>,
}

impl EditorPane {
    pub fn new(rect: Rect) -> EditorPane {
        EditorPane {
            zoom: 100,
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
            search: None,
            search_needed: false,
            notes: None,
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
        let col = b.scroll_col + ((x - self.origin.0) / cw + 0.5).floor().max(0.0) as usize;
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
        f.truncated = false;
        self.search = None;
        self.search_needed = false;
        if f.query.is_empty() {
            return;
        }
        self.search = None;
        self.search_needed = true;
        let text = b.text.clone();
        let query = f.query.clone();
        let q = query.clone();
        if let Some(job) = Task::start(move |cancel| work::search(&text, &q, cancel)) {
            self.search = Some((self.active, b.revision, query, job));
            self.search_needed = false;
        }
    }

    fn poll_search(&mut self) -> bool {
        if self.search_needed { self.refind(); }
        let Some((active, revision, query, job)) = &self.search else { return false };
        if *active != self.active || self.buf().is_none_or(|b| b.revision != *revision)
            || self.find.as_ref().is_none_or(|f| f.query != *query) {
            self.search = None;
            if self.find.is_some() { self.refind(); }
            return true;
        }
        match job.take() {
            Ok(matches) => {
                let cur = self.buf().map(|b| b.cursor).unwrap_or(0);
                if let Some(f) = &mut self.find {
                    f.matches = matches.ranges;
                    f.truncated = matches.truncated;
                    f.current = f.matches.iter().position(|&(a,_)| a >= cur).unwrap_or(0);
                }
                self.search = None;
                return true;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.search = None; self.search_needed = true; }
            Err(_) => {}
        }
        false
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
        if f.matches.is_empty() { return; }
        if f.truncated {
            self.notice = Some(("More than 10,000 matches. Narrow the query before replacing all.".into(), crate::clock::now()));
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

/// Text, as far as a look at the first bytes can tell: no NULs, and
/// mostly printable. File size is not a reason to reject a text file.
pub(crate) fn looks_text(path: &Path) -> bool {
    if crate::protected_state::is_private_path(path){return true;}
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut f, &mut buf).unwrap_or(0);
    if n == 0 {
        return true;
    }
    let head = &buf[..n];
    if head.contains(&0) {
        return false;
    }
    let odd = head.iter().filter(|&&b| b < 0x09 || (b > 0x0d && b < 0x20)).count();
    odd * 20 < n
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
        if crate::private::enabled() { self.notice(nus_render::text::icons::EYE_SLASH, "Not In Incognito", "open local files in a regular nus window"); return; }
        if !path.is_file() {
            self.notice(nus_render::text::icons::PENCIL, "Not A File", path.display().to_string());
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
                        self.notice_problem("Could Not Open File", err.to_string());
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
                        self.notice_problem("Could Not Open File", err.to_string());
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
                        self.notice_problem("Could Not Open File", err.to_string());
                        None
                    }
                }
            }
        };
        if idx.is_some() {
            self.files_root_from(path);
            self.apply_term_resizes(false);
        }
        self.dirty = true;
    }

    /// Keys for the focused editor. Returns true when consumed.
    pub(crate) fn editor_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        let pressed = ev.state == ElementState::Pressed;
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        let sup = self.mods.super_key();
        let mods = self.mods;
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
            if e.buf().is_some_and(|b| !b.ready()) { return true; }
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
                // The field's own editing: typing, erasing, paste (field.rs).
                let in_replace = f.in_replace;
                let line = if in_replace { &mut f.replace } else { &mut f.query };
                let took = crate::field::edit(line, ev, mods, 400);
                if took.changed() && !in_replace {
                    e.refind();
                    e.find_select();
                }
                let f = e.find.as_mut().unwrap();
                match &key {
                    _ if took.taken() => {}
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
                    WKey::Named(NamedKey::ArrowDown) => e.find_step(true),
                    WKey::Named(NamedKey::ArrowUp) => e.find_step(false),
                    _ => {}
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
                                truncated: false,
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
                                truncated: false,
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
        if pressed && button == MouseButton::Left && self.note_rails_mouse(x, y) {
            return true;
        }
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
                        let now = crate::clock::now();
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
                    e.rest = Some(((x, y), crate::clock::now()));
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
        let mut loaded = Vec::new();
        for (ti, tab) in self.tabs.iter_mut().enumerate() {
            for (right, p) in
                std::iter::once((false, &mut tab.left)).chain(tab.right.as_mut().map(|r| (true, r)))
            {
                let Pane::Editor(e) = p else { continue };
                for (bi, b) in e.buffers.iter_mut().enumerate() {
                    let was_loading = b.loading.is_some();
                    expired |= b.poll();
                    if was_loading && b.ready() { loaded.push((ti, right, bi)); }
                }
                expired |= e.poll_search();
                if let Some(((x, y), at)) = e.rest {
                    if crate::clock::since(at).as_millis() > 450 && e.hover.is_none() {
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
                    .is_some_and(|(_, at)| crate::clock::since(at).as_secs() > 4)
                {
                    e.notice = None;
                    expired = true;
                }
                if let Some(b) = e.buf_mut() {
                    if b.save_pending.is_some_and(|at| crate::clock::since(at).as_secs() >= 3) {
                        b.save_pending = None;
                        expired = true;
                    }
                }
            }
        }
        for (ti, right, bi) in loaded { self.lsp_open_buffer(ti, right, bi); }
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
        // A note as wide as a tab keeps its index and references beside
        // the text; the pane is still the whole rect.
        let outer = r;
        let r = self.draw_note_rails(scene, e, r);
        let place = App::note_place_word(e);
        let t = self.theme.clone();
        let (ink, paper) = (t.ink, t.paper);
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style {
            color: t.dim,
            ..label
        };
        let term_px = self.behavior.typography.editor_size * self.scale * 96.0 / 72.0 * e.zoom as f32 / 100.0;
        let mono = Style {
            font: self.f.editor,
            px: term_px,
            color: ink,
            tracking: 0.0,
        };
        let mono_dim = Style {
            color: t.dim,
            ..mono
        };
        let cw = self.fonts.measure(mono, "M").max(1.0);
        let metrics = self.fonts.metrics(self.f.editor, term_px);
        let ch = metrics.line_height.max(term_px * self.behavior.typography.editor_line);
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        let signal = self.surface.signal;
        let caret = self.caret_color();
        let (mx, my) = self.mouse;
        let strip_h = self.header_h();
        let status_h = self.px(m::PANE_FOOTER);
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
        e.rect = outer;
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
        // A note says where it lives: plain in the folder, or sealed.
        if let Some(word) = place {
            let ww = fonts.measure(dim, word);
            if x + ww + pad * 2.0 < r.right() {
                fonts.draw(scene, dim, r.right() - pad - ww, strip_base, word);
            }
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
        let columns = ((r.right() - ox) / cw).ceil().max(1.0) as usize;
        let caret_col = b.col_of(b.cursor);
        if caret_col < b.scroll_col { b.scroll_col = caret_col; }
        else if caret_col >= b.scroll_col + columns { b.scroll_col = caret_col + 1 - columns; }
        b.prepare_spans(b.scroll, rows, columns);
        let first_visible = b.text.line_to_char(b.scroll);
        let last_visible = b.text.line_to_char((b.scroll + rows + 1).min(b.text.len_lines()));
        let scroll_col = b.scroll_col;
        let cur_line = b.line_of(b.cursor);
        let sel = b.selection();
        let n_lines = b.text.len_lines();
        let find_matches: Vec<(usize, usize)> = e
            .find
            .as_ref()
            .map(|f| {
                let start = f.matches.partition_point(|&(_,z)| z <= first_visible);
                f.matches[start..].iter().take_while(|&&(a,_)| a < last_visible).copied().collect()
            })
            .unwrap_or_default();
        let find_cur = e
            .find
            .as_ref()
            .and_then(|f| f.matches.get(f.current).copied());
        let diags: Vec<(usize, usize, usize, usize, usize)> = {
            b.diags
                .iter()
                .filter(|d| (d.range.end.line as usize) >= b.scroll && (d.range.start.line as usize) < b.scroll + rows)
                .map(|d| {
                    let a = work::offset(&b.text, d.range.start);
                    let z = work::offset(&b.text, d.range.end).max(a + 1);
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
        // Against HEAD: added, changed, removed (git_gutter.rs).
        let gmarks = b.path.as_deref().and_then(|p| crate::git_gutter::for_buffer(p, b.revision, &b.text));
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
            let visible_start = ls + scroll_col.min(len);
            let visible_end = ls + len.min(scroll_col + columns);
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
            // Blame, faint, after the caret's line: saved files in a repository only.
            if line == cur_line && focused && !b.dirty && gmarks.is_some() {
                if let Some(words) = b.path.as_deref().and_then(|p| crate::git_gutter::blame_line(p, line)) {
                    let end = len.saturating_sub(scroll_col) as f32;
                    let bx = ox + (end + 3.0) * cw;
                    if bx < r.right() - cw * 12.0 {
                        let faint = Style { color: crate::surface::mix(paper, ink, 0.4), ..mono_dim };
                        let fit: String = words.chars().take(((r.right() - bx) / cw) as usize).collect();
                        fonts.draw(scene, faint, bx, base, &fit);
                    }
                }
            }
            if let Some(mk) = gmarks.as_ref().and_then(|g| g.get(line).copied().flatten()) {
                let bar = px(3.0);
                match mk {
                    crate::git_gutter::Mark::Added => scene.rect(Rect::new(r.x + px(2.0), ly, bar, ch), ansi(2)),
                    crate::git_gutter::Mark::Changed => scene.rect(Rect::new(r.x + px(2.0), ly, bar, ch), ansi(4)),
                    // A notch at the top of the line: lines went here.
                    crate::git_gutter::Mark::Removed => scene.rect(Rect::new(r.x + px(2.0), ly - px(1.5), cw * 0.9, px(3.0)), ansi(1)),
                }
            }
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
                let (s0, s1) = (a.max(visible_start), z.min(visible_end + 1));
                if s1 > s0 {
                    let w = if z > ls + len {
                        (s1 - s0) as f32 * cw + cw * 0.4
                    } else {
                        (s1 - s0) as f32 * cw
                    };
                    scene.rect(Rect::new(ox + (s0 - ls - scroll_col.min(len)) as f32 * cw, ly, w, ch), sel_color);
                }
            }
            for &(a, z) in &find_matches {
                let (s0, s1) = (a.max(visible_start), z.min(visible_end));
                if s1 > s0 {
                    let c = if Some((a, z)) == find_cur {
                        fade(ansi(3), 0.5)
                    } else {
                        match_color
                    };
                    scene.rect(
                        Rect::new(ox + (s0 - ls - scroll_col.min(len)) as f32 * cw, ly, (s1 - s0) as f32 * cw, ch),
                        c,
                    );
                }
            }
            // The text, as coloured runs.
            let spans = b.spans_for(line);
            let chars: Vec<char> = b.text.slice(visible_start..visible_end).chars().collect();
            let mut col = scroll_col;
            let mut draw_run = |scene: &mut Scene,
                                fonts: &mut nus_render::text::FontSystem,
                                from: usize,
                                to: usize,
                                color: nus_render::Color| {
                let from = from.saturating_sub(scroll_col);
                let to = to.saturating_sub(scroll_col);
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
            for &(a, l, class) in spans {
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
            draw_run(scene, fonts, col, scroll_col + chars.len(), ink);
            // Diagnostics: a dotted underline.
            for &(a, z, sev, _, _) in &diags {
                let (s0, s1) = (a.max(visible_start), z.min(visible_end.max(visible_start + 1)));
                if s1 > s0 {
                    let uy = ly + ch - px(2.5);
                    let mut ux = ox + (s0 - ls - scroll_col.min(len)) as f32 * cw;
                    let end = ox + (s1 - ls - scroll_col.min(len)) as f32 * cw;
                    while ux < end {
                        scene.rect(Rect::new(ux, uy, px(2.0), px(1.5)), ansi(sev));
                        ux += px(4.0);
                    }
                }
            }
            // The caret.
            if line == cur_line && focused && e.goto.is_none() && e.find.is_none() {
                let cx = ox + b.col_of(b.cursor).saturating_sub(scroll_col) as f32 * cw;
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
                if b.loading.is_some() { s.push_str(" · loading…"); }
                else if let Some(error) = &b.load_error { s.push_str(&format!(" · {error}")); }
                else if b.text.len_bytes() > 8 * 1024 * 1024 { s.push_str(" · large file · language server paused"); }
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
            let count = if e.search.is_some() || e.search_needed {
                "SEARCHING…".into()
            } else if f.matches.is_empty() {
                if f.query.is_empty() {
                    String::new()
                } else {
                    "NO MATCHES".into()
                }
            } else {
                format!("{} OF {}{}", f.current + 1, f.matches.len(), if f.truncated { "+" } else { "" })
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
    // Decode URI spelling, but preserve filename case on case-sensitive
    // volumes. Canonical paths reconcile aliases of existing files.
    let want = uri.to_file_path().ok();
    for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
        if let Pane::Editor(e) = p {
            if let Some(i) = e.buffers.iter().position(|b| {
                b.uri.as_ref() == Some(uri)
                    || want.as_ref().zip(b.path.as_ref()).is_some_and(|(a,b)| same_file_path(a,b))
            }) {
                return Some((e, i));
            }
        }
    }
    None
}

fn same_file_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

pub type Diags = HashMap<Url, Vec<Diagnostic>>;

#[cfg(test)]
mod performance_tests {
    use super::*;
    #[test]
    fn language_server_paths_do_not_merge_case_distinct_files() {
        let dir = tempfile::tempdir().unwrap();
        let upper = dir.path().join("Module.rs");
        let lower = dir.path().join("module.rs");
        assert!(!same_file_path(&upper, &lower));
        std::fs::write(&upper, "upper").unwrap();
        std::fs::write(&lower, "lower").unwrap();
        // On a case-sensitive filesystem these are different files. On a
        // case-insensitive filesystem both names intentionally refer to one.
        if std::fs::read_to_string(&upper).unwrap() == "upper" {
            assert!(!same_file_path(&upper, &lower));
        }
        assert!(same_file_path(&upper, &dir.path().join("./Module.rs")));
    }
    fn settle(b: &mut Buffer) {
        let until = Instant::now() + std::time::Duration::from_secs(5);
        while b.loading.is_some() || b.highlighting.is_some() {
            b.poll(); assert!(Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn loading_does_not_allow_partial_edits_and_keeps_definition_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.rs");
        std::fs::write(&path, "// α\r\nlet x = 1;\r\n").unwrap();
        let mut b = Buffer::from_path(&path).unwrap();
        assert!(!b.ready());
        b.insert("must not overwrite", false);
        b.pending_position = Some(nus_lsp::lsp_types::Position::new(1, 4));
        settle(&mut b);
        assert_eq!(b.text.to_string(), "// α\r\nlet x = 1;\r\n");
        assert_eq!(b.cursor, b.text.line_to_char(1)+4);
        assert!(!b.dirty);
        assert_eq!(b.line_len(0), 4);
    }
    #[test]
    fn large_file_highlight_window_is_bounded_and_edits_cancel_old_spans() {
        let mut b = Buffer::empty();
        b.text = Rope::from_str(&"let value = 123;\n".repeat(40_000));
        b.language = "rust";
        b.scroll = 30_000;
        b.prepare_spans(b.scroll, 50, 100);
        let (a,z,_) = b.highlighting.as_ref().unwrap();
        assert!(z-a <= 16384);
        settle(&mut b);
        assert!(!b.spans_for(30_000).is_empty());
        let revision = b.revision;
        for _ in 0..100 { b.spans_for(30_000); }
        assert_eq!(b.revision, revision);
        b.insert("x", false);
        assert!(b.spans.is_none()); assert!(b.highlighting.is_none());
        assert_eq!(b.revision, revision+1);
    }
}
