//! Find: one bar on every surface, and a ladder that widens it.
//!
//! Ctrl+F (⌘F) opens the bar on the pane in front: Ctrl+Shift+F in a
//! shell, where Ctrl+F is the shell's. In the bar, Ctrl+F again climbs the
//! ladder — this pane, this tab, this window — and Ctrl+Shift+F comes back
//! down. ↵ and ⇧↵ walk the matches, crossing panes and tabs at the wider
//! rungs; Esc closes, leaving a page's current match selected.
//!
//! Every surface answers the same questions — how many, which is current,
//! is that the final count, where on screen — and nothing is shown as known
//! before it is. A page is searched by Chromium itself (browser.rs), which
//! highlights, counts across frames and scrolls to the match; nus keeps its
//! answers honest (stale ones dropped, partial ones marked, a navigation
//! noticed). A shell is searched here: on logical lines, so a word a wrap
//! split is still found; history newest first, a slice per frame; what is
//! on screen again as it changes, so new output joins the count without
//! moving the current match.

use std::time::{Duration, Instant};

use nus_vt::{Found, Needle, Term};

/// Past this many matches a count stops and says so.
pub const CAP: usize = 10_000;
/// History rows searched per slice, between checks of the frame budget.
const SLICE: u64 = 2_000;
/// How often what is on screen is searched again while the bar is open.
const SCREEN_EVERY: Duration = Duration::from_millis(100);
/// Rows of new output one screen pass will take before it says it is behind.
const SCREEN_MOST: u64 = 20_000;

/// What one pane knows about its matches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub total: usize,
    /// The current match, counted from the top (0-based).
    pub current: Option<usize>,
    /// The count is final.
    pub done: bool,
    pub capped: bool,
    /// Matches that arrived since the bar opened, not yet walked to.
    pub fresh: usize,
    /// Something to say instead of, or beside, the count.
    pub note: Option<&'static str>,
    /// What is wrong with a pattern, in a line.
    pub error: Option<String>,
}

/// Where a walk ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Moved,
    /// The next match is back at the other end.
    Wrapped,
    /// Nothing to walk to.
    Empty,
}

/// A shell's search. Matches are kept in order, top to bottom; the current
/// one is held by where it is, not by its index, so matches arriving above
/// or below it never move it.
pub struct TermFind {
    pub query: String,
    pub case: bool,
    word: bool,
    regex: bool,
    error: Option<String>,
    needle: Needle,
    pub hits: Vec<Found>,
    pub current: Option<Found>,
    /// Lines below this are settled history: searched once, never again.
    stable: u64,
    /// History is searched downward from here, toward the oldest line.
    lo: u64,
    history_done: bool,
    /// New output came faster than one pass takes.
    behind: bool,
    capped: bool,
    alt: bool,
    /// Matches at or below this line arrived after the bar opened.
    fresh_from: Option<u64>,
    /// The newest line walked to.
    seen: u64,
    screen_at: Option<Instant>,
    /// The user has walked: the first match no longer picks itself.
    walked: bool,
    /// When the current match last moved: its ring closes in from then.
    pub moved_at: Option<Instant>,
}

impl TermFind {
    pub fn new(query: &str, case: bool, term: &Term) -> TermFind {
        TermFind::with(query, case, false, false, term)
    }

    /// With whole words or a regular expression. A pattern that does not
    /// parse searches nothing and says why.
    pub fn with(query: &str, case: bool, word: bool, regex: bool, term: &Term) -> TermFind {
        let g = term.grid();
        let start = g.logical_start(g.abs_row(0));
        let (needle, error) = match Needle::with(query, case, word, regex) {
            Ok(n) => (n, None),
            Err(e) => (Needle::new("", case), Some(e)),
        };
        TermFind {
            query: query.to_string(),
            case,
            word,
            regex,
            error,
            needle,
            hits: Vec::new(),
            current: None,
            stable: start,
            lo: start,
            history_done: start <= g.oldest_abs(),
            behind: false,
            capped: false,
            alt: term.on_alt_screen(),
            fresh_from: None,
            seen: 0,
            screen_at: None,
            walked: false,
            moved_at: None,
        }
    }

    /// Search a little more: the screen when it is due, then history for
    /// as long as `budget` allows. True when anything changed.
    pub fn tick(&mut self, term: &Term, budget: Duration) -> bool {
        if term.on_alt_screen() != self.alt {
            // A full-screen program came or went: a different text.
            *self = TermFind::with(&self.query.clone(), self.case, self.word, self.regex, term);
        }
        let start = Instant::now();
        let g = term.grid();
        let mut changed = false;
        let oldest = g.oldest_abs();
        if self.hits.first().is_some_and(|h| h.end_line < oldest) {
            self.hits.retain(|h| h.end_line >= oldest);
            changed = true;
        }
        if self.screen_at.is_none_or(|t| t.elapsed() >= SCREEN_EVERY) {
            self.screen_at = Some(Instant::now());
            let boundary = g.logical_start(g.abs_row(0));
            let end = g.end_abs();
            let to = end.min(self.stable + SCREEN_MOST);
            let before = self.hits.len();
            let newest_before = self.hits.last().copied();
            self.hits.retain(|h| h.line < self.stable);
            let (found, _) = g.find_in(&self.needle, self.stable, to);
            self.hits.extend(found);
            self.behind = to < end;
            // Whatever has scrolled into history is settled now.
            self.stable = self.stable.max(if self.behind { g.logical_start(to) } else { boundary });
            if self.fresh_from.is_none() {
                self.fresh_from = Some(end);
            }
            if self.hits.len() != before || self.hits.last().copied() != newest_before {
                changed = true;
            }
            if self.hits.len() > CAP {
                let drop = self.hits.len() - CAP;
                self.hits.drain(..drop);
                self.capped = true;
                self.history_done = true;
            }
        }
        while !self.history_done && start.elapsed() < budget {
            if self.lo <= oldest {
                self.history_done = true;
                changed = true;
                break;
            }
            let from = g.logical_start(self.lo.saturating_sub(SLICE).max(oldest));
            let (found, _) = g.find_in(&self.needle, from, self.lo);
            if !found.is_empty() {
                let mut all = found;
                all.extend(self.hits.drain(..));
                self.hits = all;
                changed = true;
            }
            self.lo = from;
            if self.hits.len() >= CAP {
                self.capped = true;
                self.history_done = true;
            }
        }
        if !self.walked && self.current.is_none() {
            if let Some(&newest) = self.hits.last() {
                self.moved_at = Some(crate::clock::now());
                self.current = Some(newest);
                self.seen = newest.line;
                changed = true;
            }
        }
        changed
    }

    /// Where the current match is in the list, or where it would be.
    pub fn index(&self) -> Option<usize> {
        let cur = self.current?;
        if self.hits.is_empty() {
            return None;
        }
        Some(match self.hits.binary_search(&cur) {
            Ok(i) => i,
            Err(i) => i.min(self.hits.len() - 1),
        })
    }

    pub fn status(&self) -> Status {
        let fresh = self.fresh_from.map_or(0, |f| self.hits.iter().rev().take_while(|h| h.line >= f && h.line > self.seen).count());
        Status {
            total: self.hits.len(),
            current: self.index(),
            done: self.error.is_some() || (self.history_done && !self.behind),
            capped: self.capped,
            error: self.error.clone(),
            fresh,
            note: if self.alt {
                Some("screen only")
            } else if self.behind {
                Some("catching up")
            } else {
                None
            },
        }
    }

    /// Walk one match. `older` goes up the history, the way a shell is
    /// read back from its prompt. At an end, nothing moves: the caller
    /// decides whether to wrap or to go on to the next pane.
    pub fn step(&mut self, older: bool) -> Step {
        let Some(i) = self.index() else { return Step::Empty };
        let cur = self.current.unwrap_or(self.hits[i]);
        // A current match that has gone (the screen changed) counts as
        // standing between its neighbours.
        let exact = self.hits.get(i) == Some(&cur);
        let next = if older {
            if exact || self.hits[i] > cur {
                i.checked_sub(1)
            } else {
                Some(i)
            }
        } else if exact || self.hits[i] < cur {
            (i + 1 < self.hits.len()).then_some(i + 1)
        } else {
            Some(i)
        };
        match next {
            Some(n) => {
                self.go(n);
                Step::Moved
            }
            None => Step::Wrapped,
        }
    }

