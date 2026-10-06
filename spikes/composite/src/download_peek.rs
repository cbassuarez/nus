//! The download peek (canvas page-9, C · Peek). A download you start puts
//! the file on a shelf: a row just above the sidebar's foot that stays until
//! you look. When the sidebar is hidden, the foot itself slides out from the
//! edge with the row on it, holds, and slides back; a notch on the hot edge
//! keeps the state while it's away. One colour says everything: working is
//! the signal filling a faint track, done is the signal lit (HDR radiance on
//! displays that have it, a lighter, richer signal elsewhere) and marked with
//! a dot, failed is the signal dulled and hatched.
use crate::anim::{Anim, Curve};
use crate::app::{App, Pane};
use crate::downloads::{list, Download, Hit};
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Color, Rect, Scene, Style};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Instant;

/// Seconds the peek stays out once it has landed (paused while hovered or
/// while the window is in the background).
const HOLD: f32 = 2.9;
/// Rows the shelf shows; the newest win.
const SHELF_MAX: usize = 3;
/// Linear-light gain for the lit state on HDR displays (the caret's range).
const LIT_GAIN: f32 = 1.7;
/// A finished download that took longer than this plays `download.done`.
const SLOW_SECS: u64 = 5;

/// Downloads on the shelf, newest first. Downloads are process-wide, so the
/// shelf is too: what you've looked at in one window is looked at in all.
static SHELF: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn shelf() -> Vec<u64> {
    SHELF.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// On the shelf now: the shelf says it finished, so nothing else need.
pub fn on_shelf(key: u64) -> bool {
    SHELF.lock().unwrap_or_else(|e| e.into_inner()).contains(&key)
}

/// Off the shelf: you've looked (opened it, showed it, retried, dismissed).
pub fn seen(key: u64) {
    SHELF.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| *k != key);
}

/// What a row, the notch and the foot's icon say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Bytes arriving; the fraction when the size is known.
    Working(Option<f32>),
    Paused(Option<f32>),
    Done,
    Failed,
}

impl State {
    pub fn of(d: &Download) -> State {
        let frac = (d.total > 0).then(|| (d.received as f32 / d.total as f32).clamp(0.0, 1.0));
        if d.done {
            State::Done
        } else if d.interrupted || (!d.live && !d.cancelled) {
            State::Failed
        } else if d.paused {
            State::Paused(frac)
        } else {
            State::Working(frac)
        }
    }

    /// Several downloads as one mark: anything moving wins (with the total
    /// fraction), then paused, then a failure, then done.
    fn of_all(rows: &[&Download]) -> Option<State> {
        if rows.is_empty() {
            return None;
        }
        let frac = |rows: &[&&Download]| {
            let total: i64 = rows.iter().map(|d| d.total).sum();
            (rows.iter().all(|d| d.total > 0) && total > 0).then(|| (rows.iter().map(|d| d.received).sum::<i64>() as f32 / total as f32).clamp(0.0, 1.0))
        };
        let moving: Vec<&&Download> = rows.iter().filter(|d| matches!(State::of(d), State::Working(_))).collect();
        if !moving.is_empty() {
            return Some(State::Working(frac(&moving)));
        }
        let paused: Vec<&&Download> = rows.iter().filter(|d| matches!(State::of(d), State::Paused(_))).collect();
        if !paused.is_empty() {
            return Some(State::Paused(frac(&paused)));
        }
        if rows.iter().any(|d| State::of(d) == State::Failed) {
            return Some(State::Failed);
        }
        Some(State::Done)
    }
}

/// The peek's own state, one per window (it lives in `downloads::Ui`).
#[derive(Default)]
pub struct Peek {
    /// Keys already accounted for; the first tick takes the history as known.
    known: Option<HashSet<u64>>,
    /// Shelf downloads already done, so `download.done` plays once.
    finished: HashSet<u64>,
    /// 0 = away, 1 = out. None when there's no peek at all.
    slide: Option<Anim>,
    /// Seconds held since it landed, and when that was last counted.
    held: f32,
    counted: Option<Instant>,
    /// Where the peek was drawn this frame (for clicks and hover).
    pub rect: Option<Rect>,
    /// Where the shelf was drawn this frame, peek or sidebar.
    pub shelf_rect: Option<Rect>,
}

