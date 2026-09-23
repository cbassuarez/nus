//! Authored-stroke liquid construction and silhouette-locked metallic flow.
use std::f32::consts::TAU;
pub const FORMS: usize = 5;
pub const INTRO_FPS: usize = 24;
pub const IDLE_FPS: usize = 16;
pub const IDLE_SECONDS: usize = 8;
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
// The pen follows the actual italic letter: entry, shoulder, downstroke,
// return up the wet stem, arch, second downstroke. The orbit follows its base.
const LETTER: &[[f32; 2]] = &[
    [0.197, 0.397],
    [0.248, 0.306],
    [0.328, 0.241],
    [0.419, 0.222],
    [0.431, 0.267],
    [0.385, 0.422],
    [0.332, 0.597],
    [0.292, 0.743],
    [0.350, 0.560],
    [0.408, 0.440],
    [0.512, 0.332],
    [0.618, 0.261],
    [0.709, 0.235],
    [0.762, 0.266],
    [0.748, 0.368],
    [0.696, 0.531],
    [0.642, 0.672],
];
fn projection(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> (f32, f32, [f32; 2]) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = dx.hypot(dy);
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (len * len)).clamp(0.0, 1.0);
    (
        (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dy * t),
        t,
        [dx / len, dy / len],
    )
}
/// Propagate through the connected metal, with slower flow in the thin orbit.
/// A wet front can branch at a junction, but cannot jump across transparent
/// space or cut a previously drawn stroke into unrelated islands.
fn arrivals(n: usize, art: &[u8]) -> Vec<f32> {
    use std::{cmp::Reverse, collections::BinaryHeap};
    let mut distance = vec![u32::MAX; n * n];
    let start = (0..n * n)
        .filter(|&i| art[i * 4 + 3] > 127)
        .min_by_key(|&i| {
            let x = i % n;
            let y = i / n;
            ((x as f32 / n as f32 - LETTER[0][0]).hypot(y as f32 / n as f32 - LETTER[0][1])
                * 100000.0) as u32
        })
        .unwrap_or(0);
    let mut heap = BinaryHeap::new();
    distance[start] = 0;
    heap.push(Reverse((0u32, start)));
    let cost: Vec<_> = (0..n * n)
        .map(|i| {
            let p = [(i % n) as f32 / n as f32, (i / n) as f32 / n as f32];
            let d = LETTER
                .windows(2)
                .map(|w| projection(p, w[0], w[1]).0)
                .fold(f32::MAX, f32::min);
            1000 + (smooth((d - 0.045) / 0.06) * 1600.0) as u32
        })
        .collect();
    while let Some(Reverse((d, i))) = heap.pop() {
        if d != distance[i] {
            continue;
        }
        let x = (i % n) as isize;
        let y = (i / n) as isize;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || yy < 0 || xx >= n as isize || yy >= n as isize {
                    continue;
                }
                let j = yy as usize * n + xx as usize;
                if art[j * 4 + 3] < 8 {
                    continue;
                }
                let next = d + cost[j] * if dx != 0 && dy != 0 { 1414 } else { 1000 } / 1000;
                if next < distance[j] {
                    distance[j] = next;
                    heap.push(Reverse((next, j)));
                }
            }
        }
    }
    let max = *distance
        .iter()
        .filter(|&&d| d != u32::MAX)
        .max()
        .unwrap_or(&1);
    let mut at: Vec<_> = distance
        .iter()
        .map(|&d| {
            if d == u32::MAX {
                1.0
            } else {
                d as f32 / max.max(1) as f32 * 0.91
            }
        })
        .collect();
    // Antialiased fringe inherits its adjacent metal. Detached orbit beads
    // collect last; their radial fronts grow outward from their own centers.
    for i in 0..n * n {
        if distance[i] != u32::MAX || art[i * 4 + 3] == 0 {
            continue;
        }
        let (x, y) = (i % n, i / n);
        let mut nearby = 1.0f32;
        for dy in -2isize..=2 {
            for dx in -2isize..=2 {
                let (xx, yy) = (x as isize + dx, y as isize + dy);
                if xx >= 0 && yy >= 0 && xx < n as isize && yy < n as isize {
                    let j = yy as usize * n + xx as usize;
                    if distance[j] != u32::MAX {
                        nearby = nearby.min(at[j]);
                    }
                }
            }
        }
        at[i] = if nearby < 1.0 {
            nearby
        } else {
            let p = [x as f32 / n as f32, y as f32 / n as f32];
            0.94 + [(0.925, 0.446), (0.965, 0.409)]
                .iter()
                .map(|&(cx, cy)| (p[0] - cx).hypot(p[1] - cy))
                .fold(0.05, f32::min)
        };
    }
    at
}
fn pen(path: &[[f32; 2]], progress: f32) -> [f32; 2] {
    let length: f32 = path
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum();
    let mut distance = progress.clamp(0.0, 1.0) * length;
    for w in path.windows(2) {
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if distance <= len {
            return [
                mix(w[0][0], w[1][0], distance / len),
                mix(w[0][1], w[1][1], distance / len),
            ];
        }
        distance -= len;
    }
    *path.last().unwrap()
}
pub struct Frames {
    pub intro: Vec<Vec<u8>>,
    pub idle: Vec<Vec<u8>>,
}
pub fn frames(size: usize, art: &[u8]) -> Frames {
    assert_eq!(art.len(), size * size * 4);
    let arrivals = arrivals(size, art);
    let mut intro = Vec::new();
    for frame in 0..84 {
        let p = frame as f32 / 83.0;
        let mut rgba = undulate(size, art, (p - 1.0) * 3.5);
        // A short meniscus leads the ink, then settles to the approved material.
        // The footprint is full scale from the outset, with no extra Dock bounce.
        for (i, &at) in arrivals.iter().enumerate() {
            let coverage = smooth((p * 1.025 - at) / 0.025);
            let rim = (1.0 - ((p * 1.025 - at - 0.014) / 0.012).abs()).max(0.0);
            for c in 0..3 {
                rgba[i * 4 + c] = mix(rgba[i * 4 + c] as f32, 245.0, rim * 0.32) as u8;
            }
            rgba[i * 4 + 3] = (rgba[i * 4 + 3] as f32 * coverage).round() as u8;
        }
        // A chrome bead feeds the stroke, stretching into the entry shoulder.
        // It merges into the already wet letter instead of changing into a
        // separate family of blobs. Native macOS still owns the bounce.
        if p < 0.18 {
            let head = pen(LETTER, p * 0.45);
            let radius = mix(0.075, 0.032, smooth(p / 0.18));
            for y in 0..size {
                for x in 0..size {
                    let dx = (x as f32 / (size - 1) as f32 - head[0]) / radius;
                    let dy = (y as f32 / (size - 1) as f32 - head[1]) / radius;
                    let rr = dx * dx + dy * dy;
                    if rr >= 1.0 {
                        continue;
                    }
                    let i = (y * size + x) * 4;
                    let a = smooth((1.0 - rr) * radius * size as f32)
                        * (1.0 - smooth((p - 0.12) / 0.06));
                    let z = (1.0 - rr).sqrt();
                    let band = (-((dy + 0.15) / 0.16).powi(2)).exp();
                    let tone = (0.56 + 0.37 * z - 0.73 * band
                        + 0.22 * (-((dx + 0.4) / 0.20).powi(2)).exp())
                    .clamp(0.05, 1.0)
                        * 255.0;
                    let base = rgba[i + 3] as f32 / 255.0;
                    let alpha = a + base * (1.0 - a);
                    for c in 0..3 {
                        rgba[i + c] = ((tone * a + rgba[i + c] as f32 * base * (1.0 - a)) / alpha)
                            .round() as u8;
                    }
                    rgba[i + 3] = (alpha * 255.0).round() as u8;
                }
            }
        }
        if frame == 83 {
            rgba = art.to_vec();
        }
        intro.push(crate::icon::png(&rgba, size as u32, size as u32));
    }
    let idle = Vec::new();
    Frames { intro, idle }
}
fn sample(n: usize, art: &[u8], u: f32, v: f32) -> [f32; 4] {
    let x = u * (n - 1) as f32;
    let y = v * (n - 1) as f32;
    if x < 0.0 || y < 0.0 || x >= n as f32 - 1.0 || y >= n as f32 - 1.0 {
        return [0.0; 4];
    }
    let ix = x as usize;
    let iy = y as usize;
    let fx = x.fract();
    let fy = y.fract();
    let mut c = [0.0; 4];
    for (dx, dy, w) in [
        (0, 0, (1.0 - fx) * (1.0 - fy)),
        (1, 0, fx * (1.0 - fy)),
        (0, 1, (1.0 - fx) * fy),
        (1, 1, fx * fy),
    ] {
        let i = ((iy + dy) * n + ix + dx) * 4;
        let a = art[i + 3] as f32 / 255.0;
        c[3] += w * a;
        for k in 0..3 {
            c[k] += w * a * art[i + k] as f32 / 255.0;
        }
    }
    if c[3] > 0.0 {
        for k in 0..3 {
            c[k] /= c[3];
        }
    }
    c
}

