//! Live previews for settings: a small picture of the thing a page changes,
//! drawn from the settings as they are this frame. Nothing here is a still;
//! flip a choice and the preview is already different. The row under the
//! pointer is passed in, and the part of the picture it governs is marked,
//! so it is always clear what a setting touches.

use nus_render::text::icons;
use nus_render::{Rect, Scene, Style};

use crate::app::{fade, App, Pane};
use crate::settings::workspace::Hit as W;
use crate::settings::{Hit, Slider};

pub const SOUND: usize = 1;
pub const START: usize = 2;
pub const SIDEBAR: usize = 3;
pub const TABS: usize = 4;
pub const TERMINAL: usize = 5;
pub const HATCH: usize = 8;
pub const SYNC: usize = 12;
pub const PROMPT: usize = 16;
pub const ASSISTANTS: usize = crate::settings::SEC_ASSISTANTS;

/// Pages that carry a live preview (and compact rows beside it).
pub fn has_live(section: usize) -> bool {
    matches!(section, SOUND | START | SIDEBAR | TABS | TERMINAL | HATCH | SYNC | PROMPT | ASSISTANTS)
}

impl App {
    /// How tall the preview wants to be at this width.
    pub(crate) fn live_height(&self, section: usize, w: f32) -> f32 {
        let k = match section {
            SIDEBAR => 0.95,
            TABS => 1.05,
            START => 0.9,
            SOUND => 0.7,
            TERMINAL => 0.62,
            HATCH => 0.8,
            SYNC => 0.8,
            PROMPT => 1.0,
            ASSISTANTS => 0.9,
            _ => 0.8,
        };
        (w * k).clamp(self.px(240.0), self.px(520.0))
    }

    pub(crate) fn draw_live(&mut self, scene: &mut Scene, r: Rect, section: usize, focus: Option<Hit>) {
        let t = self.theme.clone();
        scene.rect(r, self.paper());
        scene.outline(r, self.px(1.0), t.ink);
        let cap = Style { color: t.dim, px: self.px(9.5), tracking: self.px(1.2), ..self.label() };
        let word = if focus.is_some() { "LIVE PREVIEW · POINTING AT YOUR ROW" } else { "LIVE PREVIEW · UPDATES AS YOU CHANGE THINGS" };
        let word = self.fit(cap, word, r.w - self.px(28.0));
        let dot = Rect::new(r.x + self.px(12.0), r.y + self.px(11.0), self.px(6.0), self.px(6.0));
        scene.push(nus_render::Instance::rounded(dot, dot.w * 0.5, self.surface.signal));
        self.fonts.draw(scene, cap, r.x + self.px(24.0), r.y + self.px(18.0), &word);
        let body = Rect::new(r.x + self.px(12.0), r.y + self.px(28.0), r.w - self.px(24.0), r.h - self.px(40.0));
        match section {
            SIDEBAR => self.live_sidebar(scene, body, focus),
            TABS => self.live_tabs(scene, body, focus),
            START => self.live_start(scene, body, focus),
            SOUND => self.live_sound(scene, body, focus),
            // The atom: the model, the provider and the level, alive.
            ASSISTANTS => self.draw_intel_atom(scene, body),
            TERMINAL => self.live_terminal(scene, body, focus),
            HATCH => self.live_hatch(scene, body, focus),
            SYNC => self.live_sync(scene, body, focus),
            PROMPT => self.live_prompt(scene, body, focus),
            _ => {}
        }
    }

    // ── Shared pieces ────────────────────────────────────────────────────

    fn lv_small(&self) -> Style {
        Style { px: self.px(10.0), ..self.ui() }
    }

    fn lv_tiny(&self) -> Style {
        Style { px: self.px(8.5), tracking: self.px(0.8), ..self.label() }
    }

    fn lv_text(&mut self, scene: &mut Scene, st: Style, x: f32, y: f32, text: &str, max_w: f32) -> f32 {
        let text = self.fit(st, text, max_w.max(0.0));
        self.fonts.draw(scene, st, x, y, &text)
    }

    /// The part a row governs: a signal outline, and its name beside it
    /// when there is room.
    fn lv_mark(&mut self, scene: &mut Scene, r: Rect, on: bool) {
        if !on {
            return;
        }
        let pad = self.px(2.0);
        let m = Rect::new(r.x - pad, r.y - pad, r.w + pad * 2.0, r.h + pad * 2.0);
        scene.rect(m, fade(self.surface.signal, 0.12));
        scene.outline(m, self.px(1.5), self.surface.signal);
    }

    /// A window: its frame and title strip. Returns the body.
    fn lv_window(&mut self, scene: &mut Scene, r: Rect) -> Rect {
        let t = self.theme.clone();
        scene.rect(r, self.paper());
        scene.outline(r, self.px(1.0), t.ink);
        let strip = self.px(12.0);
        scene.hline(r.x, r.y + strip, r.w, self.px(1.0), fade(t.ink, 0.35));
        for i in 0..3 {
            let d = Rect::new(r.x + self.px(5.0) + i as f32 * self.px(6.0), r.y + self.px(4.0), self.px(4.0), self.px(4.0));
            scene.push(nus_render::Instance::rounded(d, d.w * 0.5, fade(t.ink, 0.35)));
        }
        Rect::new(r.x, r.y + strip + self.px(1.0), r.w, r.h - strip - self.px(1.0))
    }

    /// Faint lines standing in for a page or a document.
    fn lv_lines(&self, scene: &mut Scene, r: Rect, n: usize, color: nus_render::Color) {
        let step = r.h / (n as f32 + 1.0);
        for i in 0..n {
            let w = r.w * [0.9, 0.62, 0.78, 0.45, 0.84][i % 5];
            scene.rect(Rect::new(r.x, r.y + step * (i as f32 + 0.6), w, self.px(2.0)), color);
        }
    }

    /// A caption under a picture: what you would see happen.
    fn lv_caption(&mut self, scene: &mut Scene, x: f32, y: f32, w: f32, text: &str) -> f32 {
        let st = Style { color: self.theme.dim, ..self.lv_small() };
        let mut yy = y;
        for line in crate::reader::wrap(&self.fonts, st, text, w).into_iter().take(3) {
            yy += self.px(13.0);
            self.fonts.draw(scene, st, x, yy, &line);
        }
        yy - y
    }