/// The shelf's downloads as they stand now, newest first, at most SHELF_MAX.
fn shelf_rows(all: &[Download]) -> Vec<Download> {
    shelf().iter().filter_map(|k| all.iter().find(|d| d.key == *k)).take(SHELF_MAX).cloned().collect()
}

impl App {
    /// Once a tick, from `tend_downloads`: new downloads onto the shelf, the
    /// peek's hold, and the shelf's tidying.
    pub(crate) fn tend_download_peek(&mut self) {
        let all = list();
        let fresh: Vec<Download> = match &mut self.download_ui.peek.known {
            None => {
                self.download_ui.peek.known = Some(all.iter().map(|d| d.key).collect());
                Vec::new()
            }
            Some(known) => all.iter().filter(|d| known.insert(d.key)).cloned().collect(),
        };
        let mut arrived = false;
        {
            let mut s = SHELF.lock().unwrap_or_else(|e| e.into_inner());
            for d in fresh.iter().filter(|d| d.live && d.origin.is_some()) {
                if !s.contains(&d.key) {
                    s.insert(0, d.key);
                }
                arrived = true;
            }
            // Gone from history, or cancelled (your own doing): off the shelf.
            s.retain(|k| all.iter().any(|d| d.key == *k && !d.cancelled));
            // Too many: drop the oldest finished ones first.
            while s.len() > SHELF_MAX {
                let stale = s.iter().rposition(|k| all.iter().any(|d| d.key == *k && !d.active())).unwrap_or(s.len() - 1);
                s.remove(stale);
            }
        }
        // Looking at the list counts as looking at what's finished.
        let looking = self.dl_menu || self.tabs.get(self.active).is_some_and(|t| matches!(t.panes().0, Pane::Downloads(_)));
        if looking {
            SHELF.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| all.iter().any(|d| d.key == *k && d.active()));
        }
        // A shelf download that took a while: the done cue, once.
        let now_secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        for d in shelf_rows(&all) {
            if d.done && self.download_ui.peek.finished.insert(d.key) && now_secs.saturating_sub(d.started) > SLOW_SECS && self.window_focused {
                self.play_event("download.done");
            }
        }
        // Out it comes: you started it here, nothing already shows it, and
        // motion is allowed. Reduced motion keeps the shelf and the notch.
        let quiet = self.motion.reduced() || self.focus || self.fullscreen || looking;
        if arrived && self.window_focused && !self.sidebar_visible() && self.sidebar_hoverable() && !quiet {
            let mut a = self.download_ui.peek.slide.take().unwrap_or_else(|| Anim::at_on(0.0, Curve::Glide));
            a.go(1.0, self.motion.travel(crate::anim::base::SIDEBAR));
            self.download_ui.peek.slide = Some(a);
            self.download_ui.peek.held = 0.0;
            self.download_ui.peek.counted = None;
        }
        self.tend_peek_hold();
        if arrived {
            self.dirty = true;
        }
    }

    fn tend_peek_hold(&mut self) {
        let Some((value, target, moving)) = self.download_ui.peek.slide.as_ref().map(|a| (a.value(), a.target(), a.active())) else { return };
        // The sidebar came out (a hover, a pin): it shows the shelf itself.
        if self.sidebar_visible() || self.focus {
            self.download_ui.peek.slide = None;
            self.dirty = true;
            return;
        }
        if moving {
            self.dirty = true;
        }
        if target < 0.5 {
            if !moving && value <= 0.001 {
                self.download_ui.peek.slide = None;
                self.dirty = true;
            }
            return;
        }
        if moving {
            return;
        }
        let now = crate::clock::now();
        let hovered = self.download_ui.peek.rect.is_some_and(|r| r.contains(self.mouse.0, self.mouse.1));
        if let Some(last) = self.download_ui.peek.counted {
            if !hovered && self.window_focused {
                self.download_ui.peek.held += now.duration_since(last).as_secs_f32();
            }
        }
        self.download_ui.peek.counted = Some(now);
        if self.download_ui.peek.held >= HOLD {
            let dur = self.motion.travel(crate::anim::base::SIDEBAR);
            if let Some(a) = self.download_ui.peek.slide.as_mut() {
                a.go(0.0, dur);
            }
            self.dirty = true;
        }
    }

    /// The shelf's height for a sidebar (or peek) this wide.
    pub(crate) fn shelf_h(&self, w: f32) -> f32 {
        let n = shelf().iter().filter(|k| list().iter().any(|d| d.key == **k)).take(SHELF_MAX).count();
        n as f32 * self.shelf_row_h(w)
    }

    fn shelf_row_h(&self, w: f32) -> f32 {
        self.px(if self.shelf_narrow(w) { 30.0 } else { 42.0 })
    }

    fn shelf_narrow(&self, w: f32) -> bool {
        w < self.px(150.0)
    }

    /// What the foot's downloads icon says: anything moving (all of it,
    /// shelf or not), else the shelf's verdict.
    pub(crate) fn download_dial(&self) -> Option<State> {
        let all = list();
        let active: Vec<&Download> = all.iter().filter(|d| d.active()).collect();
        if !active.is_empty() {
            return State::of_all(&active);
        }
        let rows = shelf_rows(&all);
        State::of_all(&rows.iter().collect::<Vec<_>>())
    }

    /// The signal lit, for SDR: lighter and richer in OKLCH, the same hue.
    fn lit_sdr(&self) -> Color {
        use nus_render::oklch::{from_srgb, to_srgb, Lch};
        let paper = self.paper();
        let p = from_srgb(self.surface.signal);
        let dark = nus_render::policy::luminance(paper) < 0.4;
        let lit = to_srgb(Lch { l: p.l + if dark { 0.13 } else { 0.06 }, c: p.c * 1.25, h: p.h }, 1.0);
        nus_render::policy::ensure_contrast(lit, paper, 3.0)
    }

    /// The signal dulled, for a failure: toward the paper, less chroma.
    fn dull(&self) -> Color {
        use nus_render::oklch::{from_srgb, to_srgb, Lch};
        let paper = self.paper();
        let p = from_srgb(self.surface.signal);
        let toward = from_srgb(paper).l - p.l;
        let dull = to_srgb(Lch { l: p.l + toward * 0.22, c: p.c * 0.55, h: p.h }, 1.0);
        nus_render::policy::ensure_contrast(dull, paper, 2.6)
    }

    /// The colour words and icons take for a state (the HDR light is only
    /// for marks: text stays at SDR so it never blooms).
    fn state_ink(&self, s: State) -> Color {
        match s {
            State::Working(_) => self.surface.signal,
            State::Paused(_) => self.theme.dim,
            State::Done => self.lit_sdr(),
            State::Failed => self.dull(),
        }
    }

    /// A lit mark: radiance on HDR displays, the lifted signal elsewhere.
    fn lit(&self, scene: &mut Scene, r: Rect) {
        if self.target.hdr() {
            scene.caret(r, self.surface.signal, 0.35, LIT_GAIN);
        } else {
            scene.rect(r, self.lit_sdr());
        }
    }

    /// A state as a bar: horizontal fills left to right, vertical bottom up.
    pub(crate) fn draw_state_bar(&self, scene: &mut Scene, r: Rect, s: State, vertical: bool) {
        let signal = self.surface.signal;
        let part = |f: f32| {
            if vertical {
                let h = (r.h * f).round();
                Rect::new(r.x, r.bottom() - h, r.w, h)
            } else {
                Rect::new(r.x, r.y, (r.w * f).round(), r.h)
            }
        };
        match s {
            State::Working(f) | State::Paused(f) => {
                let a = if matches!(s, State::Paused(_)) { 0.55 } else { 1.0 };
                scene.rect(r, crate::app::fade(signal, 0.26));
                // Unknown size: a third, so it reads as begun rather than empty.
                scene.rect(part(f.unwrap_or(0.33).max(0.04)), crate::app::fade(signal, a));
            }
            State::Done => self.lit(scene, r),
            State::Failed => {
                let (step, on) = (self.px(4.0).round().max(3.0), self.px(2.0).round().max(1.0));
                let c = self.dull();
                let long = if vertical { r.h } else { r.w };
                let mut t = 0.0;
                while t < long {
                    let len = on.min(long - t);
                    scene.rect(if vertical { Rect::new(r.x, r.bottom() - t - len, r.w, len) } else { Rect::new(r.x + t, r.y, len, r.h) }, c);
                    t += step;
                }
            }
        }
    }

    /// The done dot: lit, round, beside a mark.
    fn done_dot(&self, scene: &mut Scene, x: f32, y: f32) {
        let d = self.px(5.0).round();
        if self.target.hdr() {
            scene.caret(Rect::new(x, y, d, d), self.surface.signal, 0.0, LIT_GAIN);
        } else {
            scene.push(nus_render::Instance::rounded(Rect::new(x, y, d, d), d * 0.5, self.lit_sdr()));
        }
    }

    /// The foot's downloads icon as a dial: the underline fills by bytes,
    /// a lit dot when something finished you haven't looked at, a hatch when
    /// something failed.
    pub(crate) fn draw_download_dial(&mut self, scene: &mut Scene, size: f32, ix: f32, iy: f32, r: Rect, key: u64) {
        let dial = self.download_dial();
        let color = match dial {
            Some(State::Working(_)) => self.surface.signal,
            Some(State::Paused(_)) => self.theme.dim,
            _ => self.theme.ink,
        };
        self.icon_button(scene, icons::DOWNLOAD, size, ix, iy, color, r, key, crate::app::IconMotion::Bob);
        let Some(s) = dial else { return };
        let bar = Rect::new(ix, r.bottom() - self.px(4.0), size, self.px(2.0));
        match s {
            State::Done => self.done_dot(scene, ix + size - self.px(3.0), iy - self.px(2.0)),
            _ => self.draw_state_bar(scene, bar, s, false),
        }
    }

    /// The shelf: one row per download, drawn into `r` (above the foot, in
    /// the sidebar or in the peek).
    pub(crate) fn draw_shelf(&mut self, scene: &mut Scene, r: Rect) {
        let rows = shelf_rows(&list());
        self.download_ui.peek.shelf_rect = None;
        if rows.is_empty() {
            return;
        }
        self.download_ui.peek.shelf_rect = Some(r);
        scene.rect(r, self.paper());
        scene.hline(r.x, r.y, r.w, self.px(1.0), self.theme.ink);
        let h = self.shelf_row_h(r.w);
        for (i, d) in rows.iter().enumerate() {
            let row = Rect::new(r.x, r.y + i as f32 * h, r.w, h);
            self.draw_shelf_row(scene, row, d);
        }
    }

    fn draw_shelf_row(&mut self, scene: &mut Scene, r: Rect, d: &Download) {
        let s = State::of(d);
        let k = d.key;
        let hot = r.contains(self.mouse.0, self.mouse.1);
        if hot {
            scene.rect(r, self.theme.tint);
        }
        scene.hline(r.x, r.bottom() - self.px(1.0), r.w, self.px(1.0), self.theme.tint);
        // The row itself: done opens, failed retries, the rest opens the list.
        let body = match s {
            State::Done => Hit::Open(k),
            State::Failed => Hit::Retry(k),
            _ => Hit::Page,
        };
        self.download_ui.hits.push((r, body));
        let icon = match s {
            State::Working(_) => icons::DOWNLOAD,
            State::Paused(_) => icons::PAUSE,
            State::Done => icons::CHECK_CIRCLE,
            State::Failed => icons::WARNING,
        };
        let ink = self.state_ink(s);
        let bar = Rect::new(r.x, r.bottom() - self.px(2.0), r.w, self.px(2.0));
        if self.shelf_narrow(r.w) {
            let size = self.px(15.0);
            self.fonts.draw_icon(scene, icon, size, (r.x + (r.w - size) * 0.5).round(), (r.y + (r.h - size) * 0.5 - self.px(1.0)).round(), ink);
            self.draw_state_bar(scene, bar, s, false);
            if hot {
                let words = format!("{} · {}", d.name, d.status());
                self.offer_tip(crate::app::hover_key("shelf", k as usize), r, words);
            }
            return;
        }
        let pad = self.px(11.0);
        let size = self.px(15.0);
        self.fonts.draw_icon(scene, icon, size, r.x + pad, (r.y + (r.h - size) * 0.5).round(), ink);
        // Actions at the right edge: what you'd want for this state, then ×.
        let b = self.px(24.0);
        let acts: Vec<Hit> = match s {
            State::Working(_) => vec![Hit::Pause(k), Hit::Cancel(k)],
            State::Paused(_) => vec![Hit::Resume(k), Hit::Cancel(k)],
            State::Done => vec![Hit::Reveal(k), Hit::Dismiss(k)],
            State::Failed => vec![Hit::Retry(k), Hit::Dismiss(k)],
        };
        let mut ax = r.right() - self.px(6.0) - b * acts.len() as f32;
        let text_w = (ax - (r.x + pad + size + self.px(9.0)) - self.px(4.0)).max(0.0);
        for hit in acts {
            self.download_button(scene, Rect::new(ax, (r.y + (r.h - b) * 0.5).round(), b, b), "", hit);
            ax += b;
        }
        let tx = r.x + pad + size + self.px(9.0);
        let name_st = self.label();
        let note_st = Style { color: if matches!(s, State::Working(_)) { self.theme.dim } else { ink }, px: name_st.px * 0.86, ..name_st };
        let name = self.fit(name_st, d.name.as_str(), text_w).into_owned();
        self.fonts.draw(scene, name_st, tx, r.y + r.h * 0.5 - self.px(2.0), &name);
        let note = match s {
            State::Done => {
                let host = crate::sites::host_of(if d.source_url.is_empty() { &d.url } else { &d.source_url });
                if host.is_empty() { format!("done · {}", crate::downloads::bytes(d.received.max(d.total))) } else { format!("done · {} · {host}", crate::downloads::bytes(d.received.max(d.total))) }
            }
            State::Failed => "failed · ready to retry".into(),
            _ => d.status().to_lowercase(),
        };
        let note = self.fit(note_st, note.as_str(), text_w).into_owned();
        self.fonts.draw(scene, note_st, tx, r.y + r.h * 0.5 + self.px(12.0), &note);
        self.draw_state_bar(scene, bar, s, false);
    }

    /// Over the content, after the hover sidebar: the notch on the hot edge
    /// and the peek itself.
    pub(crate) fn draw_download_peek(&mut self, scene: &mut Scene) {
        self.download_ui.peek.rect = None;
        if !self.sidebar_visible() {
            self.download_ui.peek.shelf_rect = None;
        }
        if self.sidebar_visible() || self.focus {
            return;
        }
        let c = self.content_rect();
        let right = self.sidebar_right();
        let all = list();
        let rows = shelf_rows(&all);
        let slide = self.download_ui.peek.slide.as_ref().map_or(0.0, |a| a.value());
        // The notch: where the foot would be, on the 4 px edge.
        if let (Some(s), true) = (State::of_all(&rows.iter().collect::<Vec<_>>()), self.sidebar_hoverable() && !self.sidebar_hover) {
            let foot = self.sidebar_foot_h();
            let h = self.px(20.0).round();
            let x = if right { c.right() } else { c.x - self.px(4.0) };
            let r = Rect::new(x, (c.bottom() - foot * 0.5 - h * 0.5).round(), self.px(4.0), h);
            scene.layer(None);
            self.draw_state_bar(scene, r, s, true);
            if s == State::Done {
                let d = self.px(5.0).round();
                self.done_dot(scene, if right { r.x - d - self.px(3.0) } else { r.right() + self.px(3.0) }, r.y - d - self.px(3.0));
            }
        }
        if slide <= 0.001 {
            return;
        }
        // The peek: the sidebar's foot with the shelf on it, sliding in from
        // the edge the sidebar lives on.
        let w = self.sidebar_w().max(self.px(260.0)).min(c.w * 0.8);
        let total = self.shelf_h(w) + self.foot_h_for(w);
        let off = ((1.0 - slide) * (w + self.px(12.0))).round();
        let sb = self.sidebar_rect();
        let x = if right { sb.right() - w + off } else { sb.x - off };
        let r = Rect::new(x, c.bottom() - total, w, total);
        let ink = self.theme.ink;
        let shadow = if right { -self.px(6.0) } else { self.px(6.0) };
        scene.layer(None);
        scene.rect(Rect::new(r.x + shadow, r.y - self.px(2.0), r.w, r.h + self.px(2.0)), crate::app::fade(ink, 0.18 * slide));
        scene.rect(r, self.paper());
        self.draw_responsive_footer(scene, r, r.y);
        scene.layer(None);
        let edge = self.px(m::STRUCTURE);
        scene.hline(r.x, r.y - edge, r.w, edge, ink);
        scene.vline(if right { r.x - edge } else { r.right() }, r.y - edge, r.h + edge, edge, ink);
        self.download_ui.peek.rect = Some(Rect::new(r.x, r.y - edge, r.w + edge, r.h + edge));
    }

    /// A press on the shelf or the peek. The row's buttons are download
    /// hits; the peek's foot answers like the sidebar's.
    pub(crate) fn download_peek_click(&mut self, x: f32, y: f32) -> bool {
        if self.dl_menu {
            return false;
        }
        let in_shelf = self.download_ui.peek.shelf_rect.is_some_and(|r| r.contains(x, y));
        let in_peek = self.download_ui.peek.rect.is_some_and(|r| r.contains(x, y));
        if !in_shelf && !in_peek {
            return false;
        }
        if in_shelf && self.download_click(x, y) {
            return true;
        }
        if in_peek {
            if let Some((_, hit)) = self.side_hits.iter().rev().find(|(r, _)| r.contains(x, y)).copied() {
                self.download_ui.peek.slide = None;
                self.side_action(hit, true);
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(received: i64, total: i64) -> Download {
        Download { key: 1, received, total, live: true, ..Default::default() }
    }

    #[test]
    fn states_read_the_row() {
        assert_eq!(State::of(&dl(50, 100)), State::Working(Some(0.5)));
        assert_eq!(State::of(&dl(50, 0)), State::Working(None));
        assert_eq!(State::of(&Download { done: true, ..dl(100, 100) }), State::Done);
        assert_eq!(State::of(&Download { interrupted: true, ..dl(10, 100) }), State::Failed);
        assert_eq!(State::of(&Download { paused: true, ..dl(25, 100) }), State::Paused(Some(0.25)));
        // From history and never finished: it won't finish now.
        assert_eq!(State::of(&Download { live: false, ..dl(10, 100) }), State::Failed);
    }

    #[test]
    fn several_read_as_one() {
        let a = dl(30, 100);
        let b = Download { done: true, ..dl(100, 100) };
        let c = Download { interrupted: true, ..dl(5, 100) };
        assert_eq!(State::of_all(&[&a, &b]), Some(State::Working(Some(0.3))));
        assert_eq!(State::of_all(&[&b, &c]), Some(State::Failed));
        assert_eq!(State::of_all(&[&b]), Some(State::Done));
        assert_eq!(State::of_all(&[]), None);
        let d = dl(70, 100);
        assert_eq!(State::of_all(&[&a, &d]), Some(State::Working(Some(0.5))));
    }
}
