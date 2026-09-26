//! Scrolling in the shell, the way neoscroll.nvim does it: the view
//! moves line by line, but on an eased clock — a wheel tick or Shift+
//! PgUp asks for a distance, and the offset follows an easing curve to
//! it over a short time; more ticks extend the trip from where it is.
//! The curves are neoscroll's (quadratic, cubic, quartic, quintic,
//! circular, sine), chosen under TERMINAL · SCROLL; INSTANT turns it
//! off. Pages scroll in Chromium, which has its own smooth scrolling
//! (BROWSER · SCROLL); trackpads send precise pixel deltas either way.

use crate::app::{App, Pane, TermPane};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Easing {
    Instant,
    Quadratic,
    #[default]
    Cubic,
    Quartic,
    Quintic,
    Circular,
    Sine,
}

impl Easing {
    pub const ALL: [Easing; 7] = [Easing::Instant, Easing::Quadratic, Easing::Cubic, Easing::Quartic, Easing::Quintic, Easing::Circular, Easing::Sine];

    pub fn name(self) -> &'static str {
        match self {
            Easing::Instant => "instant",
            Easing::Quadratic => "quadratic",
            Easing::Cubic => "cubic",
            Easing::Quartic => "quartic",
            Easing::Quintic => "quintic",
            Easing::Circular => "circular",
            Easing::Sine => "sine",
        }
    }

    /// neoscroll's easing functions, ease-out form: how far along the
    /// trip is at time fraction `x`.
    pub fn at(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Easing::Instant => 1.0,
            Easing::Quadratic => 1.0 - (1.0 - x) * (1.0 - x),
            Easing::Cubic => 1.0 - (1.0 - x).powi(3),
            Easing::Quartic => 1.0 - (1.0 - x).powi(4),
            Easing::Quintic => 1.0 - (1.0 - x).powi(5),
            Easing::Circular => (1.0 - (x - 1.0) * (x - 1.0)).max(0.0).sqrt(),
            Easing::Sine => (x * std::f32::consts::FRAC_PI_2).sin(),
        }
    }
}

/// One trip: from an offset to another, over `dur` seconds.
#[derive(Clone, Copy, Debug)]
pub struct Trip {
    pub from: f32,
    pub to: f32,
    pub start: Instant,
    pub dur: f32,
}

impl Trip {
    /// The offset now, on the curve.
    pub fn at(&self, easing: Easing) -> f32 {
        let t = if self.dur <= 0.0 { 1.0 } else { (crate::clock::since(self.start).as_secs_f32() / self.dur).min(1.0) };
        self.from + (self.to - self.from) * easing.at(t)
    }

    pub fn done(&self) -> bool {
        self.dur <= 0.0 || crate::clock::since(self.start).as_secs_f32() >= self.dur
    }
}

/// Ask the shell's view to move by `lines` (positive = into history),
/// eased; a trip in flight extends from where it is.
pub fn scroll_shell(t: &mut TermPane, lines: f32, easing: Easing, motion: &crate::anim::Motion) {
    let max = t.term.grid().scrollback_len() as f32;
    let now_off = t.trip.map(|tr| tr.at(easing)).unwrap_or(t.term.grid().display_offset as f32);
    let goal = t.trip.map(|tr| tr.to).unwrap_or(now_off);
    let to = (goal + lines).clamp(0.0, max);
    if easing == Easing::Instant || motion.reduced() {
        t.trip = None;
        let cur = t.term.grid().display_offset as isize;
        t.term.grid_mut().scroll_display(to.round() as isize - cur);
        return;
    }
    // Longer trips take longer, within reason: 90ms for a few lines,
    // ~300ms for a page, on the motion register.
    let dist = (to - now_off).abs();
    let base = (70.0 + dist * 7.0).clamp(90.0, 320.0);
    t.trip = Some(Trip { from: now_off, to, start: crate::clock::now(), dur: motion.dur(base).max(0.001) });
}

/// A surface of nus's own that scrolls in pixels: its offset is a plain
/// number the drawing reads, and a trip in `App::glides` moves that
/// number along the same curve the shell rides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glider {
    /// The settings page in the tab with this id.
    Settings(u64),
    /// The welcome page in the tab with this id.
    Welcome(u64),
    /// The reader over the page in this tab (right pane when true).
    Reader(u64, bool),
    /// The downloads page in this tab.
    Downloads(u64),
    Sidebar,
    Palette,
    Look,
    DownloadsMenu,
    Tree,
}

