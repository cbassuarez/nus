//! The prompt: nus's home, a terminal with no PTY behind it.
//!
//! One line, centred, a caret and nothing else. Type a URL and Enter: the
//! pane becomes that page. Type a command: the pane becomes a shell running
//! it. Enter on nothing: a shell. As you type, rows come up beneath the
//! line — the palette's rows, so tabs, recent pages and shells, layouts,
//! history, held shells and settings are all one keystroke away; ↑/↓ pick
//! one and Enter takes it. STARTUP · THEN · THE PROMPT puts it up first;
//! `home` in the palette brings it back. HOME · THE PLATE puts the line
//! under the icon, with the first places as stops on its band (plate.rs).
//! Drawn from the tokens, like every other surface: paper, ink, one signal.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, PaletteMode, PaletteRow, Pane};
use crate::settings::HomeLook;
use nus_render::theme::metric as m;

pub struct HomePane {
    pub rect: Rect,
    pub input: String,
    pub sel: usize,
    pub hits: Vec<(Rect, usize)>,
    pub since: Instant,
    /// The splash drew the plate's band already; it is not drawn in again.
    pub handed: bool,
    /// The plate's stops (plate.rs), gathered when the pane first draws
    /// and again now and then — holders are asked over a socket.
    pub places: Option<(Instant, Vec<PaletteRow>)>,
}

impl HomePane {
    pub fn new() -> HomePane {
        HomePane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), input: String::new(), sel: 0, hits: Vec::new(), since: Instant::now(), handed: false, places: None }
    }
}

/// What a line means: a page, or a command for a shell.
fn is_url(s: &str) -> bool {
    let q = s.trim();
    q.contains("://") || q.starts_with("localhost") || (q.contains('.') && !q.contains(' ') && !q.starts_with('.') && !q.contains('\\') && !q.contains('/'))
}

impl App {
    /// The rows under the line: the palette's, for what is typed; a short
    /// list of places to go when nothing is — under the plate, the stops.
    fn home_rows(&self, input: &str, places: &[PaletteRow]) -> Vec<PaletteRow> {
        let empty = input.trim().is_empty();
        if empty && self.behavior.home_look == HomeLook::Plate {
            return places.to_vec();
        }
        let mut rows: Vec<PaletteRow> = self.palette_rows(PaletteMode::Go, input).into_iter().filter(|r| !r.text.starts_with("search ") && !r.text.starts_with("open ")).collect();
        rows.truncate(if empty { 7 } else { 9 });
        rows
    }

    /// The pane's places, gathered on first use and refreshed every so often.
    fn home_places(&self, p: &mut HomePane) -> Vec<PaletteRow> {
        let stale = p.places.as_ref().map(|(at, _)| at.elapsed().as_secs() >= 20).unwrap_or(true);
        if stale && self.behavior.home_look == HomeLook::Plate {
            p.places = Some((Instant::now(), self.places()));
        }
        p.places.as_ref().map(|(_, v)| v.to_vec()).unwrap_or_default()
    }

