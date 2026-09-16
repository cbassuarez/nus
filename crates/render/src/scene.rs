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
    /// Gradient phase in turns (kinds 3/4).
    pub phase: f32,
    pub _pad: u32,
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
            _pad: 0,
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
            _pad: 0,
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
            _pad: 0,
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
            _pad: 0,
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
            _pad: 0,
        }
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
        self.close();
        let start = self.instances.len();
        self.instances.push(Instance::textured(rect, 1.0));
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
