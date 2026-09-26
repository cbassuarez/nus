//! A page's own questions — `alert`, `confirm`, `prompt`, leave-page — and
//! a site's sign-in, made as hard to fake as Chromium's (the "sheet from
//! the strip", option A of the reskins).
//!
//! 1. Drawn where a page can't draw. The page owns every pixel of its rect,
//!    so nothing here lives only inside it: the top strip turns (a signal
//!    label, who asks) with a signal rule across the window, and the sheet
//!    hangs from that rule down over the page, which is held faint.
//! 2. The origin is Chromium's, never the page's: the registrable domain
//!    large, the full origin under it, and a frame's own origin when a frame
//!    asks. The page's words are quoted, capped and dim.
//! 3. Only nus gets the keys. The page gets no keys, clicks, wheel or hover
//!    while a question stands (`BrowserTab` drops them), Enter and clicks are
//!    held for `HOLD` after the sheet appears, and a secret field turns on
//!    macOS secure input while it has the caret.
//! 4. A secret never leaves the sheet: it lives in `Secret` (not Clone,
//!    wiped when dropped), not in the `Page` the overlay clones each frame,
//!    and it is drawn, logged and described only as dots.
//! 5. One question per tab; a background tab's waits without taking focus
//!    (the strip names it). After `STOP_AFTER` in a row a page can be told
//!    to stop asking, which Chromium's `suppress_message` carries out.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};

use crate::app::{fade, App, Pane, WebPane};
use crate::interstitial::{Kind, Page};

/// Enter and clicks do nothing this long after a sheet appears, so a page
/// can't raise one under a key or click already on its way.
pub const HOLD: Duration = Duration::from_millis(500);
/// Questions from one page before it can be told to stop.
pub const STOP_AFTER: u32 = 3;
/// The most of a page's words a sheet shows.
const QUOTE_CHARS: usize = 300;

/// Text typed into a secret field. Not Clone; zeroed when dropped.
#[derive(Default)]
pub struct Secret(String);

impl Secret {
    pub fn push_str(&mut self, s: &str) {
        // Grow by hand so the old buffer is wiped, not left for the allocator.
        if self.0.len() + s.len() > self.0.capacity() {
            let mut next = String::with_capacity((self.0.len() + s.len()).max(64) * 2);
            next.push_str(&self.0);
            wipe(&mut self.0);
            self.0 = next;
        }
        self.0.push_str(s);
    }
    pub fn pop(&mut self) {
        if let Some(c) = self.0.chars().last() {
            let at = self.0.len() - c.len_utf8();
            // SAFETY: zeroing the tail leaves valid UTF-8 up to `at`.
            unsafe { self.0.as_bytes_mut()[at..].iter_mut().for_each(|b| std::ptr::write_volatile(b, 0)) };
            self.0.truncate(at);
        }
    }
    pub fn chars(&self) -> usize {
        self.0.chars().count()
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(••••)")
    }
}

/// Overwrite a string's bytes before letting it go.
pub fn wipe(s: &mut String) {
    // SAFETY: zero bytes are valid UTF-8.
    unsafe { s.as_bytes_mut().iter_mut().for_each(|b| std::ptr::write_volatile(b, 0)) };
    s.clear();
}

thread_local! {
    /// Questions each page (browser id) has asked since it last navigated,
    /// and the pages told to stop.
    static ASKED: RefCell<HashMap<i32, u32>> = RefCell::new(HashMap::new());
    static STOPPED: RefCell<HashSet<i32>> = RefCell::new(HashSet::new());
}

/// A page is about to ask: false when it was told to stop (the caller sets
/// `suppress_message` and returns 0). Counts the question otherwise.
pub fn may_ask(browser: i32) -> bool {
    if STOPPED.with(|s| s.borrow().contains(&browser)) {
        return false;
    }
    ASKED.with(|a| *a.borrow_mut().entry(browser).or_default() += 1);
    true
}

pub fn asked(browser: i32) -> u32 {
    ASKED.with(|a| a.borrow().get(&browser).copied().unwrap_or(0))
}

pub fn stop(browser: i32) {
    STOPPED.with(|s| s.borrow_mut().insert(browser));
}

/// The page navigated (or closed): it starts over.
pub fn reset(browser: i32) {
    ASKED.with(|a| a.borrow_mut().remove(&browser));
    STOPPED.with(|s| s.borrow_mut().remove(&browser));
}

/// What a sheet is, read from the page the handlers built.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    SignIn,
    Leave,
    Prompt,
    Confirm,
    Alert,
}

fn ask_of(page: &Page) -> Ask {
    let has = |v: &str| page.acts.iter().any(|a| a.verb == v);
    if has("signin") {
        Ask::SignIn
    } else if has("stay") {
        Ask::Leave
    } else if !page.fields.is_empty() {
        Ask::Prompt
    } else if has("cancel") {
        Ask::Confirm
    } else {
        Ask::Alert
    }
}

