//! Protected video through the system's WebKit. Netflix, Prime and the
//! rest play only under a DRM the browser is licensed for: Chromium's
//! Widevine wants a VMP signature a custom build can't get for free, while
//! WebKit's FairPlay ships with macOS. A page on one of these hosts is
//! shown in a WKWebView laid over the pane, signed in with the pane's own
//! cookies, and the Chromium page underneath goes to about:blank.
//!
//! Its video is nus's like any other page's. The tracker Chromium's pages
//! run (assets/video.js) runs in WebKit's page too and is asked what it
//! saw a few times a second, so the tab knows it is playing and picture in
//! picture has the time, the ratio and the transport. The picture itself
//! can't be lent as a texture — that is what the DRM is for — so for
//! picture in picture the view goes into nus's window, under nus's
//! controls, and comes back after. The system's own picture in picture is
//! not used: a page that asks for it is given nus's.

/// Hosts whose video is protected, and where WebKit takes over.
const HOSTS: &[&str] = &[
    "netflix.com",
    "primevideo.com",
    "disneyplus.com",
    "max.com",
    "hbomax.com",
    "hulu.com",
    "tv.apple.com",
    "peacocktv.com",
    "paramountplus.com",
];

/// Whether this address is a protected-video page WebKit should show.
pub fn protected(url: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let host = host(url);
    let under = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    // For checks: more hosts to treat as protected (plain http too), comma-separated.
    let also = std::env::var("NUS_WEBKIT_ALSO").unwrap_or_default();
    if also.split(',').any(|d| !d.is_empty() && under(d)) {
        return true;
    }
    if !url.starts_with("https://") {
        return false;
    }
    if HOSTS.iter().any(|d| under(d)) {
        return true;
    }
    // Prime Video on amazon.<tld>: only its video pages.
    let amazon = host == "amazon.com" || host.starts_with("amazon.") || host.starts_with("www.amazon.");
    let path = url.splitn(4, '/').nth(3).unwrap_or("");
    amazon && (path.starts_with("gp/video") || path.starts_with("Amazon-Video"))
}

