//! Draws a `nus_vt::Term` into a rect. Rows are shaped once and cached by
//! content hash; only damaged rows (and the cursor row) are rebuilt.

use std::hash::{Hash, Hasher};

use nus_vt::{Cell, Color as VtColor, CursorShape, Flags, Modes, Term};

use crate::policy::{ensure_contrast, Policy};
use crate::scene::{Color, Instance, Rect, Scene};
use crate::text::{FontId, FontSystem, Metrics};

struct CachedRow {
    hash: u64,
    /// Instances positioned relative to the row's top-left.
    bg: Vec<Instance>,
    fg: Vec<Instance>,
}

pub struct GridRenderer {
    pub font: FontId,
    pub px: f32,
    pub metrics: Metrics,
    /// What happens to the program's colours on the way to the screen;
    /// the host sets it per pane (the theme's grade, a program's own).
    pub policy: Policy,
    rows: Vec<CachedRow>,
    text: String,
    col_of: Vec<usize>,
}

/// How the app wants the cursor drawn, over what the program asked for.
#[derive(Clone, Copy, Debug)]
pub struct CursorLook {
    /// Force a shape (None = the shell's own via DECSCUSR).
    pub shape: Option<CursorShape>,
    /// Force a colour (None = the palette's cursor colour).
    pub color: Option<Color>,
    /// Beam / underline thickness in physical px.
    pub weight: f32,
    /// False during the off half of a blink, or while the app draws a
    /// gliding cursor itself.
    pub visible: bool,
    /// Draw a hollow box when unfocused (else nothing).
    pub hollow_unfocused: bool,
}

impl Default for CursorLook {
    fn default() -> Self {
        CursorLook {
            shape: None,
            color: None,
            weight: 2.0,
            visible: true,
            hollow_unfocused: true,
        }
    }
}

fn to_color(c: nus_vt::Rgb) -> Color {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        1.0,
    ]
}

impl GridRenderer {
    pub fn new(fonts: &FontSystem, font: FontId, px: f32) -> GridRenderer {
        GridRenderer {
            font,
            px,
            metrics: fonts.metrics(font, px),
            policy: Policy::default(),
            rows: Vec::new(),
            text: String::new(),
            col_of: Vec::new(),
        }
    }

