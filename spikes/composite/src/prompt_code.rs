//! Completion belongs to the visible prompt. One snapshot supplies the
//! ghost, its acceptance and the Code menu, including edits inside a word.
use std::ops::Range;

use nus_render::{text::Style, Rect, Scene};
use nus_vt::{
    input::{self, Key, KeyAction, Mods},
    Flags, MarkKind, Modes, Term,
};
use unicode_width::UnicodeWidthChar;
use winit::{
    event::ElementState,
    keyboard::{Key as WKey, NamedKey},
};

use crate::app::{fade, App, Pane, TermPane};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptStamp {
    pub text: String,
    /// UTF-8 byte offset, always a character boundary.
    pub caret: usize,
    pub start: (u64, usize),
}

#[derive(Clone, Debug)]
pub struct PromptCell {
    pub byte: usize,
    pub row: usize,
    pub col: usize,
    pub width: usize,
}

#[derive(Clone, Debug)]
pub struct PromptLine {
    pub text: String,
    pub caret: usize,
    pub cells: Vec<PromptCell>,
    pub start: (u64, usize),
    pub end: (usize, usize),
}

impl PromptLine {
    /// Read a command's entire soft-wrapped extent, not merely the cells
    /// before the caret. Unmarked/alternate/explicit continuation screens
    /// remain owned by the shell.
    pub fn read(term: &Term) -> Option<Self> {
        if term.modes().contains(Modes::ALT_SCREEN) || term.grid().display_offset != 0 {
            return None;
        }
        let mark = term
            .marks
            .last()
            .filter(|m| m.kind == MarkKind::CommandStart)?;
        let grid = term.grid();
        let top = grid.abs_row(0);
        let first = usize::try_from(mark.line.checked_sub(top)?).ok()?;
        let cur = term.cursor();
        if first > cur.row {
            return None;
        }
        let caret_cell = (cur.row, if cur.wrap_next { grid.cols() } else { cur.col });
        let mut text = String::new();
        let mut cells = Vec::new();
        let mut caret = None;
        let mut last_ink = 0;
        let mut end = (first, mark.col);
        let mut row_no = first;
        loop {
            if row_no >= grid.rows() {
                return None;
            }
            let row = grid.row(row_no);
            // VT leaves an unmarked padding cell when a double-width glyph
            // wraps early. Its blank is indistinguishable from a real space.
            if row.wrapped
                && row
                    .cells
                    .last()
                    .is_some_and(|c| c.ch == ' ' && !c.flags.contains(Flags::WIDE_SPACER))
                && row_no + 1 < grid.rows()
                && grid.cell(row_no + 1, 0).flags.contains(Flags::WIDE)
            {
                return None;
            }
            let from = if row_no == first { mark.col } else { 0 };
            for (col, cell) in row.cells.iter().enumerate().skip(from) {
                if (row_no, col) == caret_cell {
                    caret = Some(text.len());
                }
                if cell.flags.contains(Flags::WIDE_SPACER) {
                    continue;
                }
                let width = if cell.flags.contains(Flags::WIDE) {
                    2
                } else {
                    1
                };
                cells.push(PromptCell {
                    byte: text.len(),
                    row: row_no,
                    col,
                    width,
                });
                text.push(if cell.ch == '\0' { ' ' } else { cell.ch });
                if cell.ch != ' ' && cell.ch != '\0' {
                    last_ink = text.len();
                }
            }
            if caret_cell == (row_no, grid.cols()) {
                caret = Some(text.len());
            }
            if !row.wrapped {
                if row_no < cur.row {
                    return None;
                }
                break;
            }
            row_no += 1;
        }
        let caret = caret?;
        text.truncate(last_ink.max(caret));
        cells.retain(|c| c.byte < text.len());
        // A screen can also contain a right prompt or the shell's faint
        // autosuggestion. Decline ambiguous suffixes instead of editing it.
        if text[caret..].contains("   ")
            || cells.iter().filter(|c| c.byte >= caret).any(|c| {
                let cell = grid.cell(c.row, c.col);
                cell.ch != ' '
                    && (cell.flags.intersects(Flags::DIM | Flags::HIDDEN)
                        || cell.fg == nus_vt::Color::Indexed(8))
            })
        {
            return None;
        }
        if let Some(c) = cells.last() {
            end = (c.row, c.col + c.width);
        }
        Some(Self {
            text,
            caret,
            cells,
            start: (mark.line, mark.col),
            end,
        })
    }

