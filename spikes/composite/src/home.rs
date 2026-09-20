//! The prompt: nus's home, a terminal with no PTY behind it.
//!
//! One line, centred, a caret and nothing else. Type a URL and Enter: the
//! pane becomes that page. Type a command: the pane becomes a shell running
//! it. Enter on nothing: a shell. As you type, rows come up beneath the
//! line — the palette's rows, so tabs, recent pages and shells, layouts,
//! history, held shells and settings are all one keystroke away; ↑/↓ pick
//! one and Enter takes it. At the foot, the routes as marks rather than a
//! sentence — a shell, a page, an assistant, the rows — with the one Enter
//! would take lit; the pointer names them and a click puts their prefix on
//! the line (PROMPT · LAYOUT · ROUTE KEYS turns them off).
//! STARTUP · THEN · THE PROMPT puts it up first;
//! `home` in the palette brings it back. HOME · THE PLATE puts the line
//! under the icon, with the first places as stops on its band (plate.rs).
//! Drawn from the tokens, like every other surface: paper, ink, one signal.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, PaletteMode, PaletteRow, Pane};
use crate::settings::HomeLook;
use nus_render::theme::metric as m;

pub struct HomePane {
    pub rect: Rect,
    pub input: String,
    pub sel: usize,
    pub hits: Vec<(Rect, usize)>,
    pub since: Instant,
    /// The splash drew the plate's band already; it is not drawn in again.
    pub handed: bool,
    /// Clicks on the pane's paper since the last frame, for the art.
    pub taps: Vec<(f32, f32)>,
    /// The plate's stops (plate.rs), gathered when the pane first draws
    /// and again now and then — holders are asked over a socket.
    pub places: Option<(Instant, Vec<PaletteRow>)>,
    /// The route keys at the foot, where they were drawn.
    pub keys: Vec<(Rect, Key)>,
}

/// A route the prompt can take, as one mark at the foot of the page.
/// Each is a prefix you could have typed; the one Enter would take is lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// `>` — a command in a new shell. Enter on an empty line, too.
    Shell,
    /// `?` — a page, or a search when it is not an address.
    Page,
    /// `@` — a prompt for an assistant.
    Ask,
    /// A folder or a file the line names: it opens as a project.
    Path,
    /// `↓` — the rows beneath the line.
    Rows,
}

impl Key {
    /// The mark, and what it is called when the pointer rests on it.
    fn look(self) -> ((&'static str, &'static str), &'static str) {
        use nus_render::text::icons as i;
        match self {
            Key::Shell => (i::TERMINAL, "a shell · type > before a command"),
            Key::Page => (i::GLOBE, "a page · type ? to search the web"),
            Key::Ask => (i::ASSISTANT, "ask · type @claude, @codex or @ollama"),
            Key::Path => (i::FOLDER, "a project · the line names a folder"),
            Key::Rows => (i::CARET_DOWN, "the rows · ↓ picks one, Enter takes it"),
        }
    }

    /// What clicking it puts on the line.
    fn prefix(self) -> Option<&'static str> {
        match self {
            Key::Shell => Some("> "),
            Key::Page => Some("? "),
            Key::Ask => Some("@"),
            Key::Path | Key::Rows => None,
        }
    }
}

impl HomePane {
    pub fn new() -> HomePane {
        HomePane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), input: String::new(), sel: 0, hits: Vec::new(), since: crate::clock::now(), handed: false, taps: Vec::new(), places: None, keys: Vec::new() }
    }
}

/// What a line means: a page, or a command for a shell.
fn is_url(s: &str) -> bool {
    let q = s.trim();
    q.contains("://") || q.starts_with("localhost") || (q.contains('.') && !q.contains(' ') && !q.starts_with('.') && !q.contains('\\') && !q.contains('/'))
}

