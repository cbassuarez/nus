//! A page's address, edited where it is: the boxed field in the page's
//! header. ⌘L / Ctrl+L or a click on it selects the whole address in
//! place, and suggestions hang from the field, edge to edge, inside the
//! pane: the address itself, a search for what was typed, history, open
//! ports. ↵ goes, ⌘↵ (Ctrl/Alt+↵ elsewhere) opens a tab, ⇧↵ a window, Esc
//! puts the address back. ⌘K stays the palette for everything else.
//!
//! Parts, only a Tab away: while editing, Tab selects the next part of
//! the address in the field itself (the host, the port, each step of the
//! path, each query parameter) and ⇧Tab the one before. The list becomes
//! that part's alternatives, written as whole addresses with only the part
//! that changes in ink, so each row says where it lands. Typing replaces
//! the part; Esc goes back to the whole address. Alternatives come from
//! what nus already knows, never the network: the ports something is
//! listening on, and addresses in history that differ only in that part.

use nus_render::text::Style;
use nus_render::theme::metric as m;
use nus_render::Rect;
use winit::keyboard::{Key as WKey, NamedKey};

use crate::app::{App, KeyIn, Pane, WebPane};
use crate::browser::SharedRef;
use crate::field::Cursor;

/// How long an address may grow.
const ROOM: usize = 8192;
/// Rows of history under the whole address.
const HISTORY: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Host,
    Port,
    Path,
    Query,
}

/// One part of an address, in characters, start..end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// The parts of an address that can be changed one at a time. The scheme
/// and the fragment are not parts; empty path steps are skipped.
pub fn parts(line: &str) -> Vec<Part> {
    let chars = |b: usize| line[..b].chars().count();
    let mut out = Vec::new();
    let host_at = line.find("://").map_or(0, |i| i + 3);
    let rest = &line[host_at..];
    let host_end = host_at + rest.find([':', '/', '?', '#']).unwrap_or(rest.len());
    if host_end > host_at {
        out.push(Part { start: chars(host_at), end: chars(host_end), kind: Kind::Host });
    }
    let mut at = host_end;
    if line[at..].starts_with(':') {
        let from = at + 1;
        let end = from + line[from..].find(['/', '?', '#']).unwrap_or(line.len() - from);
        if end > from {
            out.push(Part { start: chars(from), end: chars(end), kind: Kind::Port });
        }
        at = end;
    }
    let path_end = at + line[at..].find(['?', '#']).unwrap_or(line.len() - at);
    let mut step = at;
    for seg in line[at..path_end].split('/') {
        if !seg.is_empty() {
            out.push(Part { start: chars(step), end: chars(step + seg.len()), kind: Kind::Path });
        }
        step += seg.len() + 1;
    }
    if line[path_end..].starts_with('?') {
        let from = path_end + 1;
        let end = from + line[from..].find('#').unwrap_or(line.len() - from);
        let mut p = from;
        for param in line[from..end].split('&') {
            if !param.is_empty() {
                out.push(Part { start: chars(p), end: chars(p + param.len()), kind: Kind::Query });
            }
            p += param.len() + 1;
        }
    }
    out
}

/// The text of a part.
pub fn text_of(line: &str, part: &Part) -> String {
    line.chars().skip(part.start).take(part.end - part.start).collect()
}

/// `line` with `part` replaced by `with`.
pub fn swap(line: &str, part: &Part, with: &str) -> String {
    let mut out: String = line.chars().take(part.start).collect();
    out.push_str(with);
    out.extend(line.chars().skip(part.end));
    out
}

/// An address without its scheme, as it is shown.
fn bare(url: &str) -> &str {
    url.find("://").map_or(url, |i| &url[i + 3..])
}