fn host(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or("");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// The addresses whose cookies sign this page in: the page itself, and
/// the service's other sign-in hosts (Prime's session lives on amazon).
pub fn cookie_urls(url: &str) -> Vec<String> {
    let host = host(url);
    let mut urls = vec![url.to_string(), format!("https://{host}/")];
    let apex = HOSTS.iter().find(|d| host == **d || host.ends_with(&format!(".{d}")));
    if let Some(d) = apex {
        urls.push(format!("https://{d}/"));
        urls.push(format!("https://www.{d}/"));
    }
    if host.contains("amazon.") || host.ends_with("primevideo.com") {
        let amazon = if host.contains("amazon.") { host.trim_start_matches("www.").to_string() } else { "amazon.com".into() };
        urls.push(format!("https://www.{amazon}/"));
        urls.push(format!("https://www.{amazon}/gp/video/"));
        urls.push("https://www.primevideo.com/".into());
    }
    urls.dedup();
    urls
}

/// One cookie as DevTools' Network.getCookies gives it, as a Set-Cookie line.
pub fn set_cookie_line(c: &serde_json::Value, now: f64) -> Option<(String, String)> {
    let name = c.get("name")?.as_str()?;
    let value = c.get("value")?.as_str()?;
    let domain = c.get("domain")?.as_str()?;
    let path = c.get("path").and_then(|p| p.as_str()).unwrap_or("/");
    let mut line = format!("{name}={value}; Path={path}");
    // A host-only cookie has no leading dot and no Domain attribute.
    if domain.starts_with('.') {
        line.push_str(&format!("; Domain={domain}"));
    }
    let session = c.get("session").and_then(|s| s.as_bool()).unwrap_or(true);
    let expires = c.get("expires").and_then(|e| e.as_f64()).unwrap_or(-1.0);
    if !session && expires > 0.0 {
        // Max-Age, not Expires: no comma in the line, no date to format.
        line.push_str(&format!("; Max-Age={}", (expires - now).max(1.0) as i64));
    }
    if c.get("secure").and_then(|s| s.as_bool()).unwrap_or(false) {
        line.push_str("; Secure");
    }
    if c.get("httpOnly").and_then(|s| s.as_bool()).unwrap_or(false) {
        line.push_str("; HttpOnly");
    }
    if let Some(s) = c.get("sameSite").and_then(|s| s.as_str()) {
        line.push_str(&format!("; SameSite={s}"));
    }
    let url = format!("https://{}{}", domain.trim_start_matches('.'), path);
    Some((line, url))
}

/// What WebKit's page runs before its own scripts: the tracker, keeping
/// its report in `__nusLast` for `poll` (Chromium's pages post theirs to a
/// binding instead), then what WebKit's pages need besides. Netflix's
/// player stops with an error when `currentTime` is set under it, so its
/// seeks (milliseconds), and its play and pause, go through its own API. A request for picture in
/// picture, by the page's own button or WebKit's menu, is handed to nus,
/// and `__nusPip` shows only the video, and the captions the services
/// draw themselves, while the page is in nus's window.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const TRACKER: &str = concat!(
    "window.nusVideo=window.nusVideo||(p=>{window.__nusLast=p});\n",
    include_str!("../assets/video.js"),
    "\n",
    r#"(()=>{const n=window.__nus;if(!n||n.__webkit)return;n.__webkit=true;
const nf=()=>{try{const p=window.netflix.appContext.state.playerApp.getAPI().videoPlayer;const ids=p.getAllPlayerSessionIds();return p.getVideoPlayerBySessionId(ids.find(i=>i.startsWith('watch'))||ids[0])||null}catch(_){return null}};
const seek=n.seek,seekTo=n.seekTo,step=n.step,toggle=n.toggle;
n.toggle=()=>{const p=nf();if(!p)return toggle();if(p.isPaused())p.play();else p.pause()};
n.seek=d=>{const p=nf();if(!p)return seek(d);if(Number.isFinite(d))p.seek(Math.max(0,p.getCurrentTime()+d*1000));n.report()};
n.seekTo=f=>{const p=nf();if(!p)return seekTo(f);const d=p.getDuration();if(d>0&&Number.isFinite(f))p.seek(Math.max(0,Math.min(d,f*d)));n.report()};
n.step=f=>{const p=nf();if(!p)return step(f);p.pause();p.seek(Math.max(0,p.getCurrentTime()+f*1000/30))};
const ask=()=>{window.__nusWantPip=1};
const proto=HTMLVideoElement.prototype,mode=proto.webkitSetPresentationMode;
proto.requestPictureInPicture=function(){ask();return Promise.reject(new DOMException('Picture in picture opens in nus','NotAllowedError'))};
if(mode)proto.webkitSetPresentationMode=function(m){if(m==='picture-in-picture'){ask();return}return mode.call(this,m)};
const back=e=>{const v=e.target;if(!(v instanceof HTMLVideoElement))return;
if(mode&&v.webkitPresentationMode==='picture-in-picture'){mode.call(v,'inline');ask()}
else if(document.pictureInPictureElement===v){document.exitPictureInPicture().catch(()=>{});ask()}};
document.addEventListener('webkitpresentationmodechanged',back,true);
document.addEventListener('enterpictureinpicture',back,true);
const css='html.__nus-pip,html.__nus-pip body{background:#000!important;overflow:hidden!important}html.__nus-pip body *{visibility:hidden!important}html.__nus-pip video,html.__nus-pip .player-timedtext,html.__nus-pip .player-timedtext *,html.__nus-pip .atvwebplayersdk-captions-overlay,html.__nus-pip .atvwebplayersdk-captions-overlay *{visibility:visible!important}';
window.__nusPip=on=>{const d=document.documentElement;if(!d)return;
if(on&&!document.getElementById('__nus-pip-style')){const s=document.createElement('style');s.id='__nus-pip-style';s.textContent=css;(document.head||d).appendChild(s)}
d.classList.toggle('__nus-pip',!!on);n.report()};
})();"#,
);

