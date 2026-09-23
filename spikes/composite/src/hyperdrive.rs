//! First arrival: one transparent native surface, from a star card to the
//! actual workspace. No extra window, hidden application screenshot, or video.
use crate::{
    app::{fade, App},
    settings::SplashMode,
};
use nus_render::{Color, Instance, Rect, Scene};
use std::{f32::consts::TAU, sync::Arc};

pub(crate) const DURATION: f32 = 14.6;
const GATHER: f32 = 4.4;
const ORBIT: f32 = 6.6;
const ESCAPE: f32 = 8.6;
const OUTLINE: f32 = 9.7;
const PAPER: f32 = 12.4;
const REVEAL: f32 = 12.75;

pub(crate) struct Art {
    mark: Arc<wgpu::BindGroup>,
    motion: Option<Baked>,
    workspace: Option<(Scene, (u32, u32))>,
    last_frame: Option<std::time::Instant>,
}

/// Authored 120 Hz vector samples, inflated once before the first presentation.
/// Playback only interpolates two adjacent records. No random generation,
/// trigonometry, glyph rasterization, image decoding, or IO in the star loop.
struct Baked {
    data: Vec<u8>,
    fps: usize,
    frames: usize,
    count: usize,
    background: usize,
}
impl Baked {
    fn load() -> Self {
        use std::io::Read;
        let bytes = include_bytes!("../../../assets/arrival/motion.bin");
        assert_eq!(&bytes[..8], b"NUSHYP1\0");
        let n = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let (fps, frames, count, background) = (n(8), n(12), n(16), n(20));
        let mut data = Vec::with_capacity(count * 2 + frames * count * 9);
        flate2::read::ZlibDecoder::new(&bytes[24..])
            .read_to_end(&mut data)
            .expect("baked arrival motion");
        assert_eq!(data.len(), count * 2 + frames * count * 9);
        Self {
            data,
            fps,
            frames,
            count,
            background,
        }
    }
    fn draw(&self, scene: &mut Scene, t: f32, card: Rect, scale: f32, color: Color, alpha: f32) {
        let at = (t.max(0.0) * self.fps as f32).min((self.frames - 1) as f32);
        let frame = at.floor() as usize;
        let mix = at - frame as f32;
        let start = self.count * 2 + frame * self.count * 9;
        let next = self.count * 2 + (frame + 1).min(self.frames - 1) * self.count * 9;
        for i in 0..self.count {
            let a = &self.data[start + i * 9..start + (i + 1) * 9];
            let b = &self.data[next + i * 9..next + (i + 1) * 9];
            let opacity = (a[8] as f32 + (b[8] as f32 - a[8] as f32) * mix) / 255.0 * alpha;
            if opacity < 0.002 {
                continue;
            }
            let value = |k| {
                let a = i16::from_le_bytes([a[k], a[k + 1]]) as f32;
                let b = i16::from_le_bytes([b[k], b[k + 1]]) as f32;
                (a + (b - a) * mix) / 8192.0
            };
            let head = [card.x + value(0) * card.w, card.y + value(2) * card.h];
            let tail = [card.x + value(4) * card.w, card.y + value(6) * card.h];
            let radius = self.data[i * 2] as f32 * 0.01 * scale;
            let width = self.data[i * 2 + 1] as f32 * 0.01 * scale;
            line(
                scene,
                tail,
                head,
                width,
                fade(
                    color,
                    opacity * if i < self.background { 0.55 } else { 0.7 },
                ),
            );
            dot(scene, head, radius, fade(color, opacity));
        }
    }
}
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn phase(t: f32, start: f32, duration: f32) -> f32 {
    ease((t - start) / duration)
}
fn mix(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
fn hash(i: usize) -> f32 {
    let mut n = (i as u32).wrapping_add(1).wrapping_mul(0x9e3779b9);
    n ^= n >> 16;
    n = n.wrapping_mul(0x85ebca6b);
    n ^= n >> 13;
    (n & 0xffffff) as f32 / 16777216.0
}
fn line(scene: &mut Scene, a: [f32; 2], b: [f32; 2], width: f32, color: Color) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = dx.hypot(dy);
    if len < 0.001 || color[3] <= 0.001 {
        return;
    }
    let x = -dy / len * width * 0.5;
    let y = dx / len * width * 0.5;
    scene.push(Instance::quad(
        [
            [a[0] + x, a[1] + y],
            [b[0] + x, b[1] + y],
            [b[0] - x, b[1] - y],
            [a[0] - x, a[1] - y],
        ],
        color,
    ));
}
fn dot(scene: &mut Scene, p: [f32; 2], r: f32, color: Color) {
    scene.push(Instance::rounded(
        Rect::new(p[0] - r, p[1] - r, r * 2.0, r * 2.0),
        r,
        color,
    ));
}
fn star(scene: &mut Scene, p: [f32; 2], s: f32, ink: Color, alpha: f32) {
    for (radius, a) in [(13.0, 0.025), (8.0, 0.05), (4.0, 0.13)] {
        dot(scene, p, s * radius, fade(ink, alpha * a));
    }
    scene.poly(
        &[
            [p[0], p[1] - s * 8.0],
            [p[0] + s * 1.6, p[1] - s * 1.6],
            [p[0] + s * 8.0, p[1]],
            [p[0] + s * 1.6, p[1] + s * 1.6],
            [p[0], p[1] + s * 8.0],
            [p[0] - s * 1.6, p[1] + s * 1.6],
            [p[0] - s * 8.0, p[1]],
            [p[0] - s * 1.6, p[1] - s * 1.6],
        ],
        fade(ink, alpha),
    );
    dot(scene, p, s * 1.7, fade([1.0, 0.97, 0.85, 1.0], alpha));
}
fn orbit(center: [f32; 2], size: f32, t: f32) -> [f32; 2] {
    let theta = (t - ORBIT) / 2.0 * TAU - 0.7;
    let (x, y) = (theta.cos() * size * 0.75, theta.sin() * size * 0.25);
    [
        center[0] + x * 0.94 + y * 0.342,
        center[1] - x * 0.342 + y * 0.94,
    ]
}
fn bezier(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2], t: f32) -> [f32; 2] {
    mix(
        mix(mix(a, b, t), mix(b, c, t), t),
        mix(mix(b, c, t), mix(c, d, t), t),
        t,
    )
}
/// Rounded perimeter, starting along the upper edge. The authored shell radius
/// remains unchanged, including the default square-corner window.
fn border(r: Rect, radius: f32, t: f32) -> [f32; 2] {
    let radius = radius.max(0.0).min(r.w.min(r.h) * 0.5);
    let x = r.w - 2.0 * radius;
    let y = r.h - 2.0 * radius;
    let arc = radius * TAU * 0.25;
    let lengths = [x, arc, y, arc, x, arc, y, arc];
    let mut d = t.clamp(0.0, 1.0) * lengths.iter().sum::<f32>();
    for (i, len) in lengths.into_iter().enumerate() {
        if d <= len || i == 7 {
            let u = if len > 0.0 { d / len } else { 0.0 };
            return match i {
                0 => [r.x + radius + d, r.y],
                2 => [r.right(), r.y + radius + d],
                4 => [r.right() - radius - d, r.bottom()],
                6 => [r.x, r.bottom() - radius - d],
                _ => {
                    let a = (-1.0 + (i / 2) as f32 + u) * TAU * 0.25;
                    let cx = if i < 4 {
                        r.right() - radius
                    } else {
                        r.x + radius
                    };
                    let cy = if i == 1 || i == 7 {
                        r.y + radius
                    } else {
                        r.bottom() - radius
                    };
                    [cx + radius * a.cos(), cy + radius * a.sin()]
                }
            };
        }
        d -= len;
    }
    [r.x + radius, r.y]
}

