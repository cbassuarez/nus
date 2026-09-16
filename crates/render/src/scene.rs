//! A frame's draw list: instanced quads grouped into layers, each layer with
//! an optional clip rect and a texture binding.

use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
}

pub type Color = [f32; 4];

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub kind: u32,
    /// Second color, packed RGBA8 (kinds 3/4); 0 = none.
    pub color2: u32,
    /// Gradient phase in turns (kinds 3/4); texture scale (kinds 6–10).
    pub phase: f32,
    /// Kinds 3/4: gradient stops beyond two — bits 0–7 stop count (0 = the
    /// legacy two-colour diagonal), bits 8–17 angle in degrees, bit 18 loop
    /// (aurora); stops 3 and 4 ride packed in `uv.z` / `uv.w`.
    pub extra: u32,
}

impl Instance {
    pub fn rect(r: Rect, color: Color) -> Instance {
        Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [0.0; 4],
            color,
            kind: 0,
            color2: 0,
            phase: 0.0,
            extra: 0,
        }
    }
    pub fn glyph(x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: Color) -> Instance {
        Instance {
            pos: [x, y],
            size: [w, h],
            uv,
            color,
            kind: 1,
            color2: 0,
            phase: 0.0,
            extra: 0,
        }
    }
    pub fn textured(r: Rect, alpha: f32) -> Instance {
        Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [0.0, 0.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, alpha],
            kind: 2,
            color2: 0,
            phase: 0.0,
            extra: 0,
        }
    }
    /// Rounded fill.
    pub fn rounded(r: Rect, radius: f32, color: Color) -> Instance {
        Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [radius, 0.0, 0.0, 0.0],
            color,
            kind: 3,
            color2: 0,
            phase: 0.0,
            extra: 0,
        }
    }
    /// Rounded stroke of `thickness` inside `r`; optional gradient toward
    /// `color2`, shifted by `phase` turns.
    pub fn stroke(
        r: Rect,
        radius: f32,
        thickness: f32,
        color: Color,
        color2: Option<Color>,
        phase: f32,
    ) -> Instance {
        Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [radius, thickness, 0.0, 0.0],
            color,
            kind: 4,
            color2: color2.map(pack).unwrap_or(0),
            phase,
            extra: 0,
        }
    }
}

impl Instance {
    /// Diagonal hazard tape as a stroke of `thickness` inside `r`.
    pub fn hazard(r: Rect, thickness: f32, a: Color, b: Color, period: f32) -> Instance {
        Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [0.0, thickness, 0.0, 0.0],
            color: a,
            kind: 5,
            color2: pack(b),
            phase: period,
            extra: 0,
        }
    }
}

impl Instance {
    /// Grain overlay: speckles of `color` (its alpha = strength), `grain` px each.
    pub fn grain(r: Rect, color: Color, grain: f32) -> Instance {
        Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [0.0; 4],
            color,
            kind: 6,
            color2: 0,
            phase: grain,
            extra: 0,
        }
    }
}

impl Instance {
    /// Rounded stroke through up to four `stops`, running at `angle`
    /// degrees (0 = left→right, 90 = top→bottom); `looping` wraps the
    /// ramp so `phase` can drift it forever (aurora).
    pub fn stroke_stops(
        r: Rect,
        radius: f32,
        thickness: f32,
        stops: &[Color],
        angle: f32,
        phase: f32,
        looping: bool,
    ) -> Instance {
        let mut i = Instance::stroke(r, radius, thickness, stops[0], stops.get(1).copied(), phase);
        i.set_stops(stops, angle, looping);
        i
    }

    /// Rounded fill through up to four stops (see `stroke_stops`).
    pub fn rounded_stops(
        r: Rect,
        radius: f32,
        stops: &[Color],
        angle: f32,
        phase: f32,
        looping: bool,
    ) -> Instance {
        let mut i = Instance::rounded(r, radius, stops[0]);
        i.phase = phase;
        i.set_stops(stops, angle, looping);
        i
    }

    fn set_stops(&mut self, stops: &[Color], angle: f32, looping: bool) {
        let n = stops.len().clamp(1, 4) as u32;
        if n >= 2 {
            self.color2 = pack(stops[1]);
        }
        if n >= 3 {
            self.uv[2] = f32::from_bits(pack(stops[2]));
        }
        if n >= 4 {
            self.uv[3] = f32::from_bits(pack(stops[3]));
        }
        let a = (angle.rem_euclid(360.0)) as u32;
        self.extra = n | (a << 8) | ((looping as u32) << 18);
    }