/// `scheme://host[:port]`, and the host, from an address Chromium gave.
fn origin(url: &str) -> Option<(String, String)> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_string();
    let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some((format!("{}://{host}{port}", u.scheme()), host))
}

/// The part of a host no one else can register under: `acme.dev` for
/// `intranet.acme.dev`, `bbc.co.uk` for `news.bbc.co.uk`. Addresses and
/// single-label hosts stand as they are.
pub fn registrable(host: &str) -> String {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    if h.parse::<std::net::IpAddr>().is_ok() || h.starts_with('[') || !h.contains('.') {
        return h;
    }
    psl::domain_str(&h).map(str::to_string).unwrap_or(h)
}

/// Characters that reorder text around them: a page could use them to make
/// its words read as something else.
fn bidi(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// A page's words as a sheet shows them: at most `QUOTE_CHARS`, at most
/// six lines, control characters out.
fn quoted(message: &str) -> String {
    let mut out = String::new();
    for (i, line) in message.lines().take(6).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.extend(line.chars().filter(|c| !c.is_control() && !bidi(*c)));
    }
    if out.chars().count() > QUOTE_CHARS || message.lines().count() > 6 {
        out = out.chars().take(QUOTE_CHARS).collect::<String>().trim_end().to_string() + "…";
    }
    out
}

fn ordinal(n: u32) -> String {
    match n {
        1 => "first".into(),
        2 => "second".into(),
        3 => "third".into(),
        4 => "fourth".into(),
        5 => "fifth".into(),
        n => format!("{n}th"),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Act(String),
    Field(usize),
    Stop,
}

/// The app's side: secrets by pane, when each sheet appeared, what the sheet
/// drawn this frame answers to, and macOS secure input.
#[derive(Default)]
pub struct State {
    secrets: HashMap<(u64, bool), Vec<Secret>>,
    shown: HashMap<(u64, bool), (String, Instant)>,
    hits: Vec<(Rect, Hit)>,
    drawn: Option<(u64, bool)>,
    secure: bool,
}

impl Drop for State {
    fn drop(&mut self) {
        secure_input(&mut self.secure, false);
    }
}

#[cfg(target_os = "macos")]
fn secure_input(on: &mut bool, want: bool) {
    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn EnableSecureEventInput() -> i32;
        fn DisableSecureEventInput() -> i32;
    }
    if *on != want {
        // Balanced: the system counts enables against disables.
        unsafe {
            if want { EnableSecureEventInput() } else { DisableSecureEventInput() };
        }
        *on = want;
    }
}

#[cfg(not(target_os = "macos"))]
fn secure_input(on: &mut bool, want: bool) {
    *on = want;
}

/// Chromium's id for the page's browser: what the question counts are kept by.
fn browser_id(w: &WebPane) -> Option<i32> {
    use cef::ImplBrowser;
    w.tab.browser.as_ref().map(|b| b.identifier())
}

/// The question on a web pane, if it is one of these.
fn dialog_of(w: &WebPane) -> Option<Page> {
    w.tab.shared.borrow().overlay.clone().filter(|p| p.kind == Kind::Dialog)
}

impl App {
    fn dialog_pane(&mut self, id: u64, right: bool) -> Option<&mut WebPane> {
        let tab = self.tabs.iter_mut().find(|t| t.id == id)?;
        match if right { tab.right.as_mut()? } else { &mut tab.left } {
            Pane::Web(w) => Some(w),
            _ => None,
        }
    }

    /// The visible pane with a question: the focused one first.
    fn asking_pane(&self) -> Option<(u64, bool, Rect)> {
        let tab = self.tabs.get(self.active)?;
        let order = if tab.focus_right { [true, false] } else { [false, true] };
        order.into_iter().find_map(|right| {
            let p = if right { tab.right.as_ref()? } else { &tab.left };
            match p {
                Pane::Web(w) if dialog_of(w).is_some() => Some((tab.id, right, w.page)),
                _ => None,
            }
        })
    }