    /// A tab whose pane is the prompt, in front.
    pub(crate) fn open_home(&mut self) {
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Home(_))) {
            self.activate(i);
            return;
        }
        let tab = self.make_tab(Pane::Home(HomePane::new()), None);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1);
        self.layout();
        self.dirty = true;
    }

    /// Enter: the line becomes a page or a shell in this very tab; a
    /// picked row runs and the prompt gives way to what it opened.
    fn home_commit(&mut self) {
        let i = self.active;
        let Some(Pane::Home(h)) = self.tabs.get(i).map(|t| &t.left) else { return };
        let input = h.input.trim().to_string();
        let sel = h.sel;
        let places = h.places.as_ref().map(|(_, v)| v.to_vec()).unwrap_or_default();
        let rows = self.home_rows(&input, &places);
        // A row only when you moved to one (sel is 1-based; 0 is the line itself).
        if let Some(row) = sel.checked_sub(1).and_then(|k| rows.get(k)) {
            {
                let action = row.action.clone();
                let before = self.tabs.len();
                self.run(action);
                // The prompt gives way when something else came up.
                if self.tabs.len() > before || self.active != i {
                    if let Some(k) = self.tabs.iter().position(|t| matches!(t.left, Pane::Home(_))) {
                        self.tabs.remove(k);
                        self.tab_removed(k);
                        if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        }
                    }
                }
                self.layout();
                self.dirty = true;
                return;
            }
        }
        let profile = self.behavior.default_profile;
        let replacement = if is_url(&input) {
            let url = crate::links::normalize(&input);
            let url = if url.contains("://") { url } else { format!("https://{url}") };
            self.new_web_pane(&url).map(Pane::Web)
        } else {
            match self.new_term_pane(false, profile) {
                Ok(mut t) => {
                    if !input.is_empty() {
                        t.type_at_prompt = Some(format!("{input}\r"));
                    }
                    Some(Pane::Term(t))
                }
                Err(_) => None,
            }
        };
        if let (Some(p), Some(tab)) = (replacement, self.tabs.get_mut(i)) {
            tab.left = p;
            tab.focus_right = false;
        }
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    pub(crate) fn home_commit_pub(&mut self) {
        self.home_commit();
    }

    /// Keys on the prompt. Returns true when it took the key.
    pub(crate) fn home_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key as K, NamedKey};
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let i = self.active;
        let Some(Pane::Home(h)) = self.tabs.get_mut(i).map(|t| &mut t.left) else { return false };
        if self.mods.control_key() || self.mods.alt_key() {
            return false;
        }
        match &ev.logical_key {
            K::Named(NamedKey::Enter) => {
                self.home_commit();
                return true;
            }
            K::Named(NamedKey::Backspace) => {
                h.input.pop();
                h.sel = 0;
            }
            K::Named(NamedKey::ArrowDown) => h.sel += 1, // clamped when drawn
            K::Named(NamedKey::ArrowUp) => h.sel = h.sel.saturating_sub(1),
            K::Named(NamedKey::Escape) => {
                if h.input.is_empty() {
                    return false;
                }
                h.input.clear();
                h.sel = 0;
            }
            K::Named(NamedKey::Space) => h.input.push(' '),
            K::Character(c) => {
                h.input.push_str(c);
                h.sel = 0;
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// A click on a row.
    pub(crate) fn home_click(&mut self, x: f32, y: f32) -> bool {
        let i = self.active;
        let Some(Pane::Home(h)) = self.tabs.get_mut(i).map(|t| &mut t.left) else { return false };
        let Some(&(_, k)) = h.hits.iter().find(|(r, _)| r.contains(x, y)) else { return false };
        h.sel = k + 1;
        self.home_commit();
        true
    }

    /// The prompt, drawn: the line alone, or under the plate.
    pub(crate) fn draw_home(&mut self, scene: &mut Scene, p: &mut HomePane, focused: bool) {
        let t = self.theme.clone();
        let r = p.rect;
        let ink = t.ink;
        let paper = self.paper();
        scene.rect(r, paper);
        let plate = self.behavior.home_look == HomeLook::Plate;
        // Under the plate the line sits beneath the icon and comes up once
        // the band closes; alone, it sits a third of the way down.
        let (y0, up) = if plate { self.draw_plate_icon(scene, p) } else { (r.y + r.h * 0.34, 1.0) };
        // The wordmark, small, where the pane begins — the plate is the n itself.
        if !plate {
            let word = Style { font: self.f.wordmark, px: self.px(22.0), color: fade(ink, 0.55), tracking: 0.0 };
            self.fonts.draw(scene, word, r.x + self.px(28.0), r.y + self.px(42.0), "nus");
        }
        // The line: a caret in signal, the input in mono, a rule beneath.
        let px = self.px(20.0);
        let mono = Style { font: self.f.ui, px, color: fade(ink, up), tracking: 0.0 };
        let line_w = (r.w * 0.62).max(self.px(320.0)).min(r.w - self.px(56.0));
        let x0 = r.x + (r.w - line_w) / 2.0;
        let caret_w = self.fonts.draw(scene, Style { color: fade(self.surface.signal, up), ..mono }, x0, y0, "»") + self.px(12.0);
        let shown = self.fit(mono, &p.input, line_w - caret_w - px);
        let tw = self.fonts.draw(scene, mono, x0 + caret_w, y0, &shown);
        // The block caret, breathing.
        if focused {
            let on = (self.started.elapsed().as_secs_f32() * 2.0) as u32 % 2 == 0 || p.since.elapsed().as_millis() < 600;
            if on {
                scene.rect(Rect::new(x0 + caret_w + tw + self.px(2.0), y0 - px * 0.78, px * 0.5, px * 0.95), fade(ink, up));
            }
            self.dirty = true;
        }
        scene.hline(x0, y0 + self.px(12.0), line_w, self.px(m::HAIRLINE), fade(ink, 0.45 * up));
        // Rows beneath: the palette's, for what is typed. Under the plate
        // with nothing typed, the rows are the stops on the band.
        let places = self.home_places(p);
        let rows = self.home_rows(&p.input, &places);
        p.hits.clear();
        let sel = p.sel.min(rows.len());
        p.sel = sel;
        let label = self.label();
        let dim = Style { color: fade(t.dim, up), ..label };
        let foot_y = r.bottom() - self.px(26.0);
        if plate && p.input.trim().is_empty() {
            self.draw_stops(scene, p, &rows, sel, up);
        } else {
            let row_h = self.px(30.0);
            let mut y = y0 + self.px(30.0);
            let (mx, my) = self.mouse;
            for (k, row) in rows.iter().enumerate() {
                if y + row_h > foot_y - self.px(8.0) {
                    break;
                }
                let rr = Rect::new(x0, y, line_w, row_h);
                let hot = k + 1 == sel || rr.contains(mx, my);
                if k + 1 == sel {
                    scene.rect(Rect::new(x0 - self.px(10.0), y + self.px(6.0), self.px(2.0), row_h - self.px(12.0)), fade(self.surface.signal, up));
                }
                let base = y + row_h / 2.0 + self.px(4.0);
                let num_w = self.px(28.0);
                self.fonts.draw(scene, dim, x0, base, &row.num);
                let text = self.fit(label, &row.text, line_w - num_w);
                self.fonts.draw(scene, Style { color: fade(ink, if hot { 1.0 } else { 0.75 } * up), ..label }, x0 + num_w, base, &text);
                p.hits.push((rr, k));
                y += row_h;
            }
        }
        // One dim line at the foot, the only words on the page.
        let foot = if sel > 0 {
            "enter · this row"
        } else if p.input.is_empty() {
            if plate { "enter · a shell   ·   a url · a page   ·   a command · a shell running it   ·   ↓ the stops" } else { "enter · a shell   ·   a url · a page   ·   a command · a shell running it   ·   ↓ the rows" }
        } else if is_url(&p.input) {
            "enter · this page"
        } else {
            "enter · a shell running this"
        };
        let fw = self.fonts.measure(dim, foot);
        self.fonts.draw(scene, dim, r.x + (r.w - fw) / 2.0, foot_y, foot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_commands() {
        assert!(is_url("https://nus.dev"));
        assert!(is_url("localhost:8000"));
        assert!(is_url("docs.rs"));
        assert!(!is_url("cargo test -p nus-vt"));
        assert!(!is_url("./scripts/run.sh"));
        assert!(!is_url("C:\\Users\\seb"));
    }
}
