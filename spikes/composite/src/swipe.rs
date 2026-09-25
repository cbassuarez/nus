//! Back and forward, by hand. Two fingers sideways on a page (a wheel
//! that is mostly sideways) draws an arrow at the page's edge that
//! grows with the swipe; past the threshold it is back (fingers going
//! right) or forward, once, and the arrow fills. The mouse's own back
//! and forward buttons and Alt+←/→ take the same road (`navigate`).
//!
//! Back on a page with nowhere to go closes a tab that was opened onto
//! that page — a link's new tab, a popup, one from the prompt — and
//! returns to the tab before it, the way a browser does.

use std::time::{Duration, Instant};

use nus_render::{Rect, Scene};

use crate::app::{fade, App, Pane, WebPane};
use crate::settings::SwipeLook;

/// How long after the last movement the overlay lingers.
const LINGER: Duration = Duration::from_millis(320);

/// Movement further apart than this is a new gesture.
const GAP: Duration = Duration::from_millis(250);

/// What a swipe that far would do, read when it moves (the tabs are not
/// to hand while the page draws).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Back,
    Forward,
    /// Back on a tab's first page: the tab closes.
    CloseTab,
    /// No page that way.
    Nowhere,
}

impl Dest {
    fn words(self, back: bool) -> &'static str {
        match self {
            Dest::Back => "BACK",
            Dest::Forward => "FORWARD",
            Dest::CloseTab => "CLOSE TAB",
            Dest::Nowhere if back => "NOTHING BACK",
            Dest::Nowhere => "NOTHING FORWARD",
        }
    }
}

/// A sideways swipe on a page: how far (logical px, + = back), when it
/// last moved, whether it has fired (the rest of the gesture is spent),
/// and where it goes.
#[derive(Clone, Copy, Debug)]
pub struct Swipe {
    pub far: f32,
    pub at: Instant,
    pub fired: bool,
    pub dest: Dest,
}

impl App {
    /// BACK & FORWARD · SWIPE DISTANCE, in logical px.
    pub(crate) fn swipe_reach(&self) -> f32 {
        (self.behavior.swipe_reach as f32).clamp(80.0, 400.0)
    }

    /// Where back or forward would go on the active tab's page.
    fn swipe_dest(&self, right: bool, back: bool) -> Dest {
        let Some(tab) = self.tabs.get(self.active) else { return Dest::Nowhere };
        let pane = if right { tab.right.as_ref() } else { Some(&tab.left) };
        let Some(Pane::Web(w)) = pane else { return Dest::Nowhere };
        match back {
            true if w.tab.can_go_back() => Dest::Back,
            true if !right && tab.right.is_none() && !tab.pinned && !tab.hatch && self.tabs.len() > 1 => Dest::CloseTab,
            false if w.tab.can_go_forward() => Dest::Forward,
            _ => Dest::Nowhere,
        }
    }

