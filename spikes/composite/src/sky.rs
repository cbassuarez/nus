//! The native sky's small set of cached views. Cloud geometry survives weather
//! refreshes; time stops while the prompt is being edited or the view is hidden.

use std::sync::Arc;
use std::time::{Duration, Instant};

use nus_render::sky::{SkyParams, SkyRenderer};
use nus_render::{Gpu, Rect};

const MAX_VIEWS: usize = 4;
const WEATHER_EASE_SECONDS: f32 = 45.0;
pub(crate) const TYPING_HOLD: Duration = Duration::from_millis(1800);
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 24);
const MAX_ACTIVE_GAP: Duration = Duration::from_millis(250);

#[derive(Default)]
struct ViewClock {
    last: Option<Instant>,
    time: f32,
    moving: bool,
}

impl ViewClock {
    fn advance(&mut self, now: Instant, moving: bool) -> f32 {
        // A dormant view must not catch up with another view's motion. Long
        // gaps also cover occlusion/suspension with no final paused draw.
        let dt = self.last
            .filter(|_| moving && self.moving)
            .map(|last| now.saturating_duration_since(last))
            .filter(|gap| *gap <= MAX_ACTIVE_GAP)
            .map(|gap| gap.as_secs_f32().min(0.1))
            .unwrap_or(0.0);
        self.last = Some(now);
        self.moving = moving;
        self.time += dt;
        dt
    }
}

impl crate::app::App {
    /// Match the native sky presentation rate on both mains and battery. A
    /// 30 Hz caller feeding a 24 Hz renderer would otherwise show only 15 Hz.
    pub(crate) fn request_sky_frame(&mut self) {
        if self.art_budget() == crate::power::Budget::Still || self.prompt_composing { return; }
        let at = if crate::clock::since(self.last_key) < TYPING_HOLD {
            self.last_key + TYPING_HOLD
        } else {
            self.art_frame_started + FRAME_INTERVAL
        };
        self.art_deadline = Some(self.art_deadline.map_or(at, |pending| pending.min(at)));
    }
}

struct View {
    key: (u64, usize),
    renderer: SkyRenderer,
    conditions: SkyParams,
    used: Instant,
    clock: ViewClock,
}

#[derive(Default)]
pub(crate) struct Skies {
    views: Vec<View>,
}

impl Skies {
    pub(crate) fn draw(&mut self, gpu: &Gpu, key: (u64, usize), rect: Rect, mut target: SkyParams, moving: bool, reduced: bool) -> Arc<wgpu::BindGroup> {
        let now = crate::clock::now();
        let index = if let Some(index) = self.views.iter().position(|v| v.key == key) { index } else {
            if self.views.len() == MAX_VIEWS {
                let oldest = self.views.iter().enumerate().min_by_key(|(_, v)| v.used).map(|(i, _)| i).unwrap();
                self.views.swap_remove(oldest);
            }
            self.views.push(View { key, renderer: SkyRenderer::new(gpu), conditions: target, used: now, clock: ViewClock::default() });
            self.views.len() - 1
        };
        let view = &mut self.views[index];
        let weather_dt = view.clock.advance(now, moving);
        target.time = view.clock.time;
        view.used = now;
        update_weather(&mut view.conditions, target, weather_dt, moving, reduced);
        // Direction, projection, and prompt geometry are current; only weather
        // evolves gradually. Changing a place must not put the Sun behind us
        // in front of the camera, or leave prompt ink on yesterday's lighting.
        let mut params = target;
        params.low_cover = view.conditions.low_cover;
        params.mid_cover = view.conditions.mid_cover;
        params.high_cover = view.conditions.high_cover;
        // An eclipse parts the modelled clouds for the show, at once rather than at the weather's pace.
        let clear = params.celestial.map_or(0.0, |c| c.clear_sky);
        if clear > 0.0 {
            params.low_cover = view.conditions.low_cover * (1.0 - 0.85 * clear);
            params.high_cover *= 1.0 - 0.85 * clear;
        }
        params.stratus = view.conditions.stratus;
        params.precipitation_mm_h = view.conditions.precipitation_mm_h;
        params.cloud_base_km = view.conditions.cloud_base_km;
        params.haze = view.conditions.haze;
        params.wind_low = view.conditions.wind_low;
        params.wind_mid = view.conditions.wind_mid;
        params.wind_high = view.conditions.wind_high;
        let before = crate::perf::enabled().then(|| view.renderer.stats());
        let output = view.renderer.render(gpu, (rect.w.ceil().max(1.0) as u32, rect.h.ceil().max(1.0) as u32), params);
        if let Some(before) = before {
            let after = view.renderer.stats();
            if after.present_draws > before.present_draws {
                crate::perf::interval("sky_present_interval");
                crate::perf::record("sky_present_cpu_submit", after.cpu_submit_ms);
            }
            if after.volume_draws > before.volume_draws {
                crate::perf::interval("sky_volume_interval");
            }
        }
        output
    }

