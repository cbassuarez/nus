//! The anchor: one pane a window keeps beside every tab until it is let go.
//! A shell, a page, an editor: anchored, it stays on the right while the
//! tabs change past it, and comes back with the session.
//!
//! It is the window's, not a tab's, but it is drawn, laid out and fed
//! input as the right pane of the tab in front: as tabs change it is lent
//! to the new one (`tend_anchor`), and that tab's own right pane, if it has
//! one, waits in `held_right` until the anchor moves on. So everything that
//! already knows about split panes knows about the anchor too.
//!
//! Anything that changes the tabs' structure (the director's ops, closing,
//! a tab that goes on its own) parks it first (`park_anchor`, or
//! `anchor_release` for one tab), so a tab that goes never takes it along.
//! Only its own controls end it: let go (back into a tab of its own, right
//! after the one in front) or closed. ⌘W closes the tab in front, never
//! the anchor.

use crate::app::{App, Pane};
use nus_render::text::icons;

#[derive(Default)]
pub struct Anchor {
    /// The tab it is lent to, by id; None while parked.
    pub host: Option<u64>,
    /// The pane while it is parked.
    pub parked: Option<Pane>,
    /// Its width in logical px, once dragged.
    pub w: Option<f32>,
}

impl App {
    /// Whether the tab in front holds the anchor now.
    pub(crate) fn anchor_lent_here(&self) -> bool {
        let id = self.tabs.get(self.active).map(|t| t.id);
        self.anchor.as_ref().is_some_and(|a| a.host.is_some() && a.host == id)
    }

    /// The tabs the anchor can stand beside: not the hatch's, not a peek,
    /// not while tabs are tiled.
    fn anchor_can_stand_by(&self, i: usize) -> bool {
        self.tabs.get(i).is_some_and(|t| !t.hatch && t.peek.is_none()) && !self.tiling_shown()
    }

    /// Take the anchor back from the tab it is lent to: that tab gets its
    /// own right pane back.
    pub(crate) fn park_anchor(&mut self) {
        let Some(a) = self.anchor.as_mut() else { return };
        let Some(host) = a.host.take() else { return };
        let Some(t) = self.tabs.iter_mut().find(|t| t.id == host) else {
            tracing::warn!("anchor: the tab it was lent to went without it");
            self.anchor = None;
            return;
        };
        a.parked = t.right.take();
        a.w = t.split_w;
        t.right = t.held_right.take();
        t.focus_right = t.held_focus_right && t.right.is_some();
        t.split_w = t.held_split_w.take();
        t.solo = t.held_solo && t.right.is_some();
        t.held_focus_right = false;
        t.held_solo = false;
    }

    /// Lend the anchor to the tab in front when it can stand beside it.
    pub(crate) fn tend_anchor(&mut self) {
        let i = self.active;
        let Some(a) = self.anchor.as_ref() else { return };
        if !self.anchor_can_stand_by(i) {
            // Nowhere to go (the hatch's tab in front for a moment, a peek):
            // it stays where it is, unless that tab can't hold it any more
            // (moved into the hatch, tabs tiled).
            let host = a.host.and_then(|h| self.tabs.iter().position(|t| t.id == h));
            if host.is_some_and(|k| !self.anchor_can_stand_by(k)) {
                self.park_anchor();
                self.layout();
            }
            return;
        }
        if a.host == Some(self.tabs[i].id) {
            return;
        }
        self.park_anchor();
        let Some(a) = self.anchor.as_mut() else { return };
        let Some(pane) = a.parked.take() else {
            self.anchor = None;
            return;
        };
        let t = &mut self.tabs[i];
        t.held_right = t.right.take();
        t.held_focus_right = t.focus_right;
        t.held_split_w = t.split_w;
        t.held_solo = t.solo;
        t.right = Some(pane);
        t.focus_right = false;
        t.split_w = a.w;
        t.solo = false;
        a.host = Some(t.id);
        self.layout();
        self.dirty = true;
    }

    /// Tab `i` is about to go: if it holds the anchor, the anchor is parked.
    pub(crate) fn anchor_release(&mut self, i: usize) {
        let id = self.tabs.get(i).map(|t| t.id);
        if self.anchor.as_ref().is_some_and(|a| a.host.is_some() && a.host == id) {
            self.park_anchor();
        }
    }

