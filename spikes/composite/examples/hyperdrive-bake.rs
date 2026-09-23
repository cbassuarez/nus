//! cargo run --release --manifest-path spikes/composite/Cargo.toml --example hyperdrive-bake
#[path = "support/hyperdrive_motion.rs"]
mod motion;
use nus_render::Rect;
use std::{io::Write, path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/arrival");
    std::fs::create_dir_all(&root).unwrap();
    let rgba = nus_render::icon::app_icon_at(512, [1.0; 4], [0.0; 4], 0.0);
    let mut enc = png::Encoder::new(std::fs::File::create(root.join("n.png")).unwrap(), 512, 512);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    let mut targets = Vec::new();
    for y in (0..512).step_by(6) {
        for x in (0..512).step_by(6) {
            if rgba[(y * 512 + x) * 4 + 3] > 180 {
                targets.push([x as f32 / 512.0, y as f32 / 512.0]);
            }
        }
    }
    const FPS: usize = 120;
    const BACKGROUND: usize = 420;
    let frames = (motion::ORBIT * FPS as f32).ceil() as usize + 1;
    let count = BACKGROUND + targets.len();
    let card = Rect::new(0.0, 0.0, 580.0, 380.0);
    let size = card.h * 0.57;
    let mark = Rect::new(
        card.w * 0.5 - size * 0.5,
        card.h * 0.46 - size * 0.5,
        size,
        size,
    );
    let mut data = Vec::new();
    // Immutable dot radius and line width in logical pixels.
    for i in 0..count {
        let (radius, width) = if i < BACKGROUND {
            (
                0.45 + motion::hash(i + 13000) * 0.5,
                0.45 + motion::hash(i + 12000) * 0.7,
            )
        } else {
            (
                0.55 + motion::hash(i - BACKGROUND) * 0.6,
                0.55 + motion::hash(i - BACKGROUND) * 0.8,
            )
        };
        data.extend([
            (radius * 100.0).round() as u8,
            (width * 100.0).round() as u8,
        ]);
    }
    for frame in 0..frames {
        let t = frame as f32 / FPS as f32;
        for i in 0..count {
            let (head, tail, alpha) = if i < BACKGROUND {
                let (head, tail) = motion::flight(i + 10000, t, card);
                let alpha = (0.18 + motion::hash(i + 11000) * 0.52)
                    * motion::phase(t, 0.4, 0.8)
                    * (1.0 - motion::phase(t, 3.9, 2.0) * 0.66);
                (head, tail, alpha)
            } else {
                let n = i - BACKGROUND;
                (
                    motion::constellation(n, targets[n], card, mark, t),
                    motion::constellation(n, targets[n], card, mark, t - 0.035),
                    (0.5 + motion::hash(n + 91) * 0.5)
                        * motion::phase(t, 3.9, 0.65)
                        * (1.0 - motion::phase(t, 6.05, 0.60)),
                )
            };
            for v in [
                head[0] / card.w,
                head[1] / card.h,
                tail[0] / card.w,
                tail[1] / card.h,
            ] {
                data.extend_from_slice(&((v * 8192.0).round() as i16).to_le_bytes());
            }
            data.push((alpha * 255.0).round() as u8);
        }
    }
    let raw = data.len();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&data).unwrap();
    let packed = encoder.finish().unwrap();
    let mut out = Vec::from(*b"NUSHYP1\0");
    for n in [FPS, frames, count, BACKGROUND] {
        out.extend_from_slice(&(n as u32).to_le_bytes());
    }
    out.extend_from_slice(&packed);
    std::fs::write(root.join("motion.bin"), &out).unwrap();
    println!("{count} particles × {frames} prebaked frames at {FPS} Hz; {} bytes packed, {raw} bytes playback",out.len());
}
