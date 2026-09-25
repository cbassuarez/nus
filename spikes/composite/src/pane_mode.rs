//! Pane mode and drop zones: the keyboard and the pointer ways into the
//! pane director. Both work on what is on screen, the slots: every tile
//! of a shown tiling, or the one or two panes of the active tab.
//!
//! **Pane mode** (Ctrl+Alt+P, again or Esc to leave) takes single keys:
//!
//!   h j k l        focus the pane that way
//!   H J K L        swap with the pane that way
//!   arrows         move the nearest rule that way
//!   =              even out: every tile the same area
//!   z              zoom: this pane alone, again to come back
//!   s              split: a browser beside this pane
//!   t              this pane to a tab of its own
//!   w  n           this pane to another window, or a new one
//!   x              close this pane (what runs there stops)
//!   u  r           undo, redo
//!
//! **Drop zones.** Drag a pane by its move handle, or a tab by its sidebar
//! row, over the content: the slot under the pointer lights the edge or
//! the middle it would land on. An edge splits there (the dragged pane or
//! tab takes that side); the middle swaps (or, for a pane over a tab with
//! room, joins it). Every drop is one step of the director's history, so
//! one undo takes the whole drop back.

use crate::app::{App, Caps};
use crate::director::Op;
use crate::tiles::{Axis, Node, Tiling};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};
use winit::keyboard::KeyCode;

/// A pane on screen: its tab (index), which side of the tab, where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub tab: usize,
    pub right: bool,
    pub rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl Zone {
    /// The split an edge makes: its axis, and whether the newcomer goes first.
    fn split(self) -> Option<(Axis, bool)> {
        match self {
            Zone::Left => Some((Axis::Row, true)),
            Zone::Right => Some((Axis::Row, false)),
            Zone::Top => Some((Axis::Column, true)),
            Zone::Bottom => Some((Axis::Column, false)),
            Zone::Center => None,
        }
    }
}

/// What is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dragging {
    /// One pane of a split tab.
    Pane { tab: usize, right: bool },
    /// A whole tab, by its sidebar row.
    Tab(usize),
}

/// Where a drag would land, and what it would do there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    pub slot: Slot,
    pub zone: Zone,
    pub preview: Rect,
    pub words: &'static str,
}

/// The zone of `r` a point falls in: an edge within its outer quarter
/// (the nearest edge), else the middle.
pub fn zone_of(r: Rect, x: f32, y: f32) -> Zone {
    let (fx, fy) = ((x - r.x) / r.w.max(1.0), (y - r.y) / r.h.max(1.0));
    let edges = [(fx, Zone::Left), (1.0 - fx, Zone::Right), (fy, Zone::Top), (1.0 - fy, Zone::Bottom)];
    let (d, z) = edges.into_iter().fold((f32::MAX, Zone::Center), |best, e| if e.0 < best.0 { e } else { best });
    if d > 0.25 { Zone::Center } else { z }
}

/// The part of `r` a zone would take.
pub fn preview(r: Rect, zone: Zone) -> Rect {
    match zone {
        Zone::Left => Rect::new(r.x, r.y, r.w / 2.0, r.h),
        Zone::Right => Rect::new(r.x + r.w / 2.0, r.y, r.w / 2.0, r.h),
        Zone::Top => Rect::new(r.x, r.y, r.w, r.h / 2.0),
        Zone::Bottom => Rect::new(r.x, r.y + r.h / 2.0, r.w, r.h / 2.0),
        Zone::Center => Rect::new(r.x + r.w * 0.2, r.y + r.h * 0.2, r.w * 0.6, r.h * 0.6),
    }
}

