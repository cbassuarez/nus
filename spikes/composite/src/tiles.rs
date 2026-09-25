//! Tiling: tabs share the content, Vivaldi style, as a tree of splits.
//! Select the tabs (Ctrl+click their rows), then Ctrl+Shift+D; the tiling
//! belongs to those tabs and shows whenever one of them is active.
//!
//! A tiling is a tree: a leaf is a tab, a split lays two subtrees side by
//! side (a row) or one over the other (a column) at a ratio of its own.
//! New tilings start from a template: two side by side, three an L (one
//! tall on the left, two stacked on the right), four a grid, more halved
//! and halved again. A tab added to a tiling that is showing splits the
//! largest tile along its longer side, so what you arranged stays put.
//! Every rule drags on its own; tiles swap with Ctrl+Alt+Shift+arrows;
//! closing a tiled tab gives its room to its neighbour.

use crate::app::{App, Pane};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

/// Most tabs one tiling holds. Past this, tiles are too small to use.
pub const MAX_TILES: usize = 8;

/// Side by side (a vertical rule between) or one over the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Axis {
    Row,
    Column,
}

/// A tiling's tree, over tab ids at run time and tab indexes on disk.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Node<T> {
    Leaf(T),
    Split { axis: Axis, ratio: f32, a: Box<Node<T>>, b: Box<Node<T>> },
}

/// Which child of each split, from the root: false is `a`, true is `b`.
pub type Path = Vec<bool>;

impl<T: Copy + PartialEq> Node<T> {
    fn split(axis: Axis, ratio: f32, a: Node<T>, b: Node<T>) -> Node<T> {
        Node::Split { axis, ratio, a: Box::new(a), b: Box::new(b) }
    }

    /// The leaves, in reading order.
    pub fn leaves(&self) -> Vec<T> {
        let mut out = Vec::new();
        self.walk_leaves(&mut out);
        out
    }

    fn walk_leaves(&self, out: &mut Vec<T>) {
        match self {
            Node::Leaf(t) => out.push(*t),
            Node::Split { a, b, .. } => {
                a.walk_leaves(out);
                b.walk_leaves(out);
            }
        }
    }

    /// The same shape over other leaves; None when a leaf has no mapping.
    pub fn map<U>(&self, f: &impl Fn(T) -> Option<U>) -> Option<Node<U>> {
        Some(match self {
            Node::Leaf(t) => Node::Leaf(f(*t)?),
            Node::Split { axis, ratio, a, b } => Node::Split { axis: *axis, ratio: *ratio, a: Box::new(a.map(f)?), b: Box::new(b.map(f)?) },
        })
    }

    /// The template for these leaves: 2 a row, 3 an L, 4 a grid (read
    /// top-left, top-right, bottom-left, bottom-right), more halved in
    /// turn across and down.
    pub fn template(ids: &[T]) -> Option<Node<T>> {
        Some(match ids {
            [] => return None,
            [a] => Node::Leaf(*a),
            [a, b] => Node::split(Axis::Row, 0.5, Node::Leaf(*a), Node::Leaf(*b)),
            [a, b, c] => Node::split(Axis::Row, 0.5, Node::Leaf(*a), Node::split(Axis::Column, 0.5, Node::Leaf(*b), Node::Leaf(*c))),
            [a, b, c, d] => Node::split(
                Axis::Row,
                0.5,
                Node::split(Axis::Column, 0.5, Node::Leaf(*a), Node::Leaf(*c)),
                Node::split(Axis::Column, 0.5, Node::Leaf(*b), Node::Leaf(*d)),
            ),
            _ => Self::halves(ids, Axis::Row)?,
        })
    }

    fn halves(ids: &[T], axis: Axis) -> Option<Node<T>> {
        if ids.len() <= 1 {
            return ids.first().map(|&t| Node::Leaf(t));
        }
        let k = ids.len().div_ceil(2);
        let other = if axis == Axis::Row { Axis::Column } else { Axis::Row };
        Some(Node::split(axis, k as f32 / ids.len() as f32, Self::halves(&ids[..k], other)?, Self::halves(&ids[k..], other)?))
    }

