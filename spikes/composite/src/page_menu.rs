//! One context-menu renderer for CEF, media and native Reading entries.
//! CEF command ids are returned unchanged; native commands keep their Entry id.
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style, Theme};
use crate::anim::Anim;
use crate::app::{App, Pane};
use crate::browser::{self, Media};
use cef::ImplRunContextMenuCallback;
use std::{sync::atomic::{AtomicU64, Ordering}, time::Instant};
use winit::{event::{ElementState, MouseButton, MouseScrollDelta}, keyboard::{Key, NamedKey}};

#[path = "context_menu_model.rs"]
mod model;

// A new menu gets a different accessibility identity. A delayed action for a
// closed menu must not activate the same row number in the next menu.
static ACCESS_IDS: AtomicU64 = AtomicU64::new(1u64 << 52);
const MAX_ITEMS: usize = 4096;

pub enum Source {
    Page(cef::RunContextMenuCallback),
    Media(Vec<Media>),
    Reading { entry: String, actions: Vec<Option<crate::library::MenuAction>> },
}

pub struct PageMenu {
    pub tab: u64,
    pub right: bool,
    pub at: (f32, f32),
    pub items: Vec<model::Item>,
    pub source: Source,
    pub hits: Vec<(Rect, i32)>,
    pub rise: Anim,
    pub selected: Option<i32>,
    panel: Option<Rect>,
    scroll: f32,
    scroll_max: f32,
    keyboard: bool,
    reveal: bool,
    query: String,
    typed: Instant,
    access_base: u64,
}

impl PageMenu {
    fn new(tab: u64, right: bool, at: (f32, f32), mut items: Vec<model::Item>, source: Source) -> Self {
        items.truncate(MAX_ITEMS);
        Self { tab, right, at, selected: model::edge(&items, false), items, source,
            hits: Vec::new(), rise: Anim::at(0.0), panel: None, scroll: 0.0, scroll_max: 0.0,
            keyboard: true, reveal: true, query: String::new(), typed: crate::clock::now(),
            access_base: ACCESS_IDS.fetch_add(MAX_ITEMS as u64 + 1, Ordering::Relaxed) }
    }
    fn cancel(self) {
        if let Source::Page(cb) = self.source { cb.cancel(); }
    }
}

