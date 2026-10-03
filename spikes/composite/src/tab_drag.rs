//! A tab row in hand: what letting go would do, said before it happens.
//!
//! A drag carries one verb at a time, and the keys choose it:
//!
//! - no key: MOVE. In the list, the rows' edges place the tab and its depth
//!   is read off how far across the pointer has travelled since the lift
//!   (a lane, 24 px, per level), so in and out of a stack is a sideways
//!   nudge; a row's middle stacks it under that row. Over the page, the
//!   quarters tile (pane_mode.rs's drop zones).
//! - shift: JOIN. The tab goes into another as its second pane: onto a row,
//!   or onto a pane's left or right half.
//! - ctrl: COPY. A shell in the same folder, or the same page, placed the
//!   way a move would place it.
//! - 1–9: SEND to that window (the order of the window rail), N: a new one,
//!   0 or Backspace takes it back. Near the window's top edge a shelf of
//!   windows comes down for the pointer to choose from instead; let go
//!   outside the window and it goes to the window there (or a new one).
//! - esc: nothing happens, and the ghost goes home.
//!
//! One `Intent` is worked out per frame; the ghost, the target's marks and
//! the director bar along the window's foot all draw from it, and the drop
//! applies it, so what is shown is what happens. Every drop in the window is
//! one step of the layout history (Ctrl+Alt+Z).
//!
//! Shift and ctrl change a press on a row into selecting; they take a drag
//! over only once it has started, and can change mid-drag. Release decides.

use crate::anim::Anim;
use crate::app::{fade, App, Caps, Pane};
use crate::director::Op;
use crate::pane_mode::{Dragging, Drop};
use crate::send::Dest;
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};
use std::time::Instant;
use winit::event::ElementState;
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, PhysicalKey};

/// Travel across, per level of depth.
const LANE: f32 = 24.0;
/// Extra travel before the depth gives up the level it has.
const LANE_SLACK: f32 = 4.0;
/// How far into the page the list still counts.
const SIDEBAR_EDGE: f32 = 8.0;
/// The band along the top edge that brings the shelf down, and its delay.
const SHELF_BAND: f32 = 24.0;
const SHELF_DELAY_MS: u128 = 120;
const SHELF_H: f32 = 52.0;
/// The director bar along the window's foot.
const BAR_H: f32 = 28.0;
/// How far a level indents a row as drawn (app.rs, draw_sidebar).
const INDENT: f32 = 12.0;
/// The ghost hangs this far under the pointer: what it aims at stays in sight.
const GHOST_DROP: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Move,
    Stack,
    Unstack,
    Tile,
    Join,
    Copy,
    Send,
}

