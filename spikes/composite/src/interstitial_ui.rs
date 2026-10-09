//! The app's side of interstitials (interstitial.rs): running what a
//! transcript asked for, and drawing the overlays nus puts over a live
//! page — hung, waking, a microphone macOS refuses, a blocked file — in
//! the same transcript form as the pages.

use nus_render::text::Style;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, Pane, WebPane};
use crate::interstitial::{Break, Kind, Live, Mark, Page, Route, Sev};
use nus_render::Color;
use std::f32::consts::{PI, TAU};

/// The route walks a segment in this (ms, before the motion register), the
/// break arrives in this, and a live break loops once in this.
pub(crate) const ROUTE_SEG: f32 = 120.0;
pub(crate) const ROUTE_POP: f32 = 220.0;
pub(crate) const ROUTE_LOOP: f32 = 1400.0;

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// A filled circle.
fn disc(scene: &mut Scene, cx: f32, cy: f32, r: f32, c: Color) {
    let pts: Vec<[f32; 2]> = (0..40).map(|i| { let a = i as f32 / 40.0 * TAU; [cx + r * a.cos(), cy + r * a.sin()] }).collect();
    scene.poly(&pts, c);
}

/// A band of width `w` inside the ellipse (rx, ry), from angle `a0` to `a1`.
#[allow(clippy::too_many_arguments)]
fn arc(scene: &mut Scene, cx: f32, cy: f32, rx: f32, ry: f32, w: f32, a0: f32, a1: f32, c: Color) {
    let n = (((a1 - a0).abs() / TAU) * 48.0).ceil().max(4.0) as usize;
    let at = |i: usize| a0 + (a1 - a0) * i as f32 / n as f32;
    let mut pts: Vec<[f32; 2]> = (0..=n).map(|i| [cx + rx * at(i).cos(), cy + ry * at(i).sin()]).collect();
    pts.extend((0..=n).rev().map(|i| [cx + (rx - w) * at(i).cos(), cy + (ry - w) * at(i).sin()]));
    scene.poly(&pts, c);
}

/// A circle's outline, `w` thick inward; dashed in `dashes` when nonzero.
fn ring(scene: &mut Scene, cx: f32, cy: f32, r: f32, w: f32, dashes: usize, c: Color) {
    if dashes == 0 {
        // Two halves, overlapping a little: no seam where they meet.
        arc(scene, cx, cy, r, r, w, -0.05, PI + 0.05, c);
        arc(scene, cx, cy, r, r, w, PI, TAU + 0.05, c);
    } else {
        for k in 0..dashes {
            let a0 = k as f32 / dashes as f32 * TAU;
            arc(scene, cx, cy, r, r, w, a0, a0 + TAU / dashes as f32 * 0.55, c);
        }
    }
}

/// A straight stroke `w` thick, any angle.
fn stroke(scene: &mut Scene, x1: f32, y1: f32, x2: f32, y2: f32, w: f32, c: Color) {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (nx, ny) = (-dy / len * w / 2.0, dx / len * w / 2.0);
    scene.poly(&[[x1 + nx, y1 + ny], [x2 + nx, y2 + ny], [x2 - nx, y2 - ny], [x1 - nx, y1 - ny]], c);
}

/// A dashed horizontal stroke from `a` to `a + len`.
fn dashed(scene: &mut Scene, a: f32, b: f32, y: f32, w: f32, on: f32, off: f32, c: Color) {
    let mut x = a;
    while x < b {
        scene.rect(Rect::new(x, y - w / 2.0, on.min(b - x), w), c);
        x += on + off;
    }
}

/// The highlighted command starts at a new transcript's default: a choice
/// made on the page before it (↓ to retry) must not carry to this one.
pub(crate) fn follow_page(w: &mut WebPane, page: &Page) {
    if w.overlay_page != page.token {
        // The same way as the page before (a stuck page, then stopped): it
        // isn't walked again; only a new break arrives.
        let way = |r: &Route| (r.stations.clone(), r.at);
        w.overlay_skip = match (&w.overlay_route, &page.route) {
            (Some(a), Some(b)) if way(a) == way(b) && a.mark == b.mark => 2,
            (Some(a), Some(b)) if way(a) == way(b) => 1,
            _ => 0,
        };
        w.overlay_page = page.token.clone();
        w.overlay_sel = page.acts.iter().position(|a| !a.unsafe_).unwrap_or(0);
        w.overlay_at = crate::clock::now();
        w.overlay_route = page.route.clone();
    }
}

