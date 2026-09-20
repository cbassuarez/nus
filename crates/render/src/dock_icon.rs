//! Desktop icon: a white letter emerging above the existing orbit.
//! Keep the splash/site mark in `icon` intact; share its authored Band geometry.
use crate::{icon::Band, text::bundled, Color};
use swash::{
    scale::{Render, ScaleContext, Source},
    zeno::Format,
    FontRef,
};

/// Exact order and optical size factors from nus-promo/src/design/Wordmark.tsx.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Plex,
    Silkscreen,
    PlexItalic,
    Bungee,
    Rubik,
    Newsreader,
}
impl Face {
    pub const ALL: [Self; 6] = [
        Self::Plex,
        Self::Silkscreen,
        Self::PlexItalic,
        Self::Bungee,
        Self::Rubik,
        Self::Newsreader,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Plex => "Plex",
            Self::Silkscreen => "Silkscreen",
            Self::PlexItalic => "Plex Italic",
            Self::Bungee => "Bungee",
            Self::Rubik => "Rubik Mono",
            Self::Newsreader => "Newsreader",
        }
    }
    fn font(self) -> (&'static [u8], f32) {
        match self {
            Self::Plex => (bundled::PLEX_MONO_SEMIBOLD, 0.86),
            Self::Silkscreen => (
                include_bytes!("../../../assets/fonts/Silkscreen-Regular.ttf"),
                0.84,
            ),
            Self::PlexItalic => (bundled::PLEX_MONO_ITALIC, 0.86),
            Self::Bungee => (
                include_bytes!("../../../assets/fonts/Bungee-Regular.ttf"),
                0.7,
            ),
            Self::Rubik => (
                include_bytes!("../../../assets/fonts/RubikMonoOne-Regular.ttf"),
                0.7,
            ),
            Self::Newsreader => (bundled::NEWSREADER_ITALIC, 1.0),
        }
    }
}

/// A sixteenth note at the promo's 152 bpm. Hold the final wordmark afterward.
pub const STEP_SECONDS: f32 = 60.0 / 152.0 / 4.0;
pub fn face_at(seconds: f32) -> Face {
    Face::ALL[((seconds.max(0.0) / STEP_SECONDS) as usize).min(5)]
}

/// Keep cycling while launch is pending. Readiness, not a fixed launch delay,
/// chooses the final Newsreader frame. Also used by bounded attention loops.
pub fn launch_face_at(seconds: f32) -> Face {
    Face::ALL[(seconds.max(0.0) / STEP_SECONDS) as usize % Face::ALL.len()]
}

/// Straight-alpha RGBA. The n is always pure white, never the theme paper.
/// A dark keyline and soft offset shadow protect it on light desktop grounds.
pub fn render(size: u32, signal: Color, face: Face) -> Vec<u8> {
    Field::new(size, face).frame(signal)
}

/// A still Signal badge over the existing authored mark. Empty text is a dot.
pub fn tray(size: u32, signal: Color, badge: Option<&str>) -> Vec<u8> {
    let mut rgba = render(size, signal, Face::Newsreader);
    let Some(text) = badge else {
        return rgba;
    };
    let s = size as f32;
    let radius = if text.is_empty() { s * 0.13 } else { s * 0.24 };
    let cx = s - radius - 1.0;
    let cy = s - radius - 1.0;
    let composite = |out: &mut [u8], x: u32, y: u32, c: Color, coverage: f32| {
        let i = ((y * size + x) * 4) as usize;
        let mut base = [
            out[i] as f32 / 255.0,
            out[i + 1] as f32 / 255.0,
            out[i + 2] as f32 / 255.0,
            out[i + 3] as f32 / 255.0,
        ];
        over(&mut base, c, coverage);
        for k in 0..4 {
            out[i + k] = (base[k] * 255.0).round() as u8;
        }
    };
    for y in 0..size {
        for x in 0..size {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            composite(
                &mut rgba,
                x,
                y,
                [1.0; 4],
                (radius + 1.0 - distance).clamp(0.0, 1.0),
            );
            composite(
                &mut rgba,
                x,
                y,
                signal,
                (radius - 0.8 - distance).clamp(0.0, 1.0),
            );
        }
    }
    if !text.is_empty() {
        let font =
            FontRef::from_index(bundled::PLEX_MONO_SEMIBOLD, 0).expect("bundled signal font");
        let mut ctx = ScaleContext::new();
        let mut scaler = ctx
            .builder(font)
            .size(s * if text.len() > 1 { 0.3 } else { 0.4 })
            .hint(true)
            .build();
        let images: Vec<_> = text
            .chars()
            .filter_map(|c| {
                Render::new(&[Source::Outline])
                    .format(Format::Alpha)
                    .render(&mut scaler, font.charmap().map(c))
            })
            .collect();
        let width = images
            .iter()
            .map(|i| i.placement.width + 1)
            .sum::<u32>()
            .saturating_sub(1);
        let mut left = (cx - width as f32 / 2.0).round() as i32;
        let luminance = signal[0] * 0.2126 + signal[1] * 0.7152 + signal[2] * 0.0722;
        let colour = if luminance > 0.5 {
            [0.02, 0.02, 0.025, 1.0]
        } else {
            [1.0; 4]
        };
        for img in images {
            let top = (cy - img.placement.height as f32 / 2.0).round() as i32;
            for y in 0..img.placement.height {
                for x in 0..img.placement.width {
                    let (px, py) = (left + x as i32, top + y as i32);
                    if px >= 0 && py >= 0 && px < size as i32 && py < size as i32 {
                        composite(
                            &mut rgba,
                            px as u32,
                            py as u32,
                            colour,
                            img.data[(y * img.placement.width + x) as usize] as f32 / 255.0,
                        );
                    }
                }
            }
            left += img.placement.width as i32 + 1;
        }
    }
    rgba
}