    pub fn set_font(&mut self, fonts: &FontSystem, font: FontId, px: f32) {
        self.font = font;
        self.px = px;
        self.metrics = fonts.metrics(font, px);
        self.rows.clear();
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.metrics.advance, self.metrics.line_height)
    }

    /// How many columns/rows fit in `rect`.
    pub fn grid_size(&self, rect: Rect) -> (usize, usize) {
        let cols = (rect.w / self.metrics.advance).floor().max(2.0) as usize;
        let rows = (rect.h / self.metrics.line_height).floor().max(1.0) as usize;
        (cols, rows)
    }

    /// Draw the terminal's visible grid with its top-left at `origin`.
    /// Pushes into the scene's current layer.
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        fonts: &mut FontSystem,
        term: &Term,
        origin: (f32, f32),
        focused: bool,
    ) {
        self.draw_with(scene, fonts, term, origin, focused, CursorLook::default());
    }

    /// `draw`, with the app's say on the cursor.
    pub fn draw_with(
        &mut self,
        scene: &mut Scene,
        fonts: &mut FontSystem,
        term: &Term,
        origin: (f32, f32),
        focused: bool,
        look: CursorLook,
    ) {
        let view = term.grid().display_lines(&[]);
        self.draw_view(scene, fonts, term, origin, focused, look, &view);
    }

    /// `draw_with`, through a display list: folded rows are skipped (the
    /// host draws them), the rest come from their absolute lines.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_view(
        &mut self,
        scene: &mut Scene,
        fonts: &mut FontSystem,
        term: &Term,
        origin: (f32, f32),
        focused: bool,
        look: CursorLook,
        view: &[nus_vt::grid::Display],
    ) {
        let (cw, ch) = self.cell_size();
        let baseline = self.metrics.baseline;
        let grid = term.grid();
        let rows = grid.rows();
        let cursor_abs = grid.abs_row(term.cursor().row);
        let palette = &term.palette;
        let cursor = *term.cursor();
        let show_cursor =
            term.modes().contains(Modes::SHOW_CURSOR) && grid.display_offset == 0 && look.visible;
        let shape = look.shape.unwrap_or(term.cursor_style().shape);
        let cursor_rgb = look
            .color
            .unwrap_or(to_color(palette.get(nus_vt::palette::CURSOR)));
        let weight = look.weight.max(1.0);
        let hollow = look.hollow_unfocused;
        let default_bg = to_color(palette.get(nus_vt::palette::BG));
        let policy = &self.policy;
        let policy_stamp = policy.stamp();
        let sixteen: [nus_vt::Rgb; 16] = std::array::from_fn(|i| palette.get(i));
        self.rows.resize_with(rows, || CachedRow {
            hash: 0,
            bg: Vec::new(),
            fg: Vec::new(),
        });

        let empty = nus_vt::grid::Row::blank(grid.cols(), &nus_vt::cell::Cell::default());
        for r in 0..rows {
            let (row, cursor_here) = match view.get(r) {
                Some(nus_vt::grid::Display::Line(abs)) => (
                    grid.row_abs(*abs).unwrap_or(&empty),
                    show_cursor && *abs == cursor_abs,
                ),
                _ => (&empty, false),
            };
            let mut h = std::hash::DefaultHasher::new();
            for c in &row.cells {
                c.ch.hash(&mut h);
                c.flags.bits().hash(&mut h);
                std::mem::discriminant(&c.fg).hash(&mut h);
                std::mem::discriminant(&c.bg).hash(&mut h);
                match c.fg {
                    VtColor::Indexed(i) => i.hash(&mut h),
                    VtColor::Rgb(a, b, d) => (a, b, d).hash(&mut h),
                    VtColor::Default => {}
                }
                match c.bg {
                    VtColor::Indexed(i) => i.hash(&mut h),
                    VtColor::Rgb(a, b, d) => (a, b, d).hash(&mut h),
                    VtColor::Default => {}
                }
            }
            if cursor_here {
                (
                    cursor.col,
                    shape as u8,
                    focused,
                    (weight * 4.0) as u32,
                    hollow,
                )
                    .hash(&mut h);
                for c in cursor_rgb {
                    ((c * 255.0) as u32).hash(&mut h);
                }
            }
            // Palette changes invalidate everything; fold a cheap sample in.
            palette.get(nus_vt::palette::FG).r.hash(&mut h);
            policy_stamp.hash(&mut h);
            let hash = h.finish();
            if self.rows[r].hash != hash || self.rows[r].hash == 0 {
                let cached = &mut self.rows[r];
                cached.hash = hash;
                cached.bg.clear();
                cached.fg.clear();
                self.text.clear();
                self.col_of.clear();
                for (c, cell) in row.cells.iter().enumerate() {
                    if cell.flags.contains(Flags::WIDE_SPACER) {
                        continue;
                    }
                    let is_cursor = cursor_here && c == cursor.col;
                    let block = is_cursor && shape == CursorShape::Block && focused;
                    let (fg, bg) =
                        resolve(cell, palette, policy, &sixteen, block, cursor_rgb, default_bg);
                    let x = c as f32 * cw;
                    if let Some(bgc) = bg {
                        let w = if cell.flags.contains(Flags::WIDE) {
                            2.0 * cw
                        } else {
                            cw
                        };
                        cached
                            .bg
                            .push(Instance::rect(Rect::new(x, 0.0, w, ch), bgc));
                    }
                    if is_cursor && !block {
                        let r = match (shape, focused) {
                            (_, false) => None, // hollow: drawn below
                            (CursorShape::Underline, true) => {
                                Some(Rect::new(x, ch - weight, cw, weight))
                            }
                            (CursorShape::Beam, true) => Some(Rect::new(x, 0.0, weight, ch)),
                            _ => None,
                        };
                        if let Some(r) = r {
                            cached.bg.push(Instance::rect(r, cursor_rgb));
                        }
                        if !focused && hollow {
                            let t = 1.0;
                            cached
                                .bg
                                .push(Instance::rect(Rect::new(x, 0.0, cw, t), cursor_rgb));
                            cached
                                .bg
                                .push(Instance::rect(Rect::new(x, ch - t, cw, t), cursor_rgb));
                            cached
                                .bg
                                .push(Instance::rect(Rect::new(x, 0.0, t, ch), cursor_rgb));
                            cached.bg.push(Instance::rect(
                                Rect::new(x + cw - t, 0.0, t, ch),
                                cursor_rgb,
                            ));
                        }
                    }
                    if cell.flags.intersects(Flags::ANY_UNDERLINE)
                        && !cell.flags.contains(Flags::HIDDEN)
                    {
                        let ulc = cell
                            .ul
                            .map(|u| to_color(palette.resolve(u, true)))
                            .unwrap_or(fg);
                        cached
                            .bg
                            .push(Instance::rect(Rect::new(x, baseline + 2.0, cw, 1.0), ulc));
                    }
                    if cell.flags.contains(Flags::STRIKE) {
                        cached.bg.push(Instance::rect(
                            Rect::new(x, (ch * 0.5).round(), cw, 1.0),
                            fg,
                        ));
                    }
                    if cell.ch != ' ' && !cell.flags.contains(Flags::HIDDEN) {
                        let start = self.text.len();
                        self.text.push(cell.ch);
                        for _ in start..self.text.len() {
                            self.col_of.push(c);
                        }
                    } else {
                        self.text.push(' ');
                        self.col_of.push(c);
                    }
                }
                if !self.text.trim().is_empty() {
                    for g in fonts.shape(self.font, self.px, &self.text) {
                        let Some(a) = fonts.glyph(g.font, self.px, g.id) else {
                            continue;
                        };
                        let c = self.col_of[g.cluster as usize];
                        let cell = &row.cells[c];
                        let is_cursor = cursor_here && c == cursor.col;
                        let block = is_cursor && shape == CursorShape::Block && focused;
                        let (fg, _) =
                            resolve(cell, palette, policy, &sixteen, block, cursor_rgb, default_bg);
                        let x = (c as f32 * cw + g.x_offset + a.left as f32).round();
                        let y = (baseline - g.y_offset - a.top as f32).round();
                        cached.fg.push(Instance::glyph(
                            x,
                            y,
                            a.width as f32,
                            a.height as f32,
                            a.uv,
                            fg,
                        ));
                    }
                }
            }
        }

        let (ox, oy) = origin;
        for (r, cached) in self.rows.iter().enumerate() {
            let y = oy + r as f32 * ch;
            for i in &cached.bg {
                let mut i = *i;
                i.pos[0] += ox;
                i.pos[1] += y;
                scene.push(i);
            }
        }
        for (r, cached) in self.rows.iter().enumerate() {
            let y = oy + r as f32 * ch;
            for i in &cached.fg {
                let mut i = *i;
                i.pos[0] += ox;
                i.pos[1] += y;
                scene.push(i);
            }
        }
    }
}

