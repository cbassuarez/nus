//! CEF glue: windowless browser whose accelerated paints land in a wgpu
//! texture the compositor binds as a layer. Input is forwarded from winit.

use std::cell::RefCell;
use std::rc::Rc as StdRc;
use std::sync::Arc;

use cef::rc::Rc;
use cef::*;

/// What the app reads each frame.
#[derive(Default)]
pub struct Shared {
    /// Download provenance (Finish Work, finish_work.rs). The user's own
    /// input into this page, as nus forwarded it: a page cannot make these.
    pub gesture_at: Option<std::time::Instant>,
    /// The last main-frame navigation Chromium marked as a user gesture.
    pub nav_gesture_at: Option<std::time::Instant>,
    /// nus itself started a download for the user (Save image, and so on).
    pub nus_download_at: Option<std::time::Instant>,
    pub sleep_safe: bool,
    pub suspended: bool,
    pub restore_scroll: Option<(f64,f64)>,
    pub capture_guard: bool,
    pub scroll_position: (f64,f64),
    pub viewer: crate::file_viewer::Shared,
    pub edit_source: Option<std::path::PathBuf>,
    pub save_reading: bool,
    /// Latest imported paint, bound for the quad pipeline.
    pub bind: Option<Arc<wgpu::BindGroup>>,
    pub paint_size: (u32, u32),
    pub title: String,
    pub url: String,
    pub loading: bool,
    pub requested_url: String,
    pub(crate) failed_url: Option<String>,
    /// 0..1 from on_loading_progress_change.
    pub progress: f64,
    /// Bumped on every accelerated paint; the app redraws when it changes.
    pub paints: u64,
    /// The page's dominant <video>, reported by the injected tracker.
    pub video: Option<Video>,
    pub next_msg: i32,
    /// CDP target id of this page (for the DevTools frontend URL).
    pub target_id: Option<String>,
    pub target_msg: i32,
    /// Logical size CEF should render at; app sets it, view_rect reads it.
    pub size: (f32, f32),
    pub scale: f32,
    /// Page zoom is re-rasterized by Chromium at each intermediate size.
    /// Never animate the previously painted page texture.
    pub zoom_motion: Option<crate::anim::Anim>,
    /// Where the browser pane sits in the window (physical px), for
    /// screen_point → popup placement.
    pub origin: (f32, f32),
    pub window_pos: (i32, i32),
    /// A page asked for a new window (target=_blank, window.open); the app
    /// opens it as a tab in this tab's stack.
    pub popup: Option<String>,
    /// The link under the pointer (Chromium's status message), for peeks.
    pub hover_url: String,
    pub created: Created,
    /// Results of CDP calls made with `devtools`, by message id; the app
    /// drains the ones it asked for.
    pub replies: Vec<(i32, serde_json::Value)>,
    /// Every <video> and <audio> on the page with a source, as the report
    /// last saw them (webui's strip icon, the page menu).
    pub media: Vec<Media>,
    /// A right-click's menu, waiting for the app to draw it (page_menu.rs).
    pub menu: Option<MenuRequest>,
    /// Pages the menu asked to open: (url, beside).
    pub opens: Vec<(String, bool)>,
    /// A word for a toast the menu earned ("copied · …").
    pub said: Option<(String, String)>,
    /// What the page said and fetched: console calls, exceptions, requests
    /// and responses, as `{ "kind", "at", … }`, newest last, capped.
    pub log: Vec<serde_json::Value>,
    /// The page's favicon, straight-alpha BGRA, once downloaded.
    pub favicon: Option<Favicon>,
    pub favicon_url: String,
    /// Find in page: (matches, active ordinal), from the find handler.
    pub find: Option<(i32, i32)>,
    /// A permission the page asked for, waiting on the band.
    pub permission: Option<PermissionAsk>,
    /// A <select> (or other popup widget): where it is and its texture.
    pub select: SelectPopup,
    /// The page nus shows in place of this site (interstitial.rs): a load
    /// error, a crash, a dangerous site, a `nus://` page.
    pub interstitial: Option<crate::interstitial::Page>,
    /// Write `interstitial` over the next main-frame document that loads.
    pub(crate) inject: bool,
    /// The renderer is gone or the site was refused: the app loads a blank
    /// document to write the page over.
    pub(crate) blank: bool,
    /// Commands the page's transcript sent back, for the app to run.
    pub interstitial_acts: Vec<String>,
    /// Drawn by nus over the live page: hung, waking, mic/camera, file.
    pub overlay: Option<crate::interstitial::Page>,
    /// The renderer stopped answering; wait or stop through this.
    pub hung: Option<UnresponsiveProcessCallback>,
    /// The watch on a shown page: a trivial question it hasn't answered
    /// yet (message id, asked at), and when it last answered one.
    pub(crate) ping: Option<(i32, std::time::Instant)>,
    pub(crate) answered: Option<std::time::Instant>,
    /// You chose to stop the hung page: its crash reads as that.
    pub(crate) stopping: bool,
    /// A `nus://crash`-style command, run once the blank page is there.
    pub(crate) debug: Option<crate::interstitial::Internal>,
    /// Asking the network whether it wants a sign-in (a captive portal).
    pub(crate) portal: Option<Arc<std::sync::Mutex<Option<Option<String>>>>>,
    /// The page's own question (alert, confirm, prompt, leave-page) or a
    /// site's sign-in, waiting on the overlay's answer.
    pub(crate) dialog: Option<Dialog>,
    /// An address for another app (mailto:, zoommtg:, …) the page tried to
    /// open: not a page nus can show, so the app offers to hand it on.
    pub external: Option<String>,
    /// Makes the page for a window this page opens (`window.open`,
    /// `target=_blank`): its own client, so Chromium can keep the two
    /// joined (`window.opener`, `postMessage`, the popup closing itself).
    pub(crate) popup_factory: Option<StdRc<dyn Fn() -> (Client, SharedRef)>>,
    /// Windows this page opened, waiting for a tab of their own.
    pub(crate) opened: Vec<SharedRef>,
    /// This page's browser, when Chromium made it (a popup) rather than nus.
    pub(crate) adopted: Option<Browser>,
    /// Chromium closed this page on its own account: the page closed
    /// itself, or you chose to leave it. The app takes its pane away.
    pub gone: bool,
    /// nus is closing or sleeping this page itself; its close is expected.
    pub(crate) letting_go: bool,
    /// The page has shown a document of its own (not only a download).
    pub(crate) committed: bool,
    /// Made by Chromium for a popup, waiting for its browser.
    pub(crate) created_by_chromium: bool,
    /// The page called `window.print()`: there is no print dialog for a
    /// page drawn offscreen, so it is saved as a PDF instead.
    pub(crate) print_asked: bool,
    pub(crate) print_msg: Option<i32>,
    /// Where that PDF went, or why it didn't.
    pub print_saved: Option<Result<std::path::PathBuf, String>>,
    /// The page asked to go fullscreen (true) or to come back (false).
    pub page_fullscreen: Option<bool>,
    /// Its only navigation became a download, now finished: the tab has
    /// nothing to show.
    pub download_only: bool,
    /// …and that download is still going.
    pub(crate) download_waiting: bool,
}

/// What a `Kind::Dialog` overlay answers.
pub(crate) enum Dialog {
    Js(JsdialogCallback),
    /// A site answered 401: the sign-in it wants, for this origin.
    Basic(String),
}

/// Sign-ins you gave, by origin (`https://host:port`), as the header sent
/// with every request to that origin and no other. Read on Chromium's IO
/// thread, so it lives here and not in `Shared`. Memory only: a sign-in
/// lasts as long as nus runs, as the browser's own would.
static SIGNINS: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> = std::sync::Mutex::new(None);

pub fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    matches!(u.scheme(), "http" | "https").then(|| u.origin().ascii_serialization())
}

fn signin_for(url: &str) -> Option<String> {
    let origin = origin_of(url)?;
    SIGNINS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&origin).cloned()
}

fn remember_signin(origin: String, header: Option<String>) {
    let mut map = SIGNINS.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(Default::default);
    match header {
        Some(h) => { map.insert(origin, h); }
        None => { map.remove(&origin); }
    }
}

/// `user:pass` as HTTP Basic sends it.
fn basic(user: &str, pass: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = format!("{user}:{pass}").into_bytes();
    let mut out = String::from("Basic ");
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() { out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char); } else { out.push('='); }
        }
    }
    out
}

/// Schemes a tab shows itself; any other address belongs to another app.
pub fn web_scheme(url: &str) -> bool {
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https" | "file" | "data" | "blob" | "about" | "chrome" | "chrome-error" | "devtools" | "javascript" | "filesystem" | "nus" | "chrome-extension")
}

impl Shared {
    fn address(&mut self, url: &str) {
        // Chrome's error document is an implementation detail, not the destination.
        if url.starts_with("chrome-error:") || self.failed_url.as_deref().is_some_and(|failed|failed!=url) {return;}
        // The blank document an interstitial is written over.
        if url == "about:blank" && (self.interstitial.is_some() || self.overlay.is_some()) {return;}
        if crate::sites::host_of(url)!=crate::sites::host_of(&self.url) {self.favicon=None;self.favicon_url.clear();}
        if self.url!=url {self.zoom_motion=None;}
        self.url=url.into();self.paints+=1;
    }
    pub fn redirect(&mut self,from:&str,to:&str){
        if self.url==from {
            if self.failed_url.is_some(){self.failed_url=Some(to.into());}
            self.address(to);
        }
    }
    fn failed(&mut self, url: &str, authoritative: bool) {
        // A late CEF error/display callback can name the pre-redirect URL.
        // CDP unreachableUrl is authoritative for the committed error document.
        let destination = if !authoritative && url==self.requested_url && self.url!=self.requested_url {self.url.clone()} else {url.into()};
        self.failed_url=Some(destination.clone());
        self.address(&destination);
        self.loading=false;self.progress=1.0;
    }
    fn navigation(&mut self,url:&str) {
        if url == "about:blank" && (self.interstitial.is_some() || self.overlay.is_some()) {return;}
        // Anywhere else: the interstitial is over.
        self.interstitial=None;self.inject=false;
        if self.overlay.as_ref().is_some_and(|o|o.kind!=crate::interstitial::Kind::Sleep) {self.overlay=None;}
        self.failed_url=None;
        self.address(url);self.requested_url=url.into();self.loading=true;self.progress=0.0;
    }
}

wrap_load_handler! {
    pub struct LoadBuilder { shared: SharedRef }
    impl LoadHandler {
        fn on_loading_state_change(&self,_browser:Option<&mut Browser>,is_loading: ::std::os::raw::c_int,_back: ::std::os::raw::c_int,_forward: ::std::os::raw::c_int) {
            let mut s=self.shared.borrow_mut();s.loading=is_loading!=0;
            if !s.loading {s.progress=1.0;}
            s.paints+=1;
        }
        fn on_load_error(&self,browser:Option<&mut Browser>,frame:Option<&mut Frame>,error_code:Errorcode,_error_text:Option<&CefString>,failed_url:Option<&CefString>) {
            // Aborted navigations include downloads and requests superseded by another URL.
            if cef::sys::cef_errorcode_t::from(error_code)==cef::sys::cef_errorcode_t::ERR_ABORTED || !frame.is_some_and(|f|f.is_main()!=0) {return;}
            let Some(url)=failed_url else {return};
            let code=cef::sys::cef_errorcode_t::from(error_code) as i32;
            let can_back=browser.is_some_and(|b|b.can_go_back()!=0);
            let mut s=self.shared.borrow_mut();
            s.failed(&url.to_string(),false);
            // In place of Chromium's error document: nus's transcript.
            let dest=s.failed_url.clone().unwrap_or_else(||url.to_string());
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs() as i64).unwrap_or(0);
            // The site wants a sign-in (a 401 Chromium had no answer to):
            // ask for it, over the page, instead of calling it unreachable.
            if code==-338 && s.dialog.is_none() {
                if let Some(origin)=origin_of(&dest) {
                    let rejected=signin_for(&dest).is_some();
                    remember_signin(origin.clone(),None);
                    let host=crate::interstitial::host(&dest);
                    let mut p=crate::interstitial::Page::signin(&dest,&host,"",false);
                    if rejected {p.log.insert(1,"  that username and password weren't accepted".into());}
                    s.overlay=Some(p);s.dialog=Some(Dialog::Basic(origin));s.paints+=1;
                    return;
                }
            }
            s.interstitial=Some(crate::interstitial::for_error(&dest,code,can_back,now,crate::interstitial::built()));
            s.inject=true;
            if crate::interstitial::portal_suspect(code) {
                let slot=Arc::new(std::sync::Mutex::new(None));
                s.portal=Some(slot.clone());
                std::thread::spawn(move||{
                    let found=crate::interstitial::probe_portal();
                    *slot.lock().unwrap_or_else(|e|e.into_inner())=Some(found);
                    crate::browser_runtime::wake();
                });
            }
        }
        fn on_load_start(&self,_browser:Option<&mut Browser>,frame:Option<&mut Frame>,_transition_type:TransitionType) {
            // A document of its own: this tab is more than a download.
            if frame.is_some_and(|f|f.is_main()!=0) { self.shared.borrow_mut().committed=true; }
        }
        fn on_load_end(&self,_browser:Option<&mut Browser>,frame:Option<&mut Frame>,_status: ::std::os::raw::c_int) {
            let Some(frame)=frame.filter(|f|f.is_main()!=0) else {return};
            let mut s=self.shared.borrow_mut();
            if let Some(debug)=s.debug.take() {
                drop(s);
                run_debug(frame,&debug);
                return;
            }
            let url=CefString::from(&frame.url()).to_string();
            // CEF names an error document by the address that failed.
            let ours=url.starts_with("chrome-error:") || url=="about:blank" || s.failed_url.as_deref()==Some(url.as_str()) || s.interstitial.as_ref().is_some_and(|p|p.url==url);
            if !s.inject || !ours {return;}
            if let Some(page)=s.interstitial.as_ref() {
                let script=page.script();
                s.inject=false;s.paints+=1;
                drop(s);
                frame.execute_java_script(Some(&script.as_str().into()),Some(&url.as_str().into()),0);
            }
        }
    }
}

