//! Start: the modal that greets a launch — the icon's band completes one
//! orbit as the panel rises — with the last session to restore and recent
//! pages and shells to pick from. Esc starts fresh. The planet icon in the
//! header calls it up later. Also: the session and recent files it reads,
//! and the optional startup sound (off by default).

use std::time::Instant;

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style, Theme};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, NamedKey};

use crate::anim::{base, Anim};
use crate::app::{App, Pane};

// ── Session and recent ───────────────────────────────────────────────────

/// One tab as saved: enough to bring it back.
#[derive(Clone, Debug, PartialEq)]
pub enum Saved {
    Shell { profile: String },
    Page { url: String, title: String },
}

#[derive(Clone, Debug, Default)]
pub struct SavedTab {
    pub left: Option<Saved>,
    pub right: Option<Saved>,
    pub pinned: bool,
    /// Index of the parent tab in the session, for stacks.
    pub parent: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub item: Saved,
    pub when: u64,
}

fn profile_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn saved_to_json(s: &Saved) -> serde_json::Value {
    match s {
        Saved::Shell { profile } => serde_json::json!({ "kind": "shell", "profile": profile }),
        Saved::Page { url, title } => serde_json::json!({ "kind": "page", "url": url, "title": title }),
    }
}

fn saved_from_json(v: &serde_json::Value) -> Option<Saved> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    match v.get("kind").and_then(|k| k.as_str())? {
        "shell" => Some(Saved::Shell { profile: s("profile") }),
        "page" => Some(Saved::Page { url: s("url"), title: s("title") }),
        _ => None,
    }
}