    /// Stand on the first match walking `older` would reach from outside:
    /// the newest when going older, the oldest when going newer.
    pub fn enter(&mut self, older: bool) -> Step {
        if self.hits.is_empty() {
            return Step::Empty;
        }
        self.go(if older { self.hits.len() - 1 } else { 0 });
        Step::Moved
    }

    fn go(&mut self, i: usize) {
        self.walked = true;
        self.current = Some(self.hits[i]);
        self.seen = self.seen.max(self.hits[i].line);
        self.moved_at = Some(crate::clock::now());
    }
}

// --- the bar and the ladder -------------------------------------------------

use nus_render::text::Style;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, PhysicalKey};

use crate::app::{fade, App, Pane};

/// How long a page may take to give its first answer before the bar says
/// it didn't, rather than "No matches".
const NO_ANSWER: Duration = Duration::from_millis(1500);
/// Wider rungs are counted once typing pauses this long.
const WIDE_AFTER: Duration = Duration::from_millis(250);
/// Time a frame may spend searching history, across every shell.
const BUDGET: Duration = Duration::from_millis(3);

/// How far a search reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Pane,
    Tab,
    Window,
    /// The window, and what isn't open: notes, and the commands of shells
    /// since closed (the journal), as a list under the bar.
    Nus,
}

impl Scope {
    const ALL: [Scope; 4] = [Scope::Pane, Scope::Tab, Scope::Window, Scope::Nus];
    fn wider(self) -> Scope {
        match self {
            Scope::Pane => Scope::Tab,
            Scope::Tab => Scope::Window,
            _ => Scope::Nus,
        }
    }
    fn narrower(self) -> Scope {
        match self {
            Scope::Nus => Scope::Window,
            Scope::Window => Scope::Tab,
            _ => Scope::Pane,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Scope::Pane => "PANE",
            Scope::Tab => "TAB",
            Scope::Window => "WINDOW",
            Scope::Nus => "NUS",
        }
    }
}

/// A pane, by its tab's id and side.
pub type PaneId = (u64, bool);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Field,
    Case,
    Word,
    Regex,
    Prev,
    Next,
    Close,
    Rung(Scope),
    /// A row of the NUS list.
    Away(usize),
}

