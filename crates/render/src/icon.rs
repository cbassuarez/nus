//! The app icon: the wordmark's own italic n with a thin band in orbit —
//! behind the stem, in front of the bowl. The n takes the base colour, the
//! band the signal, so the icon follows the surface. Transparent ground.

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

use crate::Color;

/// Straight-alpha RGBA, `size`×`size`.
pub fn app_icon(size: u32, n_color: Color, band: Color) -> Vec<u8> {
    app_icon_at(size, n_color, band, 1.0)
}

/// The same, with the swoosh drawn only `progress` (0..1) of the way from
/// its start — the splash draws it in. One-off; anything drawing frames
/// keeps an [`IconField`].
pub fn app_icon_at(size: u32, n_color: Color, band: Color, progress: f32) -> Vec<u8> {
    IconField::new(size).frame(n_color, band, progress)
}

/// The icon sampled once per pixel — where each pixel of the band sits
/// along it and how far off its centreline, and the n's coverage — so a
/// frame at any progress is one cheap pass over the band's pixels. The
/// splash and the plate draw the band in from this; the taskbar icon is
/// its last frame.
pub struct IconField {
    size: u32,
    band: Band,
    /// The band's candidate pixels: index, radians along the band from its
    /// start, signed distance from the centreline, and whether the pixel
    /// is on the near half (drawn over the n).
    strip: Vec<(u32, f32, f32, bool)>,
    /// The n's coverage, and where its bitmap sits in the frame.
    glyph: Mask,
    gx: i32,
    gy: i32,
}

impl IconField {
    pub fn new(size: u32) -> IconField {
        let s = size as f32;
        let band = Band::in_frame(s);
        let (cos, sin) = (band.tilt.cos(), band.tilt.sin());
        let (a, b) = (band.a, band.b);
        // Nothing thicker than the base stroke is ever drawn; keep a pixel
        // of slack for the anti-aliased edge.
        let reach = band.base_t / 2.0 + 1.0;
        let mut strip = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 + 0.5 - band.cx, y as f32 + 0.5 - band.cy);
                // Into the ellipse frame.
                let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                let r = ((u / a).powi(2) + (v / b).powi(2)).sqrt();
                // Distance to the ellipse via the gradient magnitude.
                let g = ((u / (a * a)).powi(2) + (v / (b * b)).powi(2))
                    .sqrt()
                    .max(1e-6)
                    / r.max(1e-6);
                let d = (r - 1.0) / g;
                if d.abs() > reach {
                    continue;
                }
                let th = (v / b).atan2(u / a);
                let along = (th - band.start).rem_euclid(std::f32::consts::TAU);
                strip.push((y * size + x, along, d, v > 0.0));
            }
        }
        let glyph = raster_n(s * 1.38);
        let (gw, gh) = (glyph.w as f32, glyph.h as f32);
        let gx = ((s - gw) / 2.0 + s * 0.01).round() as i32;
        let gy = ((s - gh) / 2.0 + s * 0.02).round() as i32;
        IconField {
            size,
            band,
            strip,
            glyph,
            gx,
            gy,
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Straight-alpha RGBA, the band drawn `progress` (0..1) of the way
    /// from its start: the back half, the n, the front half.
    pub fn frame(&self, n_color: Color, band: Color, progress: f32) -> Vec<u8> {
        let size = self.size;
        let mut px = vec![0.0f32; (size * size * 4) as usize];
        let drawn = self.band.span * progress.clamp(0.0, 1.0);
        let blend = |px: &mut [f32], i: usize, c: Color, cov: f32| {
            if cov <= 0.0 {
                return;
            }
            let a_src = c[3] * cov;
            let a_dst = px[i + 3];
            let a_out = a_src + a_dst * (1.0 - a_src);
            if a_out <= 0.0 {
                return;
            }
            for k in 0..3 {
                px[i + k] = (c[k] * a_src + px[i + k] * a_dst * (1.0 - a_src)) / a_out;
            }
            px[i + 3] = a_out;
        };
        let cover = |along: f32, d: f32| -> f32 {
            if along > drawn {
                return 0.0;
            }
            let thick = self.band.thickness(along, drawn);
            (thick / 2.0 - d.abs() + 0.5).clamp(0.0, 1.0)
        };
        for &(i, along, d, front) in &self.strip {
            if !front {
                blend(&mut px, i as usize * 4, band, cover(along, d));
            }
        }
        for y in 0..self.glyph.h {
            for x in 0..self.glyph.w {
                let cov = self.glyph.data[(y * self.glyph.w + x) as usize] as f32 / 255.0;
                let (ox, oy) = (self.gx + x as i32, self.gy + y as i32);
                if ox >= 0 && oy >= 0 && (ox as u32) < size && (oy as u32) < size {
                    let i = ((oy as u32 * size + ox as u32) * 4) as usize;
                    blend(&mut px, i, n_color, cov);
                }
            }
        }
        for &(i, along, d, front) in &self.strip {
            if front {
                blend(&mut px, i as usize * 4, band, cover(along, d));
            }
        }
        px.iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect()
    }
}

