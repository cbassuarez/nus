//! One clock, so a recording is exact.
//!
//! Everything in nus that animates — eases, caret springs, scroll curves,
//! the plate's band, the palette, the profile card's rise, every chip's
//! fade — reads `clock::now()`. Normally that is `Instant::now()` and
//! nothing is different. Inside a recording (`shot.rs`) it is a virtual
//! clock that advances exactly 1/60 s per written frame, however long the
//! machine actually took to draw it. So the film's beat table lands on
//! the same frame every take, on any Mac, debug or release.
//!
//! Held (`await-paint`, `await-lsp`) the clock simply stops: no frames are
//! written and no animation moves while the app waits for Chromium or a
//! language server, which are not on this clock and never will be.
//!
//! Real elapsed time — a latency measurement, a network timeout, the FPS
//! log — asks `std::time::Instant::now()` directly and is left alone.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static VIRTUAL: AtomicBool = AtomicBool::new(false);
/// Nanoseconds of virtual time since `BASE`. Only ever goes up, so a
/// second recording in one run carries on where the first left off and no
/// animation ever sees the clock run backwards.
static NANOS: AtomicU64 = AtomicU64::new(0);
static BASE: OnceLock<Instant> = OnceLock::new();

/// A frame of the recorder's fixed 60 fps.
pub const FRAME: Duration = Duration::from_nanos(16_666_667);

/// The time the UI runs on.
#[inline]
pub fn now() -> Instant {
    if VIRTUAL.load(Relaxed) {
        if let Some(b) = BASE.get() {
            return *b + Duration::from_nanos(NANOS.load(Relaxed));
        }
    }
    Instant::now()
}

/// Whether the clock is the recorder's.
pub fn recording() -> bool {
    VIRTUAL.load(Relaxed)
}

/// Take over: from here the clock only moves when a frame is written.
pub fn start() {
    // An hour of headroom, so anything that dates an event a little
    // before now (a journal entry, a hover's `since`) still has an Instant.
    let _ = BASE.set(Instant::now() - Duration::from_secs(3600));
    NANOS.fetch_max(Duration::from_secs(3600).as_nanos() as u64, Relaxed);
    VIRTUAL.store(true, Relaxed);
}

/// Hand the clock back to the machine.
pub fn stop() {
    VIRTUAL.store(false, Relaxed);
}

/// One frame of virtual time.
pub fn tick() {
    NANOS.fetch_add(FRAME.as_nanos() as u64, Relaxed);
}

/// The virtual clock's reading, in nanoseconds. A recording keeps its own
/// zero and counts from this.
pub fn nanos() -> u64 {
    NANOS.load(Relaxed)
}

/// How long since `t` on this clock. `Instant::elapsed()` always asks the
/// machine, which a recording must not, so the app asks here instead.
pub fn since<T: At>(t: T) -> Duration {
    now().saturating_duration_since(t.at())
}

/// An instant, however it is held — the call sites have it by value, by
/// reference, and out of a tuple in a closure.
pub trait At {
    fn at(&self) -> Instant;
}

impl At for Instant {
    fn at(&self) -> Instant {
        *self
    }
}

impl<T: At + ?Sized> At for &T {
    fn at(&self) -> Instant {
        (**self).at()
    }
}

impl<T: At + ?Sized> At for &mut T {
    fn at(&self) -> Instant {
        (**self).at()
    }
}
