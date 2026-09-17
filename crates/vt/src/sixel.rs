//! Sixel (DEC, VT340): `DCS P1;P2;P3 q <data> ST`. Decoded here into an
//! RGBA image the host draws like any other placement. The data is a
//! stream of six-pixel-tall columns: `"` sets the raster (pan;pad;ph;pv),
//! `#Pc;Pu;Px;Py;Pz` defines a colour (RGB or HLS, 0–100), `#Pc` picks
//! one, `!n c` repeats a sixel, `$` returns to the row's start, `-` moves
//! to the next six-pixel band, and bytes 63–126 are the sixels themselves.

/// A decoded sixel picture: RGBA, plus the transparency the DCS asked for.
pub struct Sixel {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The VT340's sixteen default colours (RGB percentages).
const DEFAULT: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (20, 20, 80),
    (80, 13, 13),
    (20, 80, 20),
    (80, 20, 80),
    (20, 80, 80),
    (80, 80, 20),
    (53, 53, 53),
    (26, 26, 26),
    (33, 33, 60),
    (60, 26, 26),
    (33, 60, 33),
    (60, 33, 60),
    (33, 60, 60),
    (60, 60, 33),
    (80, 80, 80),
];

fn pct(v: u32) -> u8 {
    ((v.min(100) * 255) / 100) as u8
}

/// HLS as DEC has it: hue 0–360 with blue at 0, lightness and saturation 0–100.
fn hls(h: u32, l: u32, s: u32) -> (u8, u8, u8) {
    let h = ((h as f32 + 240.0) % 360.0) / 60.0;
    let l = l.min(100) as f32 / 100.0;
    let s = s.min(100) as f32 / 100.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let q = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (q(r), q(g), q(b))
}

/// Decode a sixel payload (the bytes between `q` and ST). `params` are
/// the DCS parameters; P2 = 1 leaves unset pixels transparent.
pub fn decode(params: &[u32], data: &[u8]) -> Option<Sixel> {
    let transparent = params.get(1).copied().unwrap_or(0) == 1;
    // First pass: the size. Sixel pictures often omit the raster
    // attributes, so the bands and the longest row decide.
    let mut palette: Vec<(u8, u8, u8)> = DEFAULT.to_vec();
    palette.resize(256, (0, 0, 0));
    let (mut width, mut height) = (0usize, 0usize);
    let (mut x, mut band) = (0usize, 0usize);
    let mut i = 0;
    let nums = |i: &mut usize| -> Vec<u32> {
        let mut v = Vec::new();
        loop {
            let start = *i;
            while *i < data.len() && data[*i].is_ascii_digit() {
                *i += 1;
            }
            v.push(
                std::str::from_utf8(&data[start..*i])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            );
            if *i < data.len() && data[*i] == b';' {
                *i += 1;
            } else {
                break;
            }
        }
        v
    };
    let mut ops: Vec<(usize, usize, u8, u32)> = Vec::new(); // (x, band, colour, sixel) after repeats
    let mut colour: u8 = 0;
    while i < data.len() {
        let b = data[i];
        i += 1;
        match b {
            b'"' => {
                let v = nums(&mut i);
                if v.len() >= 4 {
                    width = width.max(v[2] as usize);
                    height = height.max(v[3] as usize);
                }
            }
            b'#' => {
                let v = nums(&mut i);
                let Some(&c) = v.first() else { continue };
                let c = (c as usize).min(255);
                if v.len() >= 5 {
                    let rgb = match v[1] {
                        2 => (pct(v[2]), pct(v[3]), pct(v[4])),
                        _ => hls(v[2], v[3], v[4]),
                    };
                    palette[c] = rgb;
                }
                colour = c as u8;
            }
            b'!' => {
                let v = nums(&mut i);
                let n = v.first().copied().unwrap_or(1).max(1) as usize;
                if i < data.len() {
                    let ch = data[i];
                    i += 1;
                    if (63..=126).contains(&ch) {
                        ops.push((x, band, colour, ((ch - 63) as u32) | ((n as u32) << 8)));
                        x += n;
                        width = width.max(x);
                    }
                }
            }
            b'$' => x = 0,
            b'-' => {
                band += 1;
                x = 0;
            }
            63..=126 => {
                ops.push((x, band, colour, ((b - 63) as u32) | (1 << 8)));
                x += 1;
                width = width.max(x);
            }
            _ => {}
        }
    }
    let bands = ops.iter().map(|o| o.1).max().map(|b| b + 1).unwrap_or(0);
    height = height.max(bands * 6);
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    let mut rgba = vec![0u8; width * height * 4];
    if !transparent {
        // The background: colour 0 as it ended up (usually black).
        let (r, g, b) = palette[0];
        for px in rgba.chunks_mut(4) {
            px.copy_from_slice(&[r, g, b, 255]);
        }
    }
    for (x0, band, c, packed) in ops {
        let bits = packed & 63;
        let n = (packed >> 8) as usize;
        let (r, g, b) = palette[c as usize];
        for k in 0..6 {
            if bits & (1 << k) == 0 {
                continue;
            }
            let y = band * 6 + k;
            if y >= height {
                continue;
            }
            for x in x0..(x0 + n).min(width) {
                let o = (y * width + x) * 4;
                rgba[o..o + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    Some(Sixel {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_colour_strip() {
        // 3px wide: colour 1 red for two columns of a full sixel, then colour 2 blue once.
        let data = b"#1;2;100;0;0#2;2;0;0;100#1!2~#2~";
        let s = decode(&[0, 1], data).unwrap();
        assert_eq!((s.width, s.height), (3, 6));
        assert_eq!(&s.rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&s.rgba[8..12], &[0, 0, 255, 255]);
    }

    #[test]
    fn bands_and_raster() {
        let data = b"\"1;1;4;12#0;2;0;0;0#3;2;0;100;0~~~~-~~~~";
        let s = decode(&[0, 0], data).unwrap();
        assert_eq!((s.width, s.height), (4, 12));
        // Bottom band, first column, green.
        let o = (6 * 4) * 4;
        assert_eq!(&s.rgba[o..o + 3], &[0, 255, 0]);
    }

    #[test]
    fn hls_blue_is_zero_degrees() {
        assert_eq!(hls(0, 50, 100), (0, 0, 255));
        assert_eq!(hls(120, 50, 100), (255, 0, 0));
    }
}
