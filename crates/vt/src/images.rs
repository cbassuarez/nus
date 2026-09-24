//! Terminal images: the Kitty graphics protocol (transmit + display,
//! PNG or raw RGB/RGBA, chunked, delete) and iTerm2's inline images
//! (OSC 1337 File=inline=1). Images are decoded here to RGBA; the host
//! uploads them once and draws placements at their absolute lines.

/// Per-terminal retained RGBA budget and per-transmission encoded budget.
pub const MAX_RGBA_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;

fn pixels(w: u32, h: u32) -> Option<usize> {
    let n = (w as usize).checked_mul(h as usize)?;
    (n > 0 && n <= MAX_RGBA_BYTES / 4).then_some(n)
}

/// A decoded image.
#[derive(Clone, Debug)]
pub struct Image {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA, row-major.
    pub rgba: Vec<u8>,
}

/// An image shown at a cell, spanning `cols` × `rows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub image: u32,
    /// Absolute line of the top-left cell.
    pub line: u64,
    pub col: usize,
    pub cols: usize,
    pub rows: usize,
}

/// A Kitty transmission in progress (chunked payloads).
#[derive(Clone, Debug, Default)]
pub struct Pending {
    pub id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub display: bool,
    pub cols: usize,
    pub rows: usize,
    pub quiet: u8,
}

/// Parse the control string of a Kitty graphics command into pairs.
pub fn kitty_controls(s: &str) -> Vec<(char, String)> {
    s.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            let k = k.chars().next()?;
            Some((k, v.trim().to_string()))
        })
        .collect()
}

/// A numeric control, or the default.
pub fn control_num(c: &[(char, String)], key: char, default: i64) -> i64 {
    c.iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(default)
}

pub fn control_str(c: &[(char, String)], key: char) -> Option<&str> {
    c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
}

/// Standard base64 (with or without padding) to bytes.
pub fn base64_decode(s: &[u8]) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in s {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

/// Decode PNG bytes to RGBA.
pub fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: MAX_RGBA_BYTES,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let n = pixels(info.width, info.height)?;
    let size = reader.output_buffer_size();
    if size > MAX_RGBA_BYTES {
        return None;
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).ok()?;
    let buf = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf.to_vec(),
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => return None,
    };
    if rgba.len() != n * 4 {
        return None;
    }
    Some((info.width, info.height, rgba))
}

/// Raw RGB (24) or RGBA (32) payloads; check dimensions before allocation.
pub fn decode_raw(format: u32, w: u32, h: u32, data: &[u8]) -> Option<Vec<u8>> {
    let n = pixels(w, h)?;
    match format {
        32 if data.len() >= n * 4 => Some(data[..n * 4].to_vec()),
        24 if data.len() >= n * 3 => Some(
            data[..n * 3]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
        ),
        _ => None,
    }
}

/// iTerm2 `File=` arguments: name=…;width=…;height=…;inline=1.
pub fn iterm_args(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// An iTerm2 size ("auto", "12" cells, "200px", "50%") in cells, given
/// the cell size and the image's pixel size along that axis.
pub fn iterm_size(
    spec: Option<&str>,
    image_px: u32,
    cell_px: u16,
    screen_cells: usize,
) -> Option<usize> {
    let spec = spec?.trim();
    if spec == "auto" || spec.is_empty() {
        return None;
    }
    if let Some(px) = spec.strip_suffix("px") {
        let px: f32 = px.parse().ok()?;
        return Some((px / cell_px.max(1) as f32).ceil().max(1.0) as usize);
    }
    if let Some(pc) = spec.strip_suffix('%') {
        let pc: f32 = pc.parse().ok()?;
        return Some(((pc / 100.0) * screen_cells as f32).ceil().max(1.0) as usize);
    }
    let _ = image_px;
    spec.parse::<usize>().ok().map(|c| c.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        assert_eq!(base64_decode(b"aGVsbG8="), b"hello");
        assert_eq!(base64_decode(b"aGVsbG8"), b"hello");
    }

    #[test]
    fn controls_parse() {
        let c = kitty_controls("a=T,f=100,s=2,v=2,i=7,m=0");
        assert_eq!(control_str(&c, 'a'), Some("T"));
        assert_eq!(control_num(&c, 'i', 0), 7);
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn raw_dimensions_do_not_overflow_or_allocate_past_budget() {
        assert!(decode_raw(32, u32::MAX, u32::MAX, &[0; 4]).is_none());
        assert!(decode_raw(24, 0, 1, &[]).is_none());
        assert!(decode_raw(32, 8192, 8192, &[]).is_none());
        assert_eq!(decode_raw(24, 1, 1, &[1, 2, 3]), Some(vec![1, 2, 3, 255]));
    }
    #[test]
    fn png_expansion_checks_dimensions_and_handles_sixteen_bit_samples() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Sixteen);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[1, 0, 2, 0, 3, 0, 255, 255])
                .unwrap();
        }
        assert_eq!(decode_png(&bytes), Some((1, 1, vec![1, 2, 3, 255])));
        let mut huge = Vec::new();
        let writer = png::Encoder::new(&mut huge, 8192, 8192)
            .write_header()
            .unwrap();
        drop(writer);
        assert!(decode_png(&huge).is_none());
    }
}
