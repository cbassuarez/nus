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
    /// The overlap of two rects (empty, at `self`'s origin, when they miss).
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        if r <= x || b <= y {
            return Rect::new(self.x, self.y, 0.0, 0.0);
        }
        Rect::new(x, y, r - x, b - y)
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
    /// Encoded sRGB radiance may exceed 1.0. The HDR pipeline converts to
    /// extended linear light; an SDR target clips only the small highlight.
    pub fn loading_light(r: Rect, progress: f32, scale: f32, color: Color) -> Instance {
        let mut instance = Self::rect(r, color);
        instance.kind = 18;
        instance.uv = [progress.clamp(0.0, 1.0), scale.max(0.1), 0.0, 0.0];
        instance
    }

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
    /// The intelligence atom (kind 19): see `atom` in quad.wgsl. `level` and
    /// `value` run 0–4 (the orbits follow `level`, the dial `value`);
    /// `nucleons` 0–9 is the model, `metal` and `roughness` its provider.
    pub fn atom(r: Rect, look: AtomLook) -> Instance {
        let q = |v: f32, k: f32| ((v * k).round().clamp(0.0, 255.0)) as u32;
        let mut i = Self::rect(r, look.ink);
        i.kind = 19;
        i.phase = look.seconds;
        i.uv = [
            look.level,
            look.value,
            f32::from_bits(pack(look.signal)),
            0.0,
        ];
        i.color2 = pack(look.metal);
        i.extra = look.mode as u32
            | q(look.scale, 32.0) << 8
            | q(look.nucleons, 16.0) << 16
            | q(look.roughness, 255.0) << 24;
        i
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
    /// A convex quad through four corners (clockwise or counter), in
    /// pixels. Corners ride inside the bounding box as 16-bit fractions.
    pub fn quad(corners: [[f32; 2]; 4], color: Color) -> Instance {
        let min_x = corners.iter().map(|c| c[0]).fold(f32::INFINITY, f32::min);
        let min_y = corners.iter().map(|c| c[1]).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|c| c[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let max_y = corners
            .iter()
            .map(|c| c[1])
            .fold(f32::NEG_INFINITY, f32::max);
        // A pixel of slack so the anti-aliased edge isn't clipped.
        let (x, y) = (min_x.floor() - 1.0, min_y.floor() - 1.0);
        let (w, h) = ((max_x - x).ceil() + 1.0, (max_y - y).ceil() + 1.0);
        let norm = |c: [f32; 2]| ((c[0] - x) / w, (c[1] - y) / h);
        let pack16 = |c: [f32; 2]| -> u32 {
            let (nx, ny) = norm(c);
            let a = (nx.clamp(0.0, 1.0) * 65535.0).round() as u32;
            let b = (ny.clamp(0.0, 1.0) * 65535.0).round() as u32;
            a | (b << 16)
        };
        let (c0x, c0y) = norm(corners[0]);
        Instance {
            pos: [x, y],
            size: [w, h],
            uv: [
                c0x,
                c0y,
                f32::from_bits(pack16(corners[1])),
                f32::from_bits(pack16(corners[2])),
            ],
            color,
            kind: 12,
            color2: pack16(corners[3]),
            phase: 0.0,
            extra: 0,
        }
    }

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

    /// A texture masked to a rounded stroke of `thickness` inside `r` — the
    /// carapace's own shape.
    pub fn texture_stroke(
        r: Rect,
        kind: u32,
        color: Color,
        scale: f32,
        time: f32,
        radius: f32,
        thickness: f32,
    ) -> Instance {
        let mut i = Instance::texture_kind(r, kind, color, scale, time);
        i.uv = [radius, thickness, 0.0, 0.0];
        i
    }
}

/// What the atom shows this frame.
#[derive(Clone, Copy, Debug)]
pub struct AtomLook {
    pub mode: AtomMode,
    pub seconds: f32,
    pub level: f32,
    pub value: f32,
    pub nucleons: f32,
    pub metal: Color,
    pub roughness: f32,
    pub ink: Color,
    pub signal: Color,
    /// Device pixels per layout pixel, for hairlines.
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomMode {
    /// Dial, orbits, electrons and the nucleus.
    Instrument = 0,
    /// A chrome bead alone.
    Bead = 1,
    /// Orbits, electrons and the nucleus, without the dial.
    Orbits = 2,
}

pub fn pack(c: Color) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    q(c[0]) | (q(c[1]) << 8) | (q(c[2]) << 16) | (q(c[3]) << 24)
}

#[derive(Clone)]
pub enum Bind {
    Atlas,
    External(Arc<wgpu::BindGroup>),
}

#[derive(Clone)]
pub struct Layer {
    pub range: Range<usize>,
    pub clip: Option<Rect>,
    pub bind: Bind,
}

/// Build with `push`/`text` inside `layer(...)` groups; instances within a
/// layer draw in push order, layers draw in creation order.
#[derive(Default, Clone)]
pub struct Scene {
    /// Round the complete window, including child surfaces and chrome.
    pub corner_radius: f32,
    instances: Vec<Instance>,
    layers: Vec<Layer>,
    open: Option<(usize, Option<Rect>)>,
    /// Polygon corners for the frame's `poly` instances, in the order pushed.
    points: Vec<[f32; 2]>,
}

/// The most corners one polygon may have; the shader walks them per pixel.
pub const POLY_MAX: usize = 1024;

impl Scene {
    pub fn new() -> Scene {
        Scene::default()
    }

    pub fn clear(&mut self) {
        self.corner_radius = 0.0;
        self.instances.clear();
        self.layers.clear();
        self.open = None;
        self.points.clear();
    }

    /// A filled polygon through `pts` (either winding, concave allowed,
    /// even-odd where it crosses itself), anti-aliased at its edge and
    /// seamless inside however translucent the colour: the shader fills it
    /// from the signed distance to the outline rather than from triangles.
    pub fn poly(&mut self, pts: &[[f32; 2]], color: Color) {
        let n = pts.len().min(POLY_MAX);
        if n < 3 || color[3] <= 0.0 {
            return;
        }
        let pts = &pts[..n];
        let min_x = pts.iter().map(|c| c[0]).fold(f32::INFINITY, f32::min);
        let min_y = pts.iter().map(|c| c[1]).fold(f32::INFINITY, f32::min);
        let max_x = pts.iter().map(|c| c[0]).fold(f32::NEG_INFINITY, f32::max);
        let max_y = pts.iter().map(|c| c[1]).fold(f32::NEG_INFINITY, f32::max);
        if !(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()) {
            return;
        }
        // A pixel of slack so the anti-aliased edge isn't clipped.
        let (x, y) = (min_x.floor() - 1.0, min_y.floor() - 1.0);
        let (w, h) = ((max_x - x).ceil() + 1.0, (max_y - y).ceil() + 1.0);
        let start = self.points.len() as u32;
        // Corners ride relative to the box, as the shader sees the pixel.
        self.points.extend(pts.iter().map(|c| [c[0] - x, c[1] - y]));
        self.push(Instance {
            pos: [x, y],
            size: [w, h],
            uv: [0.0; 4],
            color,
            kind: 13,
            color2: n as u32,
            phase: 0.0,
            extra: start,
        });
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.points
    }

    /// A sky over `r`: the gradient by the sun's height, the sun (or the
    /// moon and stars) where it is, and cumulus from a noise field, lit
    /// from the sun and drifting on the wind — all in the fragment shader.
    /// `az` runs -1 (east, left) to 1 (west, right); `alt` is the sine of
    /// the sun's altitude; `cover` 0..1; `t` seconds; `seed` picks the field.
    // Eight independent scalars, each one named in the lines above and each
    // going straight to the shader: a struct here would only move the list.
    #[allow(clippy::too_many_arguments)]
    pub fn sky(
        &mut self,
        r: Rect,
        az: f32,
        alt: f32,
        cover: f32,
        wind: f32,
        t: f32,
        seed: [f32; 2],
        moon: [f32; 4],
    ) {
        self.push(Instance {
            pos: [r.x, r.y],
            size: [r.w, r.h],
            uv: [az, alt, 0.0, 0.0],
            color: [cover, wind, seed[0], seed[1]],
            kind: 14,
            color2: (((moon[0].clamp(-1.0, 1.0) + 1.0) * 0.5 * 65535.0).round() as u32)
                | ((((moon[1].clamp(-1.0, 1.0) + 1.0) * 0.5 * 65535.0).round() as u32) << 16),
            phase: t,
            extra: ((moon[2].clamp(0.0, 1.0) * 65535.0).round() as u32)
                | ((u32::from(moon[3] > 0.0)) << 16),
        });
    }

    /// The clip of the layer being drawn now, if any: read it before
    /// opening a nested clip, and restore it after, so a card clipped to
    /// itself inside a scrolling page doesn't leave the page unclipped.
    pub fn clip(&self) -> Option<Rect> {
        self.open.and_then(|(_, c)| c)
    }

    /// Apply a reveal to the entire composed surface, including nested panes.
    /// Existing clips are intersected, so a reveal cannot expose pane overflow.
    pub fn clip_all(&mut self, rect: Rect) {
        self.close();
        for layer in &mut self.layers {
            layer.clip = Some(layer.clip.map(|c| c.intersect(&rect)).unwrap_or(rect));
        }
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

    /// Art-directed liquid displacement of an RGBA material study. Zero
    /// motion samples the original artwork exactly (including reduced motion).
    pub fn liquid_texture(
        &mut self,
        rect: Rect,
        bind: Arc<wgpu::BindGroup>,
        alpha: f32,
        seconds: f32,
        motion: f32,
    ) {
        let clip = self.clip();
        self.close();
        let start = self.instances.len();
        let mut i = Instance::textured(rect, alpha);
        i.kind = 15;
        i.phase = seconds;
        i.color[0] = motion.clamp(0.0, 1.0);
        self.instances.push(i);
        self.layers.push(Layer {
            range: start..start + 1,
            clip,
            bind: Bind::External(bind),
        });
    }

    /// Procedural twinkling stars and ordered-dither pointer trails. Trail
    /// positions are normalized to this rect; the third value is age in seconds.
    pub fn mercury_field(
        &mut self,
        rect: Rect,
        seconds: f32,
        alpha: f32,
        pixel: f32,
        trail: &[[f32; 3]],
    ) {
        let mut i = Instance::rect(rect, [0.17, 0.18, 0.20, alpha]);
        i.kind = 17;
        i.phase = seconds;
        i.uv[0] = pixel.max(1.0);
        i.extra = self.points.len() as u32;
        i.color2 = trail.len().min(28) as u32;
        for p in trail.iter().take(28) {
            self.points.push([p[0], p[1]]);
            self.points.push([p[2], 0.0]);
        }
        self.push(i);
    }

    /// A feathered elliptical light or contact shadow, fading to zero at its
    /// bounds. Unlike a rounded rectangle this has no straight edge segments.
    pub fn soft_ellipse(&mut self, rect: Rect, color: Color) {
        let mut i = Instance::rect(rect, color);
        i.kind = 16;
        self.push(i);
    }

    /// Draw a sub-rectangle (`uv` = u0, v0, u1, v1) of an external texture.
    pub fn texture_uv(
        &mut self,
        rect: Rect,
        uv: [f32; 4],
        bind: Arc<wgpu::BindGroup>,
        clip: Option<Rect>,
    ) {
        self.texture_uv_alpha(rect, uv, bind, clip, 1.0);
    }

    /// A texture with both opacity and an explicit clip, for nested previews.
    pub fn texture_uv_alpha(
        &mut self,
        rect: Rect,
        uv: [f32; 4],
        bind: Arc<wgpu::BindGroup>,
        clip: Option<Rect>,
        alpha: f32,
    ) {
        self.close();
        let start = self.instances.len();
        let mut i = Instance::textured(rect, alpha);
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

    /// Restrict the existing composition, retaining every nested clip. New
    /// layers added afterward are unaffected (for a reveal's drawing head).
    pub fn clip_existing(&mut self, clip: Rect) {
        self.close();
        for layer in &mut self.layers {
            layer.clip = Some(layer.clip.map_or(clip, |old| old.intersect(&clip)));
        }
    }

    /// Keep only what lies inside `a` or `b`. The two must not overlap
    /// (anything in both would draw twice): each layer is kept once clipped
    /// to `a` and once more, as a copy, clipped to `b`.
    pub fn clip_existing_pair(&mut self, a: Rect, b: Rect) {
        self.close();
        let copies: Vec<Layer> = self
            .layers
            .iter()
            .map(|layer| {
                let mut copy = layer.clone();
                copy.clip = Some(copy.clip.map_or(b, |old| old.intersect(&b)));
                copy
            })
            .collect();
        for layer in &mut self.layers {
            layer.clip = Some(layer.clip.map_or(a, |old| old.intersect(&a)));
        }
        self.layers.extend(copies);
    }

    /// Finish the frame (closes the open layer). Call before rendering.
    pub fn finish(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_of_clips_keeps_both_regions_and_draws_nothing_twice() {
        let mut scene = Scene::new();
        scene.layer(Some(Rect::new(0.0, 0.0, 100.0, 100.0)));
        scene.rect(Rect::new(0.0, 0.0, 100.0, 100.0), [1.0; 4]);
        scene.layer(None);
        scene.rect(Rect::new(10.0, 10.0, 20.0, 20.0), [1.0; 4]);
        scene.finish();
        let before = scene.layers().len();
        let a = Rect::new(0.0, 0.0, 100.0, 40.0);
        let b = Rect::new(20.0, 40.0, 50.0, 30.0);
        scene.clip_existing_pair(a, b);
        let layers = scene.layers();
        assert_eq!(layers.len(), before * 2);
        for layer in &layers[..before] {
            let c = layer.clip.unwrap();
            assert!(c.y >= a.y && c.bottom() <= a.bottom());
        }
        for layer in &layers[before..] {
            let c = layer.clip.unwrap();
            assert!(
                c.w == 0.0
                    || (c.y >= b.y
                        && c.bottom() <= b.bottom()
                        && c.x >= b.x
                        && c.right() <= b.right())
            );
        }
    }
}