impl App {
    pub(crate) fn arriving(&self) -> bool {
        self.splash.as_ref().is_some_and(|s| s.arrival)
    }

    pub(crate) fn prepare_arrival(&mut self) {
        if self.arriving() && self.behavior.splash != SplashMode::None {
            let art = self.arrival_art();
            self.splash.as_mut().unwrap().hyperdrive = Some(art);
        }
    }

    pub(crate) fn arrival_scene(&self) -> Option<Scene> {
        let art = self
            .splash
            .as_ref()
            .filter(|s| s.arrival)?
            .hyperdrive
            .as_ref()?;
        let (scene, size) = art.workspace.as_ref()?;
        (*size == self.target.size).then(|| scene.clone())
    }

    fn arrival_art(&self) -> Art {
        const SIZE: u32 = 512;
        let mut reader = png::Decoder::new(&include_bytes!("../../../assets/arrival/n.png")[..])
            .read_info()
            .expect("baked arrival mark");
        let mut rgba = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut rgba).expect("baked arrival pixels");
        let bgra: Vec<_> = rgba
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("arrival n"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bgra,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        Art {
            mark: (self.bind_texture)(&texture),
            motion: if self.motion.reduced() || self.behavior.splash == SplashMode::Still {
                None
            } else {
                Some(Baked::load())
            },
            workspace: None,
            last_frame: None,
        }
    }