    /// Take a leaf out; its sibling takes the room. None when the tree was
    /// only that leaf.
    pub fn remove(self, id: T) -> Option<Node<T>> {
        match self {
            Node::Leaf(t) => (t != id).then_some(Node::Leaf(t)),
            Node::Split { axis, ratio, a, b } => match (a.remove(id), b.remove(id)) {
                (Some(a), Some(b)) => Some(Node::split(axis, ratio, a, b)),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
        }
    }

    /// Split the leaf `at`: it and `id` share its room along `axis`, `id`
    /// after it (right or below) unless `before`. False when `at` isn't here.
    pub fn insert(&mut self, at: T, id: T, axis: Axis, before: bool) -> bool {
        match self {
            Node::Leaf(t) if *t == at => {
                let (old, new) = (Node::Leaf(at), Node::Leaf(id));
                *self = if before { Node::split(axis, 0.5, new, old) } else { Node::split(axis, 0.5, old, new) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.insert(at, id, axis, before) || b.insert(at, id, axis, before),
        }
    }

    /// Two leaves trade places.
    pub fn swap(&mut self, x: T, y: T) {
        match self {
            Node::Leaf(t) if *t == x => *t = y,
            Node::Leaf(t) if *t == y => *t = x,
            Node::Leaf(_) => {}
            Node::Split { a, b, .. } => {
                a.swap(x, y);
                b.swap(x, y);
            }
        }
    }

    /// Every leaf's rect inside `r`, with `rule` between siblings.
    pub fn rects(&self, r: Rect, rule: f32) -> Vec<(T, Rect)> {
        let mut out = Vec::new();
        self.walk_rects(r, rule, &mut out);
        out
    }

    fn walk_rects(&self, r: Rect, rule: f32, out: &mut Vec<(T, Rect)>) {
        match self {
            Node::Leaf(t) => out.push((*t, r)),
            Node::Split { axis, ratio, a, b } => {
                let (ra, rb, _) = parts(r, *axis, *ratio, rule);
                a.walk_rects(ra, rule, out);
                b.walk_rects(rb, rule, out);
            }
        }
    }

    /// Every rule: where it runs, its split's path and axis.
    pub fn rules(&self, r: Rect, rule: f32) -> Vec<(Path, Axis, Rect)> {
        let mut out = Vec::new();
        self.walk_rules(r, rule, &mut Vec::new(), &mut out);
        out
    }

    fn walk_rules(&self, r: Rect, rule: f32, path: &mut Path, out: &mut Vec<(Path, Axis, Rect)>) {
        if let Node::Split { axis, ratio, a, b } = self {
            let (ra, rb, line) = parts(r, *axis, *ratio, rule);
            out.push((path.clone(), *axis, line));
            path.push(false);
            a.walk_rules(ra, rule, path, out);
            path.pop();
            path.push(true);
            b.walk_rules(rb, rule, path, out);
            path.pop();
        }
    }

    /// The rect the split at `path` lays out, and that split's axis.
    pub fn node_rect(&self, path: &[bool], r: Rect, rule: f32) -> Option<(Rect, Axis)> {
        match (self, path.split_first()) {
            (Node::Split { axis, .. }, None) => Some((r, *axis)),
            (Node::Split { axis, ratio, a, b }, Some((&side, rest))) => {
                let (ra, rb, _) = parts(r, *axis, *ratio, rule);
                if side { b.node_rect(rest, rb, rule) } else { a.node_rect(rest, ra, rule) }
            }
            (Node::Leaf(_), _) => None,
        }
    }

    /// Where a leaf sits: the path to it.
    pub fn path_of(&self, id: T) -> Option<Path> {
        match self {
            Node::Leaf(t) => (*t == id).then(Vec::new),
            Node::Split { a, b, .. } => {
                if let Some(mut p) = a.path_of(id) {
                    p.insert(0, false);
                    return Some(p);
                }
                let mut p = b.path_of(id)?;
                p.insert(0, true);
                Some(p)
            }
        }
    }

    /// The split at `path`.
    pub fn at(&self, path: &[bool]) -> Option<&Node<T>> {
        match (self, path.split_first()) {
            (n, None) => Some(n),
            (Node::Split { a, b, .. }, Some((&side, rest))) => if side { b.at(rest) } else { a.at(rest) },
            (Node::Leaf(_), Some(_)) => None,
        }
    }

    /// Every rule evened out: each side gets room in proportion to the
    /// tiles in it, so every tile comes out the same area.
    pub fn even(&mut self) {
        if let Node::Split { ratio, a, b, .. } = self {
            a.even();
            b.even();
            let (na, nb) = (a.leaves().len() as f32, b.leaves().len() as f32);
            *ratio = na / (na + nb);
        }
    }

    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        match (self, path.split_first()) {
            (Node::Split { ratio, .. }, None) => *ratio = value,
            (Node::Split { a, b, .. }, Some((&side, rest))) => if side { b.set_ratio(rest, value) } else { a.set_ratio(rest, value) },
            (Node::Leaf(_), _) => {}
        }
    }
}

/// A split's two halves and the rule between them.
fn parts(r: Rect, axis: Axis, ratio: f32, rule: f32) -> (Rect, Rect, Rect) {
    match axis {
        Axis::Row => {
            let w = ((r.w - rule) * ratio).round();
            (Rect::new(r.x, r.y, w, r.h), Rect::new(r.x + w + rule, r.y, r.w - w - rule, r.h), Rect::new(r.x + w, r.y, rule, r.h))
        }
        Axis::Column => {
            let h = ((r.h - rule) * ratio).round();
            (Rect::new(r.x, r.y, r.w, h), Rect::new(r.x, r.y + h + rule, r.w, r.h - h - rule), Rect::new(r.x, r.y + h, r.w, rule))
        }
    }
}

/// The tiled tabs, by id (indexes shift; ids don't), in their tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Tiling {
    pub root: Node<u64>,
}

impl Tiling {
    /// A tiling from the template for these tabs; None under two.
    pub fn of(ids: &[u64]) -> Option<Tiling> {
        (ids.len() >= 2).then(|| Node::template(ids).map(|root| Tiling { root })).flatten()
    }

