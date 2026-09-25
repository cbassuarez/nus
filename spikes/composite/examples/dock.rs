//! Render exactly the images used by the desktop icon; no GPU/window required.
use nus_render::{
    dock_icon::{self, Face},
    icon::png,
    theme::hex,
};
fn main() {
    let dir = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&dir).unwrap();
    let start = std::time::Instant::now();
    for (i, face) in Face::ALL.into_iter().enumerate() {
        let rgba = dock_icon::render(256, hex(0xc8102e), face);
        std::fs::write(format!("{dir}/face-{i}.png"), png(&rgba, 256, 256)).unwrap();
    }
    println!("six frames: {:?}", start.elapsed());
    let size = 160u32;
    let (w, h) = (size * 6, size * 3);
    let mut sheet = vec![0; (w * h * 4) as usize];
    for row in 0..3 {
        for col in 0..6 {
            let bg = if row == 0 {
                [245u8, 245, 245]
            } else if row == 1 {
                [24, 26, 31]
            } else {
                [146, 143, 134]
            };
            let signal = if row == 0 {
                hex(0xc8102e)
            } else if row == 1 {
                hex(0x76a9d1)
            } else {
                hex([0xc8102e, 0xe3b341, 0x8fe388, 0x0c172b, 0xffffff, 0xa982d0][col as usize])
            };
            let face = if row == 2 {
                Face::Newsreader
            } else {
                Face::ALL[col as usize]
            };
            let rgba = dock_icon::render(size, signal, face);
            for y in 0..size {
                for x in 0..size {
                    let a = ((y * size + x) * 4) as usize;
                    let b = (((row * size + y) * w + col * size + x) * 4) as usize;
                    let alpha = rgba[a + 3] as f32 / 255.0;
                    for k in 0..3 {
                        sheet[b + k] = (rgba[a + k] as f32 * alpha + bg[k] as f32 * (1.0 - alpha))
                            .round() as u8;
                    }
                    sheet[b + 3] = 255;
                }
            }
        }
    }
    std::fs::write(format!("{dir}/contact.png"), png(&sheet, w, h)).unwrap();
    let mut entries=Vec::new();
    for size in [16, 24, 32, 48, 64, 128, 256, 512, 1024] {
        let rgba = dock_icon::render(size, hex(0xc8102e), Face::Newsreader);
        let bytes=png(&rgba,size,size);
        std::fs::write(format!("{dir}/nus-{size}.png"),&bytes).unwrap();
        std::fs::write(format!("{dir}/nus-{size}-ink.png"), &bytes).unwrap();
        if size<=256 {entries.push((size,bytes));}
    }
    std::fs::write(format!("{dir}/nus.ico"),nus_render::icon::ico(&entries)).unwrap();
    incognito_sheet(&dir);
}

/// The incognito (redacted) mark beside the regular one, at Dock sizes on
/// light, dark and mid desktops; the small sizes again at 4×, pixel for pixel.
fn incognito_sheet(dir: &str) {
    let sizes = [16u32, 32, 64, 128, 256];
    let grounds = [[245u8, 245, 245], [24, 26, 31], [146, 143, 134]];
    let cell = 272u32;
    let (w, h) = (cell * sizes.len() as u32, cell * 6);
    let mut sheet = vec![255u8; (w * h * 4) as usize];
    let mut put = |rgba: &[u8], size: u32, scale: u32, ox: u32, oy: u32, bg: [u8; 3], sheet: &mut Vec<u8>| {
        for y in 0..cell {
            for x in 0..cell {
                let b = (((oy + y) * w + ox + x) * 4) as usize;
                sheet[b..b + 3].copy_from_slice(&bg);
            }
        }
        let side = size * scale;
        let (px, py) = (ox + (cell - side) / 2, oy + (cell - side) / 2);
        for y in 0..side {
            for x in 0..side {
                let a = (((y / scale) * size + x / scale) * 4) as usize;
                let b = (((py + y) * w + px + x) * 4) as usize;
                let alpha = rgba[a + 3] as f32 / 255.0;
                for k in 0..3 {
                    sheet[b + k] = (rgba[a + k] as f32 * alpha + bg[k] as f32 * (1.0 - alpha)).round() as u8;
                }
            }
        }
    };
    for (row, bg) in grounds.iter().enumerate() {
        for (col, &size) in sizes.iter().enumerate() {
            let scale = if size <= 64 { 4 } else { 1 };
            let normal = dock_icon::render(size, hex(0xc8102e), Face::Newsreader);
            let redacted = dock_icon::redacted(size);
            put(&normal, size, scale, col as u32 * cell, row as u32 * 2 * cell, *bg, &mut sheet);
            put(&redacted, size, scale, col as u32 * cell, (row as u32 * 2 + 1) * cell, *bg, &mut sheet);
        }
    }
    std::fs::write(format!("{dir}/incognito-sheet.png"), png(&sheet, w, h)).unwrap();
    for size in [16u32, 32, 64, 128, 256, 512, 1024] {
        std::fs::write(format!("{dir}/incognito-{size}.png"), png(&dock_icon::redacted(size), size, size)).unwrap();
    }
}