/// The band's geometry, shared by the raster and the vector: where the
/// swoosh runs, how it swells, and where it sits in a `size` frame.
pub struct Band {
    pub cx: f32,
    pub cy: f32,
    pub a: f32,
    pub b: f32,
    pub tilt: f32,
    pub base_t: f32,
    pub start: f32,
    pub span: f32,
}

impl Band {
    pub fn in_frame(size: f32) -> Band {
        let s = size;
        let gap_center = 0.12f32;
        let gap = 0.9f32;
        Band {
            cx: s * 0.5,
            cy: s * 0.54,
            a: s * 0.49,
            b: s * 0.16,
            tilt: -24.0f32.to_radians(),
            base_t: if s <= 32.0 { s * 0.085 } else { s * 0.05 },
            start: gap_center + gap / 2.0,
            span: std::f32::consts::TAU - gap,
        }
    }

    /// A point on the band's centreline at parametric angle `th`, in the frame.
    pub fn at(&self, th: f32) -> (f32, f32) {
        let (u, v) = (self.a * th.cos(), self.b * th.sin());
        let (cos, sin) = (self.tilt.cos(), self.tilt.sin());
        (self.cx + u * cos - v * sin, self.cy + u * sin + v * cos)
    }

    /// The band's thickness `along` radians from its start, with `drawn`
    /// radians of it drawn so far (the swoosh tapers to both ends).
    pub fn thickness(&self, along: f32, drawn: f32) -> f32 {
        let ends = (along / 0.55).min((drawn - along) / 0.55).clamp(0.0, 1.0);
        let swell = 0.55
            + 0.45 * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * along / self.span + 0.6).cos());
        self.base_t * swell * (0.15 + 0.85 * ends)
    }

    /// The outward normal at `th`, in the frame.
    pub fn normal(&self, th: f32) -> (f32, f32) {
        let (u, v) = (self.a * th.cos(), self.b * th.sin());
        let (gu, gv) = (u / (self.a * self.a), v / (self.b * self.b));
        let n = (gu * gu + gv * gv).sqrt().max(1e-6);
        let (gu, gv) = (gu / n, gv / n);
        let (cos, sin) = (self.tilt.cos(), self.tilt.sin());
        (gu * cos - gv * sin, gu * sin + gv * cos)
    }

    /// The band as a filled outline between parametric angles `from` and `to`.
    fn outline(&self, from: f32, to: f32, steps: usize) -> String {
        let drawn = self.span;
        let mut outer = Vec::with_capacity(steps + 1);
        let mut inner = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let th = from + (to - from) * i as f32 / steps as f32;
            let (x, y) = self.at(th);
            let (nx, ny) = self.normal(th);
            let t =
                self.thickness((th - self.start).rem_euclid(std::f32::consts::TAU), drawn) / 2.0;
            outer.push((x + nx * t, y + ny * t));
            inner.push((x - nx * t, y - ny * t));
        }
        let mut d = String::new();
        for (i, (x, y)) in outer.iter().enumerate() {
            d.push_str(&format!("{}{x:.2} {y:.2} ", if i == 0 { "M" } else { "L" }));
        }
        for (x, y) in inner.iter().rev() {
            d.push_str(&format!("L{x:.2} {y:.2} "));
        }
        d.push('Z');
        d
    }
}

