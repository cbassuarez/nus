//! Folders: a region under the tab rows. A plain folder keeps pages you
//! save into it (SAVE TO FOLDER in a tab's menu); a page that lives in a
//! folder never archives. Live folders fill themselves: GITHUB from `gh`
//! (pull requests that involve you), PORTS from what's listening on this
//! machine, and any `folders` in rules.luau (static lists, or a function
//! that returns one). Clicking an item opens it, or goes to the tab that
//! already has it.

use crate::app::{Caps, hover_key, App, IconMotion, Pane, SideHit};
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Github,
    Ports,
    Rules,
    /// The editor's project tree.
    Files,
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub id: u64,
    pub name: String,
    pub kind: Kind,
    pub items: Vec<Item>,
    pub open: bool,
    /// Live folders: what the last fetch said ("12 open", "gh not signed in").
    pub note: String,
}

/// What the background fetch sends back.
pub enum Update {
    Github(Result<Vec<Item>, String>),
}

pub struct Live {
    pub rx: Receiver<Update>,
    pub ports_at: Instant,
    pub rules_at: Instant,
}

/// A row in the folders region: a folder's head, or one of its items.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FRow {
    Head(usize),
    Item(usize, usize),
}

fn folders_path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("folders.json")
}

/// Start the fetcher: `gh` every two minutes, when it's there and signed in.
pub fn start() -> Live {
    let (tx, rx) = channel();
    std::thread::spawn(move || loop {
        let r = gh_prs();
        if tx.send(Update::Github(r)).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_secs(120));
    });
    Live { rx, ports_at: Instant::now() - Duration::from_secs(60), rules_at: Instant::now() - Duration::from_secs(60) }
}

fn gh(args: &[&str]) -> Result<String, String> {
    let mut c = std::process::Command::new("gh");
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = c.output().map_err(|_| "gh isn't installed".to_string())?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(crate::surface::first_line(e.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Open pull requests that involve the signed-in user, newest first.
fn gh_prs() -> Result<Vec<Item>, String> {
    gh(&["auth", "token"]).map_err(|_| "gh isn't signed in · gh auth login".to_string())?;
    let json = gh(&["search", "prs", "--involves", "@me", "--state", "open", "--limit", "12", "--json", "title,url,repository,number"])?;
    let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let title = p.get("title")?.as_str()?.to_string();
                    let url = p.get("url")?.as_str()?.to_string();
                    let repo = p.get("repository").and_then(|r| r.get("nameWithOwner")).and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let n = p.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
                    let repo = repo.rsplit('/').next().unwrap_or(&repo).to_string();
                    Some(Item { title, url, detail: format!("{repo} #{n}") })
                })
                .collect()
        })
        .unwrap_or_default())
}

