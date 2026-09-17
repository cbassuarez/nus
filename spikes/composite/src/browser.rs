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
    pub created: Created,
    /// Results of CDP calls made with `devtools`, by message id; the app
    /// drains the ones it asked for.
    pub replies: Vec<(i32, serde_json::Value)>,
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
#[derive(Clone, Debug)]
pub struct Download {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub url: String,
    pub received: i64,
    pub total: i64,
    pub done: bool,
    pub cancelled: bool,
}

pub static DOWNLOADS: std::sync::Mutex<Vec<Download>> = std::sync::Mutex::new(Vec::new());

/// Content blocking: on/off and the hosts to refuse. Built-in list plus
/// profile/blocklist.txt (one host per line; `||host^` lines work too).
pub static BLOCKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
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
pub fn downloads_dir() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).map(std::path::PathBuf::from);
    match home {
        Ok(h) if h.join("Downloads").is_dir() => h.join("Downloads"),
        _ => std::env::current_dir().unwrap_or_default().join("profile").join("downloads"),
    }
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
        Created(std::time::Instant::now())
    }
}
impl Created {
    pub fn elapsed(&self) -> std::time::Duration {
        self.0.elapsed()
    }
}

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

pub const DEVTOOLS_PORT: u16 = 9229;

pub type SharedRef = StdRc<RefCell<Shared>>;

/// Injected into every document: tracks the largest playing <video>, reports
/// it through the `nusVideo` binding, and exposes transport on `__nus`.
pub const VIDEO_JS: &str = r#"(() => {
  if (window.__nus) return;
  const st = { best: null };
  function pick() {
    let best = null, area = 0;
    for (const v of document.querySelectorAll('video')) {
      const r = v.getBoundingClientRect();
      const a = r.width * r.height;
      if (a > area && v.readyState > 0 && r.width > 80) { area = a; best = v; }
    }
    return best;
  }
  function report() {
    const v = pick(); st.best = v;
    let p = null;
    if (v) {
      const r = v.getBoundingClientRect();
      p = { x: r.left, y: r.top, w: r.width, h: r.height, vw: innerWidth, vh: innerHeight,
            paused: v.paused, ended: v.ended, muted: v.muted, t: v.currentTime, dur: v.duration || 0 };
    }
    if (window.nusVideo) window.nusVideo(JSON.stringify(p));
  }
  const V = () => st.best;
  window.__nus = {
    report,
    seek(d) { const v = V(); if (v) v.currentTime = Math.max(0, Math.min(v.duration || 1e9, v.currentTime + d)); },
    toggle() { const v = V(); if (v) { if (v.paused) v.play(); else v.pause(); } },
    vol(d) { const v = V(); if (v) v.volume = Math.max(0, Math.min(1, v.volume + d)); },
    mute() { const v = V(); if (v) v.muted = !v.muted; },
    step(f) { const v = V(); if (v) { v.pause(); v.currentTime += f / 30; } },
    reveal() { const v = V(); if (v) v.scrollIntoView({ block: 'center', inline: 'center' }); },
  };
  setInterval(report, 100);
})();"#;

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
            cl.append_switch(Some(&"noerrdialogs".into()));
            cl.append_switch(Some(&"hide-crash-restore-bubble".into()));
            cl.append_switch(Some(&"use-mock-keychain".into()));
            if std::env::var_os("NUS_AUTOPLAY").is_some() {
                cl.append_switch_with_value(Some(&"autoplay-policy".into()), Some(&"no-user-gesture-required".into()));
            }
            // Loopback-only; our DevTools pane is the frontend attached through it.
            cl.append_switch_with_value(Some(&"remote-debugging-port".into()), Some(&DEVTOOLS_PORT.to_string().as_str().into()));
            cl.append_switch_with_value(Some(&"remote-allow-origins".into()), Some(&format!("http://127.0.0.1:{DEVTOOLS_PORT},devtools://devtools").as_str().into()));
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

        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let main = frame.map(|f| f.is_main() != 0).unwrap_or(true);
            if let (true, Some(u)) = (main, url) {
                let mut s = self.d.shared.borrow_mut();
                tracing::info!("address {} +{}ms", u, s.created.elapsed().as_millis());
                s.url = u.to_string();
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
                tracing::info!("loaded {} +{}ms", s.url, s.created.elapsed().as_millis());
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
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(result) {
                if let Some(id) = v.pointer("/targetInfo/targetId").and_then(|t| t.as_str()) {
                    self.o.shared.borrow_mut().target_id = Some(id.to_string());
                }
                let mut s = self.o.shared.borrow_mut();
                if s.replies.len() > 32 {
                    s.replies.remove(0);
                }
                s.replies.push((message_id, v));
            }
        }

        fn on_dev_tools_event(&self, _browser: Option<&mut Browser>, method: Option<&CefString>, params: Option<&[u8]>) {
            let Some(method) = method else { return };
            if method.to_string() != "Runtime.bindingCalled" {
                return;
            }
            let Some(params) = params else { return };
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(params) else { return };
            if v.get("name").and_then(|n| n.as_str()) != Some("nusVideo") {
                return;
            }
            let payload = v.get("payload").and_then(|p| p.as_str()).unwrap_or("null");
            let video = serde_json::from_str::<serde_json::Value>(payload).ok().and_then(|p| {
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
            self.o.shared.borrow_mut().video = video;
        }
    }
}

