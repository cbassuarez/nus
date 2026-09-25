//! Motion as ink: one easing, a handful of durations, a global register
//! from snappy to cinematic. Nothing scales or bounces; things slide and
//! rules extend. Frames only run while something is moving.

use std::time::Instant;

/// A value easing from `from` to `to` over `dur` seconds (ease-out cubic,
/// or a glide for things that travel).
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    from: f32,
    to: f32,
    start: Instant,
    dur: f32,
    curve: Curve,
}

/// How a value covers its distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Curve {
    /// Leaves at full speed: fades, tints, small nudges.
    #[default]
    Out,
    /// cubic-bezier(0.2, 0, 0, 1): starts from rest, so a late first frame
    /// never shows as a jump, then settles with a long, soft landing.
    /// For panels that cross the window.
    Glide,
}

impl Curve {
    fn apply(self, t: f32) -> f32 {
        match self {
            Curve::Out => 1.0 - (1.0 - t).powi(3),
            Curve::Glide => bezier(0.2, 0.0, 0.0, 1.0, t),
        }
    }
}

/// CSS cubic-bezier(x1, y1, x2, y2) at progress `t`: solve x for the
/// curve parameter (Newton, then bisection), return y.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let at = |a: f32, b: f32, u: f32| 3.0 * a * u * (1.0 - u).powi(2) + 3.0 * b * u * u * (1.0 - u) + u * u * u;
    let slope = |a: f32, b: f32, u: f32| 3.0 * a * (1.0 - u).powi(2) + 6.0 * (b - a) * u * (1.0 - u) + 3.0 * (1.0 - b) * u * u;
    let mut u = t;
    for _ in 0..6 {
        let d = slope(x1, x2, u);
        if d.abs() < 1e-5 {
            break;
        }
        u = (u - (at(x1, x2, u) - t) / d).clamp(0.0, 1.0);
    }
    if (at(x1, x2, u) - t).abs() > 1e-4 {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            u = (lo + hi) * 0.5;
            if at(x1, x2, u) < t { lo = u } else { hi = u }
        }
    }
    at(y1, y2, u)
}

impl Anim {
    /// Already at `v`.
    pub fn at(v: f32) -> Anim {
        Anim { from: v, to: v, start: crate::clock::now(), dur: 0.0, curve: Curve::Out }
    }

    /// Already at `v`, travelling on `curve` from here on.
    pub fn at_on(v: f32, curve: Curve) -> Anim {
        Anim { curve, ..Anim::at(v) }
    }

    /// Retarget from wherever the value is now. A zero duration snaps.
    pub fn go(&mut self, to: f32, dur: f32) {
        if (self.to - to).abs() < 1e-4 {
            return;
        }
        let now = self.value();
        *self = Anim { from: now, to, start: crate::clock::now(), dur, curve: self.curve };
    }

    /// Restart from `from` toward `to` (for fades that always replay).
    pub fn replay(&mut self, from: f32, to: f32, dur: f32) {
        *self = Anim { from, to, start: crate::clock::now(), dur, curve: self.curve };
    }

    pub fn value(&self) -> f32 {
        if self.dur <= 0.0 {
            return self.to;
        }
        let t = (crate::clock::since(self.start).as_secs_f32() / self.dur).clamp(0.0, 1.0);
        let e = self.curve.apply(t);
        self.from + (self.to - self.from) * e
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn active(&self) -> bool {
        self.dur > 0.0 && crate::clock::since(self.start).as_secs_f32() < self.dur
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

    /// Seconds for something that travels across the window. The snappy
    /// end keeps enough frames to read as motion (a 60 ms slide is four
    /// frames: a stutter, not a glide); the cinematic end stays under half
    /// a second so it never drags.
    pub fn travel(&self, base_ms: f32) -> f32 {
        if self.reduced() {
            return 0.0;
        }
        let k = 0.75 + (1.9 - 0.75) * self.register.clamp(0.0, 1.0);
        (base_ms / 1000.0 * k).clamp(0.16, 0.44)
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
    /// Travel base for Motion::travel: 0.16 s snappy … 0.40 s cinematic.
    pub const SIDEBAR: f32 = 210.0;
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

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn os_reduce_motion() -> bool {
    false
}

/// AppKit exposes the same Reduce Motion preference as System Settings.
#[cfg(target_os = "macos")]
pub fn os_reduce_motion() -> bool {
    use std::ffi::{c_char, c_void};
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    // SAFETY: these two parameterless Objective-C methods are declared by
    // AppKit's NSWorkspace and NSAccessibility headers. Their return ABIs
    // are object pointer and BOOL respectively; no retained object escapes.
    unsafe {
        let class = objc_getClass(c"NSWorkspace".as_ptr());
        if class.is_null() { return false; }
        let get: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as *const ());
        let flag: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool = std::mem::transmute(objc_msgSend as *const ());
        let workspace = get(class, sel_registerName(c"sharedWorkspace".as_ptr()));
        !workspace.is_null() && flag(workspace, sel_registerName(c"accessibilityDisplayShouldReduceMotion".as_ptr()))
    }
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
        let now = crate::clock::now();
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
    /// Continuous material with an extended-range highlight at its leading edge.
    Radiance,
    /// A rule growing from the left.
    Rule,
    /// A bright head with a fading tail.
    Comet,
    /// The whole carapace band fills across the window.
    Carapace,
}

impl BarStyle {
    pub const ALL: [BarStyle; 4] = [BarStyle::Radiance, BarStyle::Rule, BarStyle::Comet, BarStyle::Carapace];
    pub fn name(self) -> &'static str {
        match self {
            BarStyle::Radiance => "radiance",
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
        LoadBar { style: BarStyle::Radiance, color: BarColor::Signal, thickness: 2.0, chase: 6.0 }
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

    #[test]
    fn glide_starts_from_rest_and_lands() {
        assert_eq!(Curve::Glide.apply(0.0), 0.0);
        assert!((Curve::Glide.apply(1.0) - 1.0).abs() < 1e-4);
        // Gentle first frame, most of the distance by the middle, monotone.
        assert!(Curve::Glide.apply(0.05) < 0.05);
        assert!(Curve::Glide.apply(0.5) > 0.75);
        let mut last = 0.0;
        for i in 1..=100 {
            let v = Curve::Glide.apply(i as f32 / 100.0);
            assert!(v >= last - 1e-5);
            last = v;
        }
    }

    #[test]
    fn travel_keeps_frames_at_both_ends() {
        let snappy = Motion { register: 0.0, reduce: Some(false) };
        let cinematic = Motion { register: 1.0, reduce: Some(false) };
        assert!(snappy.travel(base::SIDEBAR) >= 0.16);
        assert!(cinematic.travel(base::SIDEBAR) <= 0.44);
        assert_eq!(Motion { register: 1.0, reduce: Some(true) }.travel(base::SIDEBAR), 0.0);
    }
}
