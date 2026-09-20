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

impl App {
    /// Once a frame: every shell's trip advances its view a line at a time.
    pub(crate) fn tend_scrolling(&mut self) {
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
