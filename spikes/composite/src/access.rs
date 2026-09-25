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
    Download(crate::downloads::Hit),
    Replay(crate::replay::HistoryHit),
    Mercury(crate::me::CardHit),
    Import(crate::me::CardHit),
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
            CrumbHit::Menu => "Application menu (F10)".into(),
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
            CrumbHit::Maximize => if cfg!(target_os="macos") {"enter full screen"} else {"maximize window"}.into(),
            CrumbHit::Minimize => "minimize window".into(),
            CrumbHit::Start => "atlas: last session and recent places".into(),
            CrumbHit::Agents => { let (w, k) = self.agent_counts(); format!("assistants: {w} waiting, {k} working") }
            CrumbHit::Nus => if self.home_latch.is_some() { "back to where you were".into() } else { "home".into() },
            CrumbHit::FinishWork => crate::finish_work::tooltip(crate::finish_work::view().phase, crate::finish_work::view().capability),
            CrumbHit::Updates => {let s=crate::updates::status();if s.busy{"Update in progress. View status"}else if s.available{"Update ready. Review update and restart"}else{"Updates. Check for a new version"}.into()},
        }
    }

    /// Build the whole tree. Ids below 100 are structure; the rest are
    /// handed out per frame and mapped back to targets in `access_map`.
    pub fn access_tree(&mut self) -> TreeUpdate {
        if let Some(tree) = self.mercury_access_tree() { return tree; }
        if let Some(tree) = self.import_access_tree() { return tree; }
        if let Some(tree) = self.page_menu_access_tree() { return tree; }
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
                S::Pinned(act) => act.label(&self.pins.items),
                S::MenuDrawer => "nus menu drawer".into(),
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
                S::Files => "files, the folder this window works in".into(),
                S::FilesPin => "keep this folder, or let it follow the shell".into(),
                S::FilesUp => "up one folder".into(),
                S::FileRow(k) => self.tree.rows.get(k).map(|n| if n.dir { format!("folder {}", n.name) } else { format!("file {}", n.name) }).unwrap_or_default(),
                S::Downloads => "downloads".into(),
                S::Fold(i) => format!("{} {}", if self.collapsed.contains(&self.tabs[i].id) { "unfold" } else { "fold" }, self.tabs.get(i).map(|t| t.title()).unwrap_or_default()),
                S::Settings => "settings".into(),
                S::TabRename(i) => format!("rename tab {}", self.tabs.get(i).map(|t| t.title()).unwrap_or_default()),
                S::TabIcon(_) => "tab icon".into(),
                S::TabColour(_, 0) => "tab color: none".into(),
                S::TabColour(_, k) => format!("tab color {}", crate::surface::SWATCHES.get(k - 1).map(|s| s.0).unwrap_or("")),
                S::TabPin(i) => (if self.tabs.get(i).map(|t| t.pinned).unwrap_or(false) { "unpin tab" } else { "pin tab" }).into(),
                S::Answer(i, a) => format!("{} for tab {}", match a { crate::agent::Answer::Allow => "allow", crate::agent::Answer::Deny => "deny", crate::agent::Answer::Always => "always allow" }, i + 1),
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
        for (i, (r, act)) in self.welcome_hits.clone().into_iter().enumerate().filter(|_| self.tabs.get(self.active).is_some_and(|t|matches!(t.left,Pane::Hints(_))||matches!(t.right,Some(Pane::Hints(_))))) {
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
                    Pane::Downloads(p) => {let mut n=Node::new(Role::Document);n.set_label("downloads");n.set_bounds(bounds(p.rect));n}
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
                if let Pane::Settings(settings) = p {
                    let states = self.setting_states(settings.section);
                    let mut kids = Vec::new();
                    let hits: Vec<(nus_render::Rect, Hit, String)> = self.settings_hits.iter().map(|(r, h)| (*r, *h, self.setting_label(*h))).collect();
                    for (r, hit, label) in hits {
                        let cid = fresh(&mut map, Target::Setting(hit, r.x + r.w / 2.0));
                        let mut c = match hit {
                            Hit::Section(_) | Hit::Tile(_) => Node::new(Role::Tab),
                            Hit::Slider(kind, _, _) => {
                                let mut c = Node::new(Role::Slider);
                                c.set_numeric_value(self.slider_value(kind) as f64 * 100.0);
                                c.set_min_numeric_value(0.0);
                                c.set_max_numeric_value(100.0);
                                c.set_numeric_value_step(5.0);
                                c.add_action(Action::SetValue);
                                c.add_action(Action::Increment);
                                c.add_action(Action::Decrement);
                                c
                            }
                            Hit::ReloadRules | Hit::OpenRules | Hit::ResetRules | Hit::Back |
                            Hit::AddArt | Hit::AskArt | Hit::OpenArtFolder | Hit::EditHomeUrl | Hit::SetLaunchTabs | Hit::ClearLaunchTabs => Node::new(Role::Button),
                            h if App::setting_is_action(h) => Node::new(Role::Button),
                            Hit::AskCtx(_) | Hit::FooterTheme(_) => Node::new(Role::CheckBox),
                            _ => Node::new(Role::RadioButton),
                        };
                        c.set_label(label);
                        if let Some(selected) = match hit { Hit::Section(k)|Hit::Tile(k) => if let Pane::Settings(s)=p {Some(k==s.section)}else{None}, _=>None }.or_else(||states.iter().find(|(h,_)|*h==hit).map(|(_,v)|*v).or_else(||self.startup_choice_selected(hit))) {
                            c.set_toggled(if selected { accesskit::Toggled::True } else { accesskit::Toggled::False });
                        }
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

        if !self.download_ui.hits.is_empty() {
            let mut kids=Vec::new();
            for d in crate::downloads::list() {
                let id=fresh(&mut map,Target::None);let mut n=Node::new(Role::Label);n.set_label(format!("{} · {}",d.name,d.status()));nodes.push((id,n));kids.push(id);
            }
            for (r,hit) in &self.download_ui.hits {
                let id=fresh(&mut map,Target::Download(*hit));let mut n=Node::new(if *hit==crate::downloads::Hit::Search{Role::TextInput}else{Role::Button});n.set_label(self.download_label(*hit));n.set_bounds(bounds(*r));n.add_action(Action::Click);n.add_action(Action::Focus);
                if *hit==crate::downloads::Hit::Search {n.set_value(self.download_ui.query.clone());n.add_action(Action::SetValue);}
                nodes.push((id,n));kids.push(id);
                if self.download_ui.focus==Some(*hit){focus=id;}
            }
            let mut group=Node::new(if self.dl_menu{Role::Dialog}else{Role::Group});group.set_label("Downloads");group.set_children(kids);
            if self.dl_menu{group.set_modal();if let Some(r)=self.download_ui.rect{group.set_bounds(bounds(r));}}
            nodes.push((NodeId(6),group));root_kids.push(NodeId(6));
        }
        if let Some(tl)=&self.timeline {
            let mut kids=Vec::new();
            for (r,hit) in &tl.hits {
                let id=fresh(&mut map,Target::Replay(*hit));
                let mut n=Node::new(match hit {crate::replay::HistoryHit::Search=>Role::TextInput,crate::replay::HistoryHit::Map=>Role::Slider,_=>Role::Button});
                let label=if let crate::replay::HistoryHit::Record(i)=hit {tl.records.get(*i).map(|v|format!("{} · exit {} · {}",v["cmd"].as_str().unwrap_or(""),v["exit"],v["cwd"].as_str().unwrap_or(""))).unwrap_or_default()}else{hit.label().into()};
                n.set_label(label);n.set_bounds(bounds(*r));n.add_action(Action::Focus);
                if *hit==crate::replay::HistoryHit::Search {n.set_value(tl.query.clone());n.add_action(Action::SetValue);}
                else if *hit==crate::replay::HistoryHit::Map {n.set_numeric_value(tl.detail_scroll as f64);n.set_min_numeric_value(0.0);n.set_max_numeric_value(tl.detail_max as f64);n.add_action(Action::SetValue);n.add_action(Action::Increment);n.add_action(Action::Decrement);}
                else {n.add_action(Action::Click);}
                if tl.focus==Some(*hit){focus=id;}nodes.push((id,n));kids.push(id);
            }
            if let Some(v)=tl.records.get(tl.at){let id=fresh(&mut map,Target::None);let mut n=Node::new(Role::Document);n.set_label("Recorded command output");n.set_value(v["output"].as_str().unwrap_or(""));nodes.push((id,n));kids.push(id);}
            let mut n=Node::new(Role::Group);n.set_label("Session history and map");n.set_children(kids);nodes.push((NodeId(7),n));root_kids.push(NodeId(7));
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
        if let Some(Target::Import(hit))=self.access_map.get(&req.target_node.0).copied() {
            match req.action {Action::Click=>self.me_hit(hit),Action::Focus=>{if let crate::me::CardHit::ImportSource(i)=hit{self.me_card.import.source=i;}self.me_card.import.focus=usize::from(hit==crate::me::CardHit::ImportOpen);self.dirty=true;},_=>{}}
            return;
        }
        if self.me_card.mercury_reveal.is_some() {
            if let Some(Target::Mercury(hit)) = self.access_map.get(&req.target_node.0).copied() {
                match req.action {
                    Action::Click => self.mercury_action(hit),
                    Action::Focus => {
                        if let Some(r) = &mut self.me_card.mercury_reveal { r.keyboard_focus = true; }
                        self.dirty = true;
                    }
                    _ => {}
                }
            }
            return;
        }
        if self.page_menu.is_some() { self.page_menu_access_action(req); return; }
        let Some(&target) = self.access_map.get(&req.target_node.0) else { return };
        if let Target::Replay(hit)=target {
            match req.action {
                Action::Click=>self.timeline_action(hit),
                Action::Focus=>{if let Some(tl)=&mut self.timeline{tl.focus=Some(hit);}},
                Action::Increment|Action::Decrement=>{self.timeline_key(&winit::keyboard::Key::Named(if req.action==Action::Increment{winit::keyboard::NamedKey::ArrowDown}else{winit::keyboard::NamedKey::ArrowUp}));},
                Action::SetValue=>{if let Some(tl)=&mut self.timeline {match req.data {
                    Some(accesskit::ActionData::Value(value))=>{tl.query=value.to_string();tl.filter_changed();},
                    Some(accesskit::ActionData::NumericValue(value)) if value.is_finite()=>{tl.detail_scroll=(value as f32).clamp(0.0,tl.detail_max);tl.reveal=false;tl.follow_scroll=true;},_=>{}
                }}},_=>{}
            }
            self.dirty=true;return;
        }
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
            (action @ (Action::SetValue | Action::Increment | Action::Decrement), Target::Setting(Hit::Slider(kind, _, _), _)) => {
                let value = match action {
                    Action::Increment => self.slider_value(kind) + 0.05,
                    Action::Decrement => self.slider_value(kind) - 0.05,
                    _ => match req.data { Some(accesskit::ActionData::NumericValue(v)) if v.is_finite() => v as f32 / 100.0, _ => return },
                };
                self.set_slider(kind, value);
                self.save_prefs();
            }
            (Action::Click, Target::Setting(hit, x)) => {
                self.apply_setting(hit, x);
                self.save_prefs();
            }
            (Action::Click, Target::Palette(i)) => {
                self.palette_sel = i;
                self.palette_commit();
            }
            (Action::Click, Target::Download(hit)) => self.download_action(hit),
            (Action::Focus, Target::Download(hit)) => {self.download_ui.focus=Some(hit);self.dirty=true;},
            (Action::SetValue, Target::Download(crate::downloads::Hit::Search)) => {if let Some(accesskit::ActionData::Value(value))=req.data{self.download_ui.query=value.to_string();self.download_ui.cursor=self.download_ui.query.len();self.download_ui.select_all=false;self.download_query_changed();}},
            (Action::Click, Target::HintSkip) => self.dismiss_hints(),
            _ => {}
        }
        self.dirty = true;
    }
}
