//! Per-site settings: the gear at the end of a page's URL row opens a
//! panel for the host — zoom (remembered), autoplay, JavaScript, cookies
//! (block, or clear), the rules' boosts, content blocking, and the
//! permissions the site was given. Kept in profile/sites.json; the
//! request handlers read the same table, so blocking and cookies apply
//! as the page loads.

use crate::app::{Caps, App, Pane, WebPane};
use cef::{ImplBrowserHost, ImplCookieManager};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, RwLock};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SitePrefs {
    /// Percent; 100 is the page's own.
    #[serde(default = "default_zoom")]
    pub zoom: u32,
    #[serde(default = "yes")]
    pub autoplay: bool,
    #[serde(default = "yes")]
    pub js: bool,
    #[serde(default = "yes")]
    pub cookies: bool,
    #[serde(default = "yes")]
    pub boosts: bool,
    #[serde(default = "yes")]
    pub blocking: bool,
}

fn yes() -> bool {
    true
}

/// BROWSER · DEFAULT ZOOM: what a site with no zoom of its own gets.
pub static DEFAULT_ZOOM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

pub fn default_zoom() -> u32 {
    DEFAULT_ZOOM.load(std::sync::atomic::Ordering::Relaxed).clamp(25, 500)
}

impl Default for SitePrefs {
    fn default() -> Self {
        SitePrefs { zoom: default_zoom(), autoplay: true, js: true, cookies: true, boosts: true, blocking: true }
    }
}

impl SitePrefs {
    /// Nothing but the defaults: the entry can go.
    pub fn is_default(&self) -> bool {
        *self == SitePrefs::default()
    }
}

pub static SITES: LazyLock<RwLock<HashMap<String, SitePrefs>>> = LazyLock::new(|| RwLock::new(load()));

fn path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("sites.json")
}

fn load() -> HashMap<String, SitePrefs> {
    // Salvaged by host when the file is from another nus (store.rs).
    crate::store::read_json::<HashMap<String, SitePrefs>>(&path()).value
}

fn save(map: &HashMap<String, SitePrefs>) {
    if crate::private::enabled() { return; }
    let _ = crate::store::write_json(&path(), map);
}

/// The host of a URL, lowercased, without a port or "www.".
pub fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .unwrap_or("")
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_lowercase()
}

pub fn prefs(host: &str) -> SitePrefs {
    SITES.read().unwrap().get(host).cloned().unwrap_or_default()
}

pub fn set(host: &str, p: SitePrefs) {
    let mut map = SITES.write().unwrap();
    if p.is_default() {
        map.remove(host);
    } else {
        map.insert(host.to_string(), p);
    }
    save(&map);
}

type PermissionBook = BTreeMap<String, BTreeMap<String, bool>>;
static PERMISSIONS: LazyLock<RwLock<PermissionBook>> = LazyLock::new(|| {
    // Legacy host-only grants in sites.json deliberately do not migrate:
    // there is no safe way to reconstruct their scheme, port or IPv6 host.
    RwLock::new(crate::store::read_json::<PermissionBook>(&permission_path()).value)
});

fn permission_path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile/permissions.json")
}

/// Security origins use a URL parser, never the shortened display hostname.
pub fn permission_origin(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !url.has_host() { return None; }
    Some(url.origin().ascii_serialization())
}

fn save_permissions(book: &PermissionBook) {
    if !crate::private::enabled() { let _ = crate::store::write_json(&permission_path(), book); }
}

pub fn permissions_for(url: &str) -> BTreeMap<String, bool> {
    permission_origin(url).and_then(|origin| PERMISSIONS.read().unwrap().get(&origin).cloned()).unwrap_or_default()
}

/// Remember only this origin's answer. Opaque/invalid origins remain temporary.
pub fn remember(url: &str, what: &str, allow: bool) {
    let Some(origin) = permission_origin(url) else { return };
    let mut book = PERMISSIONS.write().unwrap();
    book.entry(origin).or_default().insert(what.to_string(), allow);
    save_permissions(&book);
}

fn forget_permission(url: &str, what: Option<&str>) {
    let Some(origin) = permission_origin(url) else { return };
    let mut book = PERMISSIONS.write().unwrap();
    if let Some(what) = what {
        if let Some(grants) = book.get_mut(&origin) { grants.remove(what); }
        if book.get(&origin).is_some_and(|grants| grants.is_empty()) { book.remove(&origin); }
    } else { book.remove(&origin); }
    save_permissions(&book);
}