    /// The tabs, in reading order.
    pub fn ids(&self) -> Vec<u64> {
        self.root.leaves()
    }

    /// Add a tab by splitting the largest tile along its longer side.
    pub fn grow(&mut self, id: u64, area: Rect, rule: f32) {
        // The first of equals in reading order, so growing is predictable.
        let Some((at, r)) = self.root.rects(area, rule).into_iter().reduce(|best, x| if x.1.w * x.1.h > best.1.w * best.1.h { x } else { best }) else { return };
        self.root.insert(at, id, if r.w >= r.h { Axis::Row } else { Axis::Column }, false);
    }

    /// Without this tab; None when fewer than two would be left.
    pub fn without(&self, id: u64) -> Option<Tiling> {
        let root = self.root.clone().remove(id)?;
        matches!(root, Node::Split { .. }).then_some(Tiling { root })
    }
}

/// A grabbed rule: which split it belongs to.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    pub path: Path,
    pub axis: Axis,
}

/// Which way a rule under the pointer runs, for the resize arrow.
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
            Pane::Home(h) => h.rect,
            Pane::Editor(e) => e.rect,
            Pane::Ports(p) => p.rect,
            Pane::Downloads(p) => p.rect,
        }
    }
}

impl App {
    /// The tiled tabs as indexes, in reading order, when the active tab is
    /// one of them; otherwise none (the tiling waits for its tabs).
    pub(crate) fn tiled(&self) -> Vec<usize> {
        let Some(t) = &self.tiling else { return Vec::new() };
        let idx: Vec<usize> = t.ids().iter().filter_map(|id| self.tabs.iter().position(|tab| tab.id == *id)).collect();
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
        self.tiling.as_ref().is_some_and(|t| self.tabs.get(i).is_some_and(|tab| t.ids().contains(&tab.id)))
    }