/// What `poll` asks the page: the tracker's last report, and whether the
/// page asked for picture in picture since (asking clears it).
macro_rules! poll_js {
    () => {
        "(()=>{const w=!!window.__nusWantPip;window.__nusWantPip=0;return JSON.stringify({r:window.__nusLast||'',pip:w})})()"
    };
}
/// …and first, the page shows only its video while it is in nus's window
/// and all of itself while it is home: a reload in between would lose it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const POLL_IN_PIP: &str = concat!("(()=>{const d=document.documentElement;if(d&&window.__nusPip&&!d.classList.contains('__nus-pip'))__nusPip(true)})();", poll_js!());
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const POLL_AT_HOME: &str = concat!("(()=>{const d=document.documentElement;if(d&&window.__nusPip&&d.classList.contains('__nus-pip'))__nusPip(false)})();", poll_js!());

/// `poll`'s answer: the tracker's report (as Chromium's binding would have
/// had it) and whether the page asked for picture in picture.
pub fn read_poll(answer: &str) -> Option<(String, bool)> {
    let v: serde_json::Value = serde_json::from_str(answer).ok()?;
    let report = v.get("r")?.as_str()?.to_string();
    Some((report, v.get("pip").and_then(|p| p.as_bool()).unwrap_or(false)))
}