fn permission_answer(book: &PermissionBook, url: &str, what: &str) -> Option<bool> {
    let origin = permission_origin(url)?;
    let grants = book.get(&origin)?;
    let mut all = true;
    for w in what.split(" and ") {
        match grants.get(w.trim()) {
            Some(false) => return Some(false),
            Some(true) => {}
            None => all = false,
        }
    }
    if all { Some(true) } else { None }
}

/// All requested permissions must belong to the exact requesting origin.
pub fn remembered(url: &str, what: &str) -> Option<bool> {
    permission_answer(&PERMISSIONS.read().unwrap(), url, what)
}

#[cfg(test)]
mod permission_tests {
    use super::*;

    fn allowed(url: &str) -> PermissionBook {
        BTreeMap::from([(permission_origin(url).unwrap(), BTreeMap::from([("camera".into(), true)]))])
    }

    #[test]
    fn origin_canonicalization_preserves_security_boundaries() {
        let book = allowed("https://Example.com:443/first");
        assert_eq!(permission_answer(&book, "https://example.com/second?q=1", "camera"), Some(true));
        for other in ["http://example.com", "https://example.com:8443", "https://www.example.com", "https://elsewhere.example"] {
            assert_eq!(permission_answer(&book, other, "camera"), None, "{other}");
        }
        let book = allowed("http://localhost:3000");
        assert_eq!(permission_answer(&book, "http://localhost:4000", "camera"), None);
    }

    #[test]
    fn ipv6_addresses_do_not_collapse_to_a_shared_grant() {
        let book = allowed("https://[2001:db8::1]:443");
        assert_eq!(permission_answer(&book, "https://[2001:0db8:0:0:0:0:0:1]/page", "camera"), Some(true));
        assert_eq!(permission_answer(&book, "https://[2001:db8::2]", "camera"), None);
        assert_eq!(permission_answer(&book, "https://[2001:db8::1]:8443", "camera"), None);
    }

    #[test]
    fn legacy_and_opaque_permissions_require_a_new_decision() {
        let old = BTreeMap::from([("example.com".into(), BTreeMap::from([("camera".into(), true)]))]);
        assert_eq!(permission_answer(&old, "https://example.com", "camera"), None);
        for invalid in ["", "null", "example.com", "file:///tmp/page.html", "data:text/html,test", "about:blank"] {
            assert_eq!(permission_origin(invalid), None);
        }
    }

    #[test]
    fn combined_requests_require_every_grant_and_respect_denials() {
        let mut book = allowed("https://example.com");
        assert_eq!(permission_answer(&book, "https://example.com", "camera and microphone"), None);
        book.get_mut("https://example.com").unwrap().insert("microphone".into(), true);
        assert_eq!(permission_answer(&book, "https://example.com", "camera and microphone"), Some(true));
        book.get_mut("https://example.com").unwrap().insert("microphone".into(), false);
        assert_eq!(permission_answer(&book, "https://example.com", "camera and microphone"), Some(false));
    }
}

/// Chromium's zoom level for a percentage: each level is a 1.2× step.
pub fn zoom_level(pct: u32) -> f64 {
    ((pct.max(25) as f64) / 100.0).ln() / 1.2f64.ln()
}

/// The panel's controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SiteHit {
    ZoomOut,
    ZoomIn,
    ZoomReset,
    Autoplay,
    Js,
    Cookies,
    ClearCookies,
    Boosts,
    Blocking,
    Forget(usize),
    Reset,
}

/// JS that keeps media from starting on its own, and re-checks as the
/// page adds elements.
pub const NO_AUTOPLAY: &str = "(function(){if(window.__nusNoAuto)return;window.__nusNoAuto=1;function q(){document.querySelectorAll('video,audio').forEach(function(m){m.autoplay=false;if(!m.__nusUser){m.pause();}m.addEventListener('play',function(){if(!m.__nusUser){m.pause();}});m.addEventListener('click',function(){m.__nusUser=1;},true);});}q();new MutationObserver(q).observe(document.documentElement,{childList:true,subtree:true});})()";