impl App {
    /// The existing Newsreader n, with a small vector fedora. Inherits the page ink.
    fn draw_private_mark(&mut self, scene: &mut Scene, r: Rect, ink: nus_render::Color, paper: nus_render::Color) {
        let at = |x:f32,y:f32| [r.x+x*r.w, r.y+(y+0.07)*r.h];
        self.fonts.draw(scene, Style { font:self.f.wordmark,px:r.h*0.94,color:ink,tracking:0.0 },r.x+r.w*0.20,r.bottom()-r.h*0.08,"n");
        // Pinched crown, ribbon and upturned brim; no raster asset or new logo font.
        scene.poly(&[at(0.22,0.30),at(0.28,0.11),at(0.35,0.06),at(0.47,0.11),at(0.61,0.07),at(0.70,0.12),at(0.78,0.29)],ink);
        scene.poly(&[at(0.24,0.26),at(0.76,0.25),at(0.78,0.32),at(0.22,0.33)],paper);
        scene.poly(&[at(0.04,0.32),at(0.24,0.31),at(0.78,0.29),at(0.96,0.25),at(0.91,0.36),at(0.66,0.41),at(0.28,0.41),at(0.04,0.37)],ink);
    }

    /// The rows under the line: the palette's, for what is typed; a short
    /// list of places to go when nothing is — under the plate, the stops.
    fn home_rows(&self, input: &str, _places: &[PaletteRow]) -> Vec<PaletteRow> {
        self.prompt_rows(input)
    }

    fn news_count(&self, _input: &str) -> usize { 0 }

    /// Folders this window could be: the other windows', the last
    /// sessions' shells', the journal's — and the line itself when it
    /// names a folder.
    pub(crate) fn workspace_rows(&self, input: &str) -> Vec<PaletteRow> {
        let q = input.trim();
        let mut out: Vec<PaletteRow> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let typed = q.to_string();
        if !q.is_empty() && std::path::Path::new(&typed).is_dir() {
            seen.insert(typed.to_lowercase());
            out.push(PaletteRow { num: "→".into(), text: format!("{} · this window's folder", typed), action: crate::app::Action::OpenFolder(typed.clone()) });
        }
        let mut folders: Vec<String> = Vec::new();
        if let Some(w) = &self.workspace {
            folders.push(w.to_string_lossy().to_string());
        }
        if let Some(sess) = &self.last_session {
            if let Some(f) = &sess.folder {
                folders.push(f.clone());
            }
            for t in &sess.tabs {
                for st in std::iter::once(&t.shell).chain(std::iter::once(&t.shell_right)).flatten() {
                    if let Some(c) = &st.cwd {
                        folders.push(c.clone());
                    }
                }
            }
            for o in &sess.others {
                if let Some(f) = &o.folder {
                    folders.push(f.clone());
                }
            }
        }
        folders.extend(crate::journal::folders().into_iter().map(|(c, _)| c));
        let ql = q.to_lowercase();
        for f in folders.into_iter().filter(|f| !f.is_empty() && std::path::Path::new(f).is_dir()) {
            let key = f.to_lowercase();
            if !ql.is_empty() && !key.contains(&ql) {
                continue;
            }
            if seen.insert(key) {
                let tail = crate::plate::tail(&f);
                out.push(PaletteRow { num: "▸".into(), text: format!("{tail} · {f}"), action: crate::app::Action::OpenFolder(f.clone()) });
            }
            if out.len() >= 8 {
                break;
            }
        }
        out
    }

    /// The pane's places, gathered on first use and refreshed every so often.
    fn home_places(&self, p: &mut HomePane) -> Vec<PaletteRow> {
        let stale = p.places.as_ref().map(|(at, _)| crate::clock::since(at).as_secs() >= 20).unwrap_or(true);
        if stale && self.behavior.home_look == HomeLook::Plate {
            p.places = Some((crate::clock::now(), self.places()));
        }
        p.places.as_ref().map(|(_, v)| v.to_vec()).unwrap_or_default()
    }

