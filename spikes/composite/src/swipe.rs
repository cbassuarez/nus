//! Back and forward, by hand. Two fingers sideways on a page (a wheel
//! that is mostly sideways) draws an arrow at the page's edge that
//! grows with the swipe; past the threshold it is back (fingers going
//! right) or forward, once, and the arrow fills. The mouse's own back
//! and forward buttons and Alt+←/→ take the same road (`navigate`).
//!
//! Back on a page with nowhere to go closes a tab that was opened onto
//! that page — a link's new tab, a popup, one from the prompt — and
//! returns to the tab before it, the way a browser does.

use std::time::Duration;

use nus_render::{Rect, Scene};

use crate::app::{fade, App, Pane, WebPane};

/// Logical px a swipe travels before it is a navigation.
pub const THRESHOLD: f32 = 180.0;

/// How long after the last movement the arrow lingers.
const LINGER: Duration = Duration::from_millis(320);

impl App {
    /// Back or forward on the active tab's page (the right pane when
    /// `right`). See the module note for the tab that closes.
    pub(crate) fn navigate(&mut self, right: bool, back: bool) {
        let i = self.active;
        let Some(tab) = self.tabs.get(i) else { return };
        let pane = if right { tab.right.as_ref() } else { Some(&tab.left) };
        let Some(Pane::Web(w)) = pane else { return };
        if back {
            if w.tab.can_go_back() {
                w.tab.back();
            } else if !right && tab.right.is_none() && !tab.pinned && !tab.hatch && self.tabs.len() > 1 {
                // The first page of a tab of its own: the tab goes, and the
                // one before it comes back. Reopen-closed brings it again.
                self.selected.clear();
                self.close_tabs(true);
                self.notice("back · the tab closed; reopen-closed brings it back");
            }
        } else if w.tab.can_go_forward() {
            w.tab.forward();
        }
        self.dirty = true;
    }

    /// The swipe's arrow over a page: at the edge the fingers came from,
    /// growing with the distance, full once it fired; gone soon after.
    pub(crate) fn draw_swipe(&mut self, scene: &mut Scene, w: &mut WebPane) {
        let Some((far, at)) = w.swipe else { return };
        let idle = crate::clock::since(at);
        if idle > LINGER {
            w.swipe = None;
            return;
        }
        let ink = self.theme.ink;
        let paper = self.paper();
        let signal = self.surface.signal;
        let progress = (far.abs() / THRESHOLD).clamp(0.0, 1.0);
        let fired = far.abs() >= THRESHOLD;
        let fade_out = 1.0 - (idle.as_secs_f32() / LINGER.as_secs_f32()).clamp(0.0, 1.0);
        let a = if fired { fade_out } else { progress.min(1.0) * fade_out.max(0.35) };
        if a <= 0.01 {
            return;
        }
        let d = self.px(36.0);
        let inset = self.px(12.0) + self.px(24.0) * progress;
        let back = far > 0.0;
        let x = if back { w.page.x + inset } else { w.page.right() - inset - d };
        let y = w.page.y + (w.page.h - d) * 0.5;
        let r = Rect::new(x.round(), y.round(), d, d);
        scene.layer(Some(w.page));
        // A hard shadow, the disc, the arrow: signal when it has fired.
        scene.push(nus_render::Instance::rounded(Rect::new(r.x + self.px(2.0), r.y + self.px(2.0), d, d), d / 2.0, fade(ink, 0.5 * a)));
        scene.push(nus_render::Instance::rounded(r, d / 2.0, fade(if fired { signal } else { paper }, a)));
        scene.push(nus_render::Instance::stroke(r, d / 2.0, self.px(1.5), fade(ink, a), None, 0.0));
        let isz = self.px(16.0);
        let icon = if back { nus_render::text::icons::BACK } else { nus_render::text::icons::FORWARD };
        self.fonts.draw_icon(scene, icon, isz, r.x + ((d - isz) / 2.0).round(), r.y + ((d - isz) / 2.0).round(), fade(if fired { paper } else { ink }, a));
        scene.layer(None);
        self.dirty = true;
    }
}