impl App {
    fn show_context_menu(&mut self, mut menu: PageMenu) {
        if self.palette.is_some() || self.start.is_some() || self.me_card.open
            || self.timeline.is_some() || self.splash.is_some() || self.board.open {
            menu.cancel();
            return;
        }
        self.close_menus();
        self.close_page_menu();
        self.dismiss_tip();
        self.press = None;
        self.drag_armed = None;
        // A context menu ends native selection gestures, never leaves a
        // hidden reader drag waiting for a release consumed by the menu.
        for tab in &mut self.tabs {
            for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Home(h) = pane {
                    if let Some(reading) = &mut h.reading { reading.reader.saved.dragging = false; }
                }
            }
        }
        menu.rise.replay(0.0, 1.0, self.motion.dur(120.0));
        self.page_menu = Some(menu);
        self.dirty = true;
    }

    pub(crate) fn open_reading_menu(&mut self, entry: String, at: (f32, f32),
        rows: Vec<(Option<crate::library::MenuAction>, String, bool)>) {
        if crate::private::enabled() { return; }
        let Some(tab) = self.tabs.get(self.active) else { return; };
        let (id, right) = (tab.id, tab.focus_right && tab.right.is_some());
        let actions = rows.iter().map(|(action, _, _)| *action).collect();
        let items = rows.into_iter().enumerate().map(|(i, (action, label, enabled))|
            (i as i32, label, enabled && action.is_some())).collect();
        self.show_context_menu(PageMenu::new(id, right, at, items, Source::Reading { entry, actions }));
    }

    pub(crate) fn poll_page_menus(&mut self) {
        let scale = self.scale;
        let active = self.tabs.get(self.active).map(|t| t.id);
        let mut opens = Vec::new();
        let mut edits=Vec::new();
        let mut saves=Vec::new();
        let mut said = Vec::new();
        let mut menu: Option<PageMenu> = None;
        for tab in &self.tabs {
            for (right, pane) in [(false, &tab.left)].into_iter().chain(tab.right.as_ref().map(|p| (true, p))) {
                let Pane::Web(w) = pane else { continue; };
                let mut shared = w.tab.shared.borrow_mut();
                opens.append(&mut shared.opens);
                if let Some(path)=shared.edit_source.take(){edits.push(path);}
                if std::mem::take(&mut shared.save_reading){saves.push((tab.id,right));}
                if let Some(word) = shared.said.take() { said.push(word); }
                if let Some(req) = shared.menu.take() {
                    if Some(tab.id) != active || w.reader.is_some() {
                        req.callback.cancel();
                        continue;
                    }
                    if let Some(old) = menu.take() { old.cancel(); }
                    menu = Some(PageMenu::new(tab.id, right,
                        (w.page.x + req.x * scale, w.page.y + req.y * scale),
                        req.items, Source::Page(req.callback)));
                }
            }
        }
        for path in edits {self.open_file(&path,true);}
        for (id,right) in saves {if let Some(i)=self.tabs.iter().position(|t|t.id==id){self.tabs[i].focus_right=right;self.activate(i);self.save_reading();}}
        for (url, beside) in opens { self.open_url(&url, !beside); }
        for word in said {
            if word == "PIP" { self.run(crate::app::Action::Pip); }
            else { self.toast(word, None); }
        }
        if let Some(menu) = menu { self.show_context_menu(menu); }
    }

    pub(crate) fn media_menu(&mut self, tab: u64, right: bool, at: (f32, f32)) {
        let Some(w) = self.tabs.iter().find(|t| t.id == tab)
            .and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) })
            .and_then(|p| if let Pane::Web(w) = p { Some(w) } else { None }) else { return; };
        let media = w.tab.shared.borrow().media.clone();
        let saveable: Vec<_> = media.iter().filter(|m| !m.blob).collect();
        if saveable.len() == 1 {
            let src = saveable[0].src.clone();
            w.tab.download(&src);
            self.toast(format!("SAVING · {}", crate::app::fit_cmd(&src, 60)), None);
            return;
        }
        let mut items = Vec::new();
        for (i, item) in media.iter().enumerate() {
            let what = if item.kind == "audio" { "AUDIO" } else { "VIDEO" };
            let size = if item.w > 0 { format!(" · {}×{}", item.w, item.h) } else { String::new() };
            let ext = item.src.rsplit('.').next().filter(|e| e.len() <= 4 && !e.contains('/'))
                .map(|e| format!(" · {}", e.to_uppercase())).unwrap_or_default();
            items.push((i as i32, if item.blob { format!("{what}{size} · STREAMS, NOTHING TO SAVE") }
                else { format!("SAVE {what}{size}{ext}") }, !item.blob));
        }
        if !items.is_empty() { self.show_context_menu(PageMenu::new(tab, right, at, items, Source::Media(media))); }
    }

    pub(crate) fn close_page_menu(&mut self) {
        if let Some(menu) = self.page_menu.take() {
            menu.cancel();
            self.dismiss_tip();
            self.dirty = true;
        }
    }

    /// Validate eligibility centrally; keyboard and accessibility cannot bypass
    /// disabled rows. Taking the menu transfers exactly one CEF callback owner.
    fn page_menu_pick(&mut self, id: i32) {
        if !self.context_menu_owner_exists() { self.close_page_menu(); return; }
        if !self.page_menu.as_ref().is_some_and(|m| model::enabled(&m.items, id)) { return; }
        let Some(menu) = self.page_menu.take() else { return; };
        self.dismiss_tip();
        match menu.source {
            Source::Page(cb) => cb.cont(id, cef::EventFlags::default()),
            Source::Media(media) => {
                if let Some(item) = usize::try_from(id).ok().and_then(|i| media.get(i)).filter(|m| !m.blob) {
                    if let Some(Pane::Web(w)) = self.tabs.iter().find(|t| t.id == menu.tab)
                        .and_then(|t| if menu.right { t.right.as_ref() } else { Some(&t.left) }) {
                        w.tab.download(&item.src);
                        self.toast(format!("SAVING · {}", crate::app::fit_cmd(&item.src, 60)), None);
                    }
                }
            },
            Source::Reading { entry, actions } => {
                if let Some(action) = usize::try_from(id).ok().and_then(|i| actions.get(i)).copied().flatten() {
                    self.library_menu_action(&entry, action, menu.at);
                }
            },
        }
        self.dirty = true;
    }

    pub(crate) fn page_menu_click(&mut self, x: f32, y: f32) -> bool {
        let Some(menu) = self.page_menu.as_ref() else { return false; };
        let hit = menu.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, id)| *id);
        if let Some(id) = hit { self.page_menu_pick(id); }
        else { self.close_page_menu(); }
        true
    }

    /// Swallow the matching release even when the press closed/replaced a menu.
    /// This prevents a menu pick from releasing a background pin/drag/control.
    pub(crate) fn page_menu_mouse(&mut self, button: MouseButton, state: ElementState) -> bool {
        if state == ElementState::Released && self.page_menu_buttons.remove(&button) { return true; }
        if self.page_menu.is_none() { return false; }
        if state == ElementState::Pressed {
            self.page_menu_buttons.insert(button);
            if button == MouseButton::Left { self.page_menu_click(self.mouse.0, self.mouse.1); }
            else { self.close_page_menu(); }
        }
        true
    }

    pub(crate) fn page_menu_motion(&mut self, x: f32, y: f32) -> bool {
        let Some(menu) = self.page_menu.as_mut() else { return false; };
        let selected = menu.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, id)| *id);
        if menu.selected != selected || menu.keyboard {
            menu.selected = selected;
            menu.keyboard = false;
            self.dirty = true;
        }
        true
    }

    pub(crate) fn page_menu_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        if self.page_menu.is_none() { return false; }
        let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y * self.px(30.0),
            MouseScrollDelta::PixelDelta(p) => p.y as f32 };
        if !dy.is_finite() { return true; }
        let inside = self.page_menu.as_ref().and_then(|m| m.panel).is_some_and(|r| r.contains(self.mouse.0, self.mouse.1));
        if inside {
            if let Some(menu) = self.page_menu.as_mut() {
                menu.scroll = (menu.scroll - dy).clamp(0.0, menu.scroll_max);
                menu.keyboard = false; menu.selected = None; menu.reveal = false;
                // Hits describe the last frame, not the new scroll offset.
                menu.hits.clear();
            }
            self.dirty = true;
        } else { self.close_page_menu(); }
        true
    }

    pub(crate) fn page_menu_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if ev.state == ElementState::Released && self.page_menu_keys.remove(&ev.physical_key) { return true; }
        let Some(menu) = self.page_menu.as_mut() else {
            // A held Enter/Escape which closed the menu must not autorepeat
            // into the reader, terminal or page before its matching release.
            return ev.repeat && self.page_menu_keys.contains(&ev.physical_key);
        };
        if ev.state != ElementState::Pressed { return true; }
        self.page_menu_keys.insert(ev.physical_key);
        menu.keyboard = true;
        menu.reveal = true;
        let mut pick = None;
        let mut close = false;
        match &ev.logical_key {
            Key::Named(NamedKey::Escape | NamedKey::Tab) => close = true,
            Key::Named(NamedKey::ArrowDown) => menu.selected = model::step(&menu.items, menu.selected, false),
            Key::Named(NamedKey::ArrowUp) => menu.selected = model::step(&menu.items, menu.selected, true),
            Key::Named(NamedKey::Home) => menu.selected = model::edge(&menu.items, false),
            Key::Named(NamedKey::End) => menu.selected = model::edge(&menu.items, true),
            Key::Named(NamedKey::Enter | NamedKey::Space) if !ev.repeat => pick = menu.selected,
            Key::Character(text) if !self.mods.control_key() && !self.mods.super_key() && !self.mods.alt_key() => {
                let now = crate::clock::now();
                if now.saturating_duration_since(menu.typed).as_millis() >= 700 { menu.query.clear(); }
                let remaining = 64 - menu.query.chars().count().min(64);
                menu.query.extend(text.chars().filter(|c| !c.is_control()).take(remaining));
                menu.typed = now;
                menu.selected = model::prefix(&menu.items, menu.selected, &menu.query);
            },
            _ => {},
        }
        if close { self.close_page_menu(); }
        else if let Some(id) = pick { self.page_menu_pick(id); }
        self.dirty = true;
        true
    }

    fn context_menu_owner_exists(&self) -> bool {
        let Some(menu) = &self.page_menu else { return false; };
        self.tabs.get(self.active).filter(|t| t.id == menu.tab)
            .and_then(|t| if menu.right { t.right.as_ref() } else { Some(&t.left) })
            .is_some_and(|pane| match &menu.source {
                Source::Reading { .. } => matches!(pane, Pane::Home(h) if h.library),
                Source::Page(_) | Source::Media(_) => matches!(pane, Pane::Web(w) if w.reader.is_none()),
            })
    }

    pub(crate) fn draw_page_menu(&mut self, scene: &mut Scene) {
        if self.page_menu.is_none() { return; }
        if !self.context_menu_owner_exists() { self.close_page_menu(); return; }
        let menu = self.page_menu.as_ref().unwrap();
        let items = menu.items.clone();
        let (at, mut scroll, selected, keyboard, reveal, k) =
            (menu.at, menu.scroll, menu.selected, menu.keyboard, menu.reveal, menu.rise.value());
        let labels: Vec<String> = items.iter().map(|(_, text, _)| text.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        let shortcuts: Vec<String> = items.iter().map(|(id, _, _)| {
            if !matches!(menu.source, Source::Page(_)) { return String::new(); }
            match *id { browser::CMD_COPY_PAGE => crate::app::key("C", true),
                113 => crate::app::key("C", false), 114 => crate::app::key("V", false),
                112 => crate::app::key("X", false), 117 => crate::app::key("A", false),
                110 => crate::app::key("Z", false), 102 => crate::app::key("R", false), _ => String::new() }
        }).collect();
        let icons: Vec<Option<(&'static str, &'static str)>> = items.iter().map(|(id, _, _)| {
            use nus_render::text::icons;
            match &menu.source {
                Source::Media(_) => Some(icons::DOWNLOAD),
                Source::Reading { actions, .. } => {
                    use crate::library::MenuAction;
                    match usize::try_from(*id).ok().and_then(|i| actions.get(i)).copied().flatten() {
                        Some(MenuAction::OpenSaved) => Some(icons::BOOK),
                        Some(MenuAction::Original) => Some(icons::TO_TAB),
                        Some(MenuAction::CopySource | MenuAction::CopySelection) => Some(icons::COPY),
                        Some(MenuAction::Refresh) => Some(icons::RELOAD),
                        Some(MenuAction::Find) => Some(icons::SEARCH),
                        _ => None,
                    }
                },
                Source::Page(_) => match *id {
                    browser::CMD_SAVE_MEDIA => Some(icons::DOWNLOAD),
                    browser::CMD_COPY_MEDIA | browser::CMD_COPY_LINK | browser::CMD_COPY_PAGE | 113 => Some(icons::COPY),
                    browser::CMD_OPEN_MEDIA | browser::CMD_OPEN_LINK => Some(icons::TO_TAB),
                    browser::CMD_OPEN_LINK_BESIDE => Some(icons::SWAP),
                    browser::CMD_PIP => Some(icons::PIP),
                    100 => Some(icons::BACK), 101 => Some(icons::FORWARD),
                    102 => Some(icons::RELOAD), 132 => Some(icons::CODE),
                    _ => None,
                },
            }
        }).collect();
        let t = self.theme.clone();
        let label = self.label();
        let row = self.px(30.0).max(label.px * 1.8);
        let separator = self.px(9.0);
        let pad = self.px(12.0);
        let margin = self.px(8.0);
        let (win_w, win_h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        if win_w <= margin * 2.0 || win_h <= margin * 2.0 { self.close_page_menu(); return; }
        let icon_size = self.px(12.0);
        let icon_slot = icon_size + self.px(6.0);
        let measured = labels.iter().zip(&shortcuts).map(|(text, key)| self.fonts.measure(label, text)
            + if key.is_empty() { 0.0 } else { self.fonts.measure(label, key) + pad }).fold(self.px(244.0), f32::max);
        let width = (measured.min(self.px(520.0)) + 2.0 * pad + icon_slot).min(win_w - 2.0 * margin);
        let total = items.iter().map(|(_, text, _)| if text.is_empty() { separator } else { row }).sum::<f32>();
        let height = (total + 2.0).min(win_h - 2.0 * margin);
        let max_scroll = (total - (height - 2.0).max(0.0)).max(0.0);
        scroll = scroll.clamp(0.0, max_scroll);
        if reveal {
            let mut offset = 0.0;
            for (id, text, _) in &items {
                let h = if text.is_empty() { separator } else { row };
                if Some(*id) == selected {
                    scroll = scroll.min(offset).max(offset + h - (height - 2.0)).clamp(0.0, max_scroll);
                    break;
                }
                offset += h;
            }
        }
        let x = if at.0.is_finite() { at.0.round() } else { margin }.clamp(margin, win_w - width - margin);
        let y = if at.1.is_finite() { at.1.round() } else { margin };
        let y = if y + height > win_h - margin { y - height } else { y }.clamp(margin, win_h - height - margin);
        let panel = Rect::new(x, y, width, height);
        let shown = Rect::new(x, y, width, height * k.clamp(0.0, 1.0));
        let outer = scene.clip();
        scene.layer(Some(shown));
        scene.rect(panel, t.paper);
        let mut hits = Vec::new();
        let mut yy = y + 1.0 - scroll;
        for (i, (id, text, enabled)) in items.iter().enumerate() {
            if text.is_empty() {
                scene.hline(x + pad, yy + separator * 0.5, (width - 2.0 * pad).max(0.0), self.px(m::HAIRLINE), Theme::with_alpha(t.ink, 0.25));
                yy += separator; continue;
            }
            let cell = Rect::new(x, yy, width, row);
            let clipped = cell.intersect(&shown);
            let hot = *enabled && if keyboard { selected == Some(*id) } else { clipped.w > 0.0 && clipped.h > 0.0 && clipped.contains(self.mouse.0, self.mouse.1) };
            if hot { scene.rect(cell, t.tint); }
            let ink = if *enabled { t.ink } else { t.dim };
            let kw = self.fonts.measure(label, &shortcuts[i]);
            let available = (width - 2.0 * pad - icon_slot - if kw > 0.0 { kw + pad } else { 0.0 }).max(0.0);
            if let Some(icon) = icons[i] {
                self.fonts.draw_icon(scene, icon, icon_size, x + pad, yy + (row - icon_size) * 0.5, ink);
            }
            let text = self.fit(label, &labels[i], available);
            self.fonts.draw(scene, Style { color: ink, ..label }, x + pad + icon_slot, yy + (row + label.px) * 0.5 - self.px(2.0), &text);
            if kw > 0.0 {
                self.fonts.draw(scene, Style { color: t.dim, ..label }, x + width - pad - kw, yy + (row + label.px) * 0.5 - self.px(2.0), &shortcuts[i]);
            }
            if *enabled && clipped.w > 0.0 && clipped.h > 0.0 { hits.push((clipped, *id)); }
            yy += row;
        }
        if max_scroll > 0.0 {
            let track = (height - 4.0).max(1.0);
            let thumb = (track * (height / (total + 2.0))).max(self.px(12.0)).min(track);
            scene.rect(Rect::new(x + width - self.px(4.0), y + 2.0 + (track - thumb) * scroll / max_scroll,
                self.px(2.0), thumb), t.dim);
        }
        scene.outline(shown, self.px(m::HAIRLINE), t.ink);
        scene.layer(outer);
        if let Some(menu) = self.page_menu.as_mut() {
            menu.panel = Some(panel); menu.hits = hits; menu.scroll = scroll;
            menu.scroll_max = max_scroll; menu.reveal = false;
        }
        if k < 1.0 { self.dirty = true; }
    }

    pub(crate) fn page_menu_access_tree(&self) -> Option<accesskit::TreeUpdate> {
        use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let menu = self.page_menu.as_ref()?;
        let root_id = NodeId(1);
        let menu_id = NodeId(menu.access_base);
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut focus = menu_id;
        for (index, (command, text, enabled)) in menu.items.iter().enumerate() {
            if text.is_empty() { continue; }
            let id = NodeId(menu.access_base + index as u64 + 1);
            let mut node = Node::new(Role::MenuItem);
            node.set_label(text.clone());
            if *enabled { node.add_action(Action::Click); node.add_action(Action::Focus); }
            else { node.set_disabled(); }
            if let Some((r, _)) = menu.hits.iter().find(|(_, key)| key == command) {
                node.set_bounds(accesskit::Rect { x0: r.x as f64, y0: r.y as f64, x1: r.right() as f64, y1: r.bottom() as f64 });
            }
            if menu.selected == Some(*command) { focus = id; }
            nodes.push((id, node)); children.push(id);
        }
        let mut group = Node::new(Role::Menu);
        group.set_label("Context menu"); group.set_children(children); nodes.push((menu_id, group));
        let mut root = Node::new(Role::Window); root.set_label("nus"); root.set_children(vec![menu_id]); nodes.push((root_id, root));
        Some(TreeUpdate { nodes, tree: Some(TreeInfo::new(root_id)), tree_id: TreeId::ROOT, focus })
    }

    pub(crate) fn page_menu_access_action(&mut self, req: accesskit::ActionRequest) {
        if !self.context_menu_owner_exists() { self.close_page_menu(); return; }
        let Some(menu) = self.page_menu.as_ref() else { return; };
        let index = req.target_node.0.checked_sub(menu.access_base + 1).and_then(|n| usize::try_from(n).ok());
        let Some(id) = index.and_then(|i| menu.items.get(i)).filter(|(_, text, on)| *on && !text.is_empty()).map(|(id, _, _)| *id) else { return; };
        match req.action {
            accesskit::Action::Click => self.page_menu_pick(id),
            accesskit::Action::Focus => if let Some(menu) = self.page_menu.as_mut() {
                menu.selected = Some(id); menu.keyboard = true; menu.reveal = true; self.dirty = true;
            },
            _ => {},
        }
    }
}