/// Alternatives for part `i` of `line` among `known` addresses: those with
/// the same parts, equal everywhere but this one. Most visited first.
pub fn alternatives<'a>(line: &str, i: usize, known: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mine = parts(line);
    let Some(part) = mine.get(i) else { return Vec::new() };
    let own = text_of(line, part);
    let template = swap(bare(line), &shift(part, line), "\u{0}");
    let mut found: Vec<String> = Vec::new();
    for url in known {
        let theirs = parts(url);
        if theirs.len() != mine.len() || theirs.iter().zip(&mine).any(|(a, b)| a.kind != b.kind) {
            continue;
        }
        let other = &theirs[i];
        if swap(bare(url), &shift(other, url), "\u{0}") != template {
            continue;
        }
        let alt = text_of(url, other);
        if alt != own && !found.contains(&alt) {
            found.push(alt);
        }
    }
    found
}

/// A part, counted from after the scheme (as `bare` shows the address).
fn shift(part: &Part, line: &str) -> Part {
    let off = line.find("://").map_or(0, |i| line[..i + 3].chars().count());
    Part { start: part.start - off, end: part.end - off, ..*part }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Go,
    Search,
    History,
    Port,
}

/// A suggestion: where it goes, how it reads, and what is lit in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub glyph: Glyph,
    pub url: String,
    /// What the row says: an address without its scheme, or a search.
    pub text: String,
    /// Characters of `text` in ink; the rest dim. None: the host is lit.
    pub lit: Option<(usize, usize)>,
    pub tag: String,
}

/// The address being edited, and the pane it belongs to.
pub struct Address {
    pub shared: SharedRef,
    pub line: String,
    pub cur: Cursor,
    pub original: String,
    /// The part Tab selected, by index into `parts(&line)`.
    pub part: Option<usize>,
    pub sel: usize,
    pub rows: Vec<Row>,
    /// Where the field and the rows were last drawn, for the pointer.
    pub field: Rect,
    pub hits: Vec<Rect>,
    /// Characters scrolled off the field's left edge.
    pub scroll: usize,
}

