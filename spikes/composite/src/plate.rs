//! The plate: the prompt under the icon. HOME · THE PLATE keeps the app's
//! own icon — the wordmark's n with the band in orbit, the taskbar's
//! geometry exactly, from the same field — at a plate's size above the
//! line, and puts your last places on the band as stops: a square where
//! the band runs clear of the n, a caps label beside it, each one a row
//! you can pick. The band draws itself in the first time; with the splash
//! on, the splash draws it where the plate keeps it and hands over with
//! the icon never moving — only the chrome comes up around it. Typing
//! works as on the line alone: rows come up beneath.

use std::sync::Arc;

use nus_render::icon::{band_stops, Band, IconField};
use nus_render::text::Style;
use nus_render::{Color, Rect, Scene};

use crate::app::{fade, Action, App, PaletteRow, SYSTEM_PROCS};
use crate::home::HomePane;
use crate::settings::{HomeLook, SplashMode, Then};

/// Seconds the band takes to draw (before the motion register).
pub const DRAW: f32 = 0.42;
/// Seconds the line and the stops take to come up once the band closes.
const SETTLE: f32 = 0.4;
/// Stops on the band: the first places the prompt offers.
pub const STOPS: usize = 4;

pub struct PlateArt {
    field: IconField,
    /// The texture, at the progress and colours it was rendered for.
    tex: Option<(f32, Color, Color, Arc<wgpu::BindGroup>)>,
    /// Parametric angles of the stops along the band, in band order.
    stops: Vec<f32>,
}

