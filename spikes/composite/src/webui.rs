//! The page's chrome that isn't the page: find in page, the permission
//! band, <select> popups composited over the page, the downloads list in
//! the footer, sleeping and archiving of idle tabs, and history for the
//! address palette.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use winit::event::ElementState;

use crate::app::{Caps, hover_key, App, IconMotion, Pane, WebPane};
use nus_render::theme::metric as m;

/// Find in page state on a web pane.
#[derive(Clone, Debug, Default)]
pub struct Find {
    pub query: String,
}

impl App {
    /// Ctrl+Shift+F: find in whichever pane has focus.
    pub(crate) fn search_open(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        match tab.focused() {
            Pane::Term(_) => self.term_search_open(),
            Pane::Web(w) => {
                w.find = Some(Find::default());
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// Keys while a page's find band is up. Returns true when consumed.
    pub(crate) fn web_mode_key(&mut self, key: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key as WKey, NamedKey};
        if key.state != ElementState::Pressed {
            return false;
        }
        let shift = self.mods.shift_key();
        let mods = self.mods;
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let Pane::Web(w) = tab.focused() else { return false };
        let Some(f) = w.find.as_mut() else { return false };
        // The query's own editing: typing, erasing, paste (field.rs).
        let took = crate::field::edit(&mut f.query, key, mods, 400);
        if took.changed() {
            if f.query.is_empty() {
                w.tab.stop_find();
            } else {
                w.tab.find(&f.query.clone(), true, false);
            }
        }
        match &key.logical_key {
            _ if took.taken() => {}
            WKey::Named(NamedKey::Escape) => {
                w.find = None;
                w.tab.stop_find();
            }
            WKey::Named(NamedKey::Enter) => {
                if !f.query.is_empty() {
                    w.tab.find(&f.query.clone(), !shift, true);
                }
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// The find band, the permission band, the select popup and the site
    /// panel, over a page.
    pub(crate) fn draw_web_overlays(&mut self, scene: &mut Scene, w: &mut WebPane) {
        self.draw_swipe(scene, w);
        self.draw_site_panel(scene, w);
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let page = w.page;
        // <select> and friends: the popup widget's texture at its rect.
        let (select_bind, select_rect, shown) = {
            let s = w.tab.shared.borrow();
            (s.select.bind.clone(), s.select.rect, s.select.shown)
        };
        if shown {
            if let Some(b) = select_bind {
                let sc = self.scale;
                let r = Rect::new(page.x + select_rect.0 as f32 * sc, page.y + select_rect.1 as f32 * sc, select_rect.2 as f32 * sc, select_rect.3 as f32 * sc);
                scene.texture(r, b, Some(page));
                scene.layer(None);
            }
        }
        // Permission band: "<origin> asks for camera · ALLOW · DENY".
        w.perm_hits.clear();
        let ask = {
            let s = w.tab.shared.borrow();
            s.permission.as_ref().map(|a| (a.origin.clone(), a.what.clone()))
        };
        if let Some((origin, what)) = ask {
            let bh = self.header_h();
            let br = Rect::new(page.x, page.y, page.w, bh);
            scene.rect(br, ink);
            let inv = Style { color: t.paper, ..strong };
            let inv_l = Style { color: t.paper, ..label };
            let by = br.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let host = crate::sites::permission_origin(&origin).unwrap_or(origin).caps();
            let mut x = br.x + self.px(m::HEADER_PAD_X);
            x += self.fonts.draw(scene, inv, x, by, &host) + self.px(10.0);
            x += self.fonts.draw(scene, inv_l, x, by, &format!("ASKS FOR {}", what.caps())) + self.px(18.0);
            for (word, allow) in [("ALLOW", true), ("DENY", false)] {
                let ww = self.fonts.measure(inv, word) + self.px(20.0);
                let chip = Rect::new(x, br.y + self.px(6.0), ww, bh - self.px(12.0));
                scene.outline(chip, self.px(m::HAIRLINE), t.paper);
                if allow {
                    scene.rect(Rect::new(chip.x + self.px(1.0), chip.y + self.px(1.0), chip.w - self.px(2.0), chip.h - self.px(2.0)), self.surface.signal);
                }
                self.fonts.draw(scene, inv, x + self.px(10.0), by, word);
                w.perm_hits.push((chip, allow));
                x += ww + self.px(8.0);
            }
        }
        self.draw_hands(scene, w);
        // Dedupe band: this page is open in another tab. Two icon chips.
        w.dedupe_hits.clear();
        if let Some((here, _)) = w.dedupe {
            let bh = self.header_h();
            let br = Rect::new(page.x, page.y, page.w, bh);
            scene.rect(br, crate::surface::mix(t.paper, ink, 0.08));
            scene.hline(br.x, br.bottom() - self.px(m::HAIRLINE), br.w, self.px(m::HAIRLINE), ink);
            let by = br.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = br.x + self.px(m::HEADER_PAD_X);
            let label_there = &w.dedupe_label;
            x += self.fonts.draw(scene, strong, x, by, &format!("ALREADY OPEN IN {label_there}")) + self.px(18.0);
            let isz = self.px(14.0);
            let (mx, my) = self.mouse;
            for (k, icon, words, switch) in [(0usize, nus_render::text::icons::TO_TAB, "switch there, close this one", true), (1, nus_render::text::icons::CLOSE, "keep both", false)] {
                let hit = Rect::new(x - self.px(6.0), br.y + self.px(4.0), isz + self.px(12.0), bh - self.px(8.0));
                let hot = hit.contains(mx, my);
                self.icon_button(scene, icon, isz, x, br.y + (bh - isz) / 2.0, if switch { self.surface.signal } else { ink }, hit, crate::app::hover_key("dedupe", here * 10 + k), crate::app::IconMotion::Pop);
                if hot {
                    self.tip_words(hit, words);
                }
                w.dedupe_hits.push((hit, switch));
                x += isz + self.px(18.0);
            }
        }
        // Find band, below any permission band.
        if let Some(f) = &w.find {
            let bh = self.header_h();
            let y = page.y + if w.perm_hits.is_empty() { 0.0 } else { bh };
            let br = Rect::new(page.x, y, page.w, bh);
            scene.rect(br, ink);
            let inv = Style { color: t.paper, ..strong };
            let inv_l = Style { color: t.paper, ..label };
            let by = br.y + self.px(m::HEADER_PAD_Y) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = br.x + self.px(m::HEADER_PAD_X);
            x += self.fonts.draw(scene, inv_l, x, by, "FIND") + self.px(12.0);
            let q = if f.query.is_empty() { "…".to_string() } else { f.query.clone() };
            x += self.fonts.draw(scene, Style { font: self.f.ui, px: self.px(m::UI_PX), color: t.paper, tracking: 0.0 }, x, by, &q);
            scene.rect(Rect::new(x + self.px(2.0), by - self.px(11.0), self.px(1.5), self.px(14.0)), t.paper);
            let found = w.tab.shared.borrow().find;
            let count = match found {
                Some((n, _)) if n == 0 && !f.query.is_empty() => "NO MATCHES".to_string(),
                Some((n, k)) if n > 0 => format!("{k} OF {n}"),
                _ => String::new(),
            };
            let keys = "ENTER NEXT · SHIFT+ENTER BACK · ESC";
            let kw = self.fonts.measure(inv_l, keys);
            self.fonts.draw(scene, inv_l, br.right() - self.px(m::HEADER_PAD_X) - kw, by, keys);
            let cw = self.fonts.measure(inv, &count);
            self.fonts.draw(scene, inv, br.right() - self.px(m::HEADER_PAD_X) - kw - self.px(14.0) - cw, by, &count);
        }
    }

    /// A click on a dedupe band's chip. Returns true when it was one.
    pub(crate) fn dedupe_click(&mut self, x: f32, y: f32) -> bool {
        let active = self.active;
        let Some(tab) = self.tabs.get(active) else { return false };
        let mut act: Option<crate::app::Action> = None;
        for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
            let Pane::Web(w) = p else { continue };
            let Some((here, there)) = w.dedupe else { continue };
            if let Some((_, switch)) = w.dedupe_hits.iter().find(|(r, _)| r.contains(x, y)) {
                act = Some(if *switch { crate::app::Action::DedupeSwitch(here, there) } else { crate::app::Action::DedupeKeep(tab.id) });
            }
        }
        match act {
            Some(a) => {
                self.run(a);
                true
            }
            None => false,
        }
    }

    /// A click on a permission band's chip. Returns true when it was one.
    pub(crate) fn web_band_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let Pane::Web(w) = p else { continue };
            if let Some(&(_, allow)) = w.perm_hits.iter().find(|(r, _)| r.contains(x, y)) {
                if let Some(ask) = w.tab.shared.borrow().permission.as_ref() {
                    for word in ask.what.split(" and ") {
                        crate::sites::remember(&ask.origin, word.trim(), allow);
                    }
                }
                w.tab.answer_permission(allow);
                self.play_event("toggle");
                self.dirty = true;
                return true;
            }
        }
        false
    }

    /// Idle tabs: sleep pages after a while (blank them, keep the URL),
    /// archive them into "recently closed" after longer. Never the active
    /// tab, never a pinned one, never a shell.
    pub(crate) fn tend_idle_tabs(&mut self) {
        if crate::clock::since(self.last_tend).as_secs() < 5 {
            return;
        }
        self.last_tend = crate::clock::now();
        let sleep_after = self.behavior.sleep_after_min;
        let archive_after = self.behavior.archive_after_h;
        let mut archive: Vec<usize> = Vec::new();
        let kept: Vec<String> = self.folders.iter().filter(|f| f.kind == crate::folders::Kind::Plain).flat_map(|f| f.items.iter().map(|i| i.url.clone())).collect();
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == self.active || tab.pinned {
                continue;
            }
            // A page kept in a folder never archives.
            if matches!(&tab.left, Pane::Web(w) if kept.contains(&w.tab.shared.borrow().url)) {
                continue;
            }
            let idle_min = crate::clock::since(tab.last_active).as_secs_f32() / 60.0;
            if archive_after > 0 && idle_min >= archive_after as f32 * 60.0 && matches!(tab.left, Pane::Web(_)) && tab.right.is_none() {
                archive.push(i);
                continue;
            }
            if sleep_after > 0 && idle_min >= sleep_after as f32 {
                if let Pane::Web(w) = &mut tab.left {
                    if w.asleep.is_none() {
                        let url = w.tab.shared.borrow().url.clone();
                        if !url.is_empty() && !url.starts_with("about:") {
                            w.asleep = Some(url);
                            w.tab.load("about:blank");
                        }
                    }
                }
            }
        }
        if !archive.is_empty() {
            for &i in archive.iter().rev() {
                if self.tabs.len() <= 1 {
                    break;
                }
                let tab = self.tabs.remove(i);
                if let Pane::Web(w) = &tab.left {
                    let url = w.asleep.clone().unwrap_or_else(|| w.tab.shared.borrow().url.clone());
                    self.closed.push(crate::app::Closed::Web(url));
                }
                self.tab_removed(i);
            }
            self.dirty = true;
        }
    }

    /// A sleeping page wakes when it's shown.
    pub(crate) fn wake_tab(&mut self, i: usize) {
        if let Some(tab) = self.tabs.get_mut(i) {
            tab.last_active = crate::clock::now();
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Web(w) = p {
                    if let Some(url) = w.asleep.take() {
                        w.tab.load(&url);
                    }
                }
            }
        }
    }

