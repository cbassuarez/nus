//! Picture cards drawn by the real thing. Where a setting's options can be
//! drawn by the code that draws them in use (the loading bar, a scrollbar's
//! thumb, the terminal's colour policy on a real grid), each card is that,
//! with that option, not a sketch of it. Pages with a live preview show
//! these rows as cards too, so a choice is made by looking at it.
use crate::app::{fade, App};
use crate::settings::{Grade, Hit, Scrollbars, Truecolour};
use nus_render::{Rect, Scene};

/// Settings whose options have real pictures.
pub fn real(hit: Hit) -> bool {
    matches!(hit, Hit::Scrollbars(_) | Hit::BarStyle(_) | Hit::Grade(_) | Hit::Truecolour(_))
}

impl App {
    /// Draw `hit`'s option for real into `r`. False: no real picture.
    pub(crate) fn draw_real_pic(&mut self, scene: &mut Scene, r: Rect, hit: Hit) -> bool {
        match hit {
            Hit::Scrollbars(s) => self.real_scrollbars(scene, r, s),
            Hit::BarStyle(style) => {
                let page = self.real_page(scene, r);
                // Plays, so the style is seen moving; still with reduced motion.
                let v = if self.motion.reduced() { 0.62 } else {
                    self.dirty = true;
                    (crate::clock::since(self.started).as_secs_f32() * 0.28).fract()
                };
                let base = match self.load_bar.color {
                    crate::anim::BarColor::Signal | crate::anim::BarColor::Tab => self.surface.signal,
                    crate::anim::BarColor::Ink => self.theme.ink,
                };
                let th = self.px(self.load_bar.thickness);
                self.paint_load_bar(scene, page, v, style, base, 1.0, th, false);
            }
            Hit::Grade(g) => self.real_colours(scene, r, g, self.behavior.truecolour),
            Hit::Truecolour(tc) => self.real_colours(scene, r, self.behavior.grade, tc),
            _ => return false,
        }
        true
    }

    /// A page to draw on: paper, a few lines of text. Returns its body.
    fn real_page(&mut self, scene: &mut Scene, r: Rect) -> Rect {
        let t = self.theme.clone();
        scene.rect(r, t.page);
        scene.outline(r, self.px(1.0), fade(t.ink, 0.4));
        let widths = [0.62f32, 0.84, 0.46, 0.72, 0.58, 0.8];
        for (i, f) in widths.iter().enumerate() {
            let y = r.y + self.px(14.0) + i as f32 * self.px(9.0);
            if y > r.bottom() - self.px(6.0) {
                break;
            }
            scene.hline(r.x + self.px(10.0), y, (r.w - self.px(28.0)) * f, self.px(2.0), fade(t.ink, 0.22));
        }
        r
    }

    /// A list longer than its window, with that scrollbar, drawn by the
    /// same thumb code nus's own lists use.
    fn real_scrollbars(&mut self, scene: &mut Scene, r: Rect, s: Scrollbars) {
        let page = self.real_page(scene, r);
        let Some((w, always)) = self.thumb_style_for(s) else { return };
        let dim = self.theme.dim;
        let track = Rect::new(page.right() - w - self.px(3.0), page.y + self.px(3.0), w, page.h - self.px(6.0));
        if always {
            scene.rect(track, fade(dim, 0.12));
        }
        let len = track.h * 0.38;
        scene.rect(Rect::new(track.x, track.y + track.h * 0.2, track.w, len), fade(dim, 0.6));
    }

    /// One line from a program, through the real terminal: its own grid,
    /// parser and palette, and the colour policy with these options.
    fn real_colours(&mut self, scene: &mut Scene, r: Rect, grade: Grade, tc: Truecolour) {
        let t = self.theme.clone();
        scene.rect(r, t.paper);
        scene.outline(r, self.px(1.0), fade(t.ink, 0.4));
        let mut grid = nus_render::GridRenderer::new(&self.fonts, self.f.term, self.px(10.0));
        let inner = Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w - self.px(16.0), r.h - self.px(12.0));
        let (cols, rows) = grid.grid_size(inner);
        let mut term = nus_vt::Term::new(cols, rows, 0);
        self.theme.apply(&mut term.palette);
        let faint = crate::theme_edit::to_rgb(crate::surface::mix(t.paper, t.ink, 0.16));
        let script = format!(
            "\x1b[38;2;242;107;166msrc/ \x1b[38;2;89;204;242mCargo.toml \x1b[38;2;250;199;64mREADME\x1b[0m\r\n\x1b[38;2;{};{};{}m# a program's own grey\x1b[0m\r\n\x1b[32mok\x1b[0m 368 passed",
            faint.r, faint.g, faint.b
        );
        term.advance(script.as_bytes());
        grid.policy = nus_render::Policy { min_contrast: grade.ratio(), snap: tc == Truecolour::Snapped, ansi: None, remap: Vec::new() };
        grid.draw_with(scene, &mut self.fonts, &term, (inner.x, inner.y), false, nus_render::grid::CursorLook { visible: false, ..Default::default() });
    }
}
