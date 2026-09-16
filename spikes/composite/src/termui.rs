//! The terminal's interaction layer: selection with semantic zones,
//! click-to-move at a prompt, the scrollbar with prompt ticks, search in
//! scrollback, hints mode (labels over URLs, paths, hashes), block chips
//! (copy / run again), the unfocused dim and the resize overlay.
//! Everything here reads the grid through absolute lines so it survives
//! scrolling.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene, Theme};
use winit::event::{ElementState, MouseButton};

use crate::app::{fade, hover_key, App, IconMotion, Pane, TermPane};
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

/// Search in scrollback.
#[derive(Clone, Debug, Default)]
pub struct Search {
    pub query: String,
    pub matches: Vec<(u64, usize, usize)>,
    pub current: usize,
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
    fn term_cell(p: &TermPane, x: f32, y: f32) -> (u64, usize) {
        let (cw, ch) = p.grid.cell_size();
        let grid = p.term.grid();
        let col = ((x - p.origin.0) / cw).floor().max(0.0) as usize;
        let row = ((y - p.origin.1) / ch).floor().max(0.0) as usize;
        let row = row.min(grid.rows().saturating_sub(1));
        (grid.abs_of_display(row), col.min(grid.cols().saturating_sub(1)))
    }

    /// Mouse in a terminal pane. Returns true when consumed.
    pub(crate) fn term_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        let pressed = state == ElementState::Pressed;
        let shift = self.mods.shift_key();
        let ctrl = self.mods.control_key();
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let mut acted = false;
        let mut open_url: Option<String> = None;
        let mut copy: Option<String> = None;
        let mut run: Option<String> = None;
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Term(t) = p else { continue };
            // Release ends a drag anywhere.
            if !pressed && button == MouseButton::Left {
                if let Some(sel) = t.sel.as_mut() {
                    if sel.dragging {
                        sel.dragging = false;
                        acted = true;
                    }
                }
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
                if let Some((r, kind)) = t.chip_hits.iter().find(|(r, _)| r.contains(x, y)).cloned() {
                    let _ = r;
                    match kind {
                        0 => copy = Some(t.block_output_text(t.hover_block)),
                        _ => run = Some(t.block_cmd_text(t.hover_block)),
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
                            _ => copy = Some(hit.text.clone()),
                        }
                    }
                    t.hints = None;
                    acted = true;
                    continue;
                }
                // Click count.
                let now = Instant::now();
                let count = match t.clicks {
                    Some(c) if now.duration_since(c.at).as_millis() < 400 && c.pos == (line, col) => c.count + 1,
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
                // Right click: paste, or copy when something is selected (kitty's way).
                if t.sel.is_some() {
                    copy = Some(t.selection_text());
                    t.sel = None;
                } else {
                    self.paste_request = true;
                }
                acted = true;
            }
        }
        if let Some(u) = open_url {
            self.open_url(&u, true);
        }
        if let Some(text) = copy {
            if !text.is_empty() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(text);
                }
                self.play_event("toggle");
            }
        }
        if let Some(cmd) = run {
            if !cmd.is_empty() {
                if let Some(Pane::Term(t)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    let _ = t.pty.write(format!("{cmd}\r").as_bytes());
                }
            }
        }
        if acted {
            self.dirty = true;
        }
        acted
    }

    /// Drag updates: selection head, scrollbar thumb.
    pub(crate) fn term_drag(&mut self, x: f32, y: f32) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Term(t) = p else { continue };
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
            let grid = t.term.grid();
            let row = row_f.floor().clamp(0.0, grid.rows() as f32 - 1.0) as usize;
            let col = ((x - t.origin.0) / cw).floor().clamp(0.0, grid.cols() as f32 - 1.0) as usize;
            let head = (grid.abs_of_display(row), col);
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
        let top = grid.abs_of_display(0);
        let label = self.label();
        let (mx, my) = self.mouse;

        // Selection: an ink wash over the cells.
        if let Some(sel) = &p.sel {
            let (a, b) = sel.bounds(&p.term);
            for row in 0..rows {
                let line = top + row as u64;
                if line < a.0 || line > b.0 {
                    continue;
                }
                let from = if line == a.0 { a.1 } else { 0 };
                let to = if line == b.0 { (b.1 + 1).min(cols) } else { cols };
                if to > from {
                    let x = p.origin.0 + from as f32 * cw;
                    let y = p.origin.1 + row as f32 * ch;
                    scene.rect(Rect::new(x, y, (to - from) as f32 * cw, ch), fade(ink, 0.22));
                }
            }
        }
        // Search matches: outlined; the current one filled.
        if let Some(s) = &p.search {
            for (i, &(line, col, len)) in s.matches.iter().enumerate() {
                if line < top || line >= top + rows as u64 {
                    continue;
                }
                let x = p.origin.0 + col as f32 * cw;
                let y = p.origin.1 + (line - top) as f32 * ch;
                let rr = Rect::new(x, y, len as f32 * cw, ch);
                if i == s.current {
                    scene.rect(rr, fade(self.surface.signal, 0.35));
                }
                scene.outline(rr, self.px(1.0), fade(self.surface.signal, 0.9));
            }
        }
        // Hints: labels over matches; typed prefix narrows them.
        if let Some(h) = &p.hints {
            let chip = Style { color: t.paper, ..self.label_strong() };
            for item in &h.items {
                if !item.label.starts_with(&h.typed) || item.line < top || item.line >= top + rows as u64 {
                    continue;
                }
                let x = p.origin.0 + item.col as f32 * cw;
                let y = p.origin.1 + (item.line - top) as f32 * ch;
                scene.rect(Rect::new(x, y, item.len as f32 * cw, ch), fade(self.surface.signal, 0.18));
                let lw = self.fonts.measure(chip, &item.label.to_uppercase()) + self.px(8.0);
                let lr = Rect::new(x - self.px(2.0), y - self.px(2.0), lw, ch.min(self.px(18.0)));
                scene.rect(Rect::new(lr.x + self.px(2.0), lr.y + self.px(2.0), lr.w, lr.h), ink);
                scene.rect(lr, if item.kind == HintKind::Url { self.surface.signal } else { ink });
                self.fonts.draw(scene, chip, lr.x + self.px(4.0), lr.y + lr.h * 0.75, &item.label.to_uppercase());
            }
        }
        // Blocks: hover a block and a gutter rule plus chips appear.
        p.chip_hits.clear();
        if self.behavior.shell_integration && !p.term.marks.is_empty() && r.contains(mx, my) && p.hints.is_none() {
            let (line, _) = Self::term_cell(p, mx, my);
            if let Some((start, end, cmd, exit)) = p.term.block_at(line) {
                p.hover_block = start;
                let y0 = p.origin.1 + (start.max(top) - top) as f32 * ch;
                let y1 = p.origin.1 + (end.min(top + rows as u64) - top) as f32 * ch;
                if y1 > y0 && !cmd.is_empty() {
                    scene.rect(Rect::new(r.x + self.px(8.0), y0, self.px(2.0), y1 - y0 - self.px(2.0)), fade(if exit.is_some_and(|e| e != 0) { self.surface.signal } else { ink }, 0.5));
                    // Chips at the block's top right: copy output · run again.
                    let isz = self.px(14.0);
                    let mut cx = r.right() - self.px(18.0) - isz;
                    let cy = y0 + self.px(2.0);
                    for (k, icon, motion) in [(1usize, nus_render::text::icons::RELOAD, IconMotion::Spin(90.0)), (0usize, nus_render::text::icons::COPY, IconMotion::Pop)] {
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
        let show = sb_len > 0 && (grid.display_offset > 0 || r.contains(mx, my) || p.scroll_drag);
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
        // Search band along the pane's top.
        if let Some(s) = &p.search {
            let bh = self.header_h();
            let br = Rect::new(r.x, r.y + hh, r.w, bh);
            scene.rect(br, ink);
            let inv = Style { color: t.paper, ..self.label_strong() };
            let inv_l = Style { color: t.paper, ..label };
            let by = br.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = br.x + self.px(m::HEADER_PAD_X);
            x += self.fonts.draw(scene, inv_l, x, by, "FIND") + self.px(12.0);
            let q = if s.query.is_empty() { "…".to_string() } else { s.query.clone() };
            x += self.fonts.draw(scene, Style { font: self.f.ui, px: self.px(m::UI_PX), color: t.paper, tracking: 0.0 }, x, by, &q);
            scene.rect(Rect::new(x + self.px(2.0), by - self.px(11.0), self.px(1.5), self.px(14.0)), t.paper);
            let count = if s.matches.is_empty() { "NO MATCHES".to_string() } else { format!("{} OF {}", s.current + 1, s.matches.len()) };
            let cw2 = self.fonts.measure(inv, &count);
            let keys = "ENTER NEXT · SHIFT+ENTER BACK · ESC";
            let kw = self.fonts.measure(inv_l, keys);
            self.fonts.draw(scene, inv_l, br.right() - self.px(m::HEADER_PAD_X) - kw, by, keys);
            self.fonts.draw(scene, inv, br.right() - self.px(m::HEADER_PAD_X) - kw - self.px(14.0) - cw2, by, &count);
        }
        // Unfocused split: a paper wash, so the focused pane reads.
        if split && !focused {
            scene.rect(Rect::new(r.x, r.y + hh, r.w, r.h - hh), fade(self.paper(), 0.35));
        }
    }

    /// The resize overlay: cols × rows while the window is being resized.
    pub(crate) fn draw_resize_overlay(&mut self, scene: &mut Scene) {
        let Some(at) = self.resized_at else { return };
        let age = at.elapsed().as_secs_f32();
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
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if let Pane::Term(t) = tab.focused() {
            t.search = Some(Search::default());
            t.hints = None;
            self.dirty = true;
        }
    }

    /// Hints mode in the focused shell: label every URL, path and hash on screen.
    pub(crate) fn term_hints_open(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if let Pane::Term(t) = tab.focused() {
            let grid = t.term.grid();
            let top = grid.abs_of_display(0);
            let mut items = Vec::new();
            for row in 0..grid.rows() {
                let line = top + row as u64;
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

    /// Keys while search or hints is up. Returns true when consumed.
    pub(crate) fn term_mode_key(&mut self, key: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key as WKey, NamedKey};
        if key.state != ElementState::Pressed {
            return false;
        }
        let shift = self.mods.shift_key();
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        let mut open_url: Option<String> = None;
        let mut copy: Option<String> = None;
        let mut consumed = false;
        if let Some(s) = t.search.as_mut() {
            consumed = true;
            match &key.logical_key {
                WKey::Named(NamedKey::Escape) => t.search = None,
                WKey::Named(NamedKey::Enter) => {
                    if !s.matches.is_empty() {
                        s.current = if shift { (s.current + s.matches.len() - 1) % s.matches.len() } else { (s.current + 1) % s.matches.len() };
                        let line = s.matches[s.current].0;
                        t.term.grid_mut().scroll_to_abs(line.saturating_sub(2));
                    }
                }
                WKey::Named(NamedKey::Backspace) => {
                    s.query.pop();
                    s.matches = t.term.search(&s.query);
                    s.current = s.matches.len().saturating_sub(1);
                }
                WKey::Character(c) => {
                    s.query.push_str(c);
                    s.matches = t.term.search(&s.query);
                    // Latest match first: that's where the eye is.
                    s.current = s.matches.len().saturating_sub(1);
                    if let Some(&(line, _, _)) = s.matches.get(s.current) {
                        t.term.grid_mut().scroll_to_abs(line.saturating_sub(2));
                    }
                }
                WKey::Named(NamedKey::Space) => {
                    s.query.push(' ');
                    s.matches = t.term.search(&s.query);
                }
                _ => consumed = false,
            }
        } else if let Some(h) = t.hints.as_mut() {
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
