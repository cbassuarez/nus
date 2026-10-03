//! Orrery: every window, tab and held shell on one screen, grouped by the
//! place it belongs to (a git root, or the folder a shell sits in). This is
//! the part with no window in it — what a place is, where things go, which
//! letters they answer to — so it can be tested on its own; orrery_ui.rs
//! draws it and takes the keys.
//!
//! The rules come from the research boards: the map always fits one screen
//! (space-filling thumbnails beat panning and zooming for going back to
//! things); places keep their order between visits and only their area
//! changes with use (SCOTZ's stable treemap); a card's size decides how much
//! of it is drawn; and every card keeps the same two letters, so going
//! somewhere becomes a habit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nus_render::Rect;

/// Most places given a zone of their own; the rest are listed by name only.
pub const MAX_ZONES: usize = 9;
/// A card narrower than this shows its live picture: below it, people stop
/// recognising pages from the picture (Kaasten et al. 2002).
pub const LIVE_MIN: f32 = 208.0;
/// Narrower than this, a card is a chip: its letters and its kind.
pub const SNIPPET_MIN: f32 = 96.0;
/// Home-row letters first, then the rows above and below.
const LETTERS: &[u8] = b"asdfghjklqwertyuiopzxcvbnm";

/// What a card is. A card is one tab (its focused pane) or one held shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Shell,
    Page,
    Editor,
    Other,
    Held,
}

/// What state it's in, if any: drawn as a shape and a word, never colour alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    None,
    Running,
    Passed,
    Failed,
    Waiting,
    Playing,
    Asleep,
}

/// One card as its window publishes it. Text is what a person reads;
/// `preview` is the page's own texture (the one the sidebar draws), shared
/// across windows because every window renders on the same GPU device.
#[derive(Clone)]
pub struct Card {
    pub window: u64,
    pub tab: u64,
    /// Held shells have no tab: their holder's id.
    pub held: Option<String>,
    /// A shell in a tab that is attached to a holder: its id, so the
    /// held list doesn't show it twice.
    pub attached: Option<String>,
    pub kind: Kind,
    pub state: State,
    pub title: String,
    /// Written by a person (a page title, a note) or said by a machine (a
    /// command, a file): the first is set in the prose face, the second in
    /// the UI face.
    pub written: bool,
    /// The host, the folder, the file's directory.
    pub detail: String,
    /// The folder this belongs in, when it has one.
    pub cwd: Option<PathBuf>,
    pub preview: Option<std::sync::Arc<wgpu::BindGroup>>,
    /// The page's width over its height, for cropping its picture.
    pub aspect: f32,
    /// A shell's last lines, newest last.
    pub lines: Vec<String>,
    /// The window's active tab.
    pub active: bool,
    /// Seconds since it was last shown.
    pub idle: f32,
    /// The page's paint count: a new frame means a new picture.
    pub paints: u64,
}

impl Card {
    /// Text that typing is matched against.
    pub fn haystack(&self) -> String {
        let mut s = String::with_capacity(64);
        s.push_str(&self.title);
        s.push(' ');
        s.push_str(&self.detail);
        if let Some(c) = &self.cwd {
            s.push(' ');
            s.push_str(&c.to_string_lossy());
        }
        for l in &self.lines {
            s.push(' ');
            s.push_str(l);
        }
        s.to_lowercase()
    }

