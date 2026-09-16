//! Motion as ink: one easing, a handful of durations, a global register
//! from snappy to cinematic. Nothing scales or bounces; things slide and
//! rules extend. Frames only run while something is moving.

use std::time::Instant;

/// A value easing from `from` to `to` over `dur` seconds (ease-out cubic).
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    dur: f32,
}

impl Anim {
    /// Already at `v`.
    pub fn at(v: f32) -> Anim {
        Anim { from: v, to: v, start: Instant::now(), dur: 0.0 }
    }

    /// Retarget from wherever the value is now. A zero duration snaps.
    pub fn go(&mut self, to: f32, dur: f32) {
        if (self.to - to).abs() < 1e-4 {
            return;
        }
        let now = self.value();
        *self = Anim { from: now, to, start: Instant::now(), dur };
    }

    /// Restart from `from` toward `to` (for fades that always replay).
    pub fn replay(&mut self, from: f32, to: f32, dur: f32) {
        *self = Anim { from, to, start: Instant::now(), dur };
    }

    pub fn value(&self) -> f32 {
        if self.dur <= 0.0 {
            return self.to;
        }
        let t = (self.start.elapsed().as_secs_f32() / self.dur).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * e
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn active(&self) -> bool {
        self.dur > 0.0 && self.start.elapsed().as_secs_f32() < self.dur
    }
}

/// The register: 0 = snappy, 1 = cinematic. Durations scale 0.45× … 2.2×.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Motion {
    pub register: f32,
    /// Reduce-motion: None follows the OS, Some forces it.
    pub reduce: Option<bool>,
}

impl Default for Motion {
    fn default() -> Self {
        Motion { register: 0.5, reduce: None }
    }
}

impl Motion {
    pub fn reduced(&self) -> bool {
        self.reduce.unwrap_or_else(os_reduce_motion)
    }

    /// Seconds for a `base_ms` animation under this register.
    pub fn dur(&self, base_ms: f32) -> f32 {
        if self.reduced() {
            return 0.0;
        }
        let k = 0.45 + (2.2 - 0.45) * self.register.clamp(0.0, 1.0);
        base_ms / 1000.0 * k
    }

    pub fn name(&self) -> &'static str {
        match self.register {
            r if r < 0.2 => "snappy",
            r if r < 0.4 => "quick",
            r if r < 0.6 => "measured",
            r if r < 0.8 => "unhurried",
            _ => "cinematic",
        }
    }
}

/// Base durations (ms) before the register scales them.
pub mod base {
    pub const SIDEBAR: f32 = 140.0;
    pub const ROW: f32 = 140.0;
    pub const PALETTE: f32 = 120.0;
    pub const BAND: f32 = 100.0;
    pub const TINT: f32 = 80.0;
    pub const CRUMB: f32 = 120.0;
    pub const LOAD_OUT: f32 = 220.0;
}

/// The OS reduce-motion preference (Windows: client-area animation off).
#[cfg(target_os = "windows")]
pub fn os_reduce_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION};
    let mut on: i32 = 1;
    // SAFETY: plain SPI query into a BOOL-sized local.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on as *mut i32 as *mut _, 0) };
    ok != 0 && on == 0
}

#[cfg(not(target_os = "windows"))]
pub fn os_reduce_motion() -> bool {
    false
}

/// A value that chases a target smoothly (loading bars). `rate` is the
/// fraction of the gap closed per second, so it is frame-rate independent.
#[derive(Clone, Copy, Debug)]
pub struct Follow {
    pub value: f32,
    pub target: f32,
    last: Option<Instant>,
}

impl Follow {
    pub fn new(v: f32) -> Follow {
        Follow { value: v, target: v, last: None }
    }

    /// Advance toward the target; returns true while still moving.
    pub fn step(&mut self, rate: f32) -> bool {
        let now = Instant::now();
        let dt = self.last.map(|l| (now - l).as_secs_f32()).unwrap_or(1.0 / 60.0).min(0.1);
        self.last = Some(now);
        self.advance(rate, dt)
    }

    pub fn advance(&mut self, rate: f32, dt: f32) -> bool {
        let gap = self.target - self.value;
        if gap.abs() < 0.0005 {
            self.value = self.target;
            return false;
        }
        self.value += gap * (1.0 - (-rate * dt).exp());
        true
    }
}

/// How the loading bar looks; edited in settings → BROWSER.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum BarStyle {
    /// A rule growing from the left.
    Rule,
    /// A bright head with a fading tail.
    Comet,
    /// The whole carapace band fills across the window.
    Carapace,
}

impl BarStyle {
    pub const ALL: [BarStyle; 3] = [BarStyle::Rule, BarStyle::Comet, BarStyle::Carapace];
    pub fn name(self) -> &'static str {
        match self {
            BarStyle::Rule => "rule",
            BarStyle::Comet => "comet",
            BarStyle::Carapace => "carapace",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum BarColor {
    Signal,
    Tab,
    Ink,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LoadBar {
    pub style: BarStyle,
    pub color: BarColor,
    /// Logical px.
    pub thickness: f32,
    /// How fast the bar chases real progress (per second).
    pub chase: f32,
}

impl Default for LoadBar {
    fn default() -> Self {
        LoadBar { style: BarStyle::Comet, color: BarColor::Signal, thickness: 2.0, chase: 6.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anim_reaches_target() {
        let mut a = Anim::at(0.0);
        a.go(1.0, 0.0);
        assert_eq!(a.value(), 1.0);
        assert!(!a.active());
    }

    #[test]
    fn follow_converges() {
        let mut f = Follow::new(0.0);
        f.target = 1.0;
        for _ in 0..600 {
            f.advance(6.0, 1.0 / 60.0);
        }
        assert!(f.value > 0.99);
    }

    #[test]
    fn register_scales() {
        let m = Motion { register: 0.0, reduce: Some(false) };
        assert!((m.dur(100.0) - 0.045).abs() < 1e-4);
        let m = Motion { register: 1.0, reduce: Some(true) };
        assert_eq!(m.dur(100.0), 0.0);
    }
}
