//! The hatch at rest, as lamps (design/HatchLamps.dc.html, picked
//! 2026-10-09). Where there's no camera housing (Windows, Linux, a Mac on
//! another display) it is the sheet rolled up: a tab hanging from the top
//! edge with one lamp per piece of work. On a notched Mac the island keeps
//! its shape and its wings carry the lamps: what is going on the left, what
//! wants you or ended unseen on the right.
//!
//! A square breathes while it runs and fills from its foot with progress; a
//! diamond needs you (hollow: it rang); a circle finished and a hatched
//! square failed, both until you look. Shape carries the state, so a theme
//! whose signal is its ink (Blueprint) reads the same. Words only in a
//! tooltip and in a completion notice, and the only words are the session's
//! own title.

use super::*;
use crate::app::{fade, Caps};
use crate::hatch_native::Notch;
use crate::hatch_work::Status;
use nus_render::text::icons;
use nus_render::{Color, FontSystem, Instance};

/// Six lamps on the tab, then a ⋯: the rest are in Work.
pub(crate) const MAX_LAMPS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lamp { Running, Progress(u8), Input, Rang, Finished, Failed }

impl Lamp {
    /// Going, rather than wanting you or ended: the island's left wing.
    pub(crate) fn going(self) -> bool { matches!(self, Lamp::Running | Lamp::Progress(_)) }
}

pub(crate) fn lamp(item: &Item) -> Option<Lamp> {
    Some(match item.status {
        Status::Running => item.progress.map_or(Lamp::Running, Lamp::Progress),
        Status::NeedsInput => Lamp::Input,
        Status::Attention => Lamp::Rang,
        Status::Finished if item.unread => Lamp::Finished,
        Status::Failed if item.unread => Lamp::Failed,
        _ => return None,
    })
}

/// What the closed hatch shows, in the order the sessions were opened, so a
/// lamp keeps its place and new work joins on the right. (Work sorts by
/// urgency; up here that would make the lamps hop.)
pub(crate) fn lamps(work: &[Item]) -> Vec<(&Item, Lamp)> {
    let mut v: Vec<_> = work.iter().filter_map(|i| lamp(i).map(|l| (i, l))).collect();
    v.sort_by_key(|(i, _)| (i.target.window, i.target.tab, i.target.right));
    v
}

/// A lamp's tooltip: the session's title and what it's doing.
pub(crate) fn words(item: &Item) -> String {
    let state = match (item.status, item.progress, item.exit) {
        (Status::Running, Some(p), _) => format!("running {p} %"),
        (Status::Running, None, _) => "running".into(),
        (Status::NeedsInput, ..) => "needs input".into(),
        (Status::Attention, ..) => "rang the bell".into(),
        (Status::Finished, ..) => "finished".into(),
        (Status::Failed, _, Some(code)) => format!("failed · exit {code}"),
        (Status::Failed, ..) => "failed".into(),
        (Status::Idle, ..) => "shell".into(),
    };
    format!("{} · {state}", item.title)
}

/// How many lamps fit in `room` px, keeping one place for the ⋯ when they
/// don't all fit.
pub(crate) fn fit(count: usize, room: f32, size: f32, gap: f32, dots: f32) -> (usize, bool) {
    let all = if room < size { 0 } else { ((room + gap) / (size + gap)).floor() as usize };
    if count <= all { return (count, false); }
    let with_dots = if room < dots { 0 } else { ((room - dots + gap) / (size + gap)).floor() as usize };
    (with_dots.min(count), true)
}

/// A diamond inside `r`, drawn `inset` in from its points.
fn diamond(r: Rect, inset: f32) -> [[f32; 2]; 4] {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let h = r.w * 0.61 - inset;
    [[cx, cy - h], [cx + h, cy], [cx, cy + h], [cx - h, cy]]
}

/// The colours a lamp is drawn in: the tab's are the theme's; the island's
/// are its own, since it is black like the camera housing it joins.
#[derive(Clone, Copy)]
struct Marks { paper: Color, ink: Color, signal: Color, green: Color, red: Color }

/// Physical px for this display.
#[derive(Clone, Copy)]
struct Px(f32);
impl Px {
    fn of(self, v: f32) -> f32 { (v * self.0).round() }
    fn thin(self, v: f32) -> f32 { (v * self.0).round().max(1.0) }
}

