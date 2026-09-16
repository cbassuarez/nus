//! The app icon: the wordmark's own italic n with a thin band in orbit —
//! behind the stem, in front of the bowl. The n takes the base colour, the
//! band the signal, so the icon follows the surface. Transparent ground.

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

use crate::Color;

/// Straight-alpha RGBA, `size`×`size`.
pub fn app_icon(size: u32, n_color: Color, band: Color) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0.0f32; (size * size * 4) as usize];

    // The n, from Newsreader Italic, centred and sized to the frame.
    let glyph = raster_n(s * 1.22);
    let (gw, gh) = (glyph.w as f32, glyph.h as f32);
    let gx = ((s - gw) / 2.0 + s * 0.01).round();
    let gy = ((s - gh) / 2.0 + s * 0.02).round();

    // The band: an ellipse tilted 22°, thin, passing through the glyph.
    let (cx, cy) = (s * 0.5, s * 0.53);
    let (a, b) = (s * 0.485, s * 0.155);
    let tilt = -24.0f32.to_radians();
    let (cos, sin) = (tilt.cos(), tilt.sin());
    // Small sizes need a heavier band to survive the taskbar.
    let thick = if s <= 32.0 { s * 0.08 } else { s * 0.048 };
    let band_at = |x: f32, y: f32| -> (f32, bool) {
        // Into the ellipse frame.
        let (dx, dy) = (x - cx, y - cy);
        let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
        let r = ((u / a).powi(2) + (v / b).powi(2)).sqrt();
        // Distance to the ellipse via the gradient magnitude.
        let g = ((u / (a * a)).powi(2) + (v / (b * b)).powi(2))
            .sqrt()
            .max(1e-6)
            / r.max(1e-6);
        let d = (r - 1.0) / g;
        let cov = (thick / 2.0 - d.abs() + 0.5).clamp(0.0, 1.0);
        (cov, v > 0.0) // v > 0: the near half, drawn over the n.
    };

    let blend = |px: &mut [f32], x: u32, y: u32, c: Color, cov: f32| {
        if cov <= 0.0 {
            return;
        }
        let i = ((y * size + x) * 4) as usize;
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

    // Back half of the band, then the n, then the front half.
    for y in 0..size {
        for x in 0..size {
            let (cov, front) = band_at(x as f32 + 0.5, y as f32 + 0.5);
            if !front {
                blend(&mut px, x, y, band, cov);
            }
        }
    }
    for y in 0..glyph.h {
        for x in 0..glyph.w {
            let cov = glyph.data[(y * glyph.w + x) as usize] as f32 / 255.0;
            let (ox, oy) = (gx as i32 + x as i32, gy as i32 + y as i32);
            if ox >= 0 && oy >= 0 && (ox as u32) < size && (oy as u32) < size {
                blend(&mut px, ox as u32, oy as u32, n_color, cov);
            }
        }
    }
    for y in 0..size {
        for x in 0..size {
            let (cov, front) = band_at(x as f32 + 0.5, y as f32 + 0.5);
            if front {
                blend(&mut px, x, y, band, cov);
            }
        }
    }
    px.iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect()
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
    fn png_roundtrip_header() {
        let p = png(&[255, 0, 0, 255], 1, 1);
        assert_eq!(&p[1..4], b"PNG");
        let i = ico(&[(1, p)]);
        assert_eq!(&i[..4], &[0, 0, 1, 0]);
    }
}