impl App {
    /// ⌘L, Ctrl+L or a click on a page's address: edit it in place. `right`
    /// names the pane; None is the focused one. Not a page: the palette.
    pub(crate) fn edit_address(&mut self, right: Option<bool>) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let right = right.unwrap_or(tab.focus_right && tab.right.is_some());
        let pane = if right { tab.right.as_ref() } else { Some(&tab.left) };
        let Some(Pane::Web(w)) = pane else {
            return self.open_palette(crate::app::PaletteMode::Url);
        };
        if w.bare {
            return self.open_palette(crate::app::PaletteMode::Url);
        }
        let shared = w.tab.shared.clone();
        let url = w.asleep.clone().unwrap_or_else(|| shared.borrow().url.clone());
        // Ports something else is serving: never nus's own (its instance,
        // DevTools), by process or by its executable's name.
        let own = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        self.ports = nus_pty::listening_ports().into_iter()
            .filter(|p| p.port >= 1024 && p.pid != std::process::id() && own.as_deref() != Some(p.process.to_lowercase().as_str()))
            .collect();
        let mut cur = Cursor::default();
        cur.select_all(&url);
        let mut a = Address { shared, line: url.clone(), cur, original: url, part: None, sel: 0, rows: Vec::new(), field: Rect::new(0.0, 0.0, 0.0, 0.0), hits: Vec::new(), scroll: 0 };
        a.rows = self.address_rows(&a);
        self.address = Some(a);
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.focus_right = right;
        }
        self.dirty = true;
    }

    /// What the list offers for the address as it stands.
    fn address_rows(&self, a: &Address) -> Vec<Row> {
        if let Some(i) = a.part {
            return self.part_rows(&a.line, i);
        }
        let mut rows = Vec::new();
        let typed = a.line.trim();
        let unchanged = a.line == a.original;
        if !typed.is_empty() {
            let (url, _) = self.url_or_search(typed);
            let search = self.behavior.prompt.search_url(typed);
            let go = Row { glyph: Glyph::Go, text: bare(&url).trim_end_matches('/').to_string(), url: url.clone(), lit: None, tag: if unchanged { "reload".into() } else { "go".into() } };
            if url == search {
                rows.push(Row { glyph: Glyph::Search, url: search, text: typed.to_string(), lit: Some((0, typed.chars().count())), tag: "search".into() });
            } else {
                rows.push(go);
                if !unchanged {
                    rows.push(Row { glyph: Glyph::Search, url: search, text: typed.to_string(), lit: Some((0, typed.chars().count())), tag: "search".into() });
                }
            }
        }
        let q = if unchanged { String::new() } else { typed.to_lowercase() };
        for r in self.history_rows(&q, false, HISTORY + 1) {
            let crate::app::Action::OpenInPane(url) = r.action else { continue };
            if url == a.original || rows.iter().any(|x| x.url == url) || rows.len() >= 1 + HISTORY {
                continue;
            }
            rows.push(Row { glyph: Glyph::History, text: bare(&url).trim_end_matches('/').to_string(), url, lit: None, tag: "history".into() });
        }
        for p in &self.ports {
            let local = format!("localhost:{}", p.port);
            if (q.is_empty() || local.contains(&q) || p.process.to_lowercase().contains(&q)) && !a.original.contains(&local) {
                let url = format!("http://{local}/");
                if rows.iter().all(|x| x.url != url) {
                    rows.push(Row { glyph: Glyph::Port, url, text: local, lit: None, tag: format!("port · {}", p.process.to_lowercase()) });
                }
            }
            if rows.len() >= 8 {
                break;
            }
        }
        rows
    }

    /// A part's alternatives, as whole addresses with that part lit.
    fn part_rows(&self, line: &str, i: usize) -> Vec<Row> {
        let all = parts(line);
        let Some(part) = all.get(i) else { return Vec::new() };
        let shown = |url: &str, p: &Part| {
            let p = shift(p, url);
            (bare(url).to_string(), Some((p.start, p.end)))
        };
        let mut rows = Vec::new();
        if part.kind == Kind::Port || (part.kind == Kind::Host && matches!(text_of(line, part).as_str(), "localhost" | "127.0.0.1")) {
            let own = all.iter().find(|p| p.kind == Kind::Port).map(|p| text_of(line, p));
            for p in &self.ports {
                let port = p.port.to_string();
                if own.as_deref() == Some(port.as_str()) {
                    continue;
                }
                let url = match all.iter().find(|q| q.kind == Kind::Port) {
                    Some(pp) => swap(line, pp, &port),
                    None => swap(line, part, &format!("{}:{port}", text_of(line, part))),
                };
                let lit = parts(&url).into_iter().find(|q| q.kind == Kind::Port);
                let (text, lit) = match lit { Some(l) => shown(&url, &l), None => (bare(&url).to_string(), None) };
                rows.push(Row { glyph: Glyph::Port, url, text, lit, tag: p.process.to_lowercase() });
            }
            return rows;
        }
        let known: Vec<&str> = self.recent.iter().filter_map(|r| match &r.item {
            crate::start::Saved::Page { url, .. } => Some(url.as_str()),
            _ => None,
        }).collect();
        for alt in alternatives(line, i, known.into_iter()).into_iter().take(8) {
            let url = swap(line, part, &alt);
            let lit = parts(&url).get(i).copied();
            let (text, lit) = match lit { Some(l) => shown(&url, &l), None => (bare(&url).to_string(), None) };
            rows.push(Row { glyph: Glyph::History, url, text, lit, tag: "visited".into() });
        }
        rows
    }

    /// The pane being edited, if it is still there.
    fn address_pane(&mut self) -> Option<&mut WebPane> {
        let shared = self.address.as_ref()?.shared.clone();
        let tab = self.tabs.get_mut(self.active)?;
        std::iter::once(&mut tab.left).chain(tab.right.as_mut()).find_map(|p| match p {
            Pane::Web(w) if std::rc::Rc::ptr_eq(&w.tab.shared, &shared) => Some(w),
            _ => None,
        })
    }

    /// Whether `w` is the page whose address is being edited.
    pub(crate) fn editing_address_of(&self, w: &WebPane) -> bool {
        self.address.as_ref().is_some_and(|a| std::rc::Rc::ptr_eq(&a.shared, &w.tab.shared))
    }

    /// Put the address back and stop editing.
    pub(crate) fn close_address(&mut self) {
        if self.address.take().is_some() {
            self.dirty = true;
        }
    }

    /// Go where row `row` says, or to the line as typed when there is none.
    fn address_commit(&mut self, row: Option<usize>) {
        let Some(a) = self.address.as_ref() else { return };
        let url = match row.and_then(|i| a.rows.get(i)) {
            Some(r) => r.url.clone(),
            None if a.line.trim().is_empty() => return self.close_address(),
            None => self.url_or_search(a.line.trim()).0,
        };
        use crate::prompt::Destination;
        match crate::prompt::destination(self.mods, cfg!(target_os = "macos")) {
            Destination::Window => {
                self.close_address();
                self.new_window_urls.push(url);
                self.new_window_request = true;
            }
            Destination::Tab => {
                self.close_address();
                self.open_url(&url, true);
            }
            Destination::Current => {
                if let Some(w) = self.address_pane() {
                    w.tab.load(&url);
                    w.tab.focus(true);
                }
                self.close_address();
            }
        }
        self.dirty = true;
    }

    /// Select part `i` in the field and offer its alternatives; None is the
    /// whole address again.
    fn address_part(&mut self, i: Option<usize>) {
        let Some(mut a) = self.address.take() else { return };
        a.part = i;
        match i.and_then(|i| parts(&a.line).get(i).copied()) {
            Some(p) => {
                a.cur.move_to(&a.line, p.start, false);
                a.cur.move_to(&a.line, p.end, true);
            }
            None => {
                a.part = None;
                a.cur.select_all(&a.line);
            }
        }
        a.sel = 0;
        a.rows = self.address_rows(&a);
        self.address = Some(a);
        self.dirty = true;
    }

    /// Keys while an address is being edited. True when consumed.
    pub(crate) fn address_key(&mut self, ev: &KeyIn) -> bool {
        if self.address.is_none() {
            return false;
        }
        if self.address_pane().is_none() {
            self.close_address();
            return false;
        }
        if ev.state != winit::event::ElementState::Pressed {
            return true;
        }
        let shift = self.mods.shift_key();
        let mods = self.mods;
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                if self.address.as_ref().is_some_and(|a| a.part.is_some()) {
                    self.address_part(None);
                } else {
                    self.close_address();
                }
                return true;
            }
            WKey::Named(NamedKey::Enter) => {
                let sel = self.address.as_ref().map(|a| a.sel);
                self.address_commit(sel);
                return true;
            }
            WKey::Named(NamedKey::Tab) => {
                let Some(a) = self.address.as_ref() else { return true };
                let n = parts(&a.line).len();
                let next = match (a.part, shift) {
                    (None, false) if n > 0 => Some(0),
                    (None, true) if n > 0 => Some(n - 1),
                    (Some(i), false) if i + 1 < n => Some(i + 1),
                    (Some(i), true) if i > 0 => Some(i - 1),
                    _ => None,
                };
                self.address_part(next);
                return true;
            }
            WKey::Named(NamedKey::ArrowDown) => {
                if let Some(a) = self.address.as_mut() {
                    if a.sel + 1 < a.rows.len() {
                        a.sel += 1;
                    }
                }
                self.dirty = true;
                return true;
            }
            WKey::Named(NamedKey::ArrowUp) => {
                if let Some(a) = self.address.as_mut() {
                    a.sel = a.sel.saturating_sub(1);
                }
                self.dirty = true;
                return true;
            }
            _ => {}
        }
        let Some(mut a) = self.address.take() else { return true };
        let took = crate::field::edit_at(&mut a.line, &mut a.cur, ev, mods, ROOM);
        if took.changed() {
            // Typing in a part keeps to the part: it is still the one at the caret.
            if a.part.is_some() {
                let at = a.cur.at(&a.line);
                a.part = parts(&a.line).iter().position(|p| p.start <= at && at <= p.end);
            }
            a.sel = 0;
            a.rows = self.address_rows(&a);
        }
        self.address = Some(a);
        self.dirty = true;
        if took.taken() {
            return true;
        }
        // Any other chord is the app's: the address goes back first.
        if crate::field::command(mods) || mods.control_key() || mods.alt_key() {
            self.close_address();
            return false;
        }
        true
    }

    /// A press while editing: a row goes there, the field moves the caret,
    /// anywhere else puts the address back (and the press carries on).
    pub(crate) fn address_click(&mut self, x: f32, y: f32) -> bool {
        let Some(a) = self.address.as_ref() else { return false };
        if let Some(i) = a.hits.iter().position(|r| r.contains(x, y)) {
            self.address_commit(Some(i));
            return true;
        }
        if a.field.contains(x, y) {
            let style = self.address_style();
            let pad = self.px(9.0);
            let Some(a) = self.address.as_mut() else { return true };
            let from = x - a.field.x - pad;
            let chars: Vec<char> = a.line.chars().collect();
            let mut at = a.scroll;
            let mut width = 0.0;
            while at < chars.len() {
                let w = self.fonts.measure(style, &chars[at].to_string());
                if width + w / 2.0 > from {
                    break;
                }
                width += w;
                at += 1;
            }
            let line = a.line.clone();
            a.cur.move_to(&line, at, self.mods.shift_key());
            a.part = None;
            self.dirty = true;
            return true;
        }
        self.close_address();
        false
    }

    fn address_style(&self) -> Style {
        Style { px: self.px(12.0), ..self.ui() }
    }

    /// The field in the header while it is edited: a heavier edge, the line
    /// with its selection and caret, scrolled to keep the caret in view.
    pub(crate) fn draw_address_field(&mut self, scene: &mut nus_render::Scene, field: Rect, base: f32) {
        let (ink, paper, signal) = (self.theme.ink, self.paper(), self.surface.signal);
        let style = self.address_style();
        let pad = self.px(9.0);
        scene.rect(field, paper);
        scene.outline(field, self.px(2.0), ink);
        let Some(mut a) = self.address.take() else { return };
        a.field = field;
        let chars: Vec<char> = a.line.chars().collect();
        let caret = a.cur.at(&a.line);
        let room = field.w - 2.0 * pad;
        let width = |fonts: &nus_render::text::FontSystem, from: usize, to: usize| fonts.measure(style, &chars[from..to].iter().collect::<String>());
        // Keep the caret in view: scroll left past it, or right up to it.
        if caret < a.scroll {
            a.scroll = caret;
        }
        while a.scroll < caret && width(&self.fonts, a.scroll, caret) > room - self.px(8.0) {
            a.scroll += 1;
        }
        let mut end = a.scroll;
        while end < chars.len() && width(&self.fonts, a.scroll, end + 1) <= room {
            end += 1;
        }
        let x0 = field.x + pad;
        let (s0, s1) = a.cur.range(&a.line).unwrap_or((caret, caret));
        let (s0, s1) = (s0.clamp(a.scroll, end), s1.clamp(a.scroll, end));
        let mut x = x0;
        for (from, to, color) in [(a.scroll, s0, ink), (s0, s1, paper), (s1, end, ink)] {
            if to <= from {
                continue;
            }
            let w = width(&self.fonts, from, to);
            if color == paper {
                scene.rect(Rect::new(x, field.y + self.px(4.0), w, field.h - self.px(8.0)), ink);
            }
            let text: String = chars[from..to].iter().collect();
            self.fonts.draw(scene, Style { color, ..style }, x, base, &text);
            x += w;
        }
        if (a.scroll..=end).contains(&caret) {
            let cx = x0 + width(&self.fonts, a.scroll, caret);
            scene.rect(Rect::new(cx.round(), field.y + self.px(4.0), self.px(2.0), field.h - self.px(8.0)), signal);
        }
        self.address = Some(a);
    }

    /// The suggestions, hung from the field edge to edge, over the page.
    pub(crate) fn draw_address_list(&mut self, scene: &mut nus_render::Scene) {
        if self.address.is_some() && self.address_pane().is_none() {
            self.address = None;
            return;
        }
        let Some(mut a) = self.address.take() else { return };
        let (ink, paper, dim) = (self.theme.ink, self.paper(), self.theme.dim);
        let style = self.address_style();
        let label = self.label();
        let edge = self.px(2.0);
        let row_h = self.px(30.0);
        let foot_h = self.px(26.0);
        let pad = self.px(10.0);
        let f = a.field;
        let h = row_h * a.rows.len() as f32 + foot_h;
        let list = Rect::new(f.x, f.bottom() - edge, f.w, h + edge);
        scene.layer(None);
        // The hard shadow, the sheet, its edge (open at the top, where the field is).
        scene.rect(Rect::new(list.x + self.px(4.0), list.y + self.px(4.0), list.w, list.h), ink);
        scene.rect(list, paper);
        scene.rect(Rect::new(list.x, list.y, edge, list.h), ink);
        scene.rect(Rect::new(list.right() - edge, list.y, edge, list.h), ink);
        scene.rect(Rect::new(list.x, list.bottom() - edge, list.w, edge), ink);
        a.hits.clear();
        let mut y = list.y + edge;
        for (i, r) in a.rows.iter().enumerate() {
            let rr = Rect::new(list.x + edge, y, list.w - 2.0 * edge, row_h);
            let on = i == a.sel;
            if on {
                scene.rect(rr, ink);
            } else if i > 0 {
                scene.hline(rr.x, rr.y, rr.w, self.px(m::HAIRLINE), ink);
            }
            let (strong, soft) = if on { (paper, fade_to(dim, paper)) } else { (ink, dim) };
            let base = rr.y + row_h / 2.0 + style.px * 0.36;
            let glyph = match r.glyph { Glyph::Go => "→", Glyph::Search => "⌕", Glyph::History => "↺", Glyph::Port => "●" };
            let gcolor = if r.glyph == Glyph::Port && !on { self.surface.signal } else if on { paper } else { dim };
            self.fonts.draw(scene, Style { color: gcolor, ..style }, rr.x + pad, base, glyph);
            let tx = rr.x + pad + self.px(24.0);
            let tag_w = self.fonts.measure(label, &r.tag.to_uppercase());
            let room = rr.right() - pad - tag_w - self.px(16.0) - tx;
            let text = self.fit(style, r.text.as_str(), room).into_owned();
            let n = text.chars().count();
            let lit = r.lit.unwrap_or_else(|| host_range(&r.text));
            let lit = (lit.0.min(n), lit.1.min(n));
            let mut x = tx;
            for (from, to, color) in [(0, lit.0, soft), (lit.0, lit.1, strong), (lit.1, n, soft)] {
                if to <= from {
                    continue;
                }
                let piece: String = text.chars().skip(from).take(to - from).collect();
                x += self.fonts.draw(scene, Style { color, ..style }, x, base, &piece);
            }
            self.fonts.draw(scene, Style { color: soft, ..label }, rr.right() - pad - tag_w, base, &r.tag.to_uppercase());
            a.hits.push(rr);
            y += row_h;
        }
        // The keys, and Tab's word on parts.
        scene.hline(list.x, y, list.w, self.px(m::HAIRLINE), ink);
        let base = y + foot_h / 2.0 + label.px * 0.36;
        let mod_word = if cfg!(target_os = "macos") { "⌘↵" } else { "CTRL+↵" };
        let left = if a.part.is_some() { "↵ GO · TYPE TO CHANGE IT".to_string() } else { format!("↵ GO · {mod_word} NEW TAB · ⇧↵ WINDOW") };
        let right = if a.part.is_some() { "TAB NEXT · ⇧TAB BACK · ESC WHOLE" } else { "TAB · A PART" };
        let soft = Style { color: dim, ..label };
        self.fonts.draw(scene, soft, list.x + edge + pad, base, &left);
        let rw = self.fonts.measure(soft, right);
        self.fonts.draw(scene, soft, list.right() - edge - pad - rw, base, right);
        self.address = Some(a);
    }
}

