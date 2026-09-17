//! Pane controls for a split tab: a cluster at each pane's top-right —
//! SWAP, SOLO (this pane alone, the other parked), TO A TAB (out into
//! its own), CLOSE — shown on hover (or always, or never, under TABS ·
//! PANES); the rule between the panes drags to resize; the move handle
//! drags a pane onto a sidebar row to make it that tab's other pane, or
//! onto NEW TAB for a tab of its own.

use crate::app::{App, Pane, SideHit};
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Controls {
    #[default]
    Hover,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaneHit {
    Swap,
    Solo(bool),
    ToTab(bool),
    Close(bool),
    /// The move handle: press and drag onto a sidebar row.
    Move(bool),
}

/// Minimum width of either side of a split, logical px.
pub const MIN_SIDE: f32 = 240.0;

impl App {
    /// The width of the right pane for tab `i`, from its own setting or
    /// the metric, kept inside the content.
    pub(crate) fn split_width(&self, i: usize, content_w: f32) -> f32 {
        let want = self.tabs.get(i).and_then(|t| t.split_w).map(|w| self.px(w)).unwrap_or(self.px(m::SPLIT));
        let min = self.px(MIN_SIDE);
        want.clamp(min, (content_w - min - self.px(m::STRUCTURE)).max(min))
    }

    /// The rule between a split tab's panes, as a grab zone.
    pub(crate) fn split_divider_at(&self, x: f32, y: f32) -> bool {
        if !self.behavior.pane_divider || self.tiling_shown() || self.peeking().is_some() {
            return false;
        }
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Some(r) = tab.right.as_ref() else { return false };
        if tab.solo || self.width_class() == crate::app::Width::Narrow {
            return false;
        }
        let rr = r.rect();
        let rule_x = rr.x - self.px(m::STRUCTURE);
        (x - rule_x).abs() <= self.px(6.0) && y >= rr.y && y <= rr.bottom()
    }

    /// Follow a divider drag: the right pane's width, remembered per tab.
    pub(crate) fn split_drag_to(&mut self, x: f32) {
        let c = self.content_rect();
        let scale = self.scale.max(0.1);
        let i = self.active;
        let w = ((c.right() - x) / scale).clamp(MIN_SIDE, (c.w / scale - MIN_SIDE).max(MIN_SIDE));
        if let Some(t) = self.tabs.get_mut(i) {
            t.split_w = Some(w);
        }
        self.layout();
        self.resize_due = Some(std::time::Instant::now() + std::time::Duration::from_millis(60));
    }

    pub(crate) fn swap_panes(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(r) = tab.right.take() else { return };
        let l = std::mem::replace(&mut tab.left, r);
        tab.right = Some(l);
        tab.focus_right = !tab.focus_right;
        self.play_event("toggle");
        self.layout();
        self.save_session();
    }

    /// One pane alone in the tab; the other waits off screen. Again undoes.
    pub(crate) fn solo_pane(&mut self, right: bool) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if tab.right.is_none() {
            return;
        }
        if tab.solo && tab.focus_right == right {
            tab.solo = false;
        } else {
            tab.solo = true;
            tab.focus_right = right;
        }
        self.play_event("toggle");
        self.layout();
    }

    /// Close one pane of a split: the other keeps the tab.
    pub(crate) fn close_pane(&mut self, right: bool) {
        let i = self.active;
        let Some(tab) = self.tabs.get_mut(i) else { return };
        if tab.right.is_none() {
            return self.close_tabs(false);
        }
        if right {
            tab.right = None;
        } else if let Some(r) = tab.right.take() {
            tab.left = r;
        }
        tab.focus_right = false;
        tab.solo = false;
        self.play_event("tab.close");
        self.layout();
        self.save_session();
    }

    /// A pane of the active tab leaves into a tab of its own, right after.
    pub(crate) fn detach_pane(&mut self, right: bool) {
        let i = self.active;
        let Some(pane) = self.take_pane(i, right) else { return };
        let mut tab = self.make_tab(pane, None);
        tab.parent = self.tabs[i].parent;
        let at = self.subtree(i).last().copied().unwrap_or(i) + 1;
        self.insert_tab_at(at, tab);
        self.activate(at);
        self.save_session();
    }

    /// Take a pane out of tab `i`; the other pane keeps the tab. None
    /// when the tab isn't split (a lone pane stays where it is).
    fn take_pane(&mut self, i: usize, right: bool) -> Option<Pane> {
        let tab = self.tabs.get_mut(i)?;
        let r = tab.right.take()?;
        tab.focus_right = false;
        tab.solo = false;
        if right {
            Some(r)
        } else {
            Some(std::mem::replace(&mut tab.left, r))
        }
    }

    /// Insert a tab at `at`, keeping the indexes that point past it right.
    pub(crate) fn insert_tab_at(&mut self, at: usize, tab: crate::app::Tab) {
        self.tabs.insert(at, tab);
        for t in self.mru.iter_mut() {
            if *t >= at {
                *t += 1;
            }
        }
        self.selected = self.selected.iter().map(|&t| if t >= at { t + 1 } else { t }).collect();
        if let Some(p) = self.pip.as_mut() {
            if p.tab >= at {
                p.tab += 1;
            }
        }
        if self.active >= at {
            self.active += 1;
        }
    }

    /// Drop a dragged pane onto tab `target`: it becomes that tab's other
    /// pane when there's room. Same tab, or a full one: nothing moves.
    pub(crate) fn move_pane_to(&mut self, from: usize, right: bool, target: usize) {
        if from == target || target >= self.tabs.len() || self.tabs[target].right.is_some() {
            return;
        }
        let Some(pane) = self.take_pane(from, right) else { return };
        let t = &mut self.tabs[target];
        t.right = Some(pane);
        t.focus_right = true;
        self.play_event("tab.switch");
        self.activate(target);
        self.layout();
        self.save_session();
    }

    /// The cluster over a pane's top-right; hits into `self.pane_hits`.
    pub(crate) fn draw_pane_controls(&mut self, scene: &mut Scene, r: Rect, right: bool, split: bool) {
        if !split || self.behavior.pane_controls == Controls::Never || self.focus {
            return;
        }
        let (mx, my) = self.mouse;
        let hovered = r.contains(mx, my) && self.window_focused;
        if self.behavior.pane_controls == Controls::Hover && !hovered && self.pane_drag.is_none() {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let isz = self.px(12.0);
        let cell = self.px(22.0);
        let n = 5.0;
        // Under the pane's own header row, at the corner, over the content.
        let cy = r.y + self.px(36.0) + self.px(4.0);
        let cx0 = r.right() - self.px(6.0) - n * cell;
        let bar = Rect::new(cx0, cy, n * cell, cell);
        scene.rect(bar, crate::app::fade(self.paper(), 0.92));
        scene.outline(bar, self.px(m::HAIRLINE), crate::app::fade(t.dim, 0.7));
        let solo = self.tabs.get(self.active).is_some_and(|t| t.solo);
        let items = [
            (icons::ARROWS_OUT, PaneHit::Move(right), crate::app::IconMotion::Still),
            (icons::SWAP, PaneHit::Swap, crate::app::IconMotion::Spin(180.0)),
            (icons::SOLO, PaneHit::Solo(right), crate::app::IconMotion::Pop),
            (icons::TO_TAB, PaneHit::ToTab(right), crate::app::IconMotion::Bob),
            (icons::CLOSE, PaneHit::Close(right), crate::app::IconMotion::Spin(90.0)),
        ];
        for (k, (icon, hit, motion)) in items.into_iter().enumerate() {
            let c = Rect::new(cx0 + k as f32 * cell, cy, cell, cell);
            let on = matches!(hit, PaneHit::Solo(_)) && solo;
            let color = if on { self.surface.signal } else { ink };
            self.icon_button(scene, icon, isz, c.x + (cell - isz) / 2.0, c.y + (cell - isz) / 2.0, color, c, crate::app::hover_key("pane", k + if right { 10 } else { 0 }), motion);
            self.pane_hits.push((c, hit));
        }
        self.dirty |= hovered;
    }

    /// A click on the cluster; the move handle arms a drag. Returns true
    /// when taken.
    pub(crate) fn pane_click(&mut self, x: f32, y: f32) -> bool {
        let Some(&(_, hit)) = self.pane_hits.iter().find(|(r, _)| r.contains(x, y)) else { return false };
        match hit {
            PaneHit::Swap => self.swap_panes(),
            PaneHit::Solo(r) => self.solo_pane(r),
            PaneHit::ToTab(r) => self.detach_pane(r),
            PaneHit::Close(r) => self.close_pane(r),
            PaneHit::Move(r) => {
                self.pane_drag = Some((self.active, r, x, y));
                self.sidebar_hover = true;
            }
        }
        self.dirty = true;
        true
    }

    /// Release with a pane in hand: onto a row, the NEW TAB row, or nowhere.
    pub(crate) fn pane_drop(&mut self, x: f32, y: f32) {
        let Some((from, right, x0, y0)) = self.pane_drag.take() else { return };
        if (x - x0).abs() + (y - y0).abs() < self.px(6.0) {
            return; // a click on the handle, not a drag
        }
        if self.sidebar_visible() && self.sidebar_rect().contains(x, y) {
            let g = self.sidebar_geometry();
            if let Some(&(i, _, _)) = g.rows.iter().find(|&&(_, ry, rh)| y >= ry && y < ry + rh) {
                self.move_pane_to(from, right, i);
            } else if self.side_hits.iter().any(|(r, h)| *h == SideHit::NewShell && r.contains(x, y)) {
                self.activate(from);
                self.detach_pane(right);
            }
        }
        self.dirty = true;
    }

    /// The ghost while dragging a pane, and the row it would land on.
    pub(crate) fn draw_pane_drag(&mut self, scene: &mut Scene) {
        let Some((from, right, x0, y0)) = self.pane_drag else { return };
        let (mx, my) = self.mouse;
        if (mx - x0).abs() + (my - y0).abs() < self.px(6.0) {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let title = self.tabs.get(from).map(|tab| {
            let p = if right { tab.right.as_ref().unwrap_or(&tab.left) } else { &tab.left };
            match p {
                Pane::Term(t) => t.title.clone(),
                Pane::Web(w) => w.tab.shared.borrow().title.clone(),
                Pane::Settings(_) => "settings".into(),
                Pane::Hints(_) => "welcome".into(),
            }
        }).unwrap_or_default();
        let strong = self.label_strong();
        let text = self.fit(strong, &title.to_uppercase(), self.px(200.0));
        let w = self.fonts.measure(strong, &text) + self.px(24.0);
        let ghost = Rect::new(mx + self.px(12.0), my - self.px(12.0), w, self.px(26.0));
        scene.layer(None);
        scene.rect(Rect::new(ghost.x + self.px(3.0), ghost.y + self.px(3.0), ghost.w, ghost.h), crate::app::fade(ink, 0.5));
        scene.rect(ghost, self.paper());
        scene.outline(ghost, self.px(m::STRUCTURE), ink);
        self.fonts.draw(scene, strong, ghost.x + self.px(12.0), ghost.y + self.px(17.0), &text);
        // The row under the pointer takes a signal outline when it has room.
        if self.sidebar_visible() && self.sidebar_rect().contains(mx, my) {
            let g = self.sidebar_geometry();
            let sb = self.sidebar_rect();
            if let Some(&(i, ry, rh)) = g.rows.iter().find(|&&(_, ry, rh)| my >= ry && my < ry + rh) {
                let room = i != from && self.tabs[i].right.is_none();
                scene.outline(Rect::new(sb.x + self.px(4.0), ry + self.px(2.0), sb.w - self.px(8.0), rh - self.px(4.0)), self.px(m::STRUCTURE), if room { self.surface.signal } else { t.dim });
            }
        }
        self.dirty = true;
    }
}
