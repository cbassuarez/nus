//! Offline star choreography. Runtime reads the baked motion bank.
use nus_render::Rect;
use std::f32::consts::TAU;
pub const GATHER: f32 = 4.4;
pub const BRAKED: f32 = 6.4;
pub const ORBIT: f32 = 6.6;
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
pub fn phase(t: f32, start: f32, duration: f32) -> f32 {
    ease((t - start) / duration)
}
pub fn mix(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
pub fn hash(i: usize) -> f32 {
    let mut n = (i as u32).wrapping_add(1).wrapping_mul(0x9e3779b9);
    n ^= n >> 16;
    n = n.wrapping_mul(0x85ebca6b);
    n ^= n >> 13;
    (n & 0xffffff) as f32 / 16777216.0
}
/// Integrate forward speed through acceleration and braking. Position never
/// reverses; streak length follows velocity back to zero at rest.
pub fn warp(t: f32) -> (f32, f32) {
    let u = ((t - 1.25) / (GATHER - 1.25)).clamp(0.0, 1.0);
    if t <= GATHER {
        return (1.9 * u.powi(3), u * u);
    }
    let brake = ((t - GATHER) / (BRAKED - GATHER)).clamp(0.0, 1.0);
    let velocity = 1.9 * 3.0 / (GATHER - 1.25);
    let distance =
        1.9 + velocity * (BRAKED - GATHER) * (brake - brake.powi(3) + 0.5 * brake.powi(4));
    (distance, 1.0 - ease(brake))
}
/// A fixed cloud with an accelerating projection: streak length is exactly
/// zero for the opening hold and after braking, with no inward collapse.
pub fn flight(i: usize, t: f32, card: Rect) -> ([f32; 2], [f32; 2]) {
    let angle = hash(i * 3) * TAU;
    let (travel, speed) = warp(t);
    let depth = (hash(i * 3 + 1) + travel).fract();
    let radius = 0.06 + depth.powf(2.1) * 0.88;
    let length = speed * (0.008 + depth * 0.20);
    let at = |r: f32| {
        [
            card.x + card.w * 0.5 + angle.cos() * r * card.w,
            card.y + card.h * 0.48 + angle.sin() * r * card.h,
        ]
    };
    (at(radius), at((radius - length).max(0.025)))
}
/// A separate constellation, with its own seed, positions and clock. Its
/// convergence never consumes, pauses or redirects a background star.
pub fn constellation(i: usize, target: [f32; 2], card: Rect, mark: Rect, t: f32) -> [f32; 2] {
    let angle = hash(i + 20000) * TAU;
    let radius = 0.42 + hash(i + 23000) * 0.28;
    let origin = [
        card.x + card.w * 0.5 + angle.cos() * card.w * radius,
        card.y + card.h * 0.46 + angle.sin() * card.h * radius,
    ];
    let dest = [mark.x + target[0] * mark.w, mark.y + target[1] * mark.h];
    let gather = phase(t, GATHER + hash(i + 25000) * 0.35, 1.85);
    let mut p = mix(origin, dest, gather);
    let bend = (gather * std::f32::consts::PI).sin() * mark.w * 0.22;
    p[0] -= angle.sin() * bend;
    p[1] += angle.cos() * bend;
    p
}