/// A permission prompt or a media-access request, one at a time.
pub struct PermissionAsk {
    pub origin: String,
    /// What was asked, in words.
    pub what: String,
    pub kind: AskKind,
}

pub enum AskKind {
    Prompt(PermissionPromptCallback),
    Media(MediaAccessCallback, u32),
}

#[derive(Default)]
pub struct SelectPopup {
    pub shown: bool,
    /// Logical px, relative to the view.
    pub rect: (i32, i32, i32, i32),
    pub bind: Option<Arc<wgpu::BindGroup>>,
}

/// A download, as the footer shows it. Downloads outlive tabs, so they
/// live in one list for the process.
pub use crate::downloads::Download;

pub static DOWNLOADS: std::sync::Mutex<Vec<Download>> = std::sync::Mutex::new(Vec::new());

/// Content blocking: on/off and the hosts to refuse. Built-in list plus
/// profile/blocklist.txt (one host per line; `||host^` lines work too).
pub static BLOCKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Chromium's smooth scrolling, read once at start from the prefs.
pub static SMOOTH_SCROLL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// BROWSER · SCROLLBARS, read once at start: 0 overlay, 1 classic, 2 hidden.
pub static SCROLLBARS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// BROWSER · PRIVACY SIGNAL: Sec-GPC and DNT on every request.
pub static PRIVACY_SIGNAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Requests refused by content blocking, per browser identifier. Counted
/// on Chromium's IO thread, so it lives here rather than in `Shared`,
/// which belongs to the UI thread.
static BLOCKED: std::sync::Mutex<Option<std::collections::HashMap<i32, u32>>> = std::sync::Mutex::new(None);
static BLOCKLIST: std::sync::OnceLock<std::sync::RwLock<std::collections::HashSet<String>>> = std::sync::OnceLock::new();

const BUILTIN_BLOCKLIST: &[&str] = &[
    "doubleclick.net", "googlesyndication.com", "googleadservices.com", "google-analytics.com", "googletagmanager.com", "googletagservices.com",
    "adservice.google.com", "pagead2.googlesyndication.com", "facebook.net", "connect.facebook.net", "ads.linkedin.com", "px.ads.linkedin.com",
    "adnxs.com", "adsrvr.org", "taboola.com", "outbrain.com", "criteo.com", "criteo.net", "scorecardresearch.com", "quantserve.com",
    "hotjar.com", "mouseflow.com", "fullstory.com", "clarity.ms", "bat.bing.com", "amazon-adsystem.com", "moatads.com", "pubmatic.com",
    "rubiconproject.com", "openx.net", "casalemedia.com", "bidswitch.net", "chartbeat.com", "newrelic.com", "nr-data.net", "segment.io",
    "mixpanel.com", "optimizely.com", "sentry.io", "bugsnag.com", "yieldmo.com", "sharethrough.com", "media.net", "zedo.com", "adform.net",
];

fn blocklist() -> &'static std::sync::RwLock<std::collections::HashSet<String>> {
    BLOCKLIST.get_or_init(|| {
        let mut set: std::collections::HashSet<String> = BUILTIN_BLOCKLIST.iter().map(|s| s.to_string()).collect();
        let path = std::env::current_dir().unwrap_or_default().join("profile").join("blocklist.txt");
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let l = line.trim();
                if l.is_empty() || l.starts_with('!') || l.starts_with('#') {
                    continue;
                }
                let host = l.trim_start_matches("||").split(['^', '/', '$']).next().unwrap_or("").trim();
                if !host.is_empty() && !host.contains('*') {
                    set.insert(host.to_lowercase());
                }
            }
        }
        std::sync::RwLock::new(set)
    })
}

/// Would this request be refused on `page`? Its host, or any parent
/// domain, on the list counts — but only from someone else's page. The
/// list names trackers by their domain; on that domain's own site the
/// same host serves the page's styles, scripts and images, and refusing
/// them leaves a blank or unstyled page.
pub fn blocked(url: &str, page: &str) -> bool {
    if !BLOCKING.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    refused(&blocklist().read().unwrap(), &host_of_url(url), &host_of_url(page))
}

fn host_of_url(url: &str) -> String {
    url.split("//").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("").split('@').next_back().unwrap_or("").split(':').next().unwrap_or("").to_lowercase()
}

/// The list entry `host` falls under, if any.
fn listed<'a>(list: &std::collections::HashSet<String>, host: &'a str) -> Option<&'a str> {
    let mut h = host;
    loop {
        if h.is_empty() {
            return None;
        }
        if list.contains(h) {
            return Some(h);
        }
        h = &h[h.find('.')? + 1..];
    }
}

fn refused(list: &std::collections::HashSet<String>, host: &str, page: &str) -> bool {
    let Some(entry) = listed(list, host) else { return false };
    // First party: the page itself is under the same entry.
    !(page == entry || page.ends_with(&format!(".{entry}")))
}

#[cfg(test)]
mod print_tests {
    #[test]
    fn devtools_base64_decodes() {
        assert_eq!(super::decode64("JVBERi0xLjQ="), b"%PDF-1.4");
        assert_eq!(super::decode64("YTpi"), b"a:b");
    }
}

#[cfg(test)]
mod signin_tests {
    use super::{basic, origin_of};

    #[test]
    fn a_sign_in_is_basic_and_bound_to_its_origin() {
        // RFC 7617's own example.
        assert_eq!(basic("Aladdin", "open sesame"), "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
        assert_eq!(basic("a", "b"), "Basic YTpi");
        assert_eq!(origin_of("http://127.0.0.1:47920/auth?x=1"), Some("http://127.0.0.1:47920".into()));
        assert_eq!(origin_of("https://intranet.example/a"), Some("https://intranet.example".into()));
        // Another port, another scheme: another origin, no header.
        assert_ne!(origin_of("http://127.0.0.1:47921/"), origin_of("http://127.0.0.1:47920/"));
        assert_ne!(origin_of("https://intranet.example/"), origin_of("http://intranet.example/"));
        assert_eq!(origin_of("mailto:a@b.c"), None);
    }
}

#[cfg(test)]
mod blocking_tests {
    use super::refused;

    #[test]
    fn a_listed_domain_is_refused_only_on_other_sites() {
        let list: std::collections::HashSet<String> = ["newrelic.com", "doubleclick.net", "sentry.io"].iter().map(|s| s.to_string()).collect();
        // Someone else's page pulling in the tracker: refused.
        assert!(refused(&list, "js-agent.newrelic.com", "www.nytimes.com"));
        assert!(refused(&list, "ad.doubleclick.net", "news.example"));
        assert!(refused(&list, "o123.ingest.sentry.io", "github.com"));
        // The listed company's own site: its own hosts load.
        assert!(!refused(&list, "newrelic.com", "newrelic.com"));
        assert!(!refused(&list, "static.newrelic.com", "www.newrelic.com"));
        assert!(!refused(&list, "sentry.io", "sentry.io"));
        // But not someone else's tracker on it.
        assert!(refused(&list, "ad.doubleclick.net", "newrelic.com"));
        // Unlisted, and look-alike names that only end the same way.
        assert!(!refused(&list, "cdn.example.com", "www.nytimes.com"));
        assert!(refused(&list, "newrelic.com", "notnewrelic.com"));
        assert!(!refused(&list, "notnewrelic.com", "a.com"));
    }
}

pub fn blocklist_len() -> usize {
    blocklist().read().unwrap().len()
}

/// Where downloads go: ~/Downloads, else the profile.
/// BROWSER · DOWNLOADS · LOCATION, when one was chosen; else the system's.
static DOWNLOAD_DIR: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);
/// BROWSER · DOWNLOADS · ASK WHERE TO SAVE.
pub static DOWNLOAD_ASK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The folder downloads go to: the chosen one when it is a folder that
/// exists (or can be made), else ~/Downloads, else profile/downloads.
pub fn downloads_dir() -> std::path::PathBuf {
    if std::env::var_os("NUS_SHOT").is_some() {
        if crate::private::enabled() {
            if let Some(dir) = std::env::var_os("NUS_SHOT_DIR") { return std::path::PathBuf::from(dir).join("downloads"); }
        }
        return std::env::current_dir().unwrap_or_default().join("profile/downloads");
    }
    if crate::private::enabled() {
        if let Some(dir) = crate::private::downloads_dir() { return dir.to_path_buf(); }
    }
    if let Some(dir) = DOWNLOAD_DIR.read().ok().and_then(|d| d.clone()) {
        if dir.is_dir() || std::fs::create_dir_all(&dir).is_ok() {
            return dir;
        }
    }
    default_downloads_dir()
}

/// Where downloads go when nothing was chosen.
pub fn default_downloads_dir() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).map(std::path::PathBuf::from);
    match home {
        Ok(h) if crate::private::enabled() || h.join("Downloads").is_dir() => h.join("Downloads"),
        _ => std::env::current_dir().unwrap_or_default().join("profile").join("downloads"),
    }
}

/// The setting, applied: empty means the default.
pub fn set_downloads_dir(dir: &str) {
    let dir = dir.trim();
    if let Ok(mut d) = DOWNLOAD_DIR.write() {
        *d = if dir.is_empty() { None } else { Some(crate::links::expand_home(dir)) };
    }
}

/// A download waiting on the save dialog: where it would go, and the
/// callback that starts it once a place is chosen (downloads.rs asks).
pub struct SaveAsk {
    pub key: u64,
    pub suggested: std::path::PathBuf,
    pub callback: BeforeDownloadCallback,
}

thread_local! {
    /// Downloads that want a save dialog, for the app's tick (they arrive
    /// on the UI thread, which is the app's).
    pub static SAVE_ASKS: std::cell::RefCell<Vec<SaveAsk>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Favicon {
    pub url: String,
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

/// Instant with a Default, so Shared can derive it.
pub struct Created(pub std::time::Instant);
impl Default for Created {
    fn default() -> Self {
        Created(crate::clock::now())
    }
}
impl Created {
    pub fn elapsed(&self) -> std::time::Duration {
        self.0.elapsed()
    }
}

/// A media element on the page. `blob` sources (MediaSource players)
/// cannot be saved: there is no file behind them.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub kind: String,
    pub src: String,
    pub w: u32,
    pub h: u32,
    pub blob: bool,
}

/// What the page's right-click menu should hold, from the click's context.
pub struct MenuRequest {
    /// View coordinates (CSS px).
    pub x: f32,
    pub y: f32,
    /// (command id, label, enabled); an empty label is a separator.
    pub items: Vec<(i32, String, bool)>,
    pub callback: RunContextMenuCallback,
}

// The menu's own commands; CEF's builtins (back, copy, …) keep their ids.
pub const CMD_SAVE_MEDIA: i32 = 26500 + 1;
pub const CMD_COPY_MEDIA: i32 = 26500 + 2;
pub const CMD_OPEN_MEDIA: i32 = 26500 + 3;
pub const CMD_OPEN_LINK: i32 = 26500 + 4;
pub const CMD_OPEN_LINK_BESIDE: i32 = 26500 + 5;
pub const CMD_COPY_LINK: i32 = 26500 + 6;
pub const CMD_COPY_PAGE: i32 = 26500 + 7;
pub const CMD_PIP: i32 = 26500 + 8;
pub const CMD_NOTHING: i32 = 26500 + 9;

/// A video's state in CSS px relative to the viewport.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Video {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub vw: f32,
    pub vh: f32,
    /// Intrinsic stream dimensions, independent of the page's CSS layout.
    pub video_width: f64,
    pub video_height: f64,
    /// Visible fraction of the intrinsic picture (CSS can crop the source).
    pub picture: [f32; 4],
    pub paused: bool,
    pub ended: bool,
    pub muted: bool,
    pub t: f64,
    pub dur: f64,
}

fn debug_port(value: Option<&str>, private: bool) -> Option<u16> {
    if private { return None; }
    value.and_then(|s| s.parse::<u16>().ok()).filter(|p| *p >= 1024)
}
fn external_debug_port() -> Option<u16> {
    debug_port(std::env::var("NUS_REMOTE_DEBUGGING_PORT").ok().as_deref(), crate::private::enabled())
}

#[cfg(test)]
mod debugging_tests {
    #[test] fn external_debugging_requires_a_valid_explicit_port_and_is_never_private() {
        for value in [None, Some(""), Some("0"), Some("80"), Some("65536"), Some("localhost:9229")] {assert_eq!(super::debug_port(value, false),None);}
        assert_eq!(super::debug_port(Some("9229"), false),Some(9229));
        assert_eq!(super::debug_port(Some("9229"), true),None);
    }
}

wrap_life_span_handler! {
    struct NativeDevToolsLife { _unit: () }
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser { LIVE_BROWSERS.with(|v| v.borrow_mut().insert(b.identifier())); }
        }
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser { LIVE_BROWSERS.with(|v| v.borrow_mut().remove(&b.identifier())); }
        }
    }
}
wrap_client! {
    struct NativeDevToolsClient { life: LifeSpanHandler }
    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.life.clone()) }
    }
}

pub type SharedRef = StdRc<RefCell<Shared>>;

/// Injected into every document: `window.print()` asks nus, which saves
/// the page as a PDF (a page drawn offscreen has no print dialog).
const PRINT_JS: &str = "(()=>{try{const p=function(){try{nusPrint('')}catch(e){}};Object.defineProperty(window,'print',{value:p,writable:true,configurable:true})}catch(e){}})()";