    pub fn key(&self) -> CardKey {
        match &self.held {
            Some(h) => CardKey::Held(h.clone()),
            None => CardKey::Tab(self.window, self.tab),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CardKey {
    Tab(u64, u64),
    Held(String),
    /// A place with nothing open: going there opens it.
    Place(String),
    /// Last session, not yet brought back.
    Restore,
}

/// One window as the others see it, with its cards in tab order.
#[derive(Clone)]
pub struct WindowCards {
    pub id: u64,
    pub ordinal: usize,
    pub name: String,
    pub cards: Vec<Card>,
}

/// A fingerprint of what the map shows: when it changes, a window
/// showing the map draws again (a page painted, a command finished).
pub fn signature(world: &[WindowCards]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for w in world {
        w.id.hash(&mut h);
        w.name.hash(&mut h);
        for c in &w.cards {
            c.tab.hash(&mut h);
            c.title.hash(&mut h);
            c.detail.hash(&mut h);
            (c.state as u8).hash(&mut h);
            c.paints.hash(&mut h);
            c.active.hash(&mut h);
            c.lines.last().hash(&mut h);
            c.lines.len().hash(&mut h);
        }
    }
    h.finish()
}

// ── Places ───────────────────────────────────────────────────────────────

/// Where a folder belongs: the git root above it, or the folder itself.
/// Home and the filesystem root belong nowhere. Only ancestors are looked
/// at — nothing scans the disk — and answers are kept.
#[derive(Default)]
pub struct Roots {
    seen: HashMap<PathBuf, Option<PathBuf>>,
}

impl Roots {
    pub fn place_of(&mut self, dir: &Path) -> Option<PathBuf> {
        if let Some(p) = self.seen.get(dir) {
            return p.clone();
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        let mut found = None;
        for (depth, a) in dir.ancestors().enumerate() {
            if depth > 24 || home.as_deref() == Some(a) || a.parent().is_none() {
                break;
            }
            if a.join(".git").exists() {
                found = Some(a.to_path_buf());
                break;
            }
        }
        let place = found.or_else(|| {
            let lone = home.as_deref() == Some(dir) || dir.parent().is_none() || std::env::temp_dir() == dir;
            (!lone).then(|| dir.to_path_buf())
        });
        if self.seen.len() > 4096 {
            self.seen.clear();
        }
        self.seen.insert(dir.to_path_buf(), place.clone());
        place
    }
}

/// A place's name: its folder's last part.
pub fn place_name(key: &str) -> String {
    if key.is_empty() {
        return "loose".into();
    }
    Path::new(key).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| key.to_string())
}

/// What the map remembers between visits: places in the order they were
/// first seen (the order never changes, so a place stays where it was) and
/// how much each is used (which only changes its area).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    pub places: Vec<Remembered>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Remembered {
    pub key: String,
    #[serde(default)]
    pub uses: u32,
    /// Unix seconds when last gone to.
    #[serde(default)]
    pub last: u64,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Memory {
    fn path() -> PathBuf {
        std::env::current_dir().unwrap_or_default().join("profile").join("orrery.json")
    }

    /// The saved memory, or a fresh one: a missing, damaged or foreign
    /// file never stops the map from opening.
    pub fn load() -> Memory {
        crate::protected_state::read(&Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(d) = Self::path().parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = crate::protected_state::write_json(&Self::path(), self);
    }

    /// Learn places that are open now; returns whether anything was new.
    pub fn learn<'a>(&mut self, keys: impl Iterator<Item = &'a str>) -> bool {
        let mut changed = false;
        for k in keys {
            if !k.is_empty() && !self.places.iter().any(|p| p.key == k) {
                self.places.push(Remembered { key: k.to_string(), uses: 0, last: unix_now() });
                changed = true;
            }
        }
        // Bounded: the least used, longest ago, go first.
        if self.places.len() > 64 {
            let mut idx: Vec<usize> = (0..self.places.len()).collect();
            idx.sort_by_key(|&i| (self.places[i].uses, self.places[i].last));
            let drop: std::collections::HashSet<usize> = idx.into_iter().take(self.places.len() - 64).collect();
            let mut i = 0;
            self.places.retain(|_| {
                let keep = !drop.contains(&i);
                i += 1;
                keep
            });
        }
        changed
    }

    pub fn went(&mut self, key: &str) {
        if let Some(p) = self.places.iter_mut().find(|p| p.key == key) {
            p.uses = p.uses.saturating_add(1);
            p.last = unix_now();
        }
    }

    /// How much a place is used, weighed for its age: frecency, 0 and up.
    pub fn frecency(&self, key: &str) -> f32 {
        let Some(p) = self.places.iter().find(|p| p.key == key) else { return 0.0 };
        let days = unix_now().saturating_sub(p.last) as f32 / 86_400.0;
        (1.0 + p.uses as f32).ln() / (1.0 + days / 7.0)
    }

    pub fn order(&self, key: &str) -> usize {
        self.places.iter().position(|p| p.key == key).unwrap_or(usize::MAX)
    }
}

// ── The map ──────────────────────────────────────────────────────────────

/// A place on the map: its windows (each with its cards) and its held shells.
#[derive(Clone)]
pub struct Zone {
    /// The place's folder as text; empty for loose.
    pub key: String,
    pub name: String,
    pub windows: Vec<WindowCards>,
    pub held: Vec<Card>,
    /// Which colour of the theme's six it wears.
    pub hue: usize,
}

impl Zone {
    pub fn cards(&self) -> impl Iterator<Item = &Card> {
        self.windows.iter().flat_map(|w| w.cards.iter()).chain(self.held.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty() && self.held.is_empty()
    }
}

/// Group the windows and held shells by place. A window goes to the place
/// most of its shells and files are in (the active tab breaks a tie); a
/// window of only pages is loose. Places remembered but with nothing open
/// get a zone too — going there opens them — up to MAX_ZONES in all.
pub fn zones(windows: &[WindowCards], held: &[Card], roots: &mut Roots, memory: &Memory) -> Vec<Zone> {
    let mut by: HashMap<String, Zone> = HashMap::new();
    // Every place something open is in, even a card inside another place's window.
    let mut open: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in windows.iter().flat_map(|w| w.cards.iter()).chain(held.iter()) {
        if let Some(p) = c.cwd.as_deref().and_then(|d| roots.place_of(d)) {
            open.insert(p.to_string_lossy().into_owned());
        }
    }
    let zone = |key: String| -> Zone {
        Zone { name: place_name(&key), key, windows: Vec::new(), held: Vec::new(), hue: 0 }
    };
    for w in windows {
        let mut count: HashMap<String, (usize, bool)> = HashMap::new();
        for c in &w.cards {
            if let Some(p) = c.cwd.as_deref().and_then(|d| roots.place_of(d)) {
                let e = count.entry(p.to_string_lossy().into_owned()).or_default();
                e.0 += 1;
                e.1 |= c.active;
            }
        }
        let key = count.into_iter().max_by(|a, b| (a.1 .0, a.1 .1, &b.0).cmp(&(b.1 .0, b.1 .1, &a.0))).map(|(k, _)| k).unwrap_or_default();
        by.entry(key.clone()).or_insert_with(|| zone(key)).windows.push(w.clone());
    }
    for h in held {
        let key = h.cwd.as_deref().and_then(|d| roots.place_of(d)).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        by.entry(key.clone()).or_insert_with(|| zone(key)).held.push(h.clone());
    }
    // Live places in remembered order, loose last; then remembered places
    // with nothing open, most used first, while there is room.
    let mut live: Vec<Zone> = by.into_values().collect();
    live.sort_by_key(|z| if z.key.is_empty() { (1, usize::MAX) } else { (0, memory.order(&z.key)) });
    let room = MAX_ZONES.saturating_sub(live.len());
    let mut idle: Vec<&Remembered> = memory.places.iter().filter(|p| !live.iter().any(|z| z.key == p.key) && !open.contains(&p.key) && Path::new(&p.key).is_dir()).collect();
    idle.sort_by(|a, b| memory.frecency(&b.key).total_cmp(&memory.frecency(&a.key)));
    let mut out = live;
    let loose = out.iter().position(|z| z.key.is_empty()).map(|i| out.remove(i));
    for p in idle.into_iter().take(room) {
        out.push(zone(p.key.clone()));
    }
    // Remembered order again, so an idle place sits where it always has.
    out.sort_by_key(|z| memory.order(&z.key));
    out.extend(loose);
    for z in &mut out {
        z.hue = if z.key.is_empty() { usize::MAX } else { memory.order(&z.key) % 6 };
    }
    out
}

/// A zone's share of the screen: what it holds, lifted a little by use.
/// An empty zone stays small; nothing grows past what it has to show.
pub fn weight(z: &Zone, memory: &Memory) -> f32 {
    let cards = z.cards().count() as f32;
    if cards == 0.0 {
        return 0.45;
    }
    (1.0 + cards).sqrt() * (1.0 + 0.25 * memory.frecency(&z.key).min(3.0))
}

/// An ordered treemap (Bederson's strip layout): items keep their order,
/// laid in rows, each row closed when adding another item would make its
/// cells less square. Stable: the same order and weights give the same
/// rects, and a small change in one weight moves its neighbours a little.
pub fn treemap(weights: &[f32], r: Rect, gap: f32) -> Vec<Rect> {
    let n = weights.len();
    if n == 0 {
        return Vec::new();
    }
    let total: f32 = weights.iter().map(|w| w.max(0.01)).sum();
    let area = r.w * r.h;
    let worst = |row: &[f32], h_row: f32| -> f32 {
        row.iter()
            .map(|&w| {
                let cw = w / total * area / h_row;
                (cw / h_row).max(h_row / cw)
            })
            .fold(0.0, f32::max)
    };
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    for i in 0..n {
        let with: Vec<f32> = cur.iter().chain(std::iter::once(&i)).map(|&k| weights[k].max(0.01)).collect();
        let without: Vec<f32> = cur.iter().map(|&k| weights[k].max(0.01)).collect();
        let h_with = with.iter().sum::<f32>() / total * area / r.w;
        let h_without = without.iter().sum::<f32>() / total * area / r.w;
        if !cur.is_empty() && worst(&with, h_with) > worst(&without, h_without) {
            rows.push(std::mem::take(&mut cur));
        }
        cur.push(i);
    }
    rows.push(cur);
    let mut out = vec![Rect::new(0.0, 0.0, 0.0, 0.0); n];
    let mut y = r.y;
    for (ri, row) in rows.iter().enumerate() {
        let sum: f32 = row.iter().map(|&k| weights[k].max(0.01)).sum();
        let h = if ri + 1 == rows.len() { r.bottom() - y } else { (sum / total * r.h).round() };
        let mut x = r.x;
        for (ci, &k) in row.iter().enumerate() {
            let w = if ci + 1 == row.len() { r.right() - x } else { (weights[k].max(0.01) / sum * r.w).round() };
            out[k] = Rect::new(x, y, (w - gap).max(0.0), (h - gap).max(0.0));
            x += w;
        }
        y += h;
    }
    out
}

/// `n` equal cards in `r`, in reading order, as wide as they can be at
/// about 16:10. Returns the rects and how many fit; the rest are counted.
pub fn grid(n: usize, r: Rect, gap: f32, min_w: f32) -> (Vec<Rect>, usize) {
    if n == 0 || r.w <= 0.0 || r.h <= 0.0 {
        return (Vec::new(), 0);
    }
    let mut best = (1usize, f32::MIN);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let cw = (r.w - gap * (cols as f32 - 1.0)) / cols as f32;
        let ch = (r.h - gap * (rows as f32 - 1.0)) / rows as f32;
        if cw <= 0.0 || ch <= 0.0 {
            continue;
        }
        let score = cw.min(ch * 1.6);
        if score > best.1 {
            best = (cols, score);
        }
    }
    let cols = best.0;
    let mut rows = n.div_ceil(cols);
    let mut fit = n;
    let ch_of = |rows: usize| (r.h - gap * (rows as f32 - 1.0)) / rows as f32;
    let cw = (r.w - gap * (cols as f32 - 1.0)) / cols as f32;
    // Too many to be anything but slivers: fill what fits, count the rest.
    while rows > 1 && ch_of(rows) < min_w * 0.36 {
        rows -= 1;
        fit = (cols * rows).saturating_sub(1).max(1);
    }
    let ch = ch_of(rows);
    let rects = (0..fit)
        .map(|i| {
            let (c, rr) = (i % cols, i / cols);
            Rect::new((r.x + c as f32 * (cw + gap)).round(), (r.y + rr as f32 * (ch + gap)).round(), cw.floor(), ch.floor())
        })
        .collect();
    (rects, fit)
}

/// How much of a card is drawn at this width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Live,
    Snippet,
    Chip,
}

pub fn form(w: f32, scale: f32) -> Form {
    if w >= LIVE_MIN * scale {
        Form::Live
    } else if w >= SNIPPET_MIN * scale {
        Form::Snippet
    } else {
        Form::Chip
    }
}

/// A place's letter and a card's: the first picks the place, the second
/// the card within it, and neither changes while the order doesn't. A
/// place with nothing open answers to its letter alone.
pub fn letters(zone: usize, card: Option<usize>) -> String {
    let a = LETTERS[zone % LETTERS.len()] as char;
    match card {
        None => a.to_string(),
        Some(c) => {
            // A place's own letter is never its cards' second, so "gg" can't be mistaken.
            let pool: Vec<u8> = LETTERS.iter().copied().filter(|&l| l as char != a).collect();
            format!("{a}{}", pool[c % pool.len()] as char)
        }
    }
}

/// The card nearest `from` in a direction: the one ahead whose centre is
/// closest, with sideways distance counting double, so ↓ goes down a
/// column before it jumps across.
pub fn nearest(rects: &[Rect], from: usize, dx: i32, dy: i32) -> Option<usize> {
    let a = rects.get(from)?;
    let (ax, ay) = (a.x + a.w / 2.0, a.y + a.h / 2.0);
    rects
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != from)
        .filter_map(|(i, b)| {
            let (bx, by) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
            let (along, across) = if dx != 0 { ((bx - ax) * dx as f32, (by - ay).abs()) } else { ((by - ay) * dy as f32, (bx - ax).abs()) };
            (along > 1.0).then_some((i, along + 2.0 * across))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(window: u64, tab: u64, cwd: Option<&str>, active: bool) -> Card {
        Card {
            window,
            tab,
            held: None,
            attached: None,
            kind: if cwd.is_some() { Kind::Shell } else { Kind::Page },
            state: State::None,
            title: format!("t{tab}"),
            written: cwd.is_none(),
            detail: String::new(),
            cwd: cwd.map(PathBuf::from),
            preview: None,
            aspect: 1.6,
            lines: Vec::new(),
            active,
            idle: 0.0,
            paints: 0,
        }
    }

    #[test]
    fn treemap_fills_the_rect_in_order_and_is_stable() {
        let r = Rect::new(0.0, 0.0, 1000.0, 600.0);
        let w = [5.0, 3.0, 2.0, 1.0, 1.0];
        let a = treemap(&w, r, 0.0);
        assert_eq!(a, treemap(&w, r, 0.0));
        let area: f32 = a.iter().map(|q| q.w * q.h).sum();
        assert!((area - 600_000.0).abs() < 6_000.0, "{area}");
        // Reading order: each item starts at or after the one before it.
        for p in a.windows(2) {
            assert!(p[1].y > p[0].y || (p[1].y == p[0].y && p[1].x > p[0].x));
        }
        // Bigger weight, bigger area.
        assert!(a[0].w * a[0].h > a[4].w * a[4].h);
        // A small change in one weight keeps the order of the rest.
        let b = treemap(&[5.0, 3.0, 2.2, 1.0, 1.0], r, 0.0);
        assert!(b[0].x == a[0].x && b[0].y == a[0].y);
    }

    #[test]
    fn grid_prefers_wide_cards_and_counts_what_does_not_fit() {
        let (g, fit) = grid(4, Rect::new(0.0, 0.0, 800.0, 500.0), 8.0, 96.0);
        assert_eq!(fit, 4);
        assert!(g.iter().all(|r| r.w > 300.0));
        let (g, fit) = grid(200, Rect::new(0.0, 0.0, 300.0, 200.0), 8.0, 96.0);
        assert!(fit < 200 && fit == g.len() && fit >= 1);
    }

    #[test]
    fn forms_follow_the_recognition_thresholds() {
        assert_eq!(form(240.0, 1.0), Form::Live);
        assert_eq!(form(150.0, 1.0), Form::Snippet);
        assert_eq!(form(80.0, 1.0), Form::Chip);
        assert_eq!(form(300.0, 2.0), Form::Snippet);
    }

    #[test]
    fn letters_are_stable_and_unambiguous() {
        assert_eq!(letters(0, None), "a");
        assert_eq!(letters(0, Some(0)), "as");
        assert_eq!(letters(1, Some(0)), "sa");
        assert_eq!(letters(2, Some(3)), letters(2, Some(3)));
        for z in 0..MAX_ZONES {
            let mut seen = std::collections::HashSet::new();
            for c in 0..25 {
                let l = letters(z, Some(c));
                assert_eq!(l.len(), 2);
                assert_ne!(&l[..1], &l[1..]);
                assert!(seen.insert(l));
            }
        }
    }

    #[test]
    fn windows_go_to_the_place_most_of_their_shells_are_in() {
        let base = std::env::temp_dir().join(format!("nus-orrery-{}", std::process::id()));
        let repo = base.join("repo");
        let other = base.join("other");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let (r, s, o) = (repo.to_str().unwrap(), repo.join("src"), other.to_str().unwrap());
        let windows = vec![
            WindowCards { id: 1, ordinal: 0, name: "1".into(), cards: vec![card(1, 1, Some(r), false), card(1, 2, Some(s.to_str().unwrap()), false), card(1, 3, Some(o), true), card(1, 4, None, false)] },
            WindowCards { id: 2, ordinal: 1, name: "2".into(), cards: vec![card(2, 5, None, true)] },
        ];
        let mut memory = Memory::default();
        let mut roots = Roots::default();
        let z = zones(&windows, &[], &mut roots, &memory);
        let names: Vec<&str> = z.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(names, vec!["repo", "loose"]);
        assert_eq!(z[0].windows[0].id, 1);
        assert_eq!(z[1].windows[0].id, 2);
        // Remembered, nothing open, and still a folder: a zone of its own, in order.
        memory.learn([o].into_iter());
        memory.learn([r].into_iter());
        let z = zones(&windows, &[], &mut roots, &memory);
        let names: Vec<&str> = z.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(names, vec!["repo", "loose"], "other has a shell open in repo's window: no empty zone for it");
        let lone = vec![windows[1].clone()];
        let z = zones(&lone, &[], &mut roots, &memory);
        let names: Vec<&str> = z.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(names, vec!["other", "repo", "loose"]);
        assert!(z[0].is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn home_and_root_belong_nowhere() {
        let mut roots = Roots::default();
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(roots.place_of(Path::new(&home)), None);
        }
        assert_eq!(roots.place_of(Path::new("/")), None);
    }

    #[test]
    fn memory_survives_a_damaged_file_and_stays_bounded() {
        let mut m = Memory::default();
        let keys: Vec<String> = (0..80).map(|i| format!("/p{i}")).collect();
        m.learn(keys.iter().map(|s| s.as_str()));
        assert!(m.places.len() <= 64);
        m.went("/p79");
        assert!(m.frecency("/p79") > m.frecency("/p78"));
        let back: Result<Memory, _> = serde_json::from_str("{\"places\":[{\"key\":\"/x\"}]}");
        assert_eq!(back.unwrap().places[0].uses, 0);
    }

    #[test]
    fn arrows_move_spatially() {
        let r = |x: f32, y: f32| Rect::new(x, y, 100.0, 60.0);
        let rects = [r(0.0, 0.0), r(120.0, 0.0), r(0.0, 80.0), r(120.0, 80.0)];
        assert_eq!(nearest(&rects, 0, 1, 0), Some(1));
        assert_eq!(nearest(&rects, 0, 0, 1), Some(2));
        assert_eq!(nearest(&rects, 3, -1, 0), Some(2));
        assert_eq!(nearest(&rects, 0, -1, 0), None);
    }
}
