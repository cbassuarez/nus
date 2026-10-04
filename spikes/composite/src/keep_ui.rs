//! Keeping, in the window (keep.rs says what a kept item is). ⌘D — Ctrl+D
//! on a page elsewhere, where a shell needs Ctrl+D for itself — or KEEP THIS
//! PAGE in the palette keeps what is in front with nothing to fill in. The
//! address field wears the mark, a ribbon, filled once the page is kept, and
//! a click on it does the same as the key.
//!
//! A slip hangs from the field for four seconds to say where it went and to
//! change what it is for: on the reading list, a copy kept for when the page
//! won't load, the plain folders it sits in. Tab walks its chips, Space or
//! Enter toggles one, Esc closes it; while it is hovered or walked it stays.
//! The same key again while a fresh slip is up takes the keeping back.

use nus_render::text::{icons, Style};
use nus_render::{Rect, Scene};
use std::time::Instant;
use winit::keyboard::{Key as WKey, NamedKey};

use crate::app::{fade, App, KeyIn, Pane, WebPane};
use nus_render::theme::metric as m;

/// How long a slip nobody touches stays up.
const LIFE_MS: u128 = 4000;

/// What a slip's chips do.
#[derive(Clone, Debug, PartialEq)]
pub enum SlipHit {
    Reading,
    Copy,
    Keyword,
    /// A plain folder, by name: the item sits in it or not.
    Folder(String),
}

pub struct Slip {
    /// The library record kept.
    pub id: String,
    /// The tab and side whose address field it hangs from.
    pub tab: u64,
    pub right: bool,
    pub since: Instant,
    /// Made by this keeping: the key again takes it back.
    pub fresh: bool,
    /// The chip Tab has reached, if any.
    pub focus: Option<usize>,
    pub rect: Rect,
    pub hits: Vec<(Rect, SlipHit)>,
}

/// What can be kept from a pane: a source, its title, its container.
fn source_of(pane: &Pane) -> Result<(String, String, Option<String>), &'static str> {
    match pane {
        Pane::Web(w) => {
            let s = w.tab.shared.borrow();
            let ok = url::Url::parse(&s.url).ok().is_some_and(|u| matches!(u.scheme(), "http" | "https" | "file") && u.username().is_empty() && u.password().is_none());
            if !ok { return Err("Only web pages and files can be kept."); }
            let title = if s.title.is_empty() { s.url.clone() } else { s.title.clone() };
            Ok((s.url.clone(), title, crate::library::container(&w.container)))
        }
        Pane::Editor(e) => match e.buf().and_then(|b| b.path.clone()) {
            Some(path) => Ok((format!("file:{}", path.display()), e.title(), None)),
            None => Err("Save the file first, then keep it."),
        },
        _ => Err("Open a page or a file to keep it."),
    }
}

impl App {
    /// The address field's box for a page, as the URL row draws it.
    pub(crate) fn keep_field(&self, w: &WebPane) -> Rect {
        let r = w.rect;
        let isz = self.px(15.0);
        let media = !w.tab.shared.borrow().media.is_empty();
        let x = r.x + self.px(14.0) + self.px(m::NAV_SLOT) * 3.0 + self.px(4.0);
        let dw = isz * 3.0 + self.px(28.0) + if media { isz + self.px(14.0) } else { 0.0 };
        Rect::new(x, r.y + self.px(6.0), r.right() - self.px(14.0) - dw - self.px(18.0) - x, self.px(22.0))
    }

    /// Where the mark sits: the right end of the field.
    pub(crate) fn keep_mark_rect(&self, w: &WebPane) -> Rect {
        let f = self.keep_field(w);
        Rect::new(f.right() - self.px(24.0), f.y, self.px(24.0), f.h)
    }

    /// The ribbon, `h` tall at (x, y): the folded bookmark saved commands draw.
    pub(crate) fn draw_ribbon(&self, scene: &mut Scene, x: f32, y: f32, h: f32, color: nus_render::Color) {
        let w = h * 0.66;
        scene.poly(&[[x, y], [x + w, y], [x + w, y + h], [x + w / 2.0, y + h * 0.73], [x, y + h]], color);
    }

