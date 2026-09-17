//! Tiling: two to four tabs share the content, Vivaldi style. Select the
//! tabs (Ctrl+click their rows), then Ctrl+Shift+D; the tiling belongs to
//! those tabs and shows whenever one of them is active. Two sit side by
//! side; three make an L (one tall on the left, two stacked on the right);
//! four are a grid. The dividers drag, tiles swap with Ctrl+Alt+Shift+
//! arrows, and closing a tiled tab re-tiles the rest.

use crate::app::{App, Pane};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

/// The tiled tabs, by id (indexes shift; ids don't), and where the
/// dividers sit as fractions of the content.
#[derive(Clone, Debug, PartialEq)]
pub struct Tiling {
    pub ids: Vec<u64>,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divider {
    X,
    Y,
}

impl Pane {
    pub(crate) fn rect(&self) -> Rect {
        match self {
            Pane::Term(t) => t.rect,
            Pane::Web(w) => w.rect,
            Pane::Settings(s) => s.rect,
            Pane::Hints(h) => h.rect,
            Pane::Editor(e) => e.rect,
        }
    }
}

impl App {
    /// The tiled tabs as indexes, in tiling order, when the active tab is
    /// one of them; otherwise none (the tiling waits for its tabs).
    pub(crate) fn tiled(&self) -> Vec<usize> {
        let Some(t) = &self.tiling else { return Vec::new() };
        let idx: Vec<usize> = t.ids.iter().filter_map(|id| self.tabs.iter().position(|tab| tab.id == *id)).collect();
        if idx.len() < 2 || !idx.contains(&self.active) {
            return Vec::new();
        }
        idx
    }

    pub(crate) fn tiling_shown(&self) -> bool {
        !self.tiled().is_empty()
    }

    /// Is tab `i` in the tiling at all (shown or waiting)?
    pub(crate) fn is_tiled(&self, i: usize) -> bool {
        self.tiling.as_ref().is_some_and(|t| self.tabs.get(i).is_some_and(|tab| t.ids.contains(&tab.id)))
    }

    /// The tiles' rectangles inside `c`, for `n` tabs.
    pub(crate) fn tile_rects(&self, c: Rect, n: usize) -> Vec<Rect> {
        let rule = self.px(m::STRUCTURE);
        let (fx, fy) = self.tiling.as_ref().map(|t| (t.x, t.y)).unwrap_or((0.5, 0.5));
        let xw = ((c.w - rule) * fx).round();
        let yh = ((c.h - rule) * fy).round();
        let left = Rect::new(c.x, c.y, xw, c.h);
        let right = Rect::new(c.x + xw + rule, c.y, c.w - xw - rule, c.h);
        match n {
            2 => vec![left, right],
            3 => vec![
                left,
                Rect::new(right.x, right.y, right.w, yh),
                Rect::new(right.x, right.y + yh + rule, right.w, right.h - yh - rule),
            ],
            _ => vec![
                Rect::new(left.x, left.y, left.w, yh),
                Rect::new(right.x, right.y, right.w, yh),
                Rect::new(left.x, left.y + yh + rule, left.w, left.h - yh - rule),
                Rect::new(right.x, right.y + yh + rule, right.w, right.h - yh - rule),
            ],
        }
    }

    /// Tile the selection with the active tab, two to four of them.
    pub(crate) fn tile_selected(&mut self) {
        // A shown tiling grows by the selection; otherwise the selection
        // and the active tab make a new one.
        let mut idx: Vec<usize> = self.tiled();
        if !idx.contains(&self.active) {
            idx.insert(0, self.active);
        }
        let mut more: Vec<usize> = self.selected.iter().copied().filter(|&i| i < self.tabs.len() && !idx.contains(&i)).collect();
        more.sort_unstable();
        idx.extend(more);
        idx.truncate(4);
        if idx.len() < 2 {
            return;
        }
        let ids = idx.iter().map(|&i| self.tabs[i].id).collect();
        self.tiling = Some(Tiling { ids, x: 0.5, y: 0.5 });
        self.selected.clear();
        for &i in &idx {
            self.wake_tab(i);
        }
        self.play_event("tab.switch");
        self.layout();
        self.save_session();
    }

    pub(crate) fn untile(&mut self) {
        if self.tiling.take().is_some() {
            self.layout();
            self.save_session();
        }
    }

    /// Ctrl+Shift+D: tile a selection; untile a shown tiling; otherwise the
    /// tab's own split with a browser.
    pub(crate) fn divide(&mut self) {
        if self.selected.len() >= 1 && (self.selected.len() >= 2 || !self.selected.contains(&self.active)) {
            self.tile_selected();
        } else if self.tiling_shown() {
            self.untile();
        } else {
            self.toggle_split();
        }
    }

    /// Drop a closed tab from the tiling; one left ends it.
    pub(crate) fn tile_forget(&mut self, id: u64) {
        if let Some(t) = self.tiling.as_mut() {
            t.ids.retain(|&k| k != id);
            if t.ids.len() < 2 {
                self.tiling = None;
            }
        }
    }

    /// Move focus to the next (+1) or previous (-1) tile.
    pub(crate) fn tile_focus(&mut self, dir: i32) {
        let tiles = self.tiled();
        let Some(k) = tiles.iter().position(|&i| i == self.active) else { return };
        let n = tiles.len() as i32;
        let next = tiles[((k as i32 + dir).rem_euclid(n)) as usize];
        self.activate(next);
    }

