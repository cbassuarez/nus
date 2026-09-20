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
    /// Latest imported paint, bound for the quad pipeline.
    pub bind: Option<Arc<wgpu::BindGroup>>,
    pub title: String,
    pub url: String,
    pub loading: bool,
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
    pub said: Option<String>,
    /// What the page said and fetched: console calls, exceptions, requests
    /// and responses, as `{ "kind", "at", … }`, newest last, capped.
    pub log: Vec<serde_json::Value>,
    /// The page's favicon, straight-alpha BGRA, once downloaded.
    pub favicon: Option<Favicon>,
    pub favicon_url: String,
    /// Find in page: (matches, active ordinal), from the find handler.
    pub find: Option<(i32, i32)>,
    /// Requests refused by content blocking on this page.
    pub blocked: u32,
    /// A permission the page asked for, waiting on the band.
    pub permission: Option<PermissionAsk>,
    /// A <select> (or other popup widget): where it is and its texture.
    pub select: SelectPopup,
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

/// Would this URL's host be refused? Any parent domain on the list counts.
pub fn blocked(url: &str) -> bool {
    if !BLOCKING.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let host = url.split("//").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("").split('@').next_back().unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
    if host.is_empty() {
        return false;
    }
    let list = blocklist().read().unwrap();
    let mut h = host.as_str();
    loop {
        if list.contains(h) {
            return true;
        }
        match h.find('.') {
            Some(i) => h = &h[i + 1..],
            None => return false,
        }
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
                // A new site: the old favicon must not linger on it.
                if crate::sites::host_of(&u) != crate::sites::host_of(&s.url) {
                    s.favicon = None;
                    s.favicon_url.clear();
                }
                s.url = u;
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

        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            let mut s = self.d.shared.borrow_mut();
            if progress >= 1.0 && s.loading {
                if !crate::private::enabled() { tracing::info!("loaded {} +{}ms", s.url, s.created.elapsed().as_millis()); }
            }
            s.loading = progress < 1.0;
            s.progress = progress;
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
            let _ = message_id;
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
                    paused: b("paused"),
                    ended: b("ended"),
                    muted: b("muted"),
                    t: f("t"),
                    dur: f("dur"),
                })
            });
            let mut s = self.o.shared.borrow_mut();
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
                    if let Some(h) = browser.and_then(|b| b.host()) {
                        h.start_download(Some(&src.as_str().into()));
                    }
                }
                CMD_COPY_MEDIA => { copy(&src); sh.said = Some(format!("COPIED · {}", crate::app::fit_cmd(&src, 60))); }
                CMD_OPEN_MEDIA => sh.opens.push((src, false)),
                CMD_OPEN_LINK => sh.opens.push((link, false)),
                CMD_OPEN_LINK_BESIDE => sh.opens.push((link, true)),
                CMD_COPY_LINK => { copy(&link); sh.said = Some(format!("COPIED · {}", crate::app::fit_cmd(&link, 60))); }
                CMD_COPY_PAGE => { copy(&page); sh.said = Some(format!("COPIED · {}", crate::app::fit_cmd(&page, 60))); }
                CMD_PIP => sh.said = Some("PIP".into()),
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
            if let Some(b) = browser { LIVE_BROWSERS.with(|v| { v.borrow_mut().insert(b.identifier()); }); }
        }
        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser { LIVE_BROWSERS.with(|v| { v.borrow_mut().remove(&b.identifier()); }); }
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
            if let Some(url) = target_url {
                self.d.shared.borrow_mut().popup = Some(url.to_string());
            }
            1
        }
    }
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
            let title=self.display.shared.borrow().title.clone();
            let name=crate::downloads::filename(&original,&title,crate::downloads::rename_mode());
            let dir=downloads_dir();if std::fs::create_dir_all(&dir).is_err(){return 0;}
            let path={
                let mut rows=DOWNLOADS.lock().unwrap();
                let path=crate::downloads::available_path(&dir,&name,&rows);
                let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                let key=(now.as_micros() as u64).max(rows.iter().map(|d|d.key).max().unwrap_or(0)+1);
                rows.push(Download{key,id:item.id(),name:path.file_name().unwrap().to_string_lossy().into(),original:crate::downloads::safe_name(&original),path:path.to_string_lossy().into(),url:CefString::from(&item.url()).to_string(),container:self.container.clone(),source_url:_browser.and_then(|b|b.main_frame()).map(|f|CefString::from(&f.url()).to_string()).unwrap_or_default(),title,total:item.total_bytes(),started:now.as_secs(),live:true,..Default::default()});
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
                if before!=(d.done,d.cancelled,d.interrupted,d.paused){crate::downloads::save(&list);}
                crate::downloads::changed();
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
            let Some(cb) = callback else { return 0 };
            let origin = requesting_origin.map(|s| s.to_string()).unwrap_or_default();
            let video = requested_permissions & MediaAccessPermissionTypes::DEVICE_VIDEO_CAPTURE.get_raw() as u32 != 0;
            let audio = requested_permissions & MediaAccessPermissionTypes::DEVICE_AUDIO_CAPTURE.get_raw() as u32 != 0;
            let what = match (video, audio) { (true, true) => "camera and microphone", (true, false) => "camera", (false, true) => "microphone", _ => "screen capture" }.to_string();
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
        display: Display,
        navigation: bool,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(&self, browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _callback: Option<&mut Callback>) -> ReturnValue {
            let Some(req) = request else { return ReturnValue::CONTINUE };
            // BROWSER · PRIVACY SIGNAL: Global Privacy Control and Do Not
            // Track on every request, navigations included.
            if PRIVACY_SIGNAL.load(std::sync::atomic::Ordering::Relaxed) {
                req.set_header_by_name(Some(&"Sec-GPC".into()), Some(&"1".into()), 1);
                req.set_header_by_name(Some(&"DNT".into()), Some(&"1".into()), 1);
            }
            // Never block the navigation itself, only what the page pulls in.
            if self.navigation {
                return ReturnValue::CONTINUE;
            }
            let page = browser.and_then(|b| b.main_frame()).map(|f| CefString::from(&f.url()).to_string()).unwrap_or_default();
            if !crate::sites::prefs(&crate::sites::host_of(&page)).blocking {
                return ReturnValue::CONTINUE;
            }
            let url = CefString::from(&req.url()).to_string();
            if blocked(&url) {
                let mut s = self.display.shared.borrow_mut();
                s.blocked += 1;
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
    }

    impl RequestHandler {
        fn resource_request_handler(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, is_navigation: ::std::os::raw::c_int, _is_download: ::std::os::raw::c_int, _request_initiator: Option<&CefString>, _disable_default_handling: Option<&mut ::std::os::raw::c_int>) -> Option<ResourceRequestHandler> {
            Some(BlockBuilder::new(self.display.clone(), is_navigation != 0))
        }

        /// One certificate is trusted without asking: the one this window is
        /// serving to the phone right now, matched by its exact bytes. The
        /// phone has to decide for itself, but nus knows its own key, so
        /// opening the phone's page in a tab here is not a lesson in clicking
        /// through warnings. Every other certificate error is Chromium's to
        /// show, including anything at the same address after the phone stops.
        fn on_certificate_error(&self, _browser: Option<&mut Browser>, _cert_error: Errorcode, request_url: Option<&CefString>, ssl_info: Option<&mut Sslinfo>, callback: Option<&mut Callback>) -> ::std::os::raw::c_int {
            let Some(phone) = crate::phone::current() else { return 0 };
            let url = request_url.map(CefString::to_string).unwrap_or_default();
            let Ok(url) = url::Url::parse(&url) else { return 0 };
            let host = url.host_str().unwrap_or_default();
            let ours = url.port() == Some(phone.port)
                && (host == phone.host || host == "127.0.0.1" || host == "localhost" || host == "[::1]");
            if !ours {
                return 0;
            }
            let Some(der) = ssl_info.and_then(|i| i.x509_certificate()).and_then(|c| c.derencoded()) else { return 0 };
            let mut bytes = vec![0u8; der.size()];
            if der.data(Some(&mut bytes), 0) != bytes.len() {
                return 0;
            }
            if !crate::phone::is_session_certificate(&phone, &bytes) {
                return 0;
            }
            match callback {
                Some(c) => {
                    c.cont();
                    1
                }
                None => 0,
            }
        }
    }
}

wrap_client! {
    pub struct ClientBuilder {
        render: RenderHandler,
        display: DisplayHandler,
        life: LifeSpanHandler,
        find: FindHandler,
        download: DownloadHandler,
        permission: PermissionHandler,
        request: RequestHandler,
        menu: ContextMenuHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
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

thread_local! { static LIVE_BROWSERS: RefCell<std::collections::HashSet<i32>> = RefCell::new(Default::default()); }
pub fn live_count() -> usize { LIVE_BROWSERS.with(|b| b.borrow().len()) }

pub struct BrowserTab {
    pub browser: Browser,
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
        if let Some(host) = self.browser.host() { host.close_dev_tools(); host.close_browser(1); }
    }
}

impl BrowserTab {
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
        let osr = Osr {
            shared: shared.clone(),
            device,
            bind_texture,
        };
        let mut client = ClientBuilder::new(
            RenderBuilder::new(osr),
            DisplayBuilder::new(Display {
                shared: shared.clone(),
            }),
            LifeBuilder::new(Display { shared: shared.clone() }),
            FindBuilder::new(Display { shared: shared.clone() }),
            DownloadBuilder::new(Display { shared: shared.clone() },container.to_string()),
            PermissionBuilder::new(Display { shared: shared.clone() }),
            RequestBuilder::new(Display { shared: shared.clone() }),
            MenuBuilder::new(Display { shared: shared.clone() }),
        );
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
        let mut observer = ObserverBuilder::new(Observer { shared: shared.clone() });
        let registration = browser.host().and_then(|h| h.add_dev_tools_message_observer(Some(&mut observer)));
        let tab = BrowserTab { browser, shared, _observer: registration };
        tab.devtools("Runtime.enable", serde_json::json!({}));
        tab.devtools("Page.enable", serde_json::json!({}));
        tab.devtools("Network.enable", serde_json::json!({}));
        tab.devtools("Runtime.addBinding", serde_json::json!({ "name": "nusVideo" }));
        tab.devtools("Page.addScriptToEvaluateOnNewDocument", serde_json::json!({ "source": VIDEO_JS }));
        tab.devtools("Runtime.evaluate", serde_json::json!({ "expression": VIDEO_JS }));
        let id = tab.devtools("Target.getTargetInfo", serde_json::json!({}));
        tab.shared.borrow_mut().target_msg = id;
        Some(tab)
    }

    pub fn host(&self) -> Option<BrowserHost> {
        self.browser.host()
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
        if let Some(f) = self.browser.main_frame() {
            f.load_url(Some(&url.into()));
        }
        self.nudge();
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

    pub fn begin_frame(&self) {
        if let Some(h) = self.host() {
            h.send_external_begin_frame();
        }
    }

    pub fn mouse_move(&self, x: i32, y: i32, mods: u32, leave: bool) {
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
        self.browser.go_back();
        self.nudge();
    }

    pub fn forward(&self) {
        self.browser.go_forward();
        self.nudge();
    }

    pub fn can_go_back(&self) -> bool {
        self.browser.can_go_back() != 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.browser.can_go_forward() != 0
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
        self.browser.reload();
        self.nudge();
    }

    /// A hard reload: the page again, past the cache.
    pub fn reload_ignore_cache(&self) {
        self.browser.reload_ignore_cache();
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