    /// Tab rows for a mini sidebar: what is really open here.
    fn lv_tab_rows(&self) -> Vec<((&'static str, &'static str), String, bool)> {
        let mut v: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.pinned && t.peek.is_none())
            .map(|(i, t)| {
                let icon = match &t.left {
                    Pane::Term(_) => icons::TERMINAL,
                    Pane::Web(_) => icons::GLOBE,
                    Pane::Settings(_) => icons::SETTINGS,
                    Pane::Home(h) if h.library => icons::BOOK,
                    Pane::Home(_) | Pane::Hints(_) => icons::HOME,
                    Pane::Editor(_) => icons::CODE,
                    Pane::Ports(_) => icons::PORTS,
                    Pane::Downloads(_) => icons::DOWNLOAD,
                };
                (icon, t.title(), i == self.active)
            })
            .take(6)
            .collect();
        if v.len() < 3 {
            for (icon, title) in [(icons::TERMINAL, "~/project"), (icons::GLOBE, "docs.rs"), (icons::GLOBE, "localhost:3000")] {
                if v.len() >= 4 {
                    break;
                }
                v.push((icon, title.to_string(), false));
            }
        }
        v
    }

    // ── Sidebar ──────────────────────────────────────────────────────────

    fn live_sidebar(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        use crate::settings::HeaderStyle;
        use crate::surface::{Fullscreen, HoverFrom, Side};
        let t = self.theme.clone();
        let ink = t.ink;
        let hdr = self.header.clone();
        let rules = self.sidebar_rules.clone();
        let caption_h = self.px(44.0);
        let win = Rect::new(body.x, body.y, body.w, body.h - caption_h);
        let inner = self.lv_window(scene, win);
        // The sidebar at its real share of a 1100 px window.
        let icons_only = rules.compact || rules.width <= 104.0;
        let frac = if rules.compact { 48.0 / 1100.0 } else { (rules.width / 1100.0).clamp(0.05, 0.42) };
        let sw = (inner.w * frac).max(self.px(if icons_only { 26.0 } else { 70.0 }));
        let right = rules.side == Side::Right;
        let sb = Rect::new(if right { inner.right() - sw } else { inner.x }, inner.y, sw, inner.h);
        let page = Rect::new(if right { inner.x } else { sb.right() }, inner.y, inner.w - sw, inner.h);
        self.lv_lines(scene, Rect::new(page.x + self.px(14.0), page.y + self.px(10.0), page.w - self.px(28.0), page.h * 0.5), 5, fade(ink, 0.12));
        let floating = !self.sidebar;
        if floating {
            scene.rect(sb, fade(self.paper(), 0.94));
            let edge = if right { sb.x } else { sb.right() };
            scene.push(nus_render::Instance::hazard(Rect::new(edge - self.px(1.0), sb.y, self.px(2.0), sb.h), self.px(2.0), ink, self.paper(), self.px(6.0)));
        } else {
            scene.vline(if right { sb.x } else { sb.right() }, sb.y, sb.h, self.px(1.0), ink);
        }
        let side_focus = matches!(focus, Some(Hit::Side(_) | Hit::Slider(Slider::SidebarWidth, ..) | Hit::Pin(_) | Hit::HoverFrom(_) | Hit::Slider(Slider::Grace, ..) | Hit::Fullscreen(_)));
        let mut y = sb.y + self.px(4.0);
        let pad = self.px(5.0);
        let x0 = sb.x + pad;
        let wcol = crate::app::fade(self.surface.signal, 1.0);
        // Header.
        let head = if hdr.style == HeaderStyle::Rail && !icons_only {
            let rail = Rect::new(if right { sb.right() - self.px(12.0) } else { sb.x }, sb.y, self.px(12.0), sb.h);
            if !hdr.rail_hover || matches!(focus, Some(Hit::HdrRailHover(_))) {
                scene.vline(if right { rail.x } else { rail.right() }, rail.y, rail.h, self.px(1.0), fade(ink, 0.3));
                for i in 0..3 {
                    let sq = Rect::new(rail.x + self.px(3.0), rail.y + self.px(5.0) + i as f32 * self.px(10.0), self.px(6.0), self.px(6.0));
                    scene.rect(sq, if i == 0 { wcol } else { fade(ink, 0.35) });
                }
            }
            let hx = if right { x0 } else { x0 + self.px(12.0) };
            let r = Rect::new(hx, y, sb.w - self.px(12.0) - pad * 2.0, self.px(if hdr.masthead { 20.0 } else { 14.0 }));
            let st = if hdr.masthead { Style { font: self.f.serif, px: self.px(13.0), color: ink, tracking: 0.0 } } else { Style { color: ink, ..self.lv_tiny() } };
            let name = if hdr.masthead { self.window_name() } else { self.window_name().to_uppercase() };
            self.lv_text(scene, st, r.x, r.bottom() - self.px(3.0), &name, r.w);
            y = r.bottom() + self.px(2.0);
            self.lv_mark(scene, Rect::new(sb.x, sb.y, sb.w, r.bottom() - sb.y), matches!(focus, Some(Hit::HdrStyle(_) | Hit::HdrMasthead(_) | Hit::HdrRailHover(_) | Hit::HdrName(_))));
            r
        } else {
            let row = Rect::new(sb.x, y, sb.w, self.px(if hdr.masthead && !icons_only { 22.0 } else { 16.0 }));
            let sq = Rect::new(x0, row.y + (row.h - self.px(7.0)) * 0.5, self.px(7.0), self.px(7.0));
            scene.rect(sq, wcol);
            if hdr.show_name && !icons_only {
                let st = if hdr.masthead { Style { font: self.f.serif, px: self.px(13.0), color: ink, tracking: 0.0 } } else { Style { color: ink, ..self.lv_tiny() } };
                let name = if hdr.masthead { self.window_name() } else { self.window_name().to_uppercase() };
                self.lv_text(scene, st, sq.right() + self.px(4.0), row.bottom() - self.px(4.0), &name, row.w - self.px(40.0));
            }
            // NEW TAB in the header, with its kinds caret.
            if hdr.header_button {
                let bw = self.px(if hdr.kinds_caret && !icons_only { 20.0 } else { 12.0 });
                let b = if icons_only { Rect::new(sb.x + (sb.w - bw) * 0.5, row.bottom() + self.px(2.0), bw, self.px(12.0)) } else { Rect::new(row.right() - pad - bw, row.y + (row.h - self.px(12.0)) * 0.5, bw, self.px(12.0)) };
                let press = hdr.flash && matches!(focus, Some(Hit::HdrFlash(true)));
                if press { scene.rect(b, fade(self.surface.signal, 0.5)); }
                scene.outline(b, self.px(1.0), ink);
                self.fonts.draw_icon(scene, icons::PLUS, self.px(8.0), b.x + self.px(2.0), b.y + self.px(2.0), ink);
                if hdr.kinds_caret && !icons_only {
                    self.fonts.draw_icon(scene, icons::CARET_DOWN, self.px(7.0), b.x + self.px(11.0), b.y + self.px(3.0), ink);
                }
                self.lv_mark(scene, b, matches!(focus, Some(Hit::HdrButton(_) | Hit::HdrCaret(_) | Hit::HdrFlash(_))));
                if icons_only { y = b.bottom(); }
            }
            self.lv_mark(scene, Rect::new(sb.x, row.y, sb.w, row.h), matches!(focus, Some(Hit::HdrStyle(_) | Hit::HdrMasthead(_) | Hit::HdrName(_))));
            y = y.max(row.bottom());
            row
        };
        let _ = head;
        if hdr.dateline && !icons_only {
            let st = Style { color: t.dim, ..self.lv_tiny() };
            let place = self.workspace.as_ref().and_then(|w| w.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "~".into());
            let line = format!("{place} · {} TABS · 2 PORTS", self.tabs.len());
            let dl = Rect::new(sb.x, y, sb.w, self.px(11.0));
            self.lv_text(scene, st, x0, dl.bottom() - self.px(2.0), &line, sb.w - pad * 2.0);
            self.lv_mark(scene, dl, matches!(focus, Some(Hit::HdrDateline(_))));
            y = dl.bottom();
        } else if matches!(focus, Some(Hit::HdrDateline(false))) {
            y += self.px(2.0);
        }
        scene.hline(sb.x, y + self.px(2.0), sb.w, self.px(1.0), fade(ink, 0.3));
        y += self.px(6.0);
        // Pinned tiles.
        let pins: Vec<(&str, &str)> = self.pins.items.iter().take(if icons_only { 3 } else { 4 }).map(|p| p.icon()).collect();
        if !pins.is_empty() {
            let cols = if icons_only { 1 } else { pins.len().min(if sw > self.px(120.0) { 4 } else { 2 }) };
            let gap = self.px(3.0);
            let tw = (sb.w - pad * 2.0 - gap * (cols as f32 - 1.0)) / cols as f32;
            let th = if icons_only { self.px(14.0) } else { self.px(18.0) };
            let start = y;
            for (i, icon) in pins.iter().enumerate() {
                let r = Rect::new(x0 + (i % cols) as f32 * (tw + gap), y + (i / cols) as f32 * (th + gap), tw, th);
                scene.push(nus_render::Instance::rounded(r, self.px(3.0), fade(t.tint, 0.9)));
                if rules.pin_display == crate::pins::Display::Preview && !icons_only && i % 2 == 1 {
                    self.lv_lines(scene, Rect::new(r.x + self.px(3.0), r.y + self.px(2.0), r.w - self.px(6.0), r.h - self.px(4.0)), 3, fade(ink, 0.3));
                } else {
                    self.fonts.draw_icon(scene, *icon, self.px(8.0), r.x + (r.w - self.px(8.0)) * 0.5, r.y + (r.h - self.px(8.0)) * 0.5, ink);
                }
            }
            let rows = pins.len().div_ceil(cols);
            y += rows as f32 * (th + gap) + self.px(3.0);
            self.lv_mark(scene, Rect::new(sb.x, start, sb.w, y - start), matches!(focus, Some(Hit::PinDisplay(_))));
        }
        // Tabs.
        let rows = self.lv_tab_rows();
        let rh = self.px(if icons_only { 15.0 } else { 14.0 });
        let start = y;
        let footer_h = self.px((rules.footer_row / 36.0).clamp(0.5, 2.0) * 14.0);
        for (icon, title, active) in rows.iter() {
            if y + rh > sb.bottom() - footer_h - rh {
                break;
            }
            let row = Rect::new(sb.x + self.px(2.0), y, sb.w - self.px(4.0), rh - self.px(1.0));
            if *active { scene.rect(row, fade(t.tint, 1.0)); }
            let isz = self.px(8.0);
            if icons_only {
                let use_fav = rules.small_tabs == crate::sidebar::SmallTabs::Favicons && *icon == icons::GLOBE;
                if rules.small_tabs == crate::sidebar::SmallTabs::Preview && *icon == icons::GLOBE {
                    let pr = Rect::new(row.x + self.px(3.0), row.y + self.px(1.0), row.w - self.px(6.0), row.h - self.px(2.0));
                    scene.outline(pr, self.px(1.0), fade(ink, 0.4));
                    self.lv_lines(scene, Rect::new(pr.x + self.px(2.0), pr.y, pr.w - self.px(4.0), pr.h), 2, fade(ink, 0.35));
                } else if use_fav {
                    let d = Rect::new(row.x + (row.w - isz) * 0.5, row.y + (row.h - isz) * 0.5, isz, isz);
                    scene.push(nus_render::Instance::rounded(d, self.px(2.0), fade(self.surface.signal, 0.8)));
                } else {
                    self.fonts.draw_icon(scene, *icon, isz, row.x + (row.w - isz) * 0.5, row.y + (row.h - isz) * 0.5, ink);
                }
            } else {
                self.fonts.draw_icon(scene, *icon, isz, row.x + self.px(3.0), row.y + (row.h - isz) * 0.5, ink);
                let st = Style { color: if *active { ink } else { t.dim }, ..self.lv_small() };
                self.lv_text(scene, st, row.x + self.px(15.0), row.bottom() - self.px(3.5), title, row.w - self.px(18.0));
            }
            y += rh;
        }
        self.lv_mark(scene, Rect::new(sb.x, start, sb.w, y - start), matches!(focus, Some(Hit::SmallTabs(_) | Hit::Compact(_))));
        if hdr.next_row {
            let row = Rect::new(sb.x + self.px(2.0), y, sb.w - self.px(4.0), rh - self.px(1.0));
            scene.hline(row.x, row.y, row.w, self.px(1.0), fade(ink, 0.25));
            self.fonts.draw_icon(scene, icons::PLUS, self.px(7.0), if icons_only { row.x + (row.w - self.px(7.0)) * 0.5 } else { row.x + self.px(3.0) }, row.y + self.px(3.0), t.dim);
            if !icons_only {
                let st = Style { color: t.dim, ..self.lv_tiny() };
                self.lv_text(scene, st, row.x + self.px(15.0), row.bottom() - self.px(4.0), "NEW TAB", row.w - self.px(18.0));
            }
            self.lv_mark(scene, row, matches!(focus, Some(Hit::HdrNextRow(_))));
        }
        // Footer.
        let foot = Rect::new(sb.x, sb.bottom() - footer_h, sb.w, footer_h);
        scene.hline(foot.x, foot.y, foot.w, self.px(1.0), fade(ink, 0.3));
        let n = if icons_only { 1 } else { ((sb.w - pad * 2.0) / self.px(14.0)).floor().clamp(1.0, 5.0) as usize };
        for i in 0..n {
            let ic = [icons::SETTINGS, icons::DOWNLOAD, icons::PORTS, icons::BELL, icons::PLANET][i];
            self.fonts.draw_icon(scene, ic, self.px(8.0), x0 + i as f32 * self.px(14.0) + if icons_only { (sb.w - pad * 2.0 - self.px(8.0)) * 0.5 } else { 0.0 }, foot.y + (foot.h - self.px(8.0)) * 0.5, t.dim);
        }
        self.lv_mark(scene, foot, matches!(focus, Some(Hit::Slider(Slider::FooterSize, ..))));
        self.lv_mark(scene, sb, side_focus);
        // What it means, in words.
        let words = match focus {
            Some(Hit::HoverFrom(_) | Hit::Slider(Slider::Grace, ..)) => format!(
                "Unpinned, the sidebar slides in when the pointer reaches {} and hides {} ms after it leaves.",
                if rules.hover_from == HoverFrom::ScreenEdge { format!("the {} edge of the screen", if right { "right" } else { "left" }) } else { "the edge of this window".into() },
                rules.grace_ms
            ),
            Some(Hit::Fullscreen(_)) => match rules.fullscreen {
                Fullscreen::Hover => "In fullscreen the sidebar hides and slides in at the edge.".into(),
                Fullscreen::Hidden => "In fullscreen the sidebar stays out of the way until you pin it.".into(),
                Fullscreen::Pinned => "In fullscreen the sidebar stays exactly as it is here.".into(),
            },
            _ => format!(
                "{} · {} · {}",
                if floating { "Slides in on hover" } else { "Always visible" },
                if rules.compact { "icons only".to_string() } else { format!("{} px wide", rules.width as u32) },
                if right { "on the right" } else { "on the left" }
            ),
        };
        self.lv_caption(scene, body.x, win.bottom() + self.px(2.0), body.w, &words);
    }

    // ── Tabs ─────────────────────────────────────────────────────────────

    fn live_tabs(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        use crate::settings::{Links, OpenedBy, PromptUrl};
        let t = self.theme.clone();
        let ink = t.ink;
        let b = self.behavior.clone();
        let gap = self.px(10.0);
        let cell_w = (body.w - gap) * 0.5;
        let cap_h = self.px(40.0);
        let cell_h = ((body.h - self.px(64.0) - gap) * 0.5).max(self.px(60.0));
        let cells = [
            Rect::new(body.x, body.y, cell_w, cell_h),
            Rect::new(body.x + cell_w + gap, body.y, cell_w, cell_h),
            Rect::new(body.x, body.y + cell_h + gap, cell_w, cell_h),
            Rect::new(body.x + cell_w + gap, body.y + cell_h + gap, cell_w, cell_h),
        ];
        // A scene: a mini window with a sidebar of named rows and a page.
        let mini = |app: &mut App, scene: &mut Scene, cell: Rect, rows: &[(&str, bool, bool)], split: Option<&str>, marked: bool| -> Rect {
            let win = Rect::new(cell.x, cell.y, cell.w, cell.h - cap_h);
            let inner = app.lv_window(scene, win);
            let sw = inner.w * 0.34;
            scene.vline(inner.x + sw, inner.y, inner.h, app.px(1.0), fade(ink, 0.5));
            let mut y = inner.y + app.px(3.0);
            for (name, active, fresh) in rows {
                let indent = if name.starts_with('↳') { app.px(8.0) } else { 0.0 };
                let row = Rect::new(inner.x + app.px(2.0) + indent, y, sw - app.px(4.0) - indent, app.px(11.0));
                if *active { scene.rect(row, t.tint); }
                if *fresh { scene.rect(row, fade(app.surface.signal, 0.25)); }
                let st = Style { color: ink, px: app.px(8.5), ..app.ui() };
                app.lv_text(scene, st, row.x + app.px(2.0), row.bottom() - app.px(2.5), name.trim_start_matches('↳'), row.w - app.px(4.0));
                y += app.px(12.0);
            }
            let page = Rect::new(inner.x + sw + app.px(1.0), inner.y, inner.w - sw - app.px(1.0), inner.h);
            if let Some(name) = split {
                let half = Rect::new(page.x + page.w * 0.5, page.y, page.w * 0.5, page.h);
                scene.vline(half.x, half.y, half.h, app.px(1.0), ink);
                scene.rect(Rect::new(half.x + app.px(1.0), half.y, half.w - app.px(1.0), half.h), fade(app.surface.signal, 0.1));
                let st = Style { color: ink, px: app.px(8.5), ..app.ui() };
                app.lv_text(scene, st, half.x + app.px(4.0), half.y + app.px(11.0), name, half.w - app.px(6.0));
                app.lv_lines(scene, Rect::new(half.x + app.px(4.0), half.y + app.px(14.0), half.w - app.px(8.0), half.h - app.px(18.0)), 3, fade(ink, 0.18));
                app.lv_lines(scene, Rect::new(page.x + app.px(4.0), page.y + app.px(4.0), page.w * 0.5 - app.px(8.0), page.h - app.px(8.0)), 4, fade(ink, 0.18));
            } else {
                app.lv_lines(scene, Rect::new(page.x + app.px(5.0), page.y + app.px(5.0), page.w - app.px(10.0), page.h - app.px(10.0)), 4, fade(ink, 0.18));
            }
            app.lv_mark(scene, win, marked);
            page
        };
        // 1 · A link on a page.
        let (rows, split): (Vec<(&str, bool, bool)>, Option<&str>) = match b.links {
            Links::Stack => (vec![("docs.rs", true, false), ("↳serde · linked", false, true), ("~/project", false, false)], None),
            Links::Split => (vec![("docs.rs", true, false), ("~/project", false, false)], Some("serde · linked")),
            Links::NewTab => (vec![("docs.rs", true, false), ("~/project", false, false), ("serde · linked", false, true)], None),
        };
        let page = mini(self, scene, cells[0], &rows, split, matches!(focus, Some(Hit::Links(_))));
        let st = Style { color: self.surface.signal, px: self.px(8.5), ..self.ui() };
        self.fonts.draw(scene, st, page.x + self.px(5.0), page.bottom() - self.px(5.0), "↗ link");
        let words = match b.links { Links::Stack => "A link from a page nests under it in the sidebar.", Links::Split => "A link from a page opens beside it.", Links::NewTab => "A link from a page gets its own tab." };
        self.lv_caption(scene, cells[0].x, cells[0].bottom() - cap_h, cells[0].w, words);
        // 2 · A URL typed in a shell.
        let (rows, split): (Vec<(&str, bool, bool)>, Option<&str>) = match b.prompt_url {
            PromptUrl::Split => (vec![("~/project", true, false), ("docs.rs", false, false)], Some("localhost:3000")),
            PromptUrl::NewTab => (vec![("~/project", true, false), ("docs.rs", false, false), ("localhost:3000", false, true)], None),
        };
        let page = mini(self, scene, cells[1], &rows, split, matches!(focus, Some(Hit::PromptUrl(_))));
        let term = Style { font: self.f.term, px: self.px(8.0), color: ink, tracking: 0.0 };
        self.lv_text(scene, term, page.x + self.px(4.0), page.y + self.px(11.0), "$ open :3000", page.w * if split.is_some() { 0.5 } else { 1.0 } - self.px(6.0));
        let words = match b.prompt_url { PromptUrl::Split => "An address typed in a shell opens beside the shell.", PromptUrl::NewTab => "An address typed in a shell opens as a new tab." };
        self.lv_caption(scene, cells[1].x, cells[1].bottom() - cap_h, cells[1].w, words);
        // 3 · Another app opens a tab.
        let front = b.opened_by_others == OpenedBy::Front;
        let rows = vec![("~/project", !front, false), ("docs.rs", false, false), ("from another app", front, true)];
        let page = mini(self, scene, cells[2], &rows, None, matches!(focus, Some(Hit::OpenedBy(_) | Hit::Dedupe(_))));
        if !front || b.dedupe {
            let toast = Rect::new(page.x + self.px(4.0), page.bottom() - self.px(18.0), page.w - self.px(8.0), self.px(14.0));
            scene.rect(toast, ink);
            let st = Style { color: self.paper(), px: self.px(8.0), ..self.ui() };
            let msg = if b.dedupe && matches!(focus, Some(Hit::Dedupe(_))) { "Already open · switch to it?" } else { "Opened Behind · docs.rs" };
            self.lv_text(scene, st, toast.x + self.px(4.0), toast.bottom() - self.px(4.0), msg, toast.w - self.px(8.0));
        }
        let words = if matches!(focus, Some(Hit::Dedupe(_))) {
            if b.dedupe { "Opening a page that's already open offers to switch to it." } else { "Duplicates open without a notice." }
        } else if front { "Tabs opened by other apps come to the front." } else { "Tabs opened by other apps wait behind, with a notice." };
        self.lv_caption(scene, cells[2].x, cells[2].bottom() - cap_h, cells[2].w, words);
        // 4 · Split panes: corner controls and the divider.
        let win = Rect::new(cells[3].x, cells[3].y, cells[3].w, cells[3].h - cap_h);
        let inner = self.lv_window(scene, win);
        let at = inner.x + inner.w * if b.pane_divider && matches!(focus, Some(Hit::PaneDivider(_))) { 0.42 + 0.08 * (crate::clock::since(self.started).as_secs_f32() * 2.0).sin() } else { 0.5 };
        scene.vline(at, inner.y, inner.h, self.px(if b.pane_divider { 2.0 } else { 1.0 }), ink);
        self.lv_lines(scene, Rect::new(inner.x + self.px(4.0), inner.y + self.px(4.0), at - inner.x - self.px(8.0), inner.h - self.px(8.0)), 4, fade(ink, 0.18));
        self.lv_lines(scene, Rect::new(at + self.px(4.0), inner.y + self.px(4.0), inner.right() - at - self.px(8.0), inner.h - self.px(8.0)), 4, fade(ink, 0.18));
        if b.pane_controls == crate::panes::Controls::Near {
            for (k, ic) in [icons::ARROWS_OUT, icons::SWAP, icons::CLOSE].iter().enumerate() {
                let r = Rect::new(inner.right() - self.px(12.0) * (k as f32 + 1.0), inner.y + self.px(3.0), self.px(10.0), self.px(10.0));
                scene.rect(r, self.paper());
                scene.outline(r, self.px(1.0), ink);
                self.fonts.draw_icon(scene, *ic, self.px(7.0), r.x + self.px(1.5), r.y + self.px(1.5), ink);
            }
        }
        self.lv_mark(scene, win, matches!(focus, Some(Hit::PaneControls(_) | Hit::PaneDivider(_))));
        let words = format!(
            "{} · {}",
            if b.pane_controls == crate::panes::Controls::Near { "Controls appear near a pane's corner" } else { "No corner controls" },
            if b.pane_divider { "drag the divider to resize" } else { "the divider stays put" }
        );
        self.lv_caption(scene, cells[3].x, cells[3].bottom() - cap_h, cells[3].w, &words);
        // Idle pages: a timeline.
        let line_y = body.y + cell_h * 2.0 + gap + self.px(26.0);
        let x0 = body.x + self.px(6.0);
        let x1 = body.right() - self.px(6.0);
        scene.hline(x0, line_y, x1 - x0, self.px(1.5), ink);
        let st = Style { color: ink, ..self.lv_tiny() };
        self.fonts.draw(scene, st, x0, line_y - self.px(6.0), "IDLE PAGE");
        let horizon = 24.0 * 60.0 * 8.0;
        let pos = |min: f32| x0 + (x1 - x0) * (min / horizon).sqrt().clamp(0.0, 1.0);
        let mark = |app: &mut App, scene: &mut Scene, min: f32, word: String, on: bool, below: bool| {
            let x = pos(min);
            scene.vline(x, line_y - app.px(4.0), app.px(8.0), app.px(2.0), if on { app.surface.signal } else { ink });
            let st = Style { color: if on { app.surface.signal } else { t.dim }, ..app.lv_tiny() };
            let w = app.fonts.measure(st, &word);
            let wx = if below { (x - w * 0.5).clamp(x0, x1 - w) } else { (x + app.px(4.0)).clamp(x0 + app.px(80.0), x1 - w) };
            app.fonts.draw(scene, st, wx, if below { line_y + app.px(14.0) } else { line_y - app.px(6.0) }, &word);
        };
        if b.sleep_after_min > 0 {
            mark(self, scene, b.sleep_after_min as f32, format!("PAUSES · {} MIN", b.sleep_after_min), matches!(focus, Some(Hit::SleepAfter(_))), true);
        }
        if b.archive_after_h > 0 {
            mark(self, scene, b.archive_after_h as f32 * 60.0, format!("CLOSES · {} H", b.archive_after_h), matches!(focus, Some(Hit::ArchiveAfter(_))), false);
        }
        if b.sleep_after_min == 0 && b.archive_after_h == 0 {
            let st = Style { color: t.dim, ..self.lv_tiny() };
            self.fonts.draw(scene, st, x0 + self.px(70.0), line_y - self.px(6.0), "· NEVER SLEEPS OR CLOSES");
        }
        let close = if b.close_asks { "Closing a busy shell asks first." } else { "Closing a busy shell stops it at once." };
        let line_y = line_y + self.px(4.0);
        let tidy = match b.tidy_every { crate::settings::TidyEvery::Off => "Tab groups suggested when you ask.", crate::settings::TidyEvery::Hourly => "Tab groups suggested hourly.", crate::settings::TidyEvery::Daily => "Tab groups suggested daily." };
        self.lv_caption(scene, body.x, line_y + self.px(16.0), body.w, &format!("{close} {tidy}"));
    }

    // ── Start / new tab ──────────────────────────────────────────────────

    fn live_start(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        use crate::settings::{NewWindow, SplashMode, WindowStart};
        let t = self.theme.clone();
        let ink = t.ink;
        let b = self.behavior.clone();
        let gap = self.px(10.0);
        let launch_w = body.w * 0.56;
        let cap_h = self.px(52.0);
        // At launch: the screen, the window on it, and what it opens with.
        let screen = Rect::new(body.x, body.y + self.px(12.0), launch_w, body.h - cap_h - self.px(12.0));
        let st = Style { color: ink, ..self.lv_tiny() };
        self.fonts.draw(scene, st, body.x, body.y + self.px(8.0), "WHEN NUS LAUNCHES");
        scene.rect(screen, fade(ink, 0.08));
        scene.outline(screen, self.px(1.0), fade(ink, 0.4));
        let bar = self.px(6.0);
        scene.rect(Rect::new(screen.x, screen.y, screen.w, bar), fade(ink, 0.15));
        let area = Rect::new(screen.x, screen.y + bar, screen.w, screen.h - bar);
        let win = match b.window_start {
            WindowStart::Fullscreen => Rect::new(screen.x, screen.y, screen.w, screen.h),
            WindowStart::Maximized => area,
            WindowStart::Centered => Rect::new(area.x + area.w * 0.12, area.y + area.h * 0.1, area.w * 0.76, area.h * 0.8),
            WindowStart::Last => Rect::new(area.x + area.w * 0.06, area.y + area.h * 0.08, area.w * 0.7, area.h * 0.78),
        };
        let restoring = b.remember && self.last_session.as_ref().is_some_and(|s| !s.tabs.is_empty());
        let inner = self.lv_window(scene, win);
        self.lv_mark(scene, win, matches!(focus, Some(Hit::WindowStart(_))));
        let splash_on = matches!(focus, Some(Hit::Splash(_) | Hit::Slider(Slider::SplashHold, ..)));
        if splash_on && b.splash != SplashMode::None {
            // The splash, as it would stand: the icon, drawing or still.
            let k = if b.splash == SplashMode::Draw { (crate::clock::since(self.started).as_secs_f32() / b.splash_hold.max(0.3)).fract() } else { 1.0 };
            let c = Rect::new(inner.x + inner.w * 0.5 - self.px(12.0), inner.y + inner.h * 0.5 - self.px(12.0), self.px(24.0), self.px(24.0));
            scene.push(nus_render::Instance::rounded(c, c.w * 0.5, fade(ink, 0.15 + 0.85 * k)));
            let st = Style { color: t.dim, ..self.lv_tiny() };
            self.fonts.draw(scene, st, inner.x + self.px(4.0), inner.bottom() - self.px(4.0), &format!("{:.1} S", b.splash_hold));
        } else {
            let sw = inner.w * 0.28;
            scene.vline(inner.x + sw, inner.y, inner.h, self.px(1.0), fade(ink, 0.5));
            let mut y = inner.y + self.px(3.0);
            if restoring {
                let names: Vec<String> = self.last_session.as_ref().map(|s| s.tabs.iter().take(5).map(|t| match &t.left {
                    Some(crate::start::Saved::Page { title, url }) => if title.is_empty() { crate::links::host(url) } else { title.clone() },
                    Some(crate::start::Saved::Shell { .. }) => t.shell.as_ref().and_then(|s| s.cwd.clone()).unwrap_or_else(|| "shell".into()),
                    Some(crate::start::Saved::File { path }) => path.rsplit('/').next().unwrap_or(path).to_string(),
                    _ => "tab".into(),
                }).collect()).unwrap_or_default();
                for n in names {
                    let st = Style { color: ink, px: self.px(7.5), ..self.ui() };
                    self.lv_text(scene, st, inner.x + self.px(3.0), y + self.px(8.0), &n, sw - self.px(5.0));
                    y += self.px(10.0);
                }
            }
            let page = Rect::new(inner.x + sw + self.px(1.0), inner.y, inner.w - sw - self.px(1.0), inner.h);
            self.lv_start_page(scene, page, b.then);
            self.lv_mark(scene, Rect::new(inner.x, inner.y, sw, inner.h), restoring && matches!(focus, Some(Hit::Remember(_) | Hit::Atlas(_))));
        }
        let splash = match b.splash { SplashMode::Draw => "the logo draws in", SplashMode::Still => "the logo, still", SplashMode::None => "no animation" };
        let words = format!(
            "{} · {}{}",
            splash,
            if restoring { "your tabs from last time come back, then " } else { "then " },
            self.lv_then_words(b.then)
        );
        self.lv_caption(scene, body.x, screen.bottom() + self.px(2.0), launch_w, &words);
        // Every new tab, and a new window.
        let x = body.x + launch_w + gap;
        let w = body.w - launch_w - gap;
        let h = (body.h - self.px(24.0) - gap - cap_h) * 0.5;
        let st = Style { color: ink, ..self.lv_tiny() };
        self.fonts.draw(scene, st, x, body.y + self.px(8.0), &format!("NEW TAB · {}", crate::settings::key("T", !cfg!(target_os = "macos"))));
        let tab = Rect::new(x, body.y + self.px(12.0), w, h);
        let inner = self.lv_window(scene, tab);
        self.lv_start_page(scene, inner, b.then);
        self.lv_mark(scene, tab, matches!(focus, Some(Hit::Then(_) | Hit::HomeLook(_) | Hit::HomeArt(_) | Hit::StartupLayout(_) | Hit::EditHomeUrl | Hit::Lead(_))));
        let y2 = tab.bottom() + gap + self.px(12.0);
        self.fonts.draw(scene, st, x, y2 - self.px(4.0), &format!("NEW WINDOW · {}", crate::settings::key("N", false)));
        let nw = Rect::new(x, y2, w, h);
        let inner = self.lv_window(scene, nw);
        match b.new_window {
            NewWindow::Prompt => self.lv_start_page(scene, inner, crate::settings::Then::Palette),
            NewWindow::Shell => self.lv_start_page(scene, inner, crate::settings::Then::Shell),
            NewWindow::Launch => self.lv_start_page(scene, inner, b.then),
        }
        self.lv_mark(scene, nw, matches!(focus, Some(Hit::NewWindow(_))));
        let words = format!("New tabs: {}.", self.lv_then_words(b.then));
        self.lv_caption(scene, x, nw.bottom() + self.px(2.0), w, &words);
    }

    fn lv_then_words(&self, then: crate::settings::Then) -> String {
        use crate::settings::Then;
        match then {
            Then::Palette => "the command palette opens".into(),
            Then::Prompt => "the Home prompt".into(),
            Then::HomePage => format!("{} opens", crate::links::host(&self.behavior.home_url)),
            Then::Layout => format!("the “{}” layout opens", if self.behavior.then_layout.is_empty() { "saved" } else { &self.behavior.then_layout }),
            Then::LastPage => "the last page you visited opens".into(),
            Then::Shell => "a shell".into(),
            Then::Restore => "your saved session".into(),
        }
    }

    /// What a start page looks like, in a pane.
    fn lv_start_page(&mut self, scene: &mut Scene, r: Rect, then: crate::settings::Then) {
        use crate::settings::{HomeLook, Then};
        let t = self.theme.clone();
        let ink = t.ink;
        let term = Style { font: self.f.term, px: self.px(7.5), color: ink, tracking: 0.0 };
        match then {
            Then::Palette => {
                scene.rect(r, fade(ink, 0.06));
                let p = Rect::new(r.x + r.w * 0.15, r.y + r.h * 0.18, r.w * 0.7, (r.h * 0.6).min(self.px(60.0)));
                scene.rect(p, self.paper());
                scene.outline(p, self.px(1.0), ink);
                self.lv_text(scene, Style { color: t.dim, ..term }, p.x + self.px(4.0), p.y + self.px(10.0), "› go to…", p.w - self.px(8.0));
                scene.hline(p.x, p.y + self.px(13.0), p.w, self.px(1.0), fade(ink, 0.3));
                self.lv_lines(scene, Rect::new(p.x + self.px(4.0), p.y + self.px(15.0), p.w - self.px(8.0), p.h - self.px(17.0)), 3, fade(ink, 0.3));
            }
            Then::Prompt | Then::Restore => {
                let look = self.behavior.home_look;
                if look == HomeLook::Art {
                    for i in 0..14 {
                        let fx = ((i * 37) % 100) as f32 / 100.0;
                        let fy = ((i * 61) % 100) as f32 / 100.0;
                        let d = Rect::new(r.x + fx * r.w, r.y + fy * r.h, self.px(3.0), self.px(3.0));
                        scene.push(nus_render::Instance::rounded(d, d.w, fade(self.surface.signal, 0.45)));
                    }
                }
                let ly = r.y + r.h * if look == HomeLook::Plate { 0.62 } else { 0.45 };
                if look == HomeLook::Plate {
                    let c = Rect::new(r.x + r.w * 0.5 - self.px(9.0), ly - self.px(26.0), self.px(18.0), self.px(18.0));
                    scene.push(nus_render::Instance::rounded(c, c.w * 0.5, ink));
                }
                scene.hline(r.x + r.w * 0.15, ly, r.w * 0.7, self.px(1.0), ink);
                self.lv_text(scene, Style { color: t.dim, ..term }, r.x + r.w * 0.15, ly - self.px(3.0), "› a URL, a command or a folder", r.w * 0.7);
            }
            Then::HomePage | Then::LastPage => {
                let host = if then == Then::HomePage { crate::links::host(&self.behavior.home_url) } else {
                    self.recent.iter().find_map(|x| match &x.item { crate::start::Saved::Page { url, .. } => Some(crate::links::host(url)), _ => None }).unwrap_or_else(|| "no page yet".into())
                };
                let bar = Rect::new(r.x + self.px(3.0), r.y + self.px(3.0), r.w - self.px(6.0), self.px(10.0));
                scene.outline(bar, self.px(1.0), fade(ink, 0.5));
                self.lv_text(scene, term, bar.x + self.px(3.0), bar.bottom() - self.px(2.5), &host, bar.w - self.px(6.0));
                self.lv_lines(scene, Rect::new(r.x + self.px(6.0), bar.bottom() + self.px(4.0), r.w - self.px(12.0), r.h - bar.h - self.px(10.0)), 4, fade(ink, 0.2));
            }
            Then::Layout => {
                let half = r.w * 0.5;
                scene.vline(r.x + half, r.y, r.h, self.px(1.0), ink);
                self.lv_text(scene, term, r.x + self.px(3.0), r.y + self.px(10.0), "$ _", half - self.px(6.0));
                self.lv_lines(scene, Rect::new(r.x + half + self.px(4.0), r.y + self.px(4.0), half - self.px(8.0), r.h - self.px(8.0)), 3, fade(ink, 0.2));
            }
            Then::Shell => {
                self.lv_text(scene, term, r.x + self.px(4.0), r.y + self.px(11.0), "~ ❯ _", r.w - self.px(8.0));
            }
        }
    }

    // ── Sound ────────────────────────────────────────────────────────────

    fn live_sound(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        let t = self.theme.clone();
        let ink = t.ink;
        let on = self.sound.prefs.enabled;
        let vol = self.sound.prefs.volume.clamp(0.0, 1.0);
        let (last, age) = self.sound.last.as_ref().map(|(n, at)| (n.clone(), crate::clock::since(*at).as_secs_f32())).unwrap_or_default();
        // A speaker and its level; the last cue rings out for a moment.
        let isz = self.px(20.0);
        self.fonts.draw_icon(scene, if on { icons::SPEAKER } else { icons::SPEAKER_OFF }, isz, body.x, body.y + self.px(2.0), ink);
        let bars = 16;
        let bw = (body.w - isz - self.px(20.0)) / bars as f32;
        let ring = if on && !last.is_empty() && age < 1.2 { 1.0 - age / 1.2 } else { 0.0 };
        if ring > 0.0 { self.dirty = true; }
        for i in 0..bars {
            let k = (i as f32 + 0.5) / bars as f32;
            let lit = on && k <= vol;
            let h = self.px(6.0) + self.px(16.0) * k * if lit { 0.6 + 0.4 * ring } else { 0.6 };
            let r = Rect::new(body.x + isz + self.px(12.0) + i as f32 * bw, body.y + self.px(24.0) - h, bw - self.px(2.0), h);
            scene.rect(r, if lit { if ring > 0.0 { self.surface.signal } else { ink } } else { fade(ink, 0.15) });
        }
        self.lv_mark(scene, Rect::new(body.x + isz + self.px(12.0), body.y, bw * bars as f32, self.px(24.0)), matches!(focus, Some(Hit::Slider(Slider::Volume, ..) | Hit::SoundOn(_))));
        let st = Style { color: t.dim, ..self.lv_small() };
        let status = if !on { "Sounds are off. Nothing below will play.".to_string() } else if last.is_empty() { format!("Volume {}% · click a sound name to hear it", (vol * 100.0).round()) } else { format!("Volume {}% · last played: {last}", (vol * 100.0).round()) };
        self.lv_text(scene, st, body.x, body.y + self.px(42.0), &status, body.w);
        // What plays when.
        let mut y = body.y + self.px(56.0);
        let row_h = self.px(15.0);
        let label = Style { color: ink, ..self.lv_small() };
        for (e, (ev, _, _)) in crate::sound::EVENTS.iter().enumerate() {
            if y + row_h > body.bottom() {
                break;
            }
            let cue = self.sound.prefs.cue_for(ev);
            let playing = on && cue.as_deref() == Some(last.as_str()) && ring > 0.0;
            let row = Rect::new(body.x, y, body.w, row_h - self.px(1.0));
            if playing { scene.rect(row, fade(self.surface.signal, 0.18 * ring + 0.05)); }
            let name = ev.replace('.', " · ");
            self.lv_text(scene, label, row.x + self.px(4.0), row.bottom() - self.px(4.0), &name, row.w * 0.55);
            let right = match &cue { Some(c) if on => c.clone(), Some(_) => "muted".into(), None => "silent".into() };
            let st = Style { color: if cue.is_some() && on { ink } else { t.dim }, ..self.lv_small() };
            let w = self.fonts.measure(st, &right);
            self.fonts.draw(scene, st, row.right() - w - self.px(4.0), row.bottom() - self.px(4.0), &right);
            self.lv_mark(scene, row, matches!(focus, Some(Hit::EventCue(k, _) | Hit::EventNext(k)) if k == e));
            y += row_h;
        }
    }

    // ── Terminal ─────────────────────────────────────────────────────────

    fn live_terminal(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        use crate::settings::{LinkClick, Truecolour};
        let t = self.theme.clone();
        let ink = t.ink;
        let b = self.behavior.clone();
        let cap_h = self.px(40.0);
        let win = Rect::new(body.x, body.y, body.w, body.h - cap_h);
        let inner = self.lv_window(scene, win);
        let px = self.px(10.0);
        let term = Style { font: self.f.term, px, color: ink, tracking: 0.0 };
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        let lh = self.px(14.0);
        let gutter = if b.blocks { self.px(12.0) } else { self.px(6.0) };
        let x0 = inner.x + gutter;
        let mut y = inner.y + self.px(14.0);
        let lamp = |app: &mut App, scene: &mut Scene, y: f32, ok: bool| {
            if b.blocks {
                let d = Rect::new(inner.x + app.px(4.0), y - app.px(7.0), app.px(5.0), app.px(5.0));
                scene.push(nus_render::Instance::rounded(d, d.w * 0.5, if ok { ansi(2) } else { ansi(1) }));
            }
        };
        // A finished command with output.
        lamp(self, scene, y, true);
        let mut x = x0;
        x += self.fonts.draw(scene, Style { color: ansi(4), ..term }, x, y, "~/nus ❯ ");
        let cmd_color = |i: usize| if b.highlight { ansi(i) } else { ink };
        x += self.fonts.draw(scene, Style { color: cmd_color(2), ..term }, x, y, "ls");
        self.fonts.draw(scene, Style { color: cmd_color(3), ..term }, x, y, " -la");
        self.lv_mark(scene, Rect::new(x0, y - lh * 0.8, inner.w * 0.5, lh), matches!(focus, Some(Hit::Highlight(_))));
        y += lh;
        // Output a program colored with its own truecolor choices.
        let own = [[0.95, 0.42, 0.65, 1.0], [0.35, 0.8, 0.95, 1.0], [0.98, 0.78, 0.25, 1.0]];
        let snapped = [ansi(5), ansi(6), ansi(3)];
        let mut x = x0;
        for (i, word) in ["src/  ", "Cargo.toml  ", "README.md"].iter().enumerate() {
            let c = if b.truecolour == Truecolour::Snapped { snapped[i] } else { own[i] };
            x += self.fonts.draw(scene, Style { color: c, ..term }, x, y, word);
        }
        self.lv_mark(scene, Rect::new(x0, y - lh * 0.8, x - x0, lh), matches!(focus, Some(Hit::Truecolour(_) | Hit::Grade(_) | Hit::ShellColours(_))));
        y += lh;
        // A long output, folded or not.
        lamp(self, scene, y, false);
        let mut x = x0;
        x += self.fonts.draw(scene, Style { color: ansi(4), ..term }, x, y, "~/nus ❯ ");
        self.fonts.draw(scene, Style { color: cmd_color(2), ..term }, x, y, "cargo build");
        y += lh;
        if b.fold_over > 0 {
            let chip = format!("▸ {} lines folded · click to open", 240.max(b.fold_over + 1));
            self.fonts.draw(scene, Style { color: t.dim, ..term }, x0, y, &chip);
            self.lv_mark(scene, Rect::new(x0, y - lh * 0.8, inner.w - gutter - self.px(6.0), lh), matches!(focus, Some(Hit::FoldOver(_))));
            y += lh;
        } else {
            for i in 0..2 {
                self.fonts.draw(scene, Style { color: t.dim, ..term }, x0, y, &format!("   Compiling crate-{i} v0.1.0"));
                y += lh;
            }
        }
        self.fonts.draw(scene, Style { color: ansi(1), ..term }, x0, y, "error: see https://doc.rust-lang.org");
        let url_x = x0 + self.fonts.measure(term, "error: see ");
        let url_w = self.fonts.measure(term, "https://doc.rust-lang.org");
        if b.link_click != LinkClick::HintsOnly {
            scene.hline(url_x, y + self.px(2.0), url_w, self.px(1.0), ansi(1));
        }
        self.lv_mark(scene, Rect::new(url_x, y - lh * 0.8, url_w, lh), matches!(focus, Some(Hit::LinkClick(_))));
        y += lh;
        // The live line: predictions ghost after the caret.
        lamp(self, scene, y, true);
        let mut x = x0;
        x += self.fonts.draw(scene, Style { color: ansi(4), ..term }, x, y, "~/nus ❯ ");
        x += self.fonts.draw(scene, Style { color: cmd_color(2), ..term }, x, y, "git ");
        x += self.fonts.draw(scene, Style { color: cmd_color(3), ..term }, x, y, "st");
        scene.rect(Rect::new(x, y - px, self.px(1.5), px * 1.2), t.caret);
        if b.predict {
            self.fonts.draw(scene, Style { color: fade(ink, 0.35), ..term }, x + self.px(2.0), y, "atus --short");
        }
        self.lv_mark(scene, Rect::new(x - self.px(2.0), y - lh * 0.8, inner.right() - x - self.px(4.0), lh), matches!(focus, Some(Hit::Predict(_) | Hit::PromptLsp(_))));
        if b.copy_on_select && matches!(focus, Some(Hit::CopyOnSelect(_))) {
            scene.rect(Rect::new(x0, y - lh * 0.8 - lh * 3.0, self.px(60.0), lh), t.selection);
        }
        self.lv_mark(scene, Rect::new(inner.x, inner.y, gutter, inner.h), matches!(focus, Some(Hit::Blocks(_))));
        let words = match focus {
            Some(Hit::Journal(_) | Hit::JournalKeep(_)) => if b.journal { format!("Every finished command is remembered for {} days, encrypted, on this device only.", b.journal_keep) } else { "Commands you run are not remembered.".into() },
            _ => format!(
                "{} · {} · {}",
                if b.highlight { "commands colored as you type" } else { "plain command text" },
                if b.predict { "suggestions from history" } else { "no suggestions" },
                if b.truecolour == Truecolour::Snapped { "program colors match your theme" } else { "programs keep their colors" }
            ),
        };
        self.lv_caption(scene, body.x, win.bottom() + self.px(2.0), body.w, &words);
    }

    // ── Hatch ────────────────────────────────────────────────────────────

    fn live_hatch(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        use crate::settings::{HatchLook, HatchMonitor};
        let t = self.theme.clone();
        let ink = t.ink;
        let b = self.behavior.clone();
        let cap_h = self.px(40.0);
        // Two screens: the hatch opens on the one the setting picks.
        let gap = self.px(10.0);
        let sw = (body.w - gap) * 0.62;
        let main = Rect::new(body.x, body.y, sw, body.h - cap_h);
        let other = Rect::new(body.x + sw + gap, body.y + (body.h - cap_h) * 0.3, body.w - sw - gap, (body.h - cap_h) * 0.55);
        let pointer_on_other = true;
        let target = match b.hatch_monitor { HatchMonitor::Pointer => if pointer_on_other { other } else { main }, HatchMonitor::Foreground | HatchMonitor::Primary => main };
        for s in [main, other] {
            scene.rect(s, fade(ink, 0.07));
            scene.outline(s, self.px(1.0), fade(ink, 0.45));
            scene.rect(Rect::new(s.x, s.y, s.w, self.px(5.0)), fade(ink, 0.15));
        }
        // The nus window on the main screen, and the pointer on the other.
        let w = Rect::new(main.x + main.w * 0.08, main.y + main.h * 0.2, main.w * 0.6, main.h * 0.6);
        if !b.hatch_background || !matches!(focus, Some(Hit::HatchBackground(_))) {
            let inner = self.lv_window(scene, w);
            self.lv_lines(scene, Rect::new(inner.x + self.px(4.0), inner.y + self.px(4.0), inner.w - self.px(8.0), inner.h - self.px(8.0)), 4, fade(ink, 0.15));
        }
        self.fonts.draw_icon(scene, icons::CURSOR, self.px(10.0), other.x + other.w * 0.6, other.y + other.h * 0.6, ink);
        if b.hatch_dim {
            scene.rect(Rect::new(target.x, target.y + self.px(5.0), target.w, target.h - self.px(5.0)), fade(ink, 0.25));
        }
        let frac = b.hatch_size as f32 / 100.0;
        let area = Rect::new(target.x, target.y + self.px(5.0), target.w, target.h - self.px(5.0));
        let hatch = match b.hatch_look {
            HatchLook::Sheet => Rect::new(area.x + area.w * 0.12, area.y, area.w * 0.76, area.h * frac),
            HatchLook::Card => Rect::new(area.x + area.w * 0.15, area.y + area.h * (0.5 - frac * 0.5), area.w * 0.7, area.h * frac),
        };
        scene.rect(hatch, self.paper());
        scene.outline(hatch, self.px(1.5), ink);
        let term = Style { font: self.f.term, px: self.px(8.0), color: ink, tracking: 0.0 };
        self.lv_text(scene, term, hatch.x + self.px(4.0), hatch.y + self.px(11.0), "~ ❯ _", hatch.w - self.px(8.0));
        self.lv_mark(scene, hatch, matches!(focus, Some(Hit::HatchLook(_) | Hit::HatchSize(_) | Hit::HatchDim(_))));
        self.lv_mark(scene, target, matches!(focus, Some(Hit::HatchMonitor(_))));
        if b.hatch_status {
            let badge = Rect::new(main.x + main.w * 0.5 - self.px(16.0), main.y, self.px(32.0), self.px(7.0));
            scene.push(nus_render::Instance::rounded(badge, badge.h * 0.5, ink));
            let d = Rect::new(badge.x + self.px(3.0), badge.y + self.px(2.0), self.px(3.0), self.px(3.0));
            scene.push(nus_render::Instance::rounded(d, d.w, self.surface.signal));
            self.lv_mark(scene, badge, matches!(focus, Some(Hit::HatchStatus(_))));
        }
        // The hotkey, as keycaps.
        let key = b.hatch_hotkey.label();
        let st = Style { color: ink, ..self.lv_small() };
        let kw = self.fonts.measure(st, key) + self.px(10.0);
        let cap = Rect::new(body.right() - kw, body.y, kw, self.px(15.0));
        scene.rect(cap, self.paper());
        scene.outline(cap, self.px(1.0), ink);
        self.fonts.draw(scene, st, cap.x + self.px(5.0), cap.bottom() - self.px(4.0), key);
        self.lv_mark(scene, cap, matches!(focus, Some(Hit::HatchHotkey(_) | Hit::HatchRecord)));
        let words = format!(
            "{} opens a {} {}% tall on {}. {}",
            key,
            if b.hatch_look == HatchLook::Sheet { "sheet from the top" } else { "floating card" },
            b.hatch_size,
            match b.hatch_monitor { HatchMonitor::Pointer => "the screen with the pointer", HatchMonitor::Foreground => "nus's screen", HatchMonitor::Primary => "the main screen" },
            if b.hatch_autohide { "It hides when you click elsewhere." } else { "It stays until you hide it." }
        );
        self.lv_caption(scene, body.x, main.bottom() + self.px(2.0), body.w, &words);
    }

    // ── Sync ─────────────────────────────────────────────────────────────

    fn live_sync(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        let t = self.theme.clone();
        let ink = t.ink;
        let b = self.behavior.clone();
        let has_key = crate::syncui::key().is_some();
        let folder = !b.sync_folder.is_empty();
        let git = !b.sync_git.is_empty();
        let ready = has_key && (folder || git);
        // The path a change takes: this device → locked → destinations → other devices.
        let node_h = self.px(44.0);
        let y = body.y + self.px(6.0);
        let col = (body.w - self.px(24.0)) / 4.0;
        let node = |app: &mut App, scene: &mut Scene, i: usize, dy: f32, h: f32, title: &str, sub: &str, lit: bool, icon: (&'static str, &'static str), marked: bool| -> Rect {
            let r = Rect::new(body.x + i as f32 * (col + app.px(8.0)), y + dy, col, h);
            if lit { scene.rect(r, fade(app.surface.signal, 0.12)); }
            scene.outline(r, app.px(if lit { 1.5 } else { 1.0 }), if lit { ink } else { fade(ink, 0.35) });
            app.fonts.draw_icon(scene, icon, app.px(11.0), r.x + app.px(5.0), r.y + app.px(5.0), if lit { ink } else { t.dim });
            let st = Style { color: if lit { ink } else { t.dim }, ..app.lv_tiny() };
            app.lv_text(scene, st, r.x + app.px(5.0), r.y + app.px(28.0), title, r.w - app.px(10.0));
            let st = Style { color: t.dim, px: app.px(8.5), ..app.ui() };
            app.lv_text(scene, st, r.x + app.px(5.0), r.y + app.px(39.0), sub, r.w - app.px(10.0));
            app.lv_mark(scene, r, marked);
            r
        };
        let me = node(self, scene, 0, node_h * 0.5, node_h, "THIS DEVICE", &self.me.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| "you".into()), true, icons::DESKTOP, false);
        let lock = node(self, scene, 1, node_h * 0.5, node_h, if has_key { "ENCRYPTED" } else { "NO KEY YET" }, if has_key { "with your key" } else { "make or join one" }, has_key, icons::SHIELD, matches!(focus, Some(Hit::SyncKey | Hit::SyncForget | Hit::SyncEdit(2))));
        let f = node(self, scene, 2, 0.0, node_h, "FOLDER", if folder { &b.sync_folder } else { "not chosen" }, folder, icons::FOLDER, matches!(focus, Some(Hit::SyncEdit(0))));
        let g = node(self, scene, 2, node_h + self.px(6.0), node_h, "GIT", if git { &b.sync_git } else { "not chosen" }, git, icons::GITHUB, matches!(focus, Some(Hit::SyncEdit(1) | Hit::MeWalk(_) | Hit::ForgeForget)));
        let other = node(self, scene, 3, node_h * 0.5, node_h, "OTHER DEVICES", if ready { "same key" } else { "waiting" }, ready, icons::DESKTOP, false);
        let arrow = |app: &mut App, scene: &mut Scene, a: Rect, b2: Rect, on: bool| {
            let (x0, x1) = (a.right(), b2.x);
            let yy = b2.y + b2.h * 0.5;
            let ya = a.y + a.h * 0.5;
            let c = if on { ink } else { fade(ink, 0.25) };
            scene.hline(x0, ya, (x1 - x0) * 0.5, app.px(1.0), c);
            scene.vline(x0 + (x1 - x0) * 0.5, ya.min(yy), (ya - yy).abs().max(app.px(1.0)), app.px(1.0), c);
            scene.hline(x0 + (x1 - x0) * 0.5, yy, (x1 - x0) * 0.5, app.px(1.0), c);
        };
        arrow(self, scene, me, lock, true);
        arrow(self, scene, lock, f, has_key && folder);
        arrow(self, scene, lock, g, has_key && git);
        arrow(self, scene, f, other, ready && folder);
        arrow(self, scene, g, other, ready && git);
        // What travels, and what never does.
        let mut yy = g.bottom() + self.px(16.0);
        let head = Style { color: ink, ..self.lv_tiny() };
        self.fonts.draw(scene, head, body.x, yy, "WHAT SYNCS");
        self.fonts.draw(scene, head, body.x + body.w * 0.55, yy, "NEVER LEAVES THIS DEVICE");
        yy += self.px(4.0);
        let items = [("Settings & look", true), ("Rules", true), ("Saved layouts", true), ("Folders & ports", true), ("Assistant memory", true), ("Open tabs", b.sync_session)];
        let never = ["Cookies & sign-ins", "Caches", "Downloads", "Shell & command history"];
        let st = self.lv_small();
        for (i, (name, on)) in items.iter().enumerate() {
            let ry = yy + (i + 1) as f32 * self.px(13.0);
            let c = if *on { ink } else { t.dim };
            if *on { self.fonts.draw_icon(scene, icons::CHECK, self.px(9.0), body.x, ry - self.px(8.0), ink); }
            self.fonts.draw(scene, Style { color: c, ..st }, body.x + self.px(13.0), ry, name);
            if *name == "Open tabs" {
                let w = self.fonts.measure(st, name) + self.px(13.0);
                self.lv_mark(scene, Rect::new(body.x, ry - self.px(10.0), w, self.px(13.0)), matches!(focus, Some(Hit::SyncSession(_))));
            }
        }
        for (i, name) in never.iter().enumerate() {
            let ry = yy + (i + 1) as f32 * self.px(13.0);
            self.fonts.draw_icon(scene, icons::CLOSE, self.px(9.0), body.x + body.w * 0.55, ry - self.px(8.0), t.dim);
            self.fonts.draw(scene, Style { color: t.dim, ..st }, body.x + body.w * 0.55 + self.px(13.0), ry, name);
        }
        yy += self.px(13.0) * 6.0 + self.px(14.0);
        let when = format!(
            "{}{}",
            if b.sync_every_min == 0 { "Syncs only when you press Sync now".to_string() } else { format!("Syncs every {} minutes", b.sync_every_min) },
            if b.sync_at_quit { ", and once more when you quit." } else { "." }
        );
        let st = Style { color: ink, ..self.lv_small() };
        let w = body.w;
        let when_r = Rect::new(body.x, yy - self.px(10.0), w, self.px(13.0));
        self.lv_text(scene, st, body.x, yy, &when, w);
        self.lv_mark(scene, when_r, matches!(focus, Some(Hit::SyncEvery(_) | Hit::SyncAtQuit(_) | Hit::SyncNow)));
        yy += self.px(6.0);
        let status = if ready { self.sync_status() } else if !has_key { "Step 1: make a key here, or join with the key from your other device.".into() } else { "Step 2: choose a folder or a Git repository to sync through.".into() };
        yy += self.lv_caption(scene, body.x, yy, body.w, &status);
        if b.phone {
            let st = Style { color: ink, ..self.lv_small() };
            self.lv_text(scene, st, body.x, yy + self.px(14.0), "Phone page on: this window is readable from your phone on this network.", body.w);
        }
        if matches!(focus, Some(Hit::Phone(_) | Hit::CopyPhoneUrl)) {
            self.lv_mark(scene, Rect::new(body.x, yy + self.px(3.0), body.w, self.px(14.0)), true);
        }
    }

    // ── Prompt ───────────────────────────────────────────────────────────

    fn live_prompt(&mut self, scene: &mut Scene, body: Rect, focus: Option<Hit>) {
        let t = self.theme.clone();
        let ink = t.ink;
        let c = self.behavior.prompt.clone();
        // Typing shows the search-time mix; anything else, the home mix.
        let typing = matches!(focus, Some(Hit::Workspace(W::Source(_, 1) | W::Limit(false, _) | W::Engine(_) | W::Route(_) | W::EditSearchUrl)));
        let typed = if typing { "git" } else { "" };
        let win = Rect::new(body.x, body.y, body.w, body.h - self.px(30.0));
        let inner = self.lv_window(scene, win);
        let pad = if c.wide { self.px(10.0) } else { inner.w * 0.14 };
        let line_y = if c.top { inner.y + self.px(26.0) } else { inner.y + inner.h * 0.3 };
        let x = inner.x + pad;
        let w = inner.w - pad * 2.0;
        let ui = Style { px: self.px(11.0), ..self.ui() };
        let term = Style { font: self.f.term, px: self.px(11.0), color: ink, tracking: 0.0 };
        if typed.is_empty() {
            self.fonts.draw(scene, Style { color: t.dim, ..term }, x, line_y, "› a URL, a command or a folder");
        } else {
            let tw = self.fonts.draw(scene, term, x, line_y, &format!("› {typed}"));
            scene.rect(Rect::new(x + tw + self.px(1.0), line_y - self.px(10.0), self.px(1.5), self.px(12.0)), t.caret);
        }
        scene.hline(x, line_y + self.px(6.0), w, self.px(1.0), ink);
        let rows = self.prompt_rows(typed);
        let rh = if c.compact { self.px(15.0) } else { self.px(20.0) };
        let mut y = line_y + self.px(12.0);
        let foot = if c.hints { self.px(22.0) } else { 0.0 };
        if rows.is_empty() {
            self.fonts.draw(scene, Style { color: t.dim, ..ui }, x, y + rh * 0.7, "Nothing listed · just the line");
        }
        for (i, row) in rows.iter().enumerate() {
            if y + rh > inner.bottom() - foot {
                break;
            }
            let r = Rect::new(x, y, w, rh);
            if i == 0 && typing { scene.rect(r, t.tint); }
            let num = if row.num.chars().count() > 3 { row.num.chars().take(1).collect::<String>().to_uppercase() } else { row.num.clone() };
            self.lv_text(scene, Style { color: t.dim, ..ui }, r.x + self.px(4.0), r.y + rh * 0.7, &num, self.px(16.0));
            self.lv_text(scene, Style { color: ink, ..ui }, r.x + self.px(22.0), r.y + rh * 0.7, &row.text, r.w - self.px(26.0));
            y += rh;
        }
        self.lv_mark(scene, Rect::new(x, line_y + self.px(10.0), w, (y - line_y - self.px(10.0)).max(rh)), matches!(focus, Some(Hit::Workspace(W::Source(..) | W::Count(..) | W::Move(..) | W::Limit(..) | W::PromptPreset(_)))));
        if c.hints {
            let marks = ["› shell", "? web", "@ assistant", "↓ rows"];
            let mut mx = x;
            let fy = inner.bottom() - self.px(8.0);
            let lit = match c.route { crate::prompt::Route::Web => 1, crate::prompt::Route::Assistant => 2, _ => 0 };
            for (i, m) in marks.iter().enumerate() {
                let st = Style { color: if i == lit { ink } else { t.dim }, ..self.lv_tiny() };
                mx += self.fonts.draw(scene, st, mx, fy, m) + self.px(12.0);
            }
            self.lv_mark(scene, Rect::new(x, fy - self.px(10.0), mx - x, self.px(13.0)), matches!(focus, Some(Hit::Workspace(W::Layout(3)))));
        }
        self.lv_mark(scene, Rect::new(inner.x, line_y - self.px(14.0), inner.w, self.px(22.0)), matches!(focus, Some(Hit::Workspace(W::Layout(0 | 1 | 2)))));
        let words = if typing {
            format!("While typing “{typed}”: Enter goes to {} · web searches ask {}.", c.route.name().to_lowercase(), c.engine.name())
        } else {
            format!("Before typing: {} suggestions at most, in the order of the table.", c.home_limit)
        };
        self.lv_caption(scene, body.x, win.bottom() + self.px(2.0), body.w, &words);
    }
}