impl Verb {
    fn word(self) -> &'static str {
        match self {
            Verb::Move => "move",
            Verb::Stack => "stack",
            Verb::Unstack => "unstack",
            Verb::Tile => "tile",
            Verb::Join => "join",
            Verb::Copy => "copy",
            Verb::Send => "send",
        }
    }

    fn icon(self) -> Option<(&'static str, &'static str)> {
        match self {
            Verb::Move => None,
            Verb::Stack => Some(icons::STACK),
            Verb::Unstack => Some(icons::TO_TAB),
            Verb::Tile => Some(icons::TILES),
            Verb::Join => Some(icons::SIDEBAR),
            Verb::Copy => Some(icons::COPY),
            Verb::Send => Some(icons::APP_WINDOW),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Nothing would happen.
    Nothing,
    /// Into the list: under `parent`, before `before`; drawn as a rule at
    /// `y`, indented to `depth`.
    Place { parent: Option<usize>, before: Option<usize>, depth: usize, y: f32 },
    /// Into the stack under this row, at its end.
    Nest(usize),
    /// Into this tab as its second pane, on that side; `preview` is the
    /// half of the page it would take, when the page is where it was aimed.
    Join { tab: usize, side_right: bool, preview: Option<Rect> },
    /// Beside (or swapped with) what is on the page.
    Tile(Drop),
    /// Out of this window.
    Send(Dest),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Intent {
    pub verb: Verb,
    pub target: Target,
    /// What letting go does, in a few words.
    pub words: String,
    /// False: shown, explained, and letting go does nothing.
    pub ok: bool,
}

impl Intent {
    fn new(verb: Verb, target: Target, words: impl Into<String>) -> Intent {
        Intent { verb, target, words: words.into(), ok: true }
    }

    fn no(verb: Verb, words: impl Into<String>) -> Intent {
        Intent { verb, target: Target::Nothing, words: words.into(), ok: false }
    }
}

/// A drag's memory between frames: where it was lifted, the levels and
/// rows it holds onto against jitter, a window picked by key, and the
/// ghost's way home after esc.
#[derive(Default)]
pub struct TabDrag {
    x0: f32,
    depth0: usize,
    depth: Option<usize>,
    nest: Option<u64>,
    send: Option<Dest>,
    edge_since: Option<Instant>,
    /// esc let go of the drag; the button's release is still to come.
    swallow_release: bool,
    home: Option<(Rect, Rect, Anim, String)>,
}

/// A card on the shelf: its window (None: a new one), name, colour, tabs.
struct Card {
    rect: Rect,
    dest: Dest,
    name: String,
    colour: nus_render::Color,
    tabs: usize,
    here: bool,
    key: String,
}

fn short(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 22 {
        return s.to_string();
    }
    let mut out: String = s.chars().take(21).collect();
    out.push('…');
    out
}

/// A gap between two rows (index, depth), and the depth asked for: the
/// depth it can have (no deeper than under the row above, no shallower
/// than the row below), the parent at that depth and the row to go before.
fn gap_place(above: Option<(usize, usize)>, below: Option<(usize, usize)>, want: usize, ancestor: impl Fn(usize, usize) -> usize) -> (usize, Option<usize>, Option<usize>) {
    let hi = above.map(|a| a.1 + 1).unwrap_or(0);
    let lo = below.map(|b| b.1).unwrap_or(0).min(hi);
    let depth = want.clamp(lo, hi);
    let parent = if depth == 0 { None } else { above.map(|a| ancestor(a.0, depth - 1)) };
    let before = below.filter(|b| b.1 == depth).map(|b| b.0);
    (depth, parent, before)
}

impl App {
    /// The drag has started: remember where across it was lifted.
    pub(crate) fn tab_drag_lift(&mut self, i: usize, x: f32) {
        self.tab_drag = TabDrag { x0: x, depth0: self.depth(i), ..TabDrag::default() };
    }

    fn row_title(&self, i: usize) -> String {
        self.tabs.get(i).map(|t| short(&t.title())).unwrap_or_default()
    }

    /// The window's own size, physical px.
    fn win_size(&self) -> (f32, f32) {
        (self.target.size.0 as f32, self.target.size.1 as f32)
    }

    /// What letting go here would do. Also moves the drag's memory along.
    pub(crate) fn tab_intent(&mut self, d: usize) -> Intent {
        if d >= self.tabs.len() {
            return Intent::no(Verb::Move, "that tab has gone");
        }
        let (mx, my) = self.mouse;
        let (w, h) = self.win_size();
        // A window picked by key holds until another key changes it.
        if let Some(dest) = self.tab_drag.send {
            return self.send_intent(d, dest);
        }
        if mx < 0.0 || my < 0.0 || mx >= w || my >= h {
            let dest = self.dropped_outside().unwrap_or(Dest::New);
            return self.send_intent(d, dest);
        }
        // The top edge brings the shelf down after a moment; it stays while
        // the pointer is on it, and the card is the one above the pointer.
        let on_shelf = my < self.px(SHELF_H) && self.shelf_open();
        if my < self.px(SHELF_BAND) {
            self.tab_drag.edge_since.get_or_insert_with(crate::clock::now);
        } else if !on_shelf {
            self.tab_drag.edge_since = None;
        }
        if self.shelf_open() && my < self.px(SHELF_H) {
            if let Some(c) = self.shelf_cards().into_iter().find(|c| mx >= c.rect.x - self.px(3.0) && mx < c.rect.right() + self.px(3.0)) {
                return if c.here { Intent::no(Verb::Send, "this window · pick another") } else { self.send_intent(d, c.dest) };
            }
            return Intent::no(Verb::Send, "onto a window");
        }
        let shift = self.mods.shift_key();
        let ctrl = self.mods.control_key() && !shift;
        if self.over_list(mx, my) {
            self.list_intent(d, mx, my, shift, ctrl)
        } else {
            self.tab_drag.nest = None;
            self.page_intent(d, mx, my, shift, ctrl)
        }
    }

    fn over_list(&self, x: f32, y: f32) -> bool {
        if !self.sidebar_visible() {
            return false;
        }
        let sb = self.sidebar_rect();
        let edge = self.px(SIDEBAR_EDGE);
        let across = if self.sidebar_right() { x >= sb.x - edge } else { x < sb.right() + edge };
        across && y >= sb.y && y < sb.bottom()
    }

    fn shelf_open(&self) -> bool {
        self.tab_drag.send.is_some() || self.tab_drag.edge_since.is_some_and(|t| t.elapsed().as_millis() >= SHELF_DELAY_MS)
    }

    fn send_intent(&self, d: usize, dest: Dest) -> Intent {
        if let Some(why) = self.cannot_send(d) {
            return Intent::no(Verb::Send, why);
        }
        let words = match dest {
            Dest::Window(id) => {
                let name = self.windows.iter().find(|e| e.id == id).map(|e| e.name.clone()).unwrap_or_else(|| "that window".into());
                format!("to {name}")
            }
            Dest::New => "to a new window".into(),
            // Wayland can't say where windows are: let go out there, and a
            // new window is what it can be.
            Dest::At(..) if crate::hatch_native::wayland() => "to a new window · 1–9 picks one".into(),
            Dest::At(..) => "to the window there, or a new one".into(),
        };
        Intent::new(Verb::Send, Target::Send(dest), words)
    }

    /// The ancestor of `a` at `level` (a itself when it is there already).
    fn ancestor_at(&self, a: usize, level: usize) -> usize {
        let mut cur = a;
        while self.depth(cur) > level {
            match self.tabs[cur].parent.and_then(|p| self.tabs.iter().position(|t| t.id == p)) {
                Some(p) => cur = p,
                None => break,
            }
        }
        cur
    }

    fn copyable(&self, d: usize) -> bool {
        let t = &self.tabs[d];
        let p = if t.focus_right { t.right.as_ref().unwrap_or(&t.left) } else { &t.left };
        matches!(p, Pane::Term(_) | Pane::Web(_))
    }

    fn join_check(&self, d: usize, j: usize) -> Result<(), String> {
        if j == d {
            return Err("this is the tab in hand · aim at another".into());
        }
        if self.tabs[j].right.is_some() {
            return Err(format!("{} is already split", self.row_title(j)));
        }
        Ok(())
    }

    fn list_intent(&mut self, d: usize, mx: f32, my: f32, shift: bool, ctrl: bool) -> Intent {
        let g = self.sidebar_geometry();
        // A move takes its stack along, so its rows aren't places; a copy
        // leaves the original where it is, and its rows are.
        let moving: Vec<usize> = if ctrl { Vec::new() } else { std::iter::once(d).chain(self.subtree(d)).collect() };
        let rows: Vec<(usize, f32, f32)> = g.rows.iter().copied().filter(|r| !moving.contains(&r.0) && r.1 + r.2 > g.top && r.1 < g.foot_y).collect();
        let on = rows.iter().position(|&(_, ry, rh)| my >= ry && my < ry + rh);
        if shift {
            self.tab_drag.nest = None;
            let Some(k) = on else { return Intent::no(Verb::Join, "onto a tab, to join it") };
            let j = rows[k].0;
            return match self.join_check(d, j) {
                Ok(()) => Intent::new(Verb::Join, Target::Join { tab: j, side_right: true, preview: None }, format!("into {} · right", self.row_title(j))),
                Err(why) => Intent::no(Verb::Join, why),
            };
        }
        if ctrl && !self.copyable(d) {
            return Intent::no(Verb::Copy, "only shells and pages copy");
        }
        // On a row: its middle stacks, its edges place.
        let (above, below) = match on {
            Some(k) => {
                let (j, ry, rh) = rows[k];
                let id = self.tabs[j].id;
                let band = if self.tab_drag.nest == Some(id) { (0.25, 0.75) } else { (0.3, 0.7) };
                let frac = (my - ry) / rh.max(1.0);
                if frac >= band.0 && frac < band.1 {
                    self.tab_drag.nest = Some(id);
                    self.tab_drag.depth = None;
                    let verb = if ctrl { Verb::Copy } else { Verb::Stack };
                    let words = if ctrl { format!("into {}", self.row_title(j)) } else { format!("into {}", self.row_title(j)) };
                    return Intent::new(verb, Target::Nest(j), words);
                }
                if frac < 0.5 {
                    (k.checked_sub(1).map(|a| rows[a]), Some(rows[k]))
                } else {
                    (Some(rows[k]), rows.get(k + 1).copied())
                }
            }
            None => {
                let a = rows.iter().rposition(|&(_, ry, rh)| ry + rh / 2.0 <= my);
                match a {
                    Some(a) => (Some(rows[a]), rows.get(a + 1).copied()),
                    None => (None, rows.first().copied()),
                }
            }
        };
        self.tab_drag.nest = None;
        // The levels this gap allows, and the one the travel across asks for.
        let lane = self.px(LANE);
        let raw = self.tab_drag.depth0 as f32 + (mx - self.tab_drag.x0) / lane;
        let want = match self.tab_drag.depth {
            Some(p) if (raw - p as f32).abs() < 0.5 + self.px(LANE_SLACK) / lane => p,
            _ => raw.round().max(0.0) as usize,
        };
        let (depth, parent, before) = gap_place(above.map(|a| (a.0, self.depth(a.0))), below.map(|b| (b.0, self.depth(b.0))), want, |a, level| self.ancestor_at(a, level));
        self.tab_drag.depth = Some(depth);
        let y = above.map(|a| a.1 + a.2).or(below.map(|b| b.1)).unwrap_or(g.top);
        let target = Target::Place { parent, before, depth, y };
        let was = self.tabs[d].parent.and_then(|p| self.tabs.iter().position(|t| t.id == p));
        let place = match (before, above) {
            (Some(b), _) => format!("before {}", self.row_title(b)),
            (None, Some(a)) => format!("after {}", self.row_title(a.0)),
            (None, None) => "first".into(),
        };
        if ctrl {
            return Intent::new(Verb::Copy, target, place);
        }
        match (was, parent) {
            (w, p) if w == p => Intent::new(Verb::Move, target, place),
            (_, Some(p)) => Intent::new(Verb::Stack, target, format!("into {} · {place}", self.row_title(p))),
            (Some(w), None) => Intent::new(Verb::Unstack, target, format!("out of {} · {place}", self.row_title(w))),
            (None, None) => Intent::new(Verb::Move, target, place),
        }
    }

    fn page_intent(&mut self, d: usize, mx: f32, my: f32, shift: bool, ctrl: bool) -> Intent {
        if shift {
            let Some(s) = self.slots().into_iter().find(|s| s.rect.contains(mx, my)) else {
                return Intent::no(Verb::Join, "onto a pane, to join it");
            };
            return match self.join_check(d, s.tab) {
                Ok(()) => {
                    let side_right = mx >= s.rect.x + s.rect.w / 2.0;
                    let half = s.rect.w / 2.0;
                    let preview = Rect::new(if side_right { s.rect.x + half } else { s.rect.x }, s.rect.y, half, s.rect.h);
                    let side = if side_right { "right" } else { "left" };
                    Intent::new(Verb::Join, Target::Join { tab: s.tab, side_right, preview: Some(preview) }, format!("into {} · {side}", self.row_title(s.tab)))
                }
                Err(why) => Intent::no(Verb::Join, why),
            };
        }
        let drop = self.drop_at(mx, my, Dragging::Tab(d));
        if ctrl {
            if !self.copyable(d) {
                return Intent::no(Verb::Copy, "only shells and pages copy");
            }
            return match drop {
                Some(dr) => Intent::new(Verb::Copy, Target::Tile(dr), format!("and {}", dr.words)),
                None => Intent::no(Verb::Copy, "by a page's edge, or in the list"),
            };
        }
        match drop {
            Some(dr) => Intent::new(Verb::Tile, Target::Tile(dr), dr.words),
            // The page is the tab in hand (it was in front when lifted).
            None if self.slots().iter().any(|s| s.tab == d && s.rect.contains(mx, my)) => Intent::no(Verb::Tile, "this page is the tab in hand · tile it beside another"),
            None => Intent::no(Verb::Tile, "by an edge to tile · shift joins"),
        }
    }

    /// The tab's focused pane again, as a tab of its own at the list's end.
    fn copy_tab(&mut self, d: usize) -> Option<usize> {
        enum Make {
            Shell(usize, Option<String>),
            Page(String),
        }
        let t = &self.tabs[d];
        let p = if t.focus_right { t.right.as_ref().unwrap_or(&t.left) } else { &t.left };
        let make = match p {
            Pane::Term(tp) => Make::Shell(tp.profile, tp.cwd.clone()),
            Pane::Web(w) => Make::Page(w.tab.shared.borrow().url.clone()),
            _ => return None,
        };
        let pane = match make {
            Make::Shell(profile, cwd) => match self.new_term_pane_at(false, profile, cwd) {
                Ok(t) => Pane::Term(t),
                Err(e) => {
                    self.notice_problem("Could Not Copy", e.to_string());
                    return None;
                }
            },
            Make::Page(url) => Pane::Web(self.new_web_pane(&url)?),
        };
        let tab = self.make_tab(pane, None);
        self.tabs.push(tab);
        Some(self.tabs.len() - 1)
    }

    /// Let go: carry the intent out.
    pub(crate) fn tab_drop(&mut self, d: usize) {
        let it = self.tab_intent(d);
        self.tab_drag = TabDrag::default();
        if !it.ok || d >= self.tabs.len() {
            return;
        }
        let ids = |a: &App, i: Option<usize>| i.and_then(|i| a.tabs.get(i)).map(|t| t.id);
        let src = if it.verb == Verb::Copy {
            match self.copy_tab(d) {
                Some(k) => k,
                None => return,
            }
        } else {
            d
        };
        let id = self.tabs[src].id;
        match it.target {
            Target::Nothing => {}
            Target::Place { parent, before, .. } => {
                let op = Op::Place { tab: id, parent: ids(self, parent), before: ids(self, before) };
                self.direct(op);
                self.front(id);
                self.play_event("toggle");
            }
            Target::Nest(j) => {
                let op = Op::Place { tab: id, parent: ids(self, Some(j)), before: None };
                self.direct(op);
                self.front(id);
                self.play_event("toggle");
            }
            Target::Join { tab, side_right, .. } => {
                let to = self.tabs[tab].id;
                let right = self.tabs[src].focus_right && self.tabs[src].right.is_some();
                self.direct(Op::Join { from: id, right, to, side_right });
            }
            Target::Tile(drop) => self.apply_drop(Dragging::Tab(src), drop),
            Target::Send(dest) => {
                self.send_tab(d, dest);
            }
        }
        self.dirty = true;
    }

    fn front(&mut self, id: u64) {
        if let Some(k) = self.tabs.iter().position(|t| t.id == id) {
            self.activate(k);
        }
    }

    /// esc, or the drag otherwise abandoned: nothing happens, and the ghost
    /// slides back to its row.
    pub(crate) fn tab_drag_cancel(&mut self) {
        let Some((d, off, _)) = self.drag.take() else { return };
        self.drag_armed = None;
        self.row_host = None;
        let from = self.ghost_rect(d, off, None);
        let home = self.sidebar_geometry().rows.iter().find(|r| r.0 == d).map(|&(_, ry, rh)| {
            let sb = self.sidebar_rect();
            Rect::new(sb.x + self.px(4.0), ry, sb.w - self.px(8.0), rh.min(self.px(m::ROW_H)))
        });
        let title = self.tabs.get(d).map(|t| t.title()).unwrap_or_default();
        self.tab_drag = TabDrag { swallow_release: true, ..TabDrag::default() };
        if let (Some(home), false) = (home, self.motion.reduced()) {
            let mut a = Anim::at(0.0);
            a.go(1.0, self.motion.dur(160.0));
            self.tab_drag.home = Some((from, home, a, title));
        }
        self.dirty = true;
    }

    /// The release after an esc belongs to nobody.
    pub(crate) fn tab_drag_swallow(&mut self) -> bool {
        std::mem::take(&mut self.tab_drag.swallow_release)
    }

    /// Keys while a tab is in hand. True: the key was the drag's.
    pub(crate) fn tab_drag_key(&mut self, physical: PhysicalKey, logical: &WKey, state: ElementState) -> bool {
        if self.drag.is_none() {
            return false;
        }
        if state != ElementState::Pressed {
            return true;
        }
        if matches!(logical, WKey::Named(NamedKey::Escape)) {
            self.tab_drag_cancel();
            return true;
        }
        let digit = match physical {
            PhysicalKey::Code(KeyCode::Digit1) => Some(1),
            PhysicalKey::Code(KeyCode::Digit2) => Some(2),
            PhysicalKey::Code(KeyCode::Digit3) => Some(3),
            PhysicalKey::Code(KeyCode::Digit4) => Some(4),
            PhysicalKey::Code(KeyCode::Digit5) => Some(5),
            PhysicalKey::Code(KeyCode::Digit6) => Some(6),
            PhysicalKey::Code(KeyCode::Digit7) => Some(7),
            PhysicalKey::Code(KeyCode::Digit8) => Some(8),
            PhysicalKey::Code(KeyCode::Digit9) => Some(9),
            _ => None,
        };
        let me = u64::from(self.window.id());
        if let Some(n) = digit {
            if let Some(e) = self.windows.get(n - 1) {
                self.tab_drag.send = if e.id == me { None } else { Some(Dest::Window(e.id)) };
            }
        } else if physical == PhysicalKey::Code(KeyCode::KeyN) {
            self.tab_drag.send = Some(Dest::New);
        } else if matches!(physical, PhysicalKey::Code(KeyCode::Digit0 | KeyCode::Backspace)) {
            self.tab_drag.send = None;
        }
        self.dirty = true;
        true
    }

    /// The shelf's cards: every window in the rail's order, then a new one.
    fn shelf_cards(&self) -> Vec<Card> {
        let me = u64::from(self.window.id());
        let (w, _) = self.win_size();
        let pad = self.px(8.0);
        let gap = self.px(6.0);
        let mut entries: Vec<(Dest, String, nus_render::Color, usize, bool)> = if self.windows.is_empty() {
            vec![(Dest::Window(me), self.window_name(), self.container_colour(), self.tabs.len(), true)]
        } else {
            self.windows.iter().map(|e| (Dest::Window(e.id), e.name.clone(), e.colour, e.tabs, e.id == me)).collect()
        };
        entries.push((Dest::New, "new window".into(), self.theme.dim, 0, false));
        let n = entries.len() as f32;
        let cw = ((w - 2.0 * pad - gap * (n - 1.0)) / n).min(self.px(200.0));
        let total = cw * n + gap * (n - 1.0);
        let x0 = ((w - total) / 2.0).round();
        let ch = self.px(SHELF_H) - 2.0 * pad;
        entries
            .into_iter()
            .enumerate()
            .map(|(k, (dest, name, colour, tabs, here))| Card {
                rect: Rect::new(x0 + k as f32 * (cw + gap), pad, cw, ch),
                dest,
                name,
                colour,
                tabs,
                here,
                key: if dest == Dest::New { "N".into() } else if k < 9 { format!("{}", k + 1) } else { String::new() },
            })
            .collect()
    }

    /// The ghost: the row in hand. In the list it keeps to the list and
    /// steps in by the depth it would land at; elsewhere it follows the
    /// pointer, held where it was grabbed.
    fn ghost_rect(&self, d: usize, off: f32, it: Option<&Intent>) -> Rect {
        let (mx, my) = self.mouse;
        let sb = self.sidebar_rect();
        let row_h = self.px(m::ROW_H);
        let w = (sb.w - self.px(8.0)).max(self.px(160.0));
        if self.over_list(mx, my) && self.drag.is_some_and(|dr| dr.0 == d) {
            let depth = match it.map(|i| &i.target) {
                Some(Target::Place { depth, .. }) => *depth,
                _ => self.depth(d),
            };
            let step = self.px(INDENT) * depth as f32;
            return Rect::new(sb.x + self.px(4.0) + step, my + self.px(GHOST_DROP), w - step, row_h);
        }
        let _ = off;
        // Kept inside the window, and under the shelf when it is down.
        let (ww, wh) = self.win_size();
        let gw = w.min(self.px(240.0));
        let x = (mx + self.px(12.0)).min(ww - gw - self.px(6.0)).max(self.px(4.0));
        let floor = if self.shelf_open() { self.px(SHELF_H) + self.px(8.0) } else { 0.0 };
        let y = (my + self.px(GHOST_DROP)).max(floor).min(wh - self.px(BAR_H) - row_h - self.px(6.0));
        Rect::new(x, y, gw, row_h)
    }

    /// Everything a drag shows: lanes and rule or the row it would join,
    /// the page's half or quarter, the shelf, the ghost with its verb, and
    /// the director bar.
    pub(crate) fn draw_tab_drag(&mut self, scene: &mut Scene) {
        let Some((d, off, _)) = self.drag else {
            self.draw_tab_drag_home(scene);
            return;
        };
        let it = self.tab_intent(d);
        let t = self.theme.clone();
        let ink = t.ink;
        let sig = self.surface.signal;
        let hair = self.px(m::HAIRLINE);
        let rule_w = self.px(m::STRUCTURE);
        scene.layer(None);

        // The row in hand is lifted: dimmed where it was (not for a copy).
        if self.sidebar_visible() {
            let g = self.sidebar_geometry();
            let sb = self.sidebar_rect();
            let list = g.list(sb);
            if it.verb != Verb::Copy {
                let mut lifted: Vec<usize> = vec![d];
                lifted.extend(self.subtree(d));
                scene.layer(Some(list));
                for &(i, ry, rh) in g.rows.iter().filter(|r| lifted.contains(&r.0)) {
                    let _ = i;
                    scene.rect(Rect::new(sb.x, ry, sb.w, rh), fade(self.paper(), 0.7));
                }
                scene.layer(None);
            }
        }
        // The page: the half a join takes, or the zone a tile takes.
        let mark = match &it.target {
            Target::Join { preview: Some(p), .. } => Some(*p),
            Target::Tile(dr) => Some(dr.preview),
            _ => None,
        };
        if let Some(p) = mark {
            scene.rect(p, fade(sig, 0.16));
            scene.outline(p, rule_w, sig);
        }
        // The ghost, and its verb.
        let g = self.ghost_rect(d, off, Some(&it));
        scene.rect(Rect::new(g.x + self.px(4.0), g.y + self.px(4.0), g.w, g.h), fade(ink, 0.35));
        scene.rect(g, self.paper());
        scene.outline(g, rule_w, ink);
        let icon_sz = self.px(14.0);
        let kind = match self.tabs.get(d).map(|t| if t.focus_right { t.right.as_ref().unwrap_or(&t.left) } else { &t.left }) {
            Some(Pane::Web(_)) => icons::GLOBE,
            _ => icons::TERMINAL,
        };
        self.fonts.draw_icon(scene, kind, icon_sz, g.x + self.px(10.0), g.y + (g.h - icon_sz) / 2.0, ink);
        let ui = self.ui_strong();
        let title = self.fit(ui, self.tabs[d].title(), g.w - self.px(40.0)).into_owned();
        self.fonts.draw(scene, ui, g.x + self.px(30.0), g.y + (g.h + self.px(m::UI_PX)) / 2.0 - self.px(2.0), &title);
        let chip_style = self.label_strong();
        let verb = it.verb.word().caps();
        let ch = self.px(18.0);
        let icon = it.verb.icon();
        let cw = self.fonts.measure(chip_style, &verb) + self.px(14.0) + if icon.is_some() { self.px(16.0) } else { 0.0 };
        let (w, h) = self.win_size();
        let cx = (g.right() - cw + self.px(10.0)).min(w - cw - self.px(4.0)).max(self.px(4.0));
        // At the ghost's lower corner: above it is what it aims at.
        let chip = Rect::new(cx, g.bottom() - ch / 2.0 + self.px(4.0), cw, ch);
        let (fill, fg) = if !it.ok { (t.dim, self.on_fill(t.dim)) } else if it.verb == Verb::Move { (ink, self.paper()) } else { (sig, self.on_fill(sig)) };
        scene.rect(chip, fill);
        let mut x = chip.x + self.px(7.0);
        if let Some(i) = icon {
            self.fonts.draw_icon(scene, i, self.px(11.0), x, chip.y + (ch - self.px(11.0)) / 2.0, fg);
            x += self.px(16.0);
        }
        self.fonts.draw(scene, Style { color: fg, ..chip_style }, x, chip.y + ch / 2.0 + self.px(4.0), &verb);

        // The marks go over the ghost: where it lands is never hidden.
        if self.sidebar_visible() {
            let g = self.sidebar_geometry();
            let sb = self.sidebar_rect();
            let list = g.list(sb);
            scene.layer(Some(list));
            match &it.target {
                Target::Place { depth, y, .. } => {
                    // The lanes this gap allows, faint; the rule at the chosen one.
                    let pad = self.px(m::ROW_PAD_X);
                    let lx = |k: usize| sb.x + pad + self.px(5.0) + k as f32 * self.px(INDENT);
                    let reach = self.px(m::ROW_H) * 2.0;
                    for k in 0..=(*depth).max(1) {
                        let x = lx(k);
                        let mut yy = (*y - reach).max(list.y);
                        while yy < (*y + reach).min(list.bottom()) {
                            scene.vline(x, yy, self.px(3.0), hair, fade(t.dim, 0.7));
                            yy += self.px(6.0);
                        }
                    }
                    let x = lx(*depth) - self.px(3.0);
                    let r = Rect::new(x, *y - self.px(1.0), sb.right() - self.px(10.0) - x, self.px(2.0));
                    scene.rect(r, sig);
                    scene.rect(Rect::new(x - self.px(3.0), *y - self.px(4.0), self.px(8.0), self.px(8.0)), sig);
                }
                Target::Nest(j) => {
                    // The stack it would join: its row and what is under it.
                    let mut stack = vec![*j];
                    stack.extend(self.subtree(*j));
                    let rows: Vec<(f32, f32)> = g.rows.iter().filter(|r| stack.contains(&r.0)).map(|r| (r.1, r.1 + r.2)).collect();
                    if let (Some(top), Some(bot)) = (rows.iter().map(|r| r.0).reduce(f32::min), rows.iter().map(|r| r.1).reduce(f32::max)) {
                        let r = Rect::new(sb.x + self.px(4.0), top + self.px(1.0), sb.w - self.px(8.0), bot - top - self.px(2.0));
                        scene.rect(r, fade(sig, 0.14));
                        scene.outline(r, rule_w, sig);
                    }
                }
                Target::Join { tab, preview: None, .. } => {
                    if let Some(&(_, ry, rh)) = g.rows.iter().find(|r| r.0 == *tab) {
                        let r = Rect::new(sb.x + self.px(4.0), ry + self.px(1.0), sb.w - self.px(8.0), rh.min(self.px(m::ROW_H)) - self.px(2.0));
                        scene.outline(r, rule_w, sig);
                        // The plug: where the pane would go.
                        let s = self.px(18.0);
                        let p = Rect::new(r.right() - s - self.px(4.0), r.y + (r.h - s) / 2.0, s, s);
                        scene.rect(p, sig);
                        self.fonts.draw_icon(scene, icons::SIDEBAR, self.px(12.0), p.x + self.px(3.0), p.y + self.px(3.0), self.on_fill(sig));
                    }
                }
                _ => {}
            }
            scene.layer(None);
        }

        // The shelf, when it is down.
        if self.shelf_open() {
            let (w, _) = self.win_size();
            let band = Rect::new(0.0, 0.0, w, self.px(SHELF_H));
            scene.rect(Rect::new(0.0, band.bottom(), w, self.px(4.0)), fade(ink, 0.18));
            scene.rect(band, self.paper());
            scene.hline(0.0, band.bottom() - rule_w, w, rule_w, ink);
            let chosen = match it.target {
                Target::Send(dest) => Some(dest),
                _ => None,
            };
            let small = Style { px: self.px(10.0), ..self.label() };
            let strong = self.label_strong();
            for c in self.shelf_cards() {
                let hot = chosen == Some(c.dest);
                let (fill, fg) = if hot { (ink, self.paper()) } else { (self.paper(), if c.here { t.dim } else { ink }) };
                scene.rect(c.rect, fill);
                if c.dest == Dest::New && !hot {
                    // Dashed: a window that isn't there yet.
                    let dash = self.px(4.0);
                    let mut x = c.rect.x;
                    while x < c.rect.right() {
                        let len = dash.min(c.rect.right() - x);
                        scene.hline(x, c.rect.y, len, hair, ink);
                        scene.hline(x, c.rect.bottom() - hair, len, hair, ink);
                        x += dash * 2.0;
                    }
                    let mut y = c.rect.y;
                    while y < c.rect.bottom() {
                        let len = dash.min(c.rect.bottom() - y);
                        scene.vline(c.rect.x, y, len, hair, ink);
                        scene.vline(c.rect.right() - hair, y, len, hair, ink);
                        y += dash * 2.0;
                    }
                } else {
                    scene.outline(c.rect, if hot { rule_w } else { hair }, if c.here { t.dim } else { ink });
                }
                if hot {
                    scene.outline(Rect::new(c.rect.x - self.px(3.0), c.rect.y - self.px(3.0), c.rect.w + self.px(6.0), c.rect.h + self.px(6.0)), rule_w, sig);
                }
                let x = c.rect.x + self.px(10.0);
                let sq = self.px(8.0);
                if c.dest != Dest::New {
                    scene.rect(Rect::new(x, c.rect.y + self.px(9.0), sq, sq), if c.here { fade(c.colour, 0.4) } else { c.colour });
                }
                let name_x = if c.dest == Dest::New { x } else { x + sq + self.px(6.0) };
                let key_w = if c.key.is_empty() { 0.0 } else { self.fonts.measure(small, &c.key) + self.px(10.0) };
                let name = self.fit(Style { color: fg, ..strong }, c.name.caps(), c.rect.right() - name_x - key_w - self.px(8.0)).into_owned();
                self.fonts.draw(scene, Style { color: fg, ..strong }, name_x, c.rect.y + self.px(16.0), &name);
                let sub = if c.here { "this window".to_string() } else if c.dest == Dest::New { "here, on its own".into() } else { format!("{} tabs", c.tabs) };
                self.fonts.draw(scene, Style { color: if hot { fg } else { t.dim }, ..small }, x, c.rect.y + self.px(29.0), &sub);
                if !c.key.is_empty() {
                    let kw = self.fonts.measure(small, &c.key) + self.px(8.0);
                    let kr = Rect::new(c.rect.right() - kw - self.px(6.0), c.rect.y + self.px(5.0), kw, self.px(15.0));
                    scene.outline(kr, hair, fg);
                    self.fonts.draw(scene, Style { color: fg, ..small }, kr.x + self.px(4.0), kr.y + self.px(11.0), &c.key);
                }
            }
        }

        // The director bar: every verb and its key, the live one lit, and
        // what letting go does.
        let bar = Rect::new(0.0, h - self.px(BAR_H), w, self.px(BAR_H));
        scene.rect(bar, ink);
        let paper = self.paper();
        let label = Style { color: paper, ..self.label() };
        let lit = Style { color: self.on_fill(sig), ..self.label_strong() };
        let place = match it.verb {
            Verb::Stack | Verb::Unstack | Verb::Tile => it.verb,
            _ if !self.over_list(self.mouse.0, self.mouse.1) && self.tab_drag.send.is_none() => Verb::Tile,
            _ => Verb::Move,
        };
        let slots: [(&str, Verb); 5] = [("", place), ("shift", Verb::Join), ("ctrl", Verb::Copy), ("1–9", Verb::Send), ("esc", Verb::Move)];
        let mut x = bar.x;
        let base = bar.y + bar.h / 2.0 + self.px(4.0);
        for (k, (key, verb)) in slots.iter().enumerate() {
            let word = if k == 4 { "cancel".caps() } else { verb.word().caps() };
            let on = k < 4 && it.verb == *verb;
            let kw = if key.is_empty() { 0.0 } else { self.fonts.measure(label, key) + self.px(14.0) };
            let sw = kw + self.fonts.measure(if on { lit } else { label }, &word) + self.px(22.0);
            let cell = Rect::new(x, bar.y, sw, bar.h);
            if on {
                scene.rect(cell, if it.ok { sig } else { t.dim });
            }
            let style = if on { lit } else { label };
            let mut tx = x + self.px(11.0);
            if !key.is_empty() {
                let kr = Rect::new(tx, bar.y + self.px(6.0), kw - self.px(6.0), bar.h - self.px(12.0));
                scene.outline(kr, hair, style.color);
                self.fonts.draw(scene, style, kr.x + self.px(4.0), base, key);
                tx += kw;
            }
            self.fonts.draw(scene, style, tx, base, &word);
            x += sw;
            scene.vline(x, bar.y, bar.h, hair, fade(paper, 0.25));
        }
        let words = format!("{} · {}", it.verb.word(), it.words);
        let ws = Style { color: if it.ok { paper } else { fade(paper, 0.6) }, ..self.ui() };
        let room = (bar.right() - x - self.px(28.0)).max(0.0);
        let words = self.fit(ws, words, room).into_owned();
        let ww = self.fonts.measure(ws, &words);
        self.fonts.draw(scene, ws, bar.right() - ww - self.px(14.0), base, &words);
        self.dirty = true;
    }

    /// After esc: the ghost's slide home.
    fn draw_tab_drag_home(&mut self, scene: &mut Scene) {
        let Some((from, to, a, title)) = self.tab_drag.home.clone() else { return };
        if !a.active() {
            self.tab_drag.home = None;
            return;
        }
        let v = a.value();
        let r = Rect::new(from.x + (to.x - from.x) * v, from.y + (to.y - from.y) * v, from.w + (to.w - from.w) * v, from.h + (to.h - from.h) * v);
        let ink = self.theme.ink;
        scene.layer(None);
        scene.rect(r, fade(self.paper(), 1.0 - v * 0.6));
        scene.outline(r, self.px(m::STRUCTURE), fade(ink, 1.0 - v));
        let ui = Style { color: fade(ink, 1.0 - v), ..self.ui_strong() };
        let title = self.fit(ui, title, r.w - self.px(40.0)).into_owned();
        self.fonts.draw(scene, ui, r.x + self.px(30.0), r.y + (r.h + self.px(m::UI_PX)) / 2.0 - self.px(2.0), &title);
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_cut_to_fit_a_sentence() {
        assert_eq!(short("cargo watch"), "cargo watch");
        assert_eq!(short("a very long page title that goes on"), "a very long page titl…");
    }

    // Rows: 0 a (top), 1 b (under a), 2 c (under a), 3 d (top).
    fn up(i: usize, level: usize) -> usize {
        let depth = [0, 1, 1, 0];
        let parent = [0, 0, 0, 3];
        let mut cur = i;
        while depth[cur] > level {
            cur = parent[cur];
        }
        cur
    }

    #[test]
    fn a_gap_inside_a_stack_stays_in_it() {
        // Between b and c, asking for the top level: still under a, before c.
        assert_eq!(gap_place(Some((1, 1)), Some((2, 1)), 0, up), (1, Some(0), Some(2)));
    }

    #[test]
    fn after_a_stack_the_travel_across_decides() {
        // After c, before d: left is the top level, before d...
        assert_eq!(gap_place(Some((2, 1)), Some((3, 0)), 0, up), (0, None, Some(3)));
        // ...one lane in is a's stack, at its end...
        assert_eq!(gap_place(Some((2, 1)), Some((3, 0)), 1, up), (1, Some(0), None));
        // ...and two is a new stack under c.
        assert_eq!(gap_place(Some((2, 1)), Some((3, 0)), 2, up), (2, Some(2), None));
    }

    #[test]
    fn the_ends_of_the_list() {
        // Above everything: the top level, first.
        assert_eq!(gap_place(None, Some((0, 0)), 3, up), (0, None, Some(0)));
        // Below everything, one lane in: d's stack.
        assert_eq!(gap_place(Some((3, 0)), None, 1, up), (1, Some(3), None));
        assert_eq!(gap_place(Some((3, 0)), None, 0, up), (0, None, None));
    }

    #[test]
    fn every_verb_has_a_word() {
        for v in [Verb::Move, Verb::Stack, Verb::Unstack, Verb::Tile, Verb::Join, Verb::Copy, Verb::Send] {
            assert!(!v.word().is_empty());
        }
    }
}
