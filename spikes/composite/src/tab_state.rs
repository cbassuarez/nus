//! What's inside a tab, at a glance: the Ledger layout of a tab row.
//!
//! A row has two lines and a cell. The title, then under it where the tab
//! is (its folder and branch, its host, its site); and at the row's end a
//! cell of fixed width and place that says what the tab is doing now, so
//! the eye reads the list's state down one column:
//!
//!   1:42 ▬▬▬    running, and the bar when the shell reports progress (OSC 9;4)
//!   :5173       listening (ports.rs knows which tab's shell owns the port)
//!   WAITS       for you: an assistant's question, a bell while running.
//!               The one cell filled with signal.
//!   EXIT 1      failed while you were elsewhere: hatched, not colour alone
//!   DONE        finished while you were elsewhere: outlined until seen
//!   ▶           a page playing; … loading; ● an editor with unsaved edits
//!   🔔 2        a page told you something (notices.rs): filled with signal,
//!               and its words are the second line until the tab is in front
//!   12          a page's count (its badge or its title); ■ when it only says "new"
//!   ⚠ 9         a listening page went to a sign-in: its last count, dim
//!   idle        a shell at its prompt, dim
//!
//! "Unseen" is the shell's own `waiting`: set when a command ends or rings
//! while the tab isn't in front, cleared when it comes to the front.
//! A folded stack rolls up: its cell is the loudest of its members ("2
//! WAITS"), and its second line is a spine, one tick per member.
//! In the 48 px column the cell becomes an 8 px mark on the icon's corner,
//! keeping its shape. SIDEBAR · TAB ROWS · ONE LINE turns all of it off.

use crate::app::{fade, App, Pane, Tab};
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Instance, Rect, Scene, Style};
use std::time::Instant;

/// The cell's width, logical px: one column for every row.
pub const CELL_W: f32 = 58.0;
/// The second line's height, logical px.
pub const SUB_H: f32 = 13.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Waits,
    Failed(i32),
    Running { since: Instant, progress: Option<(u8, u8)> },
    Listening(u16),
    Working,
    Done,
    Playing,
    Loading,
    Edited,
    /// A page told you something you haven't seen: how many.
    Says(u32),
    /// A page's count (its badge, its title, a source's).
    Count(u32),
    /// A page says there is something new, without a number.
    New,
    /// A listening page went to a sign-in; its last count.
    Stale(u32),
    Idle,
    /// Nothing to say: a page at rest, settings, home.
    Quiet,
}

impl Cell {
    /// The ladder: waits, failed, busy, done, playing, the rest.
    pub fn rank(&self) -> u8 {
        match self {
            Cell::Waits | Cell::Says(_) => 8,
            Cell::Failed(_) => 7,
            Cell::Running { .. } | Cell::Listening(_) | Cell::Working => 6,
            Cell::Done | Cell::Count(_) | Cell::New => 5,
            Cell::Playing => 4,
            Cell::Loading | Cell::Edited | Cell::Stale(_) => 3,
            Cell::Idle => 2,
            Cell::Quiet => 0,
        }
    }

    /// The kind, for counting a stack's members alike.
    fn kind(&self) -> u8 {
        match self {
            Cell::Running { .. } | Cell::Listening(_) | Cell::Working => 6,
            Cell::Says(_) => 18,
            Cell::Count(_) | Cell::New => 15,
            Cell::Stale(_) => 13,
            c => c.rank(),
        }
    }

    /// Words for a stack's roll-up and for the row's accessible name.
    pub fn words(&self, n: usize) -> String {
        let one = match self {
            Cell::Waits => "waits",
            Cell::Failed(_) => "failed",
            Cell::Running { .. } | Cell::Working => "running",
            Cell::Listening(_) => "listening",
            Cell::Done => "done",
            Cell::Playing => "playing",
            Cell::Loading => "loading",
            Cell::Edited => "edited",
            Cell::Says(_) => "told you",
            Cell::Count(_) | Cell::New => "unread",
            Cell::Stale(_) => "stale",
            Cell::Idle => "idle",
            Cell::Quiet => "",
        };
        // Busy members count as one kind: "3 running", whatever each does.
        let one = if n > 1 && self.kind() == 6 { "running" } else { one };
        if n > 1 { format!("{n} {one}") } else { one.to_string() }
    }
}

