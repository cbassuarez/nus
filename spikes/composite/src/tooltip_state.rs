//! Window-local tooltip lifetime. Independent of visual hover animations and of
//! the renderer. Inputs and time are explicit so tests never sleep or need a GPU.
use std::time::{Duration, Instant};

pub const DWELL: Duration = Duration::from_millis(500);
pub const FADE: Duration = Duration::from_millis(120);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub key: u64,
    pub bounds: [f32; 4],
}
impl Target {
    fn contains(self, p: (f32, f32)) -> bool {
        let [x, y, w, h] = self.bounds;
        self.bounds.iter().all(|v| v.is_finite()) && w > 0.0 && h > 0.0
            && p.0.is_finite() && p.1.is_finite()
            && p.0 >= x && p.0 < x + w && p.1 >= y && p.1 < y + h
    }
}
#[derive(Clone, Copy, Debug)]
struct Visit { target: Target, since: Instant }

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub alpha: f32,
    pub needs_frame: bool,
}
#[derive(Default, Debug)]
pub struct State {
    pointer: Option<(f32, f32)>,
    current: Option<Visit>,
    dismissed: Option<Target>,
    wait_for_motion: bool,
    visible: bool,
}
impl State {
    pub fn visible(&self) -> bool { self.visible }

    pub fn motion(&mut self, pointer: (f32, f32)) {
        if self.pointer == Some(pointer) { return; }
        self.pointer = Some(pointer);
        self.wait_for_motion = false;
        if self.current.is_some_and(|v| !v.target.contains(pointer)) {
            self.current = None;
            self.visible = false;
        }
        // Only real movement out of the old anchor releases a same-owner latch.
        // A missing candidate caused by a menu covering it does not count.
        if self.dismissed.is_some_and(|target| !target.contains(pointer)) { self.dismissed = None; }
    }

    pub fn dismiss(&mut self) {
        if let Some(visit) = self.current.take() { self.dismissed = Some(visit.target); }
        self.wait_for_motion = true;
        self.visible = false;
    }

    pub fn leave(&mut self) {
        *self = Self::default();
    }

