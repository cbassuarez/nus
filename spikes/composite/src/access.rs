//! AccessKit: the hand-rolled chrome as an accessibility tree. Every hit
//! target the mouse can reach is a node a screen reader can reach — header
//! icons, sidebar rows, settings controls, palette rows — plus the panes.
//! Rebuilt after each frame from the same hit lists the renderer fills.

use std::collections::HashMap;

use accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

use crate::app::{App, CrumbHit, Pane};
use crate::settings::Hit;

/// What a node does when activated.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Crumb(CrumbHit),
    Welcome(usize),
    Side(crate::app::SideHit),
    Row(usize),
    Setting(Hit, f32),
    Palette(usize),
    HintSkip,
    None,
}

const ROOT: u64 = 1;
const HEADER: u64 = 2;
const SIDEBAR: u64 = 3;
const CONTENT: u64 = 4;
const PALETTE: u64 = 5;

fn bounds(r: nus_render::Rect) -> accesskit::Rect {
    accesskit::Rect { x0: r.x as f64, y0: r.y as f64, x1: r.right() as f64, y1: r.bottom() as f64 }
}

impl App {
    fn crumb_label(&self, hit: CrumbHit) -> String {
        match hit {
            CrumbHit::Space => format!("Space {}", self.space_name),
            CrumbHit::Tab => format!("tab {}", self.tabs.get(self.active).map(|t| t.title()).unwrap_or_default()),
            CrumbHit::Url => "address".into(),
            CrumbHit::Search => "search and commands".into(),
            CrumbHit::Sidebar => if self.sidebar { "unpin sidebar".into() } else { "pin sidebar".into() },
            CrumbHit::Ports => format!("{} local ports", self.ports.len()),
            CrumbHit::Assistant => "ask an assistant".into(),
            CrumbHit::Pip => "return from picture in picture".into(),
            CrumbHit::Waiting => {
                let n = self.tabs.iter().filter(|t| t.waiting()).count();
                if n > 0 { format!("{n} tabs waiting") } else { "no tabs waiting".into() }
            }
            CrumbHit::Close => "close window".into(),
            CrumbHit::Maximize => "maximize window".into(),
            CrumbHit::Minimize => "minimize window".into(),
            CrumbHit::Start => "atlas: last session and recent places".into(),
        }
    }

