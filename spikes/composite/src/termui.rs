//! The terminal's interaction layer: selection with semantic zones,
//! click-to-move at a prompt, the scrollbar with prompt ticks, search in
//! scrollback, hints mode (labels over URLs, paths, hashes), block chips
//! (copy / run again), the unfocused dim and the resize overlay.
//! Everything here reads the grid through absolute lines so it survives
//! scrolling.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use winit::event::{ElementState, MouseButton};

use crate::app::{Caps, fade, hover_key, App, IconMotion, Pane, TermPane};
use nus_render::theme::metric as m;

/// A selection between two absolute positions, in one of three zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Cell,
    Word,
    Line,
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub anchor: (u64, usize),
    pub head: (u64, usize),
    pub zone: Zone,
    /// The pointer is still down.
    pub dragging: bool,
}

impl Selection {
    /// Ordered (start, end), widened to the zone.
    pub fn bounds(&self, term: &nus_vt::Term) -> ((u64, usize), (u64, usize)) {
        let (a, b) = if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) };
        match self.zone {
            Zone::Cell => (a, b),
            Zone::Word => {
                let wa = term.word_at(a.0, a.1).map(|(s, _)| s).unwrap_or(a.1);
                let wb = term.word_at(b.0, b.1).map(|(_, e)| e).unwrap_or(b.1);
                ((a.0, wa), (b.0, wb))
            }
            Zone::Line => ((a.0, 0), (b.0, term.line_end(b.0).max(0))),
        }
    }
}

/// A hint: a match on screen with its label.
#[derive(Clone, Debug)]
pub struct Hint {
    pub line: u64,
    pub col: usize,
    pub len: usize,
    pub text: String,
    pub kind: HintKind,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintKind {
    Url,
    Path,
    Hash,
}

#[derive(Clone, Debug, Default)]
pub struct Hints {
    pub items: Vec<Hint>,
    pub typed: String,
}

/// Mouse clicks since the last, for double / triple.
#[derive(Clone, Copy, Debug)]
pub struct Clicks {
    pub at: Instant,
    pub pos: (u64, usize),
    pub count: u32,
}

const HINT_ALPHABET: &[u8] = b"asdfghjklqwertyuiopzxcvbnm";

/// Find URLs, paths and hashes in a row's text.
pub fn scan_hints(text: &str) -> Vec<(usize, usize, HintKind)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let n = chars.len();
    let mut i = 0;
    let is_url_char = |c: char| !c.is_whitespace() && !"<>\"'`()[]{}".contains(c);
    while i < n {
        let rest: String = chars[i..].iter().take(12).collect();
        // Schemed URLs.
        if rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("file://") || rest.starts_with("ssh://") || rest.starts_with("mailto:") {
            let mut j = i;
            while j < n && is_url_char(chars[j]) {
                j += 1;
            }
            while j > i && ".,;:!?".contains(chars[j - 1]) {
                j -= 1;
            }
            out.push((i, j - i, HintKind::Url));
            i = j.max(i + 1);
            continue;
        }
        // Bare localhost:port / host:port.
        if rest.starts_with("localhost:") {
            let mut j = i;
            while j < n && is_url_char(chars[j]) {
                j += 1;
            }
            out.push((i, j - i, HintKind::Url));
            i = j.max(i + 1);
            continue;
        }
        // Paths: C:\… , C:/… , /… , ./… , ../… , ~/…
        let drive = i + 2 < n && chars[i].is_ascii_alphabetic() && chars[i + 1] == ':' && (chars[i + 2] == '\\' || chars[i + 2] == '/');
        let rooted = (chars[i] == '/' || chars[i] == '~') && i + 1 < n && !chars[i + 1].is_whitespace();
        let rel = chars[i] == '.' && i + 1 < n && (chars[i + 1] == '/' || chars[i + 1] == '\\' || (chars[i + 1] == '.' && i + 2 < n && (chars[i + 2] == '/' || chars[i + 2] == '\\')));
        let at_start = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '/' || chars[i - 1] == '\\');
        if at_start && (drive || rooted || rel) {
            let mut j = i;
            while j < n && !chars[j].is_whitespace() && !"\"'`<>()[]{}|,;".contains(chars[j]) {
                j += 1;
            }
            while j > i + 1 && ".:".contains(chars[j - 1]) {
                j -= 1;
            }
            if j - i >= 2 {
                out.push((i, j - i, HintKind::Path));
                i = j;
                continue;
            }
        }
        // Git hashes: 7–40 hex, on their own.
        if chars[i].is_ascii_hexdigit() && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            let mut j = i;
            while j < n && chars[j].is_ascii_hexdigit() {
                j += 1;
            }
            let len = j - i;
            let ends = j == n || !chars[j].is_alphanumeric();
            if (7..=40).contains(&len) && ends && chars[i..j].iter().any(|c| c.is_ascii_alphabetic()) {
                out.push((i, len, HintKind::Hash));
                i = j;
                continue;
            }
            i = j.max(i + 1);
            continue;
        }
        i += 1;
    }
    out
}