    /// The mark in a page's field: filled when the page is kept.
    pub(crate) fn draw_keep_mark(&mut self, scene: &mut Scene, w: &WebPane) {
        if crate::private::enabled() { return; }
        self.library.ensure();
        let url = w.tab.shared.borrow().url.clone();
        let kept = self.library.kept(&url, &crate::library::container(&w.container)).is_some();
        let r = self.keep_mark_rect(w);
        let h = self.px(12.0);
        let color = if kept { self.surface.signal } else { fade(self.theme.ink, 0.28) };
        self.draw_ribbon(scene, r.x + self.px(8.0), r.y + (r.h - h) / 2.0, h, color);
    }

    /// ⌘D: keep what is in front, or take back a keeping just made.
    pub(crate) fn keep_front(&mut self) {
        if crate::private::enabled() {
            self.notice(icons::BOOK, "Not Kept", "nothing is kept from an incognito window");
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else { return };
        let (tab_id, right) = (tab.id, tab.focus_right && tab.right.is_some());
        if let Some(slip) = self.keep_slip.as_ref().filter(|s| s.tab == tab_id && s.right == right) {
            if slip.fresh {
                let id = slip.id.clone();
                return self.keep_undo(&id);
            }
            self.keep_slip = None;
            self.dirty = true;
            return;
        }
        let pane = tab.focused_ref();
        let web = matches!(pane, Pane::Web(_));
        let (source, title, container) = match source_of(pane) {
            Ok(v) => v,
            Err(why) => { self.notice(icons::BOOK, "Nothing To Keep", why); return; }
        };
        self.library.ensure();
        match self.library.store().keep(&source, &title, container, crate::journal::now()) {
            Ok((e, created)) => {
                let id = e.id.clone();
                self.library.remember(e);
                self.play_event("toggle");
                if web {
                    self.keep_slip = Some(Slip { id, tab: tab_id, right, since: crate::clock::now(), fresh: created, focus: None, rect: Rect::new(0.0, 0.0, 0.0, 0.0), hits: Vec::new() });
                } else if created {
                    self.notice(icons::BOOK, "Kept", title);
                } else {
                    self.notice(icons::BOOK, "Already Kept", title);
                }
            }
            Err(e) => {
                tracing::info!("keep failed: {e}");
                self.notice(icons::BOOK, "Could Not Keep This", "your library is unchanged · try again");
            }
        }
        self.dirty = true;
    }

    /// `keyword rs https://…%s`: keep a search address under a word.
    pub(crate) fn keep_template(&mut self, word: String, url: String) {
        if crate::private::enabled() { return; }
        self.library.ensure();
        let host = url.split("//").nth(1).unwrap_or(&url).split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
        match self.library.store().keep(&url, &format!("{host} search"), None, crate::journal::now()) {
            Ok((e, _)) => {
                self.keep_keyword_for = Some(e.id.clone());
                self.library.remember(e);
                self.set_keyword(word);
            }
            Err(e) => { tracing::info!("keyword keep failed: {e}"); self.notice(icons::BOOK, "Keyword Not Set", "your library is unchanged · try again"); }
        }
    }

    /// The mark clicked: the same as the key, for that pane.
    pub(crate) fn keep_mark_click(&mut self, right: bool) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.focus_right = right && t.right.is_some();
        }
        self.keep_front();
    }

    fn keep_undo(&mut self, id: &str) {
        let collections = self.library.entries.get(id).map(|e| (e.source.clone(), e.collections.clone()));
        match self.library.store().remove(id) {
            Ok(e) => {
                self.library.remember(e);
                if let Some((source, names)) = collections {
                    for name in names { self.keep_folder_item(&source, "", &name, false); }
                }
                self.notice(icons::BOOK, "No Longer Kept", "");
            }
            Err(e) => {
                tracing::info!("keep undo failed: {e}");
                self.notice(icons::BOOK, "Could Not Undo", "the page is still kept");
            }
        }
        self.keep_slip = None;
        self.dirty = true;
    }

    /// Put a source in a plain folder, or take it out; folders are still
    /// their own list until the sidebar reads collections (keep.rs).
    pub(crate) fn keep_folder_item(&mut self, source: &str, title: &str, name: &str, on: bool) {
        use crate::folders::{Item, Kind};
        let canon = crate::keep::canon(source);
        let Some(f) = self.folders.iter_mut().find(|f| f.kind == Kind::Plain && f.name == name) else { return };
        f.items.retain(|it| crate::keep::canon(&it.url) != canon);
        if on {
            let host = source.split("//").nth(1).unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
            f.items.push(Item { title: title.to_string(), url: source.to_string(), detail: host });
            f.open = true;
        }
        self.save_folders();
    }

    /// A tab's page saved into a folder from its menu is kept there too.
    pub(crate) fn keep_into_folder(&mut self, source: &str, title: &str, container: Option<String>, name: &str) {
        if crate::private::enabled() { return; }
        self.library.ensure();
        let kept = self.library.store().keep(source, title, container, crate::journal::now())
            .and_then(|(e, _)| self.library.store().update(&e.id, |e| if !e.collections.iter().any(|c| c == name) { e.collections.push(name.to_string()); }));
        match kept {
            Ok(e) => self.library.remember(e),
            Err(e) => tracing::info!("keep into folder failed: {e}"),
        }
    }

    fn keep_slip_act(&mut self, hit: SlipHit) {
        let Some(id) = self.keep_slip.as_ref().map(|s| s.id.clone()) else { return };
        if !self.library.entries.contains_key(&id) { self.keep_slip = None; return; }
        if let Some(s) = self.keep_slip.as_mut() { s.fresh = false; s.since = crate::clock::now(); }
        self.keep_role(&id, hit);
    }

    /// Change what a kept item is for: the slip's chips and the library's
    /// detail column both come here.
    pub(crate) fn keep_role(&mut self, id: &str, hit: SlipHit) {
        let id = id.to_string();
        let Some(entry) = self.library.entries.get(&id).cloned() else { return };
        self.play_event("control.press");
        let store = self.library.store().clone();
        let result = match &hit {
            SlipHit::Reading => {
                let on = entry.reading != Some(false);
                store.update(&id, |e| e.reading = Some(!on))
            }
            SlipHit::Copy => {
                if entry.snapshot.is_some() { return; }
                return self.copy_kept(&id);
            }
            SlipHit::Keyword => return self.ask_keyword(id),
            SlipHit::Folder(name) => {
                let on = !entry.collections.iter().any(|c| c == name);
                let result = store.update(&id, |e| {
                    e.collections.retain(|c| c != name);
                    if on { e.collections.push(name.clone()); }
                });
                if result.is_ok() { self.keep_folder_item(&entry.source, &entry.title, name, on); }
                result
            }
        };
        match result {
            Ok(e) => self.library.remember(e),
            Err(e) => {
                tracing::info!("keep role failed: {e}");
                self.notice(icons::BOOK, "Not Changed", "the item changed elsewhere · try again");
            }
        }
        self.dirty = true;
    }

    /// Keys while a slip is up on the pane in front: Tab walks, Space or
    /// Enter toggles, Esc closes. Everything else goes where it would.
    pub(crate) fn keep_slip_key(&mut self, ev: &KeyIn) -> bool {
        if ev.state != winit::event::ElementState::Pressed || self.palette.is_some() { return false; }
        let Some(slip) = self.keep_slip.as_mut() else { return false };
        let shift = self.mods.shift_key();
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => { self.keep_slip = None; self.dirty = true; true }
            WKey::Named(NamedKey::Tab) => {
                let n = slip.hits.len();
                if n == 0 { return false; }
                slip.focus = Some(match (slip.focus, shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                });
                slip.since = crate::clock::now();
                self.dirty = true;
                true
            }
            WKey::Named(NamedKey::Space | NamedKey::Enter) if slip.focus.is_some() => {
                let hit = slip.focus.and_then(|i| slip.hits.get(i)).map(|(_, h)| h.clone());
                if let Some(hit) = hit { self.keep_slip_act(hit); }
                true
            }
            _ => false,
        }
    }

    /// A press on the slip is the slip's; a press elsewhere closes it.
    pub(crate) fn keep_slip_mouse(&mut self, pressed: bool, left: bool, x: f32, y: f32) -> bool {
        let Some(slip) = self.keep_slip.as_ref() else { return false };
        if !slip.rect.contains(x, y) {
            if pressed && !self.keep_mark_under(x, y) { self.keep_slip = None; self.dirty = true; }
            return false;
        }
        if pressed && left {
            if let Some(hit) = slip.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| h.clone()) {
                self.keep_slip_act(hit);
            }
        }
        true
    }

    fn keep_mark_under(&self, x: f32, y: f32) -> bool {
        self.tabs.get(self.active).is_some_and(|t| {
            std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| matches!(p, Pane::Web(w) if !w.bare && self.keep_mark_rect(w).contains(x, y)))
        })
    }

    /// Each frame: a slip nobody is using goes after its time, and goes at
    /// once if its tab is no longer in front or its item is gone.
    pub(crate) fn keep_tick(&mut self) {
        let Some(slip) = self.keep_slip.as_mut() else { return };
        let here = self.tabs.get(self.active).is_some_and(|t| t.id == slip.tab);
        let alive = self.library.entries.get(&slip.id).is_some_and(|e| !e.deleted);
        if slip.rect.contains(self.mouse.0, self.mouse.1) || slip.focus.is_some() { slip.since = crate::clock::now(); }
        if !here || !alive || crate::clock::since(slip.since).as_millis() > LIFE_MS {
            self.keep_slip = None;
            self.dirty = true;
        }
    }

    /// The slip, over everything, under the field it hangs from.
    pub(crate) fn draw_keep_slip(&mut self, scene: &mut Scene) {
        let Some((id, fresh, focus, tab, right)) = self.keep_slip.as_ref().map(|s| (s.id.clone(), s.fresh, s.focus, s.tab, s.right)) else { return };
        let geometry = self.tabs.get(self.active).filter(|t| t.id == tab).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| match p {
            Pane::Web(w) if !w.bare => Some((self.keep_field(w), w.rect)),
            _ => None,
        });
        let Some((field, pane)) = geometry else { return };
        let Some(e) = self.library.entries.get(&id).cloned() else { return };
        let t = self.theme.clone();
        let (ink, paper, signal) = (t.ink, t.paper, self.surface.signal);
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let dim = Style { color: fade(ink, 0.6), ..label };
        let width = self.px(360.0).min(pane.w - self.px(28.0));
        let x = (field.right() - width).max(pane.x + self.px(14.0));
        let y = field.bottom() + self.px(6.0);
        let pad = self.px(12.0);
        let row_h = self.px(32.0);
        let folders: Vec<String> = self.folders.iter().filter(|f| f.kind == crate::folders::Kind::Plain).map(|f| f.name.clone()).collect();
        let rows = 3 + usize::from(!folders.is_empty());
        let h = row_h * rows as f32 - self.px(6.0);
        let rect = Rect::new(x, y, width, h);
        scene.layer(None);
        scene.rect(Rect::new(rect.x + self.px(6.0), rect.y + self.px(6.0), rect.w, rect.h), fade(ink, 0.3));
        scene.rect(rect, paper);
        scene.outline(rect, self.px(m::STRUCTURE), ink);
        // The first row: what happened.
        let base = y + row_h / 2.0 + self.px(4.0);
        self.draw_ribbon(scene, x + pad, y + (row_h - self.px(13.0)) / 2.0, self.px(13.0), signal);
        let said = if fresh { "KEPT" } else { "KEPT ·" };
        let hint = if fresh {
            if cfg!(target_os = "macos") { "⌘D AGAIN UNDOES" } else { "CTRL+D AGAIN UNDOES" }
        } else { "ESC CLOSES" };
        let sx = x + pad + self.px(18.0);
        self.fonts.draw(scene, strong, sx, base, said);
        let hw = self.fonts.measure(dim, hint);
        self.fonts.draw(scene, dim, rect.right() - pad - hw, base, hint);
        let tx = sx + self.fonts.measure(strong, said) + self.px(8.0);
        let title = self.fit(ui, &e.title, (rect.right() - pad - hw - self.px(10.0) - tx).max(0.0));
        self.fonts.draw(scene, ui, tx, base, &title);
        let mut ry = y + row_h;
        let mut hits: Vec<(Rect, SlipHit)> = Vec::new();
        let chip = |app: &mut App, scene: &mut Scene, hits: &mut Vec<(Rect, SlipHit)>, x: &mut f32, ry: f32, text: &str, on: bool, enabled: bool, hit: SlipHit| {
            let cw = app.fonts.measure(label, text) + app.px(16.0);
            if *x + cw > rect.right() - pad { return; }
            let r = Rect::new(*x, ry + app.px(6.0), cw, app.px(20.0));
            let focused = focus == Some(hits.len());
            if on { scene.rect(r, ink); } else { scene.outline(r, app.px(1.0), if enabled { ink } else { fade(ink, 0.35) }); }
            // Ink, not the signal: on some looks the signal is the paper.
            if focused { scene.outline(Rect::new(r.x - app.px(3.0), r.y - app.px(3.0), r.w + app.px(6.0), r.h + app.px(6.0)), app.px(2.0), ink); }
            let color = if on { app.on_fill(ink) } else if enabled { ink } else { fade(ink, 0.45) };
            app.fonts.draw(scene, Style { color, ..label }, r.x + app.px(8.0), r.y + app.px(14.0), text);
            hits.push((r, hit));
            *x += cw + app.px(6.0);
        };
        let kx = x + pad;
        let cx0 = kx + self.px(30.0);
        if !folders.is_empty() {
            scene.hline(x, ry, width, self.px(m::HAIRLINE), fade(ink, 0.2));
            self.fonts.draw(scene, dim, kx, ry + self.px(20.0), "IN");
            let mut cx = cx0;
            for name in folders {
                let on = e.collections.iter().any(|c| *c == name);
                chip(self, scene, &mut hits, &mut cx, ry, &name.to_uppercase(), on, true, SlipHit::Folder(name));
            }
            ry += row_h;
        }
        scene.hline(x, ry, width, self.px(m::HAIRLINE), fade(ink, 0.2));
        self.fonts.draw(scene, dim, kx, ry + self.px(20.0), "AS");
        let mut cx = cx0;
        chip(self, scene, &mut hits, &mut cx, ry, "TO READ", e.reading != Some(false), true, SlipHit::Reading);
        let (copy, has) = if e.snapshot.is_some() { ("COPY KEPT", true) } else if e.capture.is_some() { ("COPYING…", false) } else { ("KEEP A COPY", false) };
        chip(self, scene, &mut hits, &mut cx, ry, copy, has, !has && e.capture.is_none(), SlipHit::Copy);
        let keyword = if e.keyword.is_empty() { "KEYWORD".to_string() } else { format!("KEYWORD · {}", e.keyword) };
        chip(self, scene, &mut hits, &mut cx, ry, &keyword, !e.keyword.is_empty(), true, SlipHit::Keyword);
        ry += row_h;
        scene.hline(x, ry, width, self.px(m::HAIRLINE), fade(ink, 0.2));
        self.fonts.draw(scene, dim, kx, ry + self.px(17.0), "TAB MOVES · SPACE TOGGLES · ESC CLOSES");
        if let Some(s) = self.keep_slip.as_mut() {
            s.rect = rect;
            s.hits = hits;
        }
    }
}