    /// Build the whole tree. Ids below 100 are structure; the rest are
    /// handed out per frame and mapped back to targets in `access_map`.
    pub fn access_tree(&mut self) -> TreeUpdate {
        let mut nodes: Vec<(NodeId, Node)> = Vec::new();
        let mut map: HashMap<u64, Target> = HashMap::new();
        let mut next = 100u64;
        let mut fresh = |map: &mut HashMap<u64, Target>, t: Target| {
            let id = next;
            next += 1;
            map.insert(id, t);
            NodeId(id)
        };

        // Header.
        let mut header_kids = Vec::new();
        let crumbs: Vec<(nus_render::Rect, CrumbHit)> = self.crumb_hits.clone();
        for (r, hit) in crumbs {
            let id = fresh(&mut map, Target::Crumb(hit));
            let mut n = Node::new(Role::Button);
            n.set_label(self.crumb_label(hit));
            n.set_bounds(bounds(r));
            n.add_action(Action::Click);
            nodes.push((id, n));
            header_kids.push(id);
        }
        let mut header = Node::new(Role::Toolbar);
        header.set_label("header");
        header.set_bounds(bounds(self.strip_rect()));
        header.set_children(header_kids);
        nodes.push((NodeId(HEADER), header));

        // Sidebar rows (only when visible; hidden chrome is not in the tree).
        let mut side_kids = Vec::new();
        if self.sidebar_visible() {
            let g = self.sidebar_geometry();
            let sb = self.sidebar_rect();
            for (i, y, h) in g.rows {
                let id = fresh(&mut map, Target::Row(i));
                let mut n = Node::new(Role::ListItem);
                let t = &self.tabs[i];
                let (title, detail) = t.row_text();
                let mut label = format!("{} {}", self.tab_label(i), title);
                if !detail.is_empty() {
                    label.push_str(&format!(", {detail}"));
                }
                if t.waiting() {
                    label.push_str(", waiting");
                }
                n.set_label(label);
                n.set_bounds(bounds(nus_render::Rect::new(sb.x, y, sb.w, h)));
                n.set_selected(i == self.active);
                n.add_action(Action::Click);
                nodes.push((id, n));
                side_kids.push(id);
            }
        }
        // The header and footer buttons, and any open menu.
        for (r, hit) in self.side_hits.clone() {
            use crate::app::SideHit as S;
            let label = match hit {
                S::Close(i) => format!("close tab {}", self.tabs.get(i).map(|t| t.title()).unwrap_or_default()),
                S::Profile => "profile".into(),
                S::NewTab => "new tab, choose a kind".into(),
                S::NewShell => "new tab".into(),
                S::Window => format!("window {}", self.window_name()),
                S::Kinds => "kinds of tab".into(),
                S::Kind(k) => format!("new {} tab", self.profiles.get(k).map(|p| p.name.clone()).unwrap_or_default()),
                S::KindPage => "new page".into(),
                S::WinFront(i) => format!("window {}", self.windows.get(i).map(|e| e.name.clone()).unwrap_or_default()),
                S::Rename => "rename window".into(),
                S::NewWindow | S::RailNew => "new window".into(),
                S::Rail(k) => format!("window {}", self.windows.get(k).map(|e| e.name.clone()).unwrap_or_else(|| self.window_name())),
                S::Look => "look: hot swap".into(),
                S::LookPreset(k) => format!("theme {}", crate::themes::all().into_iter().filter(|t| !t.port).nth(k).map(|p| p.name).unwrap_or_default()),
                S::LookQuick(q) => format!("quick {:?}", q).to_lowercase(),
                S::LookStudio => "open the look studio".into(),
                S::Closed => "recently closed".into(),
                S::Downloads => "downloads".into(),
                S::DlOpen(i) => format!("download {}", i + 1),
                S::Fold(i) => format!("{} {}", if self.collapsed.contains(&self.tabs[i].id) { "unfold" } else { "fold" }, self.tabs.get(i).map(|t| t.title()).unwrap_or_default()),
                S::Settings => "settings".into(),
                S::TabRename(i) => format!("rename tab {}", self.tabs.get(i).map(|t| t.title()).unwrap_or_default()),
                S::TabIcon(_) => "tab icon".into(),
                S::TabColour(_, 0) => "tab colour: none".into(),
                S::TabColour(_, k) => format!("tab colour {}", crate::surface::SWATCHES.get(k - 1).map(|s| s.0).unwrap_or("")),
                S::TabPin(i) => (if self.tabs.get(i).map(|t| t.pinned).unwrap_or(false) { "unpin tab" } else { "pin tab" }).into(),
                S::TabClose(i) => format!("close tab {}", self.tabs.get(i).map(|t| t.title()).unwrap_or_default()),
                S::TabFolder(_) => "save to a folder".into(),
                S::Folder(fi) => format!("folder {}", self.folders.get(fi).map(|f| f.name.clone()).unwrap_or_default()),
                S::FolderItem(fi, k) => self.folders.get(fi).and_then(|f| f.items.get(k)).map(|i| i.title.clone()).unwrap_or_default(),
                S::FolderDrop(_, _) => "remove from folder".into(),
                S::TabTile(i) => (if self.is_tiled(i) && self.selected.is_empty() { "untile" } else { "tile with the selected tabs" }).into(),
            };
            let id = fresh(&mut map, Target::Side(hit));
            let mut n = Node::new(Role::Button);
            n.set_label(label);
            n.set_bounds(bounds(r));
            n.add_action(Action::Click);
            nodes.push((id, n));
            side_kids.push(id);
        }
        // The welcome page's buttons.
        for (i, (r, act)) in self.welcome_hits.clone().into_iter().enumerate() {
            let id = fresh(&mut map, Target::Welcome(i));
            let mut n = Node::new(Role::Button);
            n.set_label(format!("welcome {:?}", act).to_lowercase());
            n.set_bounds(bounds(r));
            n.add_action(Action::Click);
            nodes.push((id, n));
            side_kids.push(id);
        }
        let mut side = Node::new(Role::List);
        side.set_label("tabs");
        if self.sidebar_visible() {
            side.set_bounds(bounds(self.sidebar_rect()));
        }
        side.set_children(side_kids);
        nodes.push((NodeId(SIDEBAR), side));

        // Content: the active tab's panes, and settings / hints controls.
        let mut content_kids = Vec::new();
        let mut focus = NodeId(ROOT);
        if let Some(tab) = self.tabs.get(self.active) {
            let focus_right = tab.focus_right && tab.right.is_some();
            for (is_right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
                let id = fresh(&mut map, Target::None);
                let mut n = match p {
                    Pane::Term(t) => {
                        let mut n = Node::new(Role::Terminal);
                        n.set_label(format!("terminal {}", t.title));
                        n.set_value(crate::app::last_lines(&t.term, 6).join("\n"));
                        n.set_bounds(bounds(t.rect));
                        n
                    }
                    Pane::Web(w) => {
                        let s = w.tab.shared.borrow();
                        let mut n = if let Some(rd) = &w.reader {
                            let mut n = Node::new(Role::Document);
                            n.set_label(format!("reader: {}", rd.article.title));
                            n.set_value(rd.article.plain());
                            n
                        } else {
                            let mut n = Node::new(Role::WebView);
                            n.set_label(if s.title.is_empty() { s.url.clone() } else { format!("{} — {}", s.title, s.url) });
                            n
                        };
                        n.set_bounds(bounds(w.rect));
                        n
                    }
                    Pane::Settings(s) => {
                        let mut n = Node::new(Role::Group);
                        n.set_label("settings");
                        n.set_bounds(bounds(s.rect));
                        n
                    }
                    Pane::Ports(p) => {
                        let mut n = Node::new(Role::Document);
                        n.set_label("ports");
                        n.set_bounds(bounds(p.rect));
                        n
                    }
                    Pane::Editor(e) => {
                        let mut n = Node::new(Role::Document);
                        n.set_label(format!("editor · {}", e.title()));
                        n.set_bounds(bounds(e.rect));
                        n
                    }
                    Pane::Hints(h) => {
                        let mut n = Node::new(Role::Document);
                        n.set_label("five things to try");
                        let done = self.hints.iter().filter(|&&h| h).count();
                        n.set_value(format!("{done} of 5 done"));
                        n.set_bounds(bounds(h.rect));
                        n
                    }
                    Pane::Home(h) => {
                        let mut n = Node::new(Role::TextInput);
                        n.set_label("the prompt");
                        n.set_value(h.input.clone());
                        n.set_bounds(bounds(h.rect));
                        n
                    }
                };
                if is_right == focus_right {
                    focus = id;
                }
                n.add_action(Action::Focus);
                // Settings controls hang off the settings pane.
                if matches!(p, Pane::Settings(_)) {
                    let mut kids = Vec::new();
                    let hits: Vec<(nus_render::Rect, Hit, String)> = self.settings_hits.iter().map(|(r, h)| (*r, *h, self.setting_label(*h))).collect();
                    for (r, hit, label) in hits {
                        let cid = fresh(&mut map, Target::Setting(hit, r.x + r.w / 2.0));
                        let mut c = match hit {
                            Hit::Section(_) | Hit::Tile(_) => Node::new(Role::Tab),
                            Hit::Slider(kind, _, _) => {
                                let mut c = Node::new(Role::Slider);
                                c.set_numeric_value(self.slider_value(kind) as f64 * 100.0);
                                c
                            }
                            Hit::ReloadRules | Hit::OpenRules | Hit::ResetRules | Hit::Back => Node::new(Role::Button),
                            _ => Node::new(Role::RadioButton),
                        };
                        c.set_label(label);
                        c.set_bounds(bounds(r));
                        c.add_action(Action::Click);
                        nodes.push((cid, c));
                        kids.push(cid);
                    }
                    n.set_children(kids);
                }
                if matches!(p, Pane::Hints(_)) {
                    let mut kids = Vec::new();
                    let hits: Vec<(nus_render::Rect, usize)> = self.hint_hits.clone();
                    for (r, k) in hits {
                        let (role, label, target) = if k == usize::MAX {
                            (Role::Button, "skip the tour".to_string(), Target::HintSkip)
                        } else {
                            let (chord, what) = crate::app::HINTS[k];
                            (Role::CheckBox, format!("{chord}: {what}"), Target::None)
                        };
                        let cid = fresh(&mut map, target);
                        let mut c = Node::new(role);
                        c.set_label(label);
                        c.set_bounds(bounds(r));
                        if k != usize::MAX {
                            c.set_toggled(if self.hints[k] { accesskit::Toggled::True } else { accesskit::Toggled::False });
                        } else {
                            c.add_action(Action::Click);
                        }
                        nodes.push((cid, c));
                        kids.push(cid);
                    }
                    n.set_children(kids);
                }
                nodes.push((id, n));
                content_kids.push(id);
            }
        }
        let mut content = Node::new(Role::Group);
        content.set_label("content");
        content.set_bounds(bounds(self.content_rect()));
        content.set_children(content_kids);
        nodes.push((NodeId(CONTENT), content));

        // Palette, while open: a list box the arrow keys walk.
        let mut root_kids = vec![NodeId(HEADER), NodeId(SIDEBAR), NodeId(CONTENT)];
        if let Some((mode, input)) = self.palette.clone() {
            let rows = self.palette_rows(mode, &input);
            let mut kids = Vec::new();
            for (i, row) in rows.iter().enumerate() {
                let id = fresh(&mut map, Target::Palette(i));
                let mut n = Node::new(Role::ListBoxOption);
                n.set_label(row.text.clone());
                if let Some(r) = self.palette_hits.get(i) {
                    n.set_bounds(bounds(*r));
                }
                n.set_selected(i == self.palette_sel);
                n.add_action(Action::Click);
                if i == self.palette_sel {
                    focus = id;
                }
                nodes.push((id, n));
                kids.push(id);
            }
            let mut pal = Node::new(Role::ListBox);
            pal.set_label(format!("{:?} palette: {}", mode, if input.is_empty() { "type to search".to_string() } else { input }));
            pal.set_children(kids);
            nodes.push((NodeId(PALETTE), pal));
            root_kids.push(NodeId(PALETTE));
        }

        let mut root = Node::new(Role::Window);
        root.set_label("nus");
        root.set_children(root_kids);
        nodes.push((NodeId(ROOT), root));

        self.access_map = map;
        let mut info = TreeInfo::new(NodeId(ROOT));
        info.toolkit_name = Some("nus".into());
        TreeUpdate { nodes, tree: Some(info), tree_id: TreeId::ROOT, focus }
    }

    /// A screen reader (or automation) activated a node.
    pub fn access_action(&mut self, req: ActionRequest) {
        let Some(&target) = self.access_map.get(&req.target_node.0) else { return };
        match (req.action, target) {
            (Action::Click, Target::Crumb(hit)) => self.crumb_action(hit),
            (Action::Click, Target::Row(i)) => {
                self.selected.clear();
                self.activate(i);
            }
            (Action::Click, Target::Side(h)) => self.side_action(h, false),
            (Action::Click, Target::Welcome(i)) => {
                if let Some((_, act)) = self.welcome_hits.get(i).cloned() {
                    self.welcome_act(act);
                }
            }
            (Action::Click, Target::Setting(hit, x)) => {
                self.apply_setting(hit, x);
                self.save_prefs();
            }
            (Action::Click, Target::Palette(i)) => {
                self.palette_sel = i;
                self.palette_commit();
            }
            (Action::Click, Target::HintSkip) => self.dismiss_hints(),
            _ => {}
        }
        self.dirty = true;
    }
}