wrap_life_span_handler! {
    pub struct LifeBuilder {
        d: Display,
    }

    impl LifeSpanHandler {
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
    }

    impl DownloadHandler {
        fn on_before_download(&self, _browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, suggested_name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> ::std::os::raw::c_int {
            let Some(item) = download_item else { return 0 };
            let name = suggested_name.map(|s| s.to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "download".into());
            // ~/Downloads/name, numbered when taken.
            let dir = downloads_dir();
            let _ = std::fs::create_dir_all(&dir);
            let mut path = dir.join(&name);
            let mut n = 1;
            while path.exists() {
                n += 1;
                let (stem, ext) = match name.rsplit_once('.') { Some((s, e)) => (s.to_string(), format!(".{e}")), None => (name.clone(), String::new()) };
                path = dir.join(format!("{stem} ({n}){ext}"));
            }
            let d = Download { id: item.id(), name: name.clone(), path: path.to_string_lossy().to_string(), url: CefString::from(&item.url()).to_string(), received: 0, total: item.total_bytes(), done: false, cancelled: false };
            DOWNLOADS.lock().unwrap().push(d);
            if let Some(cb) = callback {
                cb.cont(Some(&path.to_string_lossy().as_ref().into()), 0);
            }
            self.display.shared.borrow_mut().paints += 1;
            1
        }

        fn on_download_updated(&self, _browser: Option<&mut Browser>, download_item: Option<&mut DownloadItem>, _callback: Option<&mut DownloadItemCallback>) {
            let Some(item) = download_item else { return };
            let id = item.id();
            let mut list = DOWNLOADS.lock().unwrap();
            if let Some(d) = list.iter_mut().find(|d| d.id == id) {
                d.received = item.received_bytes();
                d.total = item.total_bytes();
                d.done = item.is_complete() != 0;
                d.cancelled = item.is_canceled() != 0;
                let p = CefString::from(&item.full_path()).to_string();
                if !p.is_empty() {
                    d.path = p;
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
            let Some(cb) = callback else { return 0 };
            let origin = requesting_origin.map(|s| s.to_string()).unwrap_or_default();
            let video = requested_permissions & MediaAccessPermissionTypes::DEVICE_VIDEO_CAPTURE.get_raw() as u32 != 0;
            let audio = requested_permissions & MediaAccessPermissionTypes::DEVICE_AUDIO_CAPTURE.get_raw() as u32 != 0;
            let what = match (video, audio) { (true, true) => "camera and microphone", (true, false) => "camera", (false, true) => "microphone", _ => "screen capture" }.to_string();
            let mut s = self.display.shared.borrow_mut();
            s.permission = Some(PermissionAsk { origin, what, kind: AskKind::Media(cb.clone(), requested_permissions) });
            s.paints += 1;
            1
        }

        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _prompt_id: u64, requesting_origin: Option<&CefString>, requested_permissions: u32, callback: Option<&mut PermissionPromptCallback>) -> ::std::os::raw::c_int {
            let Some(cb) = callback else { return 0 };
            let origin = requesting_origin.map(|s| s.to_string()).unwrap_or_default();
            let mut s = self.display.shared.borrow_mut();
            s.permission = Some(PermissionAsk { origin, what: permission_words(requested_permissions), kind: AskKind::Prompt(cb.clone()) });
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
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _callback: Option<&mut Callback>) -> ReturnValue {
            let Some(req) = request else { return ReturnValue::CONTINUE };
            let url = CefString::from(&req.url()).to_string();
            if blocked(&url) {
                let mut s = self.display.shared.borrow_mut();
                s.blocked += 1;
                return ReturnValue::CANCEL;
            }
            ReturnValue::CONTINUE
        }
    }
}

wrap_request_handler! {
    pub struct RequestBuilder {
        display: Display,
    }

    impl RequestHandler {
        fn resource_request_handler(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _request: Option<&mut Request>, is_navigation: ::std::os::raw::c_int, _is_download: ::std::os::raw::c_int, _request_initiator: Option<&CefString>, _disable_default_handling: Option<&mut ::std::os::raw::c_int>) -> Option<ResourceRequestHandler> {
            // Never block the navigation itself, only what the page pulls in.
            if is_navigation != 0 || !BLOCKING.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            Some(BlockBuilder::new(self.display.clone()))
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
    }
}

pub struct BrowserTab {
    pub browser: Browser,
    pub shared: SharedRef,
    _observer: Option<Registration>,
}

impl BrowserTab {
    pub fn create(
        url: &str,
        shared: SharedRef,
        device: wgpu::Device,
        bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>,
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
            DownloadBuilder::new(Display { shared: shared.clone() }),
            PermissionBuilder::new(Display { shared: shared.clone() }),
            RequestBuilder::new(Display { shared: shared.clone() }),
        );
        // The global context: one cookie jar and cache for the Space. (v1 gives
        // each Space its own, with cache_path under the profile.)
        let mut context = request_context_get_global_context();
        let t0 = std::time::Instant::now();
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url.into()),
            Some(&settings),
            None,
            context.as_mut(),
        )?;
        tracing::info!("create_browser_sync {url} took {}ms", t0.elapsed().as_millis());
        let mut observer = ObserverBuilder::new(Observer { shared: shared.clone() });
        let registration = browser.host().and_then(|h| h.add_dev_tools_message_observer(Some(&mut observer)));
        let tab = BrowserTab { browser, shared, _observer: registration };
        tab.devtools("Runtime.enable", serde_json::json!({}));
        tab.devtools("Page.enable", serde_json::json!({}));
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

    pub fn video(&self) -> Option<Video> {
        self.shared.borrow().video.clone()
    }

    /// Open the DevTools frontend for this page as a browser we composite.
    /// (CEF refuses windowless DevTools windows in the Chrome runtime.)
    pub fn open_devtools(&self, device: wgpu::Device, bind_texture: StdRc<dyn Fn(&wgpu::Texture) -> Arc<wgpu::BindGroup>>, scale: f32, panel: &str) -> Option<DevToolsView> {
        let target = self.shared.borrow().target_id.clone()?;
        let url = format!("http://127.0.0.1:{DEVTOOLS_PORT}/devtools/inspector.html?ws=127.0.0.1:{DEVTOOLS_PORT}/devtools/page/{target}&panel={panel}");
        let shared: SharedRef = StdRc::new(RefCell::new(Shared { scale, size: (400.0, 300.0), ..Default::default() }));
        BrowserTab::create(&url, shared, device, bind_texture)
    }

    pub fn close_devtools(&self) {}

    pub fn load(&self, url: &str) {
        if let Some(f) = self.browser.main_frame() {
            f.load_url(Some(&url.into()));
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
    }

    pub fn forward(&self) {
        self.browser.go_forward();
    }

    pub fn reload(&self) {
        self.browser.reload();
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