    pub fn stamp(&self) -> PromptStamp {
        PromptStamp {
            text: self.text.clone(),
            caret: self.caret,
            start: self.start,
        }
    }
    pub fn prefix(&self) -> &str {
        &self.text[..self.caret]
    }
    pub fn at_end(&self) -> bool {
        self.caret == self.text.len()
    }
    pub fn token_range(&self) -> Range<usize> {
        token_at(&self.text, self.caret)
    }
    pub fn cell_at(&self, byte: usize) -> (usize, usize) {
        self.cells
            .iter()
            .find(|c| c.byte >= byte)
            .map(|c| (c.row, c.col))
            .unwrap_or(self.end)
    }
    pub fn matches(&self, stamp: &PromptStamp) -> bool {
        self.text == stamp.text && self.caret == stamp.caret && self.start == stamp.start
    }

    /// Relative character edits retain the rest of the command and respect
    /// application-cursor/Kitty input modes. Never include a submission key.
    pub fn replace_bytes(&self, term: &Term, range: Range<usize>, insert: &str) -> Vec<u8> {
        if range.start > range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
            || !safe_text(insert)
        {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut key = |k, n| {
            let bytes = input::encode(
                k,
                Mods::empty(),
                KeyAction::Press,
                term.modes(),
                term.keyboard_mode(),
            );
            for _ in 0..n {
                out.extend_from_slice(&bytes);
            }
        };
        if range.start < self.caret {
            key(
                Key::Left,
                self.text[range.start..self.caret].chars().count(),
            );
        } else {
            key(
                Key::Right,
                self.text[self.caret..range.start].chars().count(),
            );
        }
        key(Key::Delete, self.text[range.clone()].chars().count());
        out.extend_from_slice(insert.as_bytes());
        out
    }
}

/// Shell word extents in UTF-8 bytes. Quotes and backslash escapes keep
/// whitespace inside a word; operators end one even without spaces.
pub fn token_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (i, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            start.get_or_insert(i);
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            start.get_or_insert(i);
            quote = Some(ch);
        } else if ch.is_whitespace() || "|&;<>()".contains(ch) {
            if let Some(a) = start.take() {
                out.push(a..i);
            }
            if !ch.is_whitespace() {
                out.push(i..i + ch.len_utf8());
            }
        } else {
            start.get_or_insert(i);
        }
    }
    if let Some(a) = start {
        out.push(a..text.len());
    }
    out
}

pub fn token_at(text: &str, caret: usize) -> Range<usize> {
    token_ranges(text)
        .into_iter()
        .find(|r| r.start < caret && caret <= r.end)
        .or_else(|| token_ranges(text).into_iter().find(|r| r.start == caret))
        .unwrap_or(caret..caret)
}

pub fn next_token_end(text: &str, caret: usize) -> usize {
    token_ranges(text)
        .into_iter()
        .find(|r| r.end > caret)
        .map(|r| r.end)
        .unwrap_or(text.len())
}

/// VT currently stores codepoints, so leave combining sequences to the
/// native editor instead of making an edit whose echo cannot be verified.
pub fn safe_text(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || c.width() == Some(0))
}

#[derive(Clone, Debug)]
pub struct CodeItem {
    pub label: String,
    pub detail: String,
    pub range: Range<usize>,
    pub insert: String,
}

#[derive(Clone, Debug)]
pub struct CodeMenu {
    pub stamp: PromptStamp,
    pub items: Vec<CodeItem>,
    pub selected: usize,
    hits: std::cell::RefCell<Vec<(Rect, usize)>>,
}