/// Something that isn't open, found for the NUS rung.
#[derive(Clone, Debug)]
pub struct Away {
    pub kind: AwayKind,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub enum AwayKind {
    /// A note, at the line of the match.
    Note(std::path::PathBuf, usize),
    /// A command a shell ran, in its folder: it opens typed, not run.
    Command { cwd: String, cmd: String },
}

/// Commands in the journal matching the query, newest first: read off the
/// UI thread (the journal is files).
fn journal_matches(q: &str, o: crate::editor_work::Opts, limit: usize) -> Vec<Away> {
    let re = match crate::editor_work::pattern(q, o) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };
    let lower = q.to_lowercase();
    let hit = |cmd: &str| match &re {
        Some(re) => re.is_match(cmd),
        None if o.case => cmd.contains(q),
        None => cmd.to_lowercase().contains(&lower),
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in crate::journal::since(0) {
        if hit(&e.cmd) && seen.insert((e.cwd.clone(), e.cmd.clone())) {
            let when = crate::journal::when(e.start);
            let exit = match e.exit { Some(0) => "ok".to_string(), Some(n) => format!("exit {n}"), None => String::new() };
            out.push(Away {
                title: crate::journal::oneline(&e.cmd),
                detail: [e.cwd.clone(), when, exit].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                kind: AwayKind::Command { cwd: e.cwd, cmd: e.cmd },
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// The bar: one per window, on the pane it was opened in.
pub struct Bar {
    pub query: String,
    pub case: bool,
    pub word: bool,
    pub regex: bool,
    pub scope: Scope,
    /// Where it was opened: the walk is counted from here.
    pub home: PaneId,
    /// The pane holding the current match, where the bar is drawn.
    pub at: PaneId,
    /// Keys go to the bar (not the pane under it).
    pub focused: bool,
    notice: Option<(String, Instant)>,
    /// The other panes are searched once typing pauses.
    wide_due: Option<Instant>,
    /// Panes searched for the current query.
    searched: Vec<PaneId>,
    /// The current match has not been brought into view yet.
    reveal: bool,
    /// Drawn at the pane's foot: the current match is where it would be.
    low: bool,
    pub rect: Rect,
    pub hits: Vec<(Rect, Hit)>,
    /// The NUS rung's list: notes now, journal commands when the worker
    /// answers; `away_done` once both have.
    pub away: Vec<Away>,
    away_job: Option<std::sync::mpsc::Receiver<Vec<Away>>>,
    away_done: bool,
    /// The list row ↑↓ has picked, for ↵.
    pick: Option<usize>,
    /// Motion: when it opened (it unrolls from its top edge), the rung the
    /// fill is sliding from, where the card is on its way between top and
    /// foot, the count's last words (a new one ticks up into place), when
    /// the NUS list appeared.
    opened: Instant,
    rung_from: Option<(Rect, Instant)>,
    rung_rect: Option<Rect>,
    y_shown: Option<(f32, f32, Instant)>,
    count_shown: (String, Instant),
    list_at: Option<Instant>,
}

/// 0..1 over `ms` scaled by the motion setting since `at`, eased out; 1 at
/// once when motion is reduced.
fn eased(motion: &crate::anim::Motion, at: Instant, ms: f32) -> f32 {
    let d = motion.dur(ms);
    if d <= 0.0 {
        return 1.0;
    }
    let k = (crate::clock::since(at).as_secs_f32() / d).clamp(0.0, 1.0);
    1.0 - (1.0 - k).powi(3)
}

/// A count, in words: "3 of 41", "counting… 12", "No matches", "10,000+".
pub fn count_words(total: usize, current: Option<usize>, done: bool, capped: bool) -> String {
    let n = if capped { format!("{}+", thousands(CAP)) } else { thousands(total) };
    match (total, current, done) {
        (0, _, true) => "No matches".into(),
        (0, _, false) => "counting…".into(),
        (_, Some(k), true) => format!("{} of {n}", thousands(k + 1)),
        (_, Some(k), false) => format!("{} of {n}…", thousands(k + 1)),
        (_, None, true) => format!("{n} matches"),
        (_, None, false) => format!("counting… {n}"),
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn chord(shift: bool) -> &'static str {
    match (cfg!(target_os = "macos"), shift) {
        (true, false) => "⌘F",
        (true, true) => "⌘⇧F",
        (false, false) => "CTRL+F",
        (false, true) => "CTRL+SHIFT+F",
    }
}

impl App {
    fn tab_of(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn find_pane(&self, id: PaneId) -> Option<&Pane> {
        let t = self.tabs.get(self.tab_of(id.0)?)?;
        if id.1 { t.right.as_ref() } else { Some(&t.left) }
    }

    fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        let i = self.tab_of(id.0)?;
        let t = self.tabs.get_mut(i)?;
        if id.1 { t.right.as_mut() } else { Some(&mut t.left) }
    }

    fn searchable(p: &Pane) -> bool {
        match p {
            Pane::Term(_) | Pane::Editor(_) => true,
            Pane::Web(w) => w.asleep.is_none(),
            _ => false,
        }
    }

    /// The panes a scope covers, in walking order: the home pane, its
    /// tab's other pane, then the following tabs in strip order.
    pub(crate) fn find_panes(&self, scope: Scope, home: PaneId) -> Vec<PaneId> {
        let Some(hi) = self.tab_of(home.0) else { return Vec::new() };
        let n = self.tabs.len();
        let tabs: Vec<usize> = if scope >= Scope::Window { (0..n).map(|k| (hi + k) % n).collect() } else { vec![hi] };
        let mut out = Vec::new();
        for ti in tabs {
            let t = &self.tabs[ti];
            // The hatch's tab lives in its own window, with its own keys.
            if t.hatch && ti != hi {
                continue;
            }
            let sides: Vec<bool> = if ti == hi {
                if scope == Scope::Pane { vec![home.1] } else { vec![home.1, !home.1] }
            } else {
                vec![false, true]
            };
            for right in sides {
                let p = if right { t.right.as_ref() } else { Some(&t.left) };
                if p.is_some_and(Self::searchable) {
                    out.push((t.id, right));
                }
            }
        }
        out
    }

    /// Sleeping pages a scope would cover, not searched.
    fn find_asleep(&self, scope: Scope) -> usize {
        if scope < Scope::Window {
            return 0;
        }
        self.tabs.iter().flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref())).filter(|p| matches!(p, Pane::Web(w) if w.asleep.is_some())).count()
    }

    fn find_run(&mut self, id: PaneId) {
        let Some(bar) = self.find_bar.as_ref() else { return };
        let (q, case, word, regex) = (bar.query.clone(), bar.case, bar.word, bar.regex);
        match self.find_pane_mut(id) {
            Some(Pane::Term(p)) => {
                p.search = (!q.is_empty()).then(|| TermFind::with(&q, case, word, regex, &p.term));
            }
            Some(Pane::Web(w)) => {
                // Chromium's find is plain text: with word or regex on, a page
                // is not searched rather than searched wrongly.
                if q.is_empty() || word || regex {
                    w.tab.stop_find(false);
                } else {
                    w.tab.find(&q, case, true, false);
                }
            }
            Some(Pane::Editor(e)) => {
                if q.is_empty() {
                    e.find = None;
                } else {
                    e.find = Some(crate::editor::Find {
                        query: q,
                        replace: String::new(),
                        in_replace: false,
                        with_replace: false,
                        matches: Vec::new(),
                        current: 0,
                        truncated: false,
                        opts: crate::editor_work::Opts { case, word, regex },
                        error: None,
                        shared: true,
                    });
                    e.refind();
                }
            }
            _ => {}
        }
        if let Some(bar) = self.find_bar.as_mut() {
            if !bar.searched.contains(&id) {
                bar.searched.push(id);
            }
        }
    }

    fn find_clear(&mut self, id: PaneId, keep: bool) {
        match self.find_pane_mut(id) {
            Some(Pane::Term(p)) => p.search = None,
            Some(Pane::Web(w)) => w.tab.stop_find(keep),
            Some(Pane::Editor(e)) => {
                if e.find.as_ref().is_some_and(|f| f.shared) {
                    e.find = None;
                }
            }
            _ => {}
        }
    }

    /// What one pane knows.
    pub(crate) fn find_status(&self, id: PaneId) -> Status {
        let searched = self.find_bar.as_ref().is_some_and(|b| b.searched.contains(&id));
        let plain = self.find_bar.as_ref().is_none_or(|b| !b.word && !b.regex);
        match self.find_pane(id) {
            Some(Pane::Web(_)) if searched && !plain => Status { done: true, note: Some("pages: plain text only"), ..Status::default() },
            Some(Pane::Editor(e)) if searched => {
                let Some(f) = e.find.as_ref() else { return Status { done: true, ..Status::default() } };
                Status {
                    total: f.matches.len(),
                    current: (!f.matches.is_empty()).then_some(f.current.min(f.matches.len().saturating_sub(1))),
                    done: !e.searching(),
                    capped: f.truncated,
                    error: f.error.clone(),
                    ..Status::default()
                }
            }
            Some(Pane::Term(p)) => p.search.as_ref().map(|s| s.status()).unwrap_or_default(),
            Some(Pane::Web(w)) if searched => {
                let s = w.tab.shared.borrow();
                let f = &s.find;
                if f.changed {
                    return Status { note: Some("page changed"), ..Status::default() };
                }
                if !f.answered && f.asked.is_some_and(|a| a.elapsed() > NO_ANSWER) && f.count == 0 {
                    return Status { done: true, note: Some("the page didn't answer"), ..Status::default() };
                }
                Status {
                    total: f.count.max(0) as usize,
                    current: (f.active > 0 && f.count > 0).then(|| (f.active - 1) as usize),
                    done: f.last,
                    ..Status::default()
                }
            }
            _ => Status::default(),
        }
    }

    /// A scope's count, and the current match's place in it.
    fn find_tally(&self, scope: Scope) -> Status {
        let Some(bar) = self.find_bar.as_ref() else { return Status::default() };
        let mut out = Status { done: true, ..Status::default() };
        let mut offset = 0;
        for id in self.find_panes(scope, bar.home) {
            if !bar.searched.contains(&id) {
                out.done = false;
                continue;
            }
            let s = self.find_status(id);
            if id == bar.at {
                out.current = s.current.map(|k| offset + k);
                out.note = s.note;
            }
            if out.error.is_none() {
                out.error = s.error.clone();
            }
            offset += s.total;
            out.total += s.total;
            out.done &= s.done;
            out.capped |= s.capped;
            out.fresh += s.fresh;
        }
        if scope == Scope::Nus {
            out.total += bar.away.len();
            out.done &= bar.away_done;
        }
        out
    }

    /// Open the bar on the pane in front, or give it the keys if it is
    /// already open. False when the pane has no find of this kind (the
    /// editor keeps its own, with replace).
    pub(crate) fn open_find(&mut self) -> bool {
        if self.settings_view.is_some() { self.open_palette(crate::app::PaletteMode::Settings); return true; }
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let id = (tab.id, tab.focus_right && tab.right.is_some());
        if !Self::searchable(tab.focused_ref()) {
            return false;
        }
        // The shell's selection, one line of it, is the obvious query.
        let seed = match tab.focused_ref() {
            Pane::Term(p) if p.sel.is_some() => {
                let text = p.selection_text();
                Some(text.lines().next().unwrap_or("").trim().to_string()).filter(|s| !s.is_empty() && s.chars().count() <= 200)
            }
            _ => None,
        };
        if let Some(bar) = self.find_bar.as_mut() {
            bar.focused = true;
            if bar.at != id && bar.home != id {
                // Opened on another pane: the bar moves there.
                let old: Vec<PaneId> = bar.searched.drain(..).collect();
                bar.home = id;
                bar.at = id;
                bar.scope = Scope::Pane;
                for o in old {
                    self.find_clear(o, false);
                }
                self.find_changed();
            }
            if let Some(s) = seed {
                if let Some(bar) = self.find_bar.as_mut() {
                    bar.query = s;
                }
                self.find_changed();
            }
            self.dirty = true;
            return true;
        }
        self.find_bar = Some(Bar {
            query: String::new(),
            case: false,
            word: false,
            regex: false,
            scope: Scope::Pane,
            home: id,
            at: id,
            focused: true,
            notice: None,
            wide_due: None,
            searched: Vec::new(),
            reveal: false,
            low: false,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            hits: Vec::new(),
            away: Vec::new(),
            away_job: None,
            away_done: false,
            pick: None,
            opened: crate::clock::now(),
            rung_from: None,
            rung_rect: None,
            y_shown: None,
            count_shown: (String::new(), crate::clock::now()),
            list_at: None,
        });
        if let Some(s) = seed {
            if let Some(bar) = self.find_bar.as_mut() {
                bar.query = s;
            }
            self.find_changed();
        }
        self.dirty = true;
        true
    }

    /// The query or its case changed: the home pane now, the rest once
    /// typing pauses.
    fn find_changed(&mut self) {
        let Some(bar) = self.find_bar.as_mut() else { return };
        let stale: Vec<PaneId> = bar.searched.drain(..).filter(|id| *id != bar.home).collect();
        bar.at = bar.home;
        bar.notice = None;
        bar.reveal = true;
        bar.away.clear();
        bar.away_job = None;
        bar.away_done = false;
        bar.pick = None;
        bar.wide_due = Some(Instant::now() + WIDE_AFTER);
        let home = bar.home;
        for id in stale {
            self.find_clear(id, false);
        }
        self.find_run(home);
        self.dirty = true;
    }

    /// What isn't open: notes from the in-memory index at once (a regex has
    /// no meaning there, so not then), journal commands on a worker.
    fn find_gather_away(&mut self) {
        let Some(bar) = self.find_bar.as_mut() else { return };
        if bar.query.is_empty() || bar.away_done || bar.away_job.is_some() {
            return;
        }
        let (q, o) = (bar.query.clone(), crate::editor_work::Opts { case: bar.case, word: bar.word, regex: bar.regex });
        let mut found = Vec::new();
        if !o.regex {
            if let Ok(hits) = crate::notes_index::search(&q, None, 8) {
                for h in hits {
                    found.push(Away { title: h.title.clone(), detail: format!("note · {} · {}", h.home_name, h.snippet), kind: AwayKind::Note(h.path.clone(), h.line) });
                }
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("find journal".into()).spawn(move || {
            let _ = tx.send(journal_matches(&q, o, 8));
        });
        let bar = self.find_bar.as_mut().unwrap();
        bar.away = found;
        if spawned.is_ok() {
            bar.away_job = Some(rx);
        } else {
            bar.away_done = true;
        }
    }

    /// Open a row of the NUS list where it lives.
    fn find_open_away(&mut self, i: usize) {
        let Some(item) = self.find_bar.as_ref().and_then(|b| b.away.get(i)).cloned() else { return };
        self.close_find();
        match item.kind {
            AwayKind::Note(path, line) => self.open_note_at(&path, line),
            AwayKind::Command { cwd, cmd } => {
                let cwd = std::path::Path::new(&cwd).is_dir().then_some(cwd);
                match self.new_term_pane_at(false, self.behavior.default_profile, cwd) {
                    Ok(mut t) => {
                        // Typed, not run: you read it, you press Enter.
                        t.type_at_prompt = Some(cmd);
                        let tab = self.make_tab(Pane::Term(t), None);
                        self.tabs.push(tab);
                        self.activate(self.tabs.len() - 1);
                    }
                    Err(e) => self.notice_problem("Could Not Open A Shell", e.to_string()),
                }
            }
        }
        self.dirty = true;
    }

    /// The query was set from outside (a script): search for it.
    pub(crate) fn find_query_set(&mut self) {
        self.find_changed();
    }

    pub(crate) fn close_find(&mut self) {
        let Some(bar) = self.find_bar.take() else { return };
        self.find_ghost = Some((bar.rect, crate::clock::now()));
        for id in bar.searched {
            self.find_clear(id, id == bar.at);
        }
        self.dirty = true;
    }

    fn find_notice(&mut self, text: String) {
        if let Some(bar) = self.find_bar.as_mut() {
            bar.notice = Some((text, Instant::now()));
        }
    }

    /// Bring the current match into view: a shell scrolls to it (opening a
    /// fold around it); a page has already been scrolled by Chromium.
    fn find_reveal(&mut self, id: PaneId) {
        if let Some(Pane::Editor(e)) = self.find_pane_mut(id) {
            e.find_select();
        }
        if let Some(Pane::Term(p)) = self.find_pane_mut(id) {
            let Some(cur) = p.search.as_ref().and_then(|s| s.current) else { return };
            p.folds.retain(|&(s, e)| !(cur.line >= s && cur.line < e));
            let g = p.term.grid();
            let rows = g.rows() as u64;
            let top = g.abs_of_display(0);
            let shown = cur.line >= top && cur.end_line < top + rows;
            if !shown || !p.folds.is_empty() {
                let to = cur.line.saturating_sub(rows / 3);
                p.term.grid_mut().scroll_to_abs(to);
            }
            p.view_key = None;
        }
        self.dirty = true;
    }

    /// ↵ (forward) and ⇧↵. In a shell forward is older; on a page, down.
    /// At a pane's end the walk goes on to the next pane in scope that has
    /// matches, or wraps, and says so.
    pub(crate) fn find_step(&mut self, forward: bool) {
        let Some(bar) = self.find_bar.as_ref() else { return };
        if bar.query.is_empty() {
            return;
        }
        let at = bar.at;
        let scope = bar.scope;
        let home = bar.home;
        let step = match self.find_pane_mut(at) {
            Some(Pane::Term(p)) => p.search.as_mut().map_or(Step::Empty, |s| s.step(forward)),
            Some(Pane::Web(w)) => {
                let f = w.tab.shared.borrow().find.clone();
                if f.count <= 0 {
                    Step::Empty
                } else if (forward && f.active >= f.count) || (!forward && f.active <= 1) {
                    Step::Wrapped
                } else {
                    w.tab.find(&f.query, f.case, forward, true);
                    Step::Moved
                }
            }
            Some(Pane::Editor(e)) => match e.find.as_ref().map(|f| (f.matches.len(), f.current)) {
                None | Some((0, _)) => Step::Empty,
                Some((n, k)) if (forward && k + 1 >= n) || (!forward && k == 0) => Step::Wrapped,
                Some(_) => {
                    e.find_step(forward);
                    Step::Moved
                }
            },
            _ => Step::Empty,
        };
        if step == Step::Moved {
            if let Some(bar) = self.find_bar.as_mut() {
                bar.notice = None;
            }
            self.find_reveal(at);
            return;
        }
        // The end of this pane: the next one with matches, in order.
        let order = self.find_panes(scope, home);
        let me = order.iter().position(|p| *p == at).unwrap_or(0);
        let n = order.len();
        let next = (1..=n).map(|k| if forward { (me + k) % n } else { (me + n - k % n) % n }).map(|i| order[i]).find(|id| self.find_status(*id).total > 0);
        let Some(next) = next else { return };
        let passed_home = if forward { order.iter().position(|p| *p == next).unwrap_or(0) <= me } else { order.iter().position(|p| *p == next).unwrap_or(0) >= me };
        // Into the next pane: its first match, the way the walk goes.
        match self.find_pane_mut(next) {
            Some(Pane::Term(p)) => {
                if let Some(s) = p.search.as_mut() {
                    s.enter(forward);
                }
            }
            Some(Pane::Editor(e)) => {
                if let Some(f) = e.find.as_mut() {
                    f.current = if forward { 0 } else { f.matches.len().saturating_sub(1) };
                }
                e.find_select();
            }
            Some(Pane::Web(w)) => {
                let (q, case) = { let f = &w.tab.shared.borrow().find; (f.query.clone(), f.case) };
                if next == at {
                    // The only pane with matches: Chromium wraps by itself.
                    w.tab.find(&q, case, forward, true);
                } else {
                    w.tab.stop_find(false);
                    w.tab.find(&q, case, forward, false);
                }
            }
            _ => {}
        }
        if next == at {
            if let Some(Pane::Term(p)) = self.find_pane_mut(at) {
                if let Some(s) = p.search.as_mut() {
                    s.enter(forward);
                }
            }
        }
        if let Some(bar) = self.find_bar.as_mut() {
            bar.at = next;
        }
        if passed_home || next == at {
            let words = match (self.find_pane(next), forward) {
                (Some(Pane::Term(_)), true) => "wrapped to the newest",
                (Some(Pane::Term(_)), false) => "wrapped to the oldest",
                (_, true) => "wrapped to the first",
                (_, false) => "wrapped to the last",
            };
            self.find_notice(words.into());
        } else if let Some(bar) = self.find_bar.as_mut() {
            bar.notice = None;
        }
        // Arriving in another tab makes it the tab in front.
        if let Some(i) = self.tab_of(next.0) {
            if i != self.active {
                self.activate(i);
            }
            if let Some(t) = self.tabs.get_mut(i) {
                t.focus_right = next.1 && t.right.is_some();
            }
        }
        self.find_reveal(next);
    }

    fn find_scope(&mut self, scope: Scope) {
        let Some(bar) = self.find_bar.as_mut() else { return };
        if bar.scope == scope {
            return;
        }
        bar.rung_from = bar.rung_rect.map(|r| (r, crate::clock::now()));
        bar.scope = scope;
        bar.notice = None;
        bar.wide_due = Some(Instant::now());
        let (at, home) = (bar.at, bar.home);
        // Narrowed past where the walk had got to: back home.
        if !self.find_panes(scope, home).contains(&at) {
            if let Some(bar) = self.find_bar.as_mut() {
                bar.at = home;
            }
            if let Some(i) = self.tab_of(home.0) {
                if i != self.active {
                    self.activate(i);
                }
            }
        }
        self.dirty = true;
    }

    /// Keys while the bar has them. False for chords it leaves to the app.
    pub(crate) fn find_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        let Some(bar) = self.find_bar.as_mut() else { return false };
        if !bar.focused {
            return false;
        }
        if ev.state != ElementState::Pressed {
            return true;
        }
        let mods = self.mods;
        let command = crate::field::command(mods);
        let shift = mods.shift_key();
        let code = match ev.physical_key {
            PhysicalKey::Code(c) => Some(c),
            _ => None,
        };
        if command && code == Some(KeyCode::KeyF) {
            let here = bar.scope;
            let scope = if shift {
                here.narrower()
            } else {
                // Nothing here: straight to the nearest rung that has some,
                // which is what the bar's hint promised.
                let now = self.find_tally(here);
                let nearest = (now.total == 0 && now.done).then(|| Scope::ALL.into_iter().find(|s| *s > here && self.find_tally(*s).total > 0)).flatten();
                nearest.unwrap_or(here.wider())
            };
            self.find_scope(scope);
            return true;
        }
        if mods.alt_key() && matches!(code, Some(KeyCode::KeyC | KeyCode::KeyW | KeyCode::KeyR)) {
            match code {
                Some(KeyCode::KeyC) => bar.case = !bar.case,
                Some(KeyCode::KeyW) => bar.word = !bar.word,
                _ => bar.regex = !bar.regex,
            }
            self.find_changed();
            return true;
        }
        // Replace is the editor's own bar: it takes the query along.
        if command && code == Some(KeyCode::KeyH) {
            let (at, q) = (bar.at, bar.query.clone());
            if let Some(Pane::Editor(e)) = self.find_pane_mut(at) {
                if let Some(f) = e.find.as_mut() {
                    f.shared = false;
                    f.query = q;
                }
                self.close_find();
            }
            return false;
        }
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                self.close_find();
                return true;
            }
            WKey::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) if bar.scope == Scope::Nus && !bar.away.is_empty() => {
                let n = bar.away.len();
                let down = matches!(ev.logical_key, WKey::Named(NamedKey::ArrowDown));
                bar.pick = Some(match (bar.pick, down) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(k), true) => (k + 1) % n,
                    (Some(k), false) => (k + n - 1) % n,
                });
                self.dirty = true;
                return true;
            }
            WKey::Named(NamedKey::Enter) => {
                if let Some(k) = bar.pick.filter(|_| bar.scope == Scope::Nus) {
                    self.find_open_away(k);
                    return true;
                }
                self.find_step(!shift);
                self.dirty = true;
                return true;
            }
            WKey::Named(NamedKey::F3) => {
                self.find_step(!shift);
                self.dirty = true;
                return true;
            }
            _ => {}
        }
        let took = crate::field::edit(&mut bar.query, ev, mods, 400);
        if took.changed() {
            self.find_changed();
            return true;
        }
        if took.taken() {
            self.dirty = true;
            return true;
        }
        // Other chords (new tab, the palette…) are the app's.
        !(command || mods.alt_key())
    }

    /// A press or release; true when the bar took it. A press elsewhere
    /// gives the keys back to the pane, and the bar stays.
    pub(crate) fn find_mouse(&mut self, button: MouseButton, state: ElementState) -> bool {
        let (x, y) = self.mouse;
        let Some(bar) = self.find_bar.as_mut() else { return false };
        if !bar.rect.contains(x, y) {
            if state == ElementState::Pressed {
                bar.focused = false;
            }
            return false;
        }
        if button != MouseButton::Left || state != ElementState::Pressed {
            return true;
        }
        bar.focused = true;
        let hit = bar.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h);
        if let Some(h) = hit {
            self.find_act(h);
        }
        self.dirty = true;
        true
    }