/// The band's draw-in: quick off the mark, easing home (ease-out cubic).
pub(crate) fn swoosh(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Where the plate keeps the icon in a pane: half the pane's height (or
/// width, when narrow), centred, a little above the middle — the line
/// runs beneath it.
pub(crate) fn icon_rect(pane: Rect) -> Rect {
    let size = (pane.h * 0.5).min(pane.w * 0.5).max(64.0).round();
    Rect::new((pane.x + (pane.w - size) / 2.0).round(), (pane.y + pane.h * 0.11).round(), size, size)
}

/// The last folder of a path, for a label: `C:\\Users\\seb\\nus` → `nus`.
fn tail(cwd: &str) -> String {
    let t = cwd.trim_end_matches(['/', '\\']);
    t.rsplit(['/', '\\']).next().filter(|s| !s.is_empty()).unwrap_or(t).to_string()
}

impl App {
    /// The stops: your last places, at most STOPS — shells still running
    /// in a holder, the folders the last session's shells were in (then
    /// the journal's), dev servers listening, the last pages. Each is a
    /// row: the label on the band, and what enter does.
    pub(crate) fn places(&self) -> Vec<PaletteRow> {
        let mut out: Vec<PaletteRow> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut push = |out: &mut Vec<PaletteRow>, key: String, text: String, action: Action| {
            if out.len() < STOPS && seen.insert(key) {
                out.push(PaletteRow { num: String::new(), text, action });
            }
        };
        for info in self.held_loose() {
            let at = info.cwd.as_deref().map(tail).filter(|t| !t.is_empty()).map(|t| format!(" · {t}")).unwrap_or_default();
            push(&mut out, format!("held {}", info.id), format!("{} · held{at}", info.program), Action::AttachHeld(info.id.clone()));
        }
        let mut folders: Vec<String> = Vec::new();
        if let Some(sess) = &self.last_session {
            for t in &sess.tabs {
                for st in std::iter::once(&t.shell).chain(std::iter::once(&t.shell_right)).flatten() {
                    if let Some(c) = &st.cwd {
                        folders.push(c.clone());
                    }
                }
            }
        }
        folders.extend(crate::journal::folders().into_iter().map(|(c, _)| c));
        for cwd in folders.into_iter().filter(|c| !c.is_empty()) {
            let key = format!("shell {}", cwd.to_lowercase());
            push(&mut out, key, format!("{} · shell", tail(&cwd)), Action::ShellAt(cwd));
        }
        for port in self.ports.iter().filter(|p| p.port >= 1024 && !SYSTEM_PROCS.contains(&p.process.to_lowercase().as_str()) && !self.behavior.ports_hidden.iter().any(|h| h.eq_ignore_ascii_case(&p.process)) && !p.process.to_lowercase().starts_with("composite") && !p.process.to_lowercase().starts_with("nus")) {
            let text = if port.process.is_empty() { format!("localhost:{}", port.port) } else { format!("localhost:{} · {}", port.port, port.process.to_lowercase()) };
            push(&mut out, format!("port {}", port.port), text, Action::NewBrowser(format!("http://localhost:{}/", port.port)));
        }
        for r in &self.recent {
            if let crate::start::Saved::Page { url, .. } = &r.item {
                let host = crate::links::host(url);
                if host.is_empty() || host == "localhost" || host.starts_with("localhost:") {
                    continue;
                }
                push(&mut out, format!("page {host}"), format!("{host} · page"), Action::NewBrowser(url.clone()));
            }
        }
        out
    }

    /// The splash draws the icon where the plate keeps it and hands over
    /// with it in place: THEN · THE PROMPT, HOME · THE PLATE, a splash on.
    pub(crate) fn plate_continues(&self) -> bool {
        self.behavior.then == Then::Prompt && self.behavior.home_look == HomeLook::Plate && self.behavior.splash != SplashMode::None
    }

    /// The motion register's stretch on the plate's (and the splash's) timings.
    pub(crate) fn plate_k(&self) -> f32 {
        0.45 + (2.2 - 0.45) * self.motion.register.clamp(0.0, 1.0)
    }

    /// The icon at `size` pixels with the band drawn `progress` of the way,
    /// from the field sampled once at that size.
    pub(crate) fn plate_texture(&mut self, size: u32, progress: f32) -> Arc<wgpu::BindGroup> {
        let (n_color, band) = (self.theme.ink, self.surface.signal);
        if self.plate.as_ref().map(|a| a.field.size() != size).unwrap_or(true) {
            self.plate = Some(PlateArt { field: IconField::new(size), tex: None, stops: band_stops(size as f32, STOPS) });
        }
        let art = self.plate.as_mut().expect("plate art");
        if let Some((p, n, b, bind)) = &art.tex {
            if (*p - progress).abs() <= 1.0 / 400.0 && *n == n_color && *b == band {
                return bind.clone();
            }
        }
        let rgba = art.field.frame(n_color, band, progress);
        let bgra: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("plate"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
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
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
        let bind = (self.bind_texture)(&tex);
        art.tex = Some((progress, n_color, band, bind.clone()));
        bind
    }

    /// The icon above the line, the band drawing in unless the splash
    /// already drew it. Returns the line's baseline and how far the line
    /// and the stops have come up (0..1) — they come up once the band closes.
    pub(crate) fn draw_plate_icon(&mut self, scene: &mut Scene, p: &HomePane) -> (f32, f32) {
        let rect = icon_rect(p.rect);
        let size = (rect.w.round() as u32).clamp(64, 1024);
        let k = self.plate_k();
        let reduced = self.motion.reduced();
        let t = p.since.elapsed().as_secs_f32();
        let draw_secs = if reduced || p.handed { 0.0 } else { DRAW * k };
        let progress = if draw_secs <= 0.0 { 1.0 } else { swoosh((t / draw_secs).clamp(0.0, 1.0)) };
        let up = if reduced { 1.0 } else { ((t - draw_secs) / (SETTLE * k)).clamp(0.0, 1.0) };
        let up = 1.0 - (1.0 - up) * (1.0 - up);
        let bind = self.plate_texture(size, progress);
        scene.texture_alpha(rect, bind, 1.0);
        scene.layer(None);
        if progress < 1.0 || up < 1.0 {
            self.dirty = true;
        }
        (rect.bottom() + self.px(44.0), up)
    }

    /// The stops: `rows` on the band, a square each where it runs clear of
    /// the n and a label beside it — out to the left or right when the stop
    /// is off to a side, above or below it when it is near the middle, on
    /// a slip of paper so it reads over the band should it cross. The hits
    /// go on the pane.
    pub(crate) fn draw_stops(&mut self, scene: &mut Scene, p: &mut HomePane, rows: &[PaletteRow], sel: usize, up: f32) {
        let rect = icon_rect(p.rect);
        let Some(art) = self.plate.as_ref() else { return };
        let size = art.field.size() as f32;
        let stops = art.stops.clone();
        let band = Band::in_frame(size);
        let scale = rect.w / size;
        let ink = self.theme.ink;
        let signal = self.surface.signal;
        let paper = self.paper();
        let label = self.label();
        let side = self.px(9.0);
        let gap = self.px(16.0);
        let pad = self.px(4.0);
        let (mx, my) = self.mouse;
        for (k, (row, th)) in rows.iter().zip(stops.iter()).enumerate() {
            let (x, y) = band.at(*th);
            let (sx, sy) = (rect.x + x * scale, rect.y + y * scale);
            let (dx, dy) = (sx - (rect.x + rect.w / 2.0), sy - (rect.y + rect.h / 2.0));
            let text = self.fit(label, &row.text, self.px(220.0));
            let tw = self.fonts.measure(label, &text);
            let (tx, ty) = if dx.abs() > rect.w * 0.3 {
                // Beside the stop, away from the icon.
                let ax = sx + dx.signum() * (gap + side / 2.0);
                (if dx >= 0.0 { ax } else { ax - tw }, sy + label.px * 0.36)
            } else {
                // Above or below it, centred, clear of the band's own width.
                let reach = gap + side / 2.0 + band.base_t * scale * 0.75;
                let ay = sy + dy.signum() * reach;
                (sx - tw / 2.0, if dy >= 0.0 { ay + label.px * 0.72 } else { ay })
            };
            let slip = Rect::new((tx - pad).round(), (ty - label.px - pad / 2.0).round(), (tw + 2.0 * pad).round(), (label.px + pad * 1.5).round());
            let hit = Rect::new(slip.x.min(sx - side), slip.y.min(sy - side), slip.right().max(sx + side) - slip.x.min(sx - side), slip.bottom().max(sy + side) - slip.y.min(sy - side));
            let hot = k + 1 == sel || hit.contains(mx, my);
            scene.rect(Rect::new((sx - side / 2.0).round(), (sy - side / 2.0).round(), side, side), fade(if hot { signal } else { ink }, up));
            scene.rect(slip, fade(paper, up));
            self.fonts.draw(scene, Style { color: fade(ink, if hot { 1.0 } else { 0.62 } * up), ..label }, tx, ty, &text);
            p.hits.push((hit, k));
        }
    }
}
