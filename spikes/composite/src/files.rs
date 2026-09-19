//! FILES: the sidebar's second page — the folder a window works in, as a
//! tree, like an IDE's explorer. The root is the window's workspace when
//! it has one (a window bound to a folder: its name in the strip, the
//! tree here, new shells born there); without one it follows the focused
//! shell's folder. Ctrl+Shift+E, or the folder in the footer, turns the
//! page; a click on a folder opens it, a click on a file opens it in the
//! editor beside the shell as a preview (the next click replaces it),
//! a double-click keeps it. Heavy folders — node_modules, target, .git —
//! sit dim and closed until asked.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use nus_render::text::{icons, Style};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, Caps, Pane, SideHit};

/// Which page the sidebar shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SidePage {
    #[default]
    Tabs,
    Files,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub dir: bool,
    pub depth: usize,
    /// Dot-files, and the heavy folders nobody wants to browse.
    pub dim: bool,
}

/// Folders that are shown but not walked into on their own.
const HEAVY: &[&str] = &["node_modules", "target", ".git", "dist", "build", "__pycache__", ".venv", "venv", ".next", ".cache", "vendor"];

pub struct Tree {
    pub root: Option<PathBuf>,
    /// Folders held open, by path.
    pub open: HashSet<PathBuf>,
    /// The visible rows, root's children first.
    pub rows: Vec<Node>,
    /// Listings, by folder, with when they were read.
    cache: HashMap<PathBuf, (Instant, Vec<Node>)>,
    pub scroll: f32,
    pub rect: Rect,
    /// The file previewed in the editor split, if any.
    pub preview: Option<PathBuf>,
    /// The last click on a row, for the double-click.
    pub last_click: Option<(Instant, PathBuf)>,
    pub hover: Option<usize>,
}

impl Default for Tree {
    fn default() -> Self {
        Tree { root: None, open: HashSet::new(), rows: Vec::new(), cache: HashMap::new(), scroll: 0.0, rect: Rect::new(0.0, 0.0, 0.0, 0.0), preview: None, last_click: None, hover: None }
    }
}

impl Tree {
    /// Point the tree at a folder; the same folder is a no-op.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        if self.root == root {
            return;
        }
        self.root = root;
        self.open.clear();
        self.scroll = 0.0;
        self.rebuild();
    }

    /// One folder's entries: folders first, then files, each by name.
    fn list(&mut self, dir: &Path, depth: usize) -> Vec<Node> {
        if let Some((at, v)) = self.cache.get(dir) {
            if at.elapsed().as_secs() < 3 {
                return v.clone();
            }
        }
        let mut dirs: Vec<Node> = Vec::new();
        let mut files: Vec<Node> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let dim = name.starts_with('.') || (is_dir && HEAVY.contains(&name.as_str()));
                let n = Node { path: e.path(), name, dir: is_dir, depth, dim };
                if is_dir {
                    dirs.push(n);
                } else {
                    files.push(n);
                }
            }
        }
        let key = |n: &Node| n.name.to_lowercase();
        dirs.sort_by_key(key);
        files.sort_by_key(key);
        dirs.extend(files);
        self.cache.insert(dir.to_path_buf(), (Instant::now(), dirs.clone()));
        dirs
    }

    /// The rows, from the root down through every open folder.
    pub fn rebuild(&mut self) {
        let Some(root) = self.root.clone() else {
            self.rows.clear();
            return;
        };
        let mut out = Vec::new();
        fn walk(t: &mut Tree, dir: &Path, depth: usize, out: &mut Vec<Node>) {
            let kids = t.list(dir, depth);
            for k in kids {
                let open = k.dir && t.open.contains(&k.path);
                let path = k.path.clone();
                out.push(k);
                if open && out.len() < 4000 {
                    walk(t, &path, depth + 1, out);
                }
            }
        }
        walk(self, &root, 0, &mut out);
        self.rows = out;
    }

    /// Forget the listings so the next rebuild reads the disk.
    pub fn refresh(&mut self) {
        self.cache.clear();
        self.rebuild();
    }

    pub fn toggle(&mut self, path: &Path) {
        if !self.open.remove(path) {
            self.open.insert(path.to_path_buf());
        }
        self.rebuild();
    }
}

