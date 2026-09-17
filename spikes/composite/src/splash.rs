//! The splash: the icon, exactly as the taskbar has it, on paper. The
//! swoosh draws itself in as the window comes up, holds until the first
//! tab is ready, then the whole thing fades. Nothing else on it.

use std::sync::Arc;
use std::time::Instant;

use nus_render::Rect;

use crate::anim::Anim;
use crate::app::App;

pub struct Splash {
    pub started: Instant,
    /// The icon texture at the progress it was rendered for.
    pub tex: Option<(f32, Arc<wgpu::BindGroup>)>,
    pub fade: Anim,
    pub leaving: bool,
}

/// Seconds the swoosh takes to draw (before the motion register).
const DRAW: f32 = 0.7;
/// Never shorter than this; never longer than the max, ready or not.
const MAX: f32 = 2.6;
const SIZE: u32 = 256;

impl Splash {
    pub fn new() -> Splash {
        Splash { started: Instant::now(), tex: None, fade: Anim::at(1.0), leaving: false }
    }
}

impl App {
    /// Whether the first tab has something to show.
    fn splash_ready(&self) -> bool {
        self.tabs.iter().any(|t| {
            std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| match p {
                crate::app::Pane::Term(tp) => tp.term.grid().is_damaged() || tp.title != "shell",
                crate::app::Pane::Web(w) => w.tab.shared.borrow().paints > 0,
                _ => true,
            })
        })
    }

    pub fn draw_splash(&mut self, scene: &mut nus_render::Scene) {
        let Some(sp) = self.splash.as_mut() else { return };
        let elapsed = sp.started.elapsed().as_secs_f32();
        let k = 0.45 + (2.2 - 0.45) * self.motion.register.clamp(0.0, 1.0);
        let draw_secs = if self.motion.reduced() || self.behavior.splash != crate::settings::SplashMode::Draw { 0.0 } else { DRAW * k };
        if self.behavior.splash == crate::settings::SplashMode::None {
            self.splash = None;
            return;
        }
        let progress = if draw_secs <= 0.0 { 1.0 } else { (elapsed / draw_secs).clamp(0.0, 1.0) };
        // Render the icon at this progress (a 256² CPU raster; ~40 frames).
        let need = sp.tex.as_ref().map(|(p, _)| (*p - progress).abs() > 0.004).unwrap_or(true);
        if need {
            let n = self.theme.ink;
            let rgba = nus_render::icon::app_icon_at(SIZE, n, self.surface.signal, progress);
            let bgra: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("splash"),
                size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &bgra,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE * 4), rows_per_image: Some(SIZE) },
                wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
            );
            let bind = (self.bind_texture)(&tex);
            if let Some(sp) = self.splash.as_mut() {
                sp.tex = Some((progress, bind));
            }
        }
        let ready = self.splash_ready();
        let paper = self.paper();
        let reduced = self.motion.reduced();
        let hold = self.behavior.splash_hold.max(0.2);
        let Some(sp) = self.splash.as_mut() else { return };
        if !sp.leaving && elapsed >= hold.max(draw_secs) && (ready || elapsed >= MAX.max(hold)) {
            sp.leaving = true;
            let out = if reduced { 0.0 } else { 0.26 * k };
            sp.fade.replay(1.0, 0.0, out);
        }
        let alpha = sp.fade.value();
        if sp.leaving && alpha <= 0.001 && !sp.fade.active() {
            self.splash = None;
            self.dirty = true;
            return;
        }
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let tex = sp.tex.clone();
        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), [paper[0], paper[1], paper[2], alpha]);
        if let Some((_, bind)) = tex {
            let size = (160.0 * self.scale).round();
            let r = Rect::new(((w - size) / 2.0).round(), ((h - size) / 2.0).round() - self.px(12.0), size, size);
            scene.texture_alpha(r, bind, alpha);
            scene.layer(None);
        }
        // Keep drawing while it is up.
        self.dirty = true;
    }
}