    /// Launch and the new-tab command share the same destination. Only launch
    /// replaces the temporary shell; opening a tab preserves existing work.
    pub(crate) fn open_start_page(&mut self, launch: bool) {
        use crate::settings::Then;
        match self.behavior.then {
            Then::Restore if launch => {
                self.restore_session_pub();
                self.drop_birth = self.tabs.len() > 1;
                self.drop_birth_shell();
                return;
            },
            Then::Shell if launch => return,
            Then::Shell => return self.new_tab(self.behavior.default_profile),
            Then::Layout => {
                let layouts = crate::layout_file::saved();
                let pick = layouts.iter().find(|(name, _)| *name == self.behavior.then_layout)
                    .or_else(|| if self.behavior.then_layout.is_empty() { layouts.first() } else { None });
                if let Some((_, path)) = pick {
                    let before = self.tabs.len();
                    self.open_layout(path);
                    if self.tabs.len() > before {
                        if launch {
                            self.tabs.remove(0);
                            self.tab_removed(0);
                            self.active = self.active.saturating_sub(1);
                            self.layout();
                        }
                        return;
                    }
                }
                self.notice("No startup layout could be opened. Choose a saved layout in Startup.");
            }
            Then::HomePage | Then::LastPage => {
                let url = if self.behavior.then == Then::HomePage {
                    Some(self.behavior.home_url.clone())
                } else {
                    self.recent.iter().find_map(|r| match &r.item {
                        crate::start::Saved::Page { url, .. } => Some(url.clone()),
                        _ => None,
                    })
                };
                if let Some(url) = url.filter(|u| !u.trim().is_empty()) {
                    if let Some(web) = self.new_web_pane(&url) {
                        self.show_start_pane(Pane::Web(web), launch);
                        return;
                    }
                }
            }
            Then::Prompt | Then::Restore => {}
        }
        self.show_start_pane(Pane::Home(HomePane::new()), launch);
    }

    fn show_start_pane(&mut self, pane: Pane, launch: bool) {
        self.palette = None;
        if launch {
            self.replace_birth(pane);
        } else {
            let tab = self.make_tab(pane, None);
            self.tabs.push(tab);
            self.activate(self.tabs.len() - 1);
            self.layout();
            self.dirty = true;
        }
    }