/// Labels for `n` hints: single letters, then pairs.
pub fn labels(n: usize) -> Vec<String> {
    let a = HINT_ALPHABET;
    if n <= a.len() {
        return a.iter().take(n).map(|&c| (c as char).to_string()).collect();
    }
    let mut out = Vec::new();
    'outer: for &x in a {
        for &y in a {
            out.push(format!("{}{}", x as char, y as char));
            if out.len() == n {
                break 'outer;
            }
        }
    }
    out
}

impl App {
    /// Pointer → (absolute line, col) in a terminal pane.
    /// The grid cell under a point, for the links layer.
    pub(crate) fn term_cell_pub(p: &mut TermPane, x: f32, y: f32) -> (u64, usize) {
        Self::term_cell(p, x, y)
    }

    fn term_cell(p: &mut TermPane, x: f32, y: f32) -> (u64, usize) {
        let (cw, ch) = p.grid.cell_size();
        let cols = p.term.cols();
        let col = ((x - p.origin.0) / cw).floor().max(0.0) as usize;
        let row = ((y - p.origin.1) / ch).floor().max(0.0) as usize;
        (p.line_near_row(row), col.min(cols.saturating_sub(1)))
    }

    /// Mouse in a terminal pane. Returns true when consumed.
    pub(crate) fn term_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        let pressed = state == ElementState::Pressed;
        let shift = self.mods.shift_key();
        let ctrl = self.mods.control_key();
        let copy_on_select = self.behavior.copy_on_select;
        let middle_paste = self.behavior.middle_paste;
        let mut middle = false;
        let vt_mods = self.vt_mods();
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let mut acted = false;
        let mut open_url: Option<String> = None;
        let mut open_link: Option<String> = None;
        let mut link_asked = false;
        let link_click = self.behavior.link_click;
        let mut open_file: Option<std::path::PathBuf> = None;
        let mut copy: Option<String> = None;
        let mut block_action = None;
        let mut shell_menu: Option<(Option<u64>, Option<String>, String)> = None;
        let tab_id = tab.id;
        for (right, p) in std::iter::once((false, &mut tab.left)).chain(tab.right.as_mut().map(|p| (true, p))) {
            let Pane::Term(t) = p else { continue };
            if pressed && t.rect.contains(x, y) {
                t.prompt_edit_pending = None;
                t.code_menu = None;
            }
            // The application asked for the mouse: it gets presses in its
            // pane and the release wherever it lands. Shift keeps the click
            // for us (xterm's convention), so selection still works.
            if t.wants_mouse() && !shift {
                use nus_vt::input::{MouseAction, MouseButton as B};
                let b = match button {
                    MouseButton::Left => Some(B::Left),
                    MouseButton::Middle => Some(B::Middle),
                    MouseButton::Right => Some(B::Right),
                    _ => None,
                };
                if let Some(b) = b {
                    if pressed && t.rect.contains(x, y) && t.scrollbar.is_none_or(|s| !s.contains(x, y)) {
                        t.sel = None;
                        t.report_mouse(b, MouseAction::Press, vt_mods, x, y);
                        acted = true;
                        continue;
                    }
                    if !pressed && t.mouse_held == Some(b) {
                        t.report_mouse(b, MouseAction::Release, vt_mods, x, y);
                        acted = true;
                        continue;
                    }
                }
            }
            // Release ends a drag anywhere.
            if !pressed && button == MouseButton::Left {
                if let Some(sel) = t.sel.as_mut() {
                    if sel.dragging {
                        sel.dragging = false;
                        acted = true;
                        if copy_on_select {
                            let text = t.selection_text();
                            if !text.trim().is_empty() {
                                copy = Some(text);
                            }
                        }
                    }
                }
                continue;
            }
            if pressed && button == MouseButton::Middle && middle_paste && t.rect.contains(x, y) {
                middle = true;
                continue;
            }
            if !t.rect.contains(x, y) {
                continue;
            }
            // Scrollbar: click or drag the track.
            if let Some(track) = t.scrollbar {
                if track.contains(x, y) && pressed && button == MouseButton::Left {
                    let grid = t.term.grid();
                    let total = grid.scrollback_len() + grid.rows();
                    let frac = ((y - track.y) / track.h).clamp(0.0, 1.0);
                    let top_line = ((frac * total as f32) as usize).min(grid.scrollback_len());
                    let abs = grid.oldest_abs() + top_line as u64;
                    t.term.grid_mut().scroll_to_abs(abs);
                    t.scroll_drag = true;
                    acted = true;
                    continue;
                }
            }
            // Block chips: copy / run again.
            if pressed && button == MouseButton::Left {
                if let Some((_, kind)) = t.chip_hits.iter().find(|(r, _)| r.contains(x, y)).cloned() {
                    if let Some(action) = crate::blocks::BlockAction::from_chip(kind) {
                        block_action = Some((crate::blocks::BlockTarget { tab: tab_id, right, start: t.hover_block }, action));
                    }
                    acted = true;
                    continue;
                }
            }
            let (line, col) = Self::term_cell(t, x, y);
            if pressed && button == MouseButton::Left {
                // Hints mode: click a hint.
                if let Some(h) = &t.hints {
                    if let Some(hit) = h.items.iter().find(|i| i.line == line && col >= i.col && col < i.col + i.len) {
                        match hit.kind {
                            HintKind::Url => open_url = Some(hit.text.clone()),
                            HintKind::Path => {
                                // An existing file opens in the editor; else the text is copied.
                                let cwd = t.term.cwd.clone();
                                let p = std::path::Path::new(&hit.text);
                                let p = if p.is_absolute() { p.to_path_buf() } else { cwd.map(|c| std::path::Path::new(&c).join(p)).unwrap_or_else(|| p.to_path_buf()) };
                                if p.is_file() {
                                    open_file = Some(p);
                                } else {
                                    copy = Some(hit.text.clone());
                                }
                            }
                            _ => copy = Some(hit.text.clone()),
                        }
                    }
                    t.hints = None;
                    acted = true;
                    continue;
                }
                // A link under a plain click: ask, or open (the selection is not started).
                if !shift && !ctrl && t.link_ask.is_none() && link_click != crate::settings::LinkClick::HintsOnly {
                    if let Some(l) = Self::link_at(t, line, col) {
                        if link_click == crate::settings::LinkClick::Open {
                            open_link = Some(crate::links::normalize(&l.url));
                        } else {
                            t.link_ask = Some(l);
                            link_asked = true;
                        }
                        acted = true;
                        continue;
                    }
                }
                // Click count.
                let now = crate::clock::now();
                let count = match t.clicks {
                    // The system's double-click time, and a cell of slop: a hand
                    // that drifts a pixel across a cell edge still clicks twice.
                    Some(c) if now.duration_since(c.at) <= crate::window_resize::double_click_interval() && c.pos.0 == line && c.pos.1.abs_diff(col) <= 1 => c.count % 3 + 1,
                    _ => 1,
                };
                t.clicks = Some(Clicks { at: now, pos: (line, col), count });
                if shift {
                    if let Some(sel) = t.sel.as_mut() {
                        sel.head = (line, col);
                        sel.dragging = false;
                        acted = true;
                        continue;
                    }
                }
                match count {
                    1 => {
                        // At a prompt, a click on the command line moves the caret.
                        let grid = t.term.grid();
                        let cur_line = grid.abs_row(t.term.cursor().row);
                        let at_prompt = t.term.at_prompt() && grid.display_offset == 0;
                        let b = t.term.marks.last().filter(|mk| mk.kind == nus_vt::MarkKind::CommandStart).copied();
                        if at_prompt && line == cur_line && b.is_some_and(|b| col >= b.col) {
                            let cur = t.term.cursor().col as i64;
                            let d = col as i64 - cur;
                            let key: &[u8] = if d > 0 { b"\x1b[C" } else { b"\x1b[D" };
                            let mut out = Vec::new();
                            for _ in 0..d.unsigned_abs() {
                                out.extend_from_slice(key);
                            }
                            let _ = t.pty.write(&out);
                            t.sel = None;
                        } else {
                            t.sel = Some(Selection { anchor: (line, col), head: (line, col), zone: Zone::Cell, dragging: true });
                        }
                    }
                    2 => t.sel = Some(Selection { anchor: (line, col), head: (line, col), zone: Zone::Word, dragging: true }),
                    _ => {
                        // Triple: the line; with Ctrl, the whole block's output.
                        if ctrl {
                            if let Some((start, end, _, _)) = t.term.block_at(line) {
                                t.sel = Some(Selection { anchor: (start + 1, 0), head: (end.saturating_sub(1), usize::MAX / 2), zone: Zone::Line, dragging: false });
                            }
                        } else {
                            t.sel = Some(Selection { anchor: (line, col), head: (line, col), zone: Zone::Line, dragging: true });
                        }
                    }
                }
                acted = true;
            } else if pressed && button == MouseButton::Right {
                // Right click: the shell's menu — copy and paste, the link
                // and the block under the pointer (page_menu.rs).
                let block = t.term.block_at(line).filter(|(_, _, cmd, _)| !cmd.is_empty()).map(|(start, ..)| start);
                let link = Self::link_at(t, line, col).map(|l| crate::links::normalize(&l.url));
                let selection = if t.sel.is_some() { t.selection_text() } else { String::new() };
                shell_menu = Some((block, link, selection));
                acted = true;
            }
        }
        if let Some((block, link, selection)) = shell_menu {
            self.open_shell_menu((x, y), block, link, selection);
            return true;
        }
        if let Some(u) = open_url {
            self.open_url(&u, true);
        }
        if let Some(u) = open_link {
            let i = self.active;
            self.open_link(i, &u);
        }
        if link_asked {
            self.band_anim.replay(0.0, 1.0, self.motion.dur(crate::anim::base::BAND));
            self.play_event("toggle");
            self.dirty = true;
        }
        if let Some(p) = open_file {
            self.open_file(&p, true);
            return true;
        }
        if let Some((target, action)) = block_action {
            self.block_action(target, action);
            return true;
        }
        if middle {
            self.paste_into_shell();
            return true;
        }
        if let Some(text) = copy {
            if !text.is_empty() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(text);
                }
                self.play_event("toggle");
            }
        }
        if acted {
            self.dirty = true;
        }
        acted
    }

    /// Drag updates: selection head, scrollbar thumb.
    pub(crate) fn term_drag(&mut self, x: f32, y: f32) {
        let vt_mods = self.vt_mods();
        let shift = self.mods.shift_key();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Term(t) = p else { continue };
            if t.wants_mouse() && !shift && (t.mouse_held.is_some() || t.rect.contains(x, y)) {
                use nus_vt::input::{MouseAction, MouseButton};
                if t.mouse_held.is_some() || t.term.modes().contains(nus_vt::Modes::MOUSE_ANY) {
                    t.report_mouse(MouseButton::None, MouseAction::Motion, vt_mods, x, y);
                    continue;
                }
            }
            if t.scroll_drag {
                if let Some(track) = t.scrollbar {
                    let grid = t.term.grid();
                    let total = grid.scrollback_len() + grid.rows();
                    let frac = ((y - track.y) / track.h).clamp(0.0, 1.0);
                    let top = (frac * total as f32) as usize;
                    let abs = grid.oldest_abs() + top.min(grid.scrollback_len()) as u64;
                    t.term.grid_mut().scroll_to_abs(abs);
                    self.dirty = true;
                }
                continue;
            }
            let Some(sel) = t.sel.as_mut() else { continue };
            if !sel.dragging {
                continue;
            }
            let (cw, ch) = t.grid.cell_size();
            let grid = t.term.grid();
            // Dragging past the top or bottom scrolls.
            let row_f = (y - t.origin.1) / ch;
            if row_f < 0.0 {
                t.term.grid_mut().scroll_display(1);
            } else if row_f >= grid.rows() as f32 {
                t.term.grid_mut().scroll_display(-1);
            }
            let (rows, cols) = (t.term.rows(), t.term.cols());
            let row = row_f.floor().clamp(0.0, rows as f32 - 1.0) as usize;
            let col = ((x - t.origin.0) / cw).floor().clamp(0.0, cols as f32 - 1.0) as usize;
            let head = (t.line_near_row(row), col);
            if let Some(sel) = t.sel.as_mut() {
                if sel.head != head {
                    sel.head = head;
                    self.dirty = true;
                }
            }
        }
    }

    pub(crate) fn term_release_scroll(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Term(t) = p {
                    t.scroll_drag = false;
                }
            }
        }
    }

    /// Selection, search matches, hints, blocks, scrollbar, dim: drawn over
    /// the grid for one pane.
    pub(crate) fn draw_term_overlays(&mut self, scene: &mut Scene, p: &mut TermPane, r: Rect, hh: f32, focused: bool, split: bool) {
        let t = self.theme.clone();
        let ink = t.ink;
        let (cw, ch) = p.grid.cell_size();
        let grid = p.term.grid();
        let rows = grid.rows();
        let cols = grid.cols();
        let label = self.label();
        let (mx, my) = self.mouse;
        self.draw_link_hover(scene, p, r);
        let view = p.view().to_vec();
        let line_at = |row: usize| -> Option<u64> {
            match view.get(row) {
                Some(nus_vt::grid::Display::Line(l)) => Some(*l),
                _ => None,
            }
        };
        let row_at = |line: u64| -> Option<usize> { view.iter().position(|d| matches!(d, nus_vt::grid::Display::Line(l) if *l == line)) };

        // Selection: an ink wash over the cells.
        if let Some(sel) = &p.sel {
            let (a, b) = sel.bounds(&p.term);
            for row in 0..rows {
                let Some(line) = line_at(row) else { continue };
                if line < a.0 || line > b.0 {
                    continue;
                }
                let from = if line == a.0 { a.1 } else { 0 };
                let to = if line == b.0 { (b.1 + 1).min(cols) } else { cols };
                if to > from {
                    let x = p.origin.0 + from as f32 * cw;
                    let y = p.origin.1 + row as f32 * ch;
                    scene.rect(Rect::new(x, y, (to - from) as f32 * cw, ch), t.selection);
                }
            }
            // The live end follows the drag direction and the expanded
            // word/line zone. Folded and scrolled-away rows have no edge.
            if focused && p.search.is_none() && !p.ask.as_ref().is_some_and(|ask| ask.focus)
                && !p.prompt_history.active() && p.confirm_paste.is_none() && p.block_filter.is_none()
                && p.hints.is_none() && p.link_ask.is_none() && a <= b && cols > 0 {

                let (line, col) = if sel.head >= sel.anchor {
                    (b.0, b.1.saturating_add(1).min(cols))
                } else { (a.0, a.1.min(cols)) };
                if let Some(row) = row_at(line).filter(|row| *row < rows) {
                    let x = p.origin.0 + col as f32 * cw;
                    let baseline = p.origin.1 + row as f32 * ch + p.grid.metrics.baseline;
                    self.draw_selection_edge(scene, x, baseline, p.grid.px, 1.0, self.last_key);
                }
            }
        }
        // Find: every match on screen tinted, the current one filled and
        // ruled. A match the terminal wrapped is lit on both rows. Only
        // rows on screen are drawn, however many matches there are.
        if let Some(s) = &p.search {
            let top = view.iter().find_map(|d| match d { nus_vt::grid::Display::Line(l) => Some(*l), _ => None }).unwrap_or(0);
            let bottom = view.iter().rev().find_map(|d| match d { nus_vt::grid::Display::Line(l) => Some(*l), _ => None }).unwrap_or(0);
            let first = s.hits.partition_point(|h| h.end_line < top);
            for h in s.hits[first..].iter().take_while(|h| h.line <= bottom) {
                let now = s.current == Some(*h);
                for line in h.line..=h.end_line {
                    let Some(row) = row_at(line) else { continue };
                    let Some((col, n)) = h.on_row(line, cols) else { continue };
                    let rr = Rect::new(p.origin.0 + col as f32 * cw, p.origin.1 + row as f32 * ch, n as f32 * cw, ch);
                    // Translucent: the match's own text stays readable
                    // through it, in every theme.
                    if now {
                        scene.rect(rr, fade(self.surface.signal, 0.32));
                        scene.outline(rr, self.px(m::FLOATING), ink);
                        // Just arrived: a ring closes in on it.
                        let ring = self.motion.dur(180.0);
                        if let Some(at) = s.moved_at.filter(|_| ring > 0.0) {
                            let k = (crate::clock::since(at).as_secs_f32() / ring).clamp(0.0, 1.0);
                            if k < 1.0 {
                                let grow = (1.0 - k) * self.px(10.0);
                                scene.outline(Rect::new(rr.x - grow, rr.y - grow, rr.w + 2.0 * grow, rr.h + 2.0 * grow), self.px(m::STRUCTURE), fade(ink, 1.0 - k));
                                self.dirty = true;
                            }
                        }
                    } else {
                        scene.rect(rr, fade(self.surface.signal, 0.18));
                    }
                }
            }
        }
        // Hints: labels over matches; typed prefix narrows them.
        if let Some(h) = &p.hints {
            let chip = Style { color: t.paper, ..self.label_strong() };
            for item in &h.items {
                if !item.label.starts_with(&h.typed) {
                    continue;
                }
                let Some(row) = row_at(item.line) else { continue };
                let x = p.origin.0 + item.col as f32 * cw;
                let y = p.origin.1 + row as f32 * ch;
                scene.rect(Rect::new(x, y, item.len as f32 * cw, ch), fade(self.surface.signal, 0.18));
                let lw = self.fonts.measure(chip, &item.label.caps()) + self.px(8.0);
                let lr = Rect::new(x - self.px(2.0), y - self.px(2.0), lw, ch.min(self.px(18.0)));
                scene.rect(Rect::new(lr.x + self.px(2.0), lr.y + self.px(2.0), lr.w, lr.h), ink);
                scene.rect(lr, if item.kind == HintKind::Url { self.surface.signal } else { ink });
                self.fonts.draw(scene, chip, lr.x + self.px(4.0), lr.y + lr.h * 0.75, &item.label.caps());
            }
        }
        // Blocks: hover a block and a gutter rule plus chips appear.
        p.chip_hits.clear();
        if self.behavior.shell_integration && !p.term.marks.is_empty() && r.contains(mx, my) && p.hints.is_none() {
            let (line, _) = Self::term_cell(p, mx, my);
            if let Some((start, end, cmd, exit)) = p.term.block_at(line) {
                p.hover_block = start;
                let view = p.view().to_vec();
                let first = view.iter().position(|d| match d { nus_vt::grid::Display::Line(l) => *l >= start, nus_vt::grid::Display::Fold(s, _) => *s >= start });
                let last = view.iter().rposition(|d| match d { nus_vt::grid::Display::Line(l) => *l < end, nus_vt::grid::Display::Fold(s, _) => *s < end });
                let (y0, y1) = match (first, last) {
                    (Some(f), Some(l)) if l >= f => (p.origin.1 + f as f32 * ch, p.origin.1 + (l + 1) as f32 * ch),
                    _ => (0.0, 0.0),
                };
                if y1 > y0 && !cmd.is_empty() {
                    scene.rect(Rect::new(r.x + self.px(8.0), y0, self.px(2.0), y1 - y0 - self.px(2.0)), fade(if exit.is_some_and(|e| e != 0) { self.surface.signal } else { ink }, 0.5));
                    // Chips at the block's top right: share · run again · copy output · clip to note.
                    let isz = self.px(14.0);
                    let mut cx = r.right() - self.px(18.0) - isz;
                    let cy = y0 + self.px(2.0);
                    for (k, icon, motion) in [(2usize, nus_render::text::icons::SHARE, IconMotion::Pop), (1usize, nus_render::text::icons::RELOAD, IconMotion::Spin(90.0)), (0usize, nus_render::text::icons::COPY, IconMotion::Pop), (3usize, nus_render::text::icons::PENCIL, IconMotion::Pop)] {
                        let hit = Rect::new(cx - self.px(6.0), cy - self.px(4.0), isz + self.px(12.0), isz + self.px(8.0));
                        scene.push(nus_render::Instance::rounded(hit, self.px(4.0), fade(self.paper(), 0.92)));
                        self.icon_button(scene, icon, isz, cx, cy, ink, hit, hover_key("blockchip", k), motion);
                        p.chip_hits.push((hit, k));
                        cx -= isz + self.px(16.0);
                    }
                }
            }
        }
        // Scrollbar: a thin track at the right; the thumb, prompt ticks.
        let grid = p.term.grid();
        let sb_len = grid.scrollback_len();
        let show = sb_len > 0 && (grid.display_offset > 0 || r.contains(mx, my) || p.scroll_drag || p.search.as_ref().is_some_and(|s| !s.hits.is_empty()));
        if show {
            let track = Rect::new(r.right() - self.px(8.0), r.y + hh + self.px(4.0), self.px(4.0), r.h - hh - self.px(8.0));
            p.scrollbar = Some(track);
            let total = (sb_len + rows) as f32;
            let th = (track.h * rows as f32 / total).max(self.px(18.0));
            let pos = (sb_len - grid.display_offset) as f32 / total;
            scene.rect(track, fade(ink, 0.08));
            for mk in p.term.marks.iter().filter(|mk| mk.kind == nus_vt::MarkKind::PromptStart) {
                let f = (mk.line.saturating_sub(grid.oldest_abs())) as f32 / total;
                scene.rect(Rect::new(track.x - self.px(2.0), track.y + f * track.h, track.w + self.px(4.0), self.px(1.0)), fade(ink, 0.45));
            }
            scene.push(nus_render::Instance::rounded(Rect::new(track.x, track.y + pos * track.h, track.w, th), self.px(2.0), fade(ink, 0.55)));
            // Where the matches are; the current one heavier. A tick per
            // pixel row at most, so a huge count costs nothing.
            if let Some(s) = &p.search {
                let mut last = -1.0f32;
                for h in &s.hits {
                    let f = (h.line.saturating_sub(grid.oldest_abs())) as f32 / total;
                    let y = (track.y + f * track.h).round();
                    if y != last {
                        scene.rect(Rect::new(track.x - self.px(3.0), y, track.w + self.px(6.0), self.px(1.5)), fade(self.surface.signal, 0.9));
                        last = y;
                    }
                }
                if let Some(c) = s.current {
                    let f = (c.line.saturating_sub(grid.oldest_abs())) as f32 / total;
                    scene.rect(Rect::new(track.x - self.px(4.0), track.y + f * track.h - self.px(1.0), track.w + self.px(8.0), self.px(3.0)), ink);
                }
            }
        } else {
            p.scrollbar = None;
        }
        // "Scrolled up" note.
        if grid.display_offset > 0 {
            let text = format!("{} LINES UP · END", grid.display_offset);
            let tw = self.fonts.measure(label, &text);
            let bx = r.right() - self.px(24.0) - tw;
            let by = r.bottom() - self.px(12.0);
            scene.rect(Rect::new(bx - self.px(8.0), by - self.px(12.0), tw + self.px(16.0), self.px(18.0)), fade(self.paper(), 0.9));
            self.fonts.draw(scene, Style { color: t.dim, ..label }, bx, by, &text);
        }
        // Unfocused split: a paper wash, so the focused pane reads.
        if split && !focused {
            scene.rect(Rect::new(r.x, r.y + hh, r.w, r.h - hh), fade(self.paper(), 0.35));
        }
    }

    /// The resize overlay: cols × rows while the window is being resized.
    pub(crate) fn draw_resize_overlay(&mut self, scene: &mut Scene) {
        let Some(at) = self.resized_at else { return };
        let age = crate::clock::since(at).as_secs_f32();
        if age > 0.9 {
            self.resized_at = None;
            return;
        }
        let a = (1.0 - (age - 0.6).max(0.0) / 0.3).clamp(0.0, 1.0);
        let Some(tab) = self.tabs.get(self.active) else { return };
        let Pane::Term(t) = &tab.left else { return };
        let text = format!("{} × {}", t.term.cols(), t.term.rows());
        let st = Style { font: self.f.wordmark, px: self.px(28.0), color: fade(self.theme.ink, a), tracking: 0.0 };
        let tw = self.fonts.measure(st, &text);
        let r = t.rect;
        let bx = r.x + (r.w - tw) / 2.0;
        let by = r.y + r.h / 2.0;
        let card = Rect::new(bx - self.px(18.0), by - self.px(30.0), tw + self.px(36.0), self.px(44.0));
        scene.rect(Rect::new(card.x + self.px(4.0), card.y + self.px(4.0), card.w, card.h), fade(self.theme.ink, a));
        scene.rect(card, fade(self.paper(), a));
        scene.outline(card, self.px(m::STRUCTURE), fade(self.theme.ink, a));
        self.fonts.draw(scene, st, bx, by + self.px(2.0), &text);
        self.dirty = true;
    }

    /// Open search in the focused shell.
    pub(crate) fn term_search_open(&mut self) {
        if let Some(Pane::Term(t)) = self.tabs.get_mut(self.active).map(|t| t.focused()) {
            t.hints = None;
        }
        self.open_find();
    }

    /// Hints mode in the focused shell: label every URL, path and hash on screen.
    pub(crate) fn term_hints_open(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if let Pane::Term(t) = tab.focused() {
            let view = t.view().to_vec();
            let grid = t.term.grid();
            let mut items = Vec::new();
            for d in view.iter() {
                let nus_vt::grid::Display::Line(line) = *d else { continue };
                let Some(r) = grid.row_abs(line) else { continue };
                let text = r.text();
                for (col, len, kind) in scan_hints(&text) {
                    let s: String = text.chars().skip(col).take(len).collect();
                    items.push(Hint { line, col, len, text: s, kind, label: String::new() });
                }
            }
            let labels = labels(items.len());
            for (i, item) in items.iter_mut().enumerate() {
                item.label = labels[i].clone();
            }
            t.hints = if items.is_empty() { None } else { Some(Hints { items, typed: String::new() }) };
            t.search = None;
            self.dirty = true;
        }
    }

    /// Keys while hints are up. Returns true when consumed.
    pub(crate) fn term_mode_key(&mut self, key: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key as WKey, NamedKey};
        if key.state != ElementState::Pressed {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        let mut open_url: Option<String> = None;
        let mut copy: Option<String> = None;
        let mut consumed = false;
        if let Some(h) = t.hints.as_mut() {
            consumed = true;
            match &key.logical_key {
                WKey::Named(NamedKey::Escape) => t.hints = None,
                WKey::Named(NamedKey::Backspace) => {
                    h.typed.pop();
                }
                WKey::Character(c) => {
                    h.typed.push_str(&c.to_lowercase());
                    let live: Vec<&Hint> = h.items.iter().filter(|i| i.label.starts_with(&h.typed)).collect();
                    if live.len() == 1 && live[0].label == h.typed {
                        match live[0].kind {
                            HintKind::Url => open_url = Some(live[0].text.clone()),
                            _ => copy = Some(live[0].text.clone()),
                        }
                        t.hints = None;
                    } else if live.is_empty() {
                        h.typed.pop();
                    }
                }
                _ => consumed = false,
            }
        }
        if let Some(u) = open_url {
            let url = if u.starts_with("localhost") { format!("http://{u}") } else { u };
            self.open_url(&url, true);
        }
        if let Some(text) = copy {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(text);
            }
            self.play_event("toggle");
        }
        if consumed {
            self.dirty = true;
        }
        consumed
    }
}