impl App {
    /// Load plain folders from disk and seed the live ones.
    pub(crate) fn load_folders(&mut self) {
        let mut v: Vec<Folder> = Vec::new();
        v.push(Folder { id: 1, name: "GITHUB".into(), kind: Kind::Github, items: Vec::new(), open: true, note: "asking gh…".into() });
        v.push(Folder { id: 2, name: "PORTS".into(), kind: Kind::Ports, items: Vec::new(), open: true, note: "nothing listening".into() });
        let mut next = 100;
        if let Ok(text) = std::fs::read_to_string(folders_path()) {
            if let Ok(saved) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                for f in saved {
                    let name = f.get("name").and_then(|n| n.as_str()).unwrap_or("FOLDER").to_string();
                    let items: Vec<Item> = f.get("items").and_then(|i| serde_json::from_value(i.clone()).ok()).unwrap_or_default();
                    let open = f.get("open").and_then(|o| o.as_bool()).unwrap_or(true);
                    v.push(Folder { id: next, name, kind: Kind::Plain, items, open, note: String::new() });
                    next += 1;
                }
            }
        }
        self.folders = v;
        self.next_folder_id = next;
        self.refresh_rules_folders();
    }

    pub(crate) fn save_folders(&self) {
        let v: Vec<serde_json::Value> = self
            .folders
            .iter()
            .filter(|f| f.kind == Kind::Plain)
            .map(|f| serde_json::json!({ "name": f.name, "items": f.items, "open": f.open }))
            .collect();
        let _ = std::fs::write(folders_path(), serde_json::to_string_pretty(&v).unwrap_or_default());
    }

    /// Folders from rules.luau replace the previous set of Rules folders.
    pub(crate) fn refresh_rules_folders(&mut self) {
        let fresh = self.rules.folders();
        let open_before: std::collections::HashMap<String, bool> = self.folders.iter().filter(|f| f.kind == Kind::Rules).map(|f| (f.name.clone(), f.open)).collect();
        self.folders.retain(|f| f.kind != Kind::Rules);
        for (name, items) in fresh {
            let open = open_before.get(&name).copied().unwrap_or(true);
            let id = self.next_folder_id;
            self.next_folder_id += 1;
            self.folders.push(Folder { id, name: name.caps(), kind: Kind::Rules, items, open, note: String::new() });
        }
    }

    /// Once a loop: drain gh, refresh ports and rules folders on their clocks.
    pub(crate) fn tend_folders(&mut self) {
        let mut changed = false;
        while let Ok(u) = self.live.rx.try_recv() {
            match u {
                Update::Github(r) => {
                    if let Some(f) = self.folders.iter_mut().find(|f| f.kind == Kind::Github) {
                        match r {
                            Ok(items) => {
                                f.note = if items.is_empty() { "no open pull requests".into() } else { format!("{} open", items.len()) };
                                f.items = items;
                            }
                            Err(e) => {
                                f.note = e;
                                f.items.clear();
                            }
                        }
                        changed = true;
                    }
                }
            }
        }
        if self.live.ports_at.elapsed().as_secs() >= 10 {
            self.live.ports_at = Instant::now();
            let ports: Vec<Item> = nus_pty::listening_ports()
                .into_iter()
                .filter(|p| p.port >= 1024 && !crate::app::SYSTEM_PROCS.contains(&p.process.to_lowercase().as_str()))
                .map(|p| Item { title: format!("localhost:{}", p.port), url: format!("http://localhost:{}/", p.port), detail: if p.process.is_empty() { "?".into() } else { p.process.clone() } })
                .collect();
            if let Some(f) = self.folders.iter_mut().find(|f| f.kind == Kind::Ports) {
                if f.items != ports {
                    f.note = if ports.is_empty() { "nothing listening".into() } else { format!("{} listening", ports.len()) };
                    f.items = ports;
                    changed = true;
                }
            }
        }
        if self.live.rules_at.elapsed().as_secs() >= 60 {
            self.live.rules_at = Instant::now();
            let before: Vec<(String, Vec<Item>)> = self.folders.iter().filter(|f| f.kind == Kind::Rules).map(|f| (f.name.clone(), f.items.clone())).collect();
            self.refresh_rules_folders();
            let after: Vec<(String, Vec<Item>)> = self.folders.iter().filter(|f| f.kind == Kind::Rules).map(|f| (f.name.clone(), f.items.clone())).collect();
            changed |= before != after;
        }
        if changed && self.sidebar_visible() {
            self.dirty = true;
        }
    }

    /// Is this URL kept by a plain folder? Such pages never archive.
    pub(crate) fn in_folder(&self, url: &str) -> bool {
        self.folders.iter().filter(|f| f.kind == Kind::Plain).any(|f| f.items.iter().any(|i| i.url == url))
    }

    /// Save tab `i`'s page into folder `fi` (a plain one), once.
    pub(crate) fn save_to_folder(&mut self, i: usize, fi: usize) {
        let Some(tab) = self.tabs.get(i) else { return };
        let Pane::Web(w) = &tab.left else { return };
        let (url, title) = {
            let s = w.tab.shared.borrow();
            (s.url.clone(), if s.title.is_empty() { s.url.clone() } else { s.title.clone() })
        };
        let title = tab.name.clone().unwrap_or(title);
        let host = url.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
        if let Some(f) = self.folders.get_mut(fi) {
            if f.kind == Kind::Plain && !f.items.iter().any(|it| it.url == url) {
                f.items.push(Item { title, url, detail: host });
                f.open = true;
            }
        }
        self.save_folders();
        self.play_event("toggle");
        self.dirty = true;
    }

    /// A new plain folder, named; returns its index.
    pub(crate) fn new_folder(&mut self, name: &str) -> usize {
        let id = self.next_folder_id;
        self.next_folder_id += 1;
        let name = if name.trim().is_empty() { "SAVED".to_string() } else { name.trim().caps() };
        self.folders.push(Folder { id, name, kind: Kind::Plain, items: Vec::new(), open: true, note: String::new() });
        self.save_folders();
        self.folders.len() - 1
    }

    pub(crate) fn remove_from_folder(&mut self, fi: usize, k: usize) {
        if let Some(f) = self.folders.get_mut(fi) {
            if f.kind == Kind::Plain && k < f.items.len() {
                f.items.remove(k);
                // An empty folder goes too.
                if f.items.is_empty() {
                    self.folders.remove(fi);
                }
            }
        }
        self.save_folders();
        self.dirty = true;
    }

    /// Open an item: the tab that has it, else a new page.
    pub(crate) fn open_item(&mut self, fi: usize, k: usize) {
        let Some(it) = self.folders.get(fi).and_then(|f| f.items.get(k)).cloned() else { return };
        if self.files_click(&it.url) {
            return;
        }
        if let Some(i) = self.tabs.iter().position(|t| t.peek.is_none() && matches!(&t.left, Pane::Web(w) if w.tab.shared.borrow().url == it.url)) {
            return self.activate(i);
        }
        self.open_url(&it.url, true);
    }

    pub(crate) fn toggle_folder(&mut self, fi: usize) {
        // The live PORTS folder's head is the board.
        if self.folders.get(fi).is_some_and(|f| f.kind == Kind::Ports) {
            self.open_board();
            return;
        }
        if let Some(f) = self.folders.get_mut(fi) {
            f.open = !f.open;
        }
        self.save_folders();
        self.play_event("toggle");
        self.dirty = true;
    }

    /// Rows of the folders region from `y` down to `bottom`: heads always,
    /// items when open; nothing if there's no room for a head.
    pub(crate) fn folder_rows(&self, y: f32, bottom: f32) -> Vec<(FRow, f32, f32)> {
        let head = self.px(28.0);
        let row = self.px(m::ROW_H);
        let mut out = Vec::new();
        let mut y = y;
        for (fi, f) in self.folders.iter().enumerate() {
            // Live folders with nothing to show and no news stay out of the way.
            if y + head > bottom {
                break;
            }
            out.push((FRow::Head(fi), y, head));
            y += head;
            if f.open {
                for k in 0..f.items.len() {
                    if y + row > bottom {
                        break;
                    }
                    out.push((FRow::Item(fi, k), y, row));
                    y += row;
                }
            }
        }
        out
    }

    /// Draw the folders region under the NEW TAB row.
    pub(crate) fn draw_folders(&mut self, scene: &mut Scene, sb: Rect, top: f32, bottom: f32) {
        if self.folders.is_empty() || top + self.px(40.0) > bottom {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let (mx, my) = self.mouse;
        let pad_x = self.px(m::ROW_PAD_X);
        let top = top + self.px(8.0);
        scene.hline(sb.x + pad_x, top, sb.w - 2.0 * pad_x, self.px(m::HAIRLINE), fade(t.dim, 0.5));
        let rows = self.folder_rows(top + self.px(8.0), bottom);
        let isz = self.px(12.0);
        for (r, y, h) in rows {
            let cell = Rect::new(sb.x, y, sb.w, h);
            let hot = cell.contains(mx, my) && self.sidebar_visible();
            match r {
                FRow::Head(fi) => {
                    let f = self.folders[fi].clone();
                    if hot {
                        scene.rect(cell, fade(t.tint, 0.6));
                    }
                    let iy = y + ((h - isz) / 2.0).round();
                    let base = y + (h + self.px(m::LABEL_PX)) / 2.0 - self.px(2.0);
                    let caret = if f.open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
                    let mut x = sb.x + pad_x;
                    self.fonts.draw_icon(scene, caret, self.px(10.0), x, iy + self.px(1.0), t.dim);
                    x += self.px(16.0);
                    let icon = match f.kind {
                        Kind::Github => icons::GITHUB,
                        Kind::Ports => icons::PORTS,
                        Kind::Rules => icons::CODE,
                        Kind::Files => icons::FOLDER,
                        Kind::Plain => icons::FOLDER_SIMPLE,
                    };
                    self.icon_button(scene, icon, isz, x, iy, ink, cell, hover_key("folder", f.id as usize), IconMotion::Bob);
                    x += isz + self.px(8.0);
                    x += self.fonts.draw(scene, Style { color: ink, ..strong }, x, base, &f.name) + self.px(8.0);
                    // The note, dim, fitted to what's left.
                    let note = if f.kind == Kind::Plain { format!("{}", f.items.len()) } else { f.note.caps() };
                    let avail = sb.right() - pad_x - x;
                    let note = self.fit(Style { color: t.dim, ..label }, &note, avail);
                    self.fonts.draw(scene, Style { color: t.dim, ..label }, x, base, &note);
                    self.side_hits.push((cell, SideHit::Folder(fi)));
                }
                FRow::Item(fi, k) => {
                    let (it, plain) = {
                        let f = &self.folders[fi];
                        (f.items[k].clone(), f.kind == Kind::Plain)
                    };
                    if hot {
                        scene.rect(cell, t.tint);
                    }
                    let base = y + (h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
                    let mut x = sb.x + pad_x + self.px(16.0);
                    let open_tab = self.tabs.iter().any(|tb| matches!(&tb.left, Pane::Web(w) if w.tab.shared.borrow().url == it.url));
                    let dot = self.px(5.0);
                    if open_tab {
                        scene.rect(Rect::new(x + self.px(3.0), y + (h - dot) / 2.0, dot, dot), self.surface.signal);
                    } else {
                        scene.outline(Rect::new(x + self.px(3.0), y + (h - dot) / 2.0, dot, dot), self.px(1.0), t.dim);
                    }
                    x += self.px(16.0);
                    let mut right = sb.right() - pad_x;
                    if hot && plain {
                        let cx = right - isz;
                        self.fonts.draw_icon(scene, icons::CLOSE, isz, cx, y + ((h - isz) / 2.0).round(), ink);
                        self.side_hits.push((Rect::new(cx - self.px(6.0), y, isz + self.px(12.0), h), SideHit::FolderDrop(fi, k)));
                        right = cx - self.px(8.0);
                    }
                    let st = Style { color: if hot { ink } else { crate::app::fade(ink, 0.82) }, ..ui };
                    let dst = Style { color: t.dim, ..label };
                    let dw = if it.detail.is_empty() { 0.0 } else { self.fonts.measure(dst, &it.detail.caps()).min((right - x) * 0.38) };
                    let title = self.fit(st, &it.title, right - x - dw - self.px(8.0));
                    let tw = self.fonts.draw(scene, st, x, base, &title);
                    if dw > 0.0 {
                        let d = self.fit(dst, &it.detail.caps(), right - (x + tw + self.px(8.0)));
                        self.fonts.draw(scene, dst, x + tw + self.px(8.0), base, &d);
                    }
                    self.side_hits.push((Rect::new(cell.x, cell.y, right - cell.x, cell.h), SideHit::FolderItem(fi, k)));
                }
            }
        }
    }
}

fn fade(c: nus_render::Color, k: f32) -> nus_render::Color {
    crate::app::fade(c, k)
}