impl CodeMenu {
    pub fn new(stamp: PromptStamp, items: Vec<CodeItem>) -> Self {
        Self {
            stamp,
            items,
            selected: 0,
            hits: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PromptEdit {
    pub before: PromptStamp,
    pub expected: PromptStamp,
}

impl PromptEdit {
    pub fn new(line: &PromptLine, range: Range<usize>, insert: &str) -> Self {
        let mut expected = line.stamp();
        expected.text.replace_range(range.clone(), insert);
        expected.caret = range.start + insert.len();
        Self {
            before: line.stamp(),
            expected,
        }
    }
    pub fn waiting(&self, line: &PromptLine) -> bool {
        // A shell may paint its own suggestion or right prompt after the
        // caret. Acknowledge the inserted prefix without treating that
        // independently rendered suffix as part of our edit transaction.
        line.start == self.before.start
            && (line.caret != self.expected.caret
                || line.prefix() != &self.expected.text[..self.expected.caret])
    }
}

fn menu_window(
    total: usize,
    selected: usize,
    height: f32,
    row_height: f32,
    footer: f32,
) -> Range<usize> {
    let capacity = ((height - footer).max(0.0) / row_height.max(1.0)).floor() as usize;
    let shown = total.min(capacity.clamp(1, 8));
    let first = selected.saturating_sub(shown.saturating_sub(1));
    first..(first + shown).min(total)
}

impl TermPane {
    pub fn prompt_line(&self) -> Option<PromptLine> {
        if !safe_text(&self.line) {
            return None;
        }
        PromptLine::read(&self.term)
    }

    pub fn code_items(&self, line: &PromptLine) -> Vec<CodeItem> {
        let mut items = Vec::new();
        if line.at_end() && !line.text.trim().is_empty() {
            for command in self.prompt_history_entries() {
                if command.starts_with(&line.text)
                    && command != line.text
                    && safe_text(&command)
                    && !items.iter().any(|it: &CodeItem| it.insert == command)
                {
                    items.push(CodeItem {
                        label: command.clone(),
                        detail: "History".into(),
                        range: 0..line.text.len(),
                        insert: command,
                    });
                    if items.len() >= 12 {
                        break;
                    }
                }
            }
        }
        let range = line.token_range();
        for insert in crate::git_complete::completions(line.prefix(), self.cwd.as_deref()) {
            if !items
                .iter()
                .any(|it| it.insert == insert && it.range == range)
            {
                items.push(CodeItem {
                    label: insert.clone(),
                    detail: "Git".into(),
                    range: range.clone(),
                    insert,
                });
            }
        }
        if let Some(lsp) = &self.plsp {
            if lsp.snapshot.as_ref().is_some_and(|s| line.matches(s))
                && !crate::git_complete::is_git(line.prefix())
            {
                items.extend(
                    lsp.items
                        .iter()
                        .filter_map(|item| crate::prompt_lsp::code_item(line, item)),
                );
            }
        }
        items.truncate(40);
        items
    }

    pub fn prompt_history_entries(&self) -> impl Iterator<Item = String> + '_ {
        self.blocks()
            .into_iter()
            .rev()
            .filter(|block| !block.running)
            .filter_map(|block| crate::prompt_history::completed_command(&self.term, block.start))
            .chain(self.history.iter().rev().cloned())
    }
}

impl App {
    pub(crate) fn prompt_assist_tick(&mut self) {
        for tab in &mut self.tabs {
            for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(p) = pane else { continue };
                let line = p.prompt_line();
                if p.prompt_edit_pending
                    .as_ref()
                    .is_some_and(|e| line.as_ref().is_none_or(|l| !e.waiting(l)))
                {
                    p.prompt_edit_pending = None;
                }
                if p.code_menu
                    .as_ref()
                    .is_some_and(|m| line.as_ref().is_none_or(|l| !l.matches(&m.stamp)))
                {
                    p.code_menu = None;
                    self.dirty = true;
                }
                if p.prompt_quiet
                    .as_ref()
                    .is_some_and(|s| line.as_ref().is_none_or(|l| !l.matches(s)))
                {
                    p.prompt_quiet = None;
                }
            }
        }
    }

    /// A menu click inserts, just like Tab. No mouse path submits a command.
    pub(crate) fn prompt_code_click(&mut self, x: f32, y: f32) -> bool {
        if self.prompt_composing {
            return false;
        }
        let Some(Pane::Term(p)) = self.tabs.get_mut(self.active).map(|t| t.focused()) else {
            return false;
        };
        if p.confirm_paste.is_some() || p.confirm_close.is_some() || p.prompt_history.active() {
            return false;
        }
        let Some(menu) = p.code_menu.take() else {
            return false;
        };
        let Some(line) = p.prompt_line().filter(|l| l.matches(&menu.stamp)) else {
            return false;
        };
        let selected = menu
            .hits
            .borrow()
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, i)| *i);
        self.dirty = true;
        let Some(item) = selected.and_then(|i| menu.items.get(i)) else {
            return false;
        };
        let bytes = line.replace_bytes(&p.term, item.range.clone(), &item.insert);
        if bytes.is_empty() {
            return true;
        }
        let _ = p.pty.write(&bytes);
        p.prompt_edit_pending = Some(PromptEdit::new(&line, item.range.clone(), &item.insert));
        p.prompt_quiet = Some(line.stamp());
        p.line_ok = false;
        p.line_col = None;
        true
    }

    /// The same candidate is used for paint and acceptance. It is never
    /// offered inside a command, under an open menu, or over history recall.
    pub(crate) fn prompt_prediction(&self, p: &TermPane, line: &PromptLine) -> Option<String> {
        if self.prompt_composing
            || !self.behavior.predict
            || !line.at_end()
            || line.text.trim().is_empty()
            || p.code_menu.is_some()
            || p.prompt_history.active()
            || p.prompt_edit_pending.is_some()
            || p.prompt_quiet.as_ref().is_some_and(|s| line.matches(s))
        {
            return None;
        }
        p.prediction()
            .or_else(|| crate::git_complete::ghost(line.prefix(), p.cwd.as_deref()))
            .or_else(|| {
                if self.behavior.prompt_lsp == crate::settings::PromptLsp::Off
                    || crate::git_complete::is_git(line.prefix())
                {
                    return None;
                }
                p.plsp
                    .as_ref()
                    .filter(|l| l.snapshot.as_ref().is_some_and(|s| line.matches(s)))?
                    .items
                    .iter()
                    .filter_map(|it| crate::prompt_lsp::code_item(line, it))
                    .find_map(|it| {
                        it.insert
                            .strip_prefix(&line.text[it.range])
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    })
            })
            .filter(|s| safe_text(s))
    }

    pub(crate) fn prompt_code_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if ev.state != ElementState::Pressed || !self.behavior.shell_integration {
            return false;
        }
        let ctrl = self.mods.control_key();
        let alt = self.mods.alt_key();
        let shift = self.mods.shift_key();
        let sup = self.mods.super_key();
        let Some(Pane::Term(p)) = self.tabs.get(self.active).map(|t| t.focused_ref()) else {
            return false;
        };
        if p.prompt_history.active() || p.confirm_paste.is_some() || p.confirm_close.is_some() {
            return false;
        }
        let Some(line) = p.prompt_line() else {
            if let Some(Pane::Term(p)) = self.tabs.get_mut(self.active).map(|t| t.focused()) {
                p.code_menu = None;
            }
            return false;
        };
        let prediction = self.prompt_prediction(p, &line);
        let Pane::Term(p) = self.tabs[self.active].focused() else {
            return false;
        };
        if let Some(pending) = p.prompt_edit_pending.as_ref() {
            if pending.waiting(&line)
                && (matches!(
                    ev.logical_key,
                    WKey::Named(
                        NamedKey::Enter
                            | NamedKey::Tab
                            | NamedKey::ArrowRight
                            | NamedKey::ArrowUp
                            | NamedKey::ArrowDown
                    )
                ) || (ctrl
                    && matches!(&ev.logical_key, WKey::Character(s) if s.eq_ignore_ascii_case("f"))))
            {
                return true;
            }
            p.prompt_edit_pending = None;
        }
        if p.code_menu
            .as_ref()
            .is_some_and(|m| !line.matches(&m.stamp))
        {
            p.code_menu = None;
        }
        // Shift selection and native app chords are never completion keys.
        if shift || sup {
            p.code_menu = None;
            if shift && !ctrl && !alt && !sup && ev.logical_key == WKey::Named(NamedKey::Enter) {
                // Readline/zle accept a pasted newline as editing. Without
                // bracketed paste, defer to the encoded shifted key.
                if p.term.modes().contains(Modes::BRACKETED_PASTE) {
                    let _ = p.pty.write(b"\x1b[200~\n\x1b[201~");
                } else {
                    // A legacy terminal has no distinct shifted Enter. Do
                    // not turn an editing gesture into command execution.
                    // A negotiated Kitty mode can represent it faithfully.
                    if !p.term.keyboard_mode().is_empty() {
                        let bytes = input::encode(
                            Key::Enter,
                            Mods::SHIFT,
                            KeyAction::Press,
                            p.term.modes(),
                            p.term.keyboard_mode(),
                        );
                        if bytes != b"\r" {
                            let _ = p.pty.write(&bytes);
                        }
                    }
                }
                p.line_ok = false;
                p.line_col = None;
                p.prompt_quiet = Some(line.stamp());
                self.dirty = true;
                return true;
            }
            return false;
        }
        if !ctrl && !alt {
            if let Some(menu) = p.code_menu.as_mut() {
                match ev.logical_key {
                    WKey::Named(NamedKey::ArrowDown) => {
                        menu.selected = (menu.selected + 1).min(menu.items.len().saturating_sub(1));
                        self.dirty = true;
                        return true;
                    }
                    WKey::Named(NamedKey::ArrowUp) => {
                        menu.selected = menu.selected.saturating_sub(1);
                        self.dirty = true;
                        return true;
                    }
                    WKey::Named(NamedKey::Tab | NamedKey::Enter) => {
                        if let Some(item) = menu.items.get(menu.selected) {
                            let bytes =
                                line.replace_bytes(&p.term, item.range.clone(), &item.insert);
                            if bytes.is_empty() {
                                p.code_menu = None;
                                self.dirty = true;
                                return true;
                            }
                            let _ = p.pty.write(&bytes);
                            p.prompt_edit_pending =
                                Some(PromptEdit::new(&line, item.range.clone(), &item.insert));
                            p.line_ok = false;
                            p.line_col = None;
                        }
                        p.code_menu = None;
                        p.prompt_quiet = Some(line.stamp());
                        self.dirty = true;
                        return true;
                    }
                    WKey::Named(NamedKey::Escape) => {
                        p.code_menu = None;
                        p.prompt_quiet = Some(line.stamp());
                        self.dirty = true;
                        return true;
                    }
                    _ => {
                        p.code_menu = None;
                        self.dirty = true;
                    }
                }
            }
            if ev.logical_key == WKey::Named(NamedKey::Tab) {
                let items = p.code_items(&line);
                if !items.is_empty() {
                    p.code_menu = Some(CodeMenu::new(line.stamp(), items));
                    self.dirty = true;
                    return true;
                }
            }
            if ev.logical_key == WKey::Named(NamedKey::Escape) && prediction.is_some() {
                p.prompt_quiet = Some(line.stamp());
                self.dirty = true;
                return true;
            }
        } else if p.code_menu.take().is_some() {
            self.dirty = true;
        }
        let whole = (!ctrl && !alt && ev.logical_key == WKey::Named(NamedKey::ArrowRight))
            || (ctrl
                && !alt
                && (ev.physical_key
                    == winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyF)
                    || matches!(&ev.logical_key, WKey::Character(s) if s.eq_ignore_ascii_case("f"))));
        let token = alt && !ctrl && ev.logical_key == WKey::Named(NamedKey::ArrowRight);
        if whole || token {
            if let Some(mut rest) = prediction {
                if token {
                    let full = format!("{}{rest}", line.text);
                    rest.truncate(next_token_end(&full, line.caret) - line.caret);
                }
                let _ = p.pty.write(rest.as_bytes());
                p.prompt_edit_pending = Some(PromptEdit::new(&line, line.caret..line.caret, &rest));
                p.line_ok = false;
                p.line_col = None;
                p.prompt_quiet = Some(line.stamp());
                self.dirty = true;
                return true;
            }
        }
        false
    }

    pub(crate) fn draw_prompt_code(&mut self, scene: &mut Scene, p: &TermPane, line: &PromptLine) {
        let Some(menu) = p.code_menu.as_ref().filter(|m| line.matches(&m.stamp)) else {
            return;
        };
        if self.prompt_composing || p.prompt_history.active() || menu.items.is_empty() {
            return;
        }
        let theme = self.theme.clone();
        let (cw, ch) = p.grid.cell_size();
        let mono = Style {
            font: p.grid.font,
            px: p.grid.px,
            color: theme.ink,
            tracking: self.px(self.behavior.typography.terminal_spacing),
        };
        let dim = Style {
            color: theme.dim,
            ..mono
        };
        let row_h = ch + self.px(4.0);
        let footer = self.px(28.0).min((p.rect.h - row_h).max(0.0));
        let window = menu_window(menu.items.len(), menu.selected, p.rect.h, row_h, footer);
        let first = window.start;
        let visible = &menu.items[window];
        let width = visible
            .iter()
            .map(|it| {
                self.fonts.measure(mono, &it.label)
                    + self.fonts.measure(dim, &it.detail)
                    + self.px(36.0)
            })
            .fold(self.px(230.0), f32::max)
            .min(p.rect.w * 0.8)
            .max(cw);
        let height = (visible.len() as f32 * row_h + footer).min(p.rect.h);
        let anchor = menu
            .items
            .get(menu.selected)
            .map(|i| i.range.start)
            .unwrap_or(line.caret);
        // The live replacement span is visible in place. The surrounding
        // command remains committed text until the user inserts a choice.
        if let Some(selected) = menu.items.get(menu.selected) {
            for cell in line
                .cells
                .iter()
                .filter(|c| selected.range.contains(&c.byte))
            {
                scene.hline(
                    p.origin.0 + cell.col as f32 * cw,
                    p.origin.1 + (cell.row + 1) as f32 * ch - self.px(1.0),
                    cell.width as f32 * cw,
                    self.px(1.0),
                    self.surface.signal,
                );
            }
        }
        let (row, col) = line.cell_at(anchor);
        let bx = (p.origin.0 + col as f32 * cw)
            .min(p.rect.right() - width - self.px(4.0))
            .max(p.rect.x);
        let y = p.origin.1 + row as f32 * ch;
        let below = y + ch + self.px(2.0);
        let by = if below + height > p.rect.bottom() {
            (y - height - self.px(2.0)).max(p.rect.y)
        } else {
            below
        };
        let rect = Rect::new(bx, by, width, height);
        scene.rect(rect, theme.paper);
        scene.outline(rect, self.px(1.0), fade(theme.ink, 0.45));
        let old_clip = scene.clip();
        scene.layer(Some(rect));
        menu.hits.borrow_mut().clear();
        for (k, item) in visible.iter().enumerate() {
            let yy = by + self.px(3.0) + k as f32 * row_h;
            let hit_h = row_h.min(rect.bottom() - yy).max(0.0);
            if hit_h > 0.0 {
                menu.hits
                    .borrow_mut()
                    .push((Rect::new(bx, yy, width, hit_h), k + first));
            }
            if k + first == menu.selected {
                scene.rect(
                    Rect::new(bx, yy, width, row_h),
                    fade(self.surface.signal, 0.18),
                );
            }
            let detail_w = self.fonts.measure(dim, &item.detail);
            let detail_fits =
                self.fonts.measure(mono, &item.label) + detail_w + self.px(36.0) <= width;
            let available = if detail_fits {
                width - detail_w - self.px(36.0)
            } else {
                width - self.px(20.0)
            };
            let mut label = item.label.clone();
            if self.fonts.measure(mono, &label) > available {
                while self.fonts.measure(mono, &format!("{label}…")) > available
                    && !label.is_empty()
                {
                    label.pop();
                }
                if self.fonts.measure(mono, "…") <= available {
                    label.push('…');
                }
            }
            let baseline =
                (yy + self.px(2.0) + p.grid.metrics.baseline).min(rect.bottom() - self.px(3.0));
            self.fonts
                .draw(scene, mono, bx + self.px(10.0), baseline, &label);
            if detail_fits {
                self.fonts.draw(
                    scene,
                    dim,
                    bx + width - detail_w - self.px(10.0),
                    baseline,
                    &item.detail,
                );
            }
        }
        let hint = "CODE · TAB / ENTER INSERT · ESC";
        if footer >= self.px(20.0) {
            self.fonts.draw(
                scene,
                Style {
                    color: theme.dim,
                    ..self.label()
                },
                bx + self.px(10.0),
                by + height - self.px(7.0),
                hint,
            );
        }
        scene.layer(old_clip);
    }

    pub(crate) fn draw_prompt_ghost(&mut self, scene: &mut Scene, p: &TermPane, line: &PromptLine) {
        let Some(ghost) = self.prompt_prediction(p, line) else {
            return;
        };
        let (cw, ch) = p.grid.cell_size();
        let (mut row, mut col) = line.end;
        let style = Style {
            font: p.grid.font,
            px: p.grid.px,
            color: fade(self.theme.ink, 0.38),
            tracking: 0.0,
        };
        for c in ghost.chars() {
            let width = c.width().unwrap_or(0).min(2);
            if width == 0 {
                continue;
            }
            if col + width > p.term.cols() {
                row += 1;
                col = 0;
            }
            if row >= p.term.rows() {
                break;
            }
            let x = p.origin.0 + col as f32 * cw;
            let y = p.origin.1 + row as f32 * ch + p.grid.metrics.baseline;
            self.fonts.draw(scene, style, x, y, &c.to_string());
            col += width;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prompt(cols: usize, s: &str) -> Term {
        let mut t = Term::new(cols, 10, 100);
        t.advance(format!("$ \x1b]133;B\x07{s}").as_bytes());
        t
    }
    #[test]
    fn reads_unicode_wrapping_and_middle_caret_without_losing_tail() {
        let mut t = prompt(12, "echo 界 ./src");
        t.advance(b"\x1b[3D");
        let line = PromptLine::read(&t).unwrap();
        assert_eq!(line.text, "echo 界 ./src");
        assert_eq!(line.prefix(), "echo 界 ./");
        assert!(!line.at_end());
        assert_eq!(line.cells.iter().find(|c| c.byte == 5).unwrap().width, 2);
        assert_eq!(line.cell_at(9), (0, 10));
    }
    #[test]
    fn wrap_pending_is_after_last_character() {
        let line = PromptLine::read(&prompt(8, "123456")).unwrap();
        assert_eq!(line.text, "123456");
        assert_eq!(line.caret, 6);
        assert_eq!(line.end, (0, 8));
    }
    #[test]
    fn replacement_edits_only_current_word_with_unicode_char_counts() {
        let mut t = prompt(80, "git log -- --all");
        t.advance(b"\x1b[6D");
        let line = PromptLine::read(&t).unwrap();
        assert_eq!(line.token_range(), 8..10);
        assert_eq!(
            line.replace_bytes(&t, line.token_range(), "--oneline"),
            b"\x1b[D\x1b[D\x1b[3~\x1b[3~--oneline"
        );
        let u = PromptLine::read(&prompt(80, "echo 界é")).unwrap();
        assert_eq!(
            u.replace_bytes(&t, 5..10, "new"),
            b"\x1b[D\x1b[D\x1b[3~\x1b[3~new"
        );
    }
    #[test]
    fn token_acceptance_understands_quotes_and_escapes() {
        assert_eq!(
            next_token_end("git commit -m \"two words\" --amend", 16),
            25
        );
        assert_eq!(next_token_end("cat path\\ with\\ spaces rest", 8), 22);
        assert_eq!(next_token_end("echo 'it is' done", 5), 12);
        assert_eq!(token_at("echo 界é tail", 8), 5..10);
    }
    #[test]
    fn snapshots_include_prompt_identity_and_caret() {
        let mut t = prompt(80, "git log");
        let line = PromptLine::read(&t).unwrap();
        let stamp = line.stamp();
        t.advance(b"\x1b[D");
        assert!(!PromptLine::read(&t).unwrap().matches(&stamp));
        let mut other = stamp.clone();
        other.start.0 += 1;
        assert!(!line.matches(&other));
    }
    #[test]
    fn edit_cannot_smuggle_submission_or_split_utf8() {
        let t = prompt(80, "é");
        let line = PromptLine::read(&t).unwrap();
        assert!(line.replace_bytes(&t, 1..2, "a").is_empty());
        assert!(line.replace_bytes(&t, 0..2, "echo\n").is_empty());
    }

    #[test]
    fn decorations_and_early_wide_wrap_are_not_editable_buffer_text() {
        let mut right = prompt(80, "git st");
        right.advance(b"\x1b[s\x1b[70Gmain\x1b[u");
        assert!(PromptLine::read(&right).is_none());
        let mut native = prompt(80, "git st");
        native.advance(b"\x1b[s\x1b[2matus --short\x1b[0m\x1b[u");
        assert!(PromptLine::read(&native).is_none());
        assert!(PromptLine::read(&prompt(8, "abcde界")).is_none());
    }

    #[test]
    fn pending_waits_through_partial_repaint_then_acknowledges_exact_echo() {
        let t = prompt(80, "git st");
        let before = PromptLine::read(&t).unwrap();
        let edit = PromptEdit::new(&before, before.caret..before.caret, "atus");
        assert!(edit.waiting(&before));
        let partial = PromptLine::read(&prompt(80, "git ")).unwrap();
        assert!(edit.waiting(&partial));
        let after = PromptLine::read(&prompt(80, "git status")).unwrap();
        assert!(!edit.waiting(&after));
        let mut with_native = prompt(80, "git status");
        with_native.advance(b"\x1b[s\x1b[38;2;90;90;90m --short\x1b[0m\x1b[u");
        let with_native = PromptLine::read(&with_native).unwrap();
        assert!(!edit.waiting(&with_native));
        let mut next = partial;
        next.start.0 += 1;
        assert!(!edit.waiting(&next));
    }

    #[test]
    fn replacement_uses_negotiated_keyboard_encoding() {
        let mut t = prompt(80, "echo x");
        t.advance(b"\x1b[?1h");
        let line = PromptLine::read(&t).unwrap();
        assert_eq!(line.replace_bytes(&t, 5..6, "y"), b"\x1bOD\x1b[3~y");
    }

    #[test]
    fn unsupported_zero_width_sequences_fall_back_to_native_editor() {
        assert!(safe_text("printf 'café 界 😀'"));
        assert!(!safe_text("cafe\u{301}"));
        assert!(!safe_text("👩\u{200d}💻"));
        assert!(!safe_text("echo\r"));
    }

    #[test]
    fn short_menu_window_keeps_selected_candidate_visible() {
        assert_eq!(menu_window(40, 39, 100.0, 25.0, 28.0), 38..40);
        assert_eq!(menu_window(40, 18, 500.0, 25.0, 28.0), 11..19);
        assert_eq!(menu_window(2, 1, 15.0, 25.0, 0.0), 1..2);
    }
}