/// Reflections stream along the strokes; alpha and edge geometry never move.
/// This field is mirrored in quad.wgsl for the full-resolution claim artwork.
pub fn undulate(n: usize, art: &[u8], seconds: f32) -> Vec<u8> {
    let t = seconds * TAU / IDLE_SECONDS as f32;
    let mut out = art.to_vec();
    for y in 0..n {
        for x in 0..n {
            let i = (y * n + x) * 4;
            if art[i + 3] == 0 {
                continue;
            }
            let u = x as f32 / (n - 1) as f32;
            let v = y as f32 / (n - 1) as f32;
            let orbit = smooth((v - 0.66) / 0.10);
            let dx = mix(-0.30, 0.94, orbit);
            let dy = mix(0.954, -0.342, orbit);
            let travel = v * 19.0 - u * 7.0 - t;
            let offset = (travel.sin() + 0.3 * (travel * 2.0 + t).sin()) * 0.009;
            let reflection = sample(n, art, u + dx * offset, v + dy * offset);
            let guard = sample(n, art, u - dx * 0.012, v - dy * 0.012)[3]
                .min(sample(n, art, u + dx * 0.012, v + dy * 0.012)[3]);
            let inner = smooth((art[i + 3] as f32 / 255.0 - 0.98) / 0.02)
                * smooth((reflection[3] - 0.98) / 0.02)
                * smooth((guard - 0.98) / 0.02);
            for c in 0..3 {
                out[i + c] = mix(art[i + c] as f32, reflection[c] * 255.0, inner * 0.85)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn liquid_front_travels_through_connected_metal() {
        let n = 64;
        let mut art = vec![0; n * n * 4];
        for y in 12..55 {
            for x in 10..18 {
                art[(y * n + x) * 4 + 3] = 255;
            }
        }
        for y in 47..55 {
            for x in 10..55 {
                art[(y * n + x) * 4 + 3] = 255;
            }
        }
        for y in 12..55 {
            for x in 47..55 {
                art[(y * n + x) * 4 + 3] = 255;
            }
        }
        let at = arrivals(n, &art);
        assert!(at[20 * n + 14] < at[50 * n + 14]);
        assert!(at[50 * n + 14] < at[50 * n + 50]);
        assert!(at[50 * n + 50] < at[20 * n + 50]);
        assert_eq!(at[25 * n + 30], 1.0); // never shortcut the open counter
    }
    #[test]
    fn flow_preserves_every_alpha_and_loops_without_a_seam() {
        let n = 48;
        let mut art = vec![0; n * n * 4];
        for y in 8..40 {
            for x in 8..40 {
                let i = (y * n + x) * 4;
                art[i..i + 4].copy_from_slice(&[
                    (x * 5) as u8,
                    (y * 5) as u8,
                    220,
                    if x == 8 { 80 } else { 255 },
                ]);
            }
        }
        let a = undulate(n, &art, 0.0);
        let b = undulate(n, &art, 8.0);
        let c = undulate(n, &art, 2.0);
        assert!(a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= 1));
        assert_ne!(a, c);
        for phase in [a, c] {
            assert!(phase
                .as_chunks::<4>()
                .0
                .iter()
                .zip(art.as_chunks::<4>().0.iter())
                .all(|(p, q)| p[3] == q[3]));
        }
    }
}
