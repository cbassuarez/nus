//! Pure startup/window policy. No native handles, OS writes, or browser calls.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.w, self.h, self.x + self.w, self.y + self.h]
            .iter().all(|v| v.is_finite()) && self.w > 0.0 && self.h > 0.0
    }
    pub fn contains(self, x: f64, y: f64) -> bool {
        self.valid() && x >= self.x && y >= self.y
            && x < self.x + self.w && y < self.y + self.h
    }
}

/// Work area and result use the same native coordinate system. The caller
/// converts the preferred logical dimensions once, on the selected monitor.
pub fn centered(work: Area, preferred: (f64, f64)) -> Option<Area> {
    if !work.valid() || !preferred.0.is_finite() || !preferred.1.is_finite()
        || preferred.0 <= 0.0 || preferred.1 <= 0.0 { return None; }
    let w = preferred.0.min(work.w * 0.9).max(1.0).min(work.w);
    let h = preferred.1.min(work.h * 0.9).max(1.0).min(work.h);
    Some(Area { x: work.x + (work.w - w) * 0.5,
        y: work.y + (work.h - h) * 0.5, w, h })
}

/// Restore only when a usable segment of the caption is reachable. A valid
/// multi-monitor window is not pulled onto one screen merely for spanning it.
pub fn caption_reachable(saved: Area, work: &[Area], caption: f64) -> bool {
    if !saved.valid() || !caption.is_finite() || caption <= 0.0 { return false; }
    let need = 100.0_f64.min(saved.w);
    work.iter().copied().filter(|a| a.valid()).any(|a| {
        let width = (saved.x + saved.w).min(a.x + a.w) - saved.x.max(a.x);
        width >= need && saved.y >= a.y && saved.y + caption.min(saved.h) <= a.y + a.h
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handler { ThisInstall, Other, Missing, Unknown }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultState { Default, Partial, NotDefault, Unknown }

pub fn default_state(http: Handler, https: Handler) -> DefaultState {
    match (http, https) {
        (Handler::ThisInstall, Handler::ThisInstall) => DefaultState::Default,
        (Handler::Unknown, _) | (_, Handler::Unknown) => DefaultState::Unknown,
        (Handler::ThisInstall, _) | (_, Handler::ThisInstall) => DefaultState::Partial,
        _ => DefaultState::NotDefault,
    }
}

/// A bounded, process-local request serial. Old worker completions cannot
/// overwrite a newer observation, and repeated consent clicks coalesce.
#[derive(Default, Debug)]
pub struct Requests { serial: u64, pub busy: bool }
impl Requests {
    pub fn begin(&mut self) -> Option<u64> {
        if self.busy { return None; }
        self.serial = self.serial.checked_add(1)?;
        self.busy = true;
        Some(self.serial)
    }
    pub fn complete(&mut self, serial: u64) -> bool {
        if !self.busy || serial != self.serial { return false; }
        self.busy = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn first_window_fits_small_work_area() {
        let w = Area { x: -1366.0, y: 24.0, w: 1366.0, h: 720.0 };
        let r = centered(w, (1440.0,900.0)).unwrap();
        assert!(r.x >= w.x && r.y >= w.y);
        assert!(r.x+r.w <= w.x+w.w && r.y+r.h <= w.y+w.h);
        assert!((r.x + r.w/2.0 - (w.x + w.w/2.0)).abs() < 1e-8);
    }
    #[test] fn invalid_geometry_is_rejected() {
        for n in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert!(centered(Area{x:0.0,y:0.0,w:n,h:700.0},(1440.0,900.0)).is_none());
        }
    }
    #[test] fn restore_does_not_enforce_first_launch_size() {
        let w = Area{x:0.0,y:24.0,w:1920.0,h:1056.0};
        assert!(caption_reachable(Area{x:10.0,y:30.0,w:1890.0,h:1000.0}, &[w],32.0));
        assert!(!caption_reachable(Area{x:-4000.0,y:30.0,w:1200.0,h:800.0}, &[w],32.0));
    }
    #[test] fn defaults_are_os_observations_not_dispatch_results() {
        assert_eq!(default_state(Handler::ThisInstall,Handler::ThisInstall),DefaultState::Default);
        assert_eq!(default_state(Handler::ThisInstall,Handler::Other),DefaultState::Partial);
        assert_eq!(default_state(Handler::ThisInstall,Handler::Unknown),DefaultState::Unknown);
        assert_eq!(default_state(Handler::Missing,Handler::Other),DefaultState::NotDefault);
    }
    #[test] fn consent_clicks_coalesce_and_stale_results_are_ignored() {
        let mut q=Requests::default(); let a=q.begin().unwrap();
        assert_eq!(q.begin(),None); assert!(!q.complete(a+1)); assert!(q.complete(a));
        let b=q.begin().unwrap(); assert!(!q.complete(a)); assert!(q.complete(b));
    }
    #[test] fn deterministic_placement_trials() {
        for i in 1..=1000 {
            let w=Area{x:-(i as f64),y:i as f64/3.0,w:200.0+(i%700)as f64,h:120.0+(i%800)as f64};
            let r=centered(w,(1440.0,900.0)).unwrap();
            assert!(r.valid() && r.x>=w.x && r.y>=w.y);
            assert!(r.x+r.w <= w.x+w.w+1e-8 && r.y+r.h<=w.y+w.h+1e-8);
        }
    }
}