    /// Each shown tile's tab index and rect.
    pub(crate) fn tile_rects(&self) -> Vec<(usize, Rect)> {
        if !self.tiling_shown() {
            return Vec::new();
        }
        let Some(t) = &self.tiling else { return Vec::new() };
        t.root
            .rects(self.content_rect(), self.px(m::STRUCTURE))
            .into_iter()
            .filter_map(|(id, r)| self.tabs.iter().position(|tab| tab.id == id).map(|i| (i, r)))
            .collect()
    }

    /// Tile the selection with the active tab. A tiling that is showing
    /// grows by the selection, tile by tile; otherwise a template.
    pub(crate) fn tile_selected(&mut self) {
        let shown = self.tiled();
        let mut more: Vec<usize> = self.selected.iter().copied().filter(|&i| i < self.tabs.len() && !shown.contains(&i) && i != self.active).collect();
        more.sort_unstable();
        let next = if shown.is_empty() {
            let mut idx = vec![self.active];
            idx.extend(more);
            idx.truncate(MAX_TILES);
            Tiling::of(&idx.iter().map(|&i| self.tabs[i].id).collect::<Vec<_>>())
        } else {
            let mut t = self.tiling.clone();
            if let Some(t) = t.as_mut() {
                let (area, rule) = (self.content_rect(), self.px(m::STRUCTURE));
                for i in more.into_iter().take(MAX_TILES.saturating_sub(shown.len())) {
                    t.grow(self.tabs[i].id, area, rule);
                }
            }
            t
        };
        if next.is_none() {
            return;
        }
        self.selected.clear();
        self.direct(crate::director::Op::Tiling(next));
    }

