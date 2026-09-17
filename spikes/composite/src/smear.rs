//! The smeared caret, as Neovide does it. A port of Neovide's cursor
//! renderer (src/renderer/cursor_renderer, MIT, © the Neovide
//! contributors): the cursor is four corners, each a critically damped
//! spring toward its destination; on a jump the corners facing the
//! direction of travel get the short animation and the ones behind the
//! long one, so the body stretches and then catches up. Drawn as one
//! quad through the four corners. Settings mirror Neovide's names.

use nus_render::{Color, Instance, Scene};

/// Neovide's `CriticallyDampedSpringAnimation`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub position: f32,
    velocity: f32,
}

impl Spring {
    pub fn update(&mut self, dt: f32, animation_length: f32) -> bool {
        if animation_length <= dt {
            self.reset();
            return false;
        }
        if self.position == 0.0 {
            return false;
        }
        // A critically damped spring (a PD controller): omega so the target
        // is reached within 2% in `animation_length`.
        let zeta = 1.0;
        let omega = 4.0 / (zeta * animation_length);
        let a = self.position;
        let b = self.position * omega + self.velocity;
        let c = (-omega * dt).exp();
        self.position = (a + b * dt) * c;
        self.velocity = c * (-a * omega - b * dt * omega + b);
        if self.position.abs() < 0.01 {
            self.reset();
            false
        } else {
            true
        }
    }

    pub fn reset(&mut self) {
        self.position = 0.0;
        self.velocity = 0.0;
    }
}

/// Neovide's cursor settings, the ones that matter here.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub animation_length: f32,
    pub short_animation_length: f32,
    pub trail_size: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { animation_length: 0.150, short_animation_length: 0.04, trail_size: 1.0 }
    }
}

const STANDARD_CORNERS: [(f32, f32); 4] = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Block,
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
struct Corner {
    current: (f32, f32),
    relative: (f32, f32),
    previous_destination: (f32, f32),
    x: Spring,
    y: Spring,
    animation_length: f32,
}

impl Corner {
    fn new(relative: (f32, f32)) -> Corner {
        Corner { current: (0.0, 0.0), relative, previous_destination: (-1000.0, -1000.0), x: Spring::default(), y: Spring::default(), animation_length: 0.0 }
    }

    fn destination(&self, centre: (f32, f32), dims: (f32, f32)) -> (f32, f32) {
        (centre.0 + self.relative.0 * dims.0, centre.1 + self.relative.1 * dims.1)
    }

    fn update(&mut self, dims: (f32, f32), centre: (f32, f32), dt: f32, immediate: bool) -> bool {
        let dest = self.destination(centre, dims);
        if dest != self.previous_destination {
            self.x.position = dest.0 - self.current.0;
            self.y.position = dest.1 - self.current.1;
            self.previous_destination = dest;
        }
        if immediate {
            self.x.reset();
            self.y.reset();
            self.current = dest;
            return false;
        }
        let mut animating = self.x.update(dt, self.animation_length);
        animating |= self.y.update(dt, self.animation_length);
        self.current = (dest.0 - self.x.position, dest.1 - self.y.position);
        animating
    }

    fn jump(&mut self, s: &Settings, centre: (f32, f32), dims: (f32, f32), alignment: f32) {
        let dest = self.destination(centre, dims);
        let jump = ((dest.0 - self.previous_destination.0) / dims.0, (dest.1 - self.previous_destination.1) / dims.1);
        self.animation_length = if jump.0.abs() <= 2.001 && jump.1.abs() < 0.001 {
            // Short jumps of up to two cells (typing) take the fast path.
            s.animation_length.min(s.short_animation_length)
        } else {
            let leading = s.animation_length * (1.0 - s.trail_size).clamp(0.0, 1.0);
            let trailing = s.animation_length;
            trailing + (leading - trailing) * alignment
        };
    }

    /// How much this corner faces the direction of travel: corners in
    /// front move faster than the ones behind.
    fn alignment(&self, dims: (f32, f32), centre: (f32, f32)) -> f32 {
        let dest = self.destination(centre, dims);
        let len = (self.relative.0 * self.relative.0 + self.relative.1 * self.relative.1).sqrt().max(1e-6);
        let corner_dir = (self.relative.0 / len, self.relative.1 / len);
        let d = (dest.0 - self.previous_destination.0, dest.1 - self.previous_destination.1);
        let dl = (d.0 * d.0 + d.1 * d.1).sqrt();
        if dl < 1e-6 {
            return 0.0;
        }
        (d.0 / dl) * corner_dir.0 + (d.1 / dl) * corner_dir.1
    }
}

/// One caret's four corners.
pub struct Smear {
    corners: [Corner; 4],
    shape: Option<(Shape, f32)>,
    /// Where the cursor was last asked to go (top-left, pixels).
    destination: (f32, f32),
    jumped: bool,
    last: Option<std::time::Instant>,
    pub animating: bool,
}

impl Default for Smear {
    fn default() -> Self {
        Self::new()
    }
}

impl Smear {
    pub fn new() -> Smear {
        Smear { corners: STANDARD_CORNERS.map(Corner::new), shape: None, destination: (0.0, 0.0), jumped: false, last: None, animating: false }
    }

    /// Neovide's `set_cursor_shape`: the corners' relative positions for
    /// a block, a bar of `cell_percentage` width, or an underline.
    fn set_shape(&mut self, shape: Shape, cell_percentage: f32) {
        for (i, c) in self.corners.iter_mut().enumerate() {
            let (x, y) = STANDARD_CORNERS[i];
            c.relative = match shape {
                Shape::Block => (x, y),
                Shape::Vertical => ((x + 0.5) * cell_percentage - 0.5, y),
                Shape::Horizontal => (x, -((-y + 0.5) * cell_percentage - 0.5)),
            };
        }
        self.shape = Some((shape, cell_percentage));
    }

    /// The cursor should be at `top_left` (pixels), `dims` the cell.
    pub fn set_destination(&mut self, top_left: (f32, f32)) {
        if top_left != self.destination {
            self.destination = top_left;
            self.jumped = true;
        }
    }

    /// Advance the springs; returns whether anything still moves.
    pub fn animate(&mut self, s: &Settings, shape: Shape, cell_percentage: f32, dims: (f32, f32), immediate: bool) -> bool {
        let now = std::time::Instant::now();
        let dt = self.last.map(|t| (now - t).as_secs_f32().min(0.1)).unwrap_or(1.0 / 60.0);
        self.last = Some(now);
        if self.shape != Some((shape, cell_percentage)) {
            self.set_shape(shape, cell_percentage);
        }
        let centre = (self.destination.0 + dims.0 * 0.5, self.destination.1 + dims.1 * 0.5);
        if self.jumped {
            let al: [f32; 4] = std::array::from_fn(|i| self.corners[i].alignment(dims, centre));
            let min = al.iter().copied().fold(f32::INFINITY, f32::min);
            let max = al.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let range = max - min;
            for (i, c) in self.corners.iter_mut().enumerate() {
                let a = (al[i] - min) / range;
                let a = if a.is_finite() { a.clamp(0.0, 1.0) } else { 1.0 };
                c.jump(s, centre, dims, a);
            }
            self.jumped = false;
        }
        let mut animating = false;
        for c in self.corners.iter_mut() {
            animating |= c.update(dims, centre, dt, immediate);
        }
        self.animating = animating;
        animating
    }

    /// The quad through the corners.
    pub fn draw(&self, scene: &mut Scene, color: Color) {
        let pts = self.corners.map(|c| [c.current.0, c.current.1]);
        scene.push(Instance::quad(pts, color));
    }
}