/// A refused local port's page: what last served it, from ports that
/// remember, and the commands to start it again or watch for it. Once.
pub(crate) fn tell_last_on_port(w: &mut WebPane, remembered: &[crate::ports::Remembered]) {
    let mut s = w.tab.shared.borrow_mut();
    let Some(p) = s.interstitial.as_mut() else { return };
    let refused = p.trace.iter().any(|st| st.mark == Mark::Fail && st.what.get(1).is_some_and(|c| c == "ERR_CONNECTION_REFUSED"));
    let told = p.acts.iter().any(|a| a.verb == "watch") || p.notes.iter().any(|n| n.label == "Watching");
    if p.kind != Kind::Unreachable || !refused || told || !crate::interstitial::is_local(&p.url) {
        return;
    }
    let port = crate::interstitial::port_of(&p.url);
    let now = crate::journal::now();
    let last = remembered.iter().find(|m| m.port == port).map(|m| {
        let home = std::env::var("HOME").unwrap_or_default();
        let cwd = if !home.is_empty() && m.cwd.starts_with(&home) { format!("~{}", &m.cwd[home.len()..]) } else { m.cwd.clone() };
        (m.process.clone(), m.command.clone(), cwd, std::time::Duration::from_secs(now.saturating_sub(m.last_seen)))
    });
    p.last_on_port(last.as_ref().map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), *d)));
    s.rewrite = true;
    s.paints += 1;
}