    /// Swap the active tile with the next (+1) or previous (-1) one.
    pub(crate) fn tile_swap(&mut self, dir: i32) {
        let tiles = self.tiled();
        let Some(k) = tiles.iter().position(|&i| i == self.active) else { return };
        let n = tiles.len() as i32;
        let j = ((k as i32 + dir).rem_euclid(n)) as usize;
        if let Some(t) = self.tiling.as_mut() {
            t.ids.swap(k, j);
        }
        self.play_event("tab.switch");
        self.layout();
        self.save_session();
    }

    /// Which divider (if any) is under the point, with a 6px grab zone.
    pub(crate) fn divider_at(&self, x: f32, y: f32) -> Option<Divider> {
        let tiles = self.tiled();
        if tiles.is_empty() {
            return None;
        }
        let c = self.content_rect();
        let rects = self.tile_rects(c, tiles.len());
        let grab = self.px(6.0);
        let vx = rects[0].right();
        if (x - vx).abs() <= grab && c.contains(x, y) {
            // The three-tile L has no vertical rule below its left tile's
            // full height, but the rule runs the full height anyway.
            return Some(Divider::X);
        }
        if tiles.len() >= 3 {
            let hy = rects[1].bottom();
            let from = if tiles.len() == 3 { rects[1].x } else { c.x };
            if (y - hy).abs() <= grab && x >= from && x <= c.right() {
                return Some(Divider::Y);
            }
        }
        None
    }

    /// Follow a divider drag.
    pub(crate) fn tile_drag_to(&mut self, x: f32, y: f32) {
        let Some(d) = self.tile_drag else { return };
        let c = self.content_rect();
        if let Some(t) = self.tiling.as_mut() {
            match d {
                Divider::X => t.x = ((x - c.x) / c.w).clamp(0.2, 0.8),
                Divider::Y => t.y = ((y - c.y) / c.h).clamp(0.2, 0.8),
            }
        }
        self.layout();
        self.resize_due = Some(std::time::Instant::now() + std::time::Duration::from_millis(60));
    }

    /// Lay the tiled tabs out; false when there is no tiling to show.
    pub(crate) fn layout_tiles(&mut self) -> bool {
        let tiles = self.tiled();
        if tiles.is_empty() {
            return false;
        }
        let c = self.content_rect();
        let rects = self.tile_rects(c, tiles.len());
        let off = Rect::new(-4.0 * c.w - c.x, c.y, c.w, c.h);
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(16.0);
        let scale = self.scale;
        for (k, &i) in tiles.iter().enumerate() {
            let tab = &mut self.tabs[i];
            let right_shown = tab.focus_right && tab.right.is_some();
            crate::app::place_pane(&mut tab.left, if right_shown { off } else { rects[k] }, header, pad_x, pad_y, scale, true);
            if let Some(r) = tab.right.as_mut() {
                crate::app::place_pane(r, if right_shown { rects[k] } else { off }, header, pad_x, pad_y, scale, true);
            }
        }
        self.dirty = true;
        true
    }

    /// Draw the tiled tabs and the rules between them; false when there is
    /// no tiling to show.
    pub(crate) fn draw_tiling(&mut self, scene: &mut Scene) -> bool {
        let tiles = self.tiled();
        if tiles.is_empty() {
            return false;
        }
        let ink = self.theme.ink;
        let c = self.content_rect();
        let rects = self.tile_rects(c, tiles.len());
        let rule = self.px(m::STRUCTURE);
        let active = self.active;
        let mut tabs = std::mem::take(&mut self.tabs);
        for (k, &i) in tiles.iter().enumerate() {
            let n = format!("{:02}", i + 1);
            let look = tabs[i].look.clone();
            let tab = &mut tabs[i];
            let focused = i == active;
            let right_shown = tab.focus_right && tab.right.is_some();
            let pane = if right_shown { tab.right.as_mut().unwrap() } else { &mut tab.left };
            scene.layer(Some(rects[k]));
            self.draw_pane(scene, pane, &n, focused, &look, true);
            scene.layer(None);
            // The focused tile: a hairline of the tab's signal under its top.
            if focused && tiles.len() > 1 {
                let s = look.signal.unwrap_or(self.surface.signal);
                scene.rect(Rect::new(rects[k].x, rects[k].y, rects[k].w, self.px(2.0)), s);
            }
        }
        self.tabs = tabs;
        // Rules.
        scene.vline(rects[0].right(), c.y, c.h, rule, ink);
        if tiles.len() >= 3 {
            let from = if tiles.len() == 3 { rects[1].x } else { c.x };
            scene.hline(from, rects[1].bottom(), c.right() - from, rule, ink);
        }
        true
    }

    /// The tile under the point, as a tab index.
    pub(crate) fn tile_at(&self, x: f32, y: f32) -> Option<usize> {
        let tiles = self.tiled();
        if tiles.is_empty() {
            return None;
        }
        let rects = self.tile_rects(self.content_rect(), tiles.len());
        rects.iter().position(|r| r.contains(x, y)).map(|k| tiles[k])
    }
}