impl App {
    /// Logical px one wheel notch moves a page or a list, in physical px.
    pub(crate) fn wheel_step(&self) -> f32 {
        self.behavior.wheel_px.clamp(20, 400) as f32 * self.scale
    }

    /// Ask a surface's offset (`at`, as drawn) to move by `delta` px within
    /// 0..=max. Returns the offset to write now: the goal itself when the
    /// motion is instant, else `at` — the trip carries it from here and
    /// `tend_glides` writes each frame. A trip in flight extends from
    /// where it was going, so quick ticks add up instead of restarting.
    pub(crate) fn glide(&mut self, who: Glider, at: f32, delta: f32, max: f32) -> f32 {
        let max = max.max(0.0);
        let easing = self.behavior.scroll_easing;
        let from = self.glides.get(&who).map(|t| t.at(easing)).unwrap_or(at);
        let goal = self.glides.get(&who).map(|t| t.to).unwrap_or(from);
        let to = (goal + delta).clamp(0.0, max);
        if easing == Easing::Instant || self.motion.reduced() {
            self.glides.remove(&who);
            return to;
        }
        let dist = (to - from).abs();
        if dist < 0.5 {
            self.glides.remove(&who);
            return to;
        }
        // Longer trips take longer, within reason: ~120ms for a notch,
        // ~320ms for a page, on the motion register.
        let base = (80.0 + dist / self.scale * 0.5).clamp(110.0, 320.0);
        self.glides.insert(who, Trip { from, to, start: crate::clock::now(), dur: self.motion.dur(base).max(0.001) });
        self.dirty = true;
        at
    }

    /// A surface that stopped existing, or was scrolled by hand: forget
    /// its trip so nothing writes over it.
    pub(crate) fn glide_stop(&mut self, who: Glider) {
        self.glides.remove(&who);
    }

    /// Where a glider's offset lives, for the frame's write.
    fn glider_offset(&mut self, who: Glider) -> Option<&mut f32> {
        match who {
            Glider::Sidebar => Some(&mut self.sidebar_scroll),
            Glider::Palette => Some(&mut self.palette_scroll),
            Glider::Look => Some(&mut self.look_scroll),
            Glider::DownloadsMenu => Some(&mut self.download_ui.scroll),
            Glider::Tree => Some(&mut self.tree.scroll),
            Glider::Settings(id) => match self.tabs.iter_mut().find(|t| t.id == id).map(|t| &mut t.left) {
                Some(Pane::Settings(s)) => Some(&mut s.scroll),
                _ => None,
            },
            Glider::Welcome(id) => match self.tabs.iter_mut().find(|t| t.id == id).map(|t| &mut t.left) {
                Some(Pane::Hints(h)) => Some(&mut h.scroll),
                _ => None,
            },
            Glider::Downloads(id) => match self.tabs.iter_mut().find(|t| t.id == id).map(|t| &mut t.left) {
                Some(Pane::Downloads(d)) => Some(&mut d.scroll),
                _ => None,
            },
            Glider::Reader(id, right) => {
                let tab = self.tabs.iter_mut().find(|t| t.id == id)?;
                let pane = if right { tab.right.as_mut()? } else { &mut tab.left };
                match pane {
                    Pane::Web(w) => w.reader.as_mut().map(|r| &mut r.scroll),
                    _ => None,
                }
            }
        }
    }

    /// Once a frame: every surface's trip writes its offset.
    pub(crate) fn tend_glides(&mut self) {
        if self.glides.is_empty() {
            return;
        }
        let easing = self.behavior.scroll_easing;
        let trips: Vec<(Glider, Trip)> = self.glides.iter().map(|(k, v)| (*k, *v)).collect();
        let mut moving = false;
        for (who, trip) in trips {
            let v = trip.at(easing);
            match self.glider_offset(who) {
                Some(slot) => *slot = v,
                None => {
                    self.glides.remove(&who);
                    continue;
                }
            }
            if trip.done() {
                self.glides.remove(&who);
            } else {
                moving = true;
            }
        }
        if moving {
            self.dirty = true;
        }
    }

