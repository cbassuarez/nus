//! Protected video through the system's WebKit. Netflix, Prime and the
//! rest play only under a DRM the browser is licensed for: Chromium's
//! Widevine wants a VMP signature a custom build can't get for free, while
//! WebKit's FairPlay ships with macOS. A page on one of these hosts is
//! shown in a WKWebView laid over the pane, signed in with the pane's own
//! cookies, and the Chromium page underneath goes to about:blank.

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

pub use imp::*;

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSDictionary, NSHTTPCookie, NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest};
    use objc2_web_kit::{WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    thread_local! {
        /// The main window's content view: WebKit pages are its subviews.
        static HOST: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    }

    /// Remember the window WebKit pages go in.
    pub fn set_host(window: &winit::window::Window) {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        HOST.with(|h| *h.borrow_mut() = Some(objc2::Message::retain(view)));
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

    /// The biggest playing video to picture in picture, or back. The
    /// services mark theirs disablePictureInPicture; that's lifted first.
    const PIP_JS: &str = r#"(on)=>{const vs=[...document.querySelectorAll('video')];
const area=v=>v.clientWidth*v.clientHeight;
if(on){const v=vs.filter(v=>!v.paused&&!v.ended&&v.readyState>1).sort((a,b)=>area(b)-area(a))[0];
if(!v)return 'no playing video';v.disablePictureInPicture=false;v.removeAttribute('disablepictureinpicture');
if(v.webkitSupportsPresentationMode&&v.webkitSupportsPresentationMode('picture-in-picture')){v.webkitSetPresentationMode('picture-in-picture');return 'pip'}
if(v.requestPictureInPicture){v.requestPictureInPicture().catch(e=>{});return 'requested'}return 'unsupported'}
for(const v of vs){if(v.webkitPresentationMode==='picture-in-picture')v.webkitSetPresentationMode('inline')}
if(document.pictureInPictureElement)document.exitPictureInPicture().catch(e=>{});return 'inline'}"#;

    /// A WKWebView over one browser pane.
    pub struct NativePage {
        view: Retained<WKWebView>,
        /// The view's parent: as big as the part of the page nus's own
        /// drawing isn't over (a sliding sidebar), clipping the rest away.
        clip: Retained<NSView>,
        /// Cookies still being written before the first load.
        pending: Rc<Cell<usize>>,
        first: RefCell<Option<String>>,
        shown: Cell<bool>,
        /// Its video was sent to picture in picture: out of sight, the view
        /// is clipped to nothing rather than hidden, so WebKit still takes
        /// the page as on screen and the video carries on in the window.
        pip: Cell<bool>,
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
                config.preferences().setElementFullscreenEnabled(true);
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
                let page = NativePage { view, clip, pending: Rc::new(Cell::new(0)), first: RefCell::new(Some(url.to_string())), shown: Cell::new(false), pip: Cell::new(false) };
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
        /// the size and showing are WebKit's, not the pane's.
        fn at_home(&self) -> Option<Retained<NSView>> {
            let host = HOST.with(|h| h.borrow().clone())?;
            let parent = unsafe { self.view.superview() }?;
            (Retained::as_ptr(&parent) == Retained::as_ptr(&self.clip)).then_some(host)
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
            if self.at_home().is_none() {
                return;
            }
            if self.shown.get() != on {
                self.shown.set(on);
                if !on && self.pip.get() {
                    let f = self.clip.frame();
                    self.clip.setFrame(NSRect::new(f.origin, NSSize::new(0.0, 0.0)));
                } else {
                    self.clip.setHidden(!on);
                }
                if !on {
                    // Out of sight: give the keyboard back to the window.
                    if let (Some(w), Some(host)) = (self.view.window(), HOST.with(|h| h.borrow().clone())) {
                        w.makeFirstResponder(Some(&host));
                    }
                }
            }
        }

        /// Send the playing video to the system's picture in picture (on),
        /// or bring it back into the page (off). WebKit runs the app's
        /// script as though you had clicked, which PiP asks for.
        pub fn pip(&self, on: bool) {
            if !on && !self.pip.get() {
                return;
            }
            self.pip.set(on);
            let js = format!("({PIP_JS})({on})");
            let done = block2::RcBlock::new(|r: *mut objc2::runtime::AnyObject, _e: *mut objc2_foundation::NSError| {
                let said = unsafe { r.as_ref() }.and_then(|r| r.downcast_ref::<NSString>()).map(|s| s.to_string()).unwrap_or_default();
                tracing::info!("webkit pip: {said}");
            });
            unsafe { self.view.evaluateJavaScript_completionHandler(&NSString::from_str(&js), Some(&done)) };
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
        pub fn pip(&self, _on: bool) {}
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
    fn prime_signs_in_through_amazon() {
        let urls = cookie_urls("https://www.primevideo.com/detail/x");
        assert!(urls.iter().any(|u| u == "https://www.amazon.com/"));
    }
}
