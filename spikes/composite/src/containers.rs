//! Containers: named cookie jars. PERSONAL is the profile's own; every
//! other container is a CEF request context with its own cache under
//! profile/container-<name>, so sign-ins, cookies and storage stay
//! apart. A window has a container — its square wears the colour, and
//! new pages open in it; a new window inherits it. A page can be
//! reopened in another container from the palette.

use crate::app::Caps;
use crate::app::App;
use cef::rc::Rc;
use cef::{ImplRequestContextHandler, RequestContextHandler, WrapRequestContextHandler};
use nus_render::Color;
use std::cell::RefCell;
use std::collections::HashMap;

pub const PERSONAL: &str = "PERSONAL";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Container {
    pub name: String,
    /// #rrggbb; None means the signal.
    pub colour: Option<String>,
}

fn path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("containers.json")
}

pub fn load() -> Vec<Container> {
    let mut v: Vec<Container> = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if !v.iter().any(|c| c.name == PERSONAL) {
        v.insert(0, Container { name: PERSONAL.into(), colour: None });
    }
    v
}

pub fn save(v: &[Container]) {
    let _ = crate::store::write_json(&path(), &v);
}

thread_local! {
    static CONTEXTS: RefCell<HashMap<String, cef::RequestContext>> = RefCell::new(HashMap::new());
    /// Contexts whose profile has finished initialising (a browser made
    /// before that fails).
    static READY: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
}

cef::wrap_request_context_handler! {
    struct Ready {
        name: String,
    }

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, _request_context: Option<&mut cef::RequestContext>) {
            READY.with(|r| r.borrow_mut().insert(self.name.clone()));
        }
    }
}

/// Release cached context references before CEF shutdown.
pub fn shutdown() {
    CONTEXTS.with(|c|c.borrow_mut().clear());
    READY.with(|r|r.borrow_mut().clear());
}

/// The request context for a container: the global one for PERSONAL,
/// else one made (once) with its own cache directory.
pub fn context(name: &str) -> Option<cef::RequestContext> {
    if crate::private::enabled() { return private_context(); }
    if name.is_empty() || name == PERSONAL {
        return cef::request_context_get_global_context();
    }
    CONTEXTS.with(|c| {
        if let Some(ctx) = c.borrow().get(name) {
            return Some(ctx.clone());
        }
        // Chrome's profile manager wants a direct child of the root cache path.
        let dir = std::env::current_dir().unwrap_or_default().join("profile").join(format!("container-{}", name.to_lowercase()));
        let _ = std::fs::create_dir_all(&dir);
        let settings = cef::RequestContextSettings {
            cache_path: dir.to_string_lossy().as_ref().into(),
            persist_session_cookies: 1,
            ..Default::default()
        };
        let mut handler = Ready::new(name.to_string());
        let ctx = cef::request_context_create_context(Some(&settings), Some(&mut handler))?;
        c.borrow_mut().insert(name.to_string(), ctx.clone());
        // The profile comes up on the UI thread a few pumps later; wait for
        // it here (bounded), so the first page in the container is made
        // against a live context.
        let t0 = crate::clock::now();
        while !READY.with(|r| r.borrow().contains(name)) && crate::clock::since(t0).as_secs_f32() < 3.0 {
            cef::do_message_loop_work();
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        tracing::info!("container {name}: context ready in {}ms", crate::clock::since(t0).as_millis());
        Some(ctx)
    })
}

/// Chrome runtime's global profile may have a cache path even when the
/// global setting was empty. Explicit non-shared contexts are off the record.
fn private_context() -> Option<cef::RequestContext> {
    const KEY: &str = "__nus_private";
    if let Some(context) = CONTEXTS.with(|c| c.borrow().get(KEY).cloned()) { return Some(context); }
    let settings = cef::RequestContextSettings::default();
    let mut handler = Ready::new(KEY.into());
    let context = cef::request_context_create_context(Some(&settings), Some(&mut handler))?;
    let deadline = std::time::Instant::now()+std::time::Duration::from_secs(3);
    while !READY.with(|r| r.borrow().contains(KEY)) {
        if std::time::Instant::now() >= deadline { return None; }
        cef::do_message_loop_work();
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    CONTEXTS.with(|c| c.borrow_mut().insert(KEY.into(), context.clone()));
    Some(context)
}

pub fn release_private_context() {
    if crate::private::enabled() {
        CONTEXTS.with(|c| c.borrow_mut().clear());
        READY.with(|r| r.borrow_mut().clear());
    }
}

impl App {
    /// This window's container's colour (the signal for PERSONAL).
    pub(crate) fn container_colour(&self) -> Color {
        self.colour_of(&self.container)
    }

    pub(crate) fn colour_of(&self, name: &str) -> Color {
        self.containers
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.colour.as_deref())
            .and_then(crate::surface::parse_hex)
            .unwrap_or(self.surface.signal)
    }

    /// Switch this window's container: new pages open in it.
    pub(crate) fn set_container(&mut self, name: &str) {
        if !self.containers.iter().any(|c| c.name == name) {
            return;
        }
        self.container = name.to_string();
        self.register_window();
        self.save_session();
        self.play_event("toggle");
        self.dirty = true;
    }

    /// A new container, coloured from the swatches in turn; switches to it.
    pub(crate) fn new_container(&mut self, name: &str) {
        let name = name.trim().caps();
        if name.is_empty() {
            return;
        }
        if !self.containers.iter().any(|c| c.name == name) {
            let k = self.containers.len() % crate::surface::SWATCHES.len();
            let colour = crate::surface::hex(crate::surface::SWATCHES[k].1);
            self.containers.push(Container { name: name.clone(), colour: Some(colour) });
            save(&self.containers);
        }
        self.set_container(&name);
    }

    /// Reopen the active page in `name`: a fresh browser in that
    /// container's context takes the tab's place.
    pub(crate) fn reopen_in(&mut self, name: &str) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let crate::app::Pane::Web(w) = (if tab.focus_right && tab.right.is_some() { tab.right.as_ref().unwrap() } else { &tab.left }) else { return };
        let url = w.tab.shared.borrow().url.clone();
        let right = tab.focus_right && tab.right.is_some();
        let Some(pane) = self.new_web_pane_in(&url, name) else { return };
        let tab = &mut self.tabs[self.active];
        if right {
            tab.right = Some(crate::app::Pane::Web(pane));
        } else {
            tab.left = crate::app::Pane::Web(pane);
        }
        self.play_event("tab.switch");
        self.layout();
        self.save_session();
    }
}