    /// A control of the bar, by mouse or by a screen reader.
    pub(crate) fn find_act(&mut self, hit: Hit) {
        let Some(bar) = self.find_bar.as_mut() else { return };
        match hit {
            Hit::Case => bar.case = !bar.case,
            Hit::Word => bar.word = !bar.word,
            Hit::Regex => bar.regex = !bar.regex,
            Hit::Field => {
                bar.focused = true;
                self.dirty = true;
                return;
            }
            Hit::Prev => return self.find_step(false),
            Hit::Next => return self.find_step(true),
            Hit::Close => return self.close_find(),
            Hit::Rung(s) => return self.find_scope(s),
            Hit::Away(i) => return self.find_open_away(i),
        }
        self.find_changed();
    }

    /// A screen reader set the query.
    pub(crate) fn find_set_query(&mut self, q: &str) {
        if let Some(bar) = self.find_bar.as_mut() {
            bar.query = q.chars().take(400).collect();
        }
        self.find_changed();
    }

    /// The bar for the accessibility tree: the query, the count as said
    /// aloud, and each control with its name and, for options, its state.
    pub(crate) fn find_access(&self) -> Option<(String, String, bool, Vec<(Rect, Hit, String, Option<bool>)>)> {
        let bar = self.find_bar.as_ref()?;
        let t = self.find_tally(bar.scope);
        let count = if bar.query.is_empty() {
            String::new()
        } else if let Some(e) = t.error.as_ref() {
            format!("Not a regular expression: {e}")
        } else if let Some(n) = t.note.filter(|_| t.total == 0) {
            n.to_string()
        } else if t.total == 0 && t.done {
            let wider = Scope::ALL.into_iter().find(|s| *s > bar.scope && self.find_tally(*s).total > 0);
            match wider {
                Some(s) => format!("No matches here, {} in this {}", self.find_tally(s).total, s.word().to_lowercase()),
                None => "No matches".into(),
            }
        } else {
            count_words(t.total, t.current, t.done, t.capped)
        };
        let controls = bar
            .hits
            .iter()
            .map(|(r, h)| {
                let (name, on) = match h {
                    Hit::Field => ("Find".to_string(), None),
                    Hit::Case => ("Match case".into(), Some(bar.case)),
                    Hit::Word => ("Whole words".into(), Some(bar.word)),
                    Hit::Regex => ("Regular expression".into(), Some(bar.regex)),
                    Hit::Prev => ("Previous match".into(), None),
                    Hit::Next => ("Next match".into(), None),
                    Hit::Close => ("Close find".into(), None),
                    Hit::Rung(s) => {
                        let r = self.find_tally(*s);
                        let place = if *s == Scope::Nus { "all of nus".to_string() } else { format!("this {}", s.word().to_lowercase()) };
                        (format!("Search {place}: {} matches", r.total), Some(*s == bar.scope))
                    }
                    Hit::Away(i) => {
                        let a = &bar.away[*i];
                        (format!("Open {}: {}", a.title, a.detail), None)
                    }
                };
                (*r, *h, name, on)
            })
            .collect::<Vec<_>>();
        // Reading order: the field, its options, the walk, the rungs, close.
        let rank = |h: &Hit| match h {
            Hit::Field => 0,
            Hit::Case => 1,
            Hit::Word => 2,
            Hit::Regex => 3,
            Hit::Prev => 4,
            Hit::Next => 5,
            Hit::Rung(Scope::Pane) => 6,
            Hit::Rung(Scope::Tab) => 7,
            Hit::Rung(Scope::Window) => 8,
            Hit::Rung(Scope::Nus) => 9,
            Hit::Away(_) => 10,
            Hit::Close => 11,
        };
        let mut controls = controls;
        controls.sort_by_key(|c| rank(&c.1));
        Some((bar.query.clone(), count, bar.focused, controls))
    }

