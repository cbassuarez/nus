//! A small activity envelope for the window's material. The caller supplies
//! only the focused pane's continuous activity; background panes may supply
//! an authoritative completion or a held request for attention.
//!
//! Counters describe events, not bytes or keystroke velocity. First samples,
//! pane changes and preference changes establish a baseline, so opening a
//! pane or enabling a source never replays its history. Work ending alone
//! does not imply success or completion.

use std::time::Instant;

use crate::surface::{ReactTo, Reaction};

#[derive(Clone, Copy, Debug, Default)]
pub struct ActivityInput {
    /// Stable identity of the focused pane, including the side of a split.
    pub identity: u64,
    pub working: bool,
    pub loading: bool,
    /// Reported progress only. None means unknown, not zero.
    pub progress: Option<f32>,
    /// Remains true until the underlying request is acknowledged.
    pub attention: bool,
    /// Monotonic event counters. A jump of any size is one capped burst.
    pub output: u64,
    pub typed: u64,
    pub completion: u64,
    pub media: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActivityFrame {
    /// The response strength, including the user's reaction setting.
    pub energy: f32,
    /// Turns, 0..1. Idle and held attention never keep this clock running.
    pub phase: f32,
    pub progress: Option<f32>,
    pub attention: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    reaction: Reaction,
    sources: [bool; 6],
    reduced: bool,
}

impl Settings {
    fn new(reaction: Reaction, respond: ReactTo, reduced: bool) -> Self {
        Self {
            reaction,
            sources: [respond.work, respond.loading, respond.completion, respond.attention, respond.typing, respond.media],
            reduced,
        }
    }

    fn gain(self) -> f32 {
        match self.reaction {
            Reaction::Still => 0.0,
            Reaction::Subtle => 0.36,
            Reaction::Expressive => 1.0,
        }
    }
}

const OUTPUT_SECONDS: f32 = 0.72;
const TYPING_SECONDS: f32 = 0.50;
const COMPLETION_SECONDS: f32 = 1.45;
const SETTLED: f32 = 0.0005;

#[derive(Debug, Default)]
pub struct Activity {
    input: Option<ActivityInput>,
    settings: Option<Settings>,
    last: Option<Instant>,
    output_at: Option<Instant>,
    typed_at: Option<Instant>,
    completed_at: Option<Instant>,
    frame: ActivityFrame,
    animating: bool,
}

impl Activity {
    /// Observe current state and advance to `now`. Call from the normal
    /// animation tick; `animating()` tells the caller whether another frame
    /// is needed. Still and reduced motion never request an animation tick.
    pub fn observe(&mut self, now: Instant, input: ActivityInput, reaction: Reaction, respond: ReactTo, reduced: bool) -> bool {
        let before = self.frame;
        let settings = Settings::new(reaction, respond, reduced);
        let reset = self.settings != Some(settings) || self.input.is_none_or(|old| old.identity != input.identity);
        if reset {
            // Consume counters at a context boundary, including sources
            // enabled after an event happened. Never carry a pulse to a
            // different pane or replay it when the user returns.
            self.frame = ActivityFrame::default();
            self.output_at = None;
            self.typed_at = None;
            self.completed_at = None;
            self.last = Some(now);
        } else if let Some(old) = self.input {
            // Integrate the old state first. An event after a long idle
            // therefore begins now, instead of inheriting the idle time.
            self.advance(now, old, settings);
            if !reduced && reaction != Reaction::Still {
                if respond.work && input.output > old.output {
                    self.output_at = Some(now);
                }
                if respond.typing && input.typed > old.typed {
                    self.typed_at = Some(now);
                }
                if respond.completion && input.completion > old.completion {
                    self.completed_at = Some(now);
                }
            }
        }
        self.input = Some(input);
        self.settings = Some(settings);
        self.last = Some(now);

        if reaction == Reaction::Still {
            self.frame = ActivityFrame::default();
            self.animating = false;
        } else {
            self.frame.progress = if respond.loading {
                input.progress.filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0))
            } else {
                None
            };
            self.frame.attention = respond.attention && input.attention;
            let (target, moving) = self.target(now, input, settings);
            if reduced {
                // Current work/progress/attention remains legible, with no
                // time-varying pulse or hidden timer for transient events.
                self.frame.energy = target;
                self.frame.phase = 0.0;
                self.animating = false;
            } else {
                self.animating = moving || (self.frame.energy - target).abs() > SETTLED;
            }
        }
        self.frame != before
    }

    pub fn frame(&self) -> ActivityFrame {
        self.frame
    }

    pub fn animating(&self) -> bool {
        self.animating
    }

    fn target(&self, now: Instant, input: ActivityInput, settings: Settings) -> (f32, bool) {
        let [work, loading, completion, attention, typing, media] = settings.sources;
        let working = work && input.working;
        let loading = loading && input.loading;
        let playing = media && input.media;
        let held = attention && input.attention;
        let mut base: f32 = 0.0;
        if working { base = base.max(0.44); }
        if loading { base = base.max(0.52); }
        if playing { base = base.max(0.32); }
        if held { base = base.max(0.30); }
        let (output, typed, completed) = if settings.reduced {
            (0.0, 0.0, 0.0)
        } else {
            (
                if work { pulse(self.output_at, now, OUTPUT_SECONDS) * 0.26 } else { 0.0 },
                if typing { pulse(self.typed_at, now, TYPING_SECONDS) * 0.20 } else { 0.0 },
                if completion { pulse(self.completed_at, now, COMPLETION_SECONDS) * 0.90 } else { 0.0 },
            )
        };
        // More output never means faster movement or an unbounded stack
        // of pulses. Sources share one slow envelope with a strict ceiling.
        let energy = (base + output.max(typed)).max(completed).min(1.0) * settings.gain();
        // Determinate materials follow the reported value, so a value that
        // has stopped changing must not keep a redundant animation clock.
        let determinate = settings.sources[1] && input.progress.is_some_and(f32::is_finite);
        (energy, (!determinate && (working || loading || playing)) || output > 0.0 || typed > 0.0 || completed > 0.0)
    }

    fn advance(&mut self, now: Instant, input: ActivityInput, settings: Settings) {
        if settings.reduced || settings.reaction == Reaction::Still { return; }
        let dt = self.last.map(|last| now.saturating_duration_since(last).as_secs_f32()).unwrap_or(0.0);
        let (target, moving) = self.target(now, input, settings);
        let tau = if target > self.frame.energy { 0.13 } else { 0.34 };
        self.frame.energy += (target - self.frame.energy) * (1.0 - (-dt / tau).exp());
        if (self.frame.energy - target).abs() <= SETTLED {
            self.frame.energy = target;
        }
        if moving {
            // Both registers retain a calm pace; Expressive chiefly
            // changes the area and depth of the material's response.
            self.frame.phase = (self.frame.phase + dt * 0.075).rem_euclid(1.0);
        }
    }
}