fn draw_lamp(s: &mut Scene, r: Rect, kind: Lamp, m: Marks, breath: f32, px: Px) {
    let edge = px.thin(1.5);
    match kind {
        Lamp::Running => s.rect(r, fade(m.signal, breath)),
        Lamp::Progress(p) => {
            s.outline(r, edge, m.ink);
            let (iw, ih) = (r.w - 2.0 * edge, r.h - 2.0 * edge);
            let h = (ih * f32::from(p.min(100)) / 100.0).round();
            if h > 0.0 { s.rect(Rect::new(r.x + edge, r.y + edge + ih - h, iw, h), m.signal); }
        }
        Lamp::Input => s.push(Instance::quad(diamond(r, 0.0), fade(m.signal, breath))),
        Lamp::Rang => {
            s.push(Instance::quad(diamond(r, 0.0), m.signal));
            s.push(Instance::quad(diamond(r, edge * 1.42), m.paper));
        }
        Lamp::Finished => s.push(Instance::rounded(r, r.w / 2.0, m.green)),
        Lamp::Failed => {
            // Hatched, as a failed download is.
            let (period, stripe) = (px.thin(3.0), px.thin(1.2));
            s.layer(Some(r));
            for n in -3..=3 {
                let x = r.x + n as f32 * period;
                s.push(Instance::quad([[x, r.bottom()], [x + stripe, r.bottom()], [x + stripe + r.h, r.y], [x + r.h, r.y]], m.red));
            }
            s.layer(None);
            s.outline(r, px.thin(1.0), m.red);
        }
    }
}

/// The hovered lamp's outline, and a notice's one bloom (`q` runs 0 → 1).
fn draw_marks_over(s: &mut Scene, r: Rect, kind: Lamp, m: Marks, hovered: bool, bloom: Option<f32>, px: Px) {
    if hovered {
        s.outline(Rect::new(r.x - px.of(3.0), r.y - px.of(3.0), r.w + px.of(6.0), r.h + px.of(6.0)), px.thin(1.0), m.ink);
    }
    if let Some(q) = bloom {
        let d = r.w * (1.0 + 1.6 * q);
        let ring = Rect::new(r.x + (r.w - d) / 2.0, r.y + (r.h - d) / 2.0, d, d);
        let c = if kind == Lamp::Failed { m.red } else { m.green };
        s.push(Instance::stroke(ring, d / 2.0, px.thin(1.0), fade(c, 0.9 * (1.0 - q)), None, 0.0));
    }
}

fn draw_dots(s: &mut Scene, x: f32, cy: f32, color: Color, px: Px) {
    let dot = px.thin(2.0);
    let y = (cy - dot / 2.0).round();
    for n in 0..3 { s.rect(Rect::new(x + n as f32 * dot * 2.0, y, dot, dot), color); }
}

/// The notice's line: a check or a cross and the session's own title,
/// centred in `row`.
fn draw_notice(fonts: &mut FontSystem, s: &mut Scene, row: Rect, item: &Item, line: &str, style: Style, m: Marks, px: Px) {
    let icon = px.of(11.0);
    let w = icon + px.of(5.0) + fonts.measure(style, line);
    let x = (row.x + (row.w - w) / 2.0).round();
    let (glyph, c) = if item.status == Status::Failed { (icons::CLOSE, m.red) } else { (icons::CHECK, m.green) };
    fonts.draw_icon(s, glyph, icon, x, (row.y + (row.h - icon) / 2.0).round(), c);
    fonts.draw(s, style, x + icon + px.of(5.0), (row.y + row.h / 2.0 + style.px * 0.36).round(), line);
}

/// A tooltip in the house's: paper, a hairline, a hard shadow.
fn draw_tip(fonts: &mut FontSystem, s: &mut Scene, r: Rect, text: &str, label: Style, paper: Color, ink: Color, shadow: f32, px: Px) {
    s.layer(None);
    s.rect(Rect::new(r.x + shadow, r.y + shadow, r.w, r.h), SHADE);
    s.rect(r, paper);
    s.outline(r, px.thin(m::HAIRLINE), ink);
    fonts.draw(s, Style { color: ink, ..label }, r.x + px.of(8.0), r.y + px.of(4.0) + label.px, text);
}

const SHADE: Color = [0.0, 0.0, 0.0, 0.28];