    /// Each frame while the bar is up: search on, catch pages up, and keep
    /// frames coming while anything is still counting.
    pub(crate) fn tend_find(&mut self) {
        let Some(bar) = self.find_bar.as_ref() else { return };
        // Its pane closed: so does the bar.
        if self.find_pane(bar.home).is_none_or(|p| !Self::searchable(p)) {
            self.find_bar = None;
            self.dirty = true;
            return;
        }
        if self.find_pane(bar.at).is_none() {
            let home = bar.home;
            if let Some(b) = self.find_bar.as_mut() {
                b.at = home;
            }
        }
        let bar = self.find_bar.as_ref().unwrap();
        let query_set = !bar.query.is_empty();
        // The rest of the window, once typing has paused: every rung shows
        // its count, whichever is chosen.
        if query_set && bar.wide_due.is_some_and(|t| Instant::now() >= t) {
            let home = bar.home;
            let todo: Vec<PaneId> = self.find_panes(Scope::Window, home).into_iter().filter(|id| !bar.searched.contains(id)).collect();
            if let Some(b) = self.find_bar.as_mut() {
                b.wide_due = None;
            }
            for id in todo {
                self.find_run(id);
            }
            self.find_gather_away();
        }
        // The journal's answer.
        if let Some(bar) = self.find_bar.as_mut() {
            if let Some(rx) = bar.away_job.as_ref() {
                match rx.try_recv() {
                    Ok(found) => {
                        bar.away.extend(found);
                        bar.away_job = None;
                        bar.away_done = true;
                        self.dirty = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        bar.away_job = None;
                        bar.away_done = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => self.dirty = true,
                }
            }
        }
        let searched = self.find_bar.as_ref().map(|b| b.searched.clone()).unwrap_or_default();
        let mut busy = false;
        let mut changed = false;
        let start = Instant::now();
        for id in &searched {
            match self.find_pane_mut(*id) {
                Some(Pane::Term(p)) => {
                    let left = BUDGET.saturating_sub(start.elapsed());
                    if let Some(s) = p.search.as_mut() {
                        changed |= s.tick(&p.term, left);
                        busy |= !s.status().done;
                    }
                }
                Some(Pane::Editor(e)) => {
                    busy |= e.searching();
                }
                Some(Pane::Web(w)) => {
                    let (rerun, waiting) = {
                        let s = w.tab.shared.borrow();
                        (s.find.changed && !s.loading, !s.find.last || !s.find.answered)
                    };
                    // A new page in this pane: ask it again once it has loaded.
                    if rerun {
                        let (q, case) = { let f = &w.tab.shared.borrow().find; (f.query.clone(), f.case) };
                        w.tab.find(&q, case, true, false);
                    }
                    busy |= waiting;
                }
                _ => {}
            }
        }
        if changed {
            self.dirty = true;
        }
        // A shell's first match, once found, is brought into view.
        let (at, reveal) = self.find_bar.as_ref().map(|b| (b.at, b.reveal)).unwrap();
        if reveal && self.find_status(at).current.is_some() {
            if let Some(b) = self.find_bar.as_mut() {
                b.reveal = false;
            }
            self.find_reveal(at);
        }
        let notice_live = self.find_bar.as_ref().and_then(|b| b.notice.as_ref()).is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(3));
        if busy || notice_live || self.find_bar.as_ref().is_some_and(|b| b.wide_due.is_some()) {
            self.dirty = true;
        }
    }

