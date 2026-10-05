//! Scene-linear Blender response fields; arbitrary Space RGB remains an input.
//! Banks bake geometry, pearl film, black stroke, reflections and refraction.
//! Recolour before tone mapping, and resize with associated linear-light alpha.
use super::Face;
use crate::Color;
use std::{io::Read, sync::LazyLock};

static BLOBS: [&[u8]; 6] = [
    include_bytes!("../../../assets/icon/material/plex.bin"),
    include_bytes!("../../../assets/icon/material/silkscreen.bin"),
    include_bytes!("../../../assets/icon/material/plex-italic.bin"),
    include_bytes!("../../../assets/icon/material/bungee.bin"),
    include_bytes!("../../../assets/icon/material/rubik.bin"),
    include_bytes!("../../../assets/icon/material/newsreader.bin"),
];
static MERCURY_BLOBS: [&[u8]; 6] = [
    include_bytes!("../../../assets/icon/mercury/material/plex.bin"),
    include_bytes!("../../../assets/icon/mercury/material/silkscreen.bin"),
    include_bytes!("../../../assets/icon/mercury/material/plex-italic.bin"),
    include_bytes!("../../../assets/icon/mercury/material/bungee.bin"),
    include_bytes!("../../../assets/icon/mercury/material/rubik.bin"),
    include_bytes!("../../../assets/icon/mercury/material/newsreader.bin"),
];
const STEPS: usize = 7;
const NATIVE_SIZE: usize = 256;
static NATIVE: [LazyLock<Bank>; 6] = [
    LazyLock::new(|| Bank::read(BLOBS[0], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(BLOBS[1], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(BLOBS[2], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(BLOBS[3], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(BLOBS[4], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(BLOBS[5], NATIVE_SIZE)),
];
static MERCURY_NATIVE: [LazyLock<Bank>; 6] = [
    LazyLock::new(|| Bank::read(MERCURY_BLOBS[0], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(MERCURY_BLOBS[1], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(MERCURY_BLOBS[2], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(MERCURY_BLOBS[3], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(MERCURY_BLOBS[4], NATIVE_SIZE)),
    LazyLock::new(|| Bank::read(MERCURY_BLOBS[5], NATIVE_SIZE)),
];

struct Bank {
    size: usize,
    knots: [f32; STEPS],
    range: f32,
    alpha: Vec<u8>,
    radiance: Vec<u16>,
}
impl Bank {
    fn read(blob: &[u8], limit: usize) -> Self {
        // Keep a compact native level alongside the true 1024px export field.
        // Reading a small icon never inflates or retains the large level.
        let packed = if blob.starts_with(b"NUSD3D02") {
            let word = |at| u32::from_le_bytes(blob[at..at + 4].try_into().unwrap()) as usize;
            let count = word(8);
            let mut at = 12;
            let mut chosen = None;
            for _ in 0..count {
                let (size, bytes) = (word(at), word(at + 4));
                at += 8;
                if chosen.is_none() || size <= limit {
                    chosen = Some(&blob[at..at + bytes]);
                }
                at += bytes;
            }
            assert_eq!(at, blob.len());
            chosen.expect("embedded material resolution")
        } else {
            blob
        };
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(packed)
            .read_to_end(&mut raw)
            .expect("embedded icon material");
        assert_eq!(&raw[..8], b"NUSD3D01");
        let word = |at| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap());
        let size = word(8) as usize;
        assert_eq!(word(12) as usize, STEPS);
        let range = f32::from_bits(word(16));
        let knots = std::array::from_fn(|i| f32::from_bits(word(20 + i * 4)));
        let start = 20 + STEPS * 4;
        let count = size * size;
        assert_eq!(raw.len(), start + count + count * STEPS * 6);
        let alpha = raw[start..start + count].to_vec();
        let radiance = raw[start + count..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect();
        let bank = Self {
            size,
            knots,
            range,
            alpha,
            radiance,
        };
        drop(raw);
        bank.reduce(limit)
    }
    fn value(&self, index: usize) -> f32 {
        let v = self.radiance[index] as f32 / 65535.0;
        self.range * v * v
    }
    fn reduce(self, limit: usize) -> Self {
        if self.size <= limit {
            return self;
        }
        assert_eq!(self.size % limit, 0);
        let scale = self.size / limit;
        let count = limit * limit;
        let original = self.size * self.size;
        let mut alpha = vec![0; count];
        let mut radiance = vec![0; count * STEPS * 3];
        for y in 0..limit {
            for x in 0..limit {
                let dst = y * limit + x;
                let mut weight = 0.0;
                let mut sum = [[0.0; 3]; STEPS];
                for yy in y * scale..(y + 1) * scale {
                    for xx in x * scale..(x + 1) * scale {
                        let src = yy * self.size + xx;
                        let a = self.alpha[src] as f32;
                        weight += a;
                        for (step, rgb) in sum.iter_mut().enumerate() {
                            for (k, value) in rgb.iter_mut().enumerate() {
                                *value += self.value((step * original + src) * 3 + k) * a;
                            }
                        }
                    }
                }
                alpha[dst] = (weight / (scale * scale) as f32).round() as u8;
                if weight > 0.0 {
                    for (step, rgb) in sum.iter().enumerate() {
                        for (k, value) in rgb.iter().enumerate() {
                            radiance[(step * count + dst) * 3 + k] = (value / weight / self.range)
                                .sqrt()
                                .mul_add(65535.0, 0.0)
                                .round()
                                as u16;
                        }
                    }
                }
            }
        }
        Self {
            size: limit,
            knots: self.knots,
            range: self.range,
            alpha,
            radiance,
        }
    }
    fn frame(&self, signal: Color) -> Vec<u8> {
        let count = self.size * self.size;
        let curve: [(usize, f32); 3] = std::array::from_fn(|k| {
            let v = linear(signal[k].clamp(0.0, 1.0));
            let step = self
                .knots
                .partition_point(|k| *k <= v)
                .saturating_sub(1)
                .min(STEPS - 2);
            (
                step,
                (v - self.knots[step]) / (self.knots[step + 1] - self.knots[step]),
            )
        });
        let mut out = vec![0; count * 4];
        for (i, pixel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel[3] = self.alpha[i];
            if pixel[3] == 0 {
                continue;
            }
            let rgb = std::array::from_fn(|k| {
                let (step, t) = curve[k];
                let a = self.value((step * count + i) * 3 + k);
                let b = self.value(((step + 1) * count + i) * 3 + k);
                a + (b - a) * t
            });
            for (k, value) in neutral(rgb).into_iter().enumerate() {
                pixel[k] = (display(value).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        out
    }
}

pub(super) fn render(size: u32, signal: Color, face: Face) -> Vec<u8> {
    render_from(size, signal, face, &BLOBS, &NATIVE)
}
pub(super) fn mercury(size: u32, signal: Color, face: Face) -> Vec<u8> {
    render_from(size, signal, face, &MERCURY_BLOBS, &MERCURY_NATIVE)
}
fn render_from(
    size: u32,
    signal: Color,
    face: Face,
    blobs: &[&[u8]; 6],
    native: &[LazyLock<Bank>; 6],
) -> Vec<u8> {
    if size == 0 {
        return Vec::new();
    }
    let index = face as usize;
    if size as usize <= NATIVE_SIZE {
        let bank = &native[index];
        let pixels = bank.frame(signal);
        if size as usize == bank.size {
            pixels
        } else {
            resize(&pixels, bank.size, size as usize)
        }
    } else {
        // Full-resolution banks are temporary for package/export sizes. Native
        // icon work retains only the six 256px banks, about 17 MiB in total.
        let bank = Bank::read(blobs[index], usize::MAX);
        let pixels = bank.frame(signal);
        if size as usize == bank.size {
            pixels
        } else {
            resize(&pixels, bank.size, size as usize)
        }
    }
}
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn display(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
/// Khronos PBR Neutral's equations, matching the approved Blender view.
/// https://github.com/KhronosGroup/ToneMapping/tree/main/PBR_Neutral
fn neutral(mut rgb: [f32; 3]) -> [f32; 3] {
    let floor = rgb.into_iter().fold(f32::INFINITY, f32::min);
    let offset = if floor < 0.08 {
        floor - 6.25 * floor * floor
    } else {
        0.04
    };
    rgb.iter_mut().for_each(|v| *v -= offset);
    let peak = rgb.into_iter().fold(0.0, f32::max);
    if peak < 0.76 {
        return rgb;
    }
    let shoulder = 1.0 - 0.24 * 0.24 / (peak - 0.52);
    let blend = 1.0 - 1.0 / (1.0 + 0.15 * (peak - shoulder));
    rgb.map(|v| v * shoulder / peak * (1.0 - blend) + shoulder * blend)
}

/// Area filtering on reduction; bilinear on enlargement. Both associate alpha
/// in linear light so transparent texels cannot add a black/white fringe.
fn resize(src: &[u8], from: usize, to: usize) -> Vec<u8> {
    if from == to {
        return src.to_vec();
    }
    let colors: [f32; 256] = std::array::from_fn(|i| linear(i as f32 / 255.0));
    let mut out = vec![0; to * to * 4];
    let scale = from as f32 / to as f32;
    for y in 0..to {
        for x in 0..to {
            let mut sum = [0.0; 4];
            let mut weight = 0.0;
            let mut sample = |sx: usize, sy: usize, w: f32| {
                let pixel = &src[(sy * from + sx) * 4..][..4];
                let a = pixel[3] as f32 / 255.0;
                for k in 0..3 {
                    sum[k] += colors[pixel[k] as usize] * a * w;
                }
                sum[3] += a * w;
                weight += w;
            };
            if to < from {
                let (x0, x1) = (x as f32 * scale, (x + 1) as f32 * scale);
                let (y0, y1) = (y as f32 * scale, (y + 1) as f32 * scale);
                for sy in y0.floor() as usize..(y1.ceil() as usize).min(from) {
                    for sx in x0.floor() as usize..(x1.ceil() as usize).min(from) {
                        let w = (x1.min((sx + 1) as f32) - x0.max(sx as f32))
                            * (y1.min((sy + 1) as f32) - y0.max(sy as f32));
                        sample(sx, sy, w);
                    }
                }
            } else {
                let sx = ((x as f32 + 0.5) * scale - 0.5).clamp(0.0, (from - 1) as f32);
                let sy = ((y as f32 + 0.5) * scale - 0.5).clamp(0.0, (from - 1) as f32);
                let (ix, iy) = (sx as usize, sy as usize);
                let (fx, fy) = (sx.fract(), sy.fract());
                sample(ix, iy, (1.0 - fx) * (1.0 - fy));
                sample((ix + 1).min(from - 1), iy, fx * (1.0 - fy));
                sample(ix, (iy + 1).min(from - 1), (1.0 - fx) * fy);
                sample((ix + 1).min(from - 1), (iy + 1).min(from - 1), fx * fy);
            }
            let pixel = &mut out[(y * to + x) * 4..][..4];
            pixel[3] = (sum[3] / weight * 255.0).round() as u8;
            if sum[3] > 1e-8 {
                for k in 0..3 {
                    pixel[k] = (display(sum[k] / sum[3]).clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_native_level_preserves_the_full_resolution_response() {
        for blob in [BLOBS[5], MERCURY_BLOBS[5]] {
            let full = Bank::read(blob, usize::MAX);
            assert_eq!(full.size, 1024, "large exports must use real 1024px fields");
            let reduced = full.reduce(NATIVE_SIZE);
            let compact = Bank::read(blob, NATIVE_SIZE);
            assert_eq!(compact.alpha, reduced.alpha);
            assert!(compact
                .radiance
                .iter()
                .zip(&reduced.radiance)
                .all(|(a, b)| a.abs_diff(*b) <= 2));
        }
    }
    #[test]
    fn six_faces_and_arbitrary_signals_retain_alpha_and_remain_distinct() {
        let mut seen = Vec::new();
        for face in Face::ALL {
            let red = render(128, crate::theme::signal::RED, face);
            let custom = render(128, [0.29, 0.73, 0.61, 1.0], face);
            assert_ne!(red, custom);
            assert!(red
                .as_chunks::<4>()
                .0
                .iter()
                .zip(custom.as_chunks::<4>().0.iter())
                .all(|(a, b)| a[3] == b[3]));
            assert_eq!(red[3], 0);
            assert_eq!(red[red.len() - 1], 0);
            assert!(
                red.as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] > 240 && p[..3].iter().all(|v| *v > 220))
                    .count()
                    > 1000,
                "{}: lost enamel face",
                face.name()
            );
            assert!(!seen.contains(&red));
            seen.push(red);
        }
    }
    #[test]
    fn transparency_resizing_keeps_highlight_rgb_without_a_dark_fringe() {
        let result = resize(
            &[255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            2,
            1,
        );
        assert_eq!(result, [255, 255, 255, 64]);
        assert!(render(0, crate::theme::signal::RED, Face::Newsreader).is_empty());
    }
}