    /// A pane of tab `i` ended on its own (its page closed itself, its shell
    /// exited). True when that pane was the anchor: the anchor is over and
    /// the tab has its own right pane back. When it was the other pane of
    /// the tab holding the anchor, the anchor is parked first, so the tab's
    /// own panes close up without taking it.
    pub(crate) fn anchor_ended(&mut self, i: usize, right: bool) -> bool {
        let id = self.tabs.get(i).map(|t| t.id);
        if !self.anchor.as_ref().is_some_and(|a| a.host.is_some() && a.host == id) {
            return false;
        }
        if !right {
            self.park_anchor();
            return false;
        }
        let t = &mut self.tabs[i];
        t.right = t.held_right.take();
        t.focus_right = t.held_focus_right && t.right.is_some();
        t.split_w = t.held_split_w.take();
        t.solo = t.held_solo && t.right.is_some();
        self.anchor = None;
        self.layout();
        self.dirty = true;
        true
    }

    /// Anchor a pane of the tab in front: the right one, or the left. A
    /// pane alone in its tab takes the tab's place; the tab ends, nothing
    /// in it stops. One anchor a window: an earlier one goes back into a
    /// tab of its own.
    pub(crate) fn anchor_pane(&mut self, right: bool) {
        if self.anchor_lent_here() && right {
            self.unanchor();
            return;
        }
        if !self.anchor_can_stand_by(self.active) {
            self.notice(icons::ANCHOR, "Can't Anchor Here", "only a tab in the sidebar can be anchored");
            return;
        }
        if self.anchor.is_some() {
            self.unanchor_behind();
        }
        let i = self.active;
        let Some(t) = self.tabs.get(i) else { return };
        let pane = if t.right.is_some() {
            let Some(p) = self.take_pane(i, right) else { return };
            p
        } else {
            if self.tabs.iter().filter(|t| !t.hatch && t.peek.is_none()).count() <= 1 {
                self.notice(icons::ANCHOR, "Open Another Tab First", "an anchor stays beside other tabs");
                return;
            }
            let id = t.id;
            if self.tabs.iter().any(|t| t.parent == Some(id)) {
                self.notice(icons::STACK, "Move Its Pages First", "that tab has pages under it");
                return;
            }
            let tab = self.tabs.remove(i);
            self.tile_forget(tab.id);
            self.tab_removed(i);
            let next = self.mru.first().copied().unwrap_or(0).min(self.tabs.len() - 1);
            self.active = next;
            tab.left
        };
        self.anchor = Some(Anchor { host: None, parked: Some(pane), w: None });
        let i = self.active;
        self.activate(i);
        self.tend_anchor();
        self.notice(icons::ANCHOR, "Anchored", "beside every tab until you let it go");
        self.save_session();
        self.dirty = true;
    }

    /// The anchor back into a tab of its own, right after the one in
    /// front, and in front itself.
    pub(crate) fn unanchor(&mut self) {
        if let Some(at) = self.unanchor_behind() {
            self.activate(at);
        }
        self.layout();
        self.save_session();
        self.dirty = true;
    }

    /// The anchor back into a tab of its own right after the one in front,
    /// which stays in front. Its index, when there was an anchor.
    fn unanchor_behind(&mut self) -> Option<usize> {
        self.park_anchor();
        let pane = self.anchor.take()?.parked?;
        let tab = self.make_tab(pane, None);
        let at = (self.active + 1).min(self.tabs.len());
        self.insert_tab_at(at, tab);
        Some(at)
    }

    /// Close the anchored pane: what runs there stops. It goes the way a
    /// tab closes, so a page can still ask before it leaves.
    pub(crate) fn close_anchor(&mut self) {
        let back = self.active;
        let Some(at) = self.unanchor_behind() else { return };
        self.selected.clear();
        self.active = at;
        self.close_tabs(true);
        // The tab that was in front stays in front.
        self.activate(back.min(self.tabs.len().saturating_sub(1)));
        self.layout();
        self.dirty = true;
    }
}