/// Base64 as DevTools sends binary.
fn decode64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c { b'A'..=b'Z' => Some(c - b'A'), b'a'..=b'z' => Some(c - b'a' + 26), b'0'..=b'9' => Some(c - b'0' + 52), b'+' => Some(62), b'/' => Some(63), _ => None };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for v in s.bytes().filter_map(val) {
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// `<title>.pdf` in the downloads folder, never over a file already there.
fn save_pdf(title: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
    if !bytes.starts_with(b"%PDF") {
        return Err("the PDF came back empty".into());
    }
    let dir = downloads_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { ' ' } else { c }).collect::<String>().trim().chars().take(80).collect();
    let stem = if stem.is_empty() { "page".to_string() } else { stem };
    let mut path = dir.join(format!("{stem}.pdf"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{stem} ({n}).pdf"));
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Injected into every document: tracks the largest playing <video>, reports
/// it through the `nusVideo` binding, and exposes transport on `__nus`.
pub const VIDEO_JS: &str = include_str!("../assets/video.js");

#[derive(Clone)]
pub struct AppHandler;

wrap_app! {
    pub struct AppBuilder {
        handler: AppHandler,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefStringUtf16>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(cl) = command_line else { return };
            for flag in ["no-sandbox","disable-gpu-sandbox","disable-seccomp-filter-sandbox","disable-namespace-sandbox","single-process","in-process-gpu","disable-site-isolation-trials"] {cl.remove_switch(Some(&flag.into()));}
            cl.append_switch(Some(&"site-per-process".into()));
            cl.append_switch(Some(&"no-startup-window".into()));
            // Disposable HTTP cache; cookies and site storage stay.
            cl.append_switch_with_value(Some(&"disk-cache-size".into()), Some(&"67108864".into()));
            cl.append_switch(Some(&"noerrdialogs".into()));
            cl.append_switch(Some(&"hide-crash-restore-bubble".into()));
            // Production uses the OS keychain. Only explicitly isolated
            // screenshot fixtures may bypass it to avoid credential dialogs.
            cl.remove_switch(Some(&"use-mock-keychain".into()));
            if cfg!(target_os="macos") && std::env::var_os("NUS_SHOT").is_some()
                && std::env::var_os("NUS_SHOT_DIR").is_some() && std::env::var_os("NUS_TEST_REAL_KEYCHAIN").is_none() {
                cl.append_switch(Some(&"use-mock-keychain".into()));
            }
            if !crate::browser::SMOOTH_SCROLL.load(std::sync::atomic::Ordering::Relaxed) {
                cl.append_switch(Some(&"disable-smooth-scrolling".into()));
            }
            // BROWSER · SCROLLBARS. Overlay is a Chromium feature on Windows
            // and Linux (macOS follows the system); the feature list is
            // merged, never replaced, in case CEF put its own there first.
            match crate::browser::SCROLLBARS.load(std::sync::atomic::Ordering::Relaxed) {
                0 => {
                    let had = if cl.has_switch(Some(&"enable-features".into())) != 0 { CefString::from(&cl.switch_value(Some(&"enable-features".into()))).to_string() } else { String::new() };
                    let all = if had.is_empty() { "OverlayScrollbar".to_string() } else { format!("{had},OverlayScrollbar") };
                    cl.append_switch_with_value(Some(&"enable-features".into()), Some(&all.as_str().into()));
                }
                2 => cl.append_switch(Some(&"hide-scrollbars".into())),
                _ => {}
            }
            if std::env::var_os("NUS_AUTOPLAY").is_some() {
                cl.append_switch_with_value(Some(&"autoplay-policy".into()), Some(&"no-user-gesture-required".into()));
            }
            // Native DevTools uses CEF's in-process connection. Exposing CDP
            // over TCP is a deliberate developer opt-in, never incognito.
            cl.remove_switch(Some(&"remote-debugging-port".into()));
            cl.remove_switch(Some(&"remote-debugging-pipe".into()));
            cl.remove_switch(Some(&"remote-allow-origins".into()));
            if let Some(port) = external_debug_port() {
                cl.append_switch_with_value(Some(&"remote-debugging-address".into()), Some(&"127.0.0.1".into()));
                cl.append_switch_with_value(Some(&"remote-debugging-port".into()), Some(&port.to_string().as_str().into()));
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BpHandler::new(()))
        }
    }
}

wrap_browser_process_handler! {
    pub struct BpHandler {
        _unit: (),
    }

    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) { crate::browser_runtime::schedule(delay_ms); }
        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            let Some(cl) = command_line else { return };
            cl.append_switch(Some(&"disable-session-crashed-bubble".into()));
        }
    }
}

#[derive(Clone)]
pub struct Osr {
    pub shared: SharedRef,
    pub device: wgpu::Device,
    pub bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>,
}

wrap_render_handler! {
    pub struct RenderBuilder {
        osr: Osr,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let s = self.osr.shared.borrow();
                rect.x = 0;
                rect.y = 0;
                rect.width = (s.size.0.max(1.0)) as i32;
                rect.height = (s.size.1.max(1.0)) as i32;
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> ::std::os::raw::c_int {
            let Some(info) = info else { return 0 };
            let s = self.osr.shared.borrow();
            info.device_scale_factor = s.scale;
            info.depth = 24;
            info.depth_per_component = 8;
            let r = Rect { x: 0, y: 0, width: s.size.0 as i32, height: s.size.1 as i32 };
            info.rect = Rect { ..r };
            info.available_rect = r;
            1
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            view_x: ::std::os::raw::c_int,
            view_y: ::std::os::raw::c_int,
            screen_x: Option<&mut ::std::os::raw::c_int>,
            screen_y: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let s = self.osr.shared.borrow();
            let scale = s.scale;
            if let Some(x) = screen_x {
                *x = s.window_pos.0 + (s.origin.0 / scale) as i32 + view_x;
            }
            if let Some(y) = screen_y {
                *y = s.window_pos.1 + (s.origin.1 / scale) as i32 + view_y;
            }
            1
        }

        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: ::std::os::raw::c_int) {
            let mut s = self.osr.shared.borrow_mut();
            s.select.shown = show != 0;
            if show == 0 {
                s.select.bind = None;
            }
            s.paints += 1;
        }

        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            if let Some(r) = rect {
                let mut s = self.osr.shared.borrow_mut();
                s.select.rect = (r.x, r.y, r.width, r.height);
            }
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            if self.osr.shared.borrow().suspended { return; }
            let Some(info) = info else { return };
            let popup = type_ != PaintElementType::default();
            use cef::osr_texture_import::shared_texture_handle::SharedTextureHandle;
            let handle = SharedTextureHandle::new(info);
            match handle.import_texture(&self.osr.device) {
                Ok(texture) => {
                    let bind = (self.osr.bind_texture)(&texture);
                    let mut s = self.osr.shared.borrow_mut();
                    if popup {
                        // A <select> dropdown or similar widget, composited over the page.
                        s.select.bind = Some(bind);
                    } else {
                        s.bind = Some(bind);
                        s.paint_size = (texture.width(), texture.height());
                        if s.paints == 0 {
                            tracing::info!("first paint +{}ms", s.created.elapsed().as_millis());
                        }
                    }
                    s.paints += 1;
                }
                Err(e) => tracing::warn!("texture import: {e:?}"),
            }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            _type_: PaintElementType,
            _dirty: Option<&[Rect]>,
            _buffer: *const u8,
            _w: ::std::os::raw::c_int,
            _h: ::std::os::raw::c_int,
        ) {
            tracing::warn!("software paint path hit; accelerated OSR unavailable");
        }
    }
}

#[derive(Clone)]
pub struct Display {
    pub shared: SharedRef,
}

