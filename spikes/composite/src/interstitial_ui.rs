//! The app's side of interstitials (interstitial.rs): running what a
//! transcript asked for, and drawing the overlays nus puts over a live
//! page — hung, waking, a microphone macOS refuses, a blocked file — in
//! the same transcript form as the pages.

use nus_render::text::Style;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, Pane, WebPane};
use crate::interstitial::{Kind, Page, Sev};

/// The waking transcript stands until the woken page has painted.
pub(crate) fn settle_waking(w: &mut WebPane) {
    let mut s = w.tab.shared.borrow_mut();
    let waking = s.overlay.as_ref().is_some_and(|o| o.kind == Kind::Sleep && o.acts.is_empty());
    if waking && s.bind.is_some() && !s.loading {
        s.overlay = None;
        s.paints += 1;
    }
}

impl App {
    fn web_pane_by_id(&mut self, id: u64, right: bool) -> Option<&mut WebPane> {
        let tab = self.tabs.iter_mut().find(|t| t.id == id)?;
        match if right { tab.right.as_mut()? } else { &mut tab.left } {
            Pane::Web(w) => Some(w),
            _ => None,
        }
    }

    /// A command from a transcript (the page's, or an overlay's).
    pub(crate) fn interstitial_act(&mut self, id: u64, right: bool, verb: &str) {
        let Some(w) = self.web_pane_by_id(id, right) else { return };
        let (page, failed) = {
            let s = w.tab.shared.borrow();
            (s.interstitial.clone().or_else(|| s.overlay.clone()), s.failed_url.is_some())
        };
        let Some(page) = page else { return };
        let host = crate::interstitial::host(&page.url);
        let clear_overlay = |w: &mut WebPane| {
            let mut s = w.tab.shared.borrow_mut();
            s.overlay = None;
            s.paints += 1;
        };
        match verb {
            "back" if w.tab.can_go_back() => w.tab.back(),
            "back" | "close" => {
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.activate(i);
                    self.close_tabs(true);
                }
            }
            // A load that failed goes again as it was; a crashed or refused
            // page loads fresh.
            "retry" if failed => w.tab.reload(),
            "retry" => w.tab.load(&page.url),
            "resubmit" => w.tab.reload(),
            "proceed" if page.kind == Kind::Cert => {
                crate::interstitial::allow(format!("cert:{host}"));
                w.tab.reload();
            }
            "proceed" if page.kind == Kind::Malware => {
                crate::interstitial::allow(format!("site:{host}"));
                w.tab.load(&page.url);
            }
            "wait" | "stop" => w.tab.answer_hung(verb == "wait"),
            "dismiss" | "wake" => clear_overlay(w),
            "keep" => {
                crate::interstitial::allow(format!("file:{}", page.url));
                clear_overlay(w);
                w.tab.download(&page.url);
            }
            "downloads" => {
                clear_overlay(w);
                self.open_downloads();
            }
            "open-portal" => self.open_url("http://captive.apple.com/", true),
            "sleep-idle" => {
                let n = self.sleep_idle_tabs();
                self.notice(nus_render::text::icons::MOON, "Tabs Asleep", format!("{n} idle {}", if n == 1 { "tab" } else { "tabs" }));
            }
            "settings:date-time" => crate::app::open_with_os(std::path::Path::new(if cfg!(windows) { "ms-settings:dateandtime" } else { "x-apple.systempreferences:com.apple.Date-Time-Settings.extension" })),
            "settings:privacy" => {
                let camera = page.head.contains("camera");
                crate::app::open_with_os(std::path::Path::new(match (cfg!(windows), camera) {
                    (true, true) => "ms-settings:privacy-webcam",
                    (true, false) => "ms-settings:privacy-microphone",
                    (false, true) => "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera",
                    (false, false) => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
                }));
            }
            v if v.starts_with("open:") => w.tab.load(&v[5..]),
            _ => {}
        }
        self.dirty = true;
    }

    /// Every idle page but this tab's, asleep now (the out-of-memory page).
    pub(crate) fn sleep_idle_tabs(&mut self) -> usize {
        let media_window = self.pip.is_some() || self.little.is_some();
        let mut n = 0;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == self.active || tab.pinned || media_window {
                continue;
            }
            if let Pane::Web(w) = &mut tab.left {
                if w.asleep.is_none() && w.hands.ask.is_none() && w.reader.is_none() && w.tab.can_suspend() {
                    let url = w.tab.shared.borrow().url.clone();
                    if !url.is_empty() && !url.starts_with("about:") {
                        w.asleep = Some(url);
                        w.slept = Some(crate::clock::now());
                        w.tab.suspend();
                        n += 1;
                    }
                }
            }
        }
        n
    }

    /// Keys while an overlay stands over the focused page: ↑ ↓ move, ↵
    /// runs, Esc goes back to the page when that's one of the commands.
    pub(crate) fn overlay_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let (id, right) = (tab.id, tab.focus_right && tab.right.is_some());
        let Some(w) = self.web_pane_by_id(id, right) else { return false };
        let Some(page) = w.tab.shared.borrow().overlay.clone() else { return false };
        let n = page.acts.len().max(1);
        if !matches!(ev.logical_key, Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp | NamedKey::Enter | NamedKey::Escape)) {
            return false;
        }
        let verb = match &ev.logical_key {
            Key::Named(NamedKey::ArrowDown) => { w.overlay_sel = (w.overlay_sel + 1) % n; None }
            Key::Named(NamedKey::ArrowUp) => { w.overlay_sel = (w.overlay_sel + n - 1) % n; None }
            Key::Named(NamedKey::Enter) => page.acts.get(w.overlay_sel.min(n - 1)).map(|a| a.verb.clone()),
            Key::Named(NamedKey::Escape) => page.acts.iter().find(|a| a.verb == "dismiss").map(|a| a.verb.clone()),
            _ => None,
        };
        if let Some(v) = verb {
            self.interstitial_act(id, right, &v);
        }
        self.dirty = true;
        true
    }

    /// A click on an overlay's command.
    pub(crate) fn overlay_click(&mut self, x: f32, y: f32) -> bool {
        for tab in &self.tabs {
            let id = tab.id;
            for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                let Pane::Web(w) = p else { continue };
                if w.tab.shared.borrow().overlay.is_none() || !w.page.contains(x, y) {
                    continue;
                }
                let verb = w.overlay_hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, v)| v.clone());
                if let Some(v) = verb {
                    self.interstitial_act(id, right, &v);
                }
                // The page beneath gets nothing while the overlay stands.
                return true;
            }
        }
        false
    }

    /// The overlay, over the page it's about.
    pub(crate) fn draw_overlay(&mut self, scene: &mut Scene, w: &mut WebPane) {
        w.overlay_hits.clear();
        let Some(page) = w.tab.shared.borrow().overlay.clone() else { return };
        if page.kind == Kind::Sleep && page.acts.is_empty() {
            // Waking: how long it slept, while the page comes back — once
            // it's clear the page won't be back in a blink.
            if w.woke.is_some_and(|t| crate::clock::since(t) < std::time::Duration::from_millis(300)) {
                return;
            }
            let asleep = w.slept.map(crate::clock::since).unwrap_or_default();
            let p = Page::sleep(&page.url, asleep, true);
            self.draw_transcript(scene, w.page, &p, w.overlay_sel, &mut w.overlay_hits);
            return;
        }
        w.overlay_sel = w.overlay_sel.min(page.acts.len().saturating_sub(1));
        self.draw_transcript(scene, w.page, &page, w.overlay_sel, &mut w.overlay_hits);
    }

    /// A transcript, drawn: the same page the HTML shows.
    pub(crate) fn draw_transcript(&mut self, scene: &mut Scene, r: Rect, page: &Page, sel: usize, hits: &mut Vec<(Rect, String)>) {
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        scene.rect(r, paper);
        scene.layer(Some(r));
        let ui = self.ui();
        let strong = self.ui_strong();
        let dim = Style { color: t.dim, ..ui };
        let label = Style { color: t.dim, ..self.label() };
        let line_h = self.px(22.0);
        let pad_x = self.px(if r.w < 560.0 * self.scale { 18.0 } else { 48.0 });
        let x0 = r.x + pad_x;
        let tx = x0 + self.px(3.0) + self.px(22.0);
        let width = (r.right() - pad_x - tx).min(self.px(820.0)).max(self.px(120.0));
        let mut y = r.y + self.px(44.0) + line_h * 0.7;
        let top = y - line_h * 0.7;
        let prompt = Style { color: self.surface.signal, ..ui };
        let pw = self.fonts.draw(scene, prompt, tx, y, "»") + self.fonts.measure(ui, " ");
        for (k, l) in crate::reader::wrap(&self.fonts, ui, &page.command, width - pw).iter().enumerate() {
            self.fonts.draw(scene, ui, tx + pw, y + k as f32 * line_h, l);
            y += if k > 0 { line_h } else { 0.0 };
        }
        y += line_h;
        for (i, l) in page.log.iter().enumerate() {
            // Detail lines keep their columns; only an overlong one wraps.
            let lines = if self.fonts.measure(ui, l) <= width { vec![l.clone()] } else { crate::reader::wrap(&self.fonts, ui, l, width) };
            for w in lines {
                self.fonts.draw(scene, if i == 0 { strong } else { ui }, tx, y, &w);
                y += line_h;
            }
        }
        y += line_h * 0.8;
        for w in crate::reader::wrap(&self.fonts, strong, &page.head, width) {
            self.fonts.draw(scene, strong, tx, y, &w);
            y += line_h;
        }
        for w in crate::reader::wrap(&self.fonts, ui, &page.body, width.min(self.px(68.0 * 8.4))) {
            self.fonts.draw(scene, ui, tx, y, &w);
            y += line_h;
        }
        if !page.acts.is_empty() {
            y += line_h * 0.8;
            self.fonts.draw(scene, label, tx, y, "NEXT");
            y += line_h;
            let verb_w = page.acts.iter().map(|a| self.fonts.measure(ui, &format!("» {}", a.verb))).fold(self.fonts.measure(ui, &"x".repeat(18)), f32::max);
            for (k, a) in page.acts.iter().enumerate() {
                let row = Rect::new(tx - self.px(6.0), y - line_h * 0.72, width + self.px(12.0), line_h);
                let hot = row.contains(self.mouse.0, self.mouse.1);
                let on = k == sel;
                if on {
                    scene.rect(row, ink);
                } else if hot {
                    scene.outline(row, self.px(m::HAIRLINE), ink);
                }
                let c = if on { self.on_fill(ink) } else if a.unsafe_ { t.dim } else { ink };
                self.fonts.draw(scene, Style { color: c, ..ui }, tx, y, &format!("» {}", a.verb));
                let words = if a.key.is_empty() { a.label.clone() } else { format!("{} · {}", a.label, a.key) };
                self.fonts.draw(scene, Style { color: if on { c } else { t.dim }, ..dim }, tx + verb_w + self.px(18.0), y, &words);
                hits.push((row, a.verb.clone()));
                y += line_h;
            }
        } else {
            // Nothing to choose: the caret, waiting with it.
            y += line_h * 0.8;
            let pw = self.fonts.draw(scene, prompt, tx, y, "»") + self.fonts.measure(ui, " ");
            let cw = self.fonts.measure(ui, "x");
            scene.rect(Rect::new(tx + pw, y - ui.px * 0.8, cw, ui.px), fade(ink, 0.8));
            y += line_h;
        }
        // The rule down the left: signal for danger, ink for a problem.
        let rule = match page.sev { Sev::Danger => Some(self.surface.signal), Sev::Problem => Some(ink), Sev::Rest => None };
        if let Some(c) = rule {
            scene.rect(Rect::new(x0, top, self.px(3.0), y - top - line_h * 0.3), c);
        }
        scene.layer(None);
    }
}