    /// A sideways wheel step `sx` (logical px) on the page: the gesture
    /// grows, and past the distance it fires once — momentum after that
    /// is spent, not a second navigation.
    pub(crate) fn swipe_step(&mut self, right: bool, sx: f32) {
        let now = crate::clock::now();
        let reach = self.swipe_reach();
        let prev = {
            let Some(tab) = self.tabs.get(self.active) else { return };
            let pane = if right { tab.right.as_ref() } else { Some(&tab.left) };
            let Some(Pane::Web(w)) = pane else { return };
            w.swipe.filter(|s| crate::clock::since(&s.at) < GAP)
        };
        let (far, fired) = match prev {
            Some(s) if s.fired => (s.far, true),
            Some(s) => (s.far + sx, false),
            None => (sx, false),
        };
        let back = far > 0.0;
        let dest = match prev {
            Some(s) if s.fired => s.dest,
            _ => self.swipe_dest(right, back),
        };
        let fire = !fired && far.abs() >= reach;
        let far = if fire { far.signum() * reach } else { far };
        if let Some(tab) = self.tabs.get_mut(self.active) {
            let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
            if let Some(Pane::Web(w)) = pane {
                w.swipe = Some(Swipe { far, at: now, fired: fired || fire, dest });
            }
        }
        self.dirty = true;
        if fire && dest != Dest::Nowhere {
            self.navigate(right, back);
        }
    }

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
                self.notice(nus_render::text::icons::BACK, "Tab Closed", "reopen-closed brings it back");
            }
        } else if w.tab.can_go_forward() {
            w.tab.forward();
        }
        self.dirty = true;
    }

    /// The swipe, over the page, in the look SWIPE OVERLAY picks: how far
    /// it has come, whether it fired, and what it does — dim when there is
    /// nowhere to go. Gone soon after the fingers stop.
    pub(crate) fn draw_swipe(&mut self, scene: &mut Scene, w: &mut WebPane) {
        let Some(sw) = w.swipe else { return };
        let idle = crate::clock::since(&sw.at);
        if idle > LINGER {
            w.swipe = None;
            return;
        }
        let look = self.behavior.swipe_look;
        if look == SwipeLook::Off {
            return;
        }
        let ink = self.theme.ink;
        let paper = self.paper();
        let signal = self.surface.signal;
        let dim = self.theme.dim;
        let reach = self.swipe_reach();
        let progress = (sw.far.abs() / reach).clamp(0.0, 1.0);
        let fired = sw.fired;
        let live = sw.dest != Dest::Nowhere;
        let fade_out = 1.0 - (idle.as_secs_f32() / LINGER.as_secs_f32()).clamp(0.0, 1.0);
        let a = if fired { fade_out } else { progress * fade_out.max(0.35) };
        if a <= 0.01 {
            return;
        }
        let back = sw.far > 0.0;
        let lit = fired && live;
        let page = w.page;
        let icon = if sw.dest == Dest::CloseTab { nus_render::text::icons::CLOSE } else if back { nus_render::text::icons::BACK } else { nus_render::text::icons::FORWARD };
        let isz = self.px(16.0);
        let mark = if live { ink } else { dim };
        scene.layer(Some(page));
        match look {
            SwipeLook::Arrow | SwipeLook::Off => {
                let d = self.px(36.0);
                let inset = self.px(12.0) + self.px(24.0) * progress;
                let x = if back { page.x + inset } else { page.right() - inset - d };
                let y = page.y + (page.h - d) * 0.5;
                let r = Rect::new(x.round(), y.round(), d, d);
                // A hard shadow, the disc, a ring for the distance, the arrow.
                scene.push(nus_render::Instance::rounded(Rect::new(r.x + self.px(2.0), r.y + self.px(2.0), d, d), d / 2.0, fade(ink, 0.5 * a)));
                scene.push(nus_render::Instance::rounded(r, d / 2.0, fade(if lit { signal } else { paper }, a)));
                scene.push(nus_render::Instance::stroke(r, d / 2.0, self.px(1.5), fade(mark, a), None, 0.0));
                if !fired && live {
                    // The distance, as a bar under the disc.
                    scene.rect(Rect::new(r.x, r.bottom() + self.px(6.0), d * progress, self.px(2.0)), fade(signal, a));
                }
                self.fonts.draw_icon(scene, icon, isz, r.x + ((d - isz) / 2.0).round(), r.y + ((d - isz) / 2.0).round(), fade(if lit { paper } else { mark }, a));
            }
            SwipeLook::Card => {
                let st = nus_render::Style { color: fade(if lit { paper } else { mark }, a), ..self.label_strong() };
                let words = sw.dest.words(back);
                let tw = self.fonts.measure(st, words);
                let pad = self.px(12.0);
                let (cw, ch) = (pad * 2.0 + isz + self.px(8.0) + tw, self.px(38.0));
                let inset = self.px(12.0) + self.px(24.0) * progress;
                let x = if back { page.x + inset } else { page.right() - inset - cw };
                let y = page.y + (page.h - ch) * 0.5;
                let r = Rect::new(x.round(), y.round(), cw.round(), ch);
                scene.rect(Rect::new(r.x + self.px(2.0), r.y + self.px(2.0), r.w, r.h), fade(ink, 0.5 * a));
                scene.rect(r, fade(if lit { signal } else { paper }, a));
                scene.outline(r, self.px(1.5), fade(mark, a));
                if !fired && live {
                    // The distance, filling the card's foot.
                    scene.rect(Rect::new(r.x, r.bottom() - self.px(3.0), r.w * progress, self.px(3.0)), fade(signal, a));
                }
                let iy = r.y + ((ch - isz) / 2.0).round();
                self.fonts.draw_icon(scene, icon, isz, r.x + pad, iy, st.color);
                self.fonts.draw(scene, st, r.x + pad + isz + self.px(8.0), r.y + ch / 2.0 + st.px * 0.36, words);
            }
            SwipeLook::Edge => {
                // A band down the edge the fingers pull from, as wide as
                // the swipe has come; signal once it fires.
                let bw = (self.px(4.0) + self.px(28.0) * progress).round();
                let x = if back { page.x } else { page.right() - bw };
                let band = Rect::new(x, page.y, bw, page.h);
                let color = if lit { signal } else if live { ink } else { dim };
                scene.rect(band, fade(color, a * if lit { 0.9 } else { 0.18 + 0.4 * progress }));
                scene.rect(Rect::new(if back { band.right() - self.px(2.0) } else { band.x }, page.y, self.px(2.0), page.h), fade(color, a));
                let ix = if back { band.right() + self.px(8.0) } else { band.x - self.px(8.0) - isz };
                self.fonts.draw_icon(scene, icon, isz, ix.round(), (page.y + (page.h - isz) / 2.0).round(), fade(mark, a));
            }
        }
        scene.layer(None);
        self.dirty = true;
    }
}
