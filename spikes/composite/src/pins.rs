//! Persistent, on-demand tabs. A pin owns its starting destination; closing
//! the live page leaves the pin in place. Nothing is loaded merely by pinning.
use crate::app::{App, Pane, SideHit, Tab};
use nus_render::{text::icons, Rect, Scene, Style};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Welcome,
    Downloads,
    Ports,
    Settings,
    Prompt,
    Page { url: String, container: String },
    File { path: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub title: String,
    pub target: Target,
}
impl Pin {
    pub fn defaults() -> Vec<Self> {
        [
            ("welcome", "Welcome", Target::Welcome),
            ("downloads", "Downloads", Target::Downloads),
            ("ports", "Ports", Target::Ports),
        ]
        .into_iter()
        .map(|(id, title, target)| Self {
            id: id.into(),
            title: title.into(),
            target,
        })
        .collect()
    }
    fn icon(&self) -> (&'static str, &'static str) {
        match self.target {
            Target::Welcome => icons::HOME,
            Target::Downloads => icons::DOWNLOAD,
            Target::Ports => icons::PORTS,
            Target::Settings => icons::SETTINGS,
            Target::Prompt => icons::TERMINAL,
            Target::Page { .. } => icons::GLOBE,
            Target::File { .. } => icons::CODE,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Open(usize),
    Remove(usize),
    Up(usize),
    Down(usize),
    Edit,
    Add,
    Close(usize),
    ToggleDefault(usize),
    Github,
    LocalPorts,
}
impl Act {
    pub fn label(self, pins: &[Pin]) -> String {
        let name = |i: usize| pins.get(i).map(|p| p.title.as_str()).unwrap_or("tab");
        match self {
            Self::Github => "Show GitHub pull requests in the sidebar".into(),
            Self::LocalPorts => "Show listening ports in the sidebar".into(),
            Self::Open(i) => format!("Open pinned {}", name(i)),
            Self::Remove(i) => format!("Unpin {}", name(i)),
            Self::Up(i) => format!("Move {} up", name(i)),
            Self::Down(i) => format!("Move {} down", name(i)),
            Self::Close(i) => format!("Close {}; keep pin", name(i)),
            Self::Edit => "Edit pinned tabs".into(),
            Self::Add => "Pin current tab".into(),
            Self::ToggleDefault(i) => format!("Toggle default pin {}", i + 1),
        }
    }
}
pub struct Pins {
    pub items: Vec<Pin>,
    pub live: HashMap<String, u64>,
    pub editing: bool,
    pub scroll: f32,
    pub rect: Rect,
    pub drag: Option<(usize, f32, bool)>,
}
impl Default for Pins {
    fn default() -> Self {
        Self {
            items: if crate::private::enabled() {
                vec![]
            } else {
                Pin::defaults()
            },
            live: HashMap::new(),
            editing: false,
            scroll: 0.0,
            drag: None,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }
}
impl Pins {
    pub fn owns(&self, id: u64) -> bool {
        self.live.values().any(|&t| t == id)
    }
}
fn target(tab: &Tab) -> Option<Target> {
    Some(match &tab.left {
        Pane::Hints(_) => Target::Welcome,
        Pane::Downloads(_) => Target::Downloads,
        Pane::Ports(_) => Target::Ports,
        Pane::Settings(_) => Target::Settings,
        Pane::Home(_) => Target::Prompt,
        Pane::Web(w) => Target::Page {
            url: w
                .asleep
                .clone()
                .unwrap_or_else(|| w.tab.shared.borrow().url.clone()),
            container: w.container.clone(),
        },
        // Editor files and shells retain their existing session pin behavior.
        _ => return None,
    })
}
impl App {
    pub(crate) fn apply_pins(&mut self, items: Vec<Pin>) {
        let removed: Vec<_> = self
            .pins
            .live
            .iter()
            .filter(|(id, _)| !items.iter().any(|p| &p.id == *id))
            .map(|(_, id)| *id)
            .collect();
        for tab in &mut self.tabs {
            if removed.contains(&tab.id) {
                tab.pinned = false;
            }
        }
        self.pins
            .live
            .retain(|id, _| items.iter().any(|p| &p.id == id));
        self.pins.items = items;
    }
    pub(crate) fn pin_tab(&mut self, i: usize) {
        if crate::private::enabled() {
            return;
        }
        let Some(tab) = self.tabs.get(i) else {
            return;
        };
        let id = tab.id;
        if let Some(k) = self
            .pins
            .items
            .iter()
            .position(|p| self.pins.live.get(&p.id) == Some(&id))
        {
            self.pin_action(Act::Remove(k));
            return;
        }
        let Some(target) = target(tab) else {
            self.tabs[i].pinned = !self.tabs[i].pinned;
            self.layout();
            self.save_session();
            return;
        };
        let title = tab.title();
        let k = if let Some(k) = self.pins.items.iter().position(|p| p.target == target) {
            k
        } else {
            self.pins.items.push(Pin {
                id: format!("pin-{}", crate::remote::new_token()),
                title,
                target,
            });
            self.pins.items.len() - 1
        };
        if let Some(old) = self.pins.live.insert(self.pins.items[k].id.clone(), id) {
            if old != id {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == old) {
                    tab.pinned = false;
                }
            }
        }
        self.tabs[i].pinned = true;
        self.save_prefs();
        self.save_session();
        self.layout();
        self.dirty = true;
    }
    /// Preserve old session pins without silently replacing the user's set.
    pub(crate) fn adopt_pins(&mut self) {
        if crate::private::enabled() {
            return;
        }
        // Bind built-in pins to a page already visible at launch without
        // opening any other page or waking a saved website.
        for pin in &self.pins.items {
            if self
                .pins
                .live
                .get(&pin.id)
                .is_some_and(|id| self.tabs.iter().any(|t| &t.id == id))
            {
                continue;
            }
            if let Some(tab) = self
                .tabs
                .iter_mut()
                .find(|t| !self.pins.owns(t.id) && target(t).as_ref() == Some(&pin.target))
            {
                tab.pinned = true;
                self.pins.live.insert(pin.id.clone(), tab.id);
            }
        }
        let pending: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.pinned && !self.pins.owns(t.id) && target(t).is_some())
            .map(|(i, _)| i)
            .collect();
        for i in pending {
            self.pin_tab(i);
        }
        let mut renamed = false;
        for pin in &mut self.pins.items {
            if let Some(name) = self
                .pins
                .live
                .get(&pin.id)
                .and_then(|id| self.tabs.iter().find(|t| &t.id == id))
                .and_then(|t| t.name.as_ref())
            {
                if &pin.title != name {
                    pin.title = name.clone();
                    renamed = true;
                }
            }
        }
        if renamed {
            self.save_prefs();
        }
    }
    pub(crate) fn pin_action(&mut self, act: Act) {
        if crate::private::enabled() {
            return;
        }
        match act {
            Act::Github => {
                self.sidebar_rules.live_github = !self.sidebar_rules.live_github;
                self.sync_live_folders();
            }
            Act::LocalPorts => {
                self.sidebar_rules.live_ports = !self.sidebar_rules.live_ports;
                self.sync_live_folders();
            }
            Act::Edit => {
                self.pins.editing = !self.pins.editing;
                if self.pins.editing && self.sidebar_icons() {
                    self.sidebar_rules.compact = false;
                    self.sidebar_rules.width = 248.0;
                }
            }
            Act::Add => {
                self.pin_tab(self.active);
                return;
            }
            Act::ToggleDefault(k) => {
                let Some(pin) = Pin::defaults().get(k).cloned() else {
                    return;
                };
                if let Some(i) = self.pins.items.iter().position(|p| p.target == pin.target) {
                    self.pin_action(Act::Remove(i));
                    return;
                }
                self.pins.items.push(pin);
            }
            Act::Remove(k) => {
                if k >= self.pins.items.len() {
                    return;
                }
                let pin = self.pins.items.remove(k);
                if let Some(id) = self.pins.live.remove(&pin.id) {
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                        t.pinned = false;
                    }
                }
            }
            Act::Up(k) => {
                if k > 0 && k < self.pins.items.len() {
                    self.pins.items.swap(k, k - 1);
                }
            }
            Act::Down(k) => {
                if k + 1 < self.pins.items.len() {
                    self.pins.items.swap(k, k + 1);
                }
            }
            Act::Close(k) => {
                let live = self
                    .pins
                    .items
                    .get(k)
                    .and_then(|p| self.pins.live.get(&p.id))
                    .and_then(|id| self.tabs.iter().position(|t| &t.id == id));
                if let Some(i) = live {
                    self.selected.clear();
                    self.activate(i);
                    self.close_tabs(false);
                }
            }
            Act::Open(k) => {
                let Some(pin) = self.pins.items.get(k).cloned() else {
                    return;
                };
                if let Some(i) = self
                    .pins
                    .live
                    .get(&pin.id)
                    .and_then(|id| self.tabs.iter().position(|t| &t.id == id))
                {
                    self.activate(i);
                    return;
                }
                // Reuse an already open matching page, including first-launch Welcome.
                if let Some(i) = self
                    .tabs
                    .iter()
                    .position(|t| target(t).as_ref() == Some(&pin.target))
                {
                    self.activate(i);
                } else {
                    match &pin.target {
                        Target::Welcome => self.open_welcome(),
                        Target::Downloads => self.open_downloads(),
                        Target::Ports => {
                            let tab = self.make_tab(
                                Pane::Ports(crate::ports::PortsPane {
                                    rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                                }),
                                None,
                            );
                            self.tabs.push(tab);
                            self.activate(self.tabs.len() - 1);
                        }
                        Target::Settings => self.open_settings(),
                        Target::Prompt => self.open_home(),
                        Target::Page { url, container } => {
                            let container = if self.containers.iter().any(|c| &c.name == container)
                            {
                                container.clone()
                            } else {
                                self.container.clone()
                            };
                            let Some(web) = self.new_web_pane_in(url, &container) else {
                                return;
                            };
                            let tab = self.make_tab(Pane::Web(web), None);
                            self.tabs.push(tab);
                            self.activate(self.tabs.len() - 1);
                        }
                        Target::File { path } => self.open_file(std::path::Path::new(path), false),
                    }
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.pinned = true;
                    self.pins.live.insert(pin.id, tab.id);
                }
            }
        }
        self.save_prefs();
        self.layout();
        self.dirty = true;
    }
    fn pin_rows(&self) -> usize {
        self.pins.items.len() + if self.pins.editing { 2 } else { 0 }
    }
    pub(crate) fn pins_height(&self) -> f32 {
        if crate::private::enabled() {
            return 0.0;
        }
        let available =
            (self.list_rect().h - self.side_header_h() - self.sidebar_footer_h()).max(0.0);
        (self.px(32.0 + self.pin_rows() as f32 * 36.0) + self.px(8.0)).min(available * 0.48)
    }
    pub(crate) fn pin_drag_move(&mut self, y: f32) {
        if let Some((_, start, moved)) = &mut self.pins.drag {
            if (y - *start).abs() > self.scale * 5.0 {
                *moved = true;
            }
            if *moved {
                self.dirty = true;
            }
        }
    }
    pub(crate) fn pin_drag_release(&mut self, y: f32) -> bool {
        let Some((from, _, moved)) = self.pins.drag.take() else {
            return false;
        };
        if moved && from < self.pins.items.len() {
            let to = ((y - self.pins.rect.y + self.pins.scroll) / self.px(36.0))
                .floor()
                .max(0.0) as usize;
            let to = to.min(self.pins.items.len() - 1);
            let pin = self.pins.items.remove(from);
            self.pins.items.insert(to, pin);
            self.save_prefs();
            self.dirty = true;
        } else {
            self.pin_action(Act::Open(from));
        }
        true
    }
    pub(crate) fn pins_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !self.pins.rect.contains(x, y) {
            return false;
        }
        let max = (self.px(self.pin_rows() as f32 * 36.0) - self.pins.rect.h).max(0.0);
        self.pins.scroll = (self.pins.scroll - dy).clamp(0.0, max);
        self.dirty = true;
        true
    }
    pub(crate) fn draw_pins(&mut self, scene: &mut Scene, sb: Rect) {
        if crate::private::enabled() {
            return;
        }
        let h = self.pins_height();
        let top = sb.y + self.side_header_h();
        if h <= 0.0 {
            return;
        }
        let outer = scene.clip();
        let area = Rect::new(sb.x, top, sb.w, h);
        scene.layer(Some(area));
        let ink = self.theme.ink;
        let compact = self.sidebar_icons();
        let label = self.label();
        let header = Rect::new(sb.x, top, sb.w, self.px(32.0));
        if !compact {
            self.fonts.draw(
                scene,
                Style {
                    color: self.theme.dim,
                    ..label
                },
                sb.x + self.px(12.0),
                top + self.px(21.0),
                "PINNED",
            );
        }
        let size = self.px(16.0);
        let bw = self.px(28.0);
        for (j, act, icon, tip) in [
            (
                0,
                Act::Edit,
                icons::PENCIL,
                if self.pins.editing {
                    "Finish editing pins"
                } else {
                    "Edit pinned tabs"
                },
            ),
            (1, Act::Add, icons::PLUS, "Pin the current tab"),
        ] {
            if compact && j == 1 {
                continue;
            }
            let x = if compact {
                sb.x + (sb.w - bw) * 0.5
            } else {
                header.right() - bw * (j + 1) as f32 - self.px(6.0)
            };
            let hit = Rect::new(x, top, bw, header.h);
            self.fonts.draw_icon(
                scene,
                icon,
                size,
                x + (bw - size) * 0.5,
                top + self.px(8.0),
                if self.pins.editing && j == 0 {
                    self.surface.signal
                } else {
                    ink
                },
            );
            self.side_hits.push((hit, SideHit::Pinned(act)));
            self.tip_words(hit, tip);
        }
        let body = Rect::new(
            sb.x,
            header.bottom(),
            sb.w,
            (h - header.h - self.px(4.0)).max(0.0),
        );
        self.pins.rect = body;
        self.pins.scroll = self
            .pins
            .scroll
            .min((self.px(self.pin_rows() as f32 * 36.0) - body.h).max(0.0));
        scene.layer(Some(body));
        for (k, pin) in self.pins.items.clone().iter().enumerate() {
            let rr = Rect::new(
                sb.x + self.px(6.0),
                body.y + k as f32 * self.px(36.0) - self.pins.scroll,
                sb.w - self.px(12.0),
                self.px(32.0),
            );
            let hit = rr.intersect(&body);
            if hit.h <= 0.0 {
                continue;
            }
            let live = self
                .pins
                .live
                .get(&pin.id)
                .and_then(|id| self.tabs.iter().position(|t| &t.id == id));
            let active = live == Some(self.active);
            let hot = hit.contains(self.mouse.0, self.mouse.1);
            if active || hot {
                scene.push(nus_render::Instance::rounded(
                    rr,
                    self.px(5.0),
                    crate::app::fade(self.theme.tint, if active { 1.0 } else { 0.5 }),
                ));
            }
            let color = if active { self.surface.signal } else { ink };
            let ix = if compact {
                rr.x + (rr.w - size) * 0.5
            } else {
                rr.x + self.px(9.0)
            };
            let iy = rr.y + self.px(8.0);
            let favicon = live.and_then(|i| match &self.tabs[i].left {
                Pane::Web(w) => w.favicon.as_ref().map(|(_, tex)| tex.clone()),
                _ => None,
            });
            if let Some(tex) = favicon {
                scene.texture(Rect::new(ix, iy, size, size), tex, None);
            } else {
                self.fonts.draw_icon(scene, pin.icon(), size, ix, iy, color);
            }
            self.side_hits.push((hit, SideHit::Pinned(Act::Open(k))));
            if compact {
                self.tip_words(hit, &pin.title);
            } else {
                let controls = if self.pins.editing {
                    self.px(76.0)
                } else if hot && live.is_some() {
                    self.px(26.0)
                } else {
                    self.px(6.0)
                };
                let style = self.ui();
                let text = self.fit(style, &pin.title, rr.w - self.px(34.0) - controls);
                self.fonts.draw(
                    scene,
                    Style {
                        color: ink,
                        ..style
                    },
                    rr.x + self.px(34.0),
                    rr.y + self.px(21.0),
                    &text,
                );
            }
            let actions = if self.pins.editing && !compact {
                vec![
                    (Act::Remove(k), "×", "Unpin; keep the open tab"),
                    (Act::Down(k), "↓", "Move pin down"),
                    (Act::Up(k), "↑", "Move pin up"),
                ]
            } else if hot && live.is_some() && !compact {
                vec![(Act::Close(k), "×", "Close page; keep its pin")]
            } else {
                vec![]
            };
            for (j, (act, word, tip)) in actions.into_iter().enumerate() {
                let button = Rect::new(
                    rr.right() - self.px(25.0) * (j + 1) as f32,
                    rr.y,
                    self.px(24.0),
                    rr.h,
                )
                .intersect(&body);
                self.fonts.draw(
                    scene,
                    label,
                    button.x + self.px(7.0),
                    rr.y + self.px(21.0),
                    word,
                );
                self.side_hits.push((button, SideHit::Pinned(act)));
                self.tip_words(button, tip);
            }
        }
        if self.pins.editing && !compact {
            for (j, on, act, title) in [
                (
                    0,
                    self.sidebar_rules.live_github,
                    Act::Github,
                    "GitHub pull requests",
                ),
                (
                    1,
                    self.sidebar_rules.live_ports,
                    Act::LocalPorts,
                    "Live local ports",
                ),
            ] {
                let row = Rect::new(
                    body.x + self.px(10.0),
                    body.y + (self.pins.items.len() + j) as f32 * self.px(36.0) - self.pins.scroll,
                    body.w - self.px(20.0),
                    self.px(32.0),
                );
                let hit = row.intersect(&body);
                if hit.h <= 0.0 {
                    continue;
                }
                let box_ = Rect::new(
                    row.x + self.px(5.0),
                    row.y + self.px(8.0),
                    self.px(15.0),
                    self.px(15.0),
                );
                scene.outline(box_, self.px(1.0), ink);
                if on {
                    self.fonts.draw_icon(
                        scene,
                        icons::CHECK,
                        self.px(14.0),
                        box_.x,
                        box_.y,
                        self.surface.signal,
                    );
                }
                let style = self.ui();
                self.fonts.draw(
                    scene,
                    style,
                    row.x + self.px(29.0),
                    row.y + self.px(21.0),
                    &self.fit(style, title, row.w - self.px(34.0)),
                );
                self.side_hits.push((hit, SideHit::Pinned(act)));
                self.tip_words(
                    hit,
                    if j == 0 {
                        "Optional · uses your signed-in GitHub CLI account"
                    } else {
                        "Optional · lists listeners on this machine"
                    },
                );
            }
        }
        if self.pins.drag.is_some_and(|(_, _, moved)| moved) {
            let row = ((self.mouse.1 - body.y + self.pins.scroll) / self.px(36.0))
                .floor()
                .max(0.0);
            let y = body.y + row * self.px(36.0) - self.pins.scroll;
            scene.hline(
                body.x + self.px(8.0),
                y,
                body.w - self.px(16.0),
                self.px(2.0),
                self.surface.signal,
            );
        }
        scene.layer(Some(area));
        scene.hline(
            sb.x + self.px(10.0),
            area.bottom() - self.px(1.0),
            sb.w - self.px(20.0),
            self.px(1.0),
            self.theme.tint,
        );
        scene.layer(outer);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_and_ordered_choices_round_trip() {
        let empty: Vec<Pin> = serde_json::from_str("[]").unwrap();
        assert!(empty.is_empty());
        let mut pins = Pin::defaults();
        pins.swap(0, 2);
        pins.remove(1);
        assert_eq!(
            serde_json::from_str::<Vec<Pin>>(&serde_json::to_string(&pins).unwrap()).unwrap(),
            pins
        );
        assert_eq!(pins[0].target, Target::Ports);
    }
}
