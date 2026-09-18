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
use crate::app::{Caps, App, Pane};

// ── Session and recent ───────────────────────────────────────────────────

/// One tab as saved: enough to bring it back.
#[derive(Clone, Debug, PartialEq)]
pub enum Saved {
    Shell { profile: String },
    Page { url: String, title: String },
    File { path: String },
    Ports,
    Layout { path: String },
}

/// What a shell was doing when the session was saved: where, what was
/// running (and since when), and its screen as text (inline; a few KB).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShellState {
    pub cwd: Option<String>,
    pub running: Option<(String, u64)>,
    pub snapshot: Option<String>,
    /// The holder's id when the shell was held.
    pub held: Option<String>,
}

fn shell_state_to_json(s: &ShellState) -> serde_json::Value {
    serde_json::json!({ "cwd": s.cwd, "cmd": s.running.as_ref().map(|r| r.0.clone()), "since": s.running.as_ref().map(|r| r.1), "snapshot": s.snapshot, "held": s.held })
}

fn shell_state_from_json(v: &serde_json::Value) -> Option<ShellState> {
    if !v.is_object() {
        return None;
    }
    let st = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string());
    let running = match (st("cmd"), v.get("since").and_then(|x| x.as_u64())) {
        (Some(c), Some(at)) if !c.trim().is_empty() => Some((c, at)),
        _ => None,
    };
    Some(ShellState { cwd: st("cwd"), running, snapshot: st("snapshot"), held: st("held") })
}

#[derive(Clone, Debug, Default)]
pub struct SavedTab {
    pub left: Option<Saved>,
    pub right: Option<Saved>,
    /// The shell's state, for a shell on either side.
    pub shell: Option<ShellState>,
    pub shell_right: Option<ShellState>,
    pub pinned: bool,
    /// Index of the parent tab in the session, for stacks.
    pub parent: Option<usize>,
    pub name: Option<String>,
    pub emoji: Option<String>,
    /// A colour the user chose, as #rrggbb.
    pub colour: Option<String>,
    /// The page's container (pages only).
    pub container: Option<String>,
    /// Lives in the hatch.
    pub hatch: bool,
    /// The right pane's width, once dragged.
    pub split: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
    /// Tiled tabs by index, in tiling order (empty = none).
    pub tiles: Vec<usize>,
    /// The window's container.
    pub container: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub item: Saved,
    pub when: u64,
    /// How many times it was visited (pages) or opened (shells).
    pub visits: u32,
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
        Saved::File { path } => serde_json::json!({ "kind": "file", "path": path }),
        Saved::Ports => serde_json::json!({ "kind": "ports" }),
        Saved::Layout { path } => serde_json::json!({ "kind": "layout", "path": path }),
    }
}

