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

/// Whether the complete frame is inside this display's usable desktop.
pub fn fully_inside(frame: Area, work: Area) -> bool {
    frame.valid() && work.valid() && frame.x >= work.x && frame.y >= work.y
        && frame.x + frame.w <= work.x + work.w && frame.y + frame.h <= work.y + work.h
}

/// Preserve a restored size and position where possible, fitting its whole
/// frame when a display shrank, disappeared, or a cascade reached an edge.
pub fn fitted(work: Area, wanted: Area) -> Option<Area> {
    if !work.valid() || !wanted.valid() { return None; }
    let w = wanted.w.min(work.w);
    let h = wanted.h.min(work.h);
    Some(Area { x: wanted.x.clamp(work.x, work.x + work.w - w),
        y: wanted.y.clamp(work.y, work.y + work.h - h), w, h })
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
    #[test] fn restores_fit_the_whole_frame_and_keep_valid_geometry() {
        let work = Area{x:0.0,y:24.0,w:1920.0,h:1056.0};
        let good = Area{x:10.0,y:30.0,w:1890.0,h:1000.0};
        assert_eq!(fitted(work,good),Some(good));
        for bad in [Area{x:1800.0,y:30.0,w:1200.0,h:800.0},
            Area{x:10.0,y:900.0,w:1200.0,h:800.0},
            Area{x:-4000.0,y:-2000.0,w:5000.0,h:4000.0}] {
            assert!(!fully_inside(bad,work));
            assert!(fully_inside(fitted(work,bad).unwrap(),work));
        }
    }
    #[test] fn negative_origins_and_mixed_scale_work_areas_fit() {
        for work in [Area{x:-2560.0,y:-1440.0,w:2560.0,h:1400.0},
            Area{x:3840.0,y:40.0,w:1920.0,h:1040.0},
            Area{x:-1280.0,y:22.0,w:1280.0,h:698.0}] {
            for i in 0..1000 {
                let wanted=Area{x:i as f64*30.0-8000.0,y:i as f64*10.0-2000.0,w:200.0+i as f64*3.0,h:120.0+i as f64*2.0};
                assert!(fully_inside(fitted(work,wanted).unwrap(),work));
            }
        }
        assert!(fitted(Area{x:0.0,y:0.0,w:0.0,h:700.0},Area{x:0.0,y:0.0,w:10.0,h:10.0}).is_none());
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