    /// Texture overlay: `kind` 6 grain, 7 stipple, 8 stitch, 9 linen,
    /// 10 halftone; `color.a` is the strength, `scale` the pattern pitch,
    /// `time` (seconds, 0 = still) animates it.
    pub fn texture_kind(r: Rect, kind: u32, color: Color, scale: f32, time: f32) -> Instance {
        let mut i = Instance::grain(r, color, scale);
        i.kind = kind;
        i.extra = (time * 1000.0) as u32;
        i
    }
}

pub fn pack(c: Color) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    q(c[0]) | (q(c[1]) << 8) | (q(c[2]) << 16) | (q(c[3]) << 24)
}

pub enum Bind {
    Atlas,
    External(Arc<wgpu::BindGroup>),
}

pub struct Layer {
    pub range: Range<usize>,
    pub clip: Option<Rect>,
    pub bind: Bind,
}

/// Build with `push`/`text` inside `layer(...)` groups; instances within a
/// layer draw in push order, layers draw in creation order.
#[derive(Default)]
pub struct Scene {
    instances: Vec<Instance>,
    layers: Vec<Layer>,
    open: Option<(usize, Option<Rect>)>,
}

impl Scene {
    pub fn new() -> Scene {
        Scene::default()
    }

    pub fn clear(&mut self) {
        self.instances.clear();
        self.layers.clear();
        self.open = None;
    }

    /// Start (or restart) an atlas-bound layer with an optional clip.
    pub fn layer(&mut self, clip: Option<Rect>) {
        self.close();
        self.open = Some((self.instances.len(), clip));
    }

    fn close(&mut self) {
        if let Some((start, clip)) = self.open.take() {
            let end = self.instances.len();
            if end > start {
                self.layers.push(Layer {
                    range: start..end,
                    clip,
                    bind: Bind::Atlas,
                });
            }
        }
    }

    /// Draw an external texture into `rect`, as its own layer.
    pub fn texture(&mut self, rect: Rect, bind: Arc<wgpu::BindGroup>, clip: Option<Rect>) {
        self.texture_uv(rect, [0.0, 0.0, 1.0, 1.0], bind, clip);
    }

    /// Draw an external texture with an alpha, as its own layer.
    pub fn texture_alpha(&mut self, rect: Rect, bind: Arc<wgpu::BindGroup>, alpha: f32) {
        self.close();
        let start = self.instances.len();
        self.instances.push(Instance::textured(rect, alpha));
        self.layers.push(Layer {
            range: start..start + 1,
            clip: None,
            bind: Bind::External(bind),
        });
    }

    /// Draw a sub-rectangle (`uv` = u0, v0, u1, v1) of an external texture.
    pub fn texture_uv(
        &mut self,
        rect: Rect,
        uv: [f32; 4],
        bind: Arc<wgpu::BindGroup>,
        clip: Option<Rect>,
    ) {
        self.close();
        let start = self.instances.len();
        let mut i = Instance::textured(rect, 1.0);
        i.uv = uv;
        self.instances.push(i);
        self.layers.push(Layer {
            range: start..start + 1,
            clip,
            bind: Bind::External(bind),
        });
    }

    pub fn push(&mut self, i: Instance) {
        if self.open.is_none() {
            self.open = Some((self.instances.len(), None));
        }
        self.instances.push(i);
    }

    pub fn rect(&mut self, r: Rect, color: Color) {
        self.push(Instance::rect(r, color));
    }

    pub fn hline(&mut self, x: f32, y: f32, w: f32, thickness: f32, color: Color) {
        self.rect(Rect::new(x, y, w, thickness), color);
    }

    pub fn vline(&mut self, x: f32, y: f32, h: f32, thickness: f32, color: Color) {
        self.rect(Rect::new(x, y, thickness, h), color);
    }

    pub fn outline(&mut self, r: Rect, thickness: f32, color: Color) {
        self.hline(r.x, r.y, r.w, thickness, color);
        self.hline(r.x, r.bottom() - thickness, r.w, thickness, color);
        self.vline(r.x, r.y, r.h, thickness, color);
        self.vline(r.right() - thickness, r.y, r.h, thickness, color);
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Finish the frame (closes the open layer). Call before rendering.
    pub fn finish(&mut self) {
        self.close();
    }
}