wrap_display_handler! {
    pub struct DisplayBuilder {
        d: Display,
    }

    impl DisplayHandler {
        /// The page went fullscreen (a video's button, the Fullscreen API)
        /// or came back: the app gives it the whole screen, or takes it back.
        fn on_fullscreen_mode_change(&self, _browser: Option<&mut Browser>, fullscreen: ::std::os::raw::c_int) {
            let mut s = self.d.shared.borrow_mut();
            s.page_fullscreen = Some(fullscreen != 0);
            s.paints += 1;
        }
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(t) = title {
                self.d.shared.borrow_mut().title = t.to_string();
            }
        }

        fn on_status_message(&self, _browser: Option<&mut Browser>, value: Option<&CefString>) {
            self.d.shared.borrow_mut().hover_url = value.map(|v| v.to_string()).unwrap_or_default();
        }

        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let main = frame.map(|f| f.is_main() != 0).unwrap_or(true);
            if let (true, Some(u)) = (main, url) {
                let mut s = self.d.shared.borrow_mut();
                tracing::info!("address {} +{}ms", u, s.created.elapsed().as_millis());
                let u = u.to_string();
                s.address(&u);
            }
            // A new page is on its way: draw it as soon as it paints, even
            // when it arrives in a fresh process the frame clock has not met.
            if main {
                if let Some(h) = browser.and_then(|b| b.host()) {
                    h.invalidate(PaintElementType::VIEW);
                    h.send_external_begin_frame();
                }
                self.d.shared.borrow_mut().paints += 1;
            }
        }

        fn on_favicon_urlchange(&self, browser: Option<&mut Browser>, icon_urls: Option<&mut CefStringList>) {
            let Some(list) = icon_urls else { return };
            let raw: *const cef::sys::_cef_string_list_t = (&*list).into();
            let Some(raw) = (unsafe { raw.as_ref() }) else { return };
            let raw = raw as *const _ as *mut cef::sys::_cef_string_list_t;
            let mut first = None;
            unsafe {
                let n = cef::sys::cef_string_list_size(raw);
                for i in 0..n {
                    let mut v = std::mem::zeroed();
                    if cef::sys::cef_string_list_value(raw, i, &mut v) > 0 {
                        let s = CefString::from(std::ptr::from_ref(&v)).to_string();
                        // Prefer a raster icon; SVG favicons don't rasterize here.
                        if first.is_none() || (!s.ends_with(".svg") && first.as_deref().is_some_and(|f: &str| f.ends_with(".svg"))) {
                            first = Some(s);
                        }
                    }
                }
            }
            let Some(url) = first else { return };
            if self.d.shared.borrow().favicon_url == url {
                return;
            }
            self.d.shared.borrow_mut().favicon_url = url.clone();
            if let Some(h) = browser.and_then(|b| b.host()) {
                let mut cb = FaviconBuilder::new(FaviconSink { shared: self.d.shared.clone(), url: url.clone() });
                h.download_image(Some(&url.as_str().into()), 1, 64, 0, Some(&mut cb));
            }
        }

        fn on_loading_progress_change(&self, browser: Option<&mut Browser>, progress: f64) {
            let mut s = self.d.shared.borrow_mut();
            if progress >= 1.0 && s.loading {
                if !crate::private::enabled() { tracing::info!("loaded {} +{}ms", s.url, s.created.elapsed().as_millis()); }
            }
            s.progress = progress.clamp(0.0,1.0);
            s.paints += 1;
            let restore = if progress >= 1.0 { s.restore_scroll.take() } else { None };
            drop(s);
            if let Some((x,y)) = restore.filter(|(x,y)| x.is_finite() && y.is_finite()) {
                if let Some(frame) = browser.and_then(|b|b.main_frame()) {
                    frame.execute_java_script(Some(&format!("scrollTo({x},{y})").as_str().into()), None, 0);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct FaviconSink {
    pub shared: SharedRef,
    pub url: String,
}

wrap_download_image_callback! {
    pub struct FaviconBuilder {
        f: FaviconSink,
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(&self, _image_url: Option<&CefString>, http_status_code: ::std::os::raw::c_int, image: Option<&mut Image>) {
            let Some(img) = image else { return };
            if http_status_code >= 400 || img.is_empty() != 0 {
                return;
            }
            let (mut w, mut h) = (0i32, 0i32);
            let Some(bin) = img.as_bitmap(1.0, ColorType::BGRA_8888, AlphaType::POSTMULTIPLIED, Some(&mut w), Some(&mut h)) else { return };
            let mut bytes = vec![0u8; bin.size()];
            let n = bin.data(Some(&mut bytes), 0);
            bytes.truncate(n);
            if w <= 0 || h <= 0 || bytes.len() < (w * h * 4) as usize {
                return;
            }
            self.f.shared.borrow_mut().favicon = Some(Favicon { url: self.f.url.clone(), w: w as u32, h: h as u32, bgra: bytes });
        }
    }
}

#[derive(Clone)]
pub struct Observer {
    pub shared: SharedRef,
}

wrap_dev_tools_message_observer! {
    pub struct ObserverBuilder {
        o: Observer,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, message_id: ::std::os::raw::c_int, success: ::std::os::raw::c_int, result: Option<&[u8]>) {
            tracing::debug!("cdp result id={message_id} ok={success} {}", result.map(|r| String::from_utf8_lossy(r).chars().take(160).collect::<String>()).unwrap_or_default());
            {
                // The page's print, as a PDF: written straight to disk here,
                // since a PDF is usually larger than a reply may be kept.
                let mut s = self.o.shared.borrow_mut();
                if s.print_msg == Some(message_id) {
                    s.print_msg = None;
                    let title = s.title.clone();
                    let outcome = if success == 0 {
                        Err("the page couldn't be turned into a PDF".to_string())
                    } else {
                        result.and_then(|r| serde_json::from_slice::<serde_json::Value>(r).ok())
                            .and_then(|v| v.get("data").and_then(|d| d.as_str()).map(decode64))
                            .ok_or_else(|| "the PDF didn't arrive".to_string())
                            .and_then(|bytes| save_pdf(&title, &bytes))
                    };
                    s.print_saved = Some(outcome);
                    s.paints += 1;
                    return;
                }
            }
            {
                // The watch's question, answered: the page is alive.
                let mut s = self.o.shared.borrow_mut();
                if s.ping.is_some_and(|(id, _)| id == message_id) {
                    s.ping = None;
                    s.answered = Some(crate::clock::now());
                    if s.hung.is_none() && s.overlay.as_ref().is_some_and(|o| o.kind == crate::interstitial::Kind::Hung) {
                        s.overlay = None;
                        s.paints += 1;
                    }
                    return;
                }
            }
            if success == 0 {
                return;
            }
            let Some(result) = result else { return };
            if result.len() > 1024 * 1024 {
                let mut s=self.o.shared.borrow_mut();
                if s.replies.len()>=16 {s.replies.remove(0);}
                s.replies.push((message_id,serde_json::json!({"exceptionDetails":{"text":"Result exceeds nus’s 1 MiB inspection limit. Request a smaller section."}})));
                return;
            }
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(result) {
                if let Some(id) = v.pointer("/targetInfo/targetId").and_then(|t| t.as_str()) {
                    self.o.shared.borrow_mut().target_id = Some(id.to_string());
                }
                let mut s = self.o.shared.borrow_mut();
                if s.replies.len() >= 16 {
                    s.replies.remove(0);
                }
                s.replies.push((message_id, v));
            }
        }

        fn on_dev_tools_event(&self, _browser: Option<&mut Browser>, method: Option<&CefString>, params: Option<&[u8]>) {
            let Some(method) = method else { return };
            let method = method.to_string();
            let Some(params) = params else { return };
            // Inspectors retain a bounded diagnostic sample, not arbitrarily
            // large console objects or page-generated payloads.
            if params.len() > 256 * 1024 { return; }
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(params) else { return };
            // Chrome error documents report the attempted destination here even
            // when CEF's display callback retains the original redirect URL.
            if method=="Page.frameNavigated" && v.pointer("/frame/parentId").is_none() {
                if let Some(failed)=v.pointer("/frame/unreachableUrl").and_then(|v|v.as_str()).filter(|s|!s.is_empty()) {
                    self.o.shared.borrow_mut().failed(failed,true);
                } else if let Some(address)=v.pointer("/frame/url").and_then(|v|v.as_str()) {
                    self.o.shared.borrow_mut().address(address);
                }
            }
            // The page's console and network, kept for eyes (`nus mcp`) and the block beside.
            let entry = match method.as_str() {
                "Runtime.consoleAPICalled" => {
                    let args: Vec<String> = v.get("args").and_then(|a| a.as_array()).map(|a| a.iter().map(|x| x.get("value").map(|val| match val { serde_json::Value::String(s) => s.clone(), other => other.to_string() }).or_else(|| x.get("description").and_then(|d| d.as_str()).map(String::from)).unwrap_or_default()).collect()).unwrap_or_default();
                    Some(serde_json::json!({ "kind": "console", "level": v.get("type").and_then(|t| t.as_str()).unwrap_or("log"), "text": args.join(" "), "at": v.get("timestamp") }))
                }
                "Runtime.exceptionThrown" => Some(serde_json::json!({ "kind": "console", "level": "error", "text": v.pointer("/exceptionDetails/exception/description").or_else(|| v.pointer("/exceptionDetails/text")).and_then(|t| t.as_str()).unwrap_or("exception"), "at": v.get("timestamp") })),
                "Network.requestWillBeSent" => Some(serde_json::json!({ "kind": "request", "id": v.get("requestId"), "method": v.pointer("/request/method"), "url": v.pointer("/request/url"), "type": v.get("type"), "at": v.get("timestamp") })),
                "Network.responseReceived" => Some(serde_json::json!({ "kind": "response", "id": v.get("requestId"), "status": v.pointer("/response/status"), "url": v.pointer("/response/url"), "mime": v.pointer("/response/mimeType"), "type": v.get("type"), "at": v.get("timestamp") })),
                "Network.loadingFailed" => Some(serde_json::json!({ "kind": "response", "id": v.get("requestId"), "status": 0, "error": v.get("errorText"), "type": v.get("type"), "at": v.get("timestamp") })),
                _ => None,
            };
            if let Some(mut e) = entry {
                for key in ["text","url","id"] {
                    if let Some(serde_json::Value::String(text))=e.get_mut(key) {
                        if text.len()>4096 {let mut end=4096;while !text.is_char_boundary(end){end-=1;}text.truncate(end);text.push_str("…");}
                    }
                }
                let mut s = self.o.shared.borrow_mut();
                if s.log.len() >= 400 {
                    s.log.remove(0);
                }
                s.log.push(e);
                return;
            }
            if method != "Runtime.bindingCalled" {
                return;
            }
            // A command from an interstitial: only with the token that page
            // was written with, which no site can read.
            if v.get("name").and_then(|n| n.as_str()) == Some("nusInterstitial") {
                let Some(sent) = v.get("payload").and_then(|p| p.as_str()).and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok()) else { return };
                let mut s = self.o.shared.borrow_mut();
                let ok = s.interstitial.as_ref().is_some_and(|p| sent.get("token").and_then(|t| t.as_str()) == Some(p.token.as_str()));
                if let (true, Some(verb)) = (ok, sent.get("verb").and_then(|t| t.as_str())) {
                    s.interstitial_acts.push(verb.to_string());
                    s.paints += 1;
                    crate::browser_runtime::wake();
                }
                return;
            }
            if v.get("name").and_then(|n| n.as_str()) == Some("nusPrint") {
                let mut s = self.o.shared.borrow_mut();
                s.print_asked = true;
                s.paints += 1;
                crate::browser_runtime::wake();
                return;
            }
            if v.get("name").and_then(|n| n.as_str()) != Some("nusVideo") {
                return;
            }
            let payload = v.get("payload").and_then(|p| p.as_str()).unwrap_or("null");
            let report = serde_json::from_str::<serde_json::Value>(payload).unwrap_or(serde_json::Value::Null);
            let media: Vec<Media> = report
                .get("media")
                .and_then(|m| m.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|m| {
                            Some(Media {
                                kind: m.get("k")?.as_str()?.to_string(),
                                src: m.get("src")?.as_str()?.to_string(),
                                w: m.get("w").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                                h: m.get("h").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                                blob: m.get("blob").and_then(|x| x.as_bool()).unwrap_or(false),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let video = report.get("v").cloned().and_then(|p| {
                let f = |k: &str| p.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                let b = |k: &str| p.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
                if p.is_null() {
                    return None;
                }
                Some(Video {
                    x: f("x") as f32,
                    y: f("y") as f32,
                    w: f("w") as f32,
                    h: f("h") as f32,
                    vw: f("vw") as f32,
                    vh: f("vh") as f32,
                    video_width: f("videoWidth"),
                    video_height: f("videoHeight"),
                    picture: [f("dx") as f32,f("dy") as f32,f("dw") as f32,f("dh") as f32],
                    paused: b("paused"),
                    ended: b("ended"),
                    muted: b("muted"),
                    t: f("t"),
                    dur: f("dur"),
                })
            });
            let mut s = self.o.shared.borrow_mut();
            if report.get("top").and_then(|v|v.as_bool()) == Some(true) {
            s.sleep_safe=report.get("sleepSafe").and_then(|v|v.as_bool()).unwrap_or(false);
            s.scroll_position=(report.get("scrollX").and_then(|v|v.as_f64()).unwrap_or(0.0),report.get("scrollY").and_then(|v|v.as_f64()).unwrap_or(0.0));
            }
            s.video = video;
            if s.media != media {
                s.media = media;
                s.paints += 1;
            }
        }
    }
}

/// The page's right-click: nus draws the menu itself (page_menu.rs) and
/// CEF runs whatever was picked. The entries are Chromium's, in nus's
/// words: media first, then the link, then the page.
wrap_context_menu_handler! {
    pub struct MenuBuilder {
        display: Display,
    }

    impl ContextMenuHandler {
        fn run_context_menu(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, params: Option<&mut ContextMenuParams>, _model: Option<&mut MenuModel>, callback: Option<&mut RunContextMenuCallback>) -> ::std::os::raw::c_int {
            let (Some(p), Some(cb)) = (params, callback) else { return 0 };
            let s = |c: CefStringUserfree| CefString::from(&c).to_string();
            let media_type = p.media_type();
            let src = s(p.source_url());
            let link = s(p.link_url());
            let editable = p.is_editable() != 0;
            let selected = !s(p.selection_text()).is_empty();
            let mut items: Vec<(i32, String, bool)> = Vec::new();
            let sep = |items: &mut Vec<(i32, String, bool)>| {
                if items.last().is_some_and(|i| !i.1.is_empty()) {
                    items.push((0, String::new(), false));
                }
            };
            let blob = src.starts_with("blob:") || src.starts_with("mediasource:");
            if media_type == ContextMenuMediaType::VIDEO || media_type == ContextMenuMediaType::AUDIO {
                let what = if media_type == ContextMenuMediaType::VIDEO { "VIDEO" } else { "AUDIO" };
                if blob || src.is_empty() {
                    items.push((CMD_NOTHING, "THIS PLAYER STREAMS · NOTHING TO SAVE".into(), false));
                } else {
                    items.push((CMD_SAVE_MEDIA, format!("SAVE {what}"), true));
                    items.push((CMD_COPY_MEDIA, format!("COPY {what} ADDRESS"), true));
                    items.push((CMD_OPEN_MEDIA, format!("OPEN {what} IN A NEW TAB"), true));
                }
                if media_type == ContextMenuMediaType::VIDEO {
                    items.push((CMD_PIP, "PICTURE IN PICTURE".into(), true));
                }
            } else if media_type == ContextMenuMediaType::IMAGE && !src.is_empty() {
                items.push((CMD_SAVE_MEDIA, "SAVE IMAGE".into(), true));
                items.push((CMD_COPY_MEDIA, "COPY IMAGE ADDRESS".into(), true));
                items.push((CMD_OPEN_MEDIA, "OPEN IMAGE IN A NEW TAB".into(), true));
            }
            if !link.is_empty() {
                sep(&mut items);
                items.push((CMD_OPEN_LINK, "OPEN LINK IN A NEW TAB".into(), true));
                items.push((CMD_OPEN_LINK_BESIDE, "OPEN LINK BESIDE".into(), true));
                items.push((CMD_COPY_LINK, "COPY LINK ADDRESS".into(), true));
            }
            if editable {
                sep(&mut items);
                items.push((110, "UNDO".into(), true));
                items.push((112, "CUT".into(), true));
                items.push((113, "COPY".into(), true));
                items.push((114, "PASTE".into(), true));
                items.push((117, "SELECT ALL".into(), true));
            } else if selected {
                sep(&mut items);
                items.push((113, "COPY".into(), true));
            }
            sep(&mut items);
            items.push((100, "BACK".into(), true));
            items.push((101, "FORWARD".into(), true));
            items.push((102, "RELOAD".into(), true));
            sep(&mut items);
            items.push((CMD_COPY_PAGE, "COPY PAGE ADDRESS".into(), true));
            items.push((132, "VIEW SOURCE".into(), true));
            if url::Url::parse(&s(p.page_url())).ok().and_then(|u|u.to_file_path().ok()).is_some_and(|p|crate::file_viewer::Kind::of(&p).is_some()) {
                items.push((29001,"EDIT SOURCE".into(),true));
            }
            if !crate::private::enabled(){items.push((29002,"SAVE TO READING LIST".into(),true));}

            let mut sh = self.display.shared.borrow_mut();
            if let Some(old) = sh.menu.take() {
                old.callback.cancel();
            }
            sh.menu = Some(MenuRequest { x: p.xcoord() as f32, y: p.ycoord() as f32, items, callback: cb.clone() });
            sh.paints += 1;
            1
        }

        fn on_context_menu_command(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, params: Option<&mut ContextMenuParams>, command_id: ::std::os::raw::c_int, _event_flags: EventFlags) -> ::std::os::raw::c_int {
            let Some(p) = params else { return 0 };
            let s = |c: CefStringUserfree| CefString::from(&c).to_string();
            let src = s(p.source_url());
            let link = s(p.link_url());
            let page = s(p.page_url());
            let copy = |text: &str| {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(text.to_string());
                }
            };
            let mut sh = self.display.shared.borrow_mut();
            match command_id {
                CMD_SAVE_MEDIA => {
                    sh.nus_download_at = Some(crate::clock::now());
                    if let Some(h) = browser.and_then(|b| b.host()) {
                        h.start_download(Some(&src.as_str().into()));
                    }
                }
                CMD_COPY_MEDIA => { copy(&src); sh.said = Some(("Copied".into(), src.to_string())); }
                CMD_OPEN_MEDIA => sh.opens.push((src, false)),
                CMD_OPEN_LINK => sh.opens.push((link, false)),
                CMD_OPEN_LINK_BESIDE => sh.opens.push((link, true)),
                CMD_COPY_LINK => { copy(&link); sh.said = Some(("Copied".into(), link.to_string())); }
                CMD_COPY_PAGE => { copy(&page); sh.said = Some(("Copied".into(), page.to_string())); }
                CMD_PIP => sh.said = Some(("PIP".into(), String::new())),
                29001 => sh.edit_source=url::Url::parse(&page).ok().and_then(|u|u.to_file_path().ok()).filter(|p|crate::file_viewer::Kind::of(p).is_some()),
                29002 => sh.save_reading=true,
                CMD_NOTHING => {}
                _ => return 0,
            }
            sh.paints += 1;
            1
        }
    }
}

wrap_life_span_handler! {
    pub struct LifeBuilder {
        d: Display,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                LIVE_BROWSERS.with(|v| { v.borrow_mut().insert(b.identifier()); });
                // A popup Chromium made: the app adopts it into a tab.
                if let Ok(mut s) = self.d.shared.try_borrow_mut() {
                    if s.adopted.is_none() && s.created_by_chromium { s.adopted = Some(b.clone()); s.paints += 1; }
                }
            }
        }
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                LIVE_BROWSERS.with(|v| { v.borrow_mut().remove(&b.identifier()); });
                if let Some(counts) = BLOCKED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { counts.remove(&b.identifier()); }
            }
            if let Ok(mut s) = self.d.shared.try_borrow_mut() {
                if !s.letting_go && !s.suspended { s.gone = true; s.paints += 1; }
            }
        }

        // Popups would be separate native windows; hand the URL to the app instead.
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let _ = browser;
            let factory = self.d.shared.borrow().popup_factory.clone();
            // A real window, joined to its opener: sign-in and payment popups
            // talk back through `window.opener` and close themselves.
            if let (Some(factory), Some(client_out), Some(info)) = (factory, _client, _window_info) {
                let (client, child) = factory();
                {
                    let mut c = child.borrow_mut();
                    c.created_by_chromium = true;
                    if let Some(url) = target_url { c.url = url.to_string(); c.requested_url = url.to_string(); c.loading = true; }
                }
                *client_out = Some(client);
                info.windowless_rendering_enabled = 1;
                info.shared_texture_enabled = 1;
                info.external_begin_frame_enabled = 1;
                if let Some(settings) = _settings { settings.windowless_frame_rate = 60; }
                self.d.shared.borrow_mut().opened.push(child);
                return 0;
            }
            if let Some(url) = target_url {
                self.d.shared.borrow_mut().popup = Some(url.to_string());
            }
            1
        }
    }
}