impl TermPane {
    pub fn selection_text(&self) -> String {
        match &self.sel {
            Some(sel) => {
                let (a, b) = sel.bounds(&self.term);
                self.term.text_range(a, b)
            }
            None => String::new(),
        }
    }

    pub fn block_output_text(&self, start: u64) -> String {
        let c = self.term.marks.iter().find(|m| m.line >= start && m.kind == nus_vt::MarkKind::OutputStart).copied();
        c.map(|c| self.term.output_text(&c)).unwrap_or_default()
    }

    /// The absolute line the block's output begins on, as output_text counts it.
    pub fn block_output_first(&self, start: u64) -> Option<u64> {
        let c = self.term.marks.iter().find(|m| m.line >= start && m.kind == nus_vt::MarkKind::OutputStart)?;
        Some(if c.col == 0 { c.line } else { c.line + 1 })
    }

    pub fn block_cmd_text(&self, start: u64) -> String {
        let b = self.term.marks.iter().find(|m| m.line >= start && m.kind == nus_vt::MarkKind::CommandStart).copied();
        b.map(|b| self.term.command_text(&b)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_urls_paths_hashes() {
        let h = scan_hints("see https://docs.rs/wgpu, file C:\\Users\\seb\\nus\\Cargo.toml and ./src/main.rs at 9058fca1.");
        let kinds: Vec<HintKind> = h.iter().map(|x| x.2).collect();
        assert_eq!(kinds, vec![HintKind::Url, HintKind::Path, HintKind::Path, HintKind::Hash]);
        let text = "see https://docs.rs/wgpu, file C:\\Users\\seb\\nus\\Cargo.toml and ./src/main.rs at 9058fca1.";
        let s = |i: usize| text.chars().skip(h[i].0).take(h[i].1).collect::<String>();
        assert_eq!(s(0), "https://docs.rs/wgpu");
        assert_eq!(s(1), "C:\\Users\\seb\\nus\\Cargo.toml");
        assert_eq!(s(2), "./src/main.rs");
        assert_eq!(s(3), "9058fca1");
    }

    #[test]
    fn labels_grow() {
        assert_eq!(labels(3), vec!["a", "s", "d"]);
        assert_eq!(labels(30).len(), 30);
        assert_eq!(labels(30)[26], "sa");
    }
}