impl App {
    /// The folder the tree shows: the workspace, else the focused shell's.
    fn tree_root(&self) -> Option<PathBuf> {
        if let Some(w) = &self.workspace {
            return Some(w.clone());
        }
        self.focused_cwd().map(PathBuf::from).filter(|p| p.is_dir())
    }

    /// Once a frame while the page shows: follow the folder, re-read now and then.
    pub(crate) fn tend_tree(&mut self) {
        if self.side_page != SidePage::Files {
            return;
        }
        let root = self.tree_root();
        if self.tree.root != root {
            self.tree.set_root(root);
            self.dirty = true;
        }
    }

    pub(crate) fn toggle_files(&mut self) {
        self.side_page = if self.side_page == SidePage::Files { SidePage::Tabs } else { SidePage::Files };
        if self.side_page == SidePage::Files {
            if !self.sidebar_visible() {
                self.run(crate::app::Action::ToggleSidebar);
            }
            self.tree.refresh();
            self.tend_tree();
        }
        self.play_event("toggle");
        self.dirty = true;
    }

    /// Bind this window to a folder: its name, its tree, where new shells
    /// are born. None unbinds.
    pub(crate) fn bind_workspace(&mut self, folder: Option<PathBuf>) {
        let folder = folder.map(|f| PathBuf::from(f.to_string_lossy().trim_start_matches(r"\\?\")));
        self.workspace = folder.filter(|f| f.is_dir());
        self.auto_name_key = None;
        self.refresh_auto_name();
        self.register_window();
        self.tree.set_root(self.tree_root());
        self.save_session();
        self.dirty = true;
    }

    /// This window becomes the folder's: bound, a shell born there where
    /// the prompt was (or as a new tab), FILES showing its tree.
    pub(crate) fn open_folder(&mut self, folder: &str) {
        let path = PathBuf::from(folder);
        if !path.is_dir() {
            self.notice(&format!("not a folder · {folder}"));
            return;
        }
        self.bind_workspace(Some(path.clone()));
        self.fresh = false;
        let profile = self.behavior.default_profile;
        let i = self.active;
        let at_prompt = self.tabs.get(i).is_some_and(|t| matches!(t.left, Pane::Home(_)));
        match self.new_term_pane_at(false, profile, Some(folder.to_string())) {
            Ok(t) => {
                if at_prompt {
                    if let Some(tab) = self.tabs.get_mut(i) {
                        tab.left = Pane::Term(t);
                        tab.right = None;
                        tab.focus_right = false;
                    }
                } else {
                    let tab = self.make_tab(Pane::Term(t), None);
                    self.tabs.push(tab);
                    self.activate(self.tabs.len() - 1);
                }
            }
            Err(e) => self.notice(&format!("{e}")),
        }
        if self.side_page != SidePage::Files {
            self.toggle_files();
        }
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    /// A file from the tree: into the editor beside the shell, as the
    /// preview (the last preview goes if it was not touched); `keep`
    /// pins it — its own buffer, and nothing replaces it.
    pub(crate) fn open_from_tree(&mut self, path: &Path, keep: bool) {
        if !crate::editor::looks_text(path) {
            crate::app::open_with_os(path);
            return;
        }
        let i = self.active;
        // The previous preview goes, unless it was edited or is the same file.
        let prev = self.tree.preview.take();
        if let Some(prev) = prev.filter(|p| p != path) {
            if let Some(tab) = self.tabs.get_mut(i) {
                for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                    if let Pane::Editor(e) = p {
                        if let Some(k) = e.buffers.iter().position(|b| b.path.as_deref() == Some(prev.as_path()) && !b.dirty) {
                            e.buffers.remove(k);
                            if e.active >= e.buffers.len() {
                                e.active = e.buffers.len().saturating_sub(1);
                            }
                        }
                    }
                }
            }
        }
        // A shell in front: the editor opens beside it; an editor in front: in it.
        let split = self.tabs.get(i).is_some_and(|t| !matches!(t.left, Pane::Editor(_)) && !matches!(t.right, Some(Pane::Editor(_))));
        self.open_file(path, split);
        if !keep {
            self.tree.preview = Some(path.to_path_buf());
        }
        // The editor may have emptied out: drop it if so.
        if let Some(tab) = self.tabs.get_mut(i) {
            if matches!(&tab.right, Some(Pane::Editor(e)) if e.buffers.is_empty()) {
                tab.right = None;
                tab.focus_right = false;
            }
        }
        self.layout();
        self.dirty = true;
    }

    /// The page, in the sidebar's list area between the header and the footer.
    pub(crate) fn draw_tree(&mut self, scene: &mut Scene, sb: Rect, top: f32, bottom: f32) {
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        let row_h = self.px(m::ROW_H) * 0.86;
        let (mx, my) = self.mouse;
        let area = Rect::new(sb.x, top, sb.w, (bottom - top).max(0.0));
        self.tree.rect = area;
        // The head: FILES · the root's name; a pin when bound, the path in the tooltip.
        let head_h = self.px(34.0);
        let base = top + self.px(22.0);
        let bound = self.workspace.is_some();
        let root = self.tree.root.clone();
        let name = root.as_ref().and_then(|r| r.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "no folder".into());
        let isz = self.px(13.0);
        self.fonts.draw_icon(scene, if bound { icons::PIN } else { icons::FOLDER_SIMPLE }, isz, sb.x + self.px(m::ROW_PAD_X), base - isz + self.px(2.0), if bound { self.surface.signal } else { ink });
        let nx = sb.x + self.px(m::ROW_PAD_X) + isz + self.px(8.0);
        let shown = self.fit(label, &name.to_uppercase(), sb.w - (nx - sb.x) - self.px(36.0));
        self.fonts.draw(scene, Style { color: ink, ..label }, nx, base, &shown);
        let head = Rect::new(sb.x, top, sb.w, head_h);
        let words = match (&root, bound) {
            (Some(r), true) => format!("{} · this window's folder · click to let it follow the shell", r.display()),
            (Some(r), false) => format!("{} · following the shell · click to keep it", r.display()),
            (None, _) => "no shell here yet · open one and its folder shows".to_string(),
        };
        self.side_tip(hover_key("tree-head"), head, words);
        self.side_hits.push((head, SideHit::FilesPin));
        // Up one: a small caret at the right, when there is an up.
        if let Some(r) = &root {
            if r.parent().is_some() {
                let usz = self.px(11.0);
                let ur = Rect::new(sb.right() - self.px(m::ROW_PAD_X) - usz - self.px(8.0), top, usz + self.px(16.0), head_h);
                self.fonts.draw_icon(scene, icons::CARET_DOWN, usz, ur.x + self.px(8.0), base - usz + self.px(1.0), t.dim);
                self.side_hits.push((ur, SideHit::FilesUp));
            }
        }
        scene.hline(sb.x, top + head_h - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), t.tint);
        // The rows.
        let list = Rect::new(sb.x, top + head_h, sb.w, (bottom - top - head_h).max(0.0));
        let total = self.tree.rows.len() as f32 * row_h;
        let max_scroll = (total - list.h).max(0.0);
        self.tree.scroll = self.tree.scroll.clamp(0.0, max_scroll);
        scene.layer(Some(list));
        let mut y = list.y - self.tree.scroll;
        let rows = self.tree.rows.clone();
        let preview = self.tree.preview.clone();
        self.tree.hover = None;
        for (k, n) in rows.iter().enumerate() {
            if y + row_h < list.y {
                y += row_h;
                continue;
            }
            if y > list.bottom() {
                break;
            }
            let rr = Rect::new(sb.x, y, sb.w, row_h);
            let hot = rr.contains(mx, my) && list.contains(mx, my);
            if hot {
                self.tree.hover = Some(k);
                scene.rect(rr, fade(t.tint, 0.5));
            }
            let is_preview = preview.as_deref() == Some(n.path.as_path());
            if is_preview {
                scene.rect(Rect::new(sb.x, y, self.px(2.0), row_h), self.surface.signal);
            }
            let x = sb.x + self.px(m::ROW_PAD_X) + n.depth as f32 * self.px(14.0);
            let b = y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
            let col = if n.dim { t.dim } else { ink };
            let csz = self.px(10.0);
            if n.dir {
                let open = self.tree.open.contains(&n.path);
                self.fonts.draw_icon(scene, if open { icons::CARET_DOWN } else { icons::CARET_RIGHT }, csz, x, b - csz + self.px(1.0), col);
            }
            let tx = x + csz + self.px(6.0);
            let st = Style { color: col, ..ui };
            let shown = self.fit(st, &n.name, sb.right() - tx - self.px(10.0));
            self.fonts.draw(scene, st, tx, b, &shown);
            self.side_hits.push((rr, SideHit::FileRow(k)));
            y += row_h;
        }
        if rows.is_empty() {
            let words = if root.is_some() { "empty" } else { "a shell's folder shows here" };
            self.fonts.draw(scene, dim, sb.x + self.px(m::ROW_PAD_X), list.y + self.px(22.0), &words.caps());
        }
        scene.layer(None);
        let _ = ui;
    }

    /// A click on a row: a folder opens or closes; a file previews, twice keeps.
    pub(crate) fn tree_click(&mut self, k: usize) {
        let Some(n) = self.tree.rows.get(k).cloned() else { return };
        if n.dir {
            self.tree.toggle(&n.path);
            self.play_event("toggle");
        } else {
            let again = self.tree.last_click.as_ref().is_some_and(|(at, p)| *p == n.path && at.elapsed().as_millis() < 450);
            self.tree.last_click = Some((Instant::now(), n.path.clone()));
            self.open_from_tree(&n.path, again);
        }
        self.dirty = true;
    }

    /// The wheel over the tree.
    pub(crate) fn tree_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.side_page != SidePage::Files || !self.sidebar_visible() || !self.tree.rect.contains(x, y) {
            return false;
        }
        self.tree.scroll -= dy;
        self.dirty = true;
        true
    }

    /// A tooltip for a sidebar rect, in the footer's manner.
    fn side_tip(&mut self, key: u64, hit: Rect, words: String) {
        let (mx, my) = self.mouse;
        let hot = hit.contains(mx, my);
        let h = self.hovers.entry(key).or_insert_with(|| crate::app::Hover { alpha: crate::anim::Anim::at(0.0), pulse: crate::anim::Anim::at(1.0), hot: false, since: Instant::now() });
        if hot != h.hot {
            h.hot = hot;
            if hot {
                h.since = Instant::now();
            }
        }
        if hot {
            let since = h.since;
            self.tip = Some(crate::app::Tip { anchor: hit, text: words, since });
            if since.elapsed().as_millis() < 700 {
                self.dirty = true;
            }
        }
    }
}

fn hover_key(s: &str) -> u64 {
    crate::app::hover_key(s, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_lists_folders_first_and_walks_open_ones() {
        let d = std::env::temp_dir().join(format!("nus-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        std::fs::write(d.join("b.txt"), "b").unwrap();
        std::fs::write(d.join("a.txt"), "a").unwrap();
        std::fs::write(d.join("src").join("main.rs"), "fn main() {}").unwrap();
        let mut t = Tree::default();
        t.set_root(Some(d.clone()));
        let names: Vec<&str> = t.rows.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["node_modules", "src", "a.txt", "b.txt"]);
        assert!(t.rows[0].dim && t.rows[0].dir);
        t.toggle(&d.join("src"));
        let names: Vec<&str> = t.rows.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["node_modules", "src", "main.rs", "a.txt", "b.txt"]);
        assert_eq!(t.rows[2].depth, 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