/// What the island and the tab share for one frame.
struct Frame {
    all: Vec<(Item, Lamp)>,
    notice: Option<(Item, Instant)>,
    breath: f32,
    step: Option<f32>,
    bloom: Option<(WorkTarget, f32)>,
}

impl App {
    fn hatch_badge_hide(&mut self) {
        if let Some(badge) = self.hatch_state.badge.as_mut().filter(|b| b.visible) {
            badge.window.set_visible(false);
            badge.visible = false;
        }
        self.hatch_state.badge_live = false;
        self.hatch_state.badge_hits.clear();
        self.hatch_state.badge_tab = None;
    }

    /// The lamps, the notice, the breath (ten frames a second, as the
    /// sidebar's working square) and a notice's bloom. Still under reduced
    /// motion.
    fn hatch_badge_now(&self, shown: impl Fn(&[(Item, Lamp)]) -> usize) -> Frame {
        let reduced = self.motion.reduced();
        let all: Vec<(Item, Lamp)> = lamps(&self.hatch_state.work).into_iter().map(|(i, l)| (i.clone(), l)).collect();
        let n = shown(&all);
        let notice = self.hatch_state.completion.clone();
        let breathing = !reduced && all.iter().take(n).any(|(_, l)| matches!(l, Lamp::Running | Lamp::Input));
        let step = (crate::clock::since(self.started).as_secs_f32() * 10.0).floor();
        let breath = if breathing { 0.45 + 0.55 * ((step / 10.0 * 2.2).sin() * 0.5 + 0.5) } else { 1.0 };
        let bloom = notice.as_ref().filter(|_| !reduced).and_then(|(item, at)| {
            let q = crate::clock::since(at).as_secs_f32() / 0.5;
            (q < 1.0 && all.iter().any(|(i, _)| i.target == item.target)).then_some((item.target, q))
        });
        Frame { all, notice, breath, step: breathing.then_some(step), bloom }
    }

    /// The tooltip's words for what the pointer is over, fitted.
    fn hatch_badge_tip(&self, hovered: Option<&Item>, on_shape: bool, label: Style, px: Px) -> Option<String> {
        let words = match (hovered, on_shape) {
            (Some(item), _) => words(item),
            (None, true) => crate::hatch_work::summary(&self.hatch_state.work),
            _ => return None,
        };
        Some(self.fit(label, words.caps(), px.of(420.0)).into_owned())
    }

    fn hatch_badge_label(&self, px: Px) -> Style {
        Style { font: self.f.ui, px: px.of(m::LABEL_PX) * self.behavior.typography.ui_scale, color: self.theme.ink, tracking: px.of(m::LABEL_PX) * m::LABEL_TRACKING }
    }

