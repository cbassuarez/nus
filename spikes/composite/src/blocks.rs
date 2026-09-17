//! Blocks without a custom shell. OSC 133 marks make every command and
//! its output an object: a lamp in the gutter (green ran, red failed,
//! breathing while it runs), fold it to one ruled line (click the lamp,
//! Ctrl+Shift+←/→), walk them (Ctrl+↑/↓), select one (Ctrl+A twice),
//! filter by command (Ctrl+Shift+/), share one as a page beside the shell.
//! The grid never changes: folds are a display list the renderer draws
//! through (`Grid::display_lines`), and every hit-test in the pane goes
//! through the same list.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use nus_vt::grid::Display;
use nus_vt::MarkKind;

use crate::app::{fade, hover_key, App, IconMotion, Pane, TermPane};
use nus_render::theme::metric as m;

/// A block as the pane sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Absolute lines: the prompt's, the output's first, the next prompt's.
    pub start: u64,
    pub output: u64,
    pub end: u64,
    pub cmd: String,
    pub exit: Option<i32>,
    pub running: bool,
}

impl Block {
    pub fn lines(&self) -> u64 {
        self.end.saturating_sub(self.output)
    }
}

impl TermPane {
    /// Every block in scrollback, oldest first. Cheap: a walk over marks.
    pub fn blocks(&self) -> Vec<Block> {
        let marks = &self.term.marks;
        let mut out: Vec<Block> = Vec::new();
        let last_line = self.term.grid().abs_row(self.term.rows().saturating_sub(1)) + 1;
        let mut i = 0;
        while i < marks.len() {
            if marks[i].kind != MarkKind::PromptStart {
                i += 1;
                continue;
            }
            let start = marks[i].line;
            let mut j = i + 1;
            let mut cmd_mark = None;
            let mut output = None;
            let mut exit = None;
            let mut seen_end = false;
            while j < marks.len() && marks[j].kind != MarkKind::PromptStart {
                match marks[j].kind {
                    MarkKind::CommandStart => cmd_mark = Some(marks[j]),
                    MarkKind::OutputStart => output = Some(marks[j].line),
                    MarkKind::CommandEnd(e) => {
                        exit = e;
                        seen_end = true;
                    }
                    _ => {}
                }
                j += 1;
            }
            let end = marks.get(j).map(|m| m.line).unwrap_or(last_line);
            if let (Some(b), Some(o)) = (cmd_mark, output) {
                let cmd = self.term.command_text(&b);
                if !cmd.trim().is_empty() {
                    out.push(Block { start, output: o, end, cmd, exit, running: !seen_end && j >= marks.len() });
                }
            }
            i = j;
        }
        out
    }

    /// The block holding an absolute line.
    pub fn block_of(&self, line: u64) -> Option<Block> {
        self.blocks().into_iter().rev().find(|b| line >= b.start && line < b.end)
    }

    /// The display list for this frame: folds applied, cached until the
    /// grid or the folds move.
    pub fn view(&mut self) -> &[Display] {
        let grid = self.term.grid();
        let key = (grid.abs_of_display(0), grid.rows(), self.folds.len(), self.folds.last().copied());
        if self.view_key != Some(key) || self.view.len() != grid.rows() || grid.is_damaged() {
            self.view = grid.display_lines(&self.folds);
            self.view_key = Some(key);
        }
        &self.view
    }

    /// The absolute line on display row `row`, or None on a fold's row.
    pub fn line_of_row(&mut self, row: usize) -> Option<u64> {
        match self.view().get(row) {
            Some(Display::Line(l)) => Some(*l),
            _ => None,
        }
    }

    /// The display row showing an absolute line, if it's on screen and not folded away.
    pub fn row_of_line(&mut self, line: u64) -> Option<usize> {
        self.view().iter().position(|d| matches!(d, Display::Line(l) if *l == line))
    }

    /// A fold's row, if it's on screen.
    pub fn row_of_fold(&mut self, start: u64) -> Option<usize> {
        self.view().iter().position(|d| matches!(d, Display::Fold(s, _) if *s == start))
    }

    /// The nearest line for a display row: a fold's row answers with the
    /// fold's start (the block's first output line).
    pub fn line_near_row(&mut self, row: usize) -> u64 {
        let rows = self.term.rows();
        let row = row.min(rows.saturating_sub(1));
        match self.view().get(row) {
            Some(Display::Line(l)) => *l,
            Some(Display::Fold(s, _)) => *s,
            None => self.term.grid().abs_of_display(0),
        }
    }

    pub fn is_folded(&self, b: &Block) -> bool {
        self.folds.iter().any(|&(s, _)| s == b.output)
    }