impl App {
    /// BROWSER · DEFAULT ZOOM changed: every open page with no zoom of its
    /// own takes the new one now.
    pub(crate) fn rezoom_pages(&mut self) {
        let mut n = 0;
        for tab in &self.tabs {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                let Pane::Web(w) = p else { continue };
                let host = host_of(&w.tab.shared.borrow().url);
                if host.is_empty() || SITES.read().unwrap().contains_key(&host) {
                    continue;
                }
                if let Some(h) = w.tab.host() {
                    h.set_zoom_level(zoom_level(default_zoom()));
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.dirty = true;
        }
    }

    /// BROWSER · CLEAR: 0 every cookie, 1 the cache. Cookies go through
    /// the global cookie manager (the containers share it); the cache
    /// through DevTools on any page that is up, since it is one cache.
    pub(crate) fn clear_browsing(&mut self, what: u8) {
        if what == 0 {
            if let Some(cm) = cef::cookie_manager_get_global_manager(None) {
                cm.delete_cookies(None, None, None);
                cm.flush_store(None);
            }
            self.notice("cookies · cleared for every site");
        } else {
            let page = self.tabs.iter().flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref())).find_map(|p| match p {
                Pane::Web(w) => Some(w),
                _ => None,
            });
            match page {
                Some(w) => {
                    w.tab.devtools("Network.clearBrowserCache", serde_json::json!({}));
                    self.notice("the cache · cleared; pages fetch fresh");
                }
                None => self.notice("the cache · open a page first; it is cleared through one"),
            }
        }
        self.dirty = true;
    }

    /// Apply the host's remembered zoom, script and autoplay to a page as
    /// it starts loading (and once it's loaded, for autoplay).
    pub(crate) fn apply_site(&self, w: &WebPane, url: &str, loading: bool) {
        let host = host_of(url);
        if host.is_empty() {
            return;
        }
        let p = prefs(&host);
        if loading {
            if let Some(h) = w.tab.host() {
                h.set_zoom_level(zoom_level(p.zoom));
            }
            w.tab.devtools("Emulation.setScriptExecutionDisabled", serde_json::json!({ "value": !p.js }));
        }
        if !p.autoplay {
            w.tab.eval(NO_AUTOPLAY);
        }
    }

    /// The panel's rect: under the URL row's right end.
    pub(crate) fn site_panel_rect(&self, w: &WebPane) -> Rect {
        let width = self.px(300.0).min(w.rect.w - self.px(16.0));
        let rows = 6.0;
        let perms = permissions_for(&w.tab.shared.borrow().url).len().max(1) as f32;
        let h = self.px(42.0) + rows * self.px(32.0) + self.px(26.0) + self.px(44.0) + perms * self.px(26.0) + self.px(40.0);
        Rect::new(w.rect.right() - width - self.px(8.0), w.page.y + self.px(4.0), width, h.min(w.page.h - self.px(8.0)))
    }

    /// Draw the site panel over the page; the hits go in `w.site_hits`.
    pub(crate) fn draw_site_panel(&mut self, scene: &mut Scene, w: &mut WebPane) {
        w.site_hits.clear();
        if !w.site_panel {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let label = self.label();
        let strong = self.label_strong();
        let (mx, my) = self.mouse;
        let url = w.tab.shared.borrow().url.clone();
        let host = host_of(&url);
        let p = prefs(&host);
        let blocked = w.tab.shared.borrow().blocked;
        let r = self.site_panel_rect(w);
        scene.layer(None);
        scene.rect(Rect::new(r.x + self.px(4.0), r.y + self.px(4.0), r.w, r.h), crate::app::fade(ink, 0.6));
        scene.rect(r, paper);
        scene.outline(r, self.px(m::STRUCTURE), ink);
        scene.layer(Some(r));
        let pad = self.px(14.0);
        let mut y = r.y + self.px(8.0);
        // Head: the host.
        let head = if host.is_empty() { "THIS PAGE".to_string() } else { host.caps() };
        let head = self.fit(strong, &head, r.w - 2.0 * pad);
        self.fonts.draw(scene, Style { color: ink, ..strong }, r.x + pad, y + self.px(16.0), &head);
        y += self.px(26.0);
        scene.hline(r.x, y, r.w, self.px(m::STRUCTURE), ink);
        y += self.px(4.0);
        let row_h = self.px(32.0);
        let dim = Style { color: t.dim, ..label };
        // A row: name on the left, a control on the right.
        let toggle = |me: &mut Self, scene: &mut Scene, y: f32, name: &str, on: bool, note: &str| -> Rect {
            let cell = Rect::new(r.x, y, r.w, row_h);
            if cell.contains(mx, my) {
                scene.rect(cell, t.tint);
            }
            let base = y + (row_h + me.px(m::LABEL_PX)) / 2.0 - me.px(2.0);
            me.fonts.draw(scene, Style { color: ink, ..label }, r.x + pad, base, name);
            let word = if on { "ON" } else { "OFF" };
            let ww = me.fonts.measure(strong, word);
            let sw = me.px(30.0);
            let sx = r.right() - pad - sw;
            // A small switch: a pill with the knob at one end.
            let pill = Rect::new(sx, y + (row_h - me.px(14.0)) / 2.0, sw, me.px(14.0));
            scene.outline(pill, me.px(1.0), ink);
            let knob = me.px(10.0);
            let kx = if on { pill.right() - knob - me.px(2.0) } else { pill.x + me.px(2.0) };
            scene.rect(Rect::new(kx, pill.y + me.px(2.0), knob, knob), if on { me.surface.signal } else { t.dim });
            me.fonts.draw(scene, Style { color: if on { ink } else { t.dim }, ..strong }, sx - me.px(8.0) - ww, base, word);
            if !note.is_empty() {
                let nw = me.fonts.measure(dim, note);
                me.fonts.draw(scene, dim, sx - me.px(8.0) - ww - me.px(8.0) - nw, base, note);
            }
            cell
        };
        // Zoom: − pct +, reset on the number.
        {
            let cell = Rect::new(r.x, y, r.w, row_h);
            let base = y + (row_h + self.px(m::LABEL_PX)) / 2.0 - self.px(2.0);
            self.fonts.draw(scene, Style { color: ink, ..label }, r.x + pad, base, "ZOOM");
            let bw = self.px(26.0);
            let plus = Rect::new(r.right() - pad - bw, y + (row_h - bw) / 2.0, bw, bw);
            let pct = format!("{}%", p.zoom);
            let pw = self.fonts.measure(strong, &pct);
            let num = Rect::new(plus.x - self.px(10.0) - pw - self.px(6.0), y, pw + self.px(12.0), row_h);
            let minus = Rect::new(num.x - self.px(4.0) - bw, plus.y, bw, bw);
            for (b, ic, h) in [(minus, nus_render::text::icons::MINUS, SiteHit::ZoomOut), (plus, nus_render::text::icons::PLUS, SiteHit::ZoomIn)] {
                let hot = b.contains(mx, my);
                scene.outline(b, self.px(1.0), if hot { ink } else { t.dim });
                let isz = self.px(11.0);
                self.fonts.draw_icon(scene, ic, isz, b.x + (bw - isz) / 2.0, b.y + (bw - isz) / 2.0, ink);
                w.site_hits.push((b, h));
            }
            self.fonts.draw(scene, Style { color: if p.zoom == default_zoom() { t.dim } else { ink }, ..strong }, num.x + self.px(6.0), base, &pct);
            w.site_hits.push((num, SiteHit::ZoomReset));
            let _ = cell;
            y += row_h;
        }
        let c = toggle(self, scene, y, "AUTOPLAY", p.autoplay, "");
        w.site_hits.push((c, SiteHit::Autoplay));
        y += row_h;
        let c = toggle(self, scene, y, "JAVASCRIPT", p.js, "");
        w.site_hits.push((c, SiteHit::Js));
        y += row_h;
        let c = toggle(self, scene, y, "COOKIES", p.cookies, "");
        w.site_hits.push((c, SiteHit::Cookies));
        y += row_h;
        let c = toggle(self, scene, y, "BOOSTS", p.boosts, "rules.luau");
        w.site_hits.push((c, SiteHit::Boosts));
        y += row_h;
        let note = if blocked > 0 { format!("{blocked} refused") } else { String::new() };
        let c = toggle(self, scene, y, "BLOCKING", p.blocking, &note);
        w.site_hits.push((c, SiteHit::Blocking));
        y += row_h;
        // Cookies: clear.
        {
            let cell = Rect::new(r.x, y, r.w, self.px(22.0));
            let hot = cell.contains(mx, my);
            let base = y + self.px(15.0);
            self.fonts.draw(scene, Style { color: if hot { ink } else { t.dim }, ..label }, r.x + pad, base, "CLEAR THIS SITE'S COOKIES");
            w.site_hits.push((cell, SiteHit::ClearCookies));
            y += self.px(22.0);
        }
        scene.hline(r.x, y, r.w, self.px(m::STRUCTURE), ink);
        y += self.px(4.0);
        // Permissions given.
        let base0 = y + self.px(17.0);
        self.fonts.draw(scene, dim, r.x + pad, base0, "PERMISSIONS");
        y += self.px(22.0);
        let origin = permission_origin(&url).unwrap_or_else(|| "Temporary permissions".into());
        let origin = self.fit(dim, &origin, r.w - 2.0 * pad);
        self.fonts.draw(scene, dim, r.x + pad, y + self.px(15.0), &origin);
        y += self.px(22.0);
        let permissions = permissions_for(&url);
        if permissions.is_empty() {
            self.fonts.draw(scene, dim, r.x + pad, y + self.px(15.0), "nothing asked yet");
            y += self.px(26.0);
        }
        for (k, (what, allow)) in permissions.iter().enumerate() {
            let cell = Rect::new(r.x, y, r.w, self.px(26.0));
            let hot = cell.contains(mx, my);
            if hot {
                scene.rect(cell, t.tint);
            }
            let base = y + self.px(17.0);
            let text = format!("{}  ·  {}", what.caps(), if *allow { "ALLOWED" } else { "DENIED" });
            self.fonts.draw(scene, Style { color: ink, ..label }, r.x + pad, base, &text);
            let isz = self.px(11.0);
            self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, isz, r.right() - pad - isz, y + (self.px(26.0) - isz) / 2.0, if hot { ink } else { t.dim });
            w.site_hits.push((cell, SiteHit::Forget(k)));
            y += self.px(26.0);
        }
        // Foot: reset.
        let fy = r.bottom() - self.px(32.0);
        scene.hline(r.x, fy, r.w, self.px(m::STRUCTURE), ink);
        let cell = Rect::new(r.x, fy, r.w, self.px(32.0));
        let hot = cell.contains(mx, my);
        self.fonts.draw(scene, Style { color: if hot { ink } else { t.dim }, ..label }, r.x + pad, fy + self.px(20.0), "FORGET THIS SITE");
        w.site_hits.push((cell, SiteHit::Reset));
        scene.layer(None);
    }

    /// A click while the active tab's site panel is up: a control, or
    /// anywhere else (which closes it). Returns true when it was taken.
    pub(crate) fn site_click(&mut self, x: f32, y: f32) -> bool {
        let active = self.active;
        // Which pane has its panel open, and what the click landed on.
        let mut found: Option<(bool, Option<SiteHit>, bool)> = None;
        if let Some(tab) = self.tabs.get(active) {
            for (right, p) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|r| (true, r))) {
                let Pane::Web(w) = p else { continue };
                if !w.site_panel {
                    continue;
                }
                let hit = w.site_hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h);
                let inside = self.site_panel_rect(w).contains(x, y);
                found = Some((right, hit, inside));
            }
        }
        let Some((right, hit, inside)) = found else { return false };
        let Some(tab) = self.tabs.get_mut(active) else { return false };
        let Some(h) = hit else {
            if !inside {
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                if let Some(Pane::Web(w)) = pane {
                    w.site_panel = false;
                }
                self.dirty = true;
                return true;
            }
            return true;
        };
        let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
        let Some(Pane::Web(w)) = pane else { return false };
        let url = w.tab.shared.borrow().url.clone();
        let host = host_of(&url);
        let mut p = prefs(&host);
        let mut reload = false;
        match h {
            SiteHit::ZoomOut => p.zoom = (p.zoom.saturating_sub(10)).max(30),
            SiteHit::ZoomIn => p.zoom = (p.zoom + 10).min(300),
            SiteHit::ZoomReset => p.zoom = default_zoom(),
            SiteHit::Autoplay => p.autoplay = !p.autoplay,
            SiteHit::Js => {
                p.js = !p.js;
                reload = true;
            }
            SiteHit::Cookies => {
                p.cookies = !p.cookies;
                reload = true;
            }
            SiteHit::ClearCookies => {
                if let Some(cm) = cef::cookie_manager_get_global_manager(None) {
                    let u: cef::CefString = url.as_str().into();
                    cm.delete_cookies(Some(&u), None, None);
                }
                reload = true;
            }
            SiteHit::Boosts => {
                p.boosts = !p.boosts;
                reload = true;
            }
            SiteHit::Blocking => {
                p.blocking = !p.blocking;
                reload = true;
            }
            SiteHit::Forget(k) => {
                if let Some(key) = permissions_for(&url).keys().nth(k) {
                    forget_permission(&url, Some(key));
                }
            }
            SiteHit::Reset => {
                forget_permission(&url, None);
                p = SitePrefs::default();
                reload = true;
            }
        }
        set(&host, p.clone());
        if let Some(h) = w.tab.host() {
            h.set_zoom_level(zoom_level(p.zoom));
        }
        w.tab.devtools("Emulation.setScriptExecutionDisabled", serde_json::json!({ "value": !p.js }));
        if !p.autoplay {
            w.tab.eval(NO_AUTOPLAY);
        }
        if reload {
            w.boosted.clear();
            w.tab.reload();
        }
        self.play_event("toggle");
        self.dirty = true;
        true
    }

}