fn pulse(at: Option<Instant>, now: Instant, seconds: f32) -> f32 {
    at.map(|at| (1.0 - now.saturating_duration_since(at).as_secs_f32() / seconds).clamp(0.0, 1.0).powi(2)).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(start: Instant, seconds: f32) -> Instant {
        start + Duration::from_secs_f32(seconds)
    }

    fn observe(a: &mut Activity, start: Instant, seconds: f32, input: ActivityInput) {
        a.observe(at(start, seconds), input, Reaction::Expressive, ReactTo::default(), false);
    }

    #[test]
    fn still_is_neutral_and_never_requests_frames() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput { working: true, loading: true, progress: Some(0.7), attention: true, media: true, ..Default::default() };
        a.observe(start, input, Reaction::Still, ReactTo::default(), false);
        input.completion += 1;
        input.output += 1;
        a.observe(at(start, 1.0), input, Reaction::Still, ReactTo::default(), false);
        assert_eq!(a.frame(), ActivityFrame::default());
        assert!(!a.animating());
    }

    #[test]
    fn reduced_motion_reports_state_without_a_clock() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput { loading: true, progress: Some(0.0), attention: true, ..Default::default() };
        a.observe(start, input, Reaction::Expressive, ReactTo::default(), true);
        let frame = a.frame();
        assert!(frame.energy > 0.0 && frame.attention);
        assert_eq!(frame.progress, Some(0.0));
        input.output = 50;
        input.completion = 10;
        assert!(!a.observe(at(start, 50.0), input, Reaction::Expressive, ReactTo::default(), true));
        assert_eq!(a.frame(), frame);
        assert!(!a.animating());
        input.loading = false;
        input.attention = false;
        input.progress = None;
        a.observe(at(start, 51.0), input, Reaction::Expressive, ReactTo::default(), true);
        assert_eq!(a.frame(), ActivityFrame::default());
        assert!(!a.animating());
    }

    #[test]
    fn completion_is_authoritative_finite_and_has_no_idle_time_jump() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput::default();
        observe(&mut a, start, 0.0, input);
        input.completion = 1;
        observe(&mut a, start, 60.0, input);
        assert_eq!(a.frame().energy, 0.0);
        assert_eq!(a.frame().phase, 0.0);
        assert!(a.animating());
        observe(&mut a, start, 60.1, input);
        assert!(a.frame().energy > 0.1);
        for n in 2..=80 { observe(&mut a, start, 60.0 + n as f32 / 10.0, input); }
        assert_eq!(a.frame().energy, 0.0);
        assert!(!a.animating());
        let settled = a.frame();
        observe(&mut a, start, 120.0, input);
        assert_eq!(a.frame(), settled);
    }

    #[test]
    fn ending_work_does_not_invent_a_completion() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput { working: true, ..Default::default() };
        observe(&mut a, start, 0.0, input);
        observe(&mut a, start, 1.0, input);
        let energy = a.frame().energy;
        input.working = false;
        observe(&mut a, start, 1.0, input);
        observe(&mut a, start, 1.1, input);
        assert!(a.frame().energy < energy);
        observe(&mut a, start, 5.0, input);
        assert_eq!(a.frame().energy, 0.0);
        assert!(!a.animating());
    }

    #[test]
    fn held_attention_settles_without_repeating_motion() {
        let start = Instant::now();
        let mut a = Activity::default();
        let input = ActivityInput { attention: true, ..Default::default() };
        observe(&mut a, start, 0.0, input);
        observe(&mut a, start, 2.0, input);
        assert_eq!(a.frame().energy, 0.30);
        assert!(a.frame().attention);
        assert_eq!(a.frame().phase, 0.0);
        assert!(!a.animating());
        let settled = a.frame();
        observe(&mut a, start, 100.0, input);
        assert_eq!(a.frame(), settled);
    }

    #[test]
    fn progress_is_real_including_zero_one_and_unknown() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput { loading: true, progress: Some(0.0), ..Default::default() };
        observe(&mut a, start, 0.0, input);
        assert_eq!(a.frame().progress, Some(0.0));
        observe(&mut a, start, 10.0, input);
        assert_eq!(a.frame().progress, Some(0.0));
        assert_eq!(a.frame().phase, 0.0);
        assert!(!a.animating());
        input.progress = Some(1.0);
        observe(&mut a, start, 11.0, input);
        assert_eq!(a.frame().progress, Some(1.0));
        for invalid in [None, Some(f32::NAN), Some(f32::INFINITY)] {
            input.progress = invalid;
            observe(&mut a, start, 12.0, input);
            assert_eq!(a.frame().progress, None);
        }
    }

    #[test]
    fn disabled_sources_and_default_typing_media_are_quiet() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput { media: true, ..Default::default() };
        observe(&mut a, start, 0.0, input);
        input.typed += 1;
        observe(&mut a, start, 0.1, input);
        assert_eq!(a.frame(), ActivityFrame::default());
        assert!(!a.animating());
        let off = ReactTo { work: false, loading: false, completion: false, attention: false, typing: false, media: false };
        input.working = true;
        input.loading = true;
        input.progress = Some(0.5);
        input.attention = true;
        for n in 0..10 {
            input.output += 1;
            input.completion += 1;
            a.observe(at(start, 1.0 + n as f32), input, Reaction::Expressive, off, false);
            assert_eq!(a.frame(), ActivityFrame::default());
            assert!(!a.animating());
        }
    }

    #[test]
    fn first_sample_identity_changes_and_preferences_do_not_replay() {
        let start = Instant::now();
        let mut a = Activity::default();
        let mut input = ActivityInput { completion: 10, output: 20, typed: 30, ..Default::default() };
        observe(&mut a, start, 0.0, input);
        assert!(!a.animating());
        input.completion += 1;
        observe(&mut a, start, 0.1, input);
        observe(&mut a, start, 0.2, input);
        assert!(a.frame().energy > 0.0);
        input.identity = 1;
        input.completion = 100;
        observe(&mut a, start, 0.3, input);
        assert_eq!(a.frame(), ActivityFrame::default());
        assert!(!a.animating());
        input.identity = 0;
        input.completion = 11;
        observe(&mut a, start, 0.4, input);
        assert!(!a.animating());
        let on = ReactTo { typing: true, media: true, ..Default::default() };
        a.observe(at(start, 0.5), input, Reaction::Subtle, on, false);
        assert!(!a.animating());
        input.typed += 1;
        a.observe(at(start, 0.6), input, Reaction::Subtle, on, false);
        assert!(a.animating());
        a.observe(at(start, 0.7), input, Reaction::Subtle, on, false);
        assert!(a.frame().energy > 0.0 && a.frame().energy <= 0.36);
    }

    #[test]
    fn bursts_are_capped_and_do_not_scale_with_output_volume() {
        let start = Instant::now();
        let mut small = Activity::default();
        let mut large = Activity::default();
        let mut input = ActivityInput::default();
        observe(&mut small, start, 0.0, input);
        observe(&mut large, start, 0.0, input);
        for n in 1..=100 {
            input.output = n;
            observe(&mut small, start, n as f32 * 0.02, input);
            input.output = n * 10_000;
            observe(&mut large, start, n as f32 * 0.02, input);
            assert_eq!(small.frame(), large.frame());
            assert!(large.frame().energy <= 0.26);
        }
        observe(&mut large, start, 10.0, input);
        assert!(!large.animating());
    }

    #[test]
    fn steady_work_is_independent_of_frame_rate() {
        let start = Instant::now();
        let input = ActivityInput { working: true, ..Default::default() };
        let mut fine = Activity::default();
        let mut coarse = Activity::default();
        observe(&mut fine, start, 0.0, input);
        observe(&mut coarse, start, 0.0, input);
        for n in 1..=60 { observe(&mut fine, start, n as f32 / 60.0, input); }
        for n in 1..=10 { observe(&mut coarse, start, n as f32 / 10.0, input); }
        assert!((fine.frame().energy - coarse.frame().energy).abs() <= SETTLED);
        assert!((fine.frame().phase - coarse.frame().phase).abs() < 0.00001);
        assert!(fine.animating() && coarse.animating());
    }
}