    /// Background tabs with a question waiting, by number.
    fn waiting_tabs(&self) -> Vec<usize> {
        self.tabs.iter().enumerate().filter(|(i, t)| *i != self.active && std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| matches!(p, Pane::Web(w) if dialog_of(w).is_some()))).map(|(i, _)| i + 1).collect()
    }

    fn held(&self, key: (u64, bool)) -> bool {
        self.page_dialog.shown.get(&key).is_some_and(|(_, at)| crate::clock::since(*at) < HOLD)
    }

    /// Instead of `draw_overlay` for a question: the page held faint. The
    /// sheet itself is drawn with the chrome, by `draw_page_dialog`.
    pub(crate) fn draw_dialog_scrim(&mut self, scene: &mut Scene, w: &mut WebPane) {
        w.overlay_hits.clear();
        scene.rect(w.page, fade(self.paper(), 0.65));
    }

    /// After everything else: the strip turns, and the sheet hangs from it.
    pub(crate) fn draw_page_dialog(&mut self, scene: &mut Scene) {
        self.page_dialog.hits.clear();
        let asking = self.asking_pane();
        self.page_dialog.drawn = asking.map(|(id, right, _)| (id, right));
        // Sheets that went: their secrets go with them.
        let live: HashSet<(u64, bool)> = self.tabs.iter().flat_map(|t| {
            let id = t.id;
            std::iter::once((false, &t.left)).chain(t.right.as_ref().map(|p| (true, p))).filter_map(move |(right, p)| match p {
                Pane::Web(w) if dialog_of(w).is_some() => Some((id, right)),
                _ => None,
            })
        }).collect();
        self.page_dialog.secrets.retain(|k, _| live.contains(k));
        self.page_dialog.shown.retain(|k, _| live.contains(k));
        let waiting = self.waiting_tabs();
        let Some((id, right, page_rect)) = asking else {
            secure_input(&mut self.page_dialog.secure, false);
            if !waiting.is_empty() {
                self.draw_dialog_strip(scene, None, &waiting);
            }
            return;
        };
        let Some(w) = self.dialog_pane(id, right) else { return };
        let Some(page) = dialog_of(w) else { return };
        let browser = browser_id(w);
        let main_url = w.tab.shared.borrow().url.clone();
        let focus = page.field.min(page.fields.len().saturating_sub(1));
        let key = (id, right);
        let now = crate::clock::now();
        let fresh = self.page_dialog.shown.get(&key).is_none_or(|(t, _)| *t != page.token);
        if fresh {
            self.page_dialog.shown.insert(key, (page.token.clone(), now));
            self.page_dialog.secrets.insert(key, page.fields.iter().map(|_| Secret::default()).collect());
        }
        let secret_focus = page.fields.get(focus).is_some_and(|f| f.secret);
        secure_input(&mut self.page_dialog.secure, secret_focus && self.window_focused);
        self.draw_dialog_strip(scene, Some((&page, &main_url)), &waiting);
        self.draw_sheet(scene, key, &page, &main_url, browser, page_rect);
        if self.held(key) {
            self.dirty = true;
        }
    }

    /// The top strip, turned: a signal label, who asks, and a signal rule
    /// under it across the window. Drawn over the strip, which a page can't
    /// reach; with no strip (focus mode) it is drawn anyway.
    fn draw_dialog_strip(&mut self, scene: &mut Scene, asking: Option<(&Page, &str)>, waiting: &[usize]) {
        let t = self.theme.clone();
        let (w, _) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let strip = self.strip_rect();
        scene.layer(None);
        let label = self.label_strong();
        let base = strip.y + self.px(20.0);
        let mut x = strip.x + self.px(18.0) + self.traffic_width();
        if let Some((page, main_url)) = asking {
            scene.rect(Rect::new(strip.x, strip.y, strip.w, strip.h), self.paper());
            let who = page_origin(page, main_url);
            let (word, says) = match ask_of(page) {
                Ask::SignIn => ("SIGN-IN", format!("{} wants a username and password", who.host)),
                Ask::Leave => ("LEAVE?", format!("{} has changes that may not be saved", who.host)),
                _ if who.frame => ("ASKS", format!("a frame from {} asks", who.host)),
                _ => ("ASKS", format!("{} asks", who.host)),
            };
            let lw = self.fonts.measure(label, word) + self.px(16.0);
            let chip = Rect::new(x, strip.y + self.px(6.0), lw, strip.h - self.px(12.0));
            scene.rect(chip, self.surface.signal);
            self.fonts.draw(scene, Style { color: self.on_fill(self.surface.signal), ..label }, x + self.px(8.0), base - self.px(1.0), word);
            x += lw + self.px(12.0);
            let ui = self.ui();
            let room = strip.right() - x - self.px(240.0);
            let says = self.fit(ui, &says, room.max(self.px(80.0)));
            x += self.fonts.draw(scene, ui, x, base, &says) + self.px(14.0);
            scene.rect(Rect::new(0.0, strip.bottom(), w, self.px(2.0)), self.surface.signal);
        }
        if !waiting.is_empty() {
            let dim = Style { color: t.dim, ..self.label() };
            let list = waiting.iter().map(|n| format!("{n:02}")).collect::<Vec<_>>().join(" ");
            let text = format!("· {list} {} a question", if waiting.len() == 1 { "waits with" } else { "wait with" });
            let text = text.to_uppercase();
            if asking.is_none() {
                // Only a background question: say so, beside the crumb.
                let tw = self.fonts.measure(dim, &text);
                let at = strip.right() - self.px(260.0) - tw;
                scene.rect(Rect::new(at - self.px(8.0), strip.y + self.px(4.0), tw + self.px(16.0), strip.h - self.px(8.0)), self.paper());
                self.fonts.draw(scene, Style { color: self.surface.signal, ..dim }, at, base, &text);
            } else {
                self.fonts.draw(scene, dim, x, base, &text);
            }
        }
    }

    fn draw_sheet(&mut self, scene: &mut Scene, key: (u64, bool), page: &Page, main_url: &str, browser: Option<i32>, page_rect: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let ask = ask_of(page);
        let who = page_origin(page, main_url);
        let strip = self.strip_rect();
        let win_w = self.target.size.0 as f32;
        let sw = self.px(620.0).min(win_w - self.px(32.0));
        let sx = (page_rect.x + (page_rect.w - sw) / 2.0).clamp(self.px(12.0), (win_w - sw - self.px(20.0)).max(self.px(12.0))).round();
        let top = strip.bottom();
        let pad = self.px(28.0);
        let inner = sw - 2.0 * pad;
        let ui = self.ui();
        let strong = self.ui_strong();
        let dim = Style { color: t.dim, ..ui };
        let cap = Style { color: t.dim, ..self.label() };
        let big = Style { px: self.px(28.0), ..strong };
        let line = self.px(m::UI_PX * 1.5);
        let count = browser.map(asked).unwrap_or(0);
        let hot = page.url.starts_with("http:") && ask == Ask::SignIn;

        // Lay out first, then draw: the sheet's height is its content's.
        let mut rows: Vec<(f32, Box<dyn Fn(&mut App, &mut Scene, f32, f32)>)> = Vec::new();
        let heading = match ask {
            Ask::SignIn if hot => "SIGN-IN · NOT A PRIVATE CONNECTION".to_string(),
            Ask::SignIn => "SIGN-IN · PRIVATE CONNECTION".to_string(),
            Ask::Leave => "LEAVE THIS PAGE?".into(),
            _ if who.frame => "A FRAME ON THIS PAGE ASKS".into(),
            _ => "THE PAGE ASKS".into(),
        };
        let heading = if count >= 2 && ask != Ask::SignIn { format!("{heading} · {} TIME", ordinal(count).to_uppercase()) } else { heading };
        let heading_color = if hot { self.surface.signal } else { t.dim };
        rows.push((self.px(22.0), Box::new(move |a, s, x, y| { a.fonts.draw(s, Style { color: heading_color, ..cap }, x, y + a.px(12.0), &heading); })));
        let domain = who.domain.clone();
        rows.push((self.px(44.0), Box::new(move |a, s, x, y| { a.fonts.draw(s, big, x, y + a.px(32.0), &domain); })));
        let (prefix, rest) = who.split();
        rows.push((self.px(24.0), Box::new(move |a, s, x, y| {
            let px = a.fonts.draw(s, dim, x, y + a.px(14.0), &prefix);
            let dx = a.fonts.draw(s, ui, x + px, y + a.px(14.0), &rest.0);
            a.fonts.draw(s, dim, x + px + dx, y + a.px(14.0), &rest.1);
        })));
        if who.frame {
            let note = format!("Not {}: a frame from another site, inside it.", registrable(&origin(main_url).map(|o| o.1).unwrap_or_default()));
            let lines = crate::reader::wrap(&self.fonts, ui, &note, inner);
            let h = lines.len() as f32 * line + self.px(6.0);
            rows.push((h, Box::new(move |a, s, x, y| { for (i, l) in lines.iter().enumerate() { a.fonts.draw(s, ui, x, y + a.px(16.0) + i as f32 * line, l); } })));
        }
        // The page's words, quoted, dim, under what nus says.
        let message = match ask {
            Ask::Alert | Ask::Confirm | Ask::Prompt => Some(quoted(&page.body)),
            _ => None,
        };
        if let Some(msg) = message.filter(|m| !m.trim().is_empty()) {
            let lines: Vec<String> = msg.split('\n').flat_map(|l| crate::reader::wrap(&self.fonts, dim, l, inner - self.px(14.0))).collect();
            let lines: Vec<String> = { let n = lines.len(); lines.into_iter().enumerate().map(|(i, l)| match (i, n) { (0, 1) => format!("“{l}”"), (0, _) => format!("“{l}"), (i, n) if i + 1 == n => format!("{l}”"), _ => l }).collect() };
            let h = self.px(26.0) + lines.len() as f32 * line + self.px(6.0);
            rows.push((h, Box::new(move |a, s, x, y| {
                a.fonts.draw(s, cap, x, y + a.px(24.0), "THE PAGE SAYS");
                let qy = y + a.px(32.0);
                s.rect(Rect::new(x, qy, a.px(1.5), lines.len() as f32 * line), t.dim);
                for (i, l) in lines.iter().enumerate() { a.fonts.draw(s, dim, x + a.px(12.0), qy + a.px(14.0) + i as f32 * line, l); }
            })));
        }
        let explain = match ask {
            Ask::SignIn if hot => Some(format!("Your username and password go only to {}, but this connection isn't private: anyone on the network can read them.", who.host)),
            Ask::SignIn => Some(format!("Your username and password go only to {}.", who.host)),
            Ask::Leave => Some("Changes you made may not be saved.".into()),
            _ => None,
        };
        if let Some(text) = explain {
            let lines = crate::reader::wrap(&self.fonts, ui, &text, inner);
            let color = if hot { self.surface.signal } else { ink };
            let h = self.px(10.0) + lines.len() as f32 * line;
            rows.push((h, Box::new(move |a, s, x, y| { for (i, l) in lines.iter().enumerate() { a.fonts.draw(s, Style { color, ..ui }, x, y + a.px(24.0) + i as f32 * line, l); } })));
        }
        // Fields: a label, a box; a secret one shows dots and says so.
        let secrets: Vec<usize> = self.page_dialog.secrets.get(&key).map(|v| v.iter().map(Secret::chars).collect()).unwrap_or_default();
        for (i, f) in page.fields.iter().enumerate() {
            let value = if f.secret { "•".repeat(secrets.get(i).copied().unwrap_or(0)) } else { f.value.clone() };
            let on = i == page.field.min(page.fields.len() - 1);
            let label_text = f.label.to_uppercase();
            let secret = f.secret;
            rows.push((self.px(46.0), Box::new(move |a, s, x, y| {
                let bx = x + a.px(96.0);
                let b = Rect::new(bx, y + a.px(12.0), inner - a.px(96.0), a.px(34.0));
                a.fonts.draw(s, cap, x, b.y + a.px(21.0), &label_text);
                s.outline(b, a.px(if on { 2.0 } else { 1.0 }), ink);
                let mut tx = b.x + a.px(10.0);
                let shown = a.fit(ui, &value, b.w - a.px(if secret { 150.0 } else { 24.0 }));
                tx += a.fonts.draw(s, Style { tracking: if secret { a.px(2.0) } else { 0.0 }, ..ui }, tx, b.y + a.px(22.0), &shown);
                if on {
                    s.rect(Rect::new(tx + a.px(1.0), b.y + a.px(9.0), a.px(8.0), a.px(16.0)), ink);
                }
                if secret {
                    let tag = "SECURE INPUT";
                    let tw = a.fonts.measure(cap, tag);
                    let lx = b.right() - a.px(10.0) - tw;
                    a.fonts.draw(s, cap, lx, b.y + a.px(21.0), tag);
                    a.fonts.draw_icon(s, icons::LOCK_KEY, a.px(13.0), lx - a.px(18.0), b.y + a.px(10.0), t.dim);
                }
                a.page_dialog.hits.push((b, Hit::Field(i)));
            })));
        }
        // Commands: the safe one first and filled, with the hold drawn as a
        // signal rule filling under it; the rest ruled.
        let held_for = self.page_dialog.shown.get(&key).map(|(_, at)| crate::clock::since(*at)).unwrap_or(HOLD);
        let arm = (held_for.as_secs_f32() / HOLD.as_secs_f32()).min(1.0);
        let acts = page.acts.clone();
        let footer = ask == Ask::SignIn;
        rows.push((self.px(64.0), Box::new(move |a, s, x, y| {
            let label = a.label_strong();
            let mut bx = x;
            let by = y + a.px(24.0);
            for (i, act) in acts.iter().enumerate() {
                let text = act.label.to_uppercase();
                let keyw = if act.key.is_empty() { 0.0 } else { a.fonts.measure(label, act.key) + a.px(10.0) };
                let bw = a.fonts.measure(label, &text) + keyw + a.px(28.0);
                let b = Rect::new(bx, by, bw, a.px(32.0));
                let primary = i == 0 && !act.unsafe_;
                let fg = if primary { a.on_fill(ink) } else if act.unsafe_ { t.dim } else { ink };
                if primary { s.rect(b, ink); } else { s.outline(b, a.px(1.5), if act.unsafe_ { t.dim } else { ink }); }
                a.fonts.draw(s, Style { color: fg, ..label }, b.x + a.px(14.0), b.y + a.px(21.0), &text);
                if !act.key.is_empty() {
                    a.fonts.draw(s, Style { color: fade(fg, 0.7), ..label }, b.right() - a.px(14.0) - keyw + a.px(10.0), b.y + a.px(21.0), act.key);
                }
                if primary && arm < 1.0 {
                    s.rect(Rect::new(b.x, b.bottom() + a.px(3.0), b.w * arm, a.px(2.0)), a.surface.signal);
                }
                a.page_dialog.hits.push((b, Hit::Act(act.verb.clone())));
                bx += bw + a.px(10.0);
            }
            if footer {
                let text = "THE PAGE CAN'T SEE THIS OR TYPE HERE";
                let tw = a.fonts.measure(cap, text);
                if bx + tw < x + inner {
                    a.fonts.draw(s, cap, x + inner - tw, by + a.px(21.0), text);
                }
            }
        })));
        if count >= STOP_AFTER && ask != Ask::SignIn && browser.is_some() {
            let name = format!("STOP {} ASKING", who.domain.to_uppercase());
            rows.push((self.px(58.0), Box::new(move |a, s, x, y| {
                s.hline(x, y + a.px(10.0), inner, a.px(1.0), t.tint);
                let label = a.label_strong();
                let bw = a.fonts.measure(label, &name) + a.px(28.0);
                let b = Rect::new(x, y + a.px(20.0), bw, a.px(32.0));
                s.outline(b, a.px(1.5), ink);
                a.fonts.draw(s, label, b.x + a.px(14.0), b.y + a.px(21.0), &name);
                a.fonts.draw(s, dim, b.right() + a.px(12.0), b.y + a.px(21.0), "until it loads a new page");
                a.page_dialog.hits.push((b, Hit::Stop));
            })));
        }
        let content: f32 = rows.iter().map(|(h, _)| *h).sum();
        let band = self.px(6.0);
        let sh = band + self.px(8.0) + content + self.px(24.0);
        let sheet = Rect::new(sx, top, sw, sh);
        // Hard shadow, paper, the signal band continuing the strip's rule,
        // and a 2px edge on three sides: it hangs from the chrome.
        scene.rect(Rect::new(sheet.x + self.px(8.0), sheet.y + self.px(8.0), sheet.w, sheet.h), ink);
        scene.rect(sheet, paper);
        scene.rect(Rect::new(sheet.x, sheet.y, sheet.w, band), self.surface.signal);
        let e = self.px(m::FLOATING);
        scene.rect(Rect::new(sheet.x, sheet.y, e, sheet.h), ink);
        scene.rect(Rect::new(sheet.right() - e, sheet.y, e, sheet.h), ink);
        scene.rect(Rect::new(sheet.x, sheet.bottom() - e, sheet.w, e), ink);
        let mut y = sheet.y + band + self.px(8.0);
        for (h, draw) in rows {
            draw(self, scene, sheet.x + pad, y);
            y += h;
        }
    }

    /// Keys while a question stands on the focused pane. None: no question
    /// here. Some(true): taken. Some(false): an app chord, let through (it
    /// never reaches the page: `BrowserTab` drops input while a question
    /// stands).
    pub(crate) fn page_dialog_key(&mut self, ev: &crate::app::KeyIn) -> Option<bool> {
        use winit::keyboard::{Key, NamedKey};
        let (id, right) = {
            let tab = self.tabs.get(self.active)?;
            (tab.id, tab.focus_right && tab.right.is_some())
        };
        let key = (id, right);
        let page = dialog_of(self.dialog_pane(id, right)?)?;
        if ev.state != winit::event::ElementState::Pressed {
            return Some(true);
        }
        let mods = self.mods;
        let chord = if cfg!(target_os = "macos") { mods.super_key() } else { mods.control_key() };
        let held = self.held(key);
        let run = |app: &mut App, verb: &str| {
            app.page_dialog_act(id, right, verb);
            app.dirty = true;
            Some(true)
        };
        match &ev.logical_key {
            Key::Named(NamedKey::Enter) if chord => {
                if held { return Some(true); }
                let verb = page.acts.iter().find(|a| a.key == "⌘↵").map(|a| a.verb.clone());
                return verb.map_or(Some(true), |v| run(self, &v));
            }
            Key::Named(NamedKey::Enter) => {
                if held { return Some(true); }
                let verb = page.acts.iter().find(|a| a.key == "↵" && !a.unsafe_).or(page.acts.first()).map(|a| a.verb.clone());
                return verb.map_or(Some(true), |v| run(self, &v));
            }
            Key::Named(NamedKey::Escape) => {
                let verb = page.acts.iter().find(|a| a.key == "Esc" || matches!(a.verb.as_str(), "cancel" | "stay")).or(page.acts.first()).map(|a| a.verb.clone());
                // Esc is always the safe answer, so it is never held.
                return verb.map_or(Some(true), |v| run(self, &v));
            }
            _ => {}
        }
        if page.fields.is_empty() {
            return Some(!chord);
        }
        let k = page.field.min(page.fields.len() - 1);
        let secret = page.fields[k].secret;
        let paste = chord && matches!(&ev.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"));
        let mut typed: Option<String> = None;
        let mut back = false;
        match &ev.logical_key {
            Key::Named(NamedKey::Tab) => {
                let n = page.fields.len();
                let next = if mods.shift_key() { (k + n - 1) % n } else { (k + 1) % n };
                if let Some(w) = self.dialog_pane(id, right) {
                    if let Some(o) = w.tab.shared.borrow_mut().overlay.as_mut() { o.field = next; }
                }
                self.dirty = true;
                return Some(true);
            }
            Key::Named(NamedKey::Backspace) => back = true,
            _ if paste => {
                let text = arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default();
                typed = Some(text.lines().next().unwrap_or("").to_string());
            }
            _ if chord || mods.control_key() || mods.alt_key() && ev.text.is_none() => return Some(false),
            _ => typed = ev.text.as_deref().filter(|t| !t.chars().any(char::is_control)).map(str::to_string),
        }
        if secret {
            if let Some(s) = self.page_dialog.secrets.get_mut(&key).and_then(|v| v.get_mut(k)) {
                if back { s.pop(); }
                if let Some(mut t) = typed { s.push_str(&t); wipe(&mut t); }
            }
        } else if let Some(w) = self.dialog_pane(id, right) {
            if let Some(o) = w.tab.shared.borrow_mut().overlay.as_mut() {
                if back { o.fields[k].value.pop(); }
                if let Some(t) = typed { o.fields[k].value.push_str(&t); }
            }
        }
        self.dirty = true;
        Some(true)
    }

    /// Any button, pressed or released: the sheet's own targets answer a
    /// left press (after the hold); the asking page takes nothing at all.
    /// Everything else (strip, sidebar, other panes) is the app's.
    pub(crate) fn page_dialog_mouse(&mut self, pressed: bool, x: f32, y: f32) -> bool {
        let Some((id, right, page_rect)) = self.asking_pane() else { return false };
        let hit = self.page_dialog.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| h.clone());
        match hit {
            Some(h) if pressed => {
                let key = (id, right);
                match h {
                    Hit::Field(i) => {
                        if let Some(w) = self.dialog_pane(id, right) {
                            if let Some(o) = w.tab.shared.borrow_mut().overlay.as_mut() { o.field = i; }
                        }
                    }
                    _ if self.held(key) => {}
                    Hit::Act(v) => self.page_dialog_act(id, right, &v),
                    Hit::Stop => {
                        if let Some(b) = self.dialog_pane(id, right).and_then(|w| browser_id(w)) {
                            stop(b);
                        }
                        self.page_dialog_act(id, right, "cancel");
                    }
                }
                self.dirty = true;
                true
            }
            Some(_) => true,
            None => page_rect.contains(x, y),
        }
    }

    /// The wheel over a page with a question up goes nowhere.
    pub(crate) fn page_dialog_blocks(&self, x: f32, y: f32) -> bool {
        self.asking_pane().is_some_and(|(_, _, r)| r.contains(x, y))
            || self.page_dialog.hits.iter().any(|(r, _)| r.contains(x, y))
    }

    /// Answer: the typed secrets join the fields for the handoff to
    /// Chromium and are wiped right after.
    pub(crate) fn page_dialog_act(&mut self, id: u64, right: bool, verb: &str) {
        let key = (id, right);
        let secrets = self.page_dialog.secrets.remove(&key);
        self.page_dialog.shown.remove(&key);
        let Some(w) = self.dialog_pane(id, right) else { return };
        let Some(page) = dialog_of(w) else { return };
        let yes = matches!(verb, "ok" | "signin" | "leave" | "reload");
        let mut fields = page.fields.clone();
        if let Some(secrets) = &secrets {
            for (f, s) in fields.iter_mut().zip(secrets) {
                if f.secret {
                    f.value.push_str(s.expose());
                }
            }
        }
        w.tab.answer_dialog(yes, &fields);
        for f in fields.iter_mut() {
            wipe(&mut f.value);
        }
        drop(secrets);
        secure_input(&mut self.page_dialog.secure, false);
        self.dirty = true;
    }

    /// A screen reader pressed one of the sheet's commands.
    pub(crate) fn page_dialog_access_act(&mut self, i: usize) {
        let Some((id, right, _)) = self.asking_pane() else { return };
        let verb = self.dialog_pane(id, right).and_then(|w| dialog_of(w)).and_then(|p| p.acts.get(i).map(|a| a.verb.clone()));
        if let Some(v) = verb {
            self.page_dialog_act(id, right, &v);
        }
    }

    /// For checks: the centre of a sheet target (`ok`, `stop`, `field:1`).
    pub(crate) fn page_dialog_target(&self, name: &str) -> Option<(f32, f32)> {
        let want = match name {
            "stop" => Hit::Stop,
            n if n.starts_with("field:") => Hit::Field(n[6..].parse().ok()?),
            n => Hit::Act(n.into()),
        };
        self.page_dialog.hits.iter().find(|(_, h)| *h == want).map(|(r, _)| (r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    /// For checks: (secret values found in the cloned page, secret chars
    /// held in the sheet's own buffer, still held, secure input on).
    pub(crate) fn page_dialog_probe(&self) -> Option<(usize, usize, bool, bool)> {
        let (id, right, _) = self.asking_pane()?;
        let tab = self.tabs.iter().find(|t| t.id == id)?;
        let Pane::Web(w) = (if right { tab.right.as_ref()? } else { &tab.left }) else { return None };
        let page = dialog_of(w)?;
        let leaked = page.fields.iter().filter(|f| f.secret).map(|f| f.value.len()).sum();
        let held = self.page_dialog.secrets.get(&(id, right)).map(|v| v.iter().map(Secret::chars).sum()).unwrap_or(0);
        Some((leaked, held, self.held((id, right)), self.page_dialog.secure))
    }

    /// For AccessKit: the sheet as a dialog, a secret field with no value.
    pub(crate) fn page_dialog_access(&self) -> Option<(String, String, Vec<(String, bool, String)>, Vec<String>)> {
        let (id, right, _) = self.asking_pane()?;
        let tab = self.tabs.iter().find(|t| t.id == id)?;
        let Pane::Web(w) = (if right { tab.right.as_ref()? } else { &tab.left }) else { return None };
        let page = dialog_of(w)?;
        let main_url = w.tab.shared.borrow().url.clone();
        let who = page_origin(&page, &main_url);
        let title = match ask_of(&page) {
            Ask::SignIn => format!("Sign in to {}", who.host),
            Ask::Leave => format!("Leave {}?", who.host),
            _ if who.frame => format!("A frame from {} asks", who.domain),
            _ => format!("{} asks", who.domain),
        };
        let says = match ask_of(&page) {
            Ask::Alert | Ask::Confirm | Ask::Prompt => format!("The page says: {}", quoted(&page.body)),
            _ => String::new(),
        };
        let fields = page.fields.iter().map(|f| (f.label.clone(), f.secret, if f.secret { String::new() } else { f.value.clone() })).collect();
        Some((title, says, fields, page.acts.iter().map(|a| a.label.clone()).collect()))
    }
}

/// Who asks, from Chromium's address for the question.
struct Who {
    domain: String,
    host: String,
    origin: String,
    frame: bool,
}

impl Who {
    /// The origin in three runs: before the domain, the domain, after it.
    fn split(&self) -> (String, (String, String)) {
        match self.origin.rfind(&self.domain) {
            Some(at) if !self.domain.is_empty() => (self.origin[..at].to_string(), (self.domain.clone(), self.origin[at + self.domain.len()..].to_string())),
            _ => (String::new(), (self.origin.clone(), String::new())),
        }
    }
}

fn page_origin(page: &Page, main_url: &str) -> Who {
    match origin(&page.url) {
        Some((o, host)) => {
            let frame = origin(main_url).is_some_and(|(m, _)| m != o);
            Who { domain: registrable(&host), host, origin: o, frame }
        }
        // No origin Chromium would vouch for (a sandboxed or opaque frame):
        // never borrow the page's name for it.
        None => Who { domain: "an unnamed frame".into(), host: "an unnamed frame".into(), origin: "no address".into(), frame: true },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrable_domains() {
        assert_eq!(registrable("intranet.acme.dev"), "acme.dev");
        assert_eq!(registrable("news.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(registrable("user.github.io"), "user.github.io");
        assert_eq!(registrable("localhost"), "localhost");
        assert_eq!(registrable("192.168.1.4"), "192.168.1.4");
    }

    #[test]
    fn a_page_cannot_speak_at_length() {
        let long = "nus: ".to_string() + &"x".repeat(1000);
        let q = quoted(&long);
        assert!(q.chars().count() <= QUOTE_CHARS + 1 && q.ends_with('…'));
        assert_eq!(quoted("a\u{202e}b\u{7}c\u{2066}"), "abc");
        assert!(quoted(&"line\n".repeat(20)).ends_with('…'));
    }

    #[test]
    fn a_frame_is_named_for_itself() {
        let mut p = Page::alert("https://cdn.tracker-ads.net/x", "hi");
        let who = page_origin(&p, "https://news.example/world");
        assert!(who.frame);
        assert_eq!(who.domain, "tracker-ads.net");
        p.url = String::new();
        let who = page_origin(&p, "https://news.example/world");
        assert!(who.frame && who.domain == "an unnamed frame");
        let same = page_origin(&Page::alert("https://news.example/a", "hi"), "https://news.example/world");
        assert!(!same.frame);
    }

    #[test]
    fn secrets_are_wiped() {
        let mut s = Secret::default();
        s.push_str("hunter2");
        s.pop();
        assert_eq!(s.expose(), "hunter");
        assert_eq!(format!("{s:?}"), "Secret(••••)");
        let mut t = String::from("pw");
        wipe(&mut t);
        assert!(t.is_empty());
    }

    #[test]
    fn a_page_can_be_told_to_stop() {
        reset(77);
        for _ in 0..STOP_AFTER {
            assert!(may_ask(77));
        }
        assert_eq!(asked(77), STOP_AFTER);
        stop(77);
        assert!(!may_ask(77));
        reset(77);
        assert!(may_ask(77));
    }
}