    pub(crate) fn draw_arrival(&mut self, scene: &mut Scene, t: f32) {
        let _timing = crate::perf::scope("arrival_draw");
        let still = self.motion.reduced() || self.behavior.splash == SplashMode::Still;
        if self.behavior.splash == SplashMode::None || t >= if still { 0.45 } else { DURATION } {
            self.finish_arrival();
            self.splash = None;
            self.dirty = true;
            return;
        }
        if self.splash.as_ref().unwrap().hyperdrive.is_none() {
            let art = self.arrival_art();
            self.splash.as_mut().unwrap().hyperdrive = Some(art);
        }
        let art = self.splash.as_mut().unwrap().hyperdrive.as_mut().unwrap();
        if art
            .workspace
            .as_ref()
            .is_none_or(|(_, size)| *size != self.target.size)
        {
            scene.finish();
            art.workspace = Some((scene.clone(), self.target.size));
        }
        if let Some(previous) = art.last_frame.replace(std::time::Instant::now()) {
            if !crate::clock::recording() {
                crate::perf::record(
                    "arrival_frame_interval",
                    previous.elapsed().as_secs_f64() * 1000.0,
                );
            }
        }
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let s = self.scale;
        let white = [0.97, 0.96, 0.91, 1.0];
        let black = [0.024, 0.028, 0.037, 1.0];
        let signal = self.surface.signal;
        let ink = self.theme.ink;
        let cw = (w - s * 64.0)
            .min(s * 580.0)
            .min((h - s * 80.0) * 580.0 / 380.0)
            .max(s * 100.0);
        let ch = cw * 380.0 / 580.0;
        let card = Rect::new((w - cw) * 0.5, (h - ch) * 0.5, cw, ch);
        let center = [w * 0.5, card.y + ch * 0.46];
        let size = (ch * 0.57).min(cw * 0.40);
        let mark = Rect::new(center[0] - size * 0.5, center[1] - size * 0.5, size, size);
        let art = self.splash.as_ref().unwrap().hyperdrive.as_ref().unwrap();
        if still {
            scene.clear();
            scene.layer(None);
            scene.rect(card, black);
            scene.texture_alpha(mark, art.mark.clone(), 1.0);
            self.dirty = true;
            return;
        }
        // Until the pen reaches the shell, nothing exists outside the card.
        let reveal = phase(t, REVEAL, DURATION - REVEAL);
        if reveal == 0.0 {
            scene.clear();
        } else {
            scene.clip_existing(Rect::new(0.0, 0.0, w, h * reveal));
        }
        scene.layer(None);
        let card_alpha = 1.0 - phase(t, ESCAPE, OUTLINE - ESCAPE);
        let birth = phase(t, 0.08, 0.95);
        let opening = Rect::new(
            card.x + cw * (1.0 - birth) * 0.5,
            card.y + ch * (1.0 - birth) * 0.5,
            cw * birth,
            (ch * birth).max(s),
        );
        if card_alpha > 0.0 {
            scene.rect(
                Rect::new(
                    opening.x + s * 7.0,
                    opening.y + s * 7.0,
                    opening.w,
                    opening.h,
                ),
                fade(ink, 0.3 * card_alpha * birth),
            );
            scene.rect(opening, fade(black, card_alpha * birth));
            scene.outline(opening, s, fade(white, 0.16 * card_alpha * birth));
        }
        scene.layer(Some(opening));
        if card_alpha > 0.0 {
            if let Some(motion) = &art.motion {
                motion.draw(scene, t, card, s, white, card_alpha);
            }
        }
        // Particles converge on actual glyph pixels before the continuous ink
        // takes over; this is a solid letter, never a generic point-cloud n.
        let solid = phase(t, 5.9, 0.8) * card_alpha;
        scene.texture_uv_alpha(
            mark,
            [0.0, 0.0, 1.0, 1.0],
            art.mark.clone(),
            Some(opening),
            solid,
        );
        scene.layer(None);
        if t >= 6.3 && t < OUTLINE {
            let strength = phase(t, 6.3, 0.35);
            let orbit_t = t.min(ESCAPE);
            for i in 0..90 {
                let ago = i as f32 / 90.0 * 1.25;
                if orbit_t - ago < 6.3 {
                    continue;
                }
                let p = orbit(center, size, orbit_t - ago);
                dot(
                    scene,
                    p,
                    s * (0.6 + 0.5 * (1.0 - i as f32 / 90.0)),
                    fade(
                        signal,
                        strength * (1.0 - i as f32 / 90.0) * 0.62 * card_alpha,
                    ),
                );
            }
        }
        let edge = Rect::new(s * 1.5, s * 1.5, w - s * 3.0, h - s * 3.0);
        let radius = (self.px(self.surface.shell_radius) - s * 1.5).max(0.0);
        let start = border(edge, radius, 0.0);
        let pen = if t < ESCAPE {
            orbit(center, size, t)
        } else if t < OUTLINE {
            let a = orbit(center, size, ESCAPE);
            bezier(
                a,
                [a[0] + size * 1.15, a[1] + size * 0.75],
                [start[0] + w * 0.38, start[1] + h * 0.03],
                start,
                phase(t, ESCAPE, OUTLINE - ESCAPE),
            )
        } else if t < PAPER {
            border(edge, radius, phase(t, OUTLINE, PAPER - OUTLINE))
        } else if t < REVEAL {
            mix(
                start,
                [edge.right(), edge.y],
                phase(t, PAPER, REVEAL - PAPER),
            )
        } else {
            [edge.right(), edge.y + edge.h * reveal]
        };
        if t >= OUTLINE {
            let progress = phase(t, OUTLINE, PAPER - OUTLINE);
            let alpha = 1.0 - phase(t, DURATION - 0.55, 0.55);
            let mut a = start;
            for i in 1..=240 {
                let u = (i as f32 / 240.0).min(progress);
                let b = border(edge, radius, u);
                line(scene, a, b, s * 1.5, fade(ink, alpha));
                a = b;
                if u >= progress {
                    break;
                }
            }
            if reveal > 0.0 {
                // The original UI is revealed behind a slightly irregular ink
                // edge; all dimensions, typography and final colors are intact.
                for i in 0..80 {
                    let x = w * i as f32 / 80.0;
                    line(
                        scene,
                        [x, h * reveal],
                        [x + w / 80.0, h * reveal + s * (hash(i) - 0.5) * 2.0],
                        s,
                        fade(ink, alpha * 0.38),
                    );
                }
            }
        }
        if t >= 6.3 {
            star(
                scene,
                pen,
                s,
                signal,
                phase(t, 6.3, 0.4) * (1.0 - phase(t, DURATION - 0.35, 0.35)),
            );
        }
        if t > 1.2 && t < ESCAPE {
            let style = nus_render::text::Style {
                color: fade(white, 0.42 * phase(t, 1.2, 0.5)),
                ..self.label()
            };
            let hint = "ENTER TO CONTINUE";
            let width = self.fonts.measure(style, hint);
            self.fonts.draw(
                scene,
                style,
                w * 0.5 - width * 0.5,
                card.bottom() - s * 23.0,
                hint,
            );
        }
        self.dirty = true;
    }
}