/// Geometry and shadows are sampled once; changing theme only recolours the orbit.
pub struct Field {
    under: Vec<Color>,
    letter: Vec<Color>,
    back: Vec<f32>,
    front: Vec<f32>,
}
impl Field {
    pub fn new(size: u32, face: Face) -> Self {
        assert!(size > 0);
        let s = size as f32;
        let band = Band::in_frame(s);
        let count = (size * size) as usize;
        let mut glyph = vec![0.0f32; count];
        let mut clip = vec![0.0f32; count];
        let mut back = vec![0.0f32; count];
        let mut front = vec![0.0f32; count];
        let (bytes, factor) = face.font();
        let font = FontRef::from_index(bytes, 0).expect("bundled dock face");
        let mut ctx = ScaleContext::new();
        let mut scaler = ctx
            .builder(font)
            .size(s * 1.38 * factor)
            .hint(false)
            .build();
        if let Some(img) = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, font.charmap().map('n'))
        {
            let (w, h) = (img.placement.width, img.placement.height);
            let gx = ((s - w as f32) * 0.5 + s * 0.01).round() as i32;
            let gy = ((s - h as f32) * 0.5 + s * 0.02).round() as i32;
            for y in 0..h {
                for x in 0..w {
                    let (ox, oy) = (gx + x as i32, gy + y as i32);
                    if ox >= 0 && oy >= 0 && ox < size as i32 && oy < size as i32 {
                        glyph[(oy as u32 * size + ox as u32) as usize] =
                            img.data[(y * w + x) as usize] as f32 / 255.0;
                    }
                }
            }
        }
        let (cos, sin) = (band.tilt.cos(), band.tilt.sin());
        for y in 0..size {
            for x in 0..size {
                let i = (y * size + x) as usize;
                let (dx, dy) = (x as f32 + 0.5 - band.cx, y as f32 + 0.5 - band.cy);
                clip[i] = (front_y(&band, x as f32 + 0.5) - y as f32).clamp(0.0, 1.0);
                glyph[i] *= clip[i];
                let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                let r = ((u / band.a).powi(2) + (v / band.b).powi(2)).sqrt();
                let g = ((u / (band.a * band.a)).powi(2) + (v / (band.b * band.b)).powi(2))
                    .sqrt()
                    .max(1e-6)
                    / r.max(1e-6);
                let d = (r - 1.0) / g;
                let along =
                    ((v / band.b).atan2(u / band.a) - band.start).rem_euclid(std::f32::consts::TAU);
                if along <= band.span {
                    let cov =
                        (band.thickness(along, band.span) * 0.5 - d.abs() + 0.5).clamp(0.0, 1.0);
                    if v > 0.0 {
                        front[i] = cov;
                    } else {
                        back[i] = cov;
                    }
                }
            }
        }
        // A crisp edge remains visible even at 16 px; the softer shadow adds depth.
        let edge = dilate(&glyph, size, (s * 0.004).round().max(1.0) as usize);
        let orbit: Vec<_> = front.iter().zip(&back).map(|(a, b)| a.max(*b)).collect();
        let orbit_edge = dilate(&orbit, size, (s * 0.002).round().max(1.0) as usize);
        let mut shadow = blur(&glyph, size, (s * 0.014).round().max(1.0) as usize);
        shadow = blur(&shadow, size, (s * 0.014).round().max(1.0) as usize);
        let offset = (s * 0.014).round().max(1.0) as i32;
        let mut under = vec![[0.0; 4]; count];
        let mut letter = under.clone();
        for y in 0..size {
            for x in 0..size {
                let i = (y * size + x) as usize;
                let (sx, sy) = (x as i32 - offset, y as i32 - offset);
                let shadow_cov = if sx >= 0 && sy >= 0 {
                    shadow[(sy as u32 * size + sx as u32) as usize] * clip[i]
                } else {
                    0.0
                };
                let c = &mut under[i];
                over(c, [0.015, 0.02, 0.03, 0.72], shadow_cov);
                over(c, [0.015, 0.02, 0.03, 0.6], orbit_edge[i]);
                over(&mut letter[i], [0.015, 0.02, 0.03, 0.82], edge[i] * clip[i]);
                over(&mut letter[i], [1.0; 4], glyph[i]);
            }
        }
        Self {
            under,
            letter,
            back,
            front,
        }
    }
    pub fn frame(&self, signal: Color) -> Vec<u8> {
        let mut out = vec![0u8; self.under.len() * 4];
        for (i, base) in self.under.iter().enumerate() {
            let mut c = *base;
            over(&mut c, signal, self.back[i]);
            over(&mut c, self.letter[i], 1.0);
            over(&mut c, signal, self.front[i]);
            for k in 0..4 {
                out[i * 4 + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        out
    }
}

/// Lower screen-space intersection with the authored tilted ellipse.
pub(crate) fn front_y(b: &Band, x: f32) -> f32 {
    let (cos, sin) = (b.tilt.cos(), b.tilt.sin());
    let dx = x - b.cx;
    let a = sin * sin / (b.a * b.a) + cos * cos / (b.b * b.b);
    let bb = 2.0 * dx * cos * sin * (1.0 / (b.a * b.a) - 1.0 / (b.b * b.b));
    let c = dx * dx * (cos * cos / (b.a * b.a) + sin * sin / (b.b * b.b)) - 1.0;
    let d = bb * bb - 4.0 * a * c;
    if d < 0.0 {
        f32::NEG_INFINITY
    } else {
        b.cy + (-bb + d.sqrt()) / (2.0 * a)
    }
}
fn over(dst: &mut Color, c: Color, cov: f32) {
    let a = c[3] * cov;
    let out = a + dst[3] * (1.0 - a);
    if out <= 0.0 {
        return;
    }
    for k in 0..3 {
        dst[k] = (c[k] * a + dst[k] * dst[3] * (1.0 - a)) / out;
    }
    dst[3] = out;
}
fn dilate(src: &[f32], size: u32, r: usize) -> Vec<f32> {
    filter(src, size, r, true)
}
fn blur(src: &[f32], size: u32, r: usize) -> Vec<f32> {
    filter(src, size, r, false)
}
fn filter(src: &[f32], size: u32, r: usize, max: bool) -> Vec<f32> {
    let n = size as usize;
    let mut a = vec![0.0; src.len()];
    let mut b = a.clone();
    for y in 0..n {
        for x in 0..n {
            let row = &src[y * n + x.saturating_sub(r)..y * n + (x + r + 1).min(n)];
            a[y * n + x] = if max {
                row.iter().copied().fold(0.0, f32::max)
            } else {
                row.iter().sum::<f32>() / (2 * r + 1) as f32
            };
        }
    }
    for y in 0..n {
        for x in 0..n {
            let col = (y.saturating_sub(r)..(y + r + 1).min(n)).map(|yy| a[yy * n + x]);
            b[y * n + x] = if max {
                col.fold(0.0, f32::max)
            } else {
                col.sum::<f32>() / (2 * r + 1) as f32
            };
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_faces_are_white_clipped_and_distinct() {
        let mut frames = Vec::new();
        for face in Face::ALL {
            let size = 128;
            let px = render(size, [0.8, 0.05, 0.1, 1.0], face);
            let b = Band::in_frame(size as f32);
            let mut whites = 0;
            for (i, p) in px.chunks_exact(4).enumerate() {
                if p[0] > 250 && p[1] > 250 && p[2] > 250 && p[3] > 240 {
                    whites += 1;
                    assert!(
                        (i / size as usize) as f32 <= front_y(&b, (i % size as usize) as f32 + 0.5)
                    );
                }
            }
            assert!(whites > 250, "{}: {whites}", face.name());
            assert_eq!(px[3], 0);
            assert_eq!(px[px.len() - 1], 0);
            assert!(!frames.contains(&px));
            frames.push(px);
        }
    }
    #[test]
    fn theme_changes_orbit_without_tinting_the_letter() {
        for signal in [
            [0.95, 0.8, 0.2, 1.0],
            [0.02, 0.04, 0.08, 1.0],
            [0.1, 0.6, 0.9, 1.0],
        ] {
            let px = render(64, signal, Face::Newsreader);
            assert!(px.chunks_exact(4).any(|p| p == [255, 255, 255, 255]));
            assert!(
                px.chunks_exact(4)
                    .any(|p| p[3] > 100 && p[0] < 20 && p[1] < 20 && p[2] < 20),
                "contrast edge"
            );
        }
    }
    #[test]
    fn promo_timing_settles_and_never_loops() {
        for (i, face) in Face::ALL.into_iter().enumerate() {
            assert_eq!(face_at(i as f32 * STEP_SECONDS + 0.001), face);
        }
        assert_eq!(face_at(20.0), Face::Newsreader);
    }
    #[test]
    fn launch_repeats_the_complete_clipped_sequence() {
        for cycle in 0..5 {
            for (i, face) in Face::ALL.into_iter().enumerate() {
                let time = (cycle * Face::ALL.len() + i) as f32 * STEP_SECONDS + 0.001;
                assert_eq!(launch_face_at(time), face);
            }
        }
        assert_eq!(launch_face_at(-1.0), Face::Plex);
    }
}