    /// Where a pane's content is, and its current match on screen.
    fn find_geometry(&mut self, id: PaneId) -> Option<(Rect, Option<Rect>)> {
        let scale = self.scale;
        let header = self.header_h();
        match self.find_pane_mut(id)? {
            Pane::Web(w) => {
                let page = w.page;
                let cur = w.tab.shared.borrow().find.rect.map(|(x, y, ww, hh)| Rect::new(page.x + x as f32 * scale, page.y + y as f32 * scale, ww as f32 * scale, hh as f32 * scale));
                Some((page, cur))
            }
            Pane::Editor(e) => {
                let r = e.rect;
                let (cw, ch) = e.cell;
                let origin = e.origin;
                let cur = e.find.as_ref().and_then(|f| f.matches.get(f.current).copied()).and_then(|(a, z)| {
                    let b = e.buffers.get(e.active)?;
                    let row = b.line_of(a).checked_sub(b.scroll)?;
                    Some(Rect::new(origin.0 + b.col_of(a) as f32 * cw, origin.1 + row as f32 * ch, (z - a).max(1) as f32 * cw, ch))
                });
                Some((r, cur))
            }
            Pane::Term(p) => {
                let r = p.rect;
                let top = if p.show_header { header } else { 0.0 };
                let content = Rect::new(r.x, r.y + top, r.w, (r.h - top).max(0.0));
                let (cw, ch) = p.grid.cell_size();
                let cur = p.search.as_ref().and_then(|s| s.current);
                let origin = p.origin;
                let view = p.view().to_vec();
                let cur = cur.and_then(|c| {
                    let row = view.iter().position(|d| matches!(d, nus_vt::grid::Display::Line(l) if *l == c.line))?;
                    Some(Rect::new(origin.0 + c.col as f32 * cw, origin.1 + row as f32 * ch, ((c.end_col.max(c.col + 1)) - c.col) as f32 * cw, ch * (1 + c.end_line - c.line) as f32))
                });
                Some((content, cur))
            }
            _ => None,
        }
    }

    /// `nus find`: open the bar on the pane in front with a query, or say
    /// where it has got to (`status`), or close it.
    pub(crate) fn find_remote(&mut self, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if args.get("close").and_then(|v| v.as_bool()) == Some(true) {
            self.close_find();
            return Ok(json!({ "closed": true }));
        }
        if let Some(q) = args.get("q").and_then(|v| v.as_str()) {
            if !self.open_find() {
                return Err("the pane in front has no find bar (an editor keeps its own find)".into());
            }
            let scope = match args.get("scope").and_then(|v| v.as_str()).unwrap_or("pane") {
                "tab" => Scope::Tab,
                "window" => Scope::Window,
                "nus" => Scope::Nus,
                "pane" => Scope::Pane,
                other => return Err(format!("scope is pane, tab or window, not {other}")),
            };
            if let Some(bar) = self.find_bar.as_mut() {
                bar.query = q.to_string();
                bar.case = args.get("case").and_then(|v| v.as_bool()).unwrap_or(false);
                bar.focused = false;
            }
            self.find_changed();
            if let Some(bar) = self.find_bar.as_mut() {
                bar.scope = scope;
                bar.wide_due = Some(Instant::now());
            }
        }
        let Some(bar) = self.find_bar.as_ref() else { return Ok(json!({ "open": false })) };
        let t = self.find_tally(bar.scope);
        let rung = |s: Scope| { let r = self.find_tally(s); json!({ "total": r.total, "done": r.done }) };
        Ok(json!({
            "open": true,
            "query": bar.query,
            "scope": bar.scope.word().to_lowercase(),
            "words": count_words(t.total, t.current, t.done, t.capped),
            "total": t.total,
            "current": t.current.map(|c| c + 1),
            "done": t.done,
            "note": t.note,
            "pane": rung(Scope::Pane),
            "tab": rung(Scope::Tab),
            "window": rung(Scope::Window),
            "nus": rung(Scope::Nus),
            "elsewhere": bar.away.iter().map(|a| a.title.clone()).collect::<Vec<_>>(),
        }))
    }

    /// The bar's state in words, for scripted checks: the count as shown,
    /// each rung's total, and where the walk is.
    pub(crate) fn find_report(&self, scope: Scope) -> String {
        let t = self.find_tally(scope);
        let rungs: Vec<String> = Scope::ALL.iter().map(|&s| { let r = self.find_tally(s); format!("{}={}{}", s.word(), r.total, if r.done { "" } else { "…" }) }).collect();
        let at = self.find_bar.as_ref().map(|b| b.at).unwrap_or((0, false));
        let tab = self.tab_of(at.0).map(|i| i + 1).unwrap_or(0);
        let q = self.find_bar.as_ref().map(|b| b.query.clone()).unwrap_or_default();
        let words = t.error.as_ref().map(|e| format!("regex: {e}")).unwrap_or_else(|| count_words(t.total, t.current, t.done, t.capped));
        format!("{q:?} [{words}] {} · {} · at tab {tab}{}", t.note.unwrap_or(""), rungs.join(" "), if at.1 { " right" } else { "" })
    }