/// Parametric angles along the band where `n` stops sit clear of the n
/// glyph (a margin of ink-free frame around each), spread over the clear
/// stretches of the band — the longer stretches take more. In band order.
pub fn band_stops(size: f32, n: usize) -> Vec<f32> {
    let s = size;
    let band = Band::in_frame(s);
    let glyph = raster_n(s * 1.38);
    let (gw, gh) = (glyph.w as f32, glyph.h as f32);
    let gx = ((s - gw) / 2.0 + s * 0.01).round();
    let gy = ((s - gh) / 2.0 + s * 0.02).round();
    let margin = (s * 0.055).max(3.0);
    let inked = |x: f32, y: f32| -> bool {
        let (x0, y0) = (
            (x - margin - gx).floor() as i32,
            (y - margin - gy).floor() as i32,
        );
        let (x1, y1) = (
            (x + margin - gx).ceil() as i32,
            (y + margin - gy).ceil() as i32,
        );
        for yy in y0.max(0)..y1.min(glyph.h as i32) {
            for xx in x0.max(0)..x1.min(glyph.w as i32) {
                if glyph.data[(yy as u32 * glyph.w + xx as u32) as usize] > 96 {
                    return true;
                }
            }
        }
        false
    };
    // Clear stretches of the band, away from its thin ends.
    let mut stretches: Vec<(f32, f32)> = Vec::new();
    let mut th = band.start + 0.12;
    let end = band.start + band.span - 0.05;
    while th <= end {
        let (x, y) = band.at(th);
        if !inked(x, y) {
            match stretches.last_mut() {
                Some(last) if th - last.1 < 0.02 => last.1 = th,
                _ => stretches.push((th, th)),
            }
        }
        th += 0.01;
    }
    let usable: Vec<(f32, f32)> = stretches.into_iter().filter(|r| r.1 - r.0 > 0.12).collect();
    if usable.is_empty() {
        return (0..n)
            .map(|k| band.start + band.span * (k as f32 + 0.5) / n as f32)
            .collect();
    }
    let mut counts = vec![1usize; usable.len()];
    let mut placed = usable.len();
    while placed < n {
        let mut best = 0;
        for i in 0..usable.len() {
            let room = |i: usize| (usable[i].1 - usable[i].0) / (counts[i] + 1) as f32;
            if room(i) > room(best) {
                best = i;
            }
        }
        counts[best] += 1;
        placed += 1;
    }
    let mut out = Vec::with_capacity(n);
    for (i, r) in usable.iter().enumerate() {
        for j in 0..counts[i] {
            out.push(r.0 + (r.1 - r.0) * (j as f32 + 0.5) / counts[i] as f32);
        }
    }
    out.truncate(n);
    out
}

/// The icon as SVG, `size` units square: the n's outline and the band as
/// two filled paths — the half behind the n and the half in front — in the
/// same geometry as the raster. `n_color` and `band` are CSS colours.
pub fn app_icon_svg(size: f32, n_color: &str, band: &str) -> String {
    let parts = app_icon_paths(size);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" width=\"{size}\" height=\"{size}\">\
<path d=\"{}\" fill=\"{band}\"/><path d=\"{}\" fill=\"{n_color}\"/><path d=\"{}\" fill=\"{band}\"/></svg>",
        parts.back, parts.n, parts.front
    )
}

/// The icon's three paths (`d` attributes) in a `size` frame, back to front.
pub struct IconPaths {
    pub back: String,
    pub n: String,
    pub front: String,
    pub band: Band,
}

pub fn app_icon_paths(size: f32) -> IconPaths {
    let s = size;
    let band = Band::in_frame(s);
    // The n: the outline at the raster's size, y flipped, its box centred
    // the way the raster centres its bitmap.
    let font =
        FontRef::from_index(crate::text::bundled::NEWSREADER_ITALIC, 0).expect("bundled font");
    let id = font.charmap().map('n');
    let mut ctx = ScaleContext::new();
    let mut scaler = ctx.builder(font).size(s * 1.38).hint(false).build();
    let outline = scaler.scale_outline(id).unwrap_or_default();
    let bounds = outline.bounds();
    let (gw, gh) = (bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y);
    let gx = ((s - gw) / 2.0 + s * 0.01).round();
    let gy = ((s - gh) / 2.0 + s * 0.02).round();
    let tx = |x: f32| x - bounds.min.x + gx;
    let ty = |y: f32| bounds.max.y - y + gy;
    use swash::zeno::{Command, PathData};
    let mut n = String::new();
    for c in outline.path().commands() {
        match c {
            Command::MoveTo(p) => n.push_str(&format!("M{:.2} {:.2} ", tx(p.x), ty(p.y))),
            Command::LineTo(p) => n.push_str(&format!("L{:.2} {:.2} ", tx(p.x), ty(p.y))),
            Command::QuadTo(c, p) => n.push_str(&format!(
                "Q{:.2} {:.2} {:.2} {:.2} ",
                tx(c.x),
                ty(c.y),
                tx(p.x),
                ty(p.y)
            )),
            Command::CurveTo(c1, c2, p) => n.push_str(&format!(
                "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
                tx(c1.x),
                ty(c1.y),
                tx(c2.x),
                ty(c2.y),
                tx(p.x),
                ty(p.y)
            )),
            Command::Close => n.push_str("Z "),
        }
    }
    // The near half (v > 0, angles up to π) is drawn over the n; the rest behind.
    let pi = std::f32::consts::PI;
    let end = band.start + band.span;
    let front = band.outline(band.start, pi, 180);
    let back = band.outline(pi, end, 260);
    IconPaths {
        back,
        n: n.trim_end().to_string(),
        front,
        band,
    }
}