/// A listening page that went to a sign-in: the count it last had there.
pub fn stale(s: &crate::browser::Shared) -> Option<u32> {
    let (host, n, _) = s.notices.counted.as_ref()?;
    (*host != crate::sites::host_of(&s.url) && crate::notices::sign_in(&s.url) && crate::sites::prefs(host).listen).then_some(*n)
}

/// A count as a cell has room for it.
pub fn short_count(n: u32) -> String {
    if n > 999 { "999+".into() } else { n.to_string() }
}

/// `m:ss` under an hour, `h:mm` after.
pub fn clock(secs: u64) -> String {
    if secs < 3600 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}h{:02}", secs / 3600, (secs % 3600) / 60)
    }
}

/// A folder as a row has room for it: home as ~, then the last two parts.
pub fn short_dir(dir: &str, home: Option<&str>) -> String {
    let dir = dir.trim_end_matches('/');
    let dir = match home.filter(|h| !h.is_empty() && dir.starts_with(*h)) {
        Some(h) => format!("~{}", &dir[h.len()..]),
        None => dir.to_string(),
    };
    let parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 2 || dir == "~" {
        return if dir.is_empty() { "/".into() } else { dir };
    }
    format!("…/{}", parts[parts.len() - 2..].join("/"))
}

/// The second line, in parts the row draws in order.
pub enum Bit {
    Text(String),
    /// What a page just told you, until you look: in ink.
    News(String),
    Branch(String, bool),
    Spine(Vec<u8>),
}

impl App {
    pub(crate) fn tab_state_on(&self) -> bool {
        self.behavior.tab_state
    }

    /// The second line's height for a row (0 when rows are one line).
    pub(crate) fn sub_h(&self) -> f32 {
        if self.tab_state_on() && !self.sidebar_icons() { self.px(SUB_H) } else { 0.0 }
    }