/// `nus://crash` and friends, done to the page in this frame for real.
fn run_debug(frame: &Frame, what: &crate::interstitial::Internal) {
    use crate::interstitial::Internal;
    let js = match what {
        // A renderer crash the ordinary way: a bad pointer in the page's own
        // process is not something script can do, so CDP does it.
        Internal::Crash => "",
        // V8 aborts the renderer once the heap limit is hit.
        Internal::Oom => "setTimeout(()=>{const a=[];for(;;)a.push(new Array(1e7).fill(1))},0)",
        Internal::Hang => "setTimeout(()=>{for(;;){}},0)",
        Internal::BlockDownload => "(()=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob(['MZ'],{type:'application/octet-stream'}));a.download='invoice.pdf.exe';document.documentElement.appendChild(a);a.click();a.remove()})()",
        Internal::Show(_) => return,
    };
    if js.is_empty() {
        if let Some(host) = frame.browser().and_then(|b| b.host()) {
            let msg = serde_json::json!({ "id": 1_900_000_001, "method": "Page.crash", "params": {} }).to_string();
            host.send_dev_tools_message(Some(msg.as_bytes()));
        }
        return;
    }
    frame.execute_java_script(Some(&js.into()), Some(&CefString::from(&frame.url())), 0);
}

/// The certificate this window is serving to the phone right now, matched
/// by its exact bytes at the phone's own address.
fn phone_certificate(url: &str, ssl_info: Option<&mut Sslinfo>) -> bool {
    let Some(phone) = crate::phone::current() else { return false };
    let Ok(url) = url::Url::parse(url) else { return false };
    let host = url.host_str().unwrap_or_default();
    let ours = url.port() == Some(phone.port)
        && (host == phone.host || host == "127.0.0.1" || host == "localhost" || host == "[::1]");
    if !ours {
        return false;
    }
    let Some(der) = ssl_info.and_then(|i| i.x509_certificate()).and_then(|c| c.derencoded()) else { return false };
    let mut bytes = vec![0u8; der.size()];
    if der.data(Some(&mut bytes), 0) != bytes.len() {
        return false;
    }
    crate::phone::is_session_certificate(&phone, &bytes)
}

pub type BrowserSlot = StdRc<RefCell<Option<Browser>>>;

#[derive(Clone)]
pub struct Capture {
    pub slot: BrowserSlot,
}

wrap_life_span_handler! {
    pub struct CaptureBuilder {
        c: Capture,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                *self.c.slot.borrow_mut() = Some(b.clone());
            }
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            *self.c.slot.borrow_mut() = None;
        }
    }
}

/// The DevTools frontend for a page, in a windowless browser of our own.
pub type DevToolsView = BrowserTab;

wrap_find_handler! {
    pub struct FindBuilder {
        display: Display,
    }

    impl FindHandler {
        fn on_find_result(&self, _browser: Option<&mut Browser>, _identifier: ::std::os::raw::c_int, count: ::std::os::raw::c_int, _selection_rect: Option<&Rect>, active_match_ordinal: ::std::os::raw::c_int, _final_update: ::std::os::raw::c_int) {
            let mut s = self.display.shared.borrow_mut();
            s.find = Some((count, active_match_ordinal));
            s.paints += 1;
        }
    }
}

/// How the user started a download, if they did: nus asked for it, or the
/// user's own click or key reached this page (or a navigation Chromium
/// marked as their gesture) just before it began. Anything else (a timer,
/// a service worker, a background fetch) gets None and can never keep the
/// machine awake. Pages have no say in this.
fn download_origin(s: &Shared) -> Option<crate::finish_work::Origin> {
    let recent = |t: Option<std::time::Instant>, secs: u64| t.is_some_and(|t| crate::clock::since(t) < std::time::Duration::from_secs(secs));
    if recent(s.nus_download_at, 10) {
        Some(crate::finish_work::Origin::NusAction)
    } else if recent(s.gesture_at, 5) || recent(s.nav_gesture_at, 5) {
        Some(crate::finish_work::Origin::BrowserGesture)
    } else {
        None
    }
}

wrap_download_handler! {
    pub struct DownloadBuilder {
        display: Display,
        container: String,
    }

    impl DownloadHandler {
        // The wrapper's default is 0: every download refused. Allow them all;
        // on_before_download picks the path.
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(&self, _browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, suggested_name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> ::std::os::raw::c_int {
            let Some(item) = download_item else { return 0 };
            let Some(cb)=callback else {return 0;};
            let original=suggested_name.map(|s|s.to_string()).filter(|s|!s.is_empty()).unwrap_or_else(||"download".into());
            let from=CefString::from(&item.url()).to_string();
            if let Some(why)=crate::interstitial::dangerous_file(&original).filter(|_|!crate::interstitial::allowed(&format!("file:{from}"))) {
                // Not continuing the callback cancels it: nothing is written.
                let mut s=self.display.shared.borrow_mut();
                s.overlay=Some(crate::interstitial::Page::file(&from,&original,why));
                s.paints+=1;
                return 1;
            }
            // A tab that never showed a page of its own (a link opened in a
            // new tab that turned out to be a file): nothing to keep it for.
            // It stays until the file is done: closing the page now would
            // cancel the download it started.
            {let mut s=self.display.shared.borrow_mut();if !s.committed && s.url.starts_with("http") {s.download_waiting=true;}}
            let title=self.display.shared.borrow().title.clone();
            let origin=download_origin(&self.display.shared.borrow());
            let name=crate::downloads::filename(&original,&title,crate::downloads::rename_mode());
            let dir=downloads_dir();if std::fs::create_dir_all(&dir).is_err(){return 0;}
            let path={
                let mut rows=DOWNLOADS.lock().unwrap();
                let path=crate::downloads::available_path(&dir,&name,&rows);
                let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                let key=(now.as_micros() as u64).max(rows.iter().map(|d|d.key).max().unwrap_or(0)+1);
                rows.push(Download{key,id:item.id(),name:path.file_name().unwrap().to_string_lossy().into(),original:crate::downloads::safe_name(&original),path:path.to_string_lossy().into(),url:CefString::from(&item.url()).to_string(),container:self.container.clone(),source_url:_browser.and_then(|b|b.main_frame()).map(|f|CefString::from(&f.url()).to_string()).unwrap_or_default(),title,total:item.total_bytes(),started:now.as_secs(),live:true,origin,..Default::default()});
                crate::downloads::save(&rows);path
            };
            crate::downloads::changed();
            // ASK WHERE TO SAVE: the download waits for the dialog (the
            // app's tick opens it and continues the callback); the row is
            // there already, so the list shows it waiting.
            if DOWNLOAD_ASK.load(std::sync::atomic::Ordering::Relaxed) && std::env::var_os("NUS_SHOT").is_none() {
                let key = DOWNLOADS.lock().unwrap().iter().find(|d| d.live && d.id == item.id()).map(|d| d.key).unwrap_or(0);
                SAVE_ASKS.with(|q| q.borrow_mut().push(SaveAsk { key, suggested: path, callback: cb.clone() }));
                self.display.shared.borrow_mut().paints += 1;
                return 1;
            }
            cb.cont(Some(&path.to_string_lossy().as_ref().into()),0);
            self.display.shared.borrow_mut().paints += 1;
            1
        }

        fn on_download_updated(&self, _browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, callback: Option<&mut DownloadItemCallback>) {
            let Some(item) = download_item else { return };
            let id = item.id();
            let mut list = DOWNLOADS.lock().unwrap();
            if let Some(d) = list.iter_mut().find(|d| d.live && d.id == id) {
                let before=(d.done,d.cancelled,d.interrupted,d.paused);
                d.received = item.received_bytes();
                d.total = item.total_bytes();
                d.done = item.is_complete() != 0;
                d.cancelled = item.is_canceled() != 0;
                d.interrupted = item.is_interrupted() != 0;
                d.paused = item.is_paused() != 0;
                d.speed = item.current_speed();
                crate::downloads::track(d.key,callback,d.active());
                let p = CefString::from(&item.full_path()).to_string();
                if !p.is_empty() {
                    d.name = std::path::Path::new(&p).file_name().unwrap_or_default().to_string_lossy().into();
                    d.path = p;
                }
                let finished = d.done || d.cancelled || d.interrupted;
                if before!=(d.done,d.cancelled,d.interrupted,d.paused){crate::downloads::save(&list);}
                crate::downloads::changed();
                if finished {
                    let mut s=self.display.shared.borrow_mut();
                    if s.download_waiting && !s.committed {s.download_waiting=false;s.download_only=true;}
                }
            }
            self.display.shared.borrow_mut().paints += 1;
        }
    }
}

/// The permission types, in words for the band.
fn permission_words(mask: u32) -> String {
    let names: &[(u32, &str)] = &[
        (PermissionRequestTypes::CAMERA_STREAM.get_raw() as u32, "camera"),
        (PermissionRequestTypes::MIC_STREAM.get_raw() as u32, "microphone"),
        (PermissionRequestTypes::GEOLOCATION.get_raw() as u32, "location"),
        (PermissionRequestTypes::NOTIFICATIONS.get_raw() as u32, "notifications"),
        (PermissionRequestTypes::CLIPBOARD.get_raw() as u32, "the clipboard"),
        (PermissionRequestTypes::MIDI_SYSEX.get_raw() as u32, "midi"),
        (PermissionRequestTypes::MULTIPLE_DOWNLOADS.get_raw() as u32, "multiple downloads"),
        (PermissionRequestTypes::POINTER_LOCK.get_raw() as u32, "pointer lock"),
        (PermissionRequestTypes::KEYBOARD_LOCK.get_raw() as u32, "keyboard lock"),
        (PermissionRequestTypes::IDLE_DETECTION.get_raw() as u32, "idle detection"),
        (PermissionRequestTypes::LOCAL_FONTS.get_raw() as u32, "your fonts"),
        (PermissionRequestTypes::DISK_QUOTA.get_raw() as u32, "more storage"),
    ];
    let mut out: Vec<&str> = names.iter().filter(|(bit, _)| mask & *bit != 0).map(|(_, n)| *n).collect();
    if out.is_empty() {
        out.push("a permission");
    }
    out.join(" and ")
}

wrap_permission_handler! {
    pub struct PermissionBuilder {
        display: Display,
    }

    impl PermissionHandler {
        fn on_request_media_access_permission(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, requesting_origin: Option<&CefString>, requested_permissions: u32, callback: Option<&mut MediaAccessCallback>) -> ::std::os::raw::c_int {
            self.display.shared.borrow_mut().capture_guard=true;
            let Some(cb) = callback else { return 0 };
            let origin = requesting_origin.map(|s| s.to_string()).unwrap_or_default();
            let video = requested_permissions & MediaAccessPermissionTypes::DEVICE_VIDEO_CAPTURE.get_raw() as u32 != 0;
            let audio = requested_permissions & MediaAccessPermissionTypes::DEVICE_AUDIO_CAPTURE.get_raw() as u32 != 0;
            let what = match (video, audio) { (true, true) => "camera and microphone", (true, false) => "camera", (false, true) => "microphone", _ => "screen capture" }.to_string();
            // Asking you is pointless when macOS will refuse nus anyway.
            if let Some(denied) = crate::interstitial::os_denied(audio, video) {
                cb.cont(0);
                let mut s = self.display.shared.borrow_mut();
                let url = s.url.clone();
                s.overlay = Some(crate::interstitial::Page::permission(&url, denied));
                s.paints += 1;
                return 1;
            }
            if let Some(allow) = crate::sites::remembered(&origin, &what) {
                cb.cont(if allow { requested_permissions } else { 0 });
                return 1;
            }
            let mut s = self.display.shared.borrow_mut();
            s.permission = Some(PermissionAsk { origin, what, kind: AskKind::Media(cb.clone(), requested_permissions) });
            s.paints += 1;
            1
        }

        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _prompt_id: u64, requesting_origin: Option<&CefString>, requested_permissions: u32, callback: Option<&mut PermissionPromptCallback>) -> ::std::os::raw::c_int {
            let Some(cb) = callback else { return 0 };
            let origin = requesting_origin.map(|s| s.to_string()).unwrap_or_default();
            let what = permission_words(requested_permissions);
            if let Some(allow) = crate::sites::remembered(&origin, &what) {
                cb.cont(if allow { PermissionRequestResult::ACCEPT } else { PermissionRequestResult::DENY });
                return 1;
            }
            let mut s = self.display.shared.borrow_mut();
            s.permission = Some(PermissionAsk { origin, what, kind: AskKind::Prompt(cb.clone()) });
            s.paints += 1;
            1
        }

        fn on_dismiss_permission_prompt(&self, _browser: Option<&mut Browser>, _prompt_id: u64, _result: PermissionRequestResult) {
            let mut s = self.display.shared.borrow_mut();
            s.permission = None;
            s.paints += 1;
        }
    }
}