    /// Once a frame: every shell's trip advances its view a line at a time.
    pub(crate) fn tend_scrolling(&mut self) {
        self.tend_glides();
        let easing = self.behavior.scroll_easing;
        let mut moving = false;
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(t) = p else { continue };
                let Some(trip) = t.trip else { continue };
                let want = trip.at(easing).round() as isize;
                let cur = t.term.grid().display_offset as isize;
                if want != cur {
                    t.term.grid_mut().scroll_display(want - cur);
                }
                if trip.done() {
                    t.trip = None;
                } else {
                    moving = true;
                }
            }
        }
        if moving {
            self.dirty = true;
        }
    }

    /// BROWSER · SCROLLBARS for nus's own lists: the thumb's width and
    /// whether it stays up when still, or None when they are hidden.
    pub(crate) fn thumb_style(&self) -> Option<(f32, bool)> {
        match self.behavior.scrollbars {
            crate::settings::Scrollbars::Overlay => Some((self.px(2.0), false)),
            crate::settings::Scrollbars::Classic => Some((self.px(5.0), true)),
            crate::settings::Scrollbars::Hidden => None,
        }
    }

    /// A list taller than its window says so: a thumb at the window's
    /// right edge, dim. Overlay shows it while the pointer is over the list
    /// or it is moving; classic always, on a track; hidden never.
    /// `offset` is how far it is scrolled, `reach` how tall it is.
    pub(crate) fn draw_thumb(&mut self, scene: &mut nus_render::Scene, window: nus_render::Rect, offset: f32, reach: f32, moving: bool) {
        let max = (reach - window.h).max(0.0);
        let Some((w, always)) = self.thumb_style() else { return };
        if max <= 0.0 || window.h <= 0.0 || !(always || moving || window.contains(self.mouse.0, self.mouse.1)) {
            return;
        }
        let track = nus_render::Rect::new(window.right() - w - self.px(2.0), window.y + self.px(2.0), w, window.h - self.px(4.0));
        if always {
            scene.rect(track, crate::app::fade(self.theme.dim, 0.12));
        }
        let len = (track.h * window.h / reach).max(self.px(16.0)).min(track.h);
        let y = track.y + (track.h - len) * (offset / max).clamp(0.0, 1.0);
        scene.rect(nus_render::Rect::new(track.x, y, track.w, len), crate::app::fade(self.theme.dim, 0.6));
    }

    /// Whether a glider is on a trip right now.
    pub(crate) fn gliding(&self, who: Glider) -> bool {
        self.glides.contains_key(&who)
    }

    /// A page's wheel, in the units CEF's own client sends on this platform.
    /// Windows takes WHEEL_DELTA notches and applies the system's lines
    /// setting itself; macOS and Linux take pixels. Precise deltas keep
    /// their fractions here, so a slow trackpad still moves.
    pub(crate) fn page_wheel_units(&self, delta: winit::event::MouseScrollDelta, carry: &mut (f32, f32)) -> (i32, i32) {
        use winit::event::MouseScrollDelta;
        let step = self.behavior.wheel_px.clamp(20, 400) as f32;
        // Logical pixels the wheel asks for.
        let (px_x, px_y) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (x * step, y * step),
            MouseScrollDelta::PixelDelta(p) => (p.x as f32 / self.scale, p.y as f32 / self.scale),
        };
        // What to hand CEF so the page moves that far.
        let (ux, uy) = if cfg!(windows) {
            let lines = windows_wheel_lines();
            let per_px = 120.0 / (lines * 100.0 / 3.0);
            (px_x * per_px, px_y * per_px)
        } else {
            (px_x, px_y)
        };
        carry.0 += ux;
        carry.1 += uy;
        let (ix, iy) = (carry.0.trunc(), carry.1.trunc());
        carry.0 -= ix;
        carry.1 -= iy;
        (ix as i32, iy as i32)
    }
}

/// SPI_GETWHEELSCROLLLINES: what Chromium multiplies a notch by on
/// Windows. Read once; 3 when it is "a page" or unavailable.
#[cfg(windows)]
fn windows_wheel_lines() -> f32 {
    use std::sync::OnceLock;
    static LINES: OnceLock<f32> = OnceLock::new();
    *LINES.get_or_init(|| {
        let mut lines: u32 = 3;
        let ok = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(windows_sys::Win32::UI::WindowsAndMessaging::SPI_GETWHEELSCROLLLINES, 0, &mut lines as *mut u32 as *mut _, 0) };
        if ok == 0 || lines == 0 || lines == u32::MAX {
            3.0
        } else {
            lines.min(40) as f32
        }
    })
}
#[cfg(not(windows))]
fn windows_wheel_lines() -> f32 {
    3.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_start_at_zero_and_end_at_one() {
        for e in Easing::ALL {
            if e == Easing::Instant {
                continue;
            }
            assert!(e.at(0.0).abs() < 1e-6, "{e:?}");
            assert!((e.at(1.0) - 1.0).abs() < 1e-6, "{e:?}");
            assert!(e.at(0.5) > 0.5, "ease-out is ahead of linear: {e:?}");
        }
    }
}