    /// Fold a block's output away (or open it). A block with no output
    /// has nothing to fold.
    pub fn toggle_fold(&mut self, b: &Block) {
        if b.lines() == 0 {
            return;
        }
        if let Some(i) = self.folds.iter().position(|&(s, _)| s == b.output) {
            self.folds.remove(i);
        } else {
            self.folds.push((b.output, b.end));
            self.folds.sort();
        }
        self.view_key = None;
        self.term.grid_mut().damage_all();
    }
}

impl App {
    /// Keys for blocks in the focused shell. Returns true when consumed.
    pub(crate) fn blocks_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key as WKey, NamedKey};
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        if !ctrl || alt {
            return false;
        }
        let lamps = self.behavior.blocks;
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        if t.term.modes().contains(nus_vt::Modes::ALT_SCREEN) {
            return false;
        }
        // The filter line takes letters while it's up.
        if let Some(f) = t.block_filter.as_mut() {
            match &ev.logical_key {
                WKey::Named(NamedKey::Escape) => t.block_filter = None,
                WKey::Named(NamedKey::Backspace) => {
                    if f.pop().is_none() {
                        t.block_filter = None;
                    }
                }
                _ => {
                    if let Some(s) = ev.text.as_ref().map(|s| s.to_string()).filter(|s| !s.chars().any(char::is_control)) {
                        f.push_str(&s);
                    }
                }
            }
            self.dirty = true;
            return true;
        }
        match &ev.logical_key {
            // Walk the blocks: the selected one scrolls into view.
            WKey::Named(NamedKey::ArrowUp) | WKey::Named(NamedKey::ArrowDown) if !shift => {
                let up = matches!(ev.logical_key, WKey::Named(NamedKey::ArrowUp));
                let blocks = t.blocks();
                if blocks.is_empty() {
                    return false;
                }
                let cur = t.block_sel.and_then(|s| blocks.iter().position(|b| b.start == s));
                let next = match (cur, up) {
                    (None, true) => blocks.len() - 1,
                    (None, false) => blocks.len() - 1,
                    (Some(i), true) => i.saturating_sub(1),
                    (Some(i), false) => (i + 1).min(blocks.len() - 1),
                };
                let b = &blocks[next];
                t.block_sel = Some(b.start);
                t.sel = None;
                // Scroll so the block's prompt is at the top, unless it's already on screen.
                if t.row_of_line(b.start).is_none() {
                    t.term.grid_mut().scroll_to_abs(b.start);
                    t.view_key = None;
                }
                self.dirty = true;
                true
            }
            // Fold / unfold the block under the caret (or selected).
            WKey::Named(NamedKey::ArrowLeft) | WKey::Named(NamedKey::ArrowRight) if shift && lamps => {
                let fold = matches!(ev.logical_key, WKey::Named(NamedKey::ArrowLeft));
                let line = t.block_sel.unwrap_or_else(|| t.term.grid().abs_row(t.term.cursor().row));
                let Some(b) = t.block_of(line) else { return false };
                if t.is_folded(&b) != fold {
                    t.toggle_fold(&b);
                }
                self.dirty = true;
                true
            }
            // Ctrl+A: the block; again: everything.
            WKey::Character(c) if c.eq_ignore_ascii_case("a") && !shift => {
                let now = Instant::now();
                let twice = t.select_all_at.is_some_and(|at| now.duration_since(at).as_millis() < 600);
                t.select_all_at = Some(now);
                let line = t.block_sel.unwrap_or_else(|| t.term.grid().abs_row(t.term.cursor().row));
                if twice || t.block_of(line).is_none() {
                    let grid = t.term.grid();
                    let last = grid.abs_row(grid.rows() - 1);
                    t.sel = Some(crate::termui::Selection { anchor: (grid.oldest_abs(), 0), head: (last, usize::MAX / 2), zone: crate::termui::Zone::Line, dragging: false });
                } else if let Some(b) = t.block_of(line) {
                    t.sel = Some(crate::termui::Selection { anchor: (b.output, 0), head: (b.end.saturating_sub(1), usize::MAX / 2), zone: crate::termui::Zone::Line, dragging: false });
                    t.block_sel = Some(b.start);
                }
                self.dirty = true;
                true
            }
            // Ctrl+Shift+/ : filter blocks by command.
            WKey::Character(c) if (c == "/" || c == "?") && shift => {
                t.block_filter = Some(String::new());
                self.dirty = true;
                true
            }
            WKey::Named(NamedKey::Escape) if t.block_sel.is_some() => {
                t.block_sel = None;
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    /// The gutter lamps, fold rows, the selected block's wash, the filter.
    /// Drawn after the grid, before the prompt line.
    pub(crate) fn draw_block_layer(&mut self, scene: &mut Scene, p: &mut TermPane, r: Rect, hh: f32) {
        if !self.behavior.shell_integration {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let signal = self.surface.signal;
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style { color: t.dim, ..label };
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        let (cw, ch) = p.grid.cell_size();
        let mono = Style { font: p.grid.font, px: p.grid.px, color: ink, tracking: 0.0 };
        let (mx, my) = self.mouse;
        let lamps_on = self.behavior.blocks;
        let now = self.started.elapsed().as_secs_f32();
        let blocks = p.blocks();
        let filter = p.block_filter.clone();
        let q = filter.as_deref().unwrap_or("").to_lowercase();
        p.lamp_hits.clear();
        let view = p.view().to_vec();
        let rows = view.len();
        // Filter: blocks that don't match dim; matching ones keep their ink.
        let matches = |b: &Block| q.is_empty() || b.cmd.to_lowercase().contains(&q);
        let lamp_d = self.px(7.0);
        let gutter_x = r.x + self.px(6.0);
        for b in &blocks {
            let folded = p.is_folded(b);
            let selected = p.block_sel == Some(b.start);
            let hit_row = view.iter().position(|d| matches!(d, Display::Line(l) if *l == b.start));
            let dimmed = !matches(b);
            // The selected block: a wash down its rows.
            if selected {
                let first = view.iter().position(|d| match d { Display::Line(l) => *l >= b.start, Display::Fold(s, _) => *s >= b.start });
                let last = view.iter().rposition(|d| match d { Display::Line(l) => *l < b.end, Display::Fold(s, _) => *s < b.end });
                if let (Some(f), Some(l)) = (first, last) {
                    if l >= f {
                        scene.rect(Rect::new(r.x, p.origin.1 + f as f32 * ch, r.w, (l - f + 1) as f32 * ch), fade(signal, 0.07));
                        scene.rect(Rect::new(r.x, p.origin.1 + f as f32 * ch, self.px(2.0), (l - f + 1) as f32 * ch), signal);
                    }
                }
            }
            if dimmed {
                if let (Some(f), Some(l)) = (
                    view.iter().position(|d| match d { Display::Line(x) => *x >= b.start, Display::Fold(s, _) => *s >= b.start }),
                    view.iter().rposition(|d| match d { Display::Line(x) => *x < b.end, Display::Fold(s, _) => *s < b.end }),
                ) {
                    if l >= f {
                        scene.rect(Rect::new(r.x, p.origin.1 + f as f32 * ch, r.w, (l - f + 1) as f32 * ch), fade(paper, 0.72));
                    }
                }
            }
            // The lamp, on the prompt's row.
            if let (Some(row), true) = (hit_row, lamps_on) {
                let y = p.origin.1 + row as f32 * ch;
                let color = match (b.running, b.exit) {
                    (true, _) => fade(signal, 0.45 + 0.55 * ((now * 2.2).sin() * 0.5 + 0.5)),
                    (false, Some(0)) => ansi(2),
                    (false, Some(_)) => ansi(1),
                    (false, None) => t.dim,
                };
                if b.running {
                    self.dirty = true;
                }
                let lr = Rect::new(gutter_x, y + (ch - lamp_d) / 2.0, lamp_d, lamp_d);
                let hit = Rect::new(r.x, y, self.px(18.0), ch);
                let hot = hit.contains(mx, my);
                if hot {
                    scene.rect(Rect::new(lr.x - self.px(3.0), lr.y - self.px(3.0), lamp_d + self.px(6.0), lamp_d + self.px(6.0)), fade(ink, 0.12));
                }
                scene.rect(lr, if dimmed { fade(color, 0.4) } else { color });
                if folded {
                    // A folded block's lamp wears a bar: the fold handle.
                    scene.rect(Rect::new(lr.x - self.px(2.0), lr.y + lamp_d / 2.0 - self.px(0.5), lamp_d + self.px(4.0), self.px(1.0)), paper);
                }
                p.lamp_hits.push((hit, b.start));
                if hot {
                    self.tip_words(hit, if folded { "unfold" } else { "fold this block's output" });
                }
            }
        }
        // Fold rows: one ruled line each, over the rows the fold occupies.
        for (row, d) in view.iter().enumerate() {
            let Display::Fold(s, e) = d else { continue };
            let Some(b) = blocks.iter().find(|b| b.output == *s) else { continue };
            let y = p.origin.1 + row as f32 * ch;
            scene.rect(Rect::new(r.x, y, r.w, ch), crate::surface::mix(paper, ink, 0.035));
            let base = y + p.grid.metrics.baseline;
            let n = e - s;
            let lamp = match b.exit { Some(0) => "✓", Some(_) => "×", None => "·" };
            let text = format!("{} · {} line{} · {}", crate::app::fit_cmd(&b.cmd, 60), n, if n == 1 { "" } else { "s" }, lamp);
            let x = p.origin.0;
            self.fonts.draw(scene, Style { color: t.dim, ..mono }, x, base, &self.fit(mono, &text, r.w - self.px(60.0) - (x - r.x)));
            let hint = "CTRL+SHIFT+→";
            let hw = self.fonts.measure(dim, hint);
            self.fonts.draw(scene, dim, r.right() - self.px(18.0) - hw, base, hint);
            let _ = cw;
        }
        // The filter line, along the pane's foot.
        if let Some(f) = &filter {
            let fh = self.px(24.0);
            let fr = Rect::new(r.x, r.bottom() - fh, r.w, fh);
            scene.rect(fr, paper);
            scene.hline(fr.x, fr.y, fr.w, self.px(m::HAIRLINE), ink);
            let base = fr.y + fh / 2.0 + self.px(m::LABEL_PX) / 2.0 - self.px(2.0);
            let mut x = fr.x + self.px(18.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::SEARCH, self.px(12.0), x, base - self.px(11.0), ink);
            x += self.px(18.0);
            let shown = format!("{f}_");
            x += self.fonts.draw(scene, Style { color: signal, ..mono }, x, base, &shown) + self.px(14.0);
            let n = blocks.iter().filter(|b| matches(b)).count();
            self.fonts.draw(scene, dim, x, base, &format!("{n} OF {} BLOCKS", blocks.len()));
            let hint = "ESC";
            let hw = self.fonts.measure(dim, hint);
            self.fonts.draw(scene, dim, fr.right() - self.px(18.0) - hw, base, hint);
        }
        let _ = (strong, hover_key, IconMotion::Still, rows);
    }

    /// The block page in the focused (or the split's) browser pane, by its URL.
    pub(crate) fn focused_block_page(&self) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
            if let Pane::Web(w) = p {
                let url = w.tab.shared.borrow().url.clone();
                if self.block_pages.contains_key(&url) {
                    return Some(url);
                }
            }
        }
        None
    }

    /// A tooltip for a rect that isn't an icon button.
    pub(crate) fn tip_words(&mut self, anchor: Rect, words: &str) {
        let since = self.tip_since.get_or_insert_with(Instant::now);
        let since = *since;
        self.tip = Some(crate::app::Tip { anchor, text: words.into(), since });
        if since.elapsed().as_millis() < 700 {
            self.dirty = true;
        }
    }

    /// A lamp was clicked: fold or unfold its block. Returns true when one was.
    pub(crate) fn lamp_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Term(t) = p else { continue };
            let Some((_, start)) = t.lamp_hits.iter().find(|(r, _)| r.contains(x, y)).cloned() else { continue };
            if let Some(b) = t.blocks().into_iter().find(|b| b.start == start) {
                t.block_sel = Some(b.start);
                t.toggle_fold(&b);
                self.play_event("toggle");
                self.dirty = true;
                return true;
            }
        }
        false
    }

    /// The share page for a block: a reader-style page beside the shell.
    pub(crate) fn share_block(&mut self, start: u64) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let Some(t) = (match &tab.left {
            Pane::Term(t) => Some(t),
            _ => match &tab.right {
                Some(Pane::Term(t)) => Some(t),
                _ => None,
            },
        }) else {
            return;
        };
        let Some(b) = t.blocks().into_iter().find(|b| b.start == start) else { return };
        let output = t.block_output_text(b.start);
        let cwd = t.term.cwd.clone().unwrap_or_default();
        let page = crate::blockpage::BlockPage {
            cmd: b.cmd.clone(),
            output,
            cwd,
            exit: b.exit,
            lines: b.lines(),
            when: std::time::SystemTime::now(),
            shell: self.profiles.get(t.profile).map(|p| p.name.clone()).unwrap_or_default(),
        };
        let html = page.html(&self.theme, self.surface.signal);
        let path = crate::blockpage::write(&html);
        match path {
            Some(p) => {
                let url = format!("file:///{}", p.display().to_string().replace('\\', "/"));
                self.open_url(&url, false);
                self.block_pages.insert(url, page);
            }
            None => self.notice("could not write the block page"),
        }
    }
}