wrap_resource_request_handler! {
    pub struct BlockBuilder {
        navigation: bool,
        viewer: crate::file_viewer::Shared,
    }

    impl ResourceRequestHandler {
        fn on_resource_redirect(&self,browser:Option<&mut Browser>,frame:Option<&mut Frame>,request:Option<&mut Request>,_response:Option<&mut Response>,new_url:Option<&mut CefString>) {
            if !self.navigation || !frame.is_some_and(|f|f.is_main()!=0){return;}
            if let (Some(browser),Some(request),Some(url))=(browser,request,new_url) {
                // Resource callbacks run on CEF's IO thread. Deliver through the UI loop.
                crate::browser_runtime::redirect(browser.identifier(),CefString::from(&request.url()).to_string(),url.to_string());
            }
        }
        fn resource_handler(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>) -> Option<ResourceHandler> {
            if !self.navigation || !frame.is_some_and(|f|f.is_main()!=0) {return None;}
            let url=CefString::from(&request?.url()).to_string();
            let config=self.viewer.read().ok()?.clone();
            let bytes=crate::file_viewer::load(&url,&config)?;
            Some(DocumentResource::new(Arc::new(bytes),Arc::new(std::sync::Mutex::new(0))))
        }
        fn on_before_resource_load(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _callback: Option<&mut Callback>) -> ReturnValue {
            let Some(req) = request else { return ReturnValue::CONTINUE };
            // BROWSER · PRIVACY SIGNAL: Global Privacy Control and Do Not
            // Track on every request, navigations included.
            if PRIVACY_SIGNAL.load(std::sync::atomic::Ordering::Relaxed) {
                req.set_header_by_name(Some(&"Sec-GPC".into()), Some(&"1".into()), 1);
                req.set_header_by_name(Some(&"DNT".into()), Some(&"1".into()), 1);
            }
            // A sign-in you gave this origin goes with each of its requests.
            if let Some(h) = signin_for(&CefString::from(&req.url()).to_string()) {
                req.set_header_by_name(Some(&"Authorization".into()), Some(&h.as_str().into()), 1);
            }
            // Never block the navigation itself, only what the page pulls in.
            if self.navigation {
                return ReturnValue::CONTINUE;
            }
            let Some(browser) = browser else { return ReturnValue::CONTINUE };
            let page = browser.main_frame().map(|f| CefString::from(&f.url()).to_string()).unwrap_or_default();
            if !crate::sites::prefs(&crate::sites::host_of(&page)).blocking {
                return ReturnValue::CONTINUE;
            }
            let url = CefString::from(&req.url()).to_string();
            if blocked(&url, &page) {
                let mut counts = BLOCKED.lock().unwrap_or_else(|e| e.into_inner());
                *counts.get_or_insert_with(Default::default).entry(browser.identifier()).or_default() += 1;
                return ReturnValue::CANCEL;
            }
            ReturnValue::CONTINUE
        }

        fn cookie_access_filter(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>) -> Option<CookieAccessFilter> {
            let page = browser.and_then(|b| b.main_frame()).map(|f| CefString::from(&f.url()).to_string()).unwrap_or_default();
            if crate::sites::prefs(&crate::sites::host_of(&page)).cookies {
                return None;
            }
            Some(NoCookies::new())
        }
    }
}

// The site's cookies are off: none sent, none kept.
wrap_cookie_access_filter! {
    pub struct NoCookies;

    impl CookieAccessFilter {
        fn can_send_cookie(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, _cookie: Option<&Cookie>) -> ::std::os::raw::c_int {
            0
        }
        fn can_save_cookie(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, _response: Option<&mut Response>, _cookie: Option<&Cookie>) -> ::std::os::raw::c_int {
            0
        }
    }
}

wrap_request_handler! {
    pub struct RequestBuilder {
        display: Display,
        viewer: crate::file_viewer::Shared,
    }

    impl RequestHandler {
        fn on_before_browse(&self,_browser:Option<&mut Browser>,frame:Option<&mut Frame>,request:Option<&mut Request>,_gesture: ::std::os::raw::c_int,_redirect: ::std::os::raw::c_int)->::std::os::raw::c_int {
            if _gesture != 0 { self.display.shared.borrow_mut().nav_gesture_at = Some(crate::clock::now()); }
            // An address for another app: the page stays; the app asks first.
            if let Some(url)=request.as_ref().map(|r|CefString::from(&r.url()).to_string()) {
                if !web_scheme(&url) {
                    if _gesture != 0 || frame.as_ref().is_some_and(|f|f.is_main()!=0) {
                        let mut s=self.display.shared.borrow_mut();
                        s.external=Some(url);s.paints+=1;
                    }
                    return 1;
                }
            }
            if frame.is_some_and(|f|f.is_main()!=0) {if let Some(request)=request {let url=CefString::from(&request.url()).to_string();if !url.starts_with("chrome-error:"){
                if crate::interstitial::dangerous(&url) {
                    let can_back=_browser.is_some_and(|b|b.can_go_back()!=0);
                    let mut s=self.display.shared.borrow_mut();
                    s.interstitial=Some(crate::interstitial::Page::malware(&url,"profile/dangerous.txt",can_back));
                    s.inject=true;s.blank=true;s.url=url;s.loading=false;s.paints+=1;
                    return 1;
                }
                // A new page starts over: its questions are counted afresh.
                if let Some(b)=_browser.as_ref() {crate::page_dialog::reset(b.identifier());}
                self.display.shared.borrow_mut().navigation(&url);
            }}}
            0
        }
        fn resource_request_handler(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, is_navigation: ::std::os::raw::c_int, _is_download: ::std::os::raw::c_int, _request_initiator: Option<&CefString>, _disable_default_handling: Option<&mut ::std::os::raw::c_int>) -> Option<ResourceRequestHandler> {
            // Chromium's IO thread: nothing here may touch `Display`, whose
            // `Rc<RefCell>` belongs to the UI thread.
            Some(BlockBuilder::new(is_navigation != 0,self.viewer.clone()))
        }

        fn on_render_process_terminated(&self,_browser:Option<&mut Browser>,status:TerminationStatus,error_code: ::std::os::raw::c_int,error_string:Option<&CefString>) {
            let status=cef::sys::cef_termination_status_t::from(status);
            let mut s=self.display.shared.borrow_mut();
            s.hung=None;
            if s.overlay.as_ref().is_some_and(|o|o.kind==crate::interstitial::Kind::Hung) {s.overlay=None;}
            let url=s.url.clone();
            let oom=status==cef::sys::cef_termination_status_t::TS_PROCESS_OOM;
            let killed=status==cef::sys::cef_termination_status_t::TS_PROCESS_WAS_KILLED || std::mem::take(&mut s.stopping);
            s.ping=None;
            let code=error_string.map(|e|e.to_string()).filter(|e|!e.is_empty()&&e.parse::<i64>().is_err()).unwrap_or_else(||format!("exit code {error_code}"));
            s.interstitial=Some(crate::interstitial::Page::crashed(&url,oom,&code,killed));
            s.inject=true;s.blank=true;s.loading=false;s.paints+=1;
            crate::browser_runtime::wake();
        }
        fn on_render_process_unresponsive(&self,_browser:Option<&mut Browser>,callback:Option<&mut UnresponsiveProcessCallback>)->::std::os::raw::c_int {
            let Some(cb)=callback else {return 0};
            let mut s=self.display.shared.borrow_mut();
            let url=s.url.clone();
            s.hung=Some(cb.clone());
            s.overlay=Some(crate::interstitial::Page::hung(&url,15));
            s.paints+=1;
            crate::browser_runtime::wake();
            1
        }
        fn on_render_process_responsive(&self,_browser:Option<&mut Browser>) {
            let mut s=self.display.shared.borrow_mut();
            s.hung=None;
            if s.overlay.as_ref().is_some_and(|o|o.kind==crate::interstitial::Kind::Hung) {s.overlay=None;}
            s.paints+=1;
        }

        /// One certificate is trusted without asking: the one this window is
        /// serving to the phone right now, matched by its exact bytes. The
        /// phone has to decide for itself, but nus knows its own key, so
        /// opening the phone's page in a tab here is not a lesson in clicking
        /// through warnings. Every other certificate error is Chromium's to
        /// show, including anything at the same address after the phone stops.
        fn on_certificate_error(&self, _browser: Option<&mut Browser>, _cert_error: Errorcode, request_url: Option<&CefString>, ssl_info: Option<&mut Sslinfo>, callback: Option<&mut Callback>) -> ::std::os::raw::c_int {
            let Some(c) = callback else { return 0 };
            let url = request_url.map(CefString::to_string).unwrap_or_default();
            // The phone's own certificate, or a host you went ahead to from
            // the transcript this session: through. Everything else fails as
            // a load error, so nus's page shows rather than Chrome's.
            if phone_certificate(&url, ssl_info) || crate::interstitial::allowed(&format!("cert:{}", crate::interstitial::host(&url))) {
                c.cont();
            } else {
                c.cancel();
            }
            1
        }
    }
}

wrap_client! {
    pub struct ClientBuilder {
        render: RenderHandler,
        display: DisplayHandler,
        load: LoadHandler,
        life: LifeSpanHandler,
        find: FindHandler,
        download: DownloadHandler,
        permission: PermissionHandler,
        request: RequestHandler,
        menu: ContextMenuHandler,
        dialog: JsdialogHandler,
    }

    impl Client {
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> {
            Some(self.dialog.clone())
        }
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {Some(self.load.clone())}
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life.clone())
        }
        fn find_handler(&self) -> Option<FindHandler> {
            Some(self.find.clone())
        }
        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(self.download.clone())
        }
        fn permission_handler(&self) -> Option<PermissionHandler> {
            Some(self.permission.clone())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request.clone())
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(self.menu.clone())
        }
    }
}

wrap_jsdialog_handler! {
    pub struct DialogBuilder {
        d: Display,
    }

    impl JsdialogHandler {
        fn on_jsdialog(&self, browser: Option<&mut Browser>, origin_url: Option<&CefString>, dialog_type: JsdialogType, message_text: Option<&CefString>, default_prompt_text: Option<&CefString>, callback: Option<&mut JsdialogCallback>, suppress_message: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            let Some(cb) = callback else { return 0 };
            // A page told to stop asking: Chromium drops the question (and
            // counts it against the page), the way its own dialogs do.
            if let Some(b) = browser.as_ref() {
                if !crate::page_dialog::may_ask(b.identifier()) {
                    if let Some(s) = suppress_message { *s = 1; }
                    return 0;
                }
            }
            // Who asks is the asking frame's own origin, from Chromium; never
            // the top page's, so an iframe can't borrow its name.
            let url = origin_url.map(|u| u.to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| "this page".into());
            let message = message_text.map(|m| m.to_string()).unwrap_or_default();
            let message = if message.chars().count() > 300 { format!("{}…", message.chars().take(300).collect::<String>()) } else { message };
            let page = match cef::sys::cef_jsdialog_type_t::from(dialog_type) {
                cef::sys::cef_jsdialog_type_t::JSDIALOGTYPE_CONFIRM => crate::interstitial::Page::confirm(&url, &message),
                cef::sys::cef_jsdialog_type_t::JSDIALOGTYPE_PROMPT => crate::interstitial::Page::prompt(&url, &message, &default_prompt_text.map(|d| d.to_string()).unwrap_or_default()),
                _ => crate::interstitial::Page::alert(&url, &message),
            };
            let mut s = self.d.shared.borrow_mut();
            // One question at a time; a second while one is up is refused.
            if s.dialog.is_some() { return 0; }
            s.dialog = Some(Dialog::Js(cb.clone()));
            s.overlay = Some(page);
            s.paints += 1;
            1
        }
        fn on_before_unload_dialog(&self, browser: Option<&mut Browser>, _message_text: Option<&CefString>, is_reload: ::std::os::raw::c_int, callback: Option<&mut JsdialogCallback>) -> ::std::os::raw::c_int {
            let Some(cb) = callback else { return 0 };
            // Told to stop asking: leaving goes ahead without the question.
            if browser.as_ref().is_some_and(|b| !crate::page_dialog::may_ask(b.identifier())) {
                cb.cont(1, None);
                return 1;
            }
            let mut s = self.d.shared.borrow_mut();
            if s.dialog.is_some() { return 0; }
            let url = s.url.clone();
            s.dialog = Some(Dialog::Js(cb.clone()));
            s.overlay = Some(crate::interstitial::Page::leave(&url, is_reload != 0));
            s.paints += 1;
            1
        }
        fn on_reset_dialog_state(&self, _browser: Option<&mut Browser>) {
            let mut s = self.d.shared.borrow_mut();
            s.dialog = None;
            if s.overlay.as_ref().is_some_and(|o| o.kind == crate::interstitial::Kind::Dialog) { s.overlay = None; s.paints += 1; }
        }
    }
}