#[cfg(test)]
#[path = "../examples/support/hyperdrive_motion.rs"]
mod authoring;

#[cfg(test)]
mod tests {
    use super::authoring::{constellation, flight, warp};
    #[test]
    fn baked_samples_match_the_authored_braking_path() {
        let bank=Baked::load();
        assert_eq!(bank.fps,120);assert!(bank.data.len()<12*1024*1024);
        let card=Rect::new(0.0,0.0,580.0,380.0);
        for frame in [0,120,408,600,768,792] {
            for i in [0,37,199,419] {
                let offset=bank.count*2+frame*bank.count*9+i*9;
                let bytes=&bank.data[offset..offset+9];
                let (head,tail)=flight(i+10000,frame as f32/120.0,card);
                for (k,(expected,extent)) in [(head[0],card.w),(head[1],card.h),(tail[0],card.w),(tail[1],card.h)].into_iter().enumerate() {
                    let actual=i16::from_le_bytes([bytes[k*2],bytes[k*2+1]]) as f32/8192.0*extent;
                    assert!((actual-expected).abs()<0.04);
                }
            }
        }
    }
    use super::*;
    #[test]
    fn stars_start_as_points_then_stretch() {
        let card = Rect::new(10.0, 20.0, 580.0, 380.0);
        for i in 0..80 {
            let (a, b) = flight(i, 1.0, card);
            assert_eq!(a, b);
            let (a, b) = flight(i, 3.4, card);
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) > 0.1);
        }
    }
    #[test]
    fn field_brakes_without_reversing_or_joining_the_constellation() {
        let card = Rect::new(0.0, 0.0, 580.0, 380.0);
        let mark = Rect::new(180.0, 80.0, 200.0, 200.0);
        let mut previous = 0.0;
        for tick in 0..900 {
            let (distance, speed) = warp(tick as f32 / 100.0);
            assert!(distance >= previous);
            assert!((0.0..=1.0).contains(&speed));
            previous = distance;
        }
        for i in 0..80 {
            assert_eq!(
                constellation(i, [0.4, 0.7], card, mark, 7.0),
                constellation(i, [0.4, 0.7], card, mark, 8.0)
            );
            let (head, tail) = flight(i + 10000, 7.0, card);
            assert_eq!(head, tail);
            assert_eq!(flight(i + 10000, 7.0, card), flight(i + 10000, 8.0, card));
            assert_ne!(head, constellation(i, [0.4, 0.7], card, mark, 7.0));
        }
    }
    #[test]
    fn perimeter_is_continuous_and_respects_corners() {
        let r = Rect::new(2.0, 2.0, 1000.0, 700.0);
        for radius in [0.0, 24.0, 100.0] {
            let mut prev = border(r, radius, 0.0);
            assert!((prev[0] - border(r, radius, 1.0)[0]).abs() < 0.001);
            for i in 1..=1000 {
                let p = border(r, radius, i as f32 / 1000.0);
                assert!(
                    p[0] >= r.x - 0.001
                        && p[0] <= r.right() + 0.001
                        && p[1] >= r.y - 0.001
                        && p[1] <= r.bottom() + 0.001
                );
                assert!((p[0] - prev[0]).hypot(p[1] - prev[1]) < 4.0);
                prev = p;
            }
        }
    }
    #[test]
    fn reveal_is_bounded_and_choreography_is_ordered() {
        assert!(
            GATHER < ORBIT
                && ORBIT < ESCAPE
                && ESCAPE < OUTLINE
                && OUTLINE < PAPER
                && PAPER < DURATION
        );
        for i in 0..1600 {
            let p = phase(i as f32 / 100.0, PAPER, DURATION - PAPER);
            assert!((0.0..=1.0).contains(&p));
        }
        assert_eq!(phase(PAPER, PAPER, DURATION - PAPER), 0.0);
        assert_eq!(phase(DURATION, PAPER, DURATION - PAPER), 1.0);
    }
}