    /// The bar, over the pane holding the current match.
    pub(crate) fn draw_find_bar(&mut self, scene: &mut Scene) {
        // Closed: the card rolls back up into its top edge.
        if self.find_bar.is_none() {
            if let Some((r, at)) = self.find_ghost {
                let e = eased(&self.motion, at, 90.0);
                if e >= 1.0 {
                    self.find_ghost = None;
                } else {
                    let h = r.h * (1.0 - e);
                    let ink = self.theme.ink;
                    scene.layer(None);
                    scene.rect(Rect::new(r.x, r.y, r.w, h), self.paper());
                    scene.outline(Rect::new(r.x, r.y, r.w, h), self.px(m::STRUCTURE), fade(ink, 1.0 - e));
                    self.dirty = true;
                }
            }
            return;
        }
        self.find_ghost = None;
        let Some(at) = self.find_bar.as_ref().map(|b| b.at) else { return };
        // Only over a pane on screen: the walk may have left it behind.
        let on_screen = self.tabs.get(self.active).is_some_and(|t| t.id == at.0);
        if !on_screen {
            return;
        }
        let Some((pane, cur)) = self.find_geometry(at) else { return };
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let s = self.scale;
        let ui = Style { font: self.f.ui, px: self.px(m::UI_PX), color: ink, tracking: 0.0 };
        let label = Style { color: ink, ..self.label() };
        let strong = Style { color: ink, ..self.label_strong() };
        let w = (pane.w - self.px(32.0)).min(self.px(560.0)).max(self.px(260.0).min(pane.w));
        let row = self.px(36.0);
        let h = row * 2.0;
        let margin = self.px(12.0);
        // Clear of a page's own scrollbar.
        let right_margin = if matches!(self.find_pane(at), Some(Pane::Web(_))) { self.px(24.0) } else { margin };
        let x = pane.right() - right_margin - w;
        let high = Rect::new(x, pane.y + margin, w, h);
        let low = Rect::new(x, pane.bottom() - margin - h, w, h);
        // Never over the current match.
        let pad = self.px(8.0);
        let under = |r: Rect| cur.is_some_and(|c| c.x < r.right() + pad && c.right() > r.x - pad && c.y < r.bottom() + pad && c.bottom() > r.y - pad);
        let was_low = self.find_bar.as_ref().is_some_and(|b| b.low);
        let go_low = if was_low { !under(low) && (under(high) || cur.is_some_and(|c| c.bottom() < pane.y + pane.h * 0.5)) } else { under(high) };
        let target = if go_low { low } else { high };
        let motion = self.motion.clone();
        let glide_y = {
            let bar = self.find_bar.as_mut().unwrap();
            let (from, to, at) = bar.y_shown.unwrap_or((target.y, target.y, crate::clock::now()));
            let now = from + (to - from) * eased(&motion, at, 160.0);
            if (to - target.y).abs() >= 0.5 {
                // A new place: from wherever it is now.
                bar.y_shown = Some((now, target.y, crate::clock::now()));
                now
            } else {
                if bar.y_shown.is_none() {
                    bar.y_shown = Some((target.y, target.y, at));
                }
                now
            }
        };
        if (glide_y - target.y).abs() >= 0.5 {
            self.dirty = true;
        }
        let card = Rect::new(target.x, glide_y, target.w, target.h);
        let opened = self.find_bar.as_ref().map(|b| b.opened).unwrap_or_else(crate::clock::now);
        let unroll = eased(&motion, opened, 110.0);
        if unroll < 1.0 {
            self.dirty = true;
        }
        // On a page, only plain text and case: Chromium's find does nothing
        // else, and a half-right search is worse than none.
        let on_page = matches!(self.find_pane(at), Some(Pane::Web(_)));
        let tally = self.find_tally(self.find_bar.as_ref().unwrap().scope);
        let rungs: Vec<(Scope, Status)> = Scope::ALL.iter().map(|&sc| (sc, self.find_tally(sc))).collect();
        let asleep = self.find_asleep(Scope::Window);
        let bar = self.find_bar.as_mut().unwrap();
        bar.low = go_low;
        bar.rect = card;
        bar.hits.clear();
        let (query, case, scope, focused) = (bar.query.clone(), bar.case, bar.scope, bar.focused);
        let (word, regex) = (bar.word, bar.regex);
        let notice = bar.notice.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_secs(3)).map(|(n, _)| n.clone());
        scene.layer(None);
        if unroll < 1.0 {
            scene.layer(Some(Rect::new(card.x - self.px(2.0), card.y - self.px(2.0), card.w + self.px(10.0), (card.h + self.px(10.0)) * unroll)));
        }
        scene.rect(Rect::new(card.x + self.px(6.0), card.y + self.px(6.0), card.w, card.h), fade(ink, 0.35));
        scene.rect(card, paper);
        scene.outline(card, self.px(if focused { m::FLOATING } else { m::STRUCTURE }) , ink);
        scene.hline(card.x, card.y + row, card.w, self.px(m::HAIRLINE), ink);
        let base = card.y + (row + ui.px) * 0.5 - self.px(2.0);
        // Right end of the first row: ✕ ↓ ↑ Aa, then the count.
        let mut right = card.right() - self.px(6.0);
        let cell = self.px(30.0);
        let mut hits = Vec::new();
        for (hit, text) in [(Hit::Close, "✕"), (Hit::Next, "↓"), (Hit::Prev, "↑"), (Hit::Regex, ".*"), (Hit::Word, "ab"), (Hit::Case, "Aa")] {
            let cw = if matches!(hit, Hit::Case | Hit::Word | Hit::Regex) { self.px(32.0) } else { cell };
            right -= cw;
            let r = Rect::new(right, card.y + self.px(4.0), cw, row - self.px(8.0));
            let hot = r.contains(self.mouse.0, self.mouse.1);
            let on = match hit { Hit::Case => case, Hit::Word => word, Hit::Regex => regex, _ => false };
            let off_here = on_page && matches!(hit, Hit::Word | Hit::Regex);
            if on {
                scene.rect(r, if off_here { fade(ink, 0.45) } else { ink });
            } else if hot {
                scene.outline(r, self.px(m::HAIRLINE), ink);
            }
            let st = if on { Style { color: paper, ..ui } } else if off_here { Style { color: fade(ink, 0.45), ..ui } } else { ui };
            let tw = self.fonts.measure(st, text);
            self.fonts.draw(scene, st, r.x + (r.w - tw) * 0.5, base, text);
            hits.push((r, hit));
        }
        let count = if query.is_empty() {
            String::new()
        } else if let Some(e) = tally.error.as_ref() {
            format!("regex: {e}")
        } else if let Some(n) = tally.note.filter(|_| tally.total == 0) {
            n.to_string()
        } else {
            count_words(tally.total, tally.current, tally.done, tally.capped)
        };
        let none = !query.is_empty() && tally.total == 0 && tally.done && tally.note.is_none();
        let fresh = if tally.fresh > 0 { format!("+{} new", tally.fresh) } else { String::new() };
        right -= self.px(10.0);
        if !fresh.is_empty() {
            let fw = self.fonts.measure(label, &fresh) + self.px(10.0);
            right -= fw;
            let r = Rect::new(right, card.y + self.px(9.0), fw, row - self.px(18.0));
            scene.outline(r, self.px(m::HAIRLINE), ink);
            self.fonts.draw(scene, label, r.x + self.px(5.0), base, &fresh);
            right -= self.px(8.0);
        }
        // A new count ticks up into place.
        let tick = {
            let bar = self.find_bar.as_mut().unwrap();
            if bar.count_shown.0 != count {
                bar.count_shown = (count.clone(), crate::clock::now());
            }
            eased(&motion, bar.count_shown.1, 90.0)
        };
        if tick < 1.0 {
            self.dirty = true;
        }
        let lift = (1.0 - tick) * self.px(5.0);
        if !count.is_empty() {
            let cw = self.fonts.measure(strong, &count);
            right -= cw + if none { self.px(10.0) } else { 0.0 };
            if none {
                scene.rect(Rect::new(right, card.y + self.px(9.0), cw + self.px(10.0), row - self.px(18.0)), ink);
                self.fonts.draw(scene, Style { color: paper, ..strong }, right + self.px(5.0), base + lift, &count);
            } else {
                self.fonts.draw(scene, Style { color: fade(ink, 0.35 + 0.65 * tick), ..strong }, right, base + lift, &count);
            }
        }
        // The field: the query's end stays in sight.
        let fx = card.x + self.px(12.0);
        let room = (right - self.px(12.0) - fx).max(self.px(40.0));
        let field = Rect::new(card.x, card.y, right - card.x, row);
        hits.push((field, Hit::Field));
        if query.is_empty() {
            let hint = match self.find_pane(at) {
                Some(Pane::Web(_)) => "Find on this page",
                _ => "Find in this shell",
            };
            self.fonts.draw(scene, Style { color: fade(ink, 0.6), ..ui }, fx, base, hint);
            if focused {
                self.draw_line_caret_on(scene, fx, base, ui.px, 1.0, self.last_key, ink);
            }
        } else {
            let mut shown = query.clone();
            while self.fonts.measure(ui, &shown) > room && shown.chars().count() > 1 {
                shown = format!("…{}", shown.chars().skip(2).collect::<String>());
            }
            let qw = self.fonts.draw(scene, ui, fx, base, &shown);
            if focused {
                self.draw_line_caret_on(scene, fx + qw + self.px(2.0), base, ui.px, 1.0, self.last_key, ink);
            }
        }
        // Second row: the ladder, each rung with its count; then what to
        // say — a notice, where else the word is, or the next key.
        let y2 = card.y + row;
        let base2 = y2 + (row + label.px) * 0.5 - self.px(2.0);
        let mut x2 = card.x;
        // The rungs, then the fill sliding to the chosen one, then their
        // words: each inverts where the fill is under it.
        let mut placed: Vec<(Scope, String, Rect)> = Vec::new();
        for (sc, st) in &rungs {
            let n = if query.is_empty() {
                String::new()
            } else if !st.done && st.total == 0 {
                " …".into()
            } else if st.capped {
                format!(" {}+", thousands(CAP))
            } else {
                format!(" {}", thousands(st.total))
            };
            let text = format!("{}{n}", sc.word());
            let tw = self.fonts.measure(label, &text) + self.px(20.0);
            let r = Rect::new(x2, y2, tw, row);
            placed.push((*sc, text, r));
            x2 = r.right();
        }
        let chosen = placed.iter().find(|(sc, _, _)| *sc == scope).map(|(_, _, r)| *r).unwrap_or(Rect::new(card.x, y2, 0.0, row));
        let fill = match self.find_bar.as_ref().and_then(|b| b.rung_from) {
            Some((from, at)) => {
                let e = eased(&motion, at, 140.0);
                if e < 1.0 {
                    self.dirty = true;
                }
                Rect::new(from.x + (chosen.x - from.x) * e, y2, from.w + (chosen.w - from.w) * e, row)
            }
            None => chosen,
        };
        if let Some(bar) = self.find_bar.as_mut() {
            bar.rung_rect = Some(chosen);
        }
        for (_, _, r) in &placed {
            if r.contains(self.mouse.0, self.mouse.1) && *r != chosen {
                scene.rect(*r, fade(ink, 0.08));
            }
        }
        scene.rect(fill, ink);
        for (sc, text, r) in &placed {
            let mid = r.x + r.w * 0.5;
            let under_fill = mid >= fill.x && mid <= fill.right();
            self.fonts.draw(scene, Style { color: if under_fill { paper } else { ink }, ..label }, r.x + self.px(10.0), base2, text);
            scene.rect(Rect::new(r.right(), y2, self.px(m::HAIRLINE), row), ink);
            hits.push((*r, Hit::Rung(*sc)));
        }
        let wider = if none {
            rungs.iter().find(|(sc, st)| *sc > scope && st.total > 0).map(|(sc, st)| format!("{} IN THIS {} · {}", thousands(st.total), sc.word(), chord(false)))
        } else {
            None
        };
        let hovered_option = hits.iter().find(|(r, h)| matches!(h, Hit::Word | Hit::Regex) && r.contains(self.mouse.0, self.mouse.1)).map(|(_, h)| *h);
        let say = if let Some(n) = notice {
            n.to_uppercase()
        } else if on_page && (word || regex || hovered_option.is_some()) {
            "PAGES: PLAIN TEXT AND CASE (CHROMIUM'S FIND)".to_string()
        } else if let Some(h) = hovered_option {
            if h == Hit::Word { "WHOLE WORDS · ALT+W".into() } else { "REGULAR EXPRESSION · ALT+R".into() }
        } else if let Some(w) = wider {
            w
        } else if asleep > 0 && scope >= Scope::Window && !query.is_empty() {
            format!("+{asleep} ASLEEP, NOT SEARCHED")
        } else if scope < Scope::Nus {
            format!("{} WIDER", chord(false))
        } else {
            format!("{} NARROWER", chord(true))
        };
        let room2 = card.right() - x2 - self.px(20.0);
        let say = self.fit(label, &say, room2);
        let sw = self.fonts.measure(label, &say);
        self.fonts.draw(scene, Style { color: fade(ink, 0.8), ..label }, card.right() - self.px(10.0) - sw, base2, &say);
        let _ = s;
        // NUS: what isn't open, as a list hanging from the bar (above it
        // when the bar sits at the pane's foot).
        let (away, away_done, pick) = self.find_bar.as_ref().map(|b| (b.away.clone(), b.away_done, b.pick)).unwrap_or_default();
        if scope == Scope::Nus && !query.is_empty() {
            let rows: Vec<(String, String)> = if away.is_empty() {
                vec![(if away_done { "Nothing in notes or closed shells".into() } else { "Looking in notes and closed shells…".into() }, String::new())]
            } else {
                away.iter().take(10).map(|a| (a.title.clone(), a.detail.clone())).collect()
            };
            let rh = self.px(30.0);
            let lh = rh * rows.len() as f32 + self.px(8.0);
            let list = if go_low { Rect::new(card.x, card.y - lh - self.px(6.0), card.w, lh) } else { Rect::new(card.x, card.bottom() + self.px(6.0), card.w, lh) };
            scene.rect(Rect::new(list.x + self.px(6.0), list.y + self.px(6.0), list.w, list.h), fade(ink, 0.35));
            scene.rect(list, paper);
            scene.outline(list, self.px(m::STRUCTURE), ink);
            let list_at = {
                let bar = self.find_bar.as_mut().unwrap();
                *bar.list_at.get_or_insert_with(crate::clock::now)
            };
            for (k, (title, detail)) in rows.iter().enumerate() {
                let r = Rect::new(list.x, list.y + self.px(4.0) + rh * k as f32, list.w, rh);
                // Rows unroll one after another, 25 ms apart.
                let e = eased(&motion, list_at + Duration::from_secs_f32(motion.dur(25.0 * k as f32)), 120.0);
                if e < 1.0 {
                    self.dirty = true;
                }
                let saved = scene.clip();
                scene.layer(Some(Rect::new(r.x, r.y, r.w, r.h * e)));
                let real = !away.is_empty();
                let picked = real && pick == Some(k);
                let hot = real && r.contains(self.mouse.0, self.mouse.1);
                if picked {
                    scene.rect(r, ink);
                } else if hot {
                    scene.rect(r, fade(ink, 0.08));
                }
                let fg = if picked { paper } else { ink };
                let b = r.y + (rh + label.px) * 0.5 - self.px(2.0);
                let kind = match away.get(k).map(|a| &a.kind) { Some(AwayKind::Note(..)) => "NOTE", Some(AwayKind::Command { .. }) => "SHELL", None => "" };
                let mut x = r.x + self.px(12.0);
                if !kind.is_empty() {
                    self.fonts.draw(scene, Style { color: fade(fg, 0.7), ..label }, x, b, kind);
                    x += self.px(52.0);
                }
                let tw_room = (r.right() - x - self.px(12.0)) * 0.55;
                let title = self.fit(Style { color: fg, ..strong }, title, tw_room);
                let tw = self.fonts.draw(scene, Style { color: fg, ..strong }, x, b, &title);
                if !detail.is_empty() {
                    let room = r.right() - (x + tw + self.px(12.0)) - self.px(12.0);
                    let d = self.fit(label, detail, room.max(0.0));
                    self.fonts.draw(scene, Style { color: fade(fg, 0.75), ..label }, x + tw + self.px(12.0), b, &d);
                }
                if real {
                    hits.push((r, Hit::Away(k)));
                }
                scene.layer(saved);
            }
        } else if let Some(bar) = self.find_bar.as_mut() {
            bar.list_at = None;
        }
        if let Some(bar) = self.find_bar.as_mut() {
            // The list is part of the bar for the pointer.
            if let Some((r, _)) = hits.iter().filter(|(_, h)| matches!(h, Hit::Away(_))).last() {
                let top = hits.iter().filter(|(_, h)| matches!(h, Hit::Away(_))).map(|(r, _)| r.y).fold(f32::MAX, f32::min);
                let (y0, y1) = (bar.rect.y.min(top), bar.rect.bottom().max(r.bottom()));
                bar.rect = Rect::new(bar.rect.x, y0, bar.rect.w, y1 - y0);
            }
            bar.hits = hits;
        }
        scene.layer(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(text: &str) -> Term {
        let mut t = Term::new(10, 4, 1000);
        t.advance(text.replace('\n', "\r\n").as_bytes());
        t
    }

    fn settle(f: &mut TermFind, t: &Term) {
        f.screen_at = None;
        while f.tick(t, Duration::from_secs(1)) {
            f.screen_at = None;
        }
    }

    #[test]
    fn history_and_screen_are_both_searched_and_the_newest_is_current() {
        let mut t = term("err 1\nok\nerr 2\nok\nok\nok\nerr 3\nok");
        let mut f = TermFind::new("ERR", false, &t);
        settle(&mut f, &t);
        let s = f.status();
        assert_eq!((s.total, s.done), (3, true));
        assert_eq!(s.current, Some(2));
        // Older, older, then the end: the caller decides what wraps.
        assert_eq!(f.step(true), Step::Moved);
        assert_eq!(f.step(true), Step::Moved);
        assert_eq!(f.status().current, Some(0));
        assert_eq!(f.step(true), Step::Wrapped);
        // New output joins the count and does not move the current match.
        t.advance(b"\r\nerr 4\r\n");
        settle(&mut f, &t);
        let s = f.status();
        assert_eq!((s.total, s.current, s.fresh), (4, Some(0), 1));
        assert_eq!(f.enter(true), Step::Moved);
        assert_eq!(f.status().fresh, 0);
    }

    #[test]
    fn a_full_screen_program_is_searched_on_its_screen() {
        let mut t = term("err in history\nok\nok\nok\nok");
        t.advance(b"\x1b[?1049h\x1b[Herr on screen");
        let mut f = TermFind::new("err", false, &t);
        settle(&mut f, &t);
        let s = f.status();
        assert_eq!((s.total, s.note), (1, Some("screen only")));
        t.advance(b"\x1b[?1049l");
        settle(&mut f, &t);
        assert_eq!(f.status().note, None);
    }
}