    /// History rows for the palette: pages visited, most and most recently first.
    pub(crate) fn history_rows(&self, q: &str, new_tab: bool, limit: usize) -> Vec<crate::app::PaletteRow> {
        let q = q.trim().to_lowercase();
        let mut hits: Vec<(&crate::start::Recent, &str, &str)> = self
            .recent
            .iter()
            .filter_map(|r| match &r.item {
                crate::start::Saved::Page { url, title } => Some((r, url.as_str(), title.as_str())),
                _ => None,
            })
            .filter(|(_, url, title)| q.is_empty() || url.to_lowercase().contains(&q) || title.to_lowercase().contains(&q))
            .collect();
        // Visits weigh more than recency; a prefix match on the host beats both.
        hits.sort_by(|a, b| {
            let score = |(r, url, _): &(&crate::start::Recent, &str, &str)| {
                let host = url.split("//").nth(1).unwrap_or(url).trim_start_matches("www.");
                let prefix = if !q.is_empty() && host.starts_with(&q) { 1000 } else { 0 };
                prefix + r.visits as i64 * 10 + (r.when as i64 / 3600).min(500_000)
            };
            score(b).cmp(&score(a))
        });
        hits.into_iter()
            .take(limit)
            .map(|(r, url, title)| {
                let host = url.split("//").nth(1).unwrap_or(url).split('/').next().unwrap_or("").trim_start_matches("www.");
                let text = if title.is_empty() { url.to_string() } else { format!("{title} · {host}") };
                let action = if new_tab { crate::app::Action::NewBrowser(url.to_string()) } else { crate::app::Action::OpenInPane(url.to_string()) };
                crate::app::PaletteRow { num: format!("{}×", r.visits.max(1)), text, action }
            })
            .collect()
    }
}

pub fn human_bytes(n: i64) -> String {
    let n = n.max(0) as f64;
    if n < 1024.0 {
        format!("{} B", n as i64)
    } else if n < 1024.0 * 1024.0 {
        format!("{:.0} KB", n / 1024.0)
    } else if n < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", n / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", n / 1024.0 / 1024.0 / 1024.0)
    }
}
