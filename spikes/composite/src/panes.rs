//! Pane controls for a split tab. Nothing is drawn until the pointer
//! nears a pane's top-right corner; then five glyphs bloom out of it, one
//! after another, on a proximity field (closer is clearer) — MOVE (drag
//! onto a sidebar row to make it that tab's other pane, or onto NEW TAB),
//! SWAP, SOLO (this pane alone, the other parked), TO A TAB, CLOSE — and
//! fade as the pointer leaves. Never persistent. The rule between the
//! panes lights in the signal as the pointer nears it and drags to
//! resize. TABS · PANE CONTROLS: NEAR, or NEVER.

use crate::app::{App, Pane, SideHit};
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Controls {
    /// Bloom as the pointer nears the corner.
    #[default]
    Near,
    Never,
}

/// How far from the corner the field reaches, logical px.
pub const REACH: f32 = 150.0;

/// A smooth step, 1 at the corner and 0 at the reach.
fn field(d: f32, reach: f32) -> f32 {
    let t = (d / reach).clamp(0.0, 1.0);
    let t = 1.0 - t;
    t * t * (3.0 - 2.0 * t)
}

/// Distance from a point to a rect (0 inside).
fn dist(r: Rect, x: f32, y: f32) -> f32 {
    let dx = (r.x - x).max(0.0).max(x - r.right());
    let dy = (r.y - y).max(0.0).max(y - r.bottom());
    (dx * dx + dy * dy).sqrt()
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

    /// The corner's field: 0 far away, 1 at the cluster.
    fn corner_field(&self, r: Rect) -> f32 {
        let (mx, my) = self.mouse;
        if !self.window_focused || !r.contains(mx, my) && dist(r, mx, my) > self.px(REACH) {
            return 0.0;
        }
        let cell = self.px(22.0);
        let cy = r.y + self.px(36.0) + self.px(4.0);
        let bar = Rect::new(r.right() - self.px(6.0) - 5.0 * cell, cy, 5.0 * cell, cell);
        field(dist(bar, mx, my), self.px(REACH))
    }

    /// The glyphs at a pane's top-right, on the field; hits into
    /// `self.pane_hits` once they're legible.
    pub(crate) fn draw_pane_controls(&mut self, scene: &mut Scene, r: Rect, right: bool, split: bool) {
        if !split || self.behavior.pane_controls == Controls::Never || self.focus {
            return;
        }
        let k = if self.pane_drag.is_some() { 1.0 } else { self.corner_field(r) };
        if k <= 0.02 {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let isz = self.px(12.0);
        let cell = self.px(22.0);
        let n = 5.0;
        let cy = r.y + self.px(36.0) + self.px(4.0);
        let cx0 = r.right() - self.px(6.0) - n * cell;
        // A soft paper wash under the glyphs so they read over anything,
        // no box; it fades with the field.
        let wash = Rect::new(cx0 - self.px(4.0), cy - self.px(2.0), n * cell + self.px(8.0), cell + self.px(4.0));
        scene.push(nus_render::Instance::rounded(wash, self.px(6.0), crate::app::fade(self.paper(), 0.82 * k)));
        let solo = self.tabs.get(self.active).is_some_and(|t| t.solo);
        let items = [
            (icons::ARROWS_OUT, PaneHit::Move(right), crate::app::IconMotion::Still),
            (icons::SWAP, PaneHit::Swap, crate::app::IconMotion::Spin(180.0)),
            (icons::SOLO, PaneHit::Solo(right), crate::app::IconMotion::Pop),
            (icons::TO_TAB, PaneHit::ToTab(right), crate::app::IconMotion::Bob),
            (icons::CLOSE, PaneHit::Close(right), crate::app::IconMotion::Spin(90.0)),
        ];
        for (i, (icon, hit, motion)) in items.into_iter().enumerate() {
            // Each glyph blooms a beat after the one nearer the corner: the
            // field is a little further along for the ones at the far end.
            let stagger = 1.0 - (4 - i) as f32 * 0.12;
            let ki = ((k - (1.0 - stagger)) / stagger).clamp(0.0, 1.0);
            if ki <= 0.01 {
                continue;
            }
            let rise = (1.0 - ki) * self.px(6.0);
            let c = Rect::new(cx0 + i as f32 * cell, cy, cell, cell);
            let on = matches!(hit, PaneHit::Solo(_)) && solo;
            let color = crate::app::fade(if on { self.surface.signal } else { ink }, ki);
            self.icon_button(scene, icon, isz, c.x + (cell - isz) / 2.0, c.y + (cell - isz) / 2.0 + rise, color, c, crate::app::hover_key("pane", i + if right { 10 } else { 0 }), motion);
            if ki > 0.5 {
                self.pane_hits.push((c, hit));
            }
        }
    }

    /// The rule between the panes: hairline ink, and the signal swelling
    /// under it as the pointer nears (or drags it).
    pub(crate) fn draw_split_rule(&mut self, scene: &mut Scene, rr: Rect) {
        let t = self.theme.clone();
        let rule = self.px(m::STRUCTURE);
        let x = rr.x - rule;
        scene.vline(x, rr.y, rr.h, rule, t.ink);
        if !self.behavior.pane_divider || self.focus {
            return;
        }
        let (mx, my) = self.mouse;
        let near = if self.split_drag { 1.0 } else if my >= rr.y && my <= rr.bottom() { field((mx - x).abs(), self.px(28.0)) } else { 0.0 };
        if near > 0.02 {
            let w = rule + self.px(2.0) * near;
            scene.rect(Rect::new(x - (w - rule) / 2.0, rr.y, w, rr.h), crate::app::fade(self.surface.signal, near));
        }
    }

    /// Is the pointer anywhere a pane control might change? (Redraw then.)
    pub(crate) fn near_pane_controls(&self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Some(r) = tab.right.as_ref() else { return false };
        let reach = self.px(REACH) + self.px(40.0);
        let rr = r.rect();
        let lr = tab.left.rect();
        let corner = |p: Rect| Rect::new(p.right() - self.px(130.0), p.y, self.px(130.0), self.px(70.0));
        dist(corner(lr), x, y) < reach || dist(corner(rr), x, y) < reach || (x - rr.x).abs() < self.px(40.0)
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
