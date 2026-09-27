//! Click counts for pages. Chromium picks a word on a double click and a
//! paragraph on a triple only when it's told the count; every press and
//! release carries it. A press counts on when it's the same button, within
//! the system's double-click time and a few pixels of the last; a release
//! carries its press's count.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default)]
pub struct Counter {
    last: Option<(Instant, f32, f32, u8, i32)>,
}

impl Counter {
    /// The count to send for this press (or release) of `button` at (x, y),
    /// in physical pixels; `slop` is how far a second click may land.
    pub fn count(&mut self, button: u8, pressed: bool, x: f32, y: f32, slop: f32, interval: Duration) -> i32 {
        self.count_at(Instant::now(), button, pressed, x, y, slop, interval)
    }

    #[allow(clippy::too_many_arguments)]
    fn count_at(&mut self, now: Instant, button: u8, pressed: bool, x: f32, y: f32, slop: f32, interval: Duration) -> i32 {
        if !pressed {
            return self.last.filter(|l| l.3 == button).map_or(1, |l| l.4);
        }
        let n = match self.last {
            Some((at, lx, ly, b, n)) if b == button && now.duration_since(at) <= interval && (x - lx).abs() <= slop && (y - ly).abs() <= slop => (n % 3) + 1,
            _ => 1,
        };
        self.last = Some((now, x, y, button, n));
        n
    }
}

/// winit's button as a small number, for the counter.
pub fn id(b: winit::event::MouseButton) -> u8 {
    match b {
        winit::event::MouseButton::Left => 0,
        winit::event::MouseButton::Right => 1,
        winit::event::MouseButton::Middle => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_two_three_then_again() {
        let mut c = Counter::default();
        let t = Instant::now();
        let iv = Duration::from_millis(500);
        let ms = |n| t + Duration::from_millis(n);
        assert_eq!(c.count_at(ms(0), 0, true, 10.0, 10.0, 4.0, iv), 1);
        assert_eq!(c.count_at(ms(50), 0, false, 10.0, 10.0, 4.0, iv), 1);
        assert_eq!(c.count_at(ms(200), 0, true, 11.0, 12.0, 4.0, iv), 2);
        assert_eq!(c.count_at(ms(250), 0, false, 11.0, 12.0, 4.0, iv), 2);
        assert_eq!(c.count_at(ms(400), 0, true, 11.0, 12.0, 4.0, iv), 3);
        assert_eq!(c.count_at(ms(600), 0, true, 11.0, 12.0, 4.0, iv), 1);
        // Too far, too late, another button: back to one.
        assert_eq!(c.count_at(ms(700), 0, true, 30.0, 12.0, 4.0, iv), 1);
        assert_eq!(c.count_at(ms(2000), 0, true, 30.0, 12.0, 4.0, iv), 1);
        assert_eq!(c.count_at(ms(2100), 1, true, 30.0, 12.0, 4.0, iv), 1);
    }
}