    /// Moves and sizes the window; false when this frame is the last one.
    fn hatch_badge_place(&mut self, pos: (i32, i32), dims: (u32, u32), key: String, island: bool) -> bool {
        let Some(badge) = &mut self.hatch_state.badge else { return false };
        crate::hatch_native::island_level(&badge.window, island);
        if badge.position != Some(pos) {
            badge.window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.0, pos.1));
            badge.position = Some(pos);
        }
        if badge.text == key && badge.target.size == dims && badge.visible { return false; }
        badge.text = key;
        if badge.target.size != dims {
            let _ = badge.window.request_inner_size(winit::dpi::PhysicalSize::new(dims.0, dims.1));
            badge.target.resize(&self.gpu.device, dims.0, dims.1);
            // Resize can move the top edge on AppKit: pin it again to screen.frame.
            badge.window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.0, pos.1));
        }
        true
    }

    fn hatch_badge_show(&mut self) {
        let Some(badge) = &mut self.hatch_state.badge else { return };
        badge.scene.finish();
        crate::app::upload_glyphs(&mut self.fonts, &self.gpu, &mut self.dirty);
        self.gpu.render(&mut badge.target, &badge.scene, [0.0; 4]);
        if !badge.visible {
            crate::hatch_native::show_passive(&badge.window);
            badge.visible = true;
        }
    }

    /// The tab, drawn into the badge window (or hidden). `top` is where it
    /// hangs from: the screen's top edge, or under a menu bar.
    pub(super) fn hatch_badge_lamps(&mut self, show: bool, mon: (i32, i32, u32, u32, f32), top: i32) {
        let (mx, my, mw, _, scale) = mon;
        let Some(translucent) = self.hatch_state.badge.as_ref().map(|b| b.target.translucent()) else { return };
        if !show { return self.hatch_badge_hide(); }
        let px = Px(scale);
        let marks = Marks {
            paper: { let p = self.theme.paper; [p[0], p[1], p[2], 1.0] },
            ink: self.theme.ink,
            signal: self.surface.signal,
            green: crate::theme_edit::from_rgb(self.theme.ansi[2]),
            red: crate::theme_edit::from_rgb(self.theme.ansi[1]),
        };
        let now = self.hatch_badge_now(|all| all.len().min(MAX_LAMPS));
        let shown = now.all.len().min(MAX_LAMPS);
        let more = now.all.len() > MAX_LAMPS;

        // The tab: lamps in a row, the notice's line under them for six
        // seconds, the lip and the edges round three sides (none on top).
        let (edge, lip, pad, size, gap, dot) = (px.thin(1.5), px.thin(2.0), px.of(7.0), px.of(7.0), px.of(6.0), px.thin(2.0));
        // A compositor without alpha gets the tab alone: no shadow, no tooltip.
        let shadow = if translucent { px.of(3.0) } else { 0.0 };
        let ready = now.all.is_empty();
        let row_h = if ready { px.of(11.0) } else { px.of(16.0) };
        let mut row_w = if ready { px.of(10.0) } else { shown as f32 * size + shown.saturating_sub(1) as f32 * gap };
        if more { row_w += gap + dot * 5.0; }
        let title = Style { font: self.f.ui, px: px.of(11.0), color: marks.ink, tracking: 0.0 };
        let line = now.notice.as_ref().map(|(item, _)| self.fit(title, item.title.as_str(), px.of(240.0)).into_owned());
        let line_w = line.as_ref().map_or(0.0, |t| px.of(16.0) + self.fonts.measure(title, t));
        let line_h = if line.is_some() { px.of(17.0) } else { 0.0 };
        let inner = row_w.max(line_w);
        let tab_w = inner + 2.0 * (pad + edge);
        let tab_h = row_h + line_h + lip + edge;

        // Where it all sits on screen. The tab is centred on the monitor
        // whether or not a tooltip widens the window around it.
        let tab_sx = mx as f32 + ((mw as f32 - tab_w) / 2.0).round();
        let tab_sy = (my + top) as f32;
        let tab_screen = Rect::new(tab_sx, tab_sy, tab_w, tab_h);
        let lx0 = tab_sx + edge + pad + ((inner - row_w) / 2.0).round();
        let ly = tab_sy + ((row_h - size) / 2.0).round();
        let rects: Vec<Rect> = (0..shown).map(|i| Rect::new(lx0 + i as f32 * (size + gap), ly, size, size)).collect();
        let hits: Vec<(Rect, WorkTarget)> = rects.iter().zip(&now.all)
            .map(|(r, (item, _))| (Rect::new(r.x - gap / 2.0, tab_sy, size + gap, row_h), item.target)).collect();
        let hover = self.hatch_state.badge_hover;
        let hovered = hover.and_then(|(x, y)| hits.iter().position(|(r, _)| r.contains(x, y)));
        let on_tab = hover.is_some_and(|(x, y)| tab_screen.contains(x, y));

        let label = self.hatch_badge_label(px);
        let tip_text = if translucent { self.hatch_badge_tip(hovered.map(|i| &now.all[i].0), on_tab, label, px) } else { None };
        let tip_size = tip_text.as_ref().map(|t| (self.fonts.measure(label, t) + px.of(16.0), (label.px * 1.4).max(px.of(16.0)) + px.of(8.0)));
        let tip_gap = px.of(6.0);
        let win_w = (tab_w.max(tip_size.map_or(0.0, |s| s.0)) + 2.0 * shadow).ceil();
        let win_h = (tab_h + shadow + tip_size.map_or(0.0, |s| tip_gap + s.1)).ceil();
        let pos = ((tab_sx + tab_w / 2.0 - win_w / 2.0).round() as i32, my + top);
        let (ox, oy) = (pos.0 as f32, pos.1 as f32);
        let local = |r: Rect| Rect::new(r.x - ox, r.y - oy, r.w, r.h);
        let tip_rect = tip_size.map(|(w, h)| {
            let anchor = hovered.map_or(tab_screen, |i| rects[i]);
            let x = (anchor.x + anchor.w / 2.0 - ox - w / 2.0).round().clamp(0.0, (win_w - shadow - w).max(0.0));
            Rect::new(x, tab_h + tip_gap, w, h)
        });

        let kinds: Vec<Lamp> = now.all.iter().map(|(_, l)| *l).collect();
        let bloom_at = now.bloom.and_then(|(t, q)| now.all.iter().take(shown).position(|(i, _)| i.target == t).map(|i| (i, q)));
        let key = format!("tab:{win_w}x{win_h}@{pos:?}|{kinds:?}|{hovered:?}|{tip_text:?}|{:?}|{:?}|{line:?}|{:?}{:?}{:?}{:?}{:?}|{translucent}",
            now.step, bloom_at.map(|(i, q)| (i, (q * 15.0) as u32)), marks.paper, marks.ink, marks.signal, marks.green, marks.red);
        self.hatch_state.badge_hits = hits;
        self.hatch_state.badge_tab = Some(tab_screen);
        self.hatch_state.badge_live = now.step.is_some() || bloom_at.is_some();
        if !self.hatch_badge_place(pos, (win_w.max(1.0) as u32, win_h.max(1.0) as u32), key, false) { return; }

        let Some(badge) = &mut self.hatch_state.badge else { return };
        let s = &mut badge.scene;
        s.clear();
        s.layer(None);
        let tab = local(tab_screen);
        if shadow > 0.0 { s.rect(Rect::new(tab.x + shadow, shadow, tab.w, tab.h), SHADE); }
        s.rect(tab, marks.paper);
        s.vline(tab.x, 0.0, tab.h, edge, marks.ink);
        s.vline(tab.right() - edge, 0.0, tab.h, edge, marks.ink);
        s.hline(tab.x, tab.bottom() - edge, tab.w, edge, marks.ink);
        s.hline(tab.x + edge, tab.bottom() - edge - lip, tab.w - 2.0 * edge, lip, marks.signal);
        if ready {
            let g = px.of(10.0);
            self.fonts.draw_icon(s, icons::CARET_DOWN, g, (tab.x + (tab.w - g) / 2.0).round(), ((row_h - g) / 2.0).round(), fade(marks.ink, 0.8));
        }
        for (i, r) in rects.iter().enumerate() {
            let r = local(*r);
            draw_lamp(s, r, kinds[i], marks, now.breath, px);
            draw_marks_over(s, r, kinds[i], marks, hovered == Some(i), bloom_at.filter(|(b, _)| *b == i).map(|(_, q)| q), px);
        }
        if more { draw_dots(s, local(rects[shown - 1]).right() + gap, ly - oy + size / 2.0, marks.ink, px); }
        if let (Some(line), Some((item, _))) = (&line, &now.notice) {
            s.hline(tab.x + edge + pad, row_h, inner, px.thin(1.0), fade(marks.ink, 0.3));
            draw_notice(&mut self.fonts, s, Rect::new(tab.x + edge + pad, row_h, inner, line_h), item, line, title, marks, px);
        }
        if let (Some(r), Some(text)) = (tip_rect, &tip_text) {
            draw_tip(&mut self.fonts, s, r, text, label, marks.paper, marks.ink, shadow, px);
        }
        self.hatch_badge_show();
    }

    /// The island on a notched Mac: black, joined to the camera housing at
    /// the top of screen.frame, its wings outside the housing carrying the
    /// lamps. While the hatch is open below it, it is only the joint.
    pub(super) fn hatch_badge_island(&mut self, show: bool, mon: (i32, i32, u32, u32, f32), n: Notch, attached: bool) {
        let (mx, my, mw, _, scale) = mon;
        let Some(translucent) = self.hatch_state.badge.as_ref().map(|b| b.target.translucent()) else { return };
        if !show { return self.hatch_badge_hide(); }
        let px = Px(scale);
        let black = [0.0, 0.0, 0.0, 1.0];
        let white = [0.95, 0.95, 0.95, 1.0];
        let marks = Marks { paper: black, ink: white, signal: white, green: nus_render::theme::hex(0x7ac77f), red: nus_render::theme::hex(0xff6f63) };
        let island_w = (n.width + px.of(if attached { 12.0 } else { 116.0 }) as u32).min(mw);
        let wing = island_w.saturating_sub(n.width) as f32 / 2.0;
        let (size, gap, pad, dots) = (px.of(7.0), px.of(6.0), px.of(9.0), px.thin(2.0) * 5.0);
        let room = (wing - 2.0 * pad).max(0.0);
        let now = self.hatch_badge_now(|all| {
            if attached { return 0; }
            let going = all.iter().filter(|(_, l)| l.going()).count();
            fit(going, room, size, gap, dots).0 + fit(all.len() - going, room, size, gap, dots).0
        });
        let all: Vec<(Item, Lamp)> = if attached { Vec::new() } else { now.all.clone() };
        let (left, right): (Vec<usize>, Vec<usize>) = (0..all.len()).partition(|&i| all[i].1.going());
        let (left_n, left_more) = fit(left.len(), room, size, gap, dots);
        let (right_n, right_more) = fit(right.len(), room, size, gap, dots);

        let title = Style { font: self.f.ui, px: px.of(12.0), color: white, tracking: 0.0 };
        let line = now.notice.as_ref().filter(|_| !attached).map(|(item, _)| self.fit(title, item.title.as_str(), island_w as f32 - px.of(48.0)).into_owned());
        let island_h = n.height + px.of(if attached { 2.0 } else if line.is_some() { 30.0 } else { 8.0 }) as u32;
        let island_screen = Rect::new((mx + n.left + (n.width as i32 - island_w as i32) / 2) as f32, my as f32, island_w as f32, island_h as f32);

        // A wing's lamps, centred in it, at the menu bar's middle.
        let cy = (my as f32 + n.height as f32 / 2.0).round();
        // Each wing's lamps, and where its ⋯ goes when they don't all fit.
        let row = |idx: &[usize], count: usize, more: bool, x0: f32| -> (Vec<(usize, Rect)>, Option<f32>) {
            let w = count as f32 * size + count.saturating_sub(1) as f32 * gap + if more { (if count > 0 { gap } else { 0.0 }) + dots } else { 0.0 };
            let start = (x0 + (wing - w) / 2.0).round();
            let rects = idx.iter().take(count).enumerate().map(|(k, &i)| (i, Rect::new(start + k as f32 * (size + gap), (cy - size / 2.0).round(), size, size))).collect();
            (rects, more.then(|| start + count as f32 * (size + gap)))
        };
        let (mut placed, left_dots) = row(&left, left_n, left_more, island_screen.x);
        let (on_right, right_dots) = row(&right, right_n, right_more, island_screen.right() - wing);
        placed.extend(on_right);
        let hits: Vec<(Rect, WorkTarget)> = placed.iter().map(|(i, r)| (Rect::new(r.x - gap / 2.0, my as f32, size + gap, n.height as f32), all[*i].0.target)).collect();
        let hover = self.hatch_state.badge_hover;
        let hovered = hover.and_then(|(x, y)| hits.iter().position(|(r, _)| r.contains(x, y))).map(|h| placed[h].0);
        let on_island = !attached && hover.is_some_and(|(x, y)| island_screen.contains(x, y));

        let label = self.hatch_badge_label(px);
        let tip_text = if translucent && !attached { self.hatch_badge_tip(hovered.map(|i| &all[i].0), on_island, label, px) } else { None };
        let shadow = px.of(3.0);
        let tip_size = tip_text.as_ref().map(|t| (self.fonts.measure(label, t) + px.of(16.0), (label.px * 1.4).max(px.of(16.0)) + px.of(8.0)));
        let tip_gap = px.of(6.0);
        // The island stays on the housing to the pixel; a tooltip widens the
        // window round it evenly.
        let win_w = island_w.max(tip_size.map_or(0, |s| (s.0 + 2.0 * shadow).ceil() as u32));
        let win_h = island_h + tip_size.map_or(0, |s| (tip_gap + s.1 + shadow).ceil() as u32);
        let pos = (island_screen.x as i32 - ((win_w - island_w) / 2) as i32, my);
        let (ox, oy) = (pos.0 as f32, pos.1 as f32);
        let local = |r: Rect| Rect::new(r.x - ox, r.y - oy, r.w, r.h);
        let tip_rect = tip_size.map(|(w, h)| {
            let anchor = hovered.and_then(|i| placed.iter().find(|(j, _)| *j == i)).map_or(island_screen, |(_, r)| *r);
            let x = (anchor.x + anchor.w / 2.0 - ox - w / 2.0).round().clamp(0.0, (win_w as f32 - shadow - w).max(0.0));
            Rect::new(x, island_h as f32 + tip_gap, w, h)
        });

        let kinds: Vec<(usize, Lamp)> = placed.iter().map(|(i, _)| (*i, all[*i].1)).collect();
        let bloom_at = now.bloom.and_then(|(t, q)| placed.iter().find(|(i, _)| all[*i].0.target == t).map(|(i, _)| (*i, q)));
        let key = format!("island:{win_w}x{win_h}@{pos:?}|{attached}|{kinds:?}|{left_more}{right_more}|{hovered:?}|{tip_text:?}|{:?}|{:?}|{line:?}|{translucent}",
            now.step, bloom_at.map(|(i, q)| (i, (q * 15.0) as u32)));
        self.hatch_state.badge_hits = hits;
        self.hatch_state.badge_tab = (!attached).then_some(island_screen);
        self.hatch_state.badge_live = !attached && (now.step.is_some() || bloom_at.is_some());
        if !self.hatch_badge_place(pos, (win_w.max(1), win_h.max(1)), key, true) { return; }

        let Some(badge) = &mut self.hatch_state.badge else { return };
        let s = &mut badge.scene;
        s.clear();
        s.layer(None);
        let island = local(island_screen);
        s.push(Instance::rounded(Rect::new(island.x, -px.of(14.0), island.w, island.h + px.of(14.0)), px.of(14.0), black));
        if !attached {
            for (i, r) in &placed {
                let r = local(*r);
                draw_lamp(s, r, all[*i].1, marks, now.breath, px);
                draw_marks_over(s, r, all[*i].1, marks, hovered == Some(*i), bloom_at.filter(|(b, _)| b == i).map(|(_, q)| q), px);
            }
            for x in [left_dots, right_dots].into_iter().flatten() { draw_dots(s, x - ox, cy - oy, white, px); }
            if all.is_empty() {
                // Nothing to show: the right wing says it opens downward.
                let g = px.of(10.0);
                self.fonts.draw_icon(s, icons::CARET_DOWN, g, (island.right() - wing + (wing - g) / 2.0).round(), (cy - oy - g / 2.0).round(), fade(white, 0.8));
            }
            if let (Some(line), Some((item, _))) = (&line, &now.notice) {
                let row = Rect::new(island.x + px.of(12.0), n.height as f32, island.w - px.of(24.0), island.h - n.height as f32);
                draw_notice(&mut self.fonts, s, row, item, line, title, marks, px);
            }
        }
        if let (Some(r), Some(text)) = (tip_rect, &tip_text) {
            draw_tip(&mut self.fonts, s, r, text, label, { let p = self.theme.paper; [p[0], p[1], p[2], 1.0] }, self.theme.ink, shadow, px);
        }
        self.hatch_badge_show();
    }

    /// A click on the closed hatch: a lamp opens its own session; anywhere
    /// else the notice's session while one shows, or Work.
    pub(crate) fn hatch_badge_click(&mut self) {
        let lamp = self.hatch_state.badge_hover
            .and_then(|(x, y)| self.hatch_state.badge_hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, t)| *t));
        self.hatch_badge_open(lamp);
    }

    fn hatch_badge_open(&mut self, lamp: Option<WorkTarget>) {
        let notice = self.hatch_state.completion.take().map(|(item, _)| item.target);
        match lamp.or(notice) {
            Some(target) => self.hatch_click(Hit::Job(target)),
            None => {
                let previous = self.hatch_state.foreground.clone();
                self.show_hatch_work();
                self.hatch_state.foreground = previous;
            }
        }
    }

    pub(crate) fn hatch_badge_access_tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let (ox, oy) = self.hatch_state.badge.as_ref().and_then(|b| b.position).unwrap_or((0, 0));
        let bounds = |r: Rect| accesskit::Rect { x0: (r.x - ox as f32) as f64, y0: (r.y - oy as f32) as f64, x1: (r.right() - ox as f32) as f64, y1: (r.bottom() - oy as f32) as f64 };
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        if let Some(tab) = self.hatch_state.badge_tab {
            let mut node = Node::new(Role::Button);
            node.set_label("Ongoing work");
            node.add_action(Action::Click);
            node.set_bounds(bounds(tab));
            children.push(NodeId(2));
            nodes.push((NodeId(2), node));
        }
        for (n, (r, target)) in self.hatch_state.badge_hits.iter().enumerate() {
            let Some(item) = self.hatch_state.work.iter().find(|i| i.target == *target) else { continue };
            let mut node = Node::new(Role::Button);
            node.set_label(format!("{}, {}, {}", words(item), item.space, item.cwd));
            node.add_action(Action::Click);
            node.set_bounds(bounds(*r));
            let id = NodeId(10 + n as u64);
            children.push(id);
            nodes.push((id, node));
        }
        let mut root = Node::new(Role::Window);
        let summary = crate::hatch_work::summary(&self.hatch_state.work);
        root.set_label(if summary.starts_with("Hatch") { summary } else { format!("Hatch · {summary}") });
        root.set_children(children);
        nodes.push((NodeId(1), root));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(NodeId(1))), tree_id: TreeId::ROOT, focus: NodeId(1) }
    }

    pub(crate) fn hatch_badge_access_action(&mut self, request: accesskit::ActionRequest) {
        if request.action != accesskit::Action::Click { return; }
        match request.target_node.0 {
            2 => self.hatch_badge_open(None),
            n if n >= 10 => {
                if let Some((_, target)) = self.hatch_state.badge_hits.get((n - 10) as usize).copied() {
                    self.hatch_badge_open(Some(target));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hatch_work::Target;

    fn item(tab: u64, status: Status, unread: bool) -> Item {
        Item { target: Target { window: 1, tab, right: false }, title: format!("job {tab}"), command: String::new(), space: String::new(), cwd: String::new(), status, exit: None, progress: None, unread }
    }

    #[test]
    fn only_work_that_is_going_or_unseen_gets_a_lamp() {
        let work = vec![item(1, Status::Idle, false), item(2, Status::Finished, false), item(3, Status::Finished, true), item(4, Status::Failed, false), item(5, Status::Failed, true)];
        let kinds: Vec<_> = lamps(&work).into_iter().map(|(_, l)| l).collect();
        assert_eq!(kinds, vec![Lamp::Finished, Lamp::Failed]);
    }

    #[test]
    fn lamps_keep_their_places_whatever_needs_you() {
        // Work arrives sorted by urgency; the lamps stay in opening order.
        let mut work = vec![item(3, Status::Running, false), item(1, Status::Running, false), item(2, Status::NeedsInput, false)];
        crate::hatch_work::sort(&mut work);
        let tabs: Vec<_> = lamps(&work).into_iter().map(|(i, _)| i.target.tab).collect();
        assert_eq!(tabs, vec![1, 2, 3]);
    }

    #[test]
    fn progress_and_attention_have_their_own_shapes() {
        let mut running = item(1, Status::Running, false);
        running.progress = Some(62);
        assert_eq!(lamp(&running), Some(Lamp::Progress(62)));
        assert_eq!(lamp(&item(2, Status::Attention, false)), Some(Lamp::Rang));
        assert_eq!(words(&running), "job 1 · running 62 %");
        let mut failed = item(3, Status::Failed, true);
        failed.exit = Some(101);
        assert_eq!(words(&failed), "job 3 · failed · exit 101");
    }

    #[test]
    fn a_wing_keeps_a_place_for_the_dots() {
        // 7 px lamps, 6 px gaps, 10 px of dots in a 40 px wing.
        assert_eq!(fit(2, 40.0, 7.0, 6.0, 10.0), (2, false));
        assert_eq!(fit(3, 40.0, 7.0, 6.0, 10.0), (3, false));
        assert_eq!(fit(5, 40.0, 7.0, 6.0, 10.0), (2, true));
        assert_eq!(fit(0, 40.0, 7.0, 6.0, 10.0), (0, false));
        assert_eq!(fit(4, 3.0, 7.0, 6.0, 10.0), (0, true));
    }

    #[test]
    fn going_work_goes_left_on_the_island() {
        assert!(Lamp::Running.going() && Lamp::Progress(3).going());
        assert!(!Lamp::Input.going() && !Lamp::Rang.going() && !Lamp::Finished.going() && !Lamp::Failed.going());
    }
}