/// The handlers a page's browser answers to, all bound to its `Shared`.
fn make_client(shared: &SharedRef, device: wgpu::Device, bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>, container: &str) -> Client {
    let osr = Osr { shared: shared.clone(), device, bind_texture };
    ClientBuilder::new(
        RenderBuilder::new(osr),
        DisplayBuilder::new(Display { shared: shared.clone() }),
        LoadBuilder::new(shared.clone()),
        LifeBuilder::new(Display { shared: shared.clone() }),
        FindBuilder::new(Display { shared: shared.clone() }),
        DownloadBuilder::new(Display { shared: shared.clone() }, container.to_string()),
        PermissionBuilder::new(Display { shared: shared.clone() }),
        RequestBuilder::new(Display { shared: shared.clone() }, shared.borrow().viewer.clone()),
        MenuBuilder::new(Display { shared: shared.clone() }),
        DialogBuilder::new(Display { shared: shared.clone() }),
    )
}

/// Makes the page (and its client) for a window a page opens. The new
/// page can open windows of its own the same way.
fn popup_factory(device: wgpu::Device, bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>, container: String) -> StdRc<dyn Fn() -> (Client, SharedRef)> {
    StdRc::new(move || {
        let shared: SharedRef = StdRc::new(RefCell::new(Shared { size: (100.0, 100.0), scale: 1.0, ..Default::default() }));
        let client = make_client(&shared, device.clone(), bind_texture.clone(), &container);
        shared.borrow_mut().popup_factory = Some(popup_factory(device.clone(), bind_texture.clone(), container.clone()));
        (client, shared)
    })
}

thread_local! { static LIVE_BROWSERS: RefCell<std::collections::HashSet<i32>> = RefCell::new(Default::default()); }
pub fn live_count() -> usize { LIVE_BROWSERS.with(|b| b.borrow().len()) }

pub struct BrowserTab {
    pub browser: Option<Browser>,
    pub shared: SharedRef,
    _observer: Option<Registration>,
}

impl Drop for BrowserTab {
    fn drop(&mut self) {
        self._observer.take();
        let menu = self.shared.borrow_mut().menu.take();
        if let Some(menu) = menu { menu.callback.cancel(); }
        // Releasing the Rust wrapper does not close a CEF browser. Without
        // this, closed/sleeping tabs keep renderers, timers and GPU surfaces.
        if let Ok(mut s) = self.shared.try_borrow_mut() { s.letting_go = true; }
        if let Some(host) = self.browser.as_ref().and_then(|b|b.host()) { host.close_dev_tools(); host.close_browser(1); }
    }
}

impl BrowserTab {
    /// Single-entry pages can be recreated without losing navigation history.
    /// Complex/dirty/media pages and DevTools keep their live browser.
    /// Requests content blocking refused on this page so far.
    pub fn blocked(&self) -> u32 {
        let Some(id) = self.browser.as_ref().map(|b| b.identifier()) else { return 0 };
        BLOCKED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&id).copied()).unwrap_or(0)
    }
    pub fn can_suspend(&self) -> bool {
        let s=self.shared.borrow();
        self.browser.is_some() && s.sleep_safe && !s.capture_guard && !s.loading && s.permission.is_none()
            && !self.can_go_back() && !self.can_go_forward() && !self.has_devtools()
    }
    pub fn suspend(&mut self) {
        self._observer.take();
        self.shared.borrow_mut().letting_go=true;
        if let Some(browser)=self.browser.take() {if let Some(host)=browser.host(){host.close_browser(1);}}
        let mut s=self.shared.borrow_mut();
        if let Some(menu)=s.menu.take(){menu.callback.cancel();}
        s.suspended=true;s.bind=None;s.select.bind=None;s.log=Vec::new();s.replies=Vec::new();
    }

    pub fn create(
        url: &str,
        shared: SharedRef,
        device: wgpu::Device,
        bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>,
    ) -> Option<BrowserTab> {
        Self::create_in(url, shared, device, bind_texture, crate::containers::PERSONAL)
    }

    /// `create`, in a container's request context (its own cookie jar).
    pub fn create_in(
        url: &str,
        shared: SharedRef,
        device: wgpu::Device,
        bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>,
        container: &str,
    ) -> Option<BrowserTab> {
        if !crate::browser_runtime::ensure() { return None; }
        // A `nus://` address: a blank document, and the page (or command) on it.
        let internal = crate::interstitial::internal(url);
        let original_url = internal.as_ref().map(|_| url.to_string());
        let url = if internal.is_some() { "about:blank" } else { url };
        if let Some(i) = internal.clone() {
            let mut s = shared.borrow_mut();
            match i {
                crate::interstitial::Internal::Show(page) if page.kind.native() => s.overlay = Some(page),
                crate::interstitial::Internal::Show(page) => { s.interstitial = Some(page); s.inject = true; }
                other => s.debug = Some(other),
            }
        }
        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            shared_texture_enabled: 1,
            external_begin_frame_enabled: 1,
            ..Default::default()
        };
        let settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };
        let mut client = make_client(&shared, device.clone(), bind_texture.clone(), container);
        let factory = popup_factory(device, bind_texture, container.to_string());
        shared.borrow_mut().popup_factory = Some(factory);
        // The container's context: the global one for PERSONAL, else its own
        // cookie jar and cache under profile/containers.
        let mut context = crate::containers::context(container)?;
        if crate::private::enabled() && !CefString::from(&context.cache_path()).to_string().is_empty() {
            tracing::error!("Refusing a persistent browser context in incognito");
            return None;
        }
        let t0 = crate::clock::now();
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url.into()),
            Some(&settings),
            None,
            Some(&mut context),
        )?;
        if !crate::private::enabled() { tracing::info!("create_browser_sync {url} took {}ms", crate::clock::since(t0).as_millis()); }
        let tab = BrowserTab::attach(browser, shared);
        if internal.is_some() {
            if let Some(original) = original_url { tab.shared.borrow_mut().url = original; }
        }
        Some(tab)
    }

    /// A browser (made by nus or by Chromium for a popup), wired up as
    /// every page is: the DevTools channel and the scripts nus injects.
    fn attach(browser: Browser, shared: SharedRef) -> BrowserTab {
        let mut observer = ObserverBuilder::new(Observer { shared: shared.clone() });
        let registration = browser.host().and_then(|h| h.add_dev_tools_message_observer(Some(&mut observer)));
        let tab = BrowserTab { browser: Some(browser), shared, _observer: registration };
        tab.devtools("Runtime.enable", serde_json::json!({}));
        tab.devtools("Page.enable", serde_json::json!({}));
        tab.devtools("Network.enable", serde_json::json!({}));
        tab.devtools("Runtime.addBinding", serde_json::json!({ "name": "nusVideo" }));
        tab.devtools("Runtime.addBinding", serde_json::json!({ "name": "nusInterstitial" }));
        tab.devtools("Runtime.addBinding", serde_json::json!({ "name": "nusPrint" }));
        tab.devtools("Page.addScriptToEvaluateOnNewDocument", serde_json::json!({ "source": VIDEO_JS }));
        tab.devtools("Page.addScriptToEvaluateOnNewDocument", serde_json::json!({ "source": PRINT_JS }));
        tab.devtools("Runtime.evaluate", serde_json::json!({ "expression": PRINT_JS }));
        tab.devtools("Runtime.evaluate", serde_json::json!({ "expression": VIDEO_JS }));
        let id = tab.devtools("Target.getTargetInfo", serde_json::json!({}));
        tab.shared.borrow_mut().target_msg = id;
        tab
    }

    /// A popup's page, once Chromium has made its browser.
    pub fn adopt(shared: SharedRef) -> Option<BrowserTab> {
        let browser = shared.borrow_mut().adopted.take()?;
        Some(BrowserTab::attach(browser, shared))
    }

    /// Close as a person would: the page's `beforeunload` gets its say.
    /// True when it's closing now; false when the page asked to stay open
    /// until you answer (its leave-page question is up).
    pub fn ask_to_close(&self) -> bool {
        let Some(h) = self.host() else { return true };
        self.shared.borrow_mut().letting_go = false;
        h.try_close_browser() != 0
    }

    pub fn host(&self) -> Option<BrowserHost> {
        self.browser.as_ref().and_then(|b|b.host())
    }

    /// Leave the page's fullscreen, as Esc does in any browser.
    pub fn exit_fullscreen(&self) {
        if let Some(h) = self.host() { h.exit_fullscreen(1); }
    }

    /// Find in page; `next` continues the same search.
    pub fn find(&self, text: &str, forward: bool, next: bool) {
        if let Some(h) = self.host() {
            h.find(Some(&text.into()), forward as i32, 0, next as i32);
        }
    }

    pub fn stop_find(&self) {
        if let Some(h) = self.host() {
            h.stop_finding(1);
        }
        self.shared.borrow_mut().find = None;
    }

    /// Answer the page's permission ask.
    pub fn answer_permission(&self, allow: bool) {
        let ask = self.shared.borrow_mut().permission.take();
        if let Some(ask) = ask {
            match ask.kind {
                AskKind::Prompt(cb) => cb.cont(if allow { PermissionRequestResult::ACCEPT } else { PermissionRequestResult::DENY }),
                AskKind::Media(cb, perms) => cb.cont(if allow { perms } else { 0 }),
            }
        }
    }

    /// Send a DevTools protocol command; returns its message id.
    pub fn devtools(&self, method: &str, params: serde_json::Value) -> i32 {
        let id = {
            let mut s = self.shared.borrow_mut();
            s.next_msg += 1;
            s.next_msg
        };
        let msg = serde_json::json!({ "id": id, "method": method, "params": params }).to_string();
        if let Some(h) = self.host() {
            h.send_dev_tools_message(Some(msg.as_bytes()));
        }
        id
    }

    /// Run JS in the page and keep the value: the id to look for in
    /// `Shared::replies` (`result.result.value`).
    pub fn eval_reply(&self, expr: &str) -> i32 {
        self.devtools("Runtime.evaluate", serde_json::json!({ "expression": expr, "returnByValue": true }))
    }

    /// Take the reply for `id`, if it has arrived.
    pub fn take_reply(&self, id: i32) -> Option<serde_json::Value> {
        let mut s = self.shared.borrow_mut();
        let k = s.replies.iter().position(|(i, _)| *i == id)?;
        Some(s.replies.remove(k).1)
    }

    /// Run JS in the page (fire and forget).
    pub fn eval(&self, expr: &str) {
        self.devtools("Runtime.evaluate", serde_json::json!({ "expression": expr, "userGesture": true }));
    }

    pub fn prepare_pip(&self) {
        self.eval("window.__nus && __nus.reveal()");
        if let Some(h)=self.host() {h.invalidate(cef::PaintElementType::VIEW);h.send_external_begin_frame();}
    }

    pub fn video(&self) -> Option<Video> {
        self.shared.borrow().video.clone()
    }

    /// Save a file the page is showing, through the page's own session
    /// (cookies and all), into ~/Downloads via the download handler.
    pub fn download(&self, url: &str) {
        self.shared.borrow_mut().nus_download_at = Some(crate::clock::now());
        if let Some(h) = self.host() {
            h.start_download(Some(&url.into()));
        }
    }

    /// Native DevTools talks directly to this browser; no listening socket.
    pub fn open_devtools(&self, _device: wgpu::Device, _bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>, _scale: f32, _panel: &str) -> Option<DevToolsView> {
        let host = self.host()?;
        let info = WindowInfo {
            window_name: "nus Developer Tools".into(),
            #[cfg(target_os = "macos")]
            hidden: i32::from(std::env::var_os("NUS_SHOT").is_some()),
            bounds: cef::Rect { x: 100, y: 100, width: 1000, height: 700 },
            runtime_style: RuntimeStyle::CHROME,
            ..Default::default()
        };
        let mut client = NativeDevToolsClient::new(NativeDevToolsLife::new(()));
        host.show_dev_tools(Some(&info), Some(&mut client), Some(&BrowserSettings::default()), None);
        None
    }

    pub fn has_devtools(&self) -> bool { self.host().is_some_and(|h| h.has_dev_tools() != 0) }
    pub fn close_devtools(&self) { if let Some(host) = self.host() { host.close_dev_tools(); } }

    pub fn load(&self, url: &str) {
        if let Some(internal) = crate::interstitial::internal(url) {
            return self.internal(url, internal);
        }
        if let Some(f) = self.browser.as_ref().and_then(|b|b.main_frame()) {
            self.shared.borrow_mut().navigation(url);
            f.load_url(Some(&url.into()));
        }
        self.nudge();
    }

    /// A `nus://` address in a page that is already open.
    fn internal(&self, url: &str, internal: crate::interstitial::Internal) {
        use crate::interstitial::Internal;
        let Some(f) = self.browser.as_ref().and_then(|b| b.main_frame()) else { return };
        match internal {
            Internal::Show(page) if page.kind.native() => { self.shared.borrow_mut().overlay = Some(page); }
            Internal::Show(page) => {
                {
                    let mut s = self.shared.borrow_mut();
                    s.interstitial = Some(page);
                    s.inject = true;
                    s.failed_url = None;
                    s.url = url.into();
                    s.paints += 1;
                }
                f.load_url(Some(&"about:blank".into()));
            }
            other => run_debug(&f, &other),
        }
        self.nudge();
    }

    /// The app's part, each frame: host a page whose renderer is gone,
    /// swap in the Wi-Fi page when the network turned out to want a
    /// sign-in, and hand over what the transcript asked for.
    pub fn tend_interstitial(&self) -> Vec<String> {
        let (blank, portal) = {
            let mut s = self.shared.borrow_mut();
            let blank = std::mem::take(&mut s.blank);
            let portal = s.portal.as_ref().and_then(|p| p.lock().unwrap_or_else(|e| e.into_inner()).take());
            if portal.is_some() { s.portal = None; }
            (blank, portal)
        };
        if blank {
            if let Some(f) = self.browser.as_ref().and_then(|b| b.main_frame()) {
                f.load_url(Some(&"about:blank".into()));
            }
        }
        if let Some(Some(network)) = portal {
            let script = {
                let mut s = self.shared.borrow_mut();
                let Some(url) = s.interstitial.as_ref().filter(|p| matches!(p.kind, crate::interstitial::Kind::Unreachable | crate::interstitial::Kind::Cert)).map(|p| p.url.clone()) else { return std::mem::take(&mut s.interstitial_acts) };
                let page = crate::interstitial::Page::portal(&url, &network);
                let script = page.script();
                s.interstitial = Some(page);
                s.paints += 1;
                script
            };
            if let Some(f) = self.browser.as_ref().and_then(|b| b.main_frame()) {
                f.execute_java_script(Some(&script.as_str().into()), Some(&CefString::from(&f.url())), 0);
            }
        }
        std::mem::take(&mut self.shared.borrow_mut().interstitial_acts)
    }

    /// The overlay's answer to the page's question (or a sign-in): yes or
    /// no, and what was typed. The overlay goes either way.
    pub fn answer_dialog(&self, yes: bool, fields: &[crate::interstitial::Field]) {
        let dialog = {
            let mut s = self.shared.borrow_mut();
            if s.overlay.as_ref().is_some_and(|o| o.kind == crate::interstitial::Kind::Dialog) { s.overlay = None; }
            s.paints += 1;
            s.dialog.take()
        };
        // No borrow held: answering can call straight back into the handlers.
        match dialog {
            Some(Dialog::Js(cb)) => {
                let text = fields.first().map(|f| CefString::from(f.value.as_str()));
                cb.cont(yes as i32, text.as_ref());
            }
            Some(Dialog::Basic(origin)) if yes => {
                let user = fields.first().map(|f| f.value.as_str()).unwrap_or("");
                let pass = fields.get(1).map(|f| f.value.as_str()).unwrap_or("");
                remember_signin(origin, Some(basic(user, pass)));
                self.reload();
            }
            Some(Dialog::Basic(_)) => {
                // No sign-in: the page says why it has nothing to show.
                let can_back = self.can_go_back();
                let mut s = self.shared.borrow_mut();
                let url = s.failed_url.clone().unwrap_or_else(|| s.url.clone());
                s.interstitial = Some(crate::interstitial::Page::unreachable(&url, "ERR_INVALID_AUTH_CREDENTIALS", can_back));
                s.inject = true;
                s.paints += 1;
            }
            None => {}
        }
    }

    /// The hung renderer: keep waiting, or end it.
    pub fn answer_hung(&self, wait: bool) {
        let cb = {
            let mut s = self.shared.borrow_mut();
            if s.overlay.as_ref().is_some_and(|o| o.kind == crate::interstitial::Kind::Hung) { s.overlay = None; }
            s.paints += 1;
            if wait {
                // Asked again from now: another stretch before it shows.
                if let Some(p) = s.ping.as_mut() { p.1 = crate::clock::now(); }
            } else {
                s.stopping = true;
            }
            s.hung.take()
        };
        match (cb, wait) {
            (Some(cb), true) => cb.wait(),
            (Some(cb), false) => cb.terminate(),
            (None, true) => {}
            // The renderer's IO thread still answers when its page is stuck.
            (None, false) => { self.devtools("Page.crash", serde_json::json!({})); }
        }
    }

    /// While shown: ask the page something trivial every few seconds. A
    /// page whose main thread is stuck can't answer, and after
    /// `HUNG_AFTER` without one the hung transcript comes up over it.
    /// Chromium's own hang signal (on input) raises it too.
    pub fn watch(&self) {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(3);
        const HUNG_AFTER: std::time::Duration = std::time::Duration::from_secs(10);
        if self.browser.is_none() { return; }
        let ask = {
            let mut s = self.shared.borrow_mut();
            if s.interstitial.is_some() || s.suspended { return; }
            match s.ping {
                Some((_, at)) if crate::clock::since(at) >= HUNG_AFTER && s.overlay.is_none() => {
                    let url = s.url.clone();
                    s.overlay = Some(crate::interstitial::Page::hung(&url, crate::clock::since(at).as_secs()));
                    s.paints += 1;
                    false
                }
                Some(_) => false,
                None => s.answered.is_none_or(|a| crate::clock::since(a) >= EVERY),
            }
        };
        if ask {
            let id = self.devtools("Runtime.evaluate", serde_json::json!({ "expression": "1", "returnByValue": true }));
            self.shared.borrow_mut().ping = Some((id, crate::clock::now()));
        }
    }

    pub fn resized(&self, w: f32, h: f32) {
        {
            let mut s = self.shared.borrow_mut();
            if s.size == (w, h) {
                return;
            }
            s.size = (w, h);
        }
        if let Some(h) = self.host() {
            h.was_resized();
        }
    }

    /// CEF caches screen information separately from its logical view size.
    /// A monitor/DPI change must invalidate it even if the view size is equal.
    pub fn set_scale(&self, scale: f32) {
        let changed = {
            let mut s = self.shared.borrow_mut();
            if s.scale == scale { false } else { s.scale = scale; true }
        };
        if changed {
            if let Some(h) = self.host() {
                h.notify_screen_info_changed();
                h.was_resized();
            }
        }
    }

    pub fn zoom_percent(&self) -> u32 {
        let pending = self.shared.borrow().zoom_motion.map(|a| a.target() as f64);
        let level = pending.or_else(|| self.host().map(|h| h.zoom_level())).unwrap_or(0.0);
        (1.2_f64.powf(level) * 100.0).round() as u32
    }

    pub fn zoom_to(&self, percent: u32, duration: f32) {
        let Some(host) = self.host() else { return };
        let target = crate::sites::zoom_level(percent);
        if duration <= 0.0 {
            self.shared.borrow_mut().zoom_motion = None;
            host.set_zoom_level(target);
        } else {
            let mut motion = crate::anim::Anim::at(host.zoom_level() as f32);
            motion.go(target as f32, duration);
            self.shared.borrow_mut().zoom_motion = Some(motion);
        }
    }

    pub fn tick_zoom(&self, reduced: bool) -> bool {
        let Some(motion) = self.shared.borrow().zoom_motion else { return false };
        let done = reduced || !motion.active();
        // Drop the borrow before entering CEF; callbacks may read Shared.
        if done { self.shared.borrow_mut().zoom_motion = None; }
        if let Some(host) = self.host() {
            host.set_zoom_level(if done { motion.target() } else { motion.value() } as f64);
            host.send_external_begin_frame();
        }
        true
    }

    pub fn begin_frame(&self) {
        if let Some(h) = self.host() {
            h.send_external_begin_frame();
        }
    }

    pub fn mouse_move(&self, x: i32, y: i32, mods: u32, leave: bool) {
        // While the page's question (or a sign-in) stands, the page gets
        // nothing from you: every key, click, wheel and move stops here.
        if self.shared.borrow().dialog.is_some() { return; }
        if let Some(h) = self.host() {
            let ev = MouseEvent {
                x,
                y,
                modifiers: mods,
            };
            h.send_mouse_move_event(Some(&ev), leave as i32);
        }
    }

    pub fn mouse_click(&self, x: i32, y: i32, mods: u32, button: MouseButtonType, up: bool, count: i32) {
        // While the page's question (or a sign-in) stands, the page gets
        // nothing from you: every key, click, wheel and move stops here.
        if self.shared.borrow().dialog.is_some() { return; }
        if !up {
            self.shared.borrow_mut().gesture_at = Some(crate::clock::now());
        }
        if let Some(h) = self.host() {
            let ev = MouseEvent {
                x,
                y,
                modifiers: mods,
            };
            h.send_mouse_click_event(Some(&ev), button, up as i32, count);
        }
    }

    pub fn wheel(&self, x: i32, y: i32, mods: u32, dx: i32, dy: i32) {
        // While the page's question (or a sign-in) stands, the page gets
        // nothing from you: every key, click, wheel and move stops here.
        if self.shared.borrow().dialog.is_some() { return; }
        if let Some(h) = self.host() {
            let ev = MouseEvent {
                x,
                y,
                modifiers: mods,
            };
            h.send_mouse_wheel_event(Some(&ev), dx, dy);
        }
    }

    pub fn key(&self, ev: &KeyEvent) {
        // While the page's question (or a sign-in) stands, the page gets
        // nothing from you: every key, click, wheel and move stops here.
        if self.shared.borrow().dialog.is_some() { return; }
        self.shared.borrow_mut().gesture_at = Some(crate::clock::now());
        if let Some(h) = self.host() {
            h.send_key_event(Some(ev));
        }
    }

    pub fn focus(&self, on: bool) {
        if let Some(h) = self.host() {
            h.set_focus(on as i32);
        }
    }

    pub fn back(&self) {
        if let Some(b)=&self.browser { b.go_back(); }
        self.nudge();
    }

    pub fn forward(&self) {
        if let Some(b)=&self.browser { b.go_forward(); }
        self.nudge();
    }

    pub fn can_go_back(&self) -> bool {
        self.browser.as_ref().is_some_and(|b|b.can_go_back()!=0)
    }

    pub fn can_go_forward(&self) -> bool {
        self.browser.as_ref().is_some_and(|b|b.can_go_forward()!=0)
    }

    /// After a navigation: ask for a paint and a frame straight away, so
    /// the page that comes in (from the cache, or a new process after a
    /// cross-site hop) is drawn without waiting on the next tick.
    pub fn nudge(&self) {
        if let Some(h) = self.host() {
            h.was_resized();
            h.invalidate(cef::PaintElementType::VIEW);
            h.send_external_begin_frame();
        }
    }

    pub fn reload(&self) {
        if let Some(b)=&self.browser { b.reload(); }
        self.nudge();
    }

    /// A hard reload: the page again, past the cache.
    pub fn reload_ignore_cache(&self) {
        if let Some(b)=&self.browser { b.reload_ignore_cache(); }
        self.nudge();
    }

    /// Zoom by `steps` (Chrome-style 0.5 zoom-level steps); 0 resets.
    pub fn zoom(&self, steps: i32) {
        if let Some(h) = self.host() {
            if steps == 0 {
                h.set_zoom_level(0.0);
            } else {
                h.set_zoom_level(h.zoom_level() + steps as f64 * 0.5);
            }
        }
    }
}