/// Where the host is in an address as shown: from the start to the first
/// `/`, `?` or `#`, so rows scan by site.
fn host_range(text: &str) -> (usize, usize) {
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    (0, text[..end].chars().count())
}

/// Dim on ink: halfway to the paper, so a selected row's path stays quiet.
fn fade_to(dim: nus_render::Color, paper: nus_render::Color) -> nus_render::Color {
    [(dim[0] + paper[0]) / 2.0, (dim[1] + paper[1]) / 2.0, (dim[2] + paper[2]) / 2.0, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(line: &str) -> Vec<(String, Kind)> {
        parts(line).iter().map(|p| (text_of(line, p), p.kind)).collect()
    }

    #[test]
    fn an_address_comes_apart_where_it_can_change() {
        assert_eq!(
            texts("https://docs.rs/winit/latest/winit/event/enum.MouseScrollDelta.html"),
            [("docs.rs", Kind::Host), ("winit", Kind::Path), ("latest", Kind::Path), ("winit", Kind::Path), ("event", Kind::Path), ("enum.MouseScrollDelta.html", Kind::Path)]
                .map(|(t, k)| (t.to_string(), k))
        );
        assert_eq!(
            texts("http://localhost:5173/dashboard?tab=logs&x=1#top"),
            [("localhost", Kind::Host), ("5173", Kind::Port), ("dashboard", Kind::Path), ("tab=logs", Kind::Query), ("x=1", Kind::Query)]
                .map(|(t, k)| (t.to_string(), k))
        );
        assert_eq!(texts("example.com"), [("example.com".to_string(), Kind::Host)]);
        assert!(parts("").is_empty());
    }

    #[test]
    fn parts_count_characters_not_bytes() {
        let line = "https://例え.jp/ページ/x";
        let p = parts(line);
        assert_eq!(text_of(line, &p[0]), "例え.jp");
        assert_eq!(text_of(line, &p[1]), "ページ");
        assert_eq!(swap(line, &p[1], "page"), "https://例え.jp/page/x");
    }

    #[test]
    fn alternatives_differ_in_that_part_only() {
        let line = "https://docs.rs/winit/latest/winit/event/enum.MouseScrollDelta.html";
        let known = [
            "https://docs.rs/winit/0.30.13/winit/event/enum.MouseScrollDelta.html",
            "https://docs.rs/winit/0.29.15/winit/event/enum.MouseScrollDelta.html",
            "https://docs.rs/winit/0.29.15/winit/event/enum.TouchPhase.html",
            "https://docs.rs/winit/latest/winit/event/enum.MouseScrollDelta.html",
            "http://docs.rs/winit/0.28.0/winit/event/enum.MouseScrollDelta.html",
        ];
        assert_eq!(alternatives(line, 2, known.into_iter()), ["0.30.13", "0.29.15", "0.28.0"]);
        // TouchPhase differs in the version too: not an alternative for the page.
        assert!(alternatives(line, 5, known.into_iter()).is_empty());
        assert!(alternatives(line, 9, known.into_iter()).is_empty());
    }

    #[test]
    fn host_is_lit_by_default() {
        assert_eq!(host_range("docs.rs/winit"), (0, 7));
        assert_eq!(host_range("localhost:5173"), (0, 14));
    }
}
