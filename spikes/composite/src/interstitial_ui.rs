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
            // The page's own question, or a sign-in: answered, and gone.
            "ok" | "leave" | "reload" | "signin" if page.kind == Kind::Dialog => w.tab.answer_dialog(true, &page.fields),
            "cancel" | "stay" if page.kind == Kind::Dialog => w.tab.answer_dialog(false, &page.fields),
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
    /// runs, ⌘↵ runs the command marked so, Esc goes back to the page (or
    /// answers no). An overlay that asks for words takes typing; Tab moves
    /// between its lines.
    pub(crate) fn overlay_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if let Some(taken) = self.page_dialog_key(ev) {
            return taken;
        }
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let chord = if cfg!(target_os = "macos") { self.mods.super_key() } else { self.mods.control_key() };
        let back = self.mods.shift_key();
        let mods = self.mods;
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let (id, right) = (tab.id, tab.focus_right && tab.right.is_some());
        let Some(w) = self.web_pane_by_id(id, right) else { return false };
        let Some(page) = w.tab.shared.borrow().overlay.clone() else { return false };
        let n = page.acts.len().max(1);
        let dialog = page.kind == Kind::Dialog;
        let other_mods = mods.alt_key() || (mods.control_key() && cfg!(target_os = "macos"));
        // Only a dialog answers to modifiers, and only these; the rest pass.
        if (!mods.is_empty() && !dialog) || other_mods {
            return false;
        }
        let paste = chord && matches!(&ev.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"));
        if chord && !paste && !matches!(ev.logical_key, Key::Named(NamedKey::Enter)) {
            return false;
        }
        if paste && !page.fields.is_empty() {
            let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
            let text: String = text.lines().next().unwrap_or("").to_string();
            let mut s = w.tab.shared.borrow_mut();
            if let Some(o) = s.overlay.as_mut() {
                let k = o.field.min(o.fields.len() - 1);
                o.fields[k].value.push_str(&text);
            }
            s.paints += 1;
            drop(s);
            self.dirty = true;
            return true;
        }
        if !page.fields.is_empty() && !chord {
            let edited = {
                let mut s = w.tab.shared.borrow_mut();
                let Some(o) = s.overlay.as_mut() else { return false };
                let k = o.field.min(o.fields.len() - 1);
                let changed = match &ev.logical_key {
                    Key::Named(NamedKey::Backspace) => { o.fields[k].value.pop(); true }
                    Key::Named(NamedKey::Tab) => { o.field = (k + if back { o.fields.len() - 1 } else { 1 }) % o.fields.len(); true }
                    Key::Named(NamedKey::Enter | NamedKey::Escape | NamedKey::ArrowUp | NamedKey::ArrowDown) => false,
                    _ => match ev.text.as_deref().filter(|t| !t.chars().any(char::is_control)) {
                        Some(t) => { o.fields[k].value.push_str(t); true }
                        None => false,
                    },
                };
                if changed { s.paints += 1; }
                changed
            };
            if edited {
                self.dirty = true;
                return true;
            }
        }
        if !matches!(ev.logical_key, Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp | NamedKey::Enter | NamedKey::Escape)) {
            // While a page's question stands, its keys are the dialog's:
            // nothing typed reaches the page underneath.
            return dialog && mods.is_empty();
        }
        let verb = match &ev.logical_key {
            Key::Named(NamedKey::ArrowDown) => { w.overlay_sel = (w.overlay_sel + 1) % n; None }
            Key::Named(NamedKey::ArrowUp) => { w.overlay_sel = (w.overlay_sel + n - 1) % n; None }
            Key::Named(NamedKey::Enter) if chord => page.acts.iter().find(|a| a.key == "⌘↵").map(|a| a.verb.clone()),
            Key::Named(NamedKey::Enter) => page.acts.get(w.overlay_sel.min(n - 1)).map(|a| a.verb.clone()),
            Key::Named(NamedKey::Escape) => page.acts.iter().find(|a| a.key == "Esc" || matches!(a.verb.as_str(), "dismiss" | "cancel" | "stay")).map(|a| a.verb.clone()),
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
        // A page's question or a sign-in: only the page held faint here; the
        // sheet hangs from the strip (page_dialog.rs), where a page can't draw.
        if page.kind == Kind::Dialog {
            self.draw_dialog_scrim(scene, w);
            return;
        }
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
        // Lines to type on: the label, then what's there, a caret on the one
        // being typed into. A secret one shows a dot a character.
        if !page.fields.is_empty() {
            y += line_h * 0.8;
            let lw = page.fields.iter().map(|f| self.fonts.measure(ui, &f.label)).fold(0.0, f32::max) + self.px(18.0);
            for (k, f) in page.fields.iter().enumerate() {
                let on = k == page.field.min(page.fields.len() - 1);
                self.fonts.draw(scene, Style { color: t.dim, ..ui }, tx, y, &f.label);
                let shown = if f.secret { "•".repeat(f.value.chars().count()) } else { f.value.clone() };
                let pw = self.fonts.draw(scene, prompt, tx + lw, y, "»") + self.fonts.measure(ui, " ");
                let vx = tx + lw + pw;
                let vw = self.fonts.draw(scene, ui, vx, y, &shown);
                if on {
                    scene.rect(Rect::new(vx + vw + self.px(1.0), y - ui.px * 0.8, self.px(2.0), ui.px), ink);
                }
                let rule = Rect::new(tx + lw, y + line_h * 0.28, width - lw, self.px(1.0));
                scene.rect(rule, if on { ink } else { fade(ink, 0.25) });
                y += line_h * 1.3;
            }
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