impl Session {
    pub fn load() -> Option<Session> {
        let text = std::fs::read_to_string(profile_dir().join("session.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let tabs = v
            .get("tabs")?
            .as_array()?
            .iter()
            .map(|t| SavedTab {
                left: t.get("left").and_then(saved_from_json),
                right: t.get("right").and_then(saved_from_json),
                pinned: t.get("pinned").and_then(|p| p.as_bool()).unwrap_or(false),
                parent: t.get("parent").and_then(|p| p.as_u64()).map(|p| p as usize),
            })
            .collect();
        Some(Session { tabs, active: v.get("active").and_then(|a| a.as_u64()).unwrap_or(0) as usize })
    }

    pub fn save(&self) {
        let tabs: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .map(|t| {
                serde_json::json!({
                    "left": t.left.as_ref().map(saved_to_json),
                    "right": t.right.as_ref().map(saved_to_json),
                    "pinned": t.pinned,
                    "parent": t.parent,
                })
            })
            .collect();
        let v = serde_json::json!({ "tabs": tabs, "active": self.active, "saved": now() });
        let _ = std::fs::create_dir_all(profile_dir());
        let _ = std::fs::write(profile_dir().join("session.json"), serde_json::to_string_pretty(&v).unwrap_or_default());
    }

    pub fn summary(&self) -> String {
        let shells = self.tabs.iter().filter(|t| matches!(t.left, Some(Saved::Shell { .. }))).count();
        let pages = self.tabs.iter().filter(|t| matches!(t.left, Some(Saved::Page { .. }))).count()
            + self.tabs.iter().filter(|t| matches!(t.right, Some(Saved::Page { .. }))).count();
        let mut parts = Vec::new();
        if shells > 0 {
            parts.push(format!("{shells} shell{}", if shells == 1 { "" } else { "s" }));
        }
        if pages > 0 {
            parts.push(format!("{pages} page{}", if pages == 1 { "" } else { "s" }));
        }
        parts.join(" · ")
    }
}

pub fn load_recent() -> Vec<Recent> {
    let Ok(text) = std::fs::read_to_string(profile_dir().join("recent.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| Some(Recent { item: saved_from_json(r)?, when: r.get("when").and_then(|w| w.as_u64()).unwrap_or(0) }))
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_recent(list: &[Recent]) {
    let v: Vec<serde_json::Value> = list
        .iter()
        .map(|r| {
            let mut j = saved_to_json(&r.item);
            j["when"] = serde_json::json!(r.when);
            j
        })
        .collect();
    let _ = std::fs::create_dir_all(profile_dir());
    let _ = std::fs::write(profile_dir().join("recent.json"), serde_json::to_string(&v).unwrap_or_default());
}

// ── The modal ────────────────────────────────────────────────────────────

pub struct Start {
    pub input: String,
    pub sel: usize,
    pub opened: Instant,
    pub rise: Anim,
    /// Row rects from the last frame.
    pub rows: Vec<Rect>,
}

#[derive(Clone, Debug)]
pub enum StartRow {
    Restore,
    Recent(Saved),
    Fresh,
}

impl App {
    /// Rows the modal offers, filtered by what is typed.
    fn start_rows(&self) -> Vec<(StartRow, String, String)> {
        let q = self.start.as_ref().map(|s| s.input.to_lowercase()).unwrap_or_default();
        let hit = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
        let mut rows = Vec::new();
        if let Some(sess) = &self.last_session {
            if !sess.tabs.is_empty() && hit("restore last session") {
                rows.push((StartRow::Restore, "restore last session".to_string(), sess.summary()));
            }
        }
        for r in &self.recent {
            let (title, detail) = match &r.item {
                Saved::Shell { profile } => (profile.clone(), "shell".to_string()),
                Saved::Page { url, title } => {
                    let host = url.split("//").nth(1).unwrap_or(url).split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
                    (if title.is_empty() { host.clone() } else { title.clone() }, host)
                }
            };
            if hit(&title) || hit(&detail) {
                rows.push((StartRow::Recent(r.item.clone()), title, detail));
            }
            if rows.len() >= 9 {
                break;
            }
        }
        rows.push((StartRow::Fresh, "start fresh".to_string(), "a new shell".to_string()));
        rows
    }

    pub fn open_start(&mut self) {
        self.palette = None;
        self.start = Some(Start { input: String::new(), sel: 0, opened: Instant::now(), rise: Anim::at(0.0), rows: Vec::new() });
        if let Some(s) = self.start.as_mut() {
            s.rise.replay(0.0, 1.0, self.motion.dur(base::PALETTE) * 2.5);
        }
        self.dirty = true;
    }

    fn start_commit(&mut self) {
        let Some(s) = self.start.as_ref() else { return };
        let rows = self.start_rows();
        let Some((row, _, _)) = rows.get(s.sel) else { return };
        let row = row.clone();
        self.start = None;
        match row {
            StartRow::Restore => self.restore_session(),
            StartRow::Recent(Saved::Page { url, .. }) => self.open_url(&url, true),
            StartRow::Recent(Saved::Shell { profile }) => {
                let idx = self.profiles.iter().position(|p| p.name == profile).unwrap_or(self.behavior.default_profile);
                self.new_tab(idx);
            }
            StartRow::Fresh => {}
        }
        self.dirty = true;
    }

    /// Bring the saved tabs back: shells restart on their profile, pages
    /// reload, stacks and pins are kept. Scrollback restore is v1.
    fn restore_session(&mut self) {
        let Some(sess) = self.last_session.clone() else { return };
        let mut ids: Vec<Option<u64>> = Vec::new();
        for t in &sess.tabs {
            let left = match &t.left {
                Some(Saved::Shell { profile }) => {
                    let idx = self.profiles.iter().position(|p| &p.name == profile).unwrap_or(self.behavior.default_profile);
                    self.new_term_pane(false, idx).ok().map(Pane::Term)
                }
                Some(Saved::Page { url, .. }) => self.new_web_pane(url).map(Pane::Web),
                None => None,
            };
            let Some(left) = left else {
                ids.push(None);
                continue;
            };
            let right = match &t.right {
                Some(Saved::Page { url, .. }) => self.new_web_pane(url).map(Pane::Web),
                Some(Saved::Shell { profile }) => {
                    let idx = self.profiles.iter().position(|p| &p.name == profile).unwrap_or(self.behavior.default_profile);
                    self.new_term_pane(false, idx).ok().map(Pane::Term)
                }
                None => None,
            };
            let mut tab = self.make_tab(left, right);
            tab.pinned = t.pinned;
            if let Some(p) = t.parent.and_then(|p| ids.get(p).copied().flatten()) {
                tab.parent = Some(p);
            }
            ids.push(Some(tab.id));
            self.tabs.push(tab);
        }
        let n = self.tabs.len();
        if n > 0 {
            let first_new = n - ids.iter().filter(|i| i.is_some()).count();
            self.activate((first_new + sess.active).min(n - 1));
        }
        self.layout();
    }

    /// Save the current tabs as the session (called when tabs change).
    pub(crate) fn save_session(&self) {
        let saved = |p: &Pane| match p {
            Pane::Term(t) => Some(Saved::Shell { profile: self.profiles.get(t.profile).map(|p| p.name.clone()).unwrap_or_default() }),
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                Some(Saved::Page { url: s.url.clone(), title: s.title.clone() })
            }
            _ => None,
        };
        let index_of = |id: u64| self.tabs.iter().position(|t| t.id == id);
        let tabs: Vec<SavedTab> = self
            .tabs
            .iter()
            .map(|t| SavedTab { left: saved(&t.left), right: t.right.as_ref().and_then(saved), pinned: t.pinned, parent: t.parent.and_then(index_of) })
            .filter(|t| t.left.is_some())
            .collect();
        Session { tabs, active: self.active }.save();
    }

    /// Remember a page or shell in the recent list (deduped, newest first).
    pub(crate) fn remember(&mut self, item: Saved) {
        if let Saved::Page { url, .. } = &item {
            if url.is_empty() || url.starts_with("http://127.0.0.1:9229") {
                return;
            }
        }
        self.recent.retain(|r| match (&r.item, &item) {
            (Saved::Page { url: a, .. }, Saved::Page { url: b, .. }) => a != b,
            (Saved::Shell { profile: a }, Saved::Shell { profile: b }) => a != b,
            _ => true,
        });
        self.recent.insert(0, Recent { item, when: now() });
        self.recent.truncate(200);
        save_recent(&self.recent);
    }

    pub fn start_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        let Some(s) = self.start.as_mut() else { return false };
        if ev.state != ElementState::Pressed {
            return true;
        }
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => self.start = None,
            WKey::Named(NamedKey::Enter) => self.start_commit(),
            WKey::Named(NamedKey::Backspace) => {
                s.input.pop();
                s.sel = 0;
            }
            WKey::Named(NamedKey::ArrowDown) => s.sel += 1,
            WKey::Named(NamedKey::ArrowUp) => s.sel = s.sel.saturating_sub(1),
            WKey::Named(NamedKey::Space) => s.input.push(' '),
            WKey::Character(c) if !self.mods.control_key() && !self.mods.super_key() && c.chars().all(|ch| !ch.is_control()) => {
                s.input.push_str(c);
                s.sel = 0;
            }
            _ => {}
        }
        self.dirty = true;
        true
    }

    pub fn start_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        let Some(s) = self.start.as_ref() else { return false };
        if state != ElementState::Pressed || button != MouseButton::Left {
            return true;
        }
        if let Some(i) = s.rows.iter().position(|r| r.contains(x, y)) {
            if let Some(st) = self.start.as_mut() {
                st.sel = i;
            }
            self.start_commit();
        } else {
            self.start = None;
        }
        self.dirty = true;
        true
    }

    /// Draw the modal: masthead with the orbiting band, the typed line,
    /// the rows, and a foot with the keys.
    pub fn draw_start(&mut self, scene: &mut Scene) {
        let Some(st) = self.start.as_ref() else { return };
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let t: Theme = self.theme.clone();
        let ink = t.ink;
        let rise = st.rise.value();
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let ui_strong = self.ui_strong();
        let dim = Style { color: t.dim, ..label };
        let rows = self.start_rows();
        let sel = st.sel.min(rows.len().saturating_sub(1));

        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), Theme::with_alpha(t.scrim, t.scrim[3] * rise));
        let pw = self.px(560.0).min(w - 2.0 * self.px(16.0));
        let head_h = self.px(56.0);
        let line_h = self.px(14.0) * 2.0 + self.px(16.0) + self.px(2.0);
        let row_h = self.px(10.0) * 2.0 + self.px(m::UI_PX) + self.px(m::HAIRLINE);
        let foot_h = self.px(8.0) * 2.0 + self.px(m::LABEL_PX) + self.px(2.0);
        let bh = head_h + line_h + row_h * rows.len() as f32 + foot_h;
        let bx = ((w - pw) / 2.0).round();
        let by = ((h - bh) * 0.38).round() + (1.0 - rise) * self.px(14.0);
        let r = Rect::new(bx, by, pw, bh);
        scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), ink);
        scene.rect(r, t.paper);
        scene.outline(r, self.px(m::FLOATING), ink);

        // Header: the planet, the word, and what last time held.
        let isz = self.px(18.0);
        let hx = r.x + self.px(18.0);
        self.fonts.draw_icon(scene, icons::PLANET, isz, hx, r.y + (head_h - isz) / 2.0, self.surface.signal);
        let big = Style { font: self.f.wordmark, px: self.px(26.0), color: ink, tracking: 0.0 };
        let ww = self.fonts.draw(scene, big, hx + isz + self.px(12.0), r.y + head_h / 2.0 + self.px(9.0), "atlas");
        let greet = match &self.last_session {
            Some(s) if !s.tabs.is_empty() => format!("LAST TIME · {}", s.summary().to_uppercase()),
            _ => "LAST SESSION · RECENT PAGES AND SHELLS".to_string(),
        };
        let gx = hx + isz + self.px(12.0) + ww + self.px(16.0);
        self.fonts.draw(scene, dim, gx, r.y + head_h / 2.0 + self.px(4.0), &self.fit(dim, &greet, r.right() - self.px(18.0) - gx));
        scene.hline(r.x, r.y + head_h - self.px(2.0), r.w, self.px(2.0), ink);

        // Typed line.
        let ly = r.y + head_h;
        let base_y = ly + self.px(14.0) + self.px(16.0);
        let big_in = Style { font: self.f.ui, px: self.px(16.0), color: ink, tracking: 0.0 };
        let shown = if st.input.is_empty() { "type to filter · Enter opens · Esc starts fresh".to_string() } else { st.input.clone() };
        let st_in = if st.input.is_empty() { Style { color: t.dim, ..big_in } } else { big_in };
        let tw = self.fonts.draw(scene, st_in, r.x + self.px(18.0), base_y, &shown);
        if !st.input.is_empty() {
            scene.rect(Rect::new(r.x + self.px(18.0) + tw + self.px(2.0), base_y - self.px(14.0), self.px(9.0), self.px(18.0)), ink);
        }
        scene.hline(r.x, ly + line_h - self.px(2.0), r.w, self.px(2.0), ink);

        // Rows.
        let mut y = ly + line_h;
        let mut rects = Vec::new();
        for (i, (row, title, detail)) in rows.iter().enumerate() {
            let on = i == sel;
            let (fg, bg) = if on { (t.paper, Some(ink)) } else { (ink, None) };
            if let Some(bg) = bg {
                scene.rect(Rect::new(r.x, y, r.w, row_h), bg);
            }
            let icon = match row {
                StartRow::Restore => icons::HISTORY,
                StartRow::Recent(Saved::Page { .. }) => icons::GLOBE,
                StartRow::Recent(Saved::Shell { .. }) => icons::TERMINAL,
                StartRow::Fresh => icons::PLUS,
            };
            let base_r = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = r.x + self.px(18.0);
            self.fonts.draw_icon(scene, icon, self.px(16.0), x, base_r - self.px(16.0) + self.px(3.0), fg);
            x += self.px(16.0) + self.px(12.0);
            let ds = Style { color: if on { Theme::with_alpha(t.paper, 0.7) } else { t.dim }, ..label };
            let dw = if detail.is_empty() { 0.0 } else { self.fonts.measure(ds, &detail.to_uppercase()) + self.px(12.0) };
            let st_row = if matches!(row, StartRow::Restore) { Style { color: fg, ..ui_strong } } else { Style { color: fg, ..ui } };
            let tfit = self.fit(st_row, title, r.right() - self.px(18.0) - dw - x);
            self.fonts.draw(scene, st_row, x, base_r, &tfit);
            if !detail.is_empty() {
                self.fonts.draw(scene, ds, r.right() - self.px(18.0) - dw + self.px(12.0), base_r, &detail.to_uppercase());
            }
            if !on {
                scene.hline(r.x, y + row_h - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), t.tint);
            }
            rects.push(Rect::new(r.x, y, r.w, row_h));
            y += row_h;
        }
        // Foot.
        let fb = y + self.px(8.0) + self.px(m::LABEL_PX);
        let mut x = r.x + self.px(18.0);
        for (k, v) in [("ENTER", "OPEN"), ("ESC", "FRESH"), ("↑↓", "MOVE")] {
            x += self.fonts.draw(scene, strong, x, fb, k) + self.px(6.0);
            x += self.fonts.draw(scene, dim, x, fb, v) + self.px(16.0);
        }
        let note = if self.behavior.start_on_launch { "AT LAUNCH · OFF IN SETTINGS" } else { "THE PLANET BRINGS IT BACK" };
        let nw = self.fonts.measure(dim, note);
        self.fonts.draw(scene, dim, r.right() - self.px(18.0) - nw, fb, note);
        if let Some(st) = self.start.as_mut() {
            st.rows = rects;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_json_roundtrip() {
        let s = Session {
            tabs: vec![
                SavedTab { left: Some(Saved::Shell { profile: "pwsh".into() }), right: Some(Saved::Page { url: "https://a".into(), title: "A".into() }), pinned: true, parent: None },
                SavedTab { left: Some(Saved::Page { url: "https://b".into(), title: "B".into() }), right: None, pinned: false, parent: Some(0) },
            ],
            active: 1,
        };
        let dir = std::env::temp_dir().join(format!("nus-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("profile")).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        s.save();
        let back = Session::load().unwrap();
        std::env::set_current_dir(prev).unwrap();
        assert_eq!(back.tabs.len(), 2);
        assert_eq!(back.tabs[1].parent, Some(0));
        assert!(back.tabs[0].pinned);
        assert_eq!(back.active, 1);
        assert_eq!(s.summary(), "1 shell · 2 pages");
    }
}