    fn pane_cell(&self, p: &Pane, tab_id: u64) -> Cell {
        match p {
            Pane::Term(t) => {
                let agent = t.agent.as_ref().map(|a| a.phase);
                if agent == Some(crate::agent::Phase::Waiting) {
                    return Cell::Waits;
                }
                let port = self.board.rows.iter().filter(|r| r.tab == Some(tab_id) && r.dying.is_none()).map(|r| r.port).min();
                if let Some(since) = t.running_since {
                    if t.waiting {
                        return Cell::Waits;
                    }
                    return match port {
                        Some(p) => Cell::Listening(p),
                        None => Cell::Running { since, progress: t.progress },
                    };
                }
                if agent == Some(crate::agent::Phase::Working) {
                    return Cell::Working;
                }
                if t.waiting {
                    return match t.last_exit {
                        Some(code) if code != 0 => Cell::Failed(code),
                        _ => Cell::Done,
                    };
                }
                match port {
                    Some(p) => Cell::Listening(p),
                    None => Cell::Idle,
                }
            }
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                if s.notices.unseen > 0 {
                    return Cell::Says(s.notices.unseen);
                }
                let mode = crate::notices::mode_for(&s.url);
                match mode.counts().then(|| s.notices.count(&s.title)).flatten() {
                    Some(crate::notices::Count::N(n)) if n > 0 => return Cell::Count(n),
                    Some(crate::notices::Count::Dot) => return Cell::New,
                    _ => {}
                }
                if s.media_playing {
                    Cell::Playing
                } else if s.loading {
                    Cell::Loading
                } else if let Some(n) = stale(&s) {
                    Cell::Stale(n)
                } else {
                    Cell::Quiet
                }
            }
            Pane::Editor(e) if e.buf().is_some_and(|b| b.dirty) => Cell::Edited,
            _ => Cell::Quiet,
        }
    }

    /// A tab's cell: the louder of its panes.
    pub(crate) fn tab_cell(&self, tab: &Tab) -> Cell {
        let a = self.pane_cell(&tab.left, tab.id);
        match tab.right.as_ref().map(|p| self.pane_cell(p, tab.id)) {
            Some(b) if b.rank() > a.rank() => b,
            _ => a,
        }
    }

    /// A folded stack's cell: its loudest member's, and how many share it.
    pub(crate) fn stack_cell(&self, tabs: &[Tab], members: &[usize]) -> (Cell, usize) {
        let cells: Vec<Cell> = members.iter().map(|&k| self.tab_cell(&tabs[k])).collect();
        let top = cells.iter().max_by_key(|c| c.rank()).cloned().unwrap_or(Cell::Quiet);
        let n = cells.iter().filter(|c| c.kind() == top.kind()).count();
        (top, n)
    }

    /// The second line: where the tab is. A folded stack's is its size
    /// and its spine: a tick per member, 2 for the one last in front,
    /// 1 for one that needs you, 0 for the rest.
    pub(crate) fn tab_where(&self, tabs: &[Tab], i: usize, members: Option<&[usize]>) -> Vec<Bit> {
        let tab = &tabs[i];
        let home = std::env::var("HOME").ok();
        if let Some(members) = members {
            let pages = members.iter().all(|&k| matches!(tabs[k].left, Pane::Web(_)));
            let last = members.iter().copied().min_by_key(|k| self.mru.iter().position(|m| m == k).unwrap_or(usize::MAX));
            let ticks = members.iter().take(9).map(|&k| {
                let c = self.tab_cell(&tabs[k]);
                if matches!(c, Cell::Waits | Cell::Failed(_)) { 1 } else if Some(k) == last { 2 } else { 0 }
            }).collect();
            let n = members.len();
            return vec![Bit::Text(format!("{n} {}", if pages { "pages" } else { "tabs" })), Bit::Spine(ticks)];
        }
        let (main, other) = tab.panes();
        let mut bits = Vec::new();
        match main {
            Pane::Term(t) => {
                if let Some(host) = t.tunnel() {
                    bits.push(Bit::Text(format!("ssh · {host}")));
                } else if let Some(dir) = t.cwd.as_deref() {
                    bits.push(Bit::Text(short_dir(dir, home.as_deref())));
                    if let Some(g) = t.git() {
                        bits.push(Bit::Branch(g.branch.clone(), g.dirty() > 0));
                    }
                }
            }
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                let host = crate::sites::host_of(&s.url);
                match s.notices.latest.as_ref().filter(|_| s.notices.unseen > 0) {
                    Some(n) => bits.push(Bit::News(n.line())),
                    None if !host.is_empty() => bits.push(Bit::Text(host)),
                    None => {}
                }
            }
            Pane::Editor(e) => {
                if let Some(dir) = e.buf().and_then(|b| b.path.as_ref()).and_then(|p| p.parent()) {
                    bits.push(Bit::Text(short_dir(&dir.display().to_string(), home.as_deref())));
                }
            }
            _ => {}
        }
        if let Some(o) = other {
            let name = match o {
                Pane::Term(t) => t.title.clone(),
                Pane::Web(w) => crate::sites::host_of(&w.tab.shared.borrow().url),
                Pane::Editor(e) => e.title(),
                _ => String::new(),
            };
            if !name.is_empty() {
                bits.push(Bit::Text(format!("+ {name}")));
            }
        }
        bits
    }

    /// The cell at the row's end: `right` is the row's right edge, `cy`
    /// the title line's middle. Returns the cell's left edge.
    pub(crate) fn draw_cell(&mut self, scene: &mut Scene, cell: &Cell, n: usize, right: f32, cy: f32, active: bool) -> f32 {
        let t = self.theme.clone();
        let ink = t.ink;
        let sig = self.surface.signal;
        let w = self.px(CELL_W);
        let h = self.px(17.0);
        let r = Rect::new(right - w, (cy - h / 2.0).round(), w, h);
        let label = self.label();
        let strong = self.label_strong();
        let base = r.y + h / 2.0 + self.px(4.0);
        let dimc = if active { fade(ink, 0.7) } else { t.dim };
        // Words, right-aligned in the cell; centred when the cell is filled.
        // Where the cell's ink begins: words longer than the cell reach left.
        let left = std::cell::Cell::new(r.x);
        let say = |app: &mut App, scene: &mut Scene, style: Style, text: &str, centre: bool, icon: Option<(&'static str, &'static str)>| {
            let isz = app.px(10.0);
            let tw = app.fonts.measure(style, text) + if icon.is_some() { isz + app.px(4.0) } else { 0.0 };
            let x = if centre { r.x + ((r.w - tw) / 2.0).round() } else { r.right() - tw - app.px(4.0) };
            left.set(left.get().min(x));
            let mut x2 = x;
            if let Some(i) = icon {
                app.fonts.draw_icon(scene, i, isz, x, r.y + (h - isz) / 2.0, style.color);
                x2 += isz + app.px(4.0);
            }
            app.fonts.draw(scene, style, x2, base, text);
        };
        match cell {
            Cell::Waits => {
                scene.rect(r, sig);
                say(self, scene, Style { color: self.on_fill(sig), ..strong }, &self.cell_caps(&cell.words(n)), true, None);
            }
            Cell::Failed(code) => {
                scene.push(Instance::hazard(r, self.px(2.0), fade(sig, 0.85), [0.0, 0.0, 0.0, 0.0], self.px(6.0)));
                let words = if n > 1 { cell.words(n) } else { format!("exit {code}") };
                let text = self.cell_caps(&words);
                let tw = self.fonts.measure(strong, &text) + self.px(8.0);
                // The words sit on a patch of paper, so the stripes never cut them.
                let patch = Rect::new(r.x + ((r.w - tw) / 2.0).round(), r.y + self.px(3.0), tw, h - self.px(6.0));
                scene.rect(patch, self.paper());
                say(self, scene, Style { color: sig, ..strong }, &text, true, None);
            }
            Cell::Done => {
                scene.outline(r, self.px(m::HAIRLINE), ink);
                say(self, scene, Style { color: ink, ..strong }, &self.cell_caps(&cell.words(n)), true, None);
            }
            Cell::Running { since, progress } => {
                if n > 1 {
                    say(self, scene, Style { color: ink, ..label }, &self.cell_caps(&cell.words(n)), false, None);
                } else {
                    // A known share says more than a clock; otherwise the clock.
                    let text = match progress {
                        Some((1, pct)) if self.behavior.progress_sidebar => format!("{pct}%"),
                        _ => clock(crate::clock::since(*since).as_secs()),
                    };
                    say(self, scene, Style { color: ink, ..label }, &text, false, None);
                    if let (Some((state, pct)), true) = (progress, self.behavior.progress_sidebar) {
                        // The bar ends where the clock begins.
                        let (v, color) = self.progress_look(*state, *pct);
                        let tw = self.fonts.measure(label, &text);
                        let bw = (r.w - tw - self.px(10.0)).max(self.px(12.0));
                        let br = Rect::new(r.x, r.y + h / 2.0 - self.px(1.5), bw, self.px(3.0));
                        scene.rect(br, fade(ink, 0.18));
                        scene.rect(Rect::new(br.x, br.y, br.w * v, br.h), color);
                    }
                }
            }
            Cell::Working => say(self, scene, Style { color: ink, ..label }, &self.cell_caps(&cell.words(n)), false, None),
            Cell::Listening(port) => {
                let text = if n > 1 { cell.words(n) } else { format!(":{port}") };
                say(self, scene, Style { color: ink, ..label }, &text, false, Some(icons::PORTS));
            }
            Cell::Playing => say(self, scene, Style { color: ink, ..label }, &self.cell_caps("plays"), false, Some(icons::SPEAKER)),
            Cell::Loading => say(self, scene, Style { color: dimc, ..label }, "…", false, None),
            Cell::Edited => say(self, scene, Style { color: ink, ..label }, &self.cell_caps("edited"), false, Some(icons::PENCIL)),
            Cell::Says(k) => {
                scene.rect(r, sig);
                let style = Style { color: self.on_fill(sig), ..strong };
                if n > 1 {
                    say(self, scene, style, &self.cell_caps(&cell.words(n)), true, None);
                } else {
                    say(self, scene, style, &k.to_string(), true, Some(icons::BELL));
                }
            }
            Cell::Count(k) => {
                let text = if n > 1 { self.cell_caps(&cell.words(n)) } else { short_count(*k) };
                say(self, scene, Style { color: ink, ..strong }, &text, false, None);
            }
            Cell::New => {
                let d = self.px(6.0);
                let sq = Rect::new(r.right() - d - self.px(4.0), r.y + (h - d) / 2.0, d, d);
                scene.rect(sq, ink);
                left.set(left.get().min(sq.x));
            }
            Cell::Stale(k) => {
                scene.outline(r, self.px(m::HAIRLINE), fade(ink, 0.35));
                say(self, scene, Style { color: dimc, ..label }, &short_count(*k), true, Some(icons::WARNING));
            }
            Cell::Idle => say(self, scene, Style { color: dimc, ..label }, "idle", false, None),
            Cell::Quiet => {}
        }
        // Running cells keep the clock ticking.
        if matches!(cell, Cell::Running { .. } | Cell::Loading) {
            self.dirty = true;
        }
        left.get()
    }

    fn cell_caps(&self, s: &str) -> String {
        s.to_uppercase()
    }

    /// The second line, from `x` at baseline `base`, no further than `right`.
    pub(crate) fn draw_where(&mut self, scene: &mut Scene, bits: &[Bit], x: f32, base: f32, right: f32, active: bool) {
        let t = self.theme.clone();
        let small = Style { px: self.px(10.5), color: if active { fade(t.ink, 0.72) } else { t.dim }, ..self.ui() };
        let mut x = x;
        let gap = self.px(6.0);
        for bit in bits {
            if x >= right - self.px(8.0) {
                break;
            }
            match bit {
                Bit::Text(s) => {
                    let s = self.fit(small, s.as_str(), right - x).into_owned();
                    x += self.fonts.draw(scene, small, x, base, &s) + gap;
                }
                Bit::News(s) => {
                    let ink = Style { color: t.ink, ..small };
                    let s = self.fit(ink, s.as_str(), right - x).into_owned();
                    x += self.fonts.draw(scene, ink, x, base, &s) + gap;
                }
                Bit::Branch(name, dirty) => {
                    let isz = self.px(10.0);
                    if x + isz + self.px(20.0) > right {
                        break;
                    }
                    self.fonts.draw_icon(scene, icons::GIT_BRANCH, isz, x, base - isz + self.px(1.0), small.color);
                    x += isz + self.px(3.0);
                    let room = right - x - if *dirty { self.px(10.0) } else { 0.0 };
                    let s = self.fit(small, name.as_str(), room).into_owned();
                    x += self.fonts.draw(scene, small, x, base, &s) + self.px(4.0);
                    if *dirty {
                        // Uncommitted work: a small signal square.
                        let d = self.px(5.0);
                        scene.rect(Rect::new(x, base - self.px(6.0), d, d), self.surface.signal);
                        x += d + gap;
                    }
                }
                Bit::Spine(ticks) => {
                    let tw = self.px(5.0);
                    let th = self.px(9.0);
                    for &k in ticks {
                        if x + tw > right {
                            break;
                        }
                        let c = match k {
                            1 => self.surface.signal,
                            2 => t.ink,
                            _ => fade(t.ink, 0.3),
                        };
                        scene.rect(Rect::new(x, base - th + self.px(1.0), tw, th), c);
                        x += tw + self.px(2.0);
                    }
                    x += gap;
                }
            }
        }
    }

    /// The 48 px column's corner mark: the cell's shape, 8 px.
    pub(crate) fn draw_corner(&mut self, scene: &mut Scene, cell: &Cell, at: Rect) {
        let ink = self.theme.ink;
        let sig = self.surface.signal;
        let d = self.px(8.0);
        let r = Rect::new(at.right() - d, at.bottom() - d, d, d);
        let ring = Rect::new(r.x - self.px(1.5), r.y - self.px(1.5), d + self.px(3.0), d + self.px(3.0));
        match cell {
            Cell::Waits | Cell::Says(_) => {
                scene.rect(ring, self.paper());
                scene.rect(r, sig);
            }
            Cell::Count(_) | Cell::New => {
                scene.rect(ring, self.paper());
                scene.rect(r, ink);
            }
            Cell::Stale(_) => {
                scene.rect(ring, self.paper());
                scene.outline(r, self.px(1.0), fade(ink, 0.45));
            }
            Cell::Failed(_) => {
                scene.rect(ring, self.paper());
                scene.push(Instance::hazard(r, self.px(1.5), sig, [0.0, 0.0, 0.0, 0.0], self.px(3.0)));
                scene.outline(r, self.px(1.0), sig);
            }
            Cell::Done | Cell::Edited => {
                scene.rect(ring, self.paper());
                scene.outline(r, self.px(1.0), ink);
            }
            Cell::Running { .. } | Cell::Listening(_) | Cell::Working | Cell::Playing => {
                scene.rect(ring, self.paper());
                scene.rect(r, ink);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_puts_you_first() {
        let run = Cell::Running { since: Instant::now(), progress: None };
        let mut cells = vec![Cell::Idle, run.clone(), Cell::Done, Cell::Waits, Cell::Failed(1), Cell::Playing, Cell::Quiet];
        cells.sort_by_key(|c| std::cmp::Reverse(c.rank()));
        assert_eq!(cells[0], Cell::Waits);
        assert_eq!(cells[1], Cell::Failed(1));
        assert_eq!(cells[2], run);
        assert_eq!(cells.last(), Some(&Cell::Quiet));
    }

    #[test]
    fn a_page_that_told_you_ranks_with_a_shell_that_waits() {
        assert_eq!(Cell::Says(2).rank(), Cell::Waits.rank());
        assert!(Cell::Count(12).rank() > Cell::Playing.rank());
        assert!(Cell::Count(12).rank() < Cell::Running { since: Instant::now(), progress: None }.rank());
        assert_ne!(Cell::Count(3).kind(), Cell::Done.kind(), "counts and finished commands roll up apart");
        assert_eq!(short_count(12_000), "999+");
    }

    #[test]
    fn busy_cells_count_together() {
        assert_eq!(Cell::Listening(5173).kind(), Cell::Working.kind());
        assert_eq!(Cell::Waits.words(2), "2 waits");
        assert_eq!(Cell::Waits.words(1), "waits");
    }

    #[test]
    fn clocks_and_folders_fit_a_cell() {
        assert_eq!(clock(102), "1:42");
        assert_eq!(clock(3 * 3600 + 5 * 60), "3h05");
        assert_eq!(short_dir("/home/seb/nus", Some("/home/seb")), "~/nus");
        assert_eq!(short_dir("/home/seb/nus/spikes/composite", Some("/home/seb")), "…/spikes/composite");
        assert_eq!(short_dir("/home/seb", Some("/home/seb")), "~");
        assert_eq!(short_dir("/etc", Some("/home/seb")), "/etc");
    }
}