struct Mask {
    w: u32,
    h: u32,
    data: Vec<u8>,
}

fn raster_n(px: f32) -> Mask {
    let font =
        FontRef::from_index(crate::text::bundled::NEWSREADER_ITALIC, 0).expect("bundled font");
    let id = font.charmap().map('n');
    let mut ctx = ScaleContext::new();
    let mut scaler = ctx.builder(font).size(px).hint(false).build();
    let img = Render::new(&[Source::Outline])
        .format(Format::Alpha)
        .render(&mut scaler, id);
    match img {
        Some(img) if img.placement.width > 0 => Mask {
            w: img.placement.width,
            h: img.placement.height,
            data: img.data,
        },
        _ => Mask {
            w: 1,
            h: 1,
            data: vec![0],
        },
    }
}

/// PNG bytes for an RGBA image (stored, zlib level 0 — icons are small).
pub fn png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for row in rgba.chunks((w * 4) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    chunk(&mut out, b"IDAT", &zlib_store(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut c = Vec::with_capacity(4 + data.len());
    c.extend_from_slice(name);
    c.extend_from_slice(data);
    out.extend_from_slice(&c);
    out.extend_from_slice(&crc32(&c).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        out.extend_from_slice(&(c.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        out.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &d in data {
        a = (a + d as u32) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// A Windows .ico holding PNG entries for each (size, png).
pub fn ico(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len() as u32;
    for (size, png) in entries {
        let s = if *size >= 256 { 0u8 } else { *size as u8 };
        out.extend_from_slice(&[s, s, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in entries {
        out.extend_from_slice(png);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_ink_and_band() {
        let px = app_icon(64, [0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]);
        let opaque = px.chunks(4).filter(|p| p[3] > 200).count();
        assert!(opaque > 200 && opaque < 64 * 64 / 2, "{opaque}");
        let red = px
            .chunks(4)
            .filter(|p| p[0] > 200 && p[1] < 60 && p[3] > 200)
            .count();
        assert!(red > 50, "{red}");
        // Corners stay transparent.
        assert_eq!(px[3], 0);
    }

    #[test]
    fn svg_has_three_paths() {
        let svg = app_icon_svg(512.0, "#141413", "#c8102e");
        assert_eq!(svg.matches("<path ").count(), 3);
        let parts = app_icon_paths(512.0);
        assert!(parts.n.starts_with('M') && parts.n.contains('C') || parts.n.contains('Q'));
        // The band lies inside the frame.
        for d in [&parts.back, &parts.front] {
            for num in d
                .split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                .filter(|t| !t.is_empty())
            {
                let v: f32 = num.parse().unwrap();
                assert!((-40.0..=552.0).contains(&v), "{v}");
            }
        }
    }

    #[test]
    fn stops_sit_clear() {
        let stops = band_stops(256.0, 4);
        assert_eq!(stops.len(), 4);
        let band = Band::in_frame(256.0);
        for w in stops.windows(2) {
            assert!(w[1] > w[0]);
        }
        assert!(stops[0] > band.start && stops[3] < band.start + band.span);
    }

    #[test]
    fn field_draws_in() {
        let field = IconField::new(64);
        let red = |px: &[u8]| {
            px.chunks(4)
                .filter(|p| p[0] > 200 && p[1] < 60 && p[3] > 200)
                .count()
        };
        let none = red(&field.frame([0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], 0.0));
        let half = red(&field.frame([0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], 0.5));
        let full = red(&field.frame([0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], 1.0));
        assert!(
            none == 0 && none < half && half < full,
            "{none} {half} {full}"
        );
        assert_eq!(
            full,
            red(&app_icon(64, [0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]))
        );
    }

    #[test]
    fn png_roundtrip_header() {
        let p = png(&[255, 0, 0, 255], 1, 1);
        assert_eq!(&p[1..4], b"PNG");
        let i = ico(&[(1, p)]);
        assert_eq!(&i[..4], &[0, 0, 1, 0]);
    }
}