    pub(crate) fn trim(&mut self) {
        let now = crate::clock::now();
        self.views.retain(|v| now.saturating_duration_since(v.used) < Duration::from_secs(60));
        if self.views.is_empty() { self.views.shrink_to_fit(); }
    }
}

fn update_weather(current: &mut SkyParams, target: SkyParams, dt: f32, moving: bool, reduced: bool) {
    // Typing/holding keeps the cached appearance. Reduced motion accepts a
    // newly available forecast in one frame instead of running a transition.
    if reduced { *current = target; }
    else if moving { ease_weather(current, target, dt); }
}

fn ease_weather(current: &mut SkyParams, target: SkyParams, dt: f32) {
    let k = 1.0 - (-dt / WEATHER_EASE_SECONDS).exp();
    fn toward(v: &mut f32, target: f32, k: f32) {
        *v += (target - *v) * k;
        if (*v - target).abs() < 0.00001 { *v = target; }
    }
    for (v, target) in [
        (&mut current.low_cover, target.low_cover), (&mut current.mid_cover, target.mid_cover),
        (&mut current.high_cover, target.high_cover), (&mut current.stratus, target.stratus),
        (&mut current.precipitation_mm_h, target.precipitation_mm_h),
        (&mut current.cloud_base_km, target.cloud_base_km), (&mut current.haze, target.haze),
    ] { toward(v, target, k); }
    for i in 0..2 {
        toward(&mut current.wind_low[i], target.wind_low[i], k);
        toward(&mut current.wind_mid[i], target.wind_mid[i], k);
        toward(&mut current.wind_high[i], target.wind_high[i], k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_clock_is_view_local_and_does_not_catch_up_hidden_time() {
        let now = Instant::now();
        let mut a = ViewClock::default();
        let mut b = ViewClock::default();
        a.advance(now, true);
        b.advance(now, true);
        a.advance(now + Duration::from_millis(40), true);
        let held = a.time;
        for frame in 1..=50 { b.advance(now + Duration::from_millis(frame * 40), true); }
        assert!(b.time > 1.9);
        assert_eq!(a.advance(now + Duration::from_secs(2), true), 0.0);
        assert_eq!(a.time, held);
        a.advance(now + Duration::from_millis(2040), true);
        assert!((a.time - held - 0.04).abs() < 0.00001);
    }

    #[test]
    fn sky_clock_clamps_active_stalls_and_resumes_without_paused_time() {
        let now = Instant::now();
        let mut clock = ViewClock::default();
        clock.advance(now, true);
        assert_eq!(clock.advance(now + Duration::from_millis(180), true), 0.1);
        clock.advance(now + Duration::from_millis(200), false);
        let held = clock.time;
        clock.advance(now + Duration::from_secs(3), false);
        assert_eq!(clock.advance(now + Duration::from_millis(3040), true), 0.0);
        assert_eq!(clock.time, held);
        assert!((clock.advance(now + Duration::from_millis(3080), true) - 0.04).abs() < 0.00001);
    }

    #[test]
    fn sky_weather_holds_while_typing_and_updates_without_motion_when_reduced() {
        let mut current = SkyParams { low_cover: 0.2, ..Default::default() };
        let target = SkyParams { low_cover: 0.9, ..current };
        update_weather(&mut current, target, 0.25, false, false);
        assert_eq!(current.low_cover, 0.2);
        update_weather(&mut current, target, 0.25, false, true);
        assert_eq!(current.low_cover, 0.9);
    }

    #[test]
    fn weather_refresh_preserves_cloud_identity_and_eases_conditions() {
        let mut current = SkyParams { low_cover: 0.2, seed: 42, ..Default::default() };
        let target = SkyParams { low_cover: 0.9, precipitation_mm_h: 4.0, wind_low: [-8.0, 4.0], ..current };
        ease_weather(&mut current, target, 0.1);
        assert!(current.low_cover > 0.2 && current.low_cover < 0.21);
        assert!(current.precipitation_mm_h > 0.0 && current.precipitation_mm_h < 0.02);
        assert_eq!(current.seed, 42);
        for _ in 0..4500 { ease_weather(&mut current, target, 0.1); }
        assert!((current.low_cover - 0.9).abs() < 0.001);
        assert!((current.wind_low[0] + 8.0).abs() < 0.001);
    }
}