pub use imp::*;

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSResponder, NSTrackingArea, NSView};
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSHTTPCookie, NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest};
    use objc2_web_kit::{WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    thread_local! {
        /// The main window's content view: WebKit pages are its subviews.
        static HOST: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    }

    /// How often the tracker is asked what it saw, and how long an answer
    /// may take before it is asked again.
    const POLL: Duration = Duration::from_millis(250);
    const POLL_LATE: Duration = Duration::from_secs(2);

    define_class!(
        // SAFETY: NSView asks nothing of a subclass but the main thread; this
        // one adds no instance variables and no Drop.
        #[unsafe(super(NSView, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "NusPipShelter"]
        struct Shelter;

        impl Shelter {
            /// Nothing under here takes a click or a wheel: over the
            /// picture they are the window's, for its controls and its drag.
            #[unsafe(method(hitTest:))]
            fn hit_test(&self, _point: NSPoint) -> *mut NSView {
                std::ptr::null_mut()
            }
        }
    );

    /// A winit window's content view.
    fn view_of(window: &winit::window::Window) -> Option<Retained<NSView>> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        Some(objc2::Message::retain(view))
    }

    /// Remember the window WebKit pages go in.
    pub fn set_host(window: &winit::window::Window) {
        if let Some(view) = view_of(window) {
            HOST.with(|h| *h.borrow_mut() = Some(view));
        }
    }

    /// Put `view` under nus's drawing in `host`: the GPU's layer is a
    /// sibling of the view's in `host`'s layer (raw-window-metal adds it
    /// there), and order among siblings is their z position.
    fn beneath(view: &NSView, host: &NSView) {
        unsafe {
            let own: Option<Retained<NSObject>> = msg_send![view, layer];
            if let Some(layer) = own {
                let _: () = msg_send![&*layer, setZPosition: -1.0f64];
            }
            let root: Option<Retained<NSObject>> = msg_send![host, layer];
            let Some(root) = root else { return };
            let Some(metal) = AnyClass::get(c"CAMetalLayer") else { return };
            let sublayers: Option<Retained<NSArray<NSObject>>> = msg_send![&*root, sublayers];
            for layer in sublayers.iter().flat_map(|a| a.iter()) {
                if layer.isKindOfClass(metal) {
                    let _: () = msg_send![&*layer, setZPosition: 1.0f64];
                }
            }
        }
    }

    /// "Version/18.6 Safari/605.1.15": without it the services take
    /// WebKit for an unknown browser and turn it away.
    fn safari_name() -> &'static str {
        static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        NAME.get_or_init(|| {
            let version = std::process::Command::new("/usr/bin/defaults")
                .args(["read", "/Applications/Safari.app/Contents/Info", "CFBundleShortVersionString"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "18.0".into());
            format!("Version/{version} Safari/605.1.15")
        })
    }

    /// A WKWebView over one browser pane.
    pub struct NativePage {
        view: Retained<WKWebView>,
        /// The view's parent: as big as the part of the page nus's own
        /// drawing isn't over (a sliding sidebar), clipping the rest away.
        clip: Retained<NSView>,
        /// The window view it belongs in (and comes back to after picture
        /// in picture).
        home: Retained<NSView>,
        /// Cookies still being written before the first load.
        pending: Rc<Cell<usize>>,
        first: RefCell<Option<String>>,
        shown: Cell<bool>,
        /// The tracker's latest answer, until `take_report`; whether a
        /// question is out, and when it went.
        answer: Rc<RefCell<Option<String>>>,
        asking: Rc<Cell<bool>>,
        asked: Cell<Option<Instant>>,
        /// In nus's picture in picture: the view that holds the page there,
        /// and that window's view, held so the page can always come home.
        shelter: RefCell<Option<(Retained<Shelter>, Retained<NSView>)>>,
        /// WebKit's own pointer tracking, off while it is in that window.
        tracking: RefCell<Vec<(Retained<NSView>, Retained<NSTrackingArea>)>>,
    }

    impl NativePage {
        /// A page for `url`, signed in with `cookies` (DevTools' shape);
        /// it loads once they are all in WebKit's jar.
        pub fn new(url: &str, cookies: &[serde_json::Value], private: bool) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let host = HOST.with(|h| h.borrow().clone())?;
            unsafe {
                let config = WKWebViewConfiguration::new(mtm);
                let store = if private { WKWebsiteDataStore::nonPersistentDataStore(mtm) } else { WKWebsiteDataStore::defaultDataStore(mtm) };
                config.setWebsiteDataStore(&store);
                let prefs = config.preferences();
                prefs.setElementFullscreenEnabled(true);
                // Picture in picture is off for apps that embed WebKit (Safari
                // turns it on for itself). On, the page offers its button,
                // and the tracker hands what it asks for to nus's own. The
                // switch is WebKit SPI, so it is only flipped where this
                // WebKit has it.
                {
                    if prefs.respondsToSelector(objc2::sel!(_setAllowsPictureInPictureMediaPlayback:)) {
                        let _: () = msg_send![&*prefs, _setAllowsPictureInPictureMediaPlayback: true];
                    }
                }
                let tracker = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(WKUserScript::alloc(mtm), &NSString::from_str(super::TRACKER), WKUserScriptInjectionTime::AtDocumentStart, true);
                config.userContentController().addUserScript(&tracker);
                config.setApplicationNameForUserAgent(Some(&NSString::from_str(safari_name())));
                let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
                let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config);
                view.setAllowsBackForwardNavigationGestures(true);
                view.setInspectable(true);
                let clip = NSView::initWithFrame(NSView::alloc(mtm), frame);
                clip.setClipsToBounds(true);
                clip.setHidden(true);
                clip.addSubview(&view);
                host.addSubview(&clip);
                let page = NativePage {
                    view,
                    clip,
                    home: host,
                    pending: Rc::new(Cell::new(0)),
                    first: RefCell::new(Some(url.to_string())),
                    shown: Cell::new(false),
                    answer: Rc::new(RefCell::new(None)),
                    asking: Rc::new(Cell::new(false)),
                    asked: Cell::new(None),
                    shelter: RefCell::new(None),
                    tracking: RefCell::new(Vec::new()),
                };
                let jar = store.httpCookieStore();
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                for c in cookies {
                    let Some((line, at)) = super::set_cookie_line(c, now) else { continue };
                    let Some(at) = NSURL::URLWithString(&NSString::from_str(&at)) else { continue };
                    let fields = NSDictionary::from_slices(&[&*NSString::from_str("Set-Cookie")], &[&*NSString::from_str(&line)]);
                    for cookie in NSHTTPCookie::cookiesWithResponseHeaderFields_forURL(&fields, &at).iter() {
                        page.pending.set(page.pending.get() + 1);
                        let pending = page.pending.clone();
                        let done = block2::RcBlock::new(move || pending.set(pending.get().saturating_sub(1)));
                        jar.setCookie_completionHandler(&cookie, Some(&done));
                    }
                }
                Some(page)
            }
        }

        /// Called every tick: the first load goes once the cookies are in.
        pub fn tend(&self) {
            if self.pending.get() == 0 {
                let first = self.first.borrow_mut().take();
                if let Some(url) = first {
                    self.load(&url);
                }
            }
        }

        pub fn load(&self, url: &str) {
            let waiting = self.first.borrow().is_some();
            if waiting {
                *self.first.borrow_mut() = Some(url.to_string());
                return;
            }
            let Some(u) = NSURL::URLWithString(&NSString::from_str(url)) else { return };
            unsafe {
                self.view.loadRequest(&NSURLRequest::requestWithURL(&u));
            }
        }

        /// The window's view, while the page is in it: WebKit's element
        /// fullscreen moves the view into a window of its own, and there
        /// the size and showing are WebKit's, not the pane's; in picture
        /// in picture they are that window's.
        fn at_home(&self) -> Option<Retained<NSView>> {
            if self.shelter.borrow().is_some() {
                return None;
            }
            let parent = unsafe { self.view.superview() }?;
            (Retained::as_ptr(&parent) == Retained::as_ptr(&self.clip)).then(|| self.home.clone())
        }

        /// Lay the page over `page`, showing only `shown` of it (physical
        /// px from the window's top left). The page keeps its full size, so
        /// nothing reflows while the sidebar slides over it.
        pub fn place(&self, page: nus_render::Rect, shown: nus_render::Rect) {
            let Some(host) = self.at_home() else { return };
            let scale = host.window().map(|w| w.backingScaleFactor()).unwrap_or(2.0);
            let height = host.bounds().size.height;
            let flipped = host.isFlipped();
            // In the window view's points, bottom-left origin unless it's flipped.
            let points = |r: nus_render::Rect| {
                let (x, y, w, h) = (r.x as f64 / scale, r.y as f64 / scale, r.w as f64 / scale, r.h as f64 / scale);
                NSRect::new(NSPoint::new(x, if flipped { y } else { height - y - h }), NSSize::new(w.max(0.0), h.max(0.0)))
            };
            let (page, clip) = (points(page), points(shown));
            // The clip view isn't flipped: the page, from the clip's bottom left.
            let inner = NSRect::new(
                NSPoint::new(page.origin.x - clip.origin.x, if flipped { (clip.origin.y + clip.size.height) - (page.origin.y + page.size.height) } else { page.origin.y - clip.origin.y }),
                NSSize::new(page.size.width.max(1.0), page.size.height.max(1.0)),
            );
            if self.clip.frame() != clip {
                self.clip.setFrame(clip);
            }
            if self.view.frame() != inner {
                self.view.setFrame(inner);
            }
        }

        pub fn show(&self, on: bool) {
            let Some(home) = self.at_home() else { return };
            if self.shown.get() != on {
                self.shown.set(on);
                self.clip.setHidden(!on);
                if !on {
                    // Out of sight: give the keyboard back to the window.
                    if let Some(w) = self.view.window() {
                        w.makeFirstResponder(Some(&home));
                    }
                }
            }
        }

        /// Ask the tracker what it saw, a few times a second; the answer
        /// arrives on a later tick, for `take_report`.
        pub fn poll(&self) {
            if self.first.borrow().is_some() {
                return;
            }
            let since = self.asked.get().map(|t| t.elapsed());
            if since.is_some_and(|d| d < POLL || (self.asking.get() && d < POLL_LATE)) {
                return;
            }
            self.asking.set(true);
            self.asked.set(Some(Instant::now()));
            let (asking, answer) = (self.asking.clone(), self.answer.clone());
            let done = block2::RcBlock::new(move |r: *mut AnyObject, _e: *mut NSError| {
                asking.set(false);
                if let Some(s) = unsafe { r.as_ref() }.and_then(|r| r.downcast_ref::<NSString>()) {
                    *answer.borrow_mut() = Some(s.to_string());
                }
            });
            let js = if self.in_pip() { super::POLL_IN_PIP } else { super::POLL_AT_HOME };
            unsafe { self.view.evaluateJavaScript_completionHandler(&NSString::from_str(js), Some(&done)) };
        }

        /// The tracker's latest answer (`read_poll` reads it), once.
        pub fn take_report(&self) -> Option<String> {
            self.answer.borrow_mut().take()
        }

        /// Run `js` in the page. WebKit runs an app's script as though you
        /// had clicked, which play() asks for.
        pub fn eval(&self, js: &str) {
            unsafe { self.view.evaluateJavaScript_completionHandler(&NSString::from_str(js), None) };
        }

        pub fn in_pip(&self) -> bool {
            self.shelter.borrow().is_some()
        }

        /// Into nus's picture-in-picture `window`: the page fills the
        /// window's view beneath nus's drawing with only its video showing,
        /// and neither clicks nor the pointer reach it. False when it can't
        /// go (WebKit's own fullscreen has it).
        pub fn enter_pip(&self, window: &winit::window::Window) -> bool {
            if self.in_pip() {
                return true;
            }
            let Some(mtm) = MainThreadMarker::new() else { return false };
            let Some(host) = view_of(window) else { return false };
            if self.at_home().is_none() {
                return false;
            }
            // The keyboard stays with the window the page leaves.
            if let Some(w) = self.home.window() {
                w.makeFirstResponder(Some(&self.home));
            }
            let shelter: Retained<Shelter> = unsafe { msg_send![Shelter::alloc(mtm), initWithFrame: host.bounds()] };
            shelter.setWantsLayer(true);
            host.addSubview(&shelter);
            beneath(&shelter, &host);
            self.clip.removeFromSuperview();
            shelter.addSubview(&self.clip);
            self.clip.setHidden(false);
            self.shown.set(false);
            *self.shelter.borrow_mut() = Some((shelter, host));
            self.fit_pip();
            self.quiet(true);
            self.eval("window.__nusPip&&__nusPip(true)");
            true
        }

        /// Keep the page as big as the picture-in-picture window.
        pub fn fit_pip(&self) {
            let resized = {
                let shelter = self.shelter.borrow();
                let Some((shelter, host)) = shelter.as_ref() else { return };
                let b = host.bounds();
                let inner = NSRect::new(NSPoint::new(0.0, 0.0), b.size);
                let resized = shelter.frame() != b || self.view.frame() != inner;
                if resized {
                    shelter.setFrame(b);
                    self.clip.setFrame(inner);
                    self.view.setFrame(inner);
                }
                resized
            };
            // A new size can bring WebKit's tracking back.
            if resized {
                self.quiet(true);
            }
        }

        /// Back into the window it came from; `place` and `show` have it again.
        pub fn leave_pip(&self) {
            let Some((shelter, _host)) = self.shelter.borrow_mut().take() else { return };
            self.eval("window.__nusPip&&__nusPip(false)");
            self.clip.removeFromSuperview();
            self.clip.setHidden(true);
            self.shown.set(false);
            self.home.addSubview(&self.clip);
            shelter.removeFromSuperview();
            self.quiet(false);
        }

        /// WebKit follows the pointer with tracking areas of its own, which
        /// reach it whoever takes the clicks: in nus's window they come off
        /// (none of the page's hover controls, none of its cursors over
        /// nus's) and go back after, each once.
        fn quiet(&self, on: bool) {
            let mut kept = self.tracking.borrow_mut();
            if on {
                let mut views: Vec<Retained<NSView>> = vec![Retained::into_super(self.view.clone())];
                while let Some(v) = views.pop() {
                    for area in v.trackingAreas().iter() {
                        v.removeTrackingArea(&area);
                        if !kept.iter().any(|(_, a)| Retained::as_ptr(a) == Retained::as_ptr(&area)) {
                            kept.push((v.clone(), area));
                        }
                    }
                    views.extend(v.subviews().iter());
                }
            } else {
                for (v, area) in kept.drain(..) {
                    let there = v.trackingAreas().iter().any(|a| Retained::as_ptr(&a) == Retained::as_ptr(&area));
                    if !there {
                        v.addTrackingArea(&area);
                    }
                }
            }
        }

        pub fn url(&self) -> String {
            unsafe { self.view.URL() }.and_then(|u| u.absoluteString()).map(|s| s.to_string()).unwrap_or_else(|| self.first.borrow().clone().unwrap_or_default())
        }
        pub fn title(&self) -> String {
            unsafe { self.view.title() }.map(|s| s.to_string()).unwrap_or_default()
        }
        pub fn loading(&self) -> bool {
            self.first.borrow().is_some() || unsafe { self.view.isLoading() }
        }
        pub fn can_go_back(&self) -> bool {
            unsafe { self.view.canGoBack() }
        }
        pub fn can_go_forward(&self) -> bool {
            unsafe { self.view.canGoForward() }
        }
        pub fn back(&self) {
            unsafe { self.view.goBack() };
        }
        pub fn forward(&self) {
            unsafe { self.view.goForward() };
        }
        pub fn reload(&self) {
            unsafe { self.view.reload() };
        }
    }

    impl Drop for NativePage {
        fn drop(&mut self) {
            unsafe {
                self.view.stopLoading();
                self.view.pauseAllMediaPlaybackWithCompletionHandler(None);
            }
            self.show(false);
            self.view.removeFromSuperview();
            self.clip.removeFromSuperview();
            if let Some((shelter, _)) = self.shelter.borrow_mut().take() {
                shelter.removeFromSuperview();
            }
            self.tracking.borrow_mut().clear();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn set_host(_window: &winit::window::Window) {}

    pub struct NativePage;

    impl NativePage {
        pub fn new(_url: &str, _cookies: &[serde_json::Value], _private: bool) -> Option<Self> { None }
        pub fn tend(&self) {}
        pub fn load(&self, _url: &str) {}
        pub fn place(&self, _page: nus_render::Rect, _shown: nus_render::Rect) {}
        pub fn show(&self, _on: bool) {}
        pub fn poll(&self) {}
        pub fn take_report(&self) -> Option<String> { None }
        pub fn eval(&self, _js: &str) {}
        pub fn in_pip(&self) -> bool { false }
        pub fn enter_pip(&self, _window: &winit::window::Window) -> bool { false }
        pub fn fit_pip(&self) {}
        pub fn leave_pip(&self) {}
        pub fn url(&self) -> String { String::new() }
        pub fn title(&self) -> String { String::new() }
        pub fn loading(&self) -> bool { false }
        pub fn can_go_back(&self) -> bool { false }
        pub fn can_go_forward(&self) -> bool { false }
        pub fn back(&self) {}
        pub fn forward(&self) {}
        pub fn reload(&self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_hosts() {
        let on = cfg!(target_os = "macos");
        assert_eq!(protected("https://www.netflix.com/browse"), on);
        assert_eq!(protected("https://www.primevideo.com/"), on);
        assert_eq!(protected("https://www.amazon.com/gp/video/storefront"), on);
        assert!(!protected("https://www.amazon.com/dp/B000"));
        assert!(!protected("https://notnetflix.com/"));
        assert!(!protected("http://www.netflix.com/"));
        assert!(!protected("https://example.com/?u=netflix.com"));
    }

    #[test]
    fn cookie_lines() {
        let c = serde_json::json!({"name":"NetflixId","value":"v=1&x","domain":".netflix.com","path":"/","expires":2000.0,"session":false,"secure":true,"httpOnly":true,"sameSite":"Lax"});
        let (line, url) = set_cookie_line(&c, 1000.0).unwrap();
        assert_eq!(line, "NetflixId=v=1&x; Path=/; Domain=.netflix.com; Max-Age=1000; Secure; HttpOnly; SameSite=Lax");
        assert_eq!(url, "https://netflix.com/");
        let host_only = serde_json::json!({"name":"a","value":"b","domain":"www.netflix.com","path":"/","session":true});
        assert_eq!(set_cookie_line(&host_only, 0.0).unwrap().0, "a=b; Path=/");
    }

    #[test]
    fn a_poll_answer_carries_the_report_and_the_ask() {
        let (report, asked) = read_poll(r#"{"r":"{\"v\":null,\"top\":true}","pip":true}"#).unwrap();
        assert_eq!(report, r#"{"v":null,"top":true}"#);
        assert!(asked);
        assert_eq!(read_poll(r#"{"r":"","pip":false}"#), Some((String::new(), false)));
        assert_eq!(read_poll("not json"), None);
        assert_eq!(read_poll(r#"{"pip":true}"#), None);
    }

    #[test]
    fn the_tracker_runs_in_webkit_with_its_own_ends() {
        assert!(TRACKER.starts_with("window.nusVideo=window.nusVideo||"));
        assert!(TRACKER.contains("window.__nus = {"));
        assert!(TRACKER.contains("window.__nusPip="));
        assert!(!TRACKER.contains("webkitSetPresentationMode('picture-in-picture')"));
    }

    #[test]
    fn prime_signs_in_through_amazon() {
        let urls = cookie_urls("https://www.primevideo.com/detail/x");
        assert!(urls.iter().any(|u| u == "https://www.amazon.com/"));
    }
}