/// The slot `dir` of `from`: its centre that way, the nearest by distance
/// along the way plus twice the distance across it.
pub fn neighbour(slots: &[Slot], from: Slot, dir: Dir) -> Option<Slot> {
    let c = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (fx, fy) = c(from.rect);
    slots
        .iter()
        .filter(|s| **s != from)
        .filter_map(|s| {
            let (x, y) = c(s.rect);
            let (along, across) = match dir {
                Dir::Left => (fx - x, (y - fy).abs()),
                Dir::Right => (x - fx, (y - fy).abs()),
                Dir::Up => (fy - y, (x - fx).abs()),
                Dir::Down => (y - fy, (x - fx).abs()),
            };
            (along > 1.0).then_some((along + 2.0 * across, *s))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| s)
}

fn pair(first: u64, second: u64, axis: Axis) -> Tiling {
    Tiling { root: Node::Split { axis, ratio: 0.5, a: Box::new(Node::Leaf(first)), b: Box::new(Node::Leaf(second)) } }
}

impl App {
    /// What is on screen: the shown tiles, or the active tab's panes.
    pub(crate) fn slots(&self) -> Vec<Slot> {
        let tiles = self.tile_rects();
        if !tiles.is_empty() {
            return tiles.into_iter().map(|(i, rect)| Slot { tab: i, right: self.tabs[i].focus_right && self.tabs[i].right.is_some(), rect }).collect();
        }
        let Some(t) = self.tabs.get(self.active) else { return Vec::new() };
        let mut out = Vec::new();
        if !(t.solo && t.focus_right && t.right.is_some()) {
            out.push(Slot { tab: self.active, right: false, rect: t.left.rect() });
        }
        if let Some(r) = t.right.as_ref().filter(|_| !(t.solo && !t.focus_right)) {
            out.push(Slot { tab: self.active, right: true, rect: r.rect() });
        }
        // A narrow window shows a split one pane at a time, the other parked
        // off screen; only what is on screen is a slot.
        let c = self.content_rect();
        out.retain(|s| s.rect.x < c.right() && s.rect.right() > c.x);
        out
    }

    fn focused_slot(&self) -> Option<Slot> {
        let t = self.tabs.get(self.active)?;
        let right = t.focus_right && t.right.is_some();
        self.slots().into_iter().find(|s| s.tab == self.active && s.right == right)
    }

    fn focus_slot(&mut self, s: Slot) {
        if s.tab != self.active {
            self.activate(s.tab);
        }
        if let Some(t) = self.tabs.get_mut(s.tab) {
            t.focus_right = s.right && t.right.is_some();
        }
        self.dirty = true;
    }

    // ── Pane mode ───────────────────────────────────────────────────────

    pub(crate) fn toggle_pane_mode(&mut self) {
        self.pane_mode = !self.pane_mode;
        self.dirty = true;
    }

    /// A key while pane mode is on. Every key is the mode's; the shell and
    /// the page hear nothing until it ends.
    pub(crate) fn pane_mode_key(&mut self, code: Option<KeyCode>, shift: bool) {
        let dir = match code {
            Some(KeyCode::KeyH) => Some(Dir::Left),
            Some(KeyCode::KeyJ) => Some(Dir::Down),
            Some(KeyCode::KeyK) => Some(Dir::Up),
            Some(KeyCode::KeyL) => Some(Dir::Right),
            _ => None,
        };
        if let Some(d) = dir {
            if shift { self.pane_swap_dir(d) } else { self.pane_focus_dir(d) }
            return;
        }
        match code {
            Some(KeyCode::ArrowLeft) => self.pane_resize(Dir::Left),
            Some(KeyCode::ArrowRight) => self.pane_resize(Dir::Right),
            Some(KeyCode::ArrowUp) => self.pane_resize(Dir::Up),
            Some(KeyCode::ArrowDown) => self.pane_resize(Dir::Down),
            Some(KeyCode::Equal) => self.pane_even(),
            Some(KeyCode::KeyZ) => self.pane_zoom(),
            Some(KeyCode::KeyS) => {
                if let Some(tab) = self.tabs.get(self.active).map(|t| t.id) {
                    self.direct(Op::Split { tab });
                }
            }
            Some(KeyCode::KeyT) => {
                if let Some(s) = self.focused_slot().filter(|s| self.tabs[s.tab].right.is_some()) {
                    let tab = self.tabs[s.tab].id;
                    self.direct(Op::ToTab { tab, right: s.right });
                }
            }
            Some(KeyCode::KeyX) => {
                if let Some(s) = self.focused_slot() {
                    let tab = self.tabs[s.tab].id;
                    self.direct(Op::Kill { tab, right: s.right });
                }
            }
            Some(KeyCode::KeyW) => self.send_pick(),
            Some(KeyCode::KeyN) => self.send_focused(crate::send::Dest::New),
            Some(KeyCode::KeyU) => self.pane_undo(),
            Some(KeyCode::KeyR) => self.pane_redo(),
            Some(KeyCode::Escape) | Some(KeyCode::Enter) | Some(KeyCode::KeyP) | Some(KeyCode::KeyQ) => self.pane_mode = false,
            _ => {}
        }
        self.dirty = true;
    }

    pub(crate) fn pane_focus_dir(&mut self, d: Dir) {
        let Some(from) = self.focused_slot() else { return };
        if let Some(s) = neighbour(&self.slots(), from, d) {
            self.focus_slot(s);
        }
    }

    pub(crate) fn pane_swap_dir(&mut self, d: Dir) {
        let Some(from) = self.focused_slot() else { return };
        let Some(to) = neighbour(&self.slots(), from, d) else { return };
        if to.tab == from.tab {
            let tab = self.tabs[from.tab].id;
            self.direct(Op::Swap { tab });
        } else if let Some(mut t) = self.tiling.clone() {
            t.root.swap(self.tabs[from.tab].id, self.tabs[to.tab].id);
            self.direct(Op::Tiling(Some(t)));
        }
    }

    /// Move the nearest rule of the focused pane that way, a twentieth of
    /// its split (a tiling) or 40px (a tab's own split).
    pub(crate) fn pane_resize(&mut self, d: Dir) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let id = tab.id;
        let sign = if matches!(d, Dir::Left | Dir::Up) { -1.0 } else { 1.0 };
        let axis = if matches!(d, Dir::Left | Dir::Right) { Axis::Row } else { Axis::Column };
        if self.tiling_shown() {
            let Some(mut t) = self.tiling.clone() else { return };
            let Some(path) = t.root.path_of(id) else { return };
            // The deepest split above this tile that runs the right way.
            let Some(k) = (0..path.len()).rev().find(|&k| matches!(t.root.at(&path[..k]), Some(Node::Split { axis: a, .. }) if *a == axis)) else { return };
            let Some(Node::Split { ratio, .. }) = t.root.at(&path[..k]) else { return };
            let r = (ratio + 0.05 * sign).clamp(0.1, 0.9);
            t.root.set_ratio(&path[..k], r);
            self.direct(Op::Tiling(Some(t)));
        } else if tab.right.is_some() && axis == Axis::Row {
            // The right pane's width: the rule moving left widens it.
            let c = self.content_rect();
            let now = self.split_width(self.active, c.w) / self.scale.max(0.1);
            let w = (now - 40.0 * sign).clamp(crate::panes::MIN_SIDE, (c.w / self.scale.max(0.1) - crate::panes::MIN_SIDE).max(crate::panes::MIN_SIDE));
            self.direct(Op::SplitWidth { tab: id, w: Some(w.round()) });
        }
    }

    pub(crate) fn pane_even(&mut self) {
        if self.tiling_shown() {
            if let Some(mut t) = self.tiling.clone() {
                t.root.even();
                self.direct(Op::Tiling(Some(t)));
            }
        } else if let Some(tab) = self.tabs.get(self.active).map(|t| t.id) {
            self.direct(Op::SplitWidth { tab, w: None });
        }
    }

    /// This pane alone, and back. A tiling steps aside for its tab and
    /// comes back as it was; a split tab solos the pane.
    pub(crate) fn pane_zoom(&mut self) {
        if self.tiling_shown() {
            self.pane_zoomed = self.tiling.clone();
            self.direct(Op::Tiling(None));
            return;
        }
        let id = self.tabs.get(self.active).map(|t| t.id);
        if let Some(z) = self.pane_zoomed.take().filter(|z| id.is_some_and(|id| z.ids().contains(&id))) {
            self.direct(Op::Tiling(Some(z)));
            return;
        }
        let Some(t) = self.tabs.get(self.active) else { return };
        if t.right.is_some() {
            let (tab, solo, right) = (t.id, !t.solo, t.focus_right);
            self.direct(Op::Solo { tab, solo, right });
        }
    }

    /// The mode's frame: the focused pane in the signal, and what the keys do.
    pub(crate) fn draw_pane_mode(&mut self, scene: &mut Scene) {
        if !self.pane_mode {
            return;
        }
        let sig = self.surface.signal;
        scene.layer(None);
        if let Some(s) = self.focused_slot() {
            scene.outline(s.rect, self.px(3.0), sig);
        }
        let c = self.content_rect();
        let strong = self.label_strong();
        let text = "PANE · HJKL FOCUS · ⇧HJKL SWAP · ARROWS RESIZE · = EVEN · Z ZOOM · S SPLIT · T TO A TAB · W TO A WINDOW · N NEW WINDOW · X CLOSE · U UNDO · R REDO · ESC DONE";
        let style = Style { color: self.on_fill(sig), ..strong };
        let text = self.fit(style, text, c.w - self.px(40.0));
        let w = self.fonts.measure(style, &text) + self.px(28.0);
        let h = self.px(m::LABEL_PX) + self.px(18.0);
        // Above the notices, which rise from the bottom edge.
        let band = Rect::new((c.x + (c.w - w) / 2.0).round(), (c.bottom() - h - self.px(84.0)).round(), w, h);
        scene.rect(band, sig);
        self.fonts.draw(scene, style, band.x + self.px(14.0), band.y + h / 2.0 + self.px(4.0), &text);
        self.dirty = true;
    }

    // ── Drop zones ──────────────────────────────────────────────────────

    /// Where a drag would land at (x, y), if anywhere useful.
    pub(crate) fn drop_at(&self, x: f32, y: f32, what: Dragging) -> Option<Drop> {
        if self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            return None;
        }
        let slot = self.slots().into_iter().find(|s| s.rect.contains(x, y))?;
        let zone = zone_of(slot.rect, x, y);
        let tiled = self.tiling_shown();
        let words = match what {
            Dragging::Pane { tab, right } => {
                if slot.tab == tab && slot.right == right {
                    return None;
                }
                if slot.tab == tab {
                    match zone {
                        Zone::Center => "swap",
                        // Beside the other pane: a swap when it isn't there already.
                        Zone::Left if right => "swap",
                        Zone::Right if !right => "swap",
                        Zone::Left | Zone::Right => return None,
                        Zone::Top => "own tab, above",
                        Zone::Bottom => "own tab, below",
                    }
                } else {
                    match zone {
                        Zone::Center if self.tabs[slot.tab].right.is_none() => "join",
                        Zone::Center => return None,
                        _ if !tiled => return None,
                        _ => "own tab, beside",
                    }
                }
            }
            Dragging::Tab(d) => {
                if slot.tab == d {
                    return None;
                }
                let d_tiled = self.tiling.as_ref().is_some_and(|t| t.ids().contains(&self.tabs[d].id));
                match zone {
                    Zone::Center if tiled && d_tiled => "swap",
                    Zone::Center => return None,
                    _ => "tile",
                }
            }
        };
        Some(Drop { slot, zone, preview: preview(slot.rect, zone), words })
    }

    /// Carry a drop out, as one step of the history.
    pub(crate) fn apply_drop(&mut self, what: Dragging, drop: Drop) {
        let target = self.tabs[drop.slot.tab].id;
        self.director.begin_group();
        match what {
            Dragging::Pane { tab, right } => {
                let from = self.tabs[tab].id;
                match (drop.zone.split(), drop.slot.tab == tab) {
                    (None, true) | (Some((Axis::Row, _)), true) => {
                        self.direct(Op::Swap { tab: from });
                    }
                    (None, false) => {
                        self.direct(Op::Join { from, right, to: target, side_right: true });
                    }
                    (Some((axis, first)), same) => {
                        // The pane becomes a tab, then takes that side of the slot.
                        if self.direct(Op::ToTab { tab: from, right }) {
                            let new = self.tabs[self.active].id;
                            let beside = if same { from } else { target };
                            let t = match self.tiling.clone().filter(|t| t.ids().contains(&beside)) {
                                Some(mut t) => {
                                    t.root.insert(beside, new, axis, first);
                                    t
                                }
                                None => if first { pair(new, beside, axis) } else { pair(beside, new, axis) },
                            };
                            self.direct(Op::Tiling(Some(t)));
                        }
                    }
                }
            }
            Dragging::Tab(d) => {
                let id = self.tabs[d].id;
                let shown = self.tiling.clone().filter(|t| t.ids().contains(&target));
                let next = match (drop.zone.split(), shown) {
                    (None, Some(mut t)) => {
                        t.root.swap(target, id);
                        Some(t)
                    }
                    (None, None) => None,
                    // Already tiled elsewhere in it: moved, not copied. When
                    // only the target would be left, the two make a new pair.
                    (Some((axis, first)), Some(t)) => match t.without(id) {
                        Some(mut t) => {
                            t.root.insert(target, id, axis, first);
                            Some(t)
                        }
                        None => Some(if first { pair(id, target, axis) } else { pair(target, id, axis) }),
                    },
                    (Some((axis, first)), None) => Some(if first { pair(id, target, axis) } else { pair(target, id, axis) }),
                };
                if let Some(t) = next {
                    self.direct(Op::Tiling(Some(t)));
                }
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.activate(i);
                }
            }
        }
        self.director.end_group();
        self.dirty = true;
    }

    /// The zone a drag would land in: a wash and a rule in the signal, and
    /// what letting go does.
    pub(crate) fn draw_drop(&mut self, scene: &mut Scene, what: Dragging) {
        let (x, y) = self.mouse;
        let Some(d) = self.drop_at(x, y, what) else { return };
        let sig = self.surface.signal;
        scene.layer(None);
        scene.rect(d.preview, crate::app::fade(sig, 0.16));
        scene.outline(d.preview, self.px(m::STRUCTURE), sig);
        let style = Style { color: self.on_fill(sig), ..self.label_strong() };
        let text = d.words.caps();
        let w = self.fonts.measure(style, &text) + self.px(20.0);
        let h = self.px(m::LABEL_PX) + self.px(14.0);
        let chip = Rect::new((d.preview.x + (d.preview.w - w) / 2.0).round(), (d.preview.y + (d.preview.h - h) / 2.0).round(), w, h);
        scene.rect(chip, sig);
        self.fonts.draw(scene, style, chip.x + self.px(10.0), chip.y + h / 2.0 + self.px(4.0), &text);
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(tab: usize, x: f32, y: f32, w: f32, h: f32) -> Slot {
        Slot { tab, right: false, rect: Rect::new(x, y, w, h) }
    }

    #[test]
    fn zones_are_the_outer_quarters_and_the_middle() {
        let r = Rect::new(0.0, 0.0, 400.0, 200.0);
        assert_eq!(zone_of(r, 20.0, 100.0), Zone::Left);
        assert_eq!(zone_of(r, 390.0, 100.0), Zone::Right);
        assert_eq!(zone_of(r, 200.0, 10.0), Zone::Top);
        assert_eq!(zone_of(r, 200.0, 190.0), Zone::Bottom);
        assert_eq!(zone_of(r, 200.0, 100.0), Zone::Center);
        assert_eq!(preview(r, Zone::Right), Rect::new(200.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn neighbours_follow_the_layout() {
        // The L: 0 tall on the left, 1 over 2 on the right.
        let slots = [s(0, 0.0, 0.0, 500.0, 600.0), s(1, 500.0, 0.0, 500.0, 300.0), s(2, 500.0, 300.0, 500.0, 300.0)];
        assert_eq!(neighbour(&slots, slots[0], Dir::Right).map(|s| s.tab), Some(1));
        assert_eq!(neighbour(&slots, slots[1], Dir::Down).map(|s| s.tab), Some(2));
        assert_eq!(neighbour(&slots, slots[2], Dir::Left).map(|s| s.tab), Some(0));
        assert_eq!(neighbour(&slots, slots[2], Dir::Up).map(|s| s.tab), Some(1));
        assert_eq!(neighbour(&slots, slots[0], Dir::Left), None);
    }

    #[test]
    fn edges_split_the_right_way() {
        assert_eq!(Zone::Left.split(), Some((Axis::Row, true)));
        assert_eq!(Zone::Bottom.split(), Some((Axis::Column, false)));
        assert_eq!(Zone::Center.split(), None);
    }
}