    /// A tab whose pane is the prompt, in front.
    pub(crate) fn open_home(&mut self) {
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Home(_))) {
            self.activate(i);
            return;
        }
        let tab = self.make_tab(Pane::Home(HomePane::new()), None);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1);
        self.layout();
        self.dirty = true;
    }

    /// Enter: the line becomes a page or a shell in this very tab; a
    /// picked row runs and the prompt gives way to what it opened.
    fn home_commit(&mut self) {
        let i = self.active;
        let Some(Pane::Home(h)) = self.tabs.get(i).map(|t| &t.left) else { return };
        let input = h.input.trim().to_string();
        if crate::private::enabled() && input.is_empty() { return; }
        let sel = h.sel;
        let places = h.places.as_ref().map(|(_, v)| v.to_vec()).unwrap_or_default();
        let rows = self.home_rows(&input, &places);
        // Acting on anything is having seen the news.
        if self.news.since.is_some() {
            self.dismiss_news();
        }
        // A row only when you moved to one (sel is 1-based; 0 is the line itself).
        let direct = if sel == 0 && !input.is_empty() { rows.first().cloned() } else { None };
        if let Some(row) = sel.checked_sub(1).and_then(|k| rows.get(k)).or(direct.as_ref()) {
            {
                let action = row.action.clone();
                let before = self.tabs.len();
                self.run(action);
                // The prompt gives way when something else came up.
                if self.tabs.len() > before || self.active != i {
                    if let Some(k) = self.tabs.get(i).filter(|t| matches!(t.left, Pane::Home(_))).map(|_| i) {
                        self.tabs.remove(k);
                        self.tab_removed(k);
                        if self.active > k {
                            self.active -= 1;
                        } else if self.active >= self.tabs.len() {
                            self.active = self.tabs.len().saturating_sub(1);
                        }
                    }
                }
                self.layout();
                self.dirty = true;
                return;
            }
        }
        if input.starts_with('@') {self.notice("Choose @claude, @codex or @ollama, followed by your prompt.");return;}
        if self.fresh && !input.is_empty() && std::path::Path::new(&input).is_dir() {
            self.open_folder(&input);
            return;
        }
        if input.is_empty() && self.behavior.lead == crate::settings::Lead::Browser {
            self.open_start();
            return;
        }
        let profile = self.behavior.default_profile;
        let replacement = if is_url(&input) {
            let url = crate::links::normalize(&input);
            let url = if url.contains("://") { url } else { format!("https://{url}") };
            self.new_web_pane(&url).map(Pane::Web)
        } else {
            match self.new_term_pane(false, profile) {
                Ok(mut t) => {
                    if !input.is_empty() {
                        t.type_at_prompt = Some(format!("{input}\r"));
                    }
                    Some(Pane::Term(t))
                }
                Err(_) => None,
            }
        };
        if let (Some(p), Some(tab)) = (replacement, self.tabs.get_mut(i)) {
            tab.left = p;
            tab.focus_right = false;
        }
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    pub(crate) fn home_commit_pub(&mut self) {
        self.home_commit();
    }

    /// Keys on the prompt. Returns true when it took the key.
    pub(crate) fn home_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key as K, NamedKey};
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let i = self.active;
        let mods = self.mods;
        let Some(Pane::Home(h)) = self.tabs.get_mut(i).map(|t| &mut t.left) else { return false };
        // The line's own editing: typing, erasing, paste, copy (field.rs).
        let took = crate::field::edit(&mut h.input, ev, mods, 2000);
        if took.changed() {
            h.sel = 0;
        }
        if took.taken() {
            self.dirty = true;
            return true;
        }
        if mods.control_key() || mods.alt_key() || mods.super_key() {
            return false;
        }
        let shift = mods.shift_key();
        match &ev.logical_key {
            K::Named(NamedKey::Enter) => {
                self.home_commit();
                return true;
            }
            // Tab walks the rows like the arrows; Shift+Tab back.
            K::Named(NamedKey::ArrowDown) | K::Named(NamedKey::Tab) if !(shift && matches!(ev.logical_key, K::Named(NamedKey::Tab))) => h.sel += 1, // clamped when drawn
            K::Named(NamedKey::ArrowUp) | K::Named(NamedKey::Tab) => h.sel = h.sel.saturating_sub(1),
            K::Named(NamedKey::Escape) => {
                if h.input.is_empty() {
                    if self.news.since.is_some() {
                        self.dismiss_news();
                        return true;
                    }
                    return false;
                }
                h.input.clear();
                h.sel = 0;
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// A click on a row.
    pub(crate) fn home_click(&mut self, x: f32, y: f32) -> bool {
        let i = self.active;
        let pad = self.touch_pad();
        let Some(Pane::Home(h)) = self.tabs.get_mut(i).map(|t| &mut t.left) else { return false };
        // The route keys sit over the paper, so they answer first.
        if let Some(&(_, key)) = h.keys.iter().find(|(r, _)| r.contains(x, y)) {
            match key {
                // The rows: down to the first one, or back to the line.
                Key::Rows => h.sel = if h.sel == 0 { 1 } else { 0 },
                // The line already names a folder; take it.
                Key::Path => {
                    h.sel = 0;
                    self.home_commit();
                    return true;
                }
                k => {
                    if let Some(prefix) = k.prefix() {
                        // Swap the route, keep what was typed after it.
                        let rest = h.input.trim_start().trim_start_matches(['>', '?', '@']).trim_start().to_string();
                        h.input = format!("{prefix}{rest}");
                        h.sel = 0;
                        h.since = crate::clock::now();
                    }
                }
            }
            self.dirty = true;
            return true;
        }
        let Some(&(_, k)) = h.hits.iter().find(|(r, _)| crate::touch::grown(*r, pad).contains(x, y)) else {
            // Paper, not a row: the art may want to know.
            if h.rect.contains(x, y) {
                h.taps.push((x - h.rect.x, y - h.rect.y));
                self.dirty = true;
            }
            return false;
        };
        h.sel = k + 1;
        self.home_commit();
        true
    }

    /// The prompt, drawn: the line alone, or under the plate.
    pub(crate) fn draw_home(&mut self, scene: &mut Scene, p: &mut HomePane, focused: bool) {
        let t = self.theme.clone();
        let r = p.rect;
        let ink = t.ink;
        let paper = self.paper();
        scene.rect(r, paper);
        let plate = self.behavior.home_look == HomeLook::Plate;
        let art = self.behavior.home_look == HomeLook::Art;
        // Under the plate the line sits beneath the icon and comes up once
        // the band closes; alone, it sits a third of the way down.
        let (mut y0, up) = if plate { self.draw_plate_icon(scene, p) } else { (r.y + r.h * if self.behavior.prompt.top { 0.18 } else { 0.34 }, 1.0) };
        if art {
            self.draw_home_art(scene, p, y0);
        }
        // Match the artwork's brightness independently of the chrome theme.
        let backdrop = if art { self.art.as_ref().map(|a| a.backdrop).unwrap_or_default() } else { crate::art::Backdrop::Theme };
        let dark = backdrop == crate::art::Backdrop::Dark;
        let ink = backdrop.foreground(t.mode, ink, t.paper);
        // The line: a caret in signal, the input in mono, a rule beneath.
        let px = self.px(20.0);
        let mono = Style { font: self.f.ui, px, color: fade(ink, up), tracking: 0.0 };
        let line_w = (r.w * if self.behavior.prompt.wide { 0.84 } else { 0.62 }).max(self.px(320.0)).min(r.w - self.px(56.0));
        let x0 = r.x + (r.w - line_w) / 2.0;
        if crate::private::enabled() {
            let label = Style { font: self.f.strong, px: self.px(16.0), color: ink, tracking: 0.0 };
            let note = Style { font: self.f.ui, px: self.px(12.0), color: ink, tracking: 0.0 };
            let text_x=x0+self.px(66.0);
            let lines:Vec<String>=crate::private::NOTE.iter().flat_map(|text|crate::reader::wrap(&self.fonts,note,text,(line_w-self.px(66.0)).max(self.px(160.0)))).collect();
            let note_h=self.px(26.0+lines.len() as f32*18.0);
            let head_y=(y0-note_h-self.px(36.0)).max(r.y+self.px(68.0));
            y0=y0.max(head_y+note_h+self.px(30.0));
            let mark = Rect::new(x0, head_y-self.px(42.0), self.px(52.0), self.px(58.0));
            self.draw_private_mark(scene,mark,ink,t.paper);
            self.fonts.draw(scene,label,text_x,head_y,"INCOGNITO");
            for (i,text) in lines.iter().enumerate() {
                self.fonts.draw(scene,note,text_x,head_y+self.px(26.0)+i as f32*self.px(18.0),text);
            }
        }
        let caret_w = self.draw_lit(scene, Style { color: fade(self.surface.signal, up), ..mono }, x0, y0, "»", dark) + self.px(12.0);
        let shown = self.fit(mono, &p.input, line_w - caret_w - px);
        let tw = self.draw_lit(scene, mono, x0 + caret_w, y0, &shown, dark);
        // The block caret, breathing.
        if focused {
            let blinking = match self.cursor.blink {
                crate::settings::Blink::Never => false,
                crate::settings::Blink::AfterIdle => crate::clock::since(p.since).as_secs_f32() > 2.0,
                crate::settings::Blink::Always => true,
            };
            let on = !blinking || (crate::clock::since(self.started).as_millis() / self.cursor.period.max(100) as u128) % 2 == 0;
            if on {
                scene.rect(Rect::new(x0 + caret_w + tw + self.px(2.0), y0 - px * 0.78, px * 0.5, px * 0.95), fade(ink, up));
            }
            // App::tick requests a frame only when the blink phase changes.
        }
        scene.hline(x0, y0 + self.px(12.0), line_w, self.px(m::HAIRLINE), fade(ink, 0.45 * up));
        // Rows beneath: the palette's, for what is typed. Under the plate
        // with nothing typed, the rows are the stops on the band.
        let places = self.home_places(p);
        let _ = self.news_rows();
        let news_n = self.news_count(&p.input);
        let rows = self.home_rows(&p.input, &places);
        p.hits.clear();
        let sel = p.sel.min(rows.len());
        p.sel = sel;
        let label = self.label();
        let dim = Style { color: fade(if backdrop != crate::art::Backdrop::Theme { fade(ink, 0.75) } else { t.dim }, up), ..label };
        let foot_y = r.bottom() - self.px(26.0);
        if plate && p.input.trim().is_empty() {
            self.draw_stops(scene, p, &rows, sel, up);
        } else {
            let row_h = self.px(if self.behavior.prompt.compact { 25.0 } else { 34.0 });
            let mut y = y0 + self.px(30.0);
            let (mx, my) = self.mouse;
            if news_n > 0 {
                // The caption: since when, and how it goes.
                let since = self.news.since.filter(|s| crate::journal::now().saturating_sub(*s) < 7 * 86400).map(|s| format!("SINCE {}", crate::journal::when(s).to_uppercase())).unwrap_or_else(|| "SINCE LAST TIME".into());
                let cap = Style { color: fade(self.surface.signal, up), px: self.px(10.0), tracking: self.px(1.2), ..label };
                self.draw_lit(scene, cap, x0, y + self.px(18.0), &format!("WHILE YOU WERE AWAY · {since} · ESC DISMISSES"), dark);
                y += self.px(26.0);
            }
            for (k, row) in rows.iter().enumerate() {
                if y + row_h > foot_y - self.px(8.0) {
                    break;
                }
                if news_n > 0 && k == news_n {
                    // A rule between the news and the usual rows.
                    scene.hline(x0, y + self.px(2.0), line_w, self.px(m::HAIRLINE), fade(ink, 0.25 * up));
                    y += self.px(8.0);
                }
                let rr = Rect::new(x0, y, line_w, row_h);
                let hot = k + 1 == sel || rr.contains(mx, my);
                if k + 1 == sel {
                    scene.rect(Rect::new(x0 - self.px(10.0), y + self.px(6.0), self.px(2.0), row_h - self.px(12.0)), fade(self.surface.signal, up));
                }
                let base = y + row_h / 2.0 + self.px(4.0);
                let num_w = self.px(28.0);
                self.draw_lit(scene, dim, x0, base, &row.num, dark);
                let text = self.fit(label, &row.text, line_w - num_w);
                self.draw_lit(scene, Style { color: fade(ink, if hot { 1.0 } else { 0.75 } * up), ..label }, x0 + num_w, base, &text, dark);
                p.hits.push((rr, k));
                y += row_h;
            }
        }
        // The routes at the foot: marks, not a sentence. The one Enter
        // would take is lit; LAYOUT · ROUTE KEYS turns them off.
        p.keys.clear();
        if self.behavior.prompt.hints {
            self.draw_home_keys(scene, p, foot_y, up, ink, sel, rows.len());
        }
        let _ = dim;
    }

    /// Which way Enter goes, from what is on the line. `None` is an empty
    /// line: nothing is lit, because nothing has been chosen yet.
    fn live_key(&self, input: &str) -> Option<Key> {
        let q = input.trim();
        if q.is_empty() {
            return None;
        }
        if q.starts_with('>') {
            return Some(Key::Shell);
        }
        if q.starts_with('?') {
            return Some(Key::Page);
        }
        if q.starts_with('@') {
            return Some(Key::Ask);
        }
        if std::path::Path::new(q).is_dir() || std::path::Path::new(q).is_file() {
            return Some(Key::Path);
        }
        Some(match self.behavior.prompt.route {
            crate::prompt::Route::Assistant => Key::Ask,
            crate::prompt::Route::Web => Key::Page,
            crate::prompt::Route::Shell => Key::Shell,
            crate::prompt::Route::Automatic => {
                if crate::prompt::looks_like_url(q) { Key::Page } else { Key::Shell }
            }
        })
    }

    /// The routes, as a centred row of marks at the foot. Dim until one
    /// applies; the live one takes the signal and a rule under it. The
    /// pointer names a mark; a click puts its prefix on the line, so the
    /// row teaches the typing rather than describing it.
    fn draw_home_keys(&mut self, scene: &mut Scene, p: &mut HomePane, foot_y: f32, up: f32, ink: [f32; 4], sel: usize, rows_n: usize) {
        if crate::private::enabled() { p.keys.clear(); return; }
        let r = p.rect;
        let live = if sel > 0 { Some(Key::Rows) } else { self.live_key(&p.input) };
        let mut cells = vec![Key::Shell, Key::Page, Key::Ask];
        // The path mark earns its place only when the line names one.
        if live == Some(Key::Path) {
            cells.push(Key::Path);
        }
        if rows_n > 0 {
            cells.push(Key::Rows);
        }
        let isz = self.px(14.0);
        let gap = self.px(30.0);
        let n = cells.len() as f32;
        let total = n * isz + (n - 1.0) * gap;
        if total > r.w - self.px(40.0) {
            return;
        }
        let mut x = r.x + ((r.w - total) / 2.0).round();
        let y = (foot_y - isz).round();
        let (mx, my) = self.mouse;
        let signal = self.surface.signal;
        let reach_pad = self.px(10.0);
        for cell in cells {
            let (icon, words) = cell.look();
            let cr = Rect::new(x, y, isz, isz);
            let reach = crate::touch::grown(cr, reach_pad);
            let hot = reach.contains(mx, my);
            let on = live == Some(cell);
            let color = if on { signal } else if hot { ink } else { fade(ink, 0.34) };
            self.fonts.draw_icon(scene, icon, isz, cr.x, cr.y, fade(color, up));
            if on {
                // The same rule the rows use for the one that is picked.
                scene.rect(Rect::new(cr.x, cr.y + isz + self.px(5.0), isz, self.px(2.0)), fade(signal, up));
            }
            if hot {
                self.tip_words(reach, words);
                self.dirty = true;
            }
            p.keys.push((reach, cell));
            x += isz + gap;
        }
    }

    /// Text over an art: with `dark`, an ink shadow a pixel under the words.
    fn draw_lit(&mut self, scene: &mut Scene, st: Style, x: f32, y: f32, text: &str, dark: bool) -> f32 {
        if dark {
            // Two passes: a soft one two pixels down, a crisp one beneath.
            let d = self.px(1.0);
            let soft = Style { color: [0.0, 0.0, 0.0, 0.35 * st.color[3]], ..st };
            let crisp = Style { color: [0.0, 0.0, 0.0, 0.6 * st.color[3]], ..st };
            self.fonts.draw(scene, soft, x + d * 2.0, y + d * 2.0, text);
            self.fonts.draw(scene, soft, x - d, y + d, text);
            self.fonts.draw(scene, crisp, x + d, y + d, text);
        }
        self.fonts.draw(scene, st, x, y, text)
    }
}

impl App {
    /// The art, running behind the line: the pane is its canvas, the
    /// line's box (and the rows' reach while typing) is what it keeps
    /// clear of, the pointer and the typing and the taps are its inputs.
    fn draw_home_art(&mut self, scene: &mut Scene, p: &mut HomePane, y0: f32) {
        let key = self.behavior.home_art.clone();
        if self.art.as_ref().map(|a| a.key != key).unwrap_or(true) {
            self.art = Some(crate::art::Art::open(&key));
        }
        let r = p.rect;
        let t = self.theme.clone();
        let px = self.px(20.0);
        let line_w = (r.w * if self.behavior.prompt.wide { 0.84 } else { 0.62 }).max(self.px(320.0)).min(r.w - self.px(56.0));
        let x0 = r.x + (r.w - line_w) / 2.0;
        // The rows' reach below the line, from the last frame's rows.
        let line_bottom = y0 - px * 0.78 + px * 0.95 + self.px(14.0);
        let rows = p.hits.last().map(|(rr, _)| (rr.bottom() - line_bottom).max(0.0)).unwrap_or(0.0);
        let (mx, my) = self.mouse;
        let pointer = if r.contains(mx, my) { Some((mx - r.x, my - r.y)) } else { None };
        let env = crate::art::Env {
            w: r.w,
            h: r.h,
            line: [x0 - r.x, y0 - px * 0.78 - r.y, line_w, px * 0.95 + self.px(14.0)],
            rows,
            pointer,
            typed: p.input.clone(),
            taps: std::mem::take(&mut p.taps),
            face: if t.mode == nus_render::Mode::Ink { "ink".into() } else { "paper".into() },
            paper: self.paper(),
            ink: t.ink,
            signal: self.surface.signal,
            dim: t.dim,
            tint: t.tint,
            place: self.place(),
                pieces: Vec::new(),
            procs: Some(self.procs_shared()),
            scale: self.scale,
        };
        let reduced = self.motion.reduced();
        let (cmds, status) = {
            let art = self.art.as_mut().unwrap();
            art.tend();
            // Keep the completed composition visible when animation is disabled;
            // an initial frame can contain only the artwork's entrance delay.
            let cmds = if reduced { art.frame_at(env, 8.0) } else { art.frame(env) };
            (cmds, art.status.clone())
        };
        self.draw_art_cmds(scene, r, cmds);
        if let Some(err) = status {
            let dim = Style { color: self.surface.signal, ..self.label() };
            let line = format!("ART · {} · {}", key.to_uppercase(), err);
            self.fonts.draw(scene, dim, r.x + self.px(28.0), r.bottom() - self.px(48.0), &line);
        }
        // Alive: keep drawing — as the power budget allows (power.rs).
        if !reduced && self.art_wants_frame() {
            self.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_commands() {
        assert!(is_url("https://nus.dev"));
        assert!(is_url("localhost:8000"));
        assert!(is_url("docs.rs"));
        assert!(!is_url("cargo test -p nus-vt"));
        assert!(!is_url("./scripts/run.sh"));
        assert!(!is_url("C:\\Users\\seb"));
    }
}