fn saved_from_json(v: &serde_json::Value) -> Option<Saved> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    match v.get("kind").and_then(|k| k.as_str())? {
        "shell" => Some(Saved::Shell { profile: s("profile") }),
        "page" => Some(Saved::Page { url: s("url"), title: s("title") }),
        "file" => Some(Saved::File { path: s("path") }),
        "ports" => Some(Saved::Ports),
        "layout" => Some(Saved::Layout { path: s("path") }),
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
                shell: t.get("shell").and_then(shell_state_from_json),
                shell_right: t.get("shell_right").and_then(shell_state_from_json),
                pinned: t.get("pinned").and_then(|p| p.as_bool()).unwrap_or(false),
                parent: t.get("parent").and_then(|p| p.as_u64()).map(|p| p as usize),
                name: t.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
                emoji: t.get("emoji").and_then(|v| v.as_str()).map(|s| s.to_string()),
                colour: t.get("colour").and_then(|v| v.as_str()).map(|s| s.to_string()),
                container: t.get("container").and_then(|v| v.as_str()).map(|s| s.to_string()),
                split: t.get("split").and_then(|v| v.as_f64()).map(|v| v as f32),
                hatch: t.get("hatch").and_then(|v| v.as_bool()).unwrap_or(false),
            })
            .collect();
        let tiles = v.get("tiles").and_then(|t| t.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()).unwrap_or_default();
        let container = v.get("container").and_then(|c| c.as_str()).unwrap_or(crate::containers::PERSONAL).to_string();
        Some(Session { tabs, active: v.get("active").and_then(|a| a.as_u64()).unwrap_or(0) as usize, tiles, container })
    }

    pub fn save(&self) {
        let tabs: Vec<serde_json::Value> = self
            .tabs
            .iter()
            .map(|t| {
                serde_json::json!({
                    "left": t.left.as_ref().map(saved_to_json),
                    "right": t.right.as_ref().map(saved_to_json),
                    "shell": t.shell.as_ref().map(shell_state_to_json),
                    "shell_right": t.shell_right.as_ref().map(shell_state_to_json),
                    "pinned": t.pinned,
                    "parent": t.parent,
                    "name": t.name,
                    "emoji": t.emoji,
                    "colour": t.colour,
                    "container": t.container,
                    "split": t.split,
                    "hatch": t.hatch,
                })
            })
            .collect();
        let v = serde_json::json!({ "tabs": tabs, "active": self.active, "tiles": self.tiles, "container": self.container, "saved": now() });
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
                .filter_map(|r| Some(Recent { item: saved_from_json(r)?, when: r.get("when").and_then(|w| w.as_u64()).unwrap_or(0), visits: r.get("visits").and_then(|v| v.as_u64()).unwrap_or(1) as u32 }))
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
            j["visits"] = serde_json::json!(r.visits);
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
    /// A held shell still running, with no tab: attach.
    Held(nus_pty::hold::Info),
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
        // Shells still running in their holders, with no tab of ours.
        for info in self.held_loose() {
            let title = format!("{} · still running", info.program);
            let detail = info.cwd.clone().unwrap_or_else(|| "held".into());
            if hit(&title) || hit(&detail) || hit("held") {
                rows.push((StartRow::Held(info), title, detail));
            }
        }
        for r in &self.recent {
            let (title, detail) = match &r.item {
                Saved::Shell { profile } => (profile.clone(), "shell".to_string()),
                Saved::File { path } => (std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), "file".to_string()),
                Saved::Ports => ("ports".to_string(), "board".to_string()),
                Saved::Layout { path } => (std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().trim_end_matches(".nus.luau").to_string()).unwrap_or_default(), "layout".to_string()),
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
            StartRow::Recent(Saved::File { path }) => self.open_file(std::path::Path::new(&path), false),
            StartRow::Recent(Saved::Ports) => self.expand_board(),
            StartRow::Recent(Saved::Layout { path }) => self.open_layout(std::path::Path::new(&path)),
            StartRow::Recent(Saved::Shell { profile }) => {
                let idx = self.profiles.iter().position(|p| p.name == profile).unwrap_or(self.behavior.default_profile);
                self.new_tab(idx);
            }
            StartRow::Held(info) => self.attach_held(info),
            StartRow::Fresh => {}
        }
        self.dirty = true;
    }

    /// A saved shell, back: attached to its holder when that is still
    /// running (the ring replays, nothing was cut); else a fresh shell in
    /// its folder with the snapshot laid down and the cut-off chip.
    pub(crate) fn restore_shell(&mut self, idx: usize, s: &ShellState) -> Option<crate::app::TermPane> {
        if let Some(id) = &s.held {
            let dir = crate::app::App::hold_dir();
            if let Some(info) = nus_pty::hold::Info::read(&dir, id) {
                if info.alive(&dir) {
                    if let Ok(t) = self.new_term_pane_attached(false, info) {
                        return Some(t);
                    }
                }
            }
        }
        let mut t = self.new_term_pane_at(false, idx, s.cwd.clone()).ok()?;
        self.restore_shell_state(&mut t, s);
        Some(t)
    }

    /// The snapshot as dim history above the fresh prompt, and the cut-off
    /// chip when a command was still running at save time.
    pub(crate) fn restore_shell_state(&mut self, t: &mut crate::app::TermPane, s: &ShellState) {
        if let Some(text) = s.snapshot.as_ref() {
            self.lay_snapshot(t, text);
        }
        let mode = self.behavior.cutoff;
        if let (Some((cmd, at)), true) = (&s.running, mode != crate::settings::CutOff::Off) {
            let cwd = s.cwd.clone().unwrap_or_default();
            let (kind, resume, label) = match self.rules.on_cutoff(cmd, &cwd) {
                Some((label, resume)) => (crate::cutoff::Kind::RunAgain, resume, label),
                None => crate::cutoff::resume_for(cmd, &cwd, *at),
            };
            match mode {
                crate::settings::CutOff::RunAgain => t.type_at_prompt = Some(format!("{resume}\r")),
                _ => {
                    let line = t.term.grid().abs_row(t.term.cursor().row);
                    t.cutoff = Some(crate::cutoff::CutOff { cmd: crate::cutoff::oneline(cmd), at: *at, kind, resume, label, line });
                }
            }
        }
    }

    pub(crate) fn restore_session_pub(&mut self) {
        self.restore_session();
    }

    /// Bring the saved tabs back: shells restart on their profile, pages
    /// reload, stacks and pins are kept. Scrollback restore is v1.
    fn restore_session(&mut self) {
        let Some(sess) = self.last_session.clone() else { return };
        if self.containers.iter().any(|c| c.name == sess.container) {
            self.container = sess.container.clone();
            self.register_window();
        }
        let mut ids: Vec<Option<u64>> = Vec::new();
        for t in &sess.tabs {
            let container = t.container.clone().filter(|c| self.containers.iter().any(|k| &k.name == c)).unwrap_or_else(|| self.container.clone());
            let left = match &t.left {
                Some(Saved::Shell { profile }) => {
                    let idx = self.profiles.iter().position(|p| &p.name == profile).unwrap_or(self.behavior.default_profile);
                    let state = t.shell.clone().unwrap_or_default();
                    self.restore_shell(idx, &state).map(Pane::Term)
                }
                Some(Saved::Page { url, .. }) => self.new_web_pane_in(url, &container).map(Pane::Web),
                Some(Saved::File { path }) => {
                    let mut e = crate::editor::EditorPane::new(nus_render::Rect::new(0.0, 0.0, 1.0, 1.0));
                    e.open(std::path::Path::new(path)).ok().map(|_| Pane::Editor(e))
                }
                Some(Saved::Ports) => Some(Pane::Ports(crate::ports::PortsPane { rect: nus_render::Rect::new(0.0, 0.0, 1.0, 1.0) })),
                Some(Saved::Layout { .. }) | None => None,
            };
            let Some(left) = left else {
                ids.push(None);
                continue;
            };
            let right = match &t.right {
                Some(Saved::Page { url, .. }) => self.new_web_pane(url).map(Pane::Web),
                Some(Saved::Shell { profile }) => {
                    let idx = self.profiles.iter().position(|p| &p.name == profile).unwrap_or(self.behavior.default_profile);
                    let state = t.shell_right.clone().unwrap_or_default();
                    self.restore_shell(idx, &state).map(Pane::Term)
                }
                Some(Saved::File { path }) => {
                    let mut e = crate::editor::EditorPane::new(nus_render::Rect::new(0.0, 0.0, 1.0, 1.0));
                    e.open(std::path::Path::new(path)).ok().map(|_| Pane::Editor(e))
                }
                Some(Saved::Ports) => Some(Pane::Ports(crate::ports::PortsPane { rect: nus_render::Rect::new(0.0, 0.0, 1.0, 1.0) })),
                Some(Saved::Layout { .. }) | None => None,
            };
            let mut tab = self.make_tab(left, right);
            tab.pinned = t.pinned;
            tab.name = t.name.clone();
            tab.emoji = t.emoji.clone();
            tab.split_w = t.split;
            tab.hatch = t.hatch;
            if let Some(c) = t.colour.as_deref().and_then(crate::surface::parse_hex) {
                tab.tint = Some(c);
                tab.look.signal = Some(c);
                tab.look.bg = Some(App::tab_tint(self.theme.mode, c));
            }
            if let Some(p) = t.parent.and_then(|p| ids.get(p).copied().flatten()) {
                tab.parent = Some(p);
            }
            ids.push(Some(tab.id));
            self.tabs.push(tab);
        }
        let n = self.tabs.len();
        if n > 0 {
            let first_new = n - ids.iter().filter(|i| i.is_some()).count();
            let tiled: Vec<u64> = sess.tiles.iter().filter_map(|&k| ids.get(k).copied().flatten()).collect();
            if tiled.len() >= 2 {
                self.tiling = Some(crate::tiles::Tiling { ids: tiled, x: 0.5, y: 0.5 });
            }
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
            Pane::Editor(e) => e.buf().and_then(|b| b.path.as_ref()).map(|p| Saved::File { path: p.display().to_string() }),
            Pane::Ports(_) => Some(Saved::Ports),
            _ => None,
        };
        // A shell's state: cwd, what is running, and its screen as text under
        // profile/session/, so restore can lay it down and offer the cut-off chip.
        let state = |p: &Pane, id: u64, side: &str| match p {
            Pane::Term(t) => {
                let running = match (t.running_since, t.running_at) {
                    (Some(_), Some(at)) => t.term.marks.iter().rev().find(|m| m.kind == nus_vt::MarkKind::CommandStart).map(|b| (t.term.command_text(b), at)).filter(|(c, _)| !c.trim().is_empty()),
                    _ => None,
                };
                // Inline, not a file: a file keyed by tab id is overwritten by
                // the next launch's first save before restore gets to read it.
                let _ = (id, side);
                let text = t.snapshot_text(400);
                let snapshot = (!text.is_empty()).then_some(text);
                Some(ShellState { cwd: t.term.cwd.clone(), running, snapshot, held: t.pty.held_id().map(String::from) })
            }
            _ => None,
        };
        let listed: Vec<&crate::app::Tab> = self.tabs.iter().filter(|t| t.peek.is_none()).collect();
        let index_of = |id: u64| listed.iter().position(|t| t.id == id);
        let tabs: Vec<SavedTab> = listed
            .iter()
            .map(|t| SavedTab { left: saved(&t.left), right: t.right.as_ref().and_then(saved), shell: state(&t.left, t.id, "l"), shell_right: t.right.as_ref().and_then(|p| state(p, t.id, "r")), pinned: t.pinned, parent: t.parent.and_then(index_of), name: t.name.clone(), emoji: t.emoji.clone(), colour: t.tint.map(crate::surface::hex), container: match &t.left { Pane::Web(w) => Some(w.container.clone()), _ => None }, split: t.split_w, hatch: t.hatch })
            .filter(|t| t.left.is_some())
            .collect();
        let tiles = self.tiling.as_ref().map(|t| t.ids.iter().filter_map(|&id| index_of(id)).collect()).unwrap_or_default();
        Session { tabs, active: self.active, tiles, container: self.container.clone() }.save();
    }

    /// Remember a page or shell in the recent list (deduped, newest first).
    pub(crate) fn remember(&mut self, item: Saved) {
        if let Saved::Page { url, .. } = &item {
            if url.is_empty() || url.starts_with("http://127.0.0.1:9229") {
                return;
            }
        }
        let same = |r: &Recent| match (&r.item, &item) {
            (Saved::Page { url: a, .. }, Saved::Page { url: b, .. }) => a == b,
            (Saved::Shell { profile: a }, Saved::Shell { profile: b }) => a == b,
            _ => false,
        };
        let visits = self.recent.iter().find(|r| same(r)).map(|r| r.visits + 1).unwrap_or(1);
        self.recent.retain(|r| !same(r));
        self.recent.insert(0, Recent { item, when: now(), visits });
        // History for the palette: keep plenty; the atlas shows the top anyway.
        self.recent.truncate(2000);
        save_recent(&self.recent);
    }

    pub fn start_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        let Some(s) = self.start.as_mut() else { return false };
        if ev.state != ElementState::Pressed {
            return true;
        }
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                // Persistent at launch: Esc only clears the filter.
                if self.behavior.atlas == crate::settings::AtlasMode::Persistent && !self.atlas_used {
                    s.input.clear();
                } else {
                    self.start = None;
                }
            }
            WKey::Named(NamedKey::Enter) => {
                self.atlas_used = true;
                self.start_commit()
            }
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
            Some(s) if !s.tabs.is_empty() => format!("LAST TIME · {}", s.summary().caps()),
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
                StartRow::Recent(Saved::File { .. }) => icons::CODE,
                StartRow::Recent(Saved::Ports) => icons::PORTS,
                StartRow::Recent(Saved::Layout { .. }) => icons::STACK,
                StartRow::Held(_) => icons::TERMINAL,
                StartRow::Fresh => icons::PLUS,
            };
            let base_r = y + self.px(10.0) + self.px(m::UI_PX) - self.px(3.0);
            let mut x = r.x + self.px(18.0);
            self.fonts.draw_icon(scene, icon, self.px(16.0), x, base_r - self.px(16.0) + self.px(3.0), fg);
            x += self.px(16.0) + self.px(12.0);
            let ds = Style { color: if on { Theme::with_alpha(t.paper, 0.7) } else { t.dim }, ..label };
            let dw = if detail.is_empty() { 0.0 } else { self.fonts.measure(ds, &detail.caps()) + self.px(12.0) };
            let st_row = if matches!(row, StartRow::Restore) { Style { color: fg, ..ui_strong } } else { Style { color: fg, ..ui } };
            let tfit = self.fit(st_row, title, r.right() - self.px(18.0) - dw - x);
            self.fonts.draw(scene, st_row, x, base_r, &tfit);
            if !detail.is_empty() {
                self.fonts.draw(scene, ds, r.right() - self.px(18.0) - dw + self.px(12.0), base_r, &detail.caps());
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
        let note = match self.behavior.atlas {
            crate::settings::AtlasMode::Planet => "THE PLANET BRINGS IT BACK",
            crate::settings::AtlasMode::AtLaunch => "AT LAUNCH · OFF IN STARTUP",
            crate::settings::AtlasMode::Persistent => "PICK ONE TO CONTINUE",
        };
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
                SavedTab { left: Some(Saved::Shell { profile: "pwsh".into() }), right: Some(Saved::Page { url: "https://a".into(), title: "A".into() }), shell: Some(ShellState { cwd: Some("/x".into()), running: Some(("claude".into(), 7)), snapshot: Some("hi".into()), held: None }), shell_right: None, pinned: true, parent: None, name: Some("deploy notes".into()), emoji: Some("📌".into()), colour: Some("#2e7d32".into()), container: Some("WORK".into()), split: None, hatch: false },
                SavedTab { left: Some(Saved::Page { url: "https://b".into(), title: "B".into() }), right: None, shell: None, shell_right: None, pinned: false, parent: Some(0), name: None, emoji: None, colour: None, container: None, split: None, hatch: false },
            ],
            active: 1,
            tiles: vec![0, 1],
            container: "PERSONAL".into(),
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
        assert_eq!(back.tabs[0].name.as_deref(), Some("deploy notes"));
        assert_eq!(back.tabs[0].emoji.as_deref(), Some("📌"));
        assert_eq!(back.tabs[0].colour.as_deref(), Some("#2e7d32"));
        let sh = back.tabs[0].shell.as_ref().expect("the shell state rides along");
        assert_eq!(sh.cwd.as_deref(), Some("/x"));
        assert_eq!(sh.running, Some(("claude".into(), 7)));
        assert_eq!(sh.snapshot.as_deref(), Some("hi"));
        assert!(back.tabs[1].shell.is_none());
        assert!(back.tabs[1].name.is_none());
        assert_eq!(back.active, 1);
        assert_eq!(back.tiles, vec![0, 1]);
        assert_eq!(s.summary(), "1 shell · 2 pages");
    }
}