    pub fn frame(&mut self, candidate: Option<Target>, pointer: (f32, f32), now: Instant,
        blocked: bool, reduced_motion: bool) -> Frame {
        self.motion(pointer);
        let candidate = candidate.filter(|target| target.contains(pointer));
        if blocked || candidate.is_none() {
            self.current = None;
            self.visible = false;
            return Frame::default();
        }
        let target = candidate.unwrap();
        if self.wait_for_motion || self.dismissed.is_some_and(|old| old.key == target.key) {
            self.current = None;
            self.visible = false;
            return Frame::default();
        }
        if self.current.is_none_or(|v| v.target != target) {
            self.current = Some(Visit { target, since: now });
        }
        let age = now.saturating_duration_since(self.current.unwrap().since);
        if age < DWELL {
            self.visible = false;
            return Frame { alpha: 0.0, needs_frame: true };
        }
        let alpha = if reduced_motion { 1.0 }
            else { (age.saturating_sub(DWELL).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0) };
        self.visible = alpha > 0.0;
        Frame { alpha, needs_frame: alpha < 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(key: u64) -> Target { Target { key, bounds: [10.0, 10.0, 20.0, 20.0] } }
    fn tick(s: &mut State, key: Option<u64>, time: Instant) -> Frame {
        s.frame(key.map(target), (20.0, 20.0), time, false, false)
    }
    #[test]
    fn dwell_fades_then_stops_requesting_frames() {
        let now = Instant::now(); let mut s = State::default();
        assert_eq!(tick(&mut s, Some(1), now), Frame { alpha: 0.0, needs_frame: true });
        assert_eq!(tick(&mut s, Some(1), now + DWELL - Duration::from_millis(1)).alpha, 0.0);
        let midway = tick(&mut s, Some(1), now + DWELL + FADE / 2);
        assert!(midway.alpha > 0.0 && midway.alpha < 1.0 && midway.needs_frame);
        assert_eq!(tick(&mut s, Some(1), now + DWELL + FADE), Frame { alpha: 1.0, needs_frame: false });
    }
    #[test]
    fn leaving_before_dwell_cancels_the_old_timer() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now); s.motion((50.0, 50.0));
        assert_eq!(s.frame(None, (50.0, 50.0), now + DWELL, false, false), Frame::default());
        assert_eq!(tick(&mut s, Some(1), now + DWELL + FADE).alpha, 0.0);
    }
    #[test]
    fn click_or_escape_dismisses_both_pending_and_visible_help() {
        for visible in [false, true] {
            let now = Instant::now(); let mut s = State::default();
            tick(&mut s, Some(1), now);
            if visible { tick(&mut s, Some(1), now + DWELL + FADE); }
            s.dismiss();
            assert_eq!(tick(&mut s, Some(1), now + Duration::from_secs(5)), Frame::default());
            s.motion((21.0, 21.0));
            assert_eq!(s.frame(Some(target(1)), (21.0, 21.0), now + Duration::from_secs(6), false, false), Frame::default());
            s.motion((50.0, 50.0));
            assert_eq!(tick(&mut s, Some(1), now + Duration::from_secs(7)).alpha, 0.0);
        }
    }
    #[test]
    fn overlay_absence_does_not_release_a_dismissed_owner() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now); s.dismiss();
        s.frame(None, (20.0, 20.0), now + DWELL, true, false);
        assert_eq!(tick(&mut s, Some(1), now + Duration::from_secs(2)), Frame::default());
    }
    #[test]
    fn overlays_stop_the_timer_and_require_a_fresh_dwell_afterwards() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now);
        assert_eq!(s.frame(Some(target(1)), (20.0, 20.0), now + DWELL, true, false), Frame::default());
        assert_eq!(tick(&mut s, Some(1), now + Duration::from_secs(2)).alpha, 0.0);
    }
    #[test]
    fn replacement_or_geometry_change_cannot_inherit_the_old_dwell() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now); tick(&mut s, Some(1), now + DWELL + FADE);
        assert_eq!(tick(&mut s, Some(2), now + Duration::from_secs(1)).alpha, 0.0);
        let moved = Target { bounds: [11.0, 10.0, 20.0, 20.0], ..target(2) };
        assert_eq!(s.frame(Some(moved), (20.0, 20.0), now + Duration::from_secs(2), false, false).alpha, 0.0);
    }
    #[test]
    fn removed_owner_erases_visible_state_without_a_new_timer() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now); tick(&mut s, Some(1), now + DWELL + FADE);
        assert!(s.visible());
        assert_eq!(tick(&mut s, None, now + Duration::from_secs(1)), Frame::default());
        assert!(!s.visible());
    }
    #[test]
    fn reduced_motion_still_has_dwell_but_no_fade() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now);
        assert_eq!(s.frame(Some(target(1)), (20.0, 20.0), now + DWELL, false, true), Frame { alpha: 1.0, needs_frame: false });
    }
    #[test]
    fn window_leave_and_invalid_geometry_cannot_show_help() {
        let now = Instant::now(); let mut s = State::default();
        tick(&mut s, Some(1), now); s.leave();
        assert_eq!(tick(&mut s, Some(1), now + Duration::from_secs(1)).alpha, 0.0);
        for bounds in [[f32::NAN, 1.0, 1.0, 1.0], [10.0, 10.0, 0.0, 10.0], [100.0, 100.0, 2.0, 2.0]] {
            assert_eq!(s.frame(Some(Target { key: 1, bounds }), (20.0, 20.0), now + Duration::from_secs(2), false, false), Frame::default());
        }
    }
    #[test]
    fn dismissal_before_first_paint_cannot_create_help_under_a_stationary_pointer() {
        let now = Instant::now(); let mut s = State::default();
        s.motion((20.0, 20.0)); s.dismiss();
        assert_eq!(tick(&mut s, Some(1), now), Frame::default());
        s.motion((21.0, 20.0));
        assert_eq!(s.frame(Some(target(1)), (21.0, 20.0), now, false, false).alpha, 0.0);
    }
}