/// One of the program's colours through the palette and the policy: a
/// program's own sixteen over the theme's, remaps, the snap for anything
/// that isn't one of the sixteen.
fn place(
    c: VtColor,
    is_fg: bool,
    palette: &nus_vt::Palette,
    policy: &Policy,
    sixteen: &[nus_vt::Rgb; 16],
) -> nus_vt::Rgb {
    let of_sixteen = matches!(c, VtColor::Indexed(0..=15) | VtColor::Default);
    let rgb = match (c, &policy.ansi) {
        (VtColor::Indexed(i @ 0..=15), Some(own)) => own[i as usize],
        _ => palette.resolve(c, is_fg),
    };
    policy.place(rgb, of_sixteen, sixteen)
}

#[allow(clippy::too_many_arguments)]
fn resolve(
    cell: &Cell,
    palette: &nus_vt::Palette,
    policy: &Policy,
    sixteen: &[nus_vt::Rgb; 16],
    block_cursor: bool,
    cursor_rgb: Color,
    default_bg: Color,
) -> (Color, Option<Color>) {
    let mut fg = cell.fg;
    let mut bg = cell.bg;
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
        if fg == VtColor::Default {
            fg = VtColor::Indexed(0);
        }
    }
    if cell.flags.contains(Flags::BOLD) {
        if let VtColor::Indexed(i @ 0..=7) = fg {
            fg = VtColor::Indexed(i + 8);
        }
    }
    let mut fgc = to_color(place(fg, true, palette, policy, sixteen));
    if cell.flags.contains(Flags::DIM) {
        fgc = [fgc[0] * 0.6, fgc[1] * 0.6, fgc[2] * 0.6, 1.0];
    }
    if block_cursor {
        return (default_bg, Some(cursor_rgb));
    }
    let bgc = if bg == VtColor::Default && !cell.flags.contains(Flags::INVERSE) {
        None
    } else {
        Some(to_color(place(bg, false, palette, policy, sixteen)))
    };
    // The grade: what can't be read against its background is walked
    // until it can. A blank cell has nothing to read.
    if policy.min_contrast > 1.0 && cell.ch != ' ' && cell.ch != ' ' {
        fgc = ensure_contrast(fgc, bgc.unwrap_or(default_bg), policy.min_contrast);
    }
    (fgc, bgc)
}