    pub(crate) fn untile(&mut self) {
        self.direct(crate::director::Op::Tiling(None));
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

    /// Drop a closed tab from the tiling; its neighbour takes the room,
    /// and one left ends it.
    pub(crate) fn tile_forget(&mut self, id: u64) {
        if let Some(t) = self.tiling.take() {
            self.tiling = t.without(id);
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
        let Some(mut t) = self.tiling.clone() else { return };
        t.root.swap(self.tabs[tiles[k]].id, self.tabs[tiles[j]].id);
        self.direct(crate::director::Op::Tiling(Some(t)));
    }

    /// The rule under the point, with a 6px grab zone; the innermost wins
    /// where two meet.
    pub(crate) fn tile_rule_at(&self, x: f32, y: f32) -> Option<Grab> {
        if !self.tiling_shown() {
            return None;
        }
        let t = self.tiling.as_ref()?;
        let grab = self.px(6.0);
        t.root
            .rules(self.content_rect(), self.px(m::STRUCTURE))
            .into_iter()
            .filter(|(_, axis, r)| match axis {
                Axis::Row => (x - (r.x + r.w / 2.0)).abs() <= grab && y >= r.y && y <= r.bottom(),
                Axis::Column => (y - (r.y + r.h / 2.0)).abs() <= grab && x >= r.x && x <= r.right(),
            })
            .max_by_key(|(path, _, _)| path.len())
            .map(|(path, axis, _)| Grab { path, axis })
    }

    /// Which way the rule under the point runs, for the pointer.
    pub(crate) fn divider_at(&self, x: f32, y: f32) -> Option<Divider> {
        self.tile_rule_at(x, y).map(|g| if g.axis == Axis::Row { Divider::X } else { Divider::Y })
    }

    /// Follow a rule drag; neither side goes under 120px.
    pub(crate) fn tile_drag_to(&mut self, x: f32, y: f32) {
        let Some(g) = self.tile_drag.clone() else { return };
        let (c, rule, min) = (self.content_rect(), self.px(m::STRUCTURE), self.px(120.0));
        let Some(t) = self.tiling.as_mut() else { return };
        let Some((r, axis)) = t.root.node_rect(&g.path, c, rule) else { return };
        let (at, from, span) = match axis {
            Axis::Row => (x, r.x, r.w),
            Axis::Column => (y, r.y, r.h),
        };
        let lo = (min / span).min(0.5);
        t.root.set_ratio(&g.path, ((at - from) / span).clamp(lo, 1.0 - lo));
        self.layout();
        self.resize_due = Some(crate::clock::now() + std::time::Duration::from_millis(60));
    }

    /// Lay the tiled tabs out; false when there is no tiling to show.
    pub(crate) fn layout_tiles(&mut self) -> bool {
        let rects = self.tile_rects();
        if rects.is_empty() {
            return false;
        }
        let c = self.content_rect();
        let off = Rect::new(-4.0 * c.w - c.x, c.y, c.w, c.h);
        let header = self.header_h();
        let pad_x = self.px(18.0);
        let pad_y = self.px(16.0);
        let scale = self.scale;
        for (i, r) in rects {
            let tab = &mut self.tabs[i];
            let right_shown = tab.focus_right && tab.right.is_some();
            crate::app::place_pane(&mut tab.left, if right_shown { off } else { r }, header, pad_x, pad_y, scale, true);
            if let Some(p) = tab.right.as_mut() {
                crate::app::place_pane(p, if right_shown { r } else { off }, header, pad_x, pad_y, scale, true);
            }
        }
        self.dirty = true;
        true
    }

    /// Draw the tiled tabs and the rules between them; false when there is
    /// no tiling to show.
    pub(crate) fn draw_tiling(&mut self, scene: &mut Scene) -> bool {
        let rects = self.tile_rects();
        if rects.is_empty() {
            return false;
        }
        let ink = self.theme.ink;
        let rule = self.px(m::STRUCTURE);
        let rules = self.tiling.as_ref().map(|t| t.root.rules(self.content_rect(), rule)).unwrap_or_default();
        let active = self.active;
        let many = rects.len() > 1;
        let mut tabs = std::mem::take(&mut self.tabs);
        for &(i, r) in &rects {
            let n = format!("{:02}", i + 1);
            let look = tabs[i].look.clone();
            let tab = &mut tabs[i];
            let focused = i == active;
            let right_shown = tab.focus_right && tab.right.is_some();
            let pane = if right_shown { tab.right.as_mut().unwrap() } else { &mut tab.left };
            scene.layer(Some(r));
            self.draw_pane(scene, pane, &n, focused, &look, true);
            scene.layer(None);
            // The focused tile: a hairline of the tab's signal under its top.
            if focused && many {
                let s = look.signal.unwrap_or(self.surface.signal);
                scene.rect(Rect::new(r.x, r.y, r.w, self.px(2.0)), s);
            }
        }
        self.tabs = tabs;
        // Rules; the one being dragged, or under the pointer, in the signal.
        let hot = self.tile_drag.clone().or_else(|| self.tile_rule_at(self.mouse.0, self.mouse.1));
        for (path, _, r) in rules {
            let c = if hot.as_ref().is_some_and(|g| g.path == path) { self.surface.signal } else { ink };
            scene.rect(r, c);
        }
        true
    }

    /// The tile under the point, as a tab index.
    pub(crate) fn tile_at(&self, x: f32, y: f32) -> Option<usize> {
        self.tile_rects().into_iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 1000.0, 600.0)
    }

    #[test]
    fn templates_keep_the_old_shapes() {
        let two = Node::template(&[1u64, 2]).unwrap().rects(area(), 2.0);
        assert_eq!(two.len(), 2);
        assert!(two[0].1.right() < two[1].1.x && two[0].1.h == 600.0);
        // The L: one tall on the left, two stacked on the right.
        let three = Node::template(&[1u64, 2, 3]).unwrap().rects(area(), 2.0);
        assert_eq!(three[0].1.h, 600.0);
        assert_eq!(three[1].1.x, three[2].1.x);
        assert!(three[1].1.bottom() < three[2].1.y);
        // The grid reads top-left, top-right, bottom-left, bottom-right.
        let four = Node::template(&[1u64, 2, 3, 4]).unwrap();
        assert_eq!(four.leaves(), vec![1, 3, 2, 4]);
        let r: std::collections::HashMap<u64, Rect> = four.rects(area(), 2.0).into_iter().collect();
        assert!(r[&1].x < r[&2].x && r[&1].y == r[&2].y && r[&3].y > r[&1].y && r[&3].x == r[&1].x);
        // More than four: every tab gets a tile, and they cover the area.
        let eight = Node::template(&[1u64, 2, 3, 4, 5, 6, 7, 8]).unwrap().rects(area(), 0.0);
        assert_eq!(eight.len(), 8);
        let covered: f32 = eight.iter().map(|(_, r)| r.w * r.h).sum();
        assert!((covered - 600_000.0).abs() < 1.0);
    }

    #[test]
    fn growing_splits_the_largest_tile_along_its_long_side() {
        let mut t = Tiling::of(&[1, 2]).unwrap();
        t.grow(3, area(), 2.0);
        let r: std::collections::HashMap<u64, Rect> = t.root.rects(area(), 2.0).into_iter().collect();
        // Tile 1 was 499×600: taller than wide, so 3 goes under it.
        assert_eq!(r[&1].x, r[&3].x);
        assert!(r[&3].y > r[&1].y);
        assert_eq!(r[&2].h, 600.0);
    }

    #[test]
    fn removing_gives_the_room_to_the_neighbour() {
        let t = Tiling::of(&[1, 2, 3]).unwrap();
        let t = t.without(2).unwrap();
        let r = t.root.rects(area(), 0.0);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|(_, r)| r.h == 600.0));
        assert!(t.without(1).is_none(), "one tab left is no tiling");
    }

    #[test]
    fn every_rule_is_addressable_and_resizes_its_own_split() {
        let mut t = Tiling::of(&[1, 2, 3]).unwrap();
        let rules = t.root.rules(area(), 2.0);
        assert_eq!(rules.len(), 2);
        let (inner, axis, _) = rules.iter().find(|(p, _, _)| p == &vec![true]).unwrap().clone();
        assert_eq!(axis, Axis::Column);
        t.root.set_ratio(&inner, 0.25);
        let r: std::collections::HashMap<u64, Rect> = t.root.rects(area(), 2.0).into_iter().collect();
        assert!(r[&2].h < r[&3].h);
        assert_eq!(r[&1].h, 600.0);
        let (nr, ax) = t.root.node_rect(&inner, area(), 2.0).unwrap();
        assert_eq!(ax, Axis::Column);
        assert!(nr.x > 400.0);
    }

    #[test]
    fn swap_and_insert_before() {
        let mut t = Tiling::of(&[1, 2]).unwrap();
        t.root.swap(1, 2);
        assert_eq!(t.ids(), vec![2, 1]);
        assert!(t.root.insert(1, 9, Axis::Column, true));
        assert_eq!(t.ids(), vec![2, 9, 1]);
        assert!(!t.root.insert(42, 10, Axis::Row, false));
    }

    #[test]
    fn paths_and_evening_out() {
        let mut t = Node::template(&[1u64, 2, 3]).unwrap();
        assert_eq!(t.path_of(3), Some(vec![true, true]));
        assert_eq!(t.path_of(1), Some(vec![false]));
        assert_eq!(t.path_of(9), None);
        t.set_ratio(&[], 0.8);
        t.even();
        // Even means every tile the same area: the L's tall tile takes a third.
        let r: std::collections::HashMap<u64, Rect> = t.rects(area(), 0.0).into_iter().collect();
        assert!((r[&1].w - 333.0).abs() < 1.0);
        assert!((r[&1].w * r[&1].h - r[&2].w * r[&2].h).abs() < 1000.0);
        assert!(matches!(t.at(&[true]), Some(Node::Split { axis: Axis::Column, .. })));
    }

    #[test]
    fn saved_shapes_round_trip() {
        let t = Node::template(&[0usize, 1, 2]).unwrap();
        let back: Node<usize> = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        let ids = [10u64, 20, 30];
        assert_eq!(back.map(&|k| ids.get(k).copied()).unwrap().leaves(), vec![10, 20, 30]);
    }
}