/// The waking transcript stands until the woken page paints a document
/// of its own — its first paint, not the end of its load: the rest of
/// the page arrives in view, the way a page you opened does.
pub(crate) fn settle_waking(w: &mut WebPane) {
    let mut s = w.tab.shared.borrow_mut();
    let waking = s.overlay.as_ref().is_some_and(|o| o.kind == Kind::Sleep && o.acts.is_empty());
    if waking && s.bind.is_some() && (s.painted_committed || !s.loading) {
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
            (s.transcript().or(s.interstitial.as_ref()).cloned(), s.failed_url.is_some())
        };
        let Some(page) = page else { return };
        // Dispatch only actions offered by the current native transcript. In
        // particular, a stale action must not activate a hidden security choice.
        if page.kind != Kind::Index && !page.acts.iter().any(|action| action.verb == verb) { return; }
        match verb {
            // The pages whose renderer ended with this one, reloaded with it.
            "retry-all" => {
                let at = w.tab.shared.borrow().ended.as_ref().map(|e| e.at);
                let others = at.map(|at| self.ended_with(at, id, right)).unwrap_or_default();
                self.interstitial_act(id, right, "retry");
                for (other, on_right) in others {
                    self.interstitial_act(other, on_right, "retry");
                }
                return;
            }
            "details" => {
                let text = page.details(&format!("{} ({})", env!("NUS_BUILD_VERSION"), env!("NUS_BUILD_REVISION")));
                match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
                    Ok(()) => self.notice(nus_render::text::icons::COPY, "Copied Diagnostics", crate::interstitial::host(&page.url)),
                    Err(_) => self.notice_problem("Could Not Copy", "clipboard unavailable"),
                }
                self.dirty = true;
                return;
            }
            // Its saved command, again, in a new shell: never typed into an
            // existing one (see ports' RUN SAVED COMMAND). The page waits for
            // the port and loads when it answers.
            "start" => {
                let port = crate::interstitial::port_of(&page.url);
                let Some(m) = self.board.remembered.iter().find(|m| m.port == port).cloned() else { return };
                let profile = self.behavior.default_profile;
                match self.new_term_pane_at(false, profile, Some(m.cwd.clone())) {
                    Ok(mut t) => {
                        t.type_at_prompt = Some(format!("{}\r", m.command));
                        t.type_origin = Some(crate::finish_work::Origin::NusAction);
                        let tab = self.make_tab(Pane::Term(t), None);
                        let at = self.add_tab(tab);
                        let said = format!("{} started in tab {}", m.process, self.tab_label(at));
                        if let Some(w) = self.web_pane_by_id(id, right) {
                            w.tab.watch_port(Some(said));
                        }
                    }
                    Err(_) => self.notice_problem("Could Not Open Terminal", format!("for port {port}")),
                }
                self.dirty = true;
                return;
            }
            _ => {}
        }
        if verb == "retry" && w.tab.browser.is_none() {
            let container = w.container.clone();
            if let Some(replacement) = self.new_web_pane_in(&page.url, &container) {
                if let Some(w) = self.web_pane_by_id(id, right) { *w = replacement; }
            }
            self.layout();
            self.dirty = true;
            return;
        }
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
            "wait" | "stop" if page.kind == Kind::Slow => w.tab.answer_slow(verb == "wait"),
            "wait" | "stop" => w.tab.answer_hung(verb == "wait"),
            "watch" => w.tab.watch_port(None),
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

    /// Pages whose renderer ended within a moment of `at`, other than this one.
    fn ended_with(&self, at: std::time::Instant, id: u64, right: bool) -> Vec<(u64, bool)> {
        let mut out = Vec::new();
        for tab in &self.tabs {
            for (on_right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                let Pane::Web(w) = p else { continue };
                if (tab.id, on_right) == (id, right) {
                    continue;
                }
                let s = w.tab.shared.borrow();
                let together = s.ended.as_ref().is_some_and(|e| e.at.max(at).saturating_duration_since(e.at.min(at)) < std::time::Duration::from_secs(2));
                if together && s.interstitial.as_ref().is_some_and(|p| matches!(p.kind, Kind::Crash | Kind::Oom)) {
                    out.push((tab.id, on_right));
                }
            }
        }
        out
    }

    /// Every idle page but this tab's, asleep now (the out-of-memory page).
    pub(crate) fn sleep_idle_tabs(&mut self) -> usize {
        let media_window = self.pip.is_some() || self.little.is_some() || self.docked.is_some();
        let working: Vec<bool> = (0..self.tabs.len()).map(|i| self.tab_working(i)).collect();
        let mut n = 0;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == self.active || tab.pinned || media_window || working[i] {
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
        let Some(page) = w.tab.shared.borrow().transcript().cloned() else { return false };
        follow_page(w, &page);
        let n = page.acts.len().max(1);
        let dialog = page.kind == Kind::Dialog;
        let action_tab = page.fields.is_empty()
            && matches!(ev.logical_key, Key::Named(NamedKey::Tab))
            && !mods.control_key() && !mods.super_key() && !mods.alt_key();
        let other_mods = mods.alt_key() || (mods.control_key() && cfg!(target_os = "macos"));
        // Preserve application shortcuts, but Shift by itself is ordinary
        // typing. A blocking transcript must also consume shifted characters.
        let application_chord = mods.control_key() || mods.super_key() || mods.alt_key();
        if (!dialog && application_chord) || other_mods {
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
        if action_tab {
            w.overlay_sel = (w.overlay_sel + if back { n - 1 } else { 1 }) % n;
            self.dirty = true;
            return true;
        }
        if !matches!(ev.logical_key, Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp | NamedKey::Enter | NamedKey::Escape)) {
            // Blocking native transcripts, not only dialogs, own ordinary keys.
            // Do not type into a web page that the user cannot currently see.
            return !application_chord;
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
        if let Some(tab) = self.tabs.get(self.active) {
            let id = tab.id;
            for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                let Pane::Web(w) = p else { continue };
                if w.tab.shared.borrow().transcript().is_none() || !w.page.contains(x, y) {
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
        let Some(page) = w.tab.shared.borrow().transcript().cloned() else { return };
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
            let age = crate::clock::since(w.overlay_at).as_secs_f32();
            self.draw_transcript(scene, w.page, &p, w.overlay_sel, &mut w.overlay_hits, (age, w.overlay_skip));
            return;
        }
        follow_page(w, &page);
        w.overlay_sel = w.overlay_sel.min(page.acts.len().saturating_sub(1));
        let age = crate::clock::since(w.overlay_at).as_secs_f32();
        self.draw_transcript(scene, w.page, &page, w.overlay_sel, &mut w.overlay_hits, (age, w.overlay_skip));
    }

    /// The route, nus to page, walked from `age` 0 to the station where it
    /// broke, the break arriving there, and a loop while it is still going
    /// on. Lowercase names under the stations. The same geometry as the
    /// page's own (`interstitial::ROUTE_JS`). Returns the height drawn.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_route(&mut self, scene: &mut Scene, x0: f32, top: f32, w: f32, route: &Route, danger: bool, age: f32, skip: u8) -> f32 {
        let t = self.theme.clone();
        let (ink, dim, hot) = (t.ink, t.dim, self.surface.signal);
        let paper = { let p = self.paper(); [p[0], p[1], p[2], 1.0] };
        let faint = fade(ink, 0.18);
        let u = self.px(1.0);
        // The route keeps its proportions; a narrow pane squeezes the spacing only.
        let n = route.stations.len();
        let pad = 14.0 * u;
        let y = top + 34.0 * u;
        let height = 92.0 * u;
        let xs: Vec<f32> = (0..n).map(|i| x0 + pad + i as f32 * (w - 2.0 * pad) / (n - 1) as f32).collect();
        let reduced = self.motion.reduced();
        let seg = if skip >= 1 { 0.0 } else { self.motion.dur(ROUTE_SEG) };
        let pop = if skip >= 2 { 0.0 } else { self.motion.dur(ROUTE_POP) };
        let age = if reduced { f32::INFINITY } else { age };
        let at = route.at;
        let fail = at as f32 * seg;
        let walked = |i: usize| if seg <= 0.0 { 1.0 } else { ((age - i as f32 * seg) / seg).clamp(0.0, 1.0) };
        let arrived = if pop <= 0.0 { 1.0 } else { ease_out((age - fail) / pop) };
        // The way: walked in ink, the rest faint.
        for i in 0..n - 1 {
            let (a, b) = (xs[i] + 8.0 * u, xs[i + 1] - 8.0 * u);
            if i + 1 > at {
                dashed(scene, a, b, y, 1.5 * u, 4.0 * u, 4.0 * u, faint);
                continue;
            }
            let p = walked(i);
            let end = a + (b - a) * p;
            match route.mark {
                Break::Gap if i + 1 == at => dashed(scene, a, end, y, 4.0 * u, 7.0 * u, 6.0 * u, ink),
                Break::Glass if i + 1 == at => dashed(scene, a, end, y, 4.0 * u, 2.0 * u, 5.0 * u, ink),
                Break::Cut if i + 1 == at => {
                    let m = (a + b) / 2.0;
                    scene.rect(Rect::new(a, y - 2.0 * u, ((m - 8.0 * u) - a) * p, 4.0 * u), ink);
                    let c = fade(hot, arrived);
                    for o in [-6.0, 2.0] {
                        scene.poly(&[[m + o * u, y + 11.0 * u], [m + (o + 4.0) * u, y + 11.0 * u], [m + (o + 8.0) * u, y - 11.0 * u], [m + (o + 4.0) * u, y - 11.0 * u]], c);
                    }
                }
                _ => scene.rect(Rect::new(a, y - 2.0 * u, end - a, 4.0 * u), ink),
            }
        }
        // Stations: reached ones fill as the walk passes them.
        for (i, &x) in xs.iter().enumerate() {
            if i == at {
                continue;
            }
            let name = route.stations[i];
            let s = if name == "page" { 16.0 } else { 12.0 } * u;
            let boxed = name == "page" || name == "nus";
            if i < at {
                let shown = if i == 0 { 1.0 } else { walked(i - 1) };
                if shown <= 0.0 {
                    continue;
                }
                let c = fade(ink, shown);
                if boxed { scene.rect(Rect::new(x - s / 2.0, y - s / 2.0, s, s), c); } else { disc(scene, x, y, 6.5 * u * (0.6 + 0.4 * shown), c); }
            } else if boxed {
                scene.outline(Rect::new(x - s / 2.0, y - s / 2.0, s, s), 2.0 * u, faint);
            } else {
                ring(scene, x, y, 6.0 * u, 2.0 * u, 0, faint);
            }
        }
        // The break, arriving: growing into place.
        let x = xs[at];
        let px = if at > 0 { xs[at - 1] } else { x };
        let k = u * (0.6 + 0.4 * arrived);
        let (ink_a, hot_a) = (fade(ink, arrived), fade(hot, arrived));
        let pulse = matches!(route.live, Live::Ending) && age > fail + pop && !reduced;
        let hot_m = if pulse { fade(hot, 0.35 + 0.65 * (0.5 + 0.5 * ((age - fail - pop) / (ROUTE_LOOP / 1000.0) * TAU).cos())) } else { hot_a };
        if arrived > 0.0 {
            match route.mark {
                Break::Unknown => ring(scene, x, y, 9.0 * k, 3.5 * k, 8, hot_a),
                Break::Wall => scene.rect(Rect::new(x - 4.0 * u, y - 17.0 * k, 8.0 * u, 34.0 * k), hot_a),
                Break::Gap => ring(scene, x, y, 7.0 * k, 3.0 * k, 0, hot_a),
                Break::Cut => ring(scene, x, y, 6.0 * u, 2.0 * u, 0, faint),
                Break::Detour => {
                    disc(scene, x, y, 6.5 * u, ink_a);
                    scene.rect(Rect::new(x - 2.0 * u, y + 7.0 * u, 4.0 * u, 19.0 * k), hot_a);
                    scene.rect(Rect::new(x - 8.0 * u, y + 7.0 * u + 19.0 * k, 16.0 * u, 10.0 * u), hot_a);
                }
                Break::Loop => {
                    disc(scene, x, y, 6.5 * u, hot_a);
                    // Back over the way it came, from this station to the one before.
                    let cx = (x + px) / 2.0;
                    arc(scene, cx, y - 10.0 * u, (x - px) / 2.0 + 1.75 * u, 20.0 * k, 3.5 * u, PI, TAU, hot_a);
                    scene.poly(&[[px - 6.0 * u, y - 14.0 * u], [px + 6.0 * u, y - 14.0 * u], [px, y - 6.0 * u]], hot_a);
                }
                Break::Glass => {
                    scene.poly(&[[x - 9.0 * k, y - 12.0 * k], [x + 9.0 * k, y - 12.0 * k], [x, y]], hot_a);
                    for (a, b) in [([x, y], [x - 9.0 * k, y + 12.0 * k]), ([x - 9.0 * k, y + 12.0 * k], [x + 9.0 * k, y + 12.0 * k]), ([x + 9.0 * k, y + 12.0 * k], [x, y])] {
                        stroke(scene, a[0], a[1], b[0], b[1], 2.5 * u, hot_a);
                    }
                }
                Break::Empty => scene.outline(Rect::new(x - 8.0 * k, y - 8.0 * k, 16.0 * k, 16.0 * k), 3.5 * u, hot_a),
                Break::Seal => {
                    scene.poly(&[[x - 1.5 * u, y - 12.0 * k], [x - 13.0 * k, y], [x - 1.5 * u, y + 12.0 * k]], ink_a);
                    scene.poly(&[[x + 1.5 * u, y - 12.0 * k], [x + 13.0 * k, y], [x + 1.5 * u, y + 12.0 * k]], hot_a);
                }
                Break::Hazard => {
                    let sq = Rect::new(x - 9.0 * k, y - 9.0 * k, 18.0 * k, 18.0 * k);
                    scene.rect(sq, hot_a);
                    let parent = scene.clip();
                    scene.layer(Some(parent.map_or(sq, |c| c.intersect(&sq))));
                    for o in [-18.0, -10.0, -2.0, 6.0, 14.0] {
                        stroke(scene, x + (o - 9.0) * k, y + 9.0 * k, x + (o + 9.0) * k, y - 9.0 * k, 4.0 * k, ink_a);
                    }
                    scene.layer(parent);
                }
                Break::Ban => {
                    ring(scene, x, y, 10.0 * k, 3.5 * k, 0, ink_a);
                    stroke(scene, x - 7.0 * k, y + 7.0 * k, x + 7.0 * k, y - 7.0 * k, 3.5 * k, hot_a);
                }
                Break::File => {
                    disc(scene, x, y, 6.5 * u, ink_a);
                    scene.rect(Rect::new(x - 2.0 * u, y + 7.0 * u, 4.0 * u, 13.0 * u), ink_a);
                    scene.rect(Rect::new(x - 7.0 * k, y + 22.0 * u, 14.0 * k, 12.0 * k), hot_a);
                }
                Break::Stuck => {
                    scene.outline(Rect::new(x - 10.0 * k, y - 10.0 * k, 20.0 * k, 20.0 * k), 2.5 * u, ink_a);
                    ring(scene, x, y, 5.0 * k, 2.5 * u, 0, ink_a);
                }
                Break::Split => {
                    scene.poly(&[[x - 10.0 * k, y - 10.0 * k], [x + 6.0 * k, y - 10.0 * k], [x - 10.0 * k, y + 6.0 * k]], ink_a);
                    scene.poly(&[[x + 10.0 * k, y - 6.0 * k], [x + 10.0 * k, y + 10.0 * k], [x - 6.0 * k, y + 10.0 * k]], hot_a);
                }
                Break::Overflow => {
                    scene.outline(Rect::new(x - 8.0 * k, y - 8.0 * k, 16.0 * k, 16.0 * k), 2.5 * u, ink_a);
                    scene.rect(Rect::new(x - 5.0 * k, y - 5.0 * k, 10.0 * k, 10.0 * k), ink_a);
                    scene.rect(Rect::new(x - 10.0 * k, y - 15.0 * k, 20.0 * k, 4.0 * u), hot_a);
                }
                Break::Stop => scene.rect(Rect::new(x - 9.0 * k, y - 9.0 * k, 18.0 * k, 18.0 * k), hot_m),
                Break::Moon => {
                    disc(scene, x, y, 9.0 * k, ink_a);
                    disc(scene, x + 4.0 * k, y - 3.0 * k, 7.5 * k, paper);
                }
                Break::Clock => {
                    ring(scene, x, y, 9.0 * k, 3.0 * u, 0, hot_a);
                    scene.rect(Rect::new(x - 1.25 * u, y - 6.0 * k, 2.5 * u, 6.0 * k), hot_a);
                    scene.rect(Rect::new(x, y - 1.25 * u, 5.0 * k, 2.5 * u), hot_a);
                    scene.outline(Rect::new(xs[0] - 7.0 * u, y - 7.0 * u, 14.0 * u, 14.0 * u), 2.5 * u, hot_a);
                }
                Break::Device => {
                    scene.rect(Rect::new(x - 8.0 * u, y - 8.0 * u, 16.0 * u, 16.0 * u), ink_a);
                    scene.rect(Rect::new(x - 2.0 * u, y + 9.0 * u, 4.0 * u, 13.0 * u), ink_a);
                    scene.rect(Rect::new(x - 8.0 * k, y + 22.0 * u, 16.0 * k, 10.0 * k), hot_a);
                }
                Break::Uturn => {
                    disc(scene, x, y, 6.5 * u, ink_a);
                    scene.rect(Rect::new(x + 9.0 * u, y - 1.75 * u, 11.0 * u, 3.5 * u), hot_a);
                    scene.rect(Rect::new(x + 18.25 * u, y - 1.75 * u, 3.5 * u, 19.75 * u), hot_a);
                    scene.rect(Rect::new(x - 4.0 * u, y + 16.25 * u, 25.75 * u, 3.5 * u), hot_a);
                    scene.poly(&[[x - 2.0 * u, y + 12.0 * u], [x - 2.0 * u, y + 24.0 * u], [x - 10.0 * u, y + 18.0 * u]], hot_a);
                }
            }
        }
        // What is still going on there: a loop, while it lasts.
        let looping = !reduced && route.live != Live::Still && age > fail + pop;
        if looping {
            let phase = ((age - fail - pop) / (ROUTE_LOOP / 1000.0)).fract();
            match route.live {
                Live::InFlight => {
                    let (a, b) = (px + 8.0 * u, x - 14.0 * u);
                    let cx = a + (b - a) * phase;
                    scene.rect(Rect::new(cx - 3.0 * u, y - 3.0 * u, 6.0 * u, 6.0 * u), ink);
                }
                Live::Polling => {
                    let (a, b) = (px + 8.0 * u, x - 12.0 * u);
                    let there = 1.0 - (2.0 * phase - 1.0).abs();
                    disc(scene, a + (b - a) * (0.5 - 0.5 * (there * PI).cos()), y, 4.0 * u, ink);
                }
                Live::Spinning => {
                    let a = phase * TAU - PI / 2.0;
                    let (bx, by) = (x + 5.5 * u * a.cos(), y + 5.5 * u * a.sin());
                    scene.rect(Rect::new(bx - 3.0 * u, by - 3.0 * u, 6.0 * u, 6.0 * u), hot);
                }
                Live::Ending | Live::Still => {}
            }
        } else if route.live == Live::Spinning && arrived >= 1.0 {
            // Still, the bead stays where the loop stopped: at the top.
            scene.rect(Rect::new(x - 3.0 * u, y - 8.5 * u, 6.0 * u, 6.0 * u), hot);
        }
        // The stations, named, in lowercase.
        let small = Style { px: self.px(12.0), color: dim, ..self.ui() };
        for (i, (&x, name)) in xs.iter().zip(&route.stations).enumerate() {
            let style = if i == at { Style { color: if danger { hot } else { ink }, ..Style { px: self.px(12.0), ..self.ui_strong() } } } else { small };
            let tw = self.fonts.measure(style, name);
            self.fonts.draw(scene, style, x - tw / 2.0, top + height - 8.0 * u, name);
        }
        if looping || (!reduced && age < fail + pop) {
            self.dirty = true;
        }
        height
    }

    /// A transcript, drawn: the same page the HTML shows.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_transcript(&mut self, scene: &mut Scene, r: Rect, page: &Page, sel: usize, hits: &mut Vec<(Rect, String)>, (age, skip): (f32, u8)) {
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let paper = [paper[0], paper[1], paper[2], 1.0];
        let parent_clip = scene.clip();
        let clip = parent_clip.map_or(r, |parent| parent.intersect(&r));
        scene.layer(Some(clip));
        // A browser failure must not reveal stale site pixels through a theme's
        // transparent paper. Draw the native surface within the enclosing clip.
        scene.rect(r, paper);
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
        for l in crate::reader::wrap(&self.fonts, ui, &page.command, width - pw) {
            self.fonts.draw(scene, ui, tx + pw, y, &l);
            y += line_h;
        }
        // Where on its way the page failed: a rule, the route, its stations.
        if let Some(route) = &page.route {
            let top = y - line_h * 0.3;
            scene.rect(Rect::new(tx, top, width, self.px(m::STRUCTURE)), ink);
            let h = self.draw_route(scene, tx, top + self.px(6.0), width, route, page.sev == Sev::Danger, age, skip);
            scene.rect(Rect::new(tx, top + self.px(6.0) + h, width, self.px(1.0)), fade(ink, 0.14));
            y = top + self.px(6.0) + h + line_h * 1.2;
        }
        for (i, l) in page.log.iter().enumerate() {
            // Detail lines keep their columns; only an overlong one wraps.
            let lines = if self.fonts.measure(ui, l) <= width { vec![l.clone()] } else { crate::reader::wrap(&self.fonts, ui, l, width) };
            for w in lines {
                self.fonts.draw(scene, if i == 0 { strong } else { ui }, tx, y, &w);
                y += line_h;
            }
        }
        // With a trace, the verdict is the headline: a size up, right under the command.
        let traced = !page.trace.is_empty();
        let head = if traced { Style { px: strong.px * 16.0 / 13.0, ..strong } } else { strong };
        y += if page.log.is_empty() { line_h * 0.3 } else { line_h * 0.8 };
        for w in crate::reader::wrap(&self.fonts, head, &page.head, width) {
            self.fonts.draw(scene, head, tx, y, &w);
            y += line_h * if traced { 1.15 } else { 1.0 };
        }
        for w in crate::reader::wrap(&self.fonts, ui, &page.body, width.min(self.px(68.0 * 8.4))) {
            self.fonts.draw(scene, ui, tx, y, &w);
            y += line_h;
        }
        // The trace: a ruled row a step, the failing one marked by the rule.
        if traced {
            y += line_h * 0.8;
            self.fonts.draw(scene, label, tx, y, "TRACE");
            y += line_h * 0.5;
            let edge = fade(ink, 0.14);
            let rule_color = if page.sev == Sev::Danger { self.surface.signal } else { ink };
            let pad = self.px(4.0);
            let name_w = self.px(96.0);
            let time_w = self.px(72.0);
            let mark_w = self.px(16.0);
            let gap = self.px(14.0);
            let what_x = tx + self.px(12.0) + name_w + gap;
            let what_w = (tx + width - mark_w - gap - time_w - gap - what_x).max(self.px(80.0));
            for st in &page.trace {
                let fail = st.mark == Mark::Fail;
                scene.rect(Rect::new(tx, y, width, self.px(1.0)), edge);
                let top = y;
                y += pad + line_h * 0.72;
                let first = y;
                let name_style = if fail { Style { px: self.px(12.0), color: ink, ..strong } } else { Style { px: self.px(12.0), color: t.dim, ..ui } };
                self.fonts.draw(scene, name_style, tx + self.px(12.0), y, &st.name);
                for (i, line) in st.what.iter().enumerate() {
                    let style = if i == 0 { if fail { strong } else { ui } } else { dim };
                    for w in crate::reader::wrap(&self.fonts, style, line, what_w) {
                        self.fonts.draw(scene, style, what_x, y, &w);
                        y += line_h;
                    }
                }
                if !st.time.is_empty() {
                    let tw = self.fonts.measure(dim, &st.time);
                    self.fonts.draw(scene, dim, tx + width - mark_w - gap - tw, first, &st.time);
                }
                // The mark, drawn as nus's own icons: Plex Mono has no ✓.
                let size = ui.px * 0.95;
                let icon = match st.mark {
                    Mark::Ok => Some(nus_render::text::icons::CHECK),
                    Mark::Fail => Some(nus_render::text::icons::CLOSE),
                    Mark::Skip => Some(nus_render::text::icons::MINUS),
                    Mark::Wait | Mark::Fact => None,
                };
                if let Some(icon) = icon {
                    let color = if st.mark == Mark::Skip { t.dim } else { ink };
                    self.fonts.draw_icon(scene, icon, size, tx + width - size, first - size * 0.82, color);
                } else if st.mark == Mark::Wait {
                    let gw = self.fonts.measure(ui, "…");
                    self.fonts.draw(scene, ui, tx + width - gw, first, "…");
                }
                y += pad - line_h * 0.72;
                if fail {
                    scene.rect(Rect::new(tx, top, self.px(3.0), y - top), rule_color);
                }
            }
            scene.rect(Rect::new(tx, y, width, self.px(1.0)), edge);
            y += line_h * 0.72;
        }
        for note in &page.notes {
            y += line_h * 0.8;
            self.fonts.draw(scene, label, tx, y, &note.label.to_uppercase());
            y += line_h;
            for (i, line) in note.lines.iter().enumerate() {
                let style = if i == 0 { ui } else { dim };
                for w in crate::reader::wrap(&self.fonts, style, line, width) {
                    self.fonts.draw(scene, style, tx, y, &w);
                    y += line_h;
                }
            }
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
                    self.draw_line_caret(scene, vx + vw + self.px(1.0), y, ui.px, 1.0, self.last_key);
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
                let target = row.intersect(&clip);
                if target.w > 0.0 && target.h > 0.0 {
                    hits.push((target, a.verb.clone()));
                }
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
        // The rule down the left: signal for danger, ink for a problem. A
        // traced page carries it on the step that failed instead.
        let rule = match page.sev { _ if traced => None, Sev::Danger => Some(self.surface.signal), Sev::Problem => Some(ink), Sev::Rest => None };
        if let Some(c) = rule {
            scene.rect(Rect::new(x0, top, self.px(3.0), y - top - line_h * 0.3), c);
        }
        scene.layer(parent_clip);
    }
}