// Immutable response bytes, with a synchronized cursor because CEF may move
// resource callbacks between its worker threads.
wrap_resource_handler! {
    struct DocumentResource { bytes: Arc<Vec<u8>>, cursor: Arc<std::sync::Mutex<usize>>, }
    impl ResourceHandler {
        fn open(&self, _request:Option<&mut Request>, handle:Option<&mut ::std::os::raw::c_int>, _callback:Option<&mut Callback>)->::std::os::raw::c_int {if let Some(h)=handle{*h=1;}1}
        fn response_headers(&self, response:Option<&mut Response>, length:Option<&mut i64>, _redirect:Option<&mut CefString>) {
            if let Some(r)=response{r.set_status(200);r.set_mime_type(Some(&"text/html".into()));r.set_charset(Some(&"utf-8".into()));}
            if let Some(l)=length{*l=self.bytes.len() as i64;}
        }
        fn read(&self,out:*mut u8,count: ::std::os::raw::c_int,read:Option<&mut ::std::os::raw::c_int>,_callback:Option<&mut ResourceReadCallback>)->::std::os::raw::c_int {
            let Some(read)=read else{return 0;};*read=0;if out.is_null()||count<=0{return 0;}
            let Ok(mut at)=self.cursor.lock() else{return 0;};let n=(count as usize).min(self.bytes.len().saturating_sub(*at));
            if n==0{return 0;}unsafe{std::ptr::copy_nonoverlapping(self.bytes.as_ptr().add(*at),out,n);}
            *at+=n;*read=n as i32;1
        }
        fn skip(&self,count:i64,skipped:Option<&mut i64>,_callback:Option<&mut ResourceSkipCallback>)->::std::os::raw::c_int {
            let Some(skipped)=skipped else{return 0;};*skipped=0;if count<0{return 0;}let Ok(mut at)=self.cursor.lock()else{return 0;};let n=(count as usize).min(self.bytes.len().saturating_sub(*at));*at+=n;*skipped=n as i64;i32::from(n>0)
        }
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn late_original_address_cannot_replace_a_committed_failed_redirect() {
        let mut s=Shared::default();
        s.navigation("https://original.test/");
        s.redirect("https://original.test/","https://failed.test/");
        s.failed("https://original.test/",false);
        s.address("https://original.test/");
        assert_eq!(s.url,"https://failed.test/");
        s.failed("https://final.test/",true);
        s.address("https://original.test/");
        assert_eq!(s.url,"https://final.test/");
        s.navigation("https://recovery.test/");
        s.address("https://recovery.test/ok");
        assert_eq!(s.url,"https://recovery.test/ok");
        assert!(s.failed_url.is_none());
    }
    #[test]
    fn pending_and_error_documents_keep_the_requested_address() {
        let mut s=Shared::default();s.address("https://example.com/");
        s.navigation("http://127.0.0.1:1/missing");
        assert_eq!(s.url,"http://127.0.0.1:1/missing");assert!(s.loading);assert_eq!(s.progress,0.0);
        s.address("chrome-error://chromewebdata/");assert_eq!(s.url,"http://127.0.0.1:1/missing");
        s.address("https://example.org/redirected");assert_eq!(s.url,"https://example.org/redirected");
    }
}
