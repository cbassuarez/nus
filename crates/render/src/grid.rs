//! Draws a `nus_vt::Term` into a rect. Rows are shaped once and cached by
//! content hash. Caret motion and fading reuse the cached glyph geometry.

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
    /// Opacity of the active core and its reversed ink, independent of mode.
    pub opacity: f32,
    /// Halo strength, 0..1. Dormant carets and replacement marks never glow.
    pub glow: f32,
    /// Requested linear-light core gain; the display caps the usable headroom.
    pub hdr_gain: f32,
    /// A known replacement target, independent of the caret's shape and blink.
    pub replacement: bool,
    /// Physical-pixel displacement of the focused core during motion. The
    /// replacement marker and dormant location remain at the real destination.
    pub offset: (f32, f32),
}

impl Default for CursorLook {
    fn default() -> Self {
        CursorLook {
            shape: None,
            color: None,
            weight: 2.0,
            visible: true,
            hollow_unfocused: true,
            opacity: 1.0,
            glow: 0.0,
            hdr_gain: 1.0,
            replacement: false,
            offset: (0.0, 0.0),
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

    /// Add space to the grid itself, so selection, cursor and PTY columns agree.
    pub fn set_spacing(&mut self, fonts: &FontSystem, line: f32, tracking: f32) {
        self.metrics = fonts.metrics(self.font, self.px);
        let extra = self.metrics.line_height * (line.clamp(1.0, 1.8) - 1.0);
        self.metrics.line_height = (self.metrics.line_height + extra).round();
        self.metrics.baseline += (extra * 0.5).round();
        self.metrics.advance = (self.metrics.advance + tracking.clamp(0.0, 8.0)).max(1.0);
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
        let shape = look.shape.unwrap_or(term.cursor_style().shape);
        let show_cursor = term.modes().contains(Modes::SHOW_CURSOR)
            && grid.display_offset == 0
            && look.visible
            && shape != CursorShape::Hidden;
        let cursor_rgb = look
            .color
            .unwrap_or(to_color(palette.get(nus_vt::palette::CURSOR)));
        let weight = look.weight.max(1.0).min(cw.min(ch));
        let opacity = if look.opacity.is_finite() {
            look.opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let default_bg = to_color(palette.get(nus_vt::palette::BG));
        let policy = &self.policy;
        let policy_stamp = policy.stamp();
        let atlas = fonts.atlas_generation();
        let sixteen: [nus_vt::Rgb; 16] = std::array::from_fn(|i| palette.get(i));
        self.rows.resize_with(rows, || CachedRow {
            hash: 0,
            bg: Vec::new(),
            fg: Vec::new(),
        });

        let empty = nus_vt::grid::Row::blank(grid.cols(), &nus_vt::cell::Cell::default());
        for r in 0..rows {
            let row = match view.get(r) {
                Some(nus_vt::grid::Display::Line(abs)) => grid.row_abs(*abs).unwrap_or(&empty),
                _ => &empty,
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
            // The caret is composited over cached rows, so its phase never
            // invalidates text. Real palette changes still rebuild the ink.
            for rgb in sixteen.iter().copied().chain([
                palette.get(nus_vt::palette::FG),
                palette.get(nus_vt::palette::BG),
            ]) {
                (rgb.r, rgb.g, rgb.b).hash(&mut h);
            }
            policy_stamp.hash(&mut h);
            // Rows hold atlas coordinates: a fresh atlas rebuilds them all.
            atlas.hash(&mut h);
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
                    let (fg, bg) = resolve(cell, palette, policy, &sixteen, default_bg);
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
                    for g in fonts.shape(self.font, self.px, &self.text).iter() {
                        let Some(a) = fonts.glyph(g.font, self.px, g.id) else {
                            continue;
                        };
                        let c = self.col_of[g.cluster as usize];
                        let cell = &row.cells[c];
                        let (fg, _) = resolve(cell, palette, policy, &sixteen, default_bg);
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
        // Find the caret through the same display list as the text, including
        // folded rows. A wide character owns both cells, even if a program
        // positions its cursor on the continuation cell.
        let cursor_cell = show_cursor
            .then(|| {
                let r = view.iter().take(rows).position(
                    |line| matches!(line, nus_vt::grid::Display::Line(abs) if *abs == cursor_abs),
                )?;
                let row = grid.row_abs(cursor_abs)?;
                let mut col = cursor.col;
                if row.cells.get(col)?.flags.contains(Flags::WIDE_SPACER) {
                    col = col.saturating_sub(1);
                }
                let cell = row.cells.get(col)?;
                let width = if cell.flags.contains(Flags::WIDE) {
                    cw * 2.0
                } else {
                    cw
                };
                let rect = Rect::new(ox + col as f32 * cw, oy + r as f32 * ch, width, ch);
                let (_, bg) = resolve(cell, palette, policy, &sixteen, default_bg);
                Some((
                    rect,
                    bg.unwrap_or(default_bg),
                    cell.ch != ' ' && cell.ch != '\0',
                ))
            })
            .flatten();
        let mut block = None;
        if let Some((cell, paper, has_target)) = cursor_cell {
            if focused {
                let destination = cell;
                let offset = |value: f32| if value.is_finite() { value } else { 0.0 };
                let cell = Rect::new(
                    cell.x + offset(look.offset.0),
                    cell.y + offset(look.offset.1),
                    cell.w,
                    cell.h,
                );
                let mut color = cursor_rgb;
                color[3] *= opacity;
                let core = match shape {
                    CursorShape::Beam => Rect::new(cell.x, cell.y, weight, cell.h),
                    CursorShape::Underline => {
                        Rect::new(cell.x, cell.bottom() - weight, cell.w, weight)
                    }
                    _ => cell,
                };
                if shape == CursorShape::HollowBlock {
                    scene.outline(core, 1.0, color);
                } else {
                    let glow = look.glow
                        * if crate::policy::luminance(paper) > 0.5 {
                            0.2
                        } else {
                            1.0
                        };
                    scene.caret(core, color, glow, look.hdr_gain);
                }
                if shape == CursorShape::Block && color[3] > 0.0 {
                    let ink = ensure_contrast(paper, cursor_rgb, 4.5);
                    block = Some((cell, ink, color[3].clamp(0.0, 1.0)));
                }
                if look.replacement && has_target {
                    scene.caret_replacement(
                        Rect::new(
                            destination.x,
                            destination.bottom() + 2.0,
                            destination.w,
                            1.0,
                        ),
                        cursor_rgb,
                    );
                }
            } else if look.hollow_unfocused {
                let mut color = cursor_rgb;
                color[3] *= 0.45;
                scene.outline(cell, 1.0, color);
            }
        }
        for (r, cached) in self.rows.iter().enumerate() {
            let y = oy + r as f32 * ch;
            for i in &cached.fg {
                let mut i = *i;
                i.pos[0] += ox;
                i.pos[1] += y;
                if let Some((cell, ink, amount)) = block {
                    glyph_through_caret(scene, i, cell, ink, amount);
                } else {
                    scene.push(i);
                }
            }
        }
    }
}

/// Reuse the exact atlas rectangle beneath a cell caret. Cropping its UVs
/// instead of reshaping a character keeps bearings, baselines and ligatures
/// fixed, including glyphs that overhang the cell from a neighboring column.
fn glyph_through_caret(scene: &mut Scene, glyph: Instance, cell: Rect, ink: Color, amount: f32) {
    let bounds = Rect::new(glyph.pos[0], glyph.pos[1], glyph.size[0], glyph.size[1]);
    let inside = bounds.intersect(&cell);
    if inside.w <= 0.0 || inside.h <= 0.0 {
        scene.push(glyph);
        return;
    }
    let regions = [
        Rect::new(bounds.x, bounds.y, bounds.w, inside.y - bounds.y),
        Rect::new(
            bounds.x,
            inside.bottom(),
            bounds.w,
            bounds.bottom() - inside.bottom(),
        ),
        Rect::new(bounds.x, inside.y, inside.x - bounds.x, inside.h),
        Rect::new(
            inside.right(),
            inside.y,
            bounds.right() - inside.right(),
            inside.h,
        ),
    ];
    for region in regions {
        if region.w > 0.0 && region.h > 0.0 {
            scene.push(crop_glyph(glyph, region));
        }
    }
    scene.push(crop_glyph(glyph, inside).caret_ink(ink, amount));
}

fn crop_glyph(mut glyph: Instance, bounds: Rect) -> Instance {
    let [x, y] = glyph.pos;
    let [w, h] = glyph.size;
    let [u0, v0, u1, v1] = glyph.uv;
    glyph.uv = [
        u0 + (u1 - u0) * (bounds.x - x) / w,
        v0 + (v1 - v0) * (bounds.y - y) / h,
        u0 + (u1 - u0) * (bounds.right() - x) / w,
        v0 + (v1 - v0) * (bounds.bottom() - y) / h,
    ];
    glyph.pos = [bounds.x, bounds.y];
    glyph.size = [bounds.w, bounds.h];
    glyph
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

fn resolve(
    cell: &Cell,
    palette: &nus_vt::Palette,
    policy: &Policy,
    sixteen: &[nus_vt::Rgb; 16],
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
    let bgc = if bg == VtColor::Default && !cell.flags.contains(Flags::INVERSE) {
        None
    } else {
        Some(to_color(place(bg, false, palette, policy, sixteen)))
    };
    // The grade: what can't be read against its background is walked
    // until it can. A blank cell has nothing to read.
    if policy.min_contrast > 1.0 && cell.ch != ' ' && cell.ch != '\0' {
        fgc = ensure_contrast(fgc, bgc.unwrap_or(default_bg), policy.min_contrast);
    }
    (fgc, bgc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::bundled;

    fn fixture() -> (FontSystem, GridRenderer, Term) {
        let mut fonts = FontSystem::new();
        let font = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
        let grid = GridRenderer::new(&fonts, font, 16.0);
        let mut term = Term::new(12, 3, 100);
        term.advance(b"fjord\r");
        (fonts, grid, term)
    }

    fn draw(
        fonts: &mut FontSystem,
        grid: &mut GridRenderer,
        term: &Term,
        focused: bool,
        look: CursorLook,
    ) -> Scene {
        let mut scene = Scene::new();
        grid.draw_with(&mut scene, fonts, term, (10.0, 20.0), focused, look);
        scene.finish();
        scene
    }

    #[test]
    fn blink_reuses_shaped_geometry_and_fades_ink_with_the_core() {
        let (mut fonts, mut grid, term) = fixture();
        let base = CursorLook {
            color: Some([1.0; 4]),
            glow: 0.5,
            ..Default::default()
        };
        let lit = draw(&mut fonts, &mut grid, &term, true, base);
        let glyphs = grid.rows[0].fg.clone();
        let stamp = grid.rows[0].hash;
        let half = draw(
            &mut fonts,
            &mut grid,
            &term,
            true,
            CursorLook {
                opacity: 0.5,
                ..base
            },
        );
        assert_eq!(stamp, grid.rows[0].hash);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&glyphs),
            bytemuck::cast_slice::<_, u8>(&grid.rows[0].fg)
        );
        let lit_ink: Vec<_> = lit.instances().iter().filter(|i| i.kind == 21).collect();
        let half_ink: Vec<_> = half.instances().iter().filter(|i| i.kind == 21).collect();
        assert!(!lit_ink.is_empty());
        assert_eq!(lit_ink.len(), half_ink.len());
        for (lit, half) in lit_ink.iter().zip(&half_ink) {
            assert_eq!((lit.pos, lit.size, lit.uv), (half.pos, half.size, half.uv));
            assert_eq!(half.phase, 0.5);
        }
        assert_eq!(
            half.instances()
                .iter()
                .find(|i| i.kind == 20)
                .unwrap()
                .color[3],
            0.5
        );
        let off = draw(
            &mut fonts,
            &mut grid,
            &term,
            true,
            CursorLook {
                opacity: 0.0,
                ..base
            },
        );
        assert!(off.instances().iter().all(|i| i.kind != 20 && i.kind != 21));
        assert_eq!(stamp, grid.rows[0].hash);
    }

    #[test]
    fn protocol_hidden_scrollback_and_folded_rows_never_leave_a_caret() {
        let (mut fonts, mut grid, mut term) = fixture();
        let look = CursorLook {
            glow: 1.0,
            hdr_gain: 2.0,
            replacement: true,
            ..Default::default()
        };
        term.advance(b"\x1b[?25l");
        let hidden = draw(&mut fonts, &mut grid, &term, true, look);
        assert!(hidden
            .instances()
            .iter()
            .all(|i| i.kind != 20 && i.kind != 21));
        term.advance(b"\x1b[?25h");
        let hidden = draw(
            &mut fonts,
            &mut grid,
            &term,
            true,
            CursorLook {
                shape: Some(CursorShape::Hidden),
                ..look
            },
        );
        assert!(hidden
            .instances()
            .iter()
            .all(|i| i.kind != 20 && i.kind != 21));
        let mut folded = Scene::new();
        grid.draw_view(&mut folded, &mut fonts, &term, (0.0, 0.0), true, look, &[]);
        assert!(folded.instances().is_empty());
        term.advance(b"\r\na\r\nb\r\nc\r\nd");
        term.grid_mut().scroll_display(1);
        assert!(term.grid().display_offset > 0);
        let history = draw(&mut fonts, &mut grid, &term, true, look);
        assert!(history
            .instances()
            .iter()
            .all(|i| i.kind != 20 && i.kind != 21));
    }

    #[test]
    fn program_palette_and_wide_cell_survive_the_shared_material() {
        let (mut fonts, mut grid, mut term) = fixture();
        term.palette.set_override(
            nus_vt::palette::CURSOR,
            nus_vt::Rgb {
                r: 80,
                g: 160,
                b: 240,
            },
        );
        term.grid_mut().row_mut(0).cells[0]
            .flags
            .insert(Flags::WIDE);
        term.grid_mut().row_mut(0).cells[1]
            .flags
            .insert(Flags::WIDE_SPACER);
        term.advance(b"\x1b[2G");
        let scene = draw(&mut fonts, &mut grid, &term, true, CursorLook::default());
        let core = scene.instances().iter().find(|i| i.kind == 20).unwrap();
        assert!((core.size[0] - 2.0 * core.uv[0] - grid.cell_size().0 * 2.0).abs() < 0.001);
        assert_eq!(core.pos[0] + core.uv[0], 10.0);
        assert_eq!(
            core.color,
            [80.0 / 255.0, 160.0 / 255.0, 240.0 / 255.0, 1.0]
        );
        let dormant = draw(
            &mut fonts,
            &mut grid,
            &term,
            false,
            CursorLook {
                glow: 1.0,
                hdr_gain: 3.0,
                ..Default::default()
            },
        );
        assert!(dormant
            .instances()
            .iter()
            .all(|i| i.kind != 20 && i.kind != 21));
        assert!(dormant
            .instances()
            .iter()
            .any(|i| i.kind == 0 && i.color[3] == 0.45));
    }

    #[test]
    fn replacement_stays_visible_through_blink_and_disappears_at_blank_end() {
        let (mut fonts, mut grid, mut term) = fixture();
        let look = CursorLook {
            opacity: 0.0,
            replacement: true,
            ..Default::default()
        };
        let off = draw(&mut fonts, &mut grid, &term, true, look);
        let rule_y = 20.0 + grid.cell_size().1 + 2.0;
        assert!(off
            .instances()
            .iter()
            .any(|i| i.kind == 0 && i.pos[1] == rule_y && i.size[1] == 1.0));
        term.advance(b"\x1b[6G");
        let end = draw(&mut fonts, &mut grid, &term, true, look);
        assert!(!end
            .instances()
            .iter()
            .any(|i| i.kind == 0 && i.pos[1] == rule_y));
    }

    #[test]
    fn glyph_mask_preserves_original_texel_positions_and_outside_ink() {
        let glyph = Instance::glyph(8.0, 4.0, 16.0, 20.0, [0.1, 0.2, 0.3, 0.6], [1.0; 4]);
        let mut scene = Scene::new();
        glyph_through_caret(
            &mut scene,
            glyph,
            Rect::new(12.0, 8.0, 8.0, 12.0),
            [0.0, 0.0, 0.0, 1.0],
            0.4,
        );
        let mut area = 0.0;
        for part in scene.instances() {
            area += part.size[0] * part.size[1];
            let expected_u = glyph.uv[0]
                + (part.pos[0] - glyph.pos[0]) / glyph.size[0] * (glyph.uv[2] - glyph.uv[0]);
            let expected_v = glyph.uv[1]
                + (part.pos[1] - glyph.pos[1]) / glyph.size[1] * (glyph.uv[3] - glyph.uv[1]);
            assert!((part.uv[0] - expected_u).abs() < 0.00001);
            assert!((part.uv[1] - expected_v).abs() < 0.00001);
        }
        assert_eq!(area, glyph.size[0] * glyph.size[1]);
        assert_eq!(scene.instances().iter().filter(|i| i.kind == 21).count(), 1);
        assert_eq!(scene.instances().last().unwrap().phase, 0.4);
    }

    #[test]
    fn moving_cell_recolors_only_ink_under_its_actual_fractional_position() {
        let (mut fonts, mut grid, mut term) = fixture();
        term.advance(b"MMMM\r");
        draw(&mut fonts, &mut grid, &term, true, CursorLook::default());
        let cached_ink = grid.rows[0].fg.clone();
        let stamp = grid.rows[0].hash;
        let (cw, ch) = grid.cell_size();
        let shift = (cw * 0.5, ch * 0.2);
        let moved = draw(
            &mut fonts,
            &mut grid,
            &term,
            true,
            CursorLook {
                offset: shift,
                replacement: true,
                ..Default::default()
            },
        );
        assert_eq!(stamp, grid.rows[0].hash);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&cached_ink),
            bytemuck::cast_slice::<_, u8>(&grid.rows[0].fg)
        );
        let core = moved.instances().iter().find(|i| i.kind == 20).unwrap();
        let bounds = Rect::new(core.pos[0] + core.uv[0], core.pos[1] + core.uv[0], cw, ch);
        assert!((bounds.x - 10.0 - shift.0).abs() < 0.001);
        assert!((bounds.y - 20.0 - shift.1).abs() < 0.001);
        let inverse: Vec<_> = moved.instances().iter().filter(|i| i.kind == 21).collect();
        assert!(
            inverse.len() >= 2,
            "motion should expose portions of both neighboring glyphs"
        );
        for part in inverse {
            assert!(part.pos[0] >= bounds.x && part.pos[1] >= bounds.y);
            assert!(part.pos[0] + part.size[0] <= bounds.right() + 0.001);
            assert!(part.pos[1] + part.size[1] <= bounds.bottom() + 0.001);
        }
        // Mode information never follows a cosmetic displacement.
        assert!(moved
            .instances()
            .iter()
            .any(|i| i.kind == 0 && i.pos == [10.0, 20.0 + ch + 2.0]));
        let dormant = draw(
            &mut fonts,
            &mut grid,
            &term,
            false,
            CursorLook {
                offset: shift,
                ..Default::default()
            },
        );
        assert!(dormant
            .instances()
            .iter()
            .any(|i| i.kind == 0 && i.pos == [10.0, 20.0]));
    }
}
