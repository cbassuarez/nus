//! Links in the terminal. Hover a URL — one the scanner finds, or an OSC 8
//! hyperlink — and it underlines; a plain click opens it where LINKS says
//! pages go (a stack, the split, a new tab). Before it opens, a band on the
//! pane asks: *github.com · open? · enter · esc · d never ask again*.
//! TERMINAL · CLICK LINKS: ASK (default) · OPEN · HINTS ONLY (the old way:
//! Ctrl+Shift+O labels, nothing on click). Selection is untouched: a drag
//! that starts on a link still selects.

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, Pane, TermPane};
use nus_render::theme::metric as m;

/// A link under the pointer: where it is in the grid, and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkAt {
    pub url: String,
    pub line: u64,
    pub col: usize,
    pub len: usize,
}

/// The URL as the browser wants it: `localhost:8000` gets its scheme.
pub fn normalize(url: &str) -> String {
    let u = url.trim();
    if u.starts_with("localhost") || u.starts_with("127.0.0.1") {
        format!("http://{u}")
    } else {
        u.to_string()
    }
}

/// The host, for the band: `github.com`, `localhost:8000`.
pub fn host(url: &str) -> String {
    let u = normalize(url);
    u.split("//").nth(1).unwrap_or(&u).split('/').next().unwrap_or("").to_string()
}

impl App {
    /// The link at a grid cell, if any: an OSC 8 hyperlink first, then a
    /// URL the hint scanner finds on that row.
    pub(crate) fn link_at(p: &TermPane, line: u64, col: usize) -> Option<LinkAt> {
        let grid = p.term.grid();
        let row = grid.row_abs(line)?;
        // OSC 8: the run of cells sharing the id.
        if let Some(cell) = row.cells.get(col) {
            if cell.link != 0 {
                let id = cell.link;
                let url = p.term.hyperlinks.get(id as usize - 1)?.clone();
                let mut a = col;
                while a > 0 && row.cells[a - 1].link == id {
                    a -= 1;
                }
                let mut b = col;
                while b + 1 < row.cells.len() && row.cells[b + 1].link == id {
                    b += 1;
                }
                return Some(LinkAt { url, line, col: a, len: b - a + 1 });
            }
        }
        let text = row.text();
        let chars: Vec<char> = text.chars().collect();
        for (start, len, kind) in crate::termui::scan_hints(&text) {
            if kind == crate::termui::HintKind::Url && col >= start && col < start + len {
                let url: String = chars[start..start + len].iter().collect();
                return Some(LinkAt { url, line, col: start, len });
            }
        }
        None
    }

    /// Hover: underline the link under the pointer and remember it.
    pub(crate) fn draw_link_hover(&mut self, scene: &mut Scene, p: &mut TermPane, r: Rect) {
        p.link_hover = None;
        if self.behavior.link_click == crate::settings::LinkClick::HintsOnly || p.hints.is_some() || p.link_ask.is_some() {
            return;
        }
        let (mx, my) = self.mouse;
        if !r.contains(mx, my) {
            return;
        }
        let (line, col) = Self::term_cell_pub(p, mx, my);
        let Some(link) = Self::link_at(p, line, col) else { return };
        let Some(row) = p.row_of_line(line) else { return };
        let (cw, ch) = p.grid.cell_size();
        let x = p.origin.0 + link.col as f32 * cw;
        let y = p.origin.1 + (row + 1) as f32 * ch - self.px(2.0);
        scene.hline(x, y, link.len as f32 * cw, self.px(m::HAIRLINE), self.theme.ink);
        let words = format!("{} · click opens", host(&link.url));
        self.tip_words(Rect::new(x, y - ch, link.len as f32 * cw, ch + self.px(4.0)), &words);
        p.link_hover = Some(link);
    }

    /// Open where LINKS says pages go from this tab.
    pub(crate) fn open_link(&mut self, tab: usize, url: &str) {
        let url = normalize(url);
        match self.behavior.links {
            crate::settings::Links::Stack => self.open_in_stack(tab, &url),
            crate::settings::Links::Split => {
                self.activate(tab);
                self.open_url(&url, false);
            }
            crate::settings::Links::NewTab => self.open_url(&url, true),
        }
        self.dirty = true;
    }

    /// The band's keys on the focused shell: Enter opens, Esc cancels, D
    /// opens and stops asking. Returns true when a band took the key.
    pub(crate) fn link_band_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key as K, NamedKey};
        let i = self.active;
        let Some(tab) = self.tabs.get_mut(i) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        if t.link_ask.is_none() {
            return false;
        }
        let action = match key {
            K::Named(NamedKey::Enter) => Some(true),
            K::Named(NamedKey::Escape) => Some(false),
            K::Character(c) if c.eq_ignore_ascii_case("d") => {
                self.behavior.link_click = crate::settings::LinkClick::Open;
                self.save_prefs();
                Some(true)
            }
            K::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super | NamedKey::Meta) => return false,
            _ => Some(false),
        };
        let Some(tab) = self.tabs.get_mut(i) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        let link = t.link_ask.take();
        if let (Some(true), Some(l)) = (action, link) {
            self.open_link(i, &l.url);
        }
        self.play_event("toggle");
        self.dirty = true;
        true
    }

    /// The band over the shell while a link waits on you.
    pub(crate) fn draw_link_band(&mut self, scene: &mut Scene, p: &TermPane, r: Rect, hh: f32) {
        let Some(link) = p.link_ask.as_ref() else { return };
        let t = self.theme.clone();
        let strong = self.label_strong();
        let label = self.label();
        let drop = self.band_anim.value();
        let bh = self.header_h();
        let br = Rect::new(r.x, r.y + hh - (1.0 - drop) * bh, r.w, bh);
        scene.layer(Some(Rect::new(r.x, r.y + hh, r.w, bh)));
        scene.rect(br, t.ink);
        let inv = Style { color: t.paper, ..strong };
        let inv_l = Style { color: t.paper, ..label };
        let by = br.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
        let mut x = br.x + self.px(m::HEADER_PAD_X);
        let isz = self.px(13.0);
        self.fonts.draw_icon(scene, nus_render::text::icons::GLOBE, isz, x, br.y + (bh - isz) / 2.0, t.paper);
        x += isz + self.px(8.0);
        x += self.fonts.draw(scene, inv, x, by, &host(&link.url).to_uppercase()) + self.px(10.0);
        x += self.fonts.draw(scene, inv_l, x, by, "OPEN?") + self.px(14.0);
        x += self.fonts.draw(scene, inv, x, by, "ENTER") + self.px(14.0);
        x += self.fonts.draw(scene, inv_l, x, by, "· ESC CANCELS ·") + self.px(10.0);
        x += self.fonts.draw(scene, inv, x, by, "D") + self.px(6.0);
        let _ = self.fonts.draw(scene, inv_l, x, by, "NEVER ASK AGAIN");
        let _ = fade(t.ink, 1.0);
        scene.layer(None);
        if drop < 1.0 {
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_schemes() {
        assert_eq!(normalize("localhost:8000/x"), "http://localhost:8000/x");
        assert_eq!(normalize("https://a.b/c"), "https://a.b/c");
        assert_eq!(host("https://github.com/x/y"), "github.com");
        assert_eq!(host("localhost:5173/"), "localhost:5173");
    }
}
