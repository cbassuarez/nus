//! One rhythm for shell cells and native text-field pipes.
//!
//! The caller measures elapsed time from the most recent input. Resetting
//! that clock always reveals the caret; there is no global blink phase.

use std::time::Duration;

use crate::settings::Blink;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    pub opacity: f32,
    /// Next visual change, relative to this sample. Steady carets need no tick.
    pub next_ms: Option<u64>,
}

const IDLE_HOLD_MS: u128 = 250;
const FRAME_MS: u64 = 16;

/// Sample a complete blink cycle: hold 55%, fade out 7%, rest 31%, fade in 7%.
/// Soft transitions preserve long, readable plateaus without constant redraws.
pub(crate) fn sample(blink: Blink, period_ms: u32, elapsed: Duration, reduced: bool) -> Sample {
    if reduced || blink == Blink::Never {
        return Sample { opacity: 1.0, next_ms: None };
    }

    let period = u128::from(period_ms.max(200));
    let elapsed = elapsed.as_millis();
    let hold = if blink == Blink::AfterIdle { IDLE_HOLD_MS } else { 0 };
    let on_end = period as f64 * 0.55;
    if elapsed < hold {
        // The hold and the first visible plateau have identical appearance.
        return Sample { opacity: 1.0, next_ms: Some((hold - elapsed) as u64 + on_end.ceil() as u64) };
    }

    let phase = ((elapsed - hold) % period) as f64;
    let out_end = period as f64 * 0.62;
    let rest_end = period as f64 * 0.93;
    let until = |boundary: f64| ((boundary - phase).ceil() as u64).max(1);
    let smooth = |t: f64| {
        let t = t.clamp(0.0, 1.0);
        (t * t * (3.0 - 2.0 * t)) as f32
    };

    if phase < on_end {
        Sample { opacity: 1.0, next_ms: Some(until(on_end)) }
    } else if phase < out_end {
        Sample { opacity: 1.0 - smooth((phase - on_end) / (out_end - on_end)), next_ms: Some(FRAME_MS.min(until(out_end))) }
    } else if phase < rest_end {
        Sample { opacity: 0.0, next_ms: Some(until(rest_end)) }
    } else {
        Sample { opacity: smooth((phase - rest_end) / (period as f64 - rest_end)), next_ms: Some(FRAME_MS.min(until(period as f64))) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(blink: Blink, elapsed_ms: u64) -> Sample {
        sample(blink, 1200, Duration::from_millis(elapsed_ms), false)
    }

    #[test]
    fn input_reveals_caret_and_idle_hold_extends_first_visible_plateau() {
        assert_eq!(at(Blink::Always, 0), Sample { opacity: 1.0, next_ms: Some(660) });
        assert_eq!(at(Blink::AfterIdle, 0), Sample { opacity: 1.0, next_ms: Some(910) });
        assert_eq!(at(Blink::AfterIdle, 249).opacity, 1.0);
        assert_eq!(at(Blink::AfterIdle, 910), at(Blink::Always, 660));
        assert_eq!(at(Blink::AfterIdle, 1450), at(Blink::Always, 0));
    }

    #[test]
    fn a_full_cycle_starts_visible_and_fades_in_both_directions() {
        assert_eq!(at(Blink::Always, 659).opacity, 1.0);
        assert_eq!(at(Blink::Always, 702).opacity, 0.5);
        assert_eq!(at(Blink::Always, 744).opacity, 0.0);
        assert_eq!(at(Blink::Always, 1116).opacity, 0.0);
        assert_eq!(at(Blink::Always, 1158).opacity, 0.5);
        assert_eq!(at(Blink::Always, 1200), at(Blink::Always, 0));
    }

    #[test]
    fn redraws_wait_through_plateaus_and_do_not_skip_fade_boundaries() {
        assert_eq!(at(Blink::Always, 600).next_ms, Some(60));
        assert_eq!(at(Blink::Always, 660).next_ms, Some(16));
        assert_eq!(at(Blink::Always, 740).next_ms, Some(4));
        assert_eq!(at(Blink::Always, 744).next_ms, Some(372));
        assert_eq!(at(Blink::Always, 1116).next_ms, Some(16));
        assert_eq!(at(Blink::Always, 1199).next_ms, Some(1));
    }

    #[test]
    fn reduced_motion_and_explicit_never_are_steady_without_redraws() {
        let steady = Sample { opacity: 1.0, next_ms: None };
        for elapsed in [0, 1000, u64::MAX] {
            assert_eq!(sample(Blink::Never, 1200, Duration::from_millis(elapsed), false), steady);
            assert_eq!(sample(Blink::Always, 1200, Duration::from_millis(elapsed), true), steady);
            assert_eq!(sample(Blink::AfterIdle, 1200, Duration::from_millis(elapsed), true), steady);
        }
    }

    #[test]
    fn malformed_short_periods_still_produce_bounded_opacity_and_future_deadlines() {
        for period in [0, 1, 199, 200, 531, 1200, u32::MAX] {
            for elapsed in [0, 109, 110, 111, 185, 199, 1000, u64::MAX] {
                let value = sample(Blink::Always, period, Duration::from_millis(elapsed), false);
                assert!((0.0..=1.0).contains(&value.opacity));
                assert!(value.next_ms.is_some_and(|ms| ms > 0));
            }
        }
    }
}
