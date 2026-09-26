//! Spike 4: one compositor, terminal + browser panes, Broadsheet chrome.
//! See docs/SPIKES.md.

mod access;
mod overscroll;
mod interstitial;
mod interstitial_ui;
mod pip_dock;
#[cfg(target_os = "macos")]
mod cef_app_mac;
mod agent;
mod ledger;
mod director;
mod pane_mode;
mod send;
mod shell_colors;
mod live;
mod intelligence;
mod page_dialog;
mod finish_work;
mod finish_work_native;
mod clock;
mod memory_pressure;
mod shell;
mod termui;
mod predict;
mod webui;
mod welcome;
mod page_signal;
mod install;
mod compatibility;
mod themes;
mod macos;
mod dock;
mod downloads;
mod sidebar;
mod pins;
mod fonts;
mod prompt;
mod saved_commands;
mod import_flow;
mod assistants;
mod look_menu;
mod windows;
mod tiles;
mod peek;
mod compact;
mod folders;
mod sites;
mod containers;
mod ask;
mod smear;
mod syntax;
mod panes;
mod scrolling;
mod bundles;
mod pointer;
mod app_icon;
mod editor;
mod editor_work;
mod perf;
mod distribution;
mod work;
mod lsp_host;
mod prompt_lsp;
mod ports;
mod hatch;
mod hatch_work;
mod hatch_native;
mod hatch_tray;
mod menu_drawer;
mod blocks;
mod journal;
mod cutoff;
mod hands;
mod replay;
mod links;
mod home;
mod field;
mod webkeys;
mod swipe;
mod store;
mod storage;
mod zoom;
mod application_menu;
#[path = "loop_.rs"]
mod loop_;
mod blockpage;
mod remote;
mod taskbar;
mod askctx;
mod layout_file;
mod ssh;
mod tidy;
mod syncui;
mod me;
mod hotkey;
mod anim;
mod split_flap;
mod app;
mod browser;
mod browser_runtime;
mod browser_cache;
mod little;
mod pip;
mod pip_policy;
mod file_viewer;
mod file_viewer_app;
mod prefs;
mod reader;
mod library;
mod celestial;
mod settings;
mod sound;
mod shot;
mod splash;
mod hyperdrive;
mod plate;
mod toast;
mod page_menu;
mod art;
mod forge;
mod power;
mod touch;
mod news;
mod diffs;
mod phone;
mod private;
mod widevine;
mod webkit;
mod security;
mod secrets;
mod protected_state;
mod updates;
mod mercury;
mod update_install;
mod support;
mod pick;
mod files;
mod procs;
mod start;
mod surface;
mod theme_edit;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use cef::args::Args;
use cef::*;
use winit::application::ApplicationHandler;
use winit::event::{WindowEvent, ElementState, MouseButton};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Window, WindowId};

use app::App;

#[derive(Debug)]
pub enum UserEvent {
    BrowserWork,
    BrowserRedirect(i32,String,String),
    Wake,
    ApplicationCommand(application_menu::Command),
    ApplicationMenuCheck(application_menu::Command),
    DockAttention,
    WindowControl(WindowId,usize),
    Access(accesskit_winit::Event),
    /// The global hotkey: summon (or hide) the hatch.
    Hatch,
    HatchWork,
    HatchShow,
    HatchHide,
    HatchSelect(hatch_work::Target),
    HatchMain,
    HatchQuit,
    MenuDrawer(Option<menu_drawer::Anchor>),
    MenuSelect(hatch_work::Target),
    HatchResize(WindowId, u32, u32),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::Access(e)
    }
}

struct Host {
    proxy: EventLoopProxy<UserEvent>,
    /// One app per window, in creation order.
    apps: Vec<App>,
    /// AccessKit adapters, one per app window; each tree is rebuilt after
    /// a frame that changed something.
    access: Vec<(WindowId, accesskit_winit::Adapter, u64)>,
    /// How many windows have been made, for naming and ordering.
    made: usize,
    /// The window the user was last in: "raise" from another launch goes here.
    focused: Option<WindowId>,
    /// The instance port's request channel, for the phone's page to speak through.
    inbound: std::sync::mpsc::Sender<little::Inbound>,
    tray: Option<hatch_tray::Tray>,
    application_menu: Option<application_menu::NativeMenu>,
    work_at: Option<std::time::Instant>,
    hatch_owner: Option<WindowId>,
    dock: dock::Dock,
    /// Finish Work's one lease for the whole process (finish_work.rs).
    finish: finish_work::FinishWork,
}

impl Host {
    fn app_index(&self, id: WindowId) -> Option<usize> {
        if let Some(i)=self.apps.iter().position(|a|a.menu_drawer.window.as_ref().is_some_and(|d|d.window.id()==id)){return Some(i);}
        self.apps.iter().position(|a| a.window.id() == id || a.little.as_ref().is_some_and(|l| l.window.id() == id) || a.pip.as_ref().is_some_and(|p| p.window.id() == id) || a.hatch.as_ref().is_some_and(|h| h.window.id() == id) || a.hatch_state.badge.as_ref().is_some_and(|b| b.window.id()==id) || a.hatch_state.shade.as_ref().is_some_and(|b| b.window.id()==id))
    }

    fn hatch_index(&self) -> Option<usize> {
        self.apps.iter().position(|a|a.hatch.as_ref().is_some_and(|h|h.visible&&!h.hiding)).or_else(|| {
            if self.apps.first().is_some_and(|a|a.behavior.hatch_spaces==settings::HatchSpaces::One) {
                self.apps.iter().enumerate().filter(|(_,a)|a.hatch_state.summoned.is_some()).max_by_key(|(_,a)|a.hatch_state.summoned).map(|(i,_)|i)
            } else {self.focused.and_then(|id|self.apps.iter().position(|a|a.window.id()==id))}
        }).or(if self.apps.is_empty(){None}else{Some(0)})
    }

    fn any_window_focused(&self)->bool {
        self.apps.iter().any(|a|a.window.has_focus()
            ||a.hatch.as_ref().is_some_and(|h|h.window.has_focus())
            ||a.pip.as_ref().is_some_and(|p|p.window.has_focus())
            ||a.little.as_ref().is_some_and(|l|l.window.has_focus())
            ||a.menu_drawer.window.as_ref().is_some_and(|d|d.window.has_focus()))
    }

    fn refresh_hatch_work(&mut self) {
        if let Some(a) = self.apps.first() {
            storage::tick(self.apps.iter().filter_map(|a| a.recorder.as_ref().map(|r| r.dir.clone())).collect(), a.behavior.journal_keep, a.behavior.replay.days().unwrap_or(7));
        }
        let owner=self.apps.iter().filter_map(|a|a.hatch.as_ref()).find(|h|h.visible&&!h.hiding).map(|h|h.window.id());
        if self.hatch_owner==owner && self.work_at.is_some_and(|at|crate::clock::since(at).as_millis()<200){return;}
        let app_focused=self.any_window_focused();
        self.hatch_owner=owner;
        self.work_at=Some(std::time::Instant::now());
        let mut work:Vec<_>=self.apps.iter().flat_map(hatch_work::collect).collect();
        hatch_work::sort(&mut work);
        let newest=self.apps.iter().enumerate().filter(|(_,a)|a.hatch.as_ref().is_some_and(|h|h.visible&&!h.hiding)).max_by_key(|(_,a)|a.hatch_state.summoned).map(|(i,_)|i);
        let island_open=newest.and_then(|i|self.apps[i].hatch.as_ref()).filter(|h|h.notch.is_some()).map(|h|h.mon);
        for (i,a) in self.apps.iter_mut().enumerate() {
            if let Some(d)=a.menu_drawer.window.as_ref().filter(|d|d.visible){d.window.request_redraw();}
            a.hatch_state.island_open=island_open;
            if newest.is_some_and(|n|n!=i){a.hide_hatch_inner(false);}
            if i == 0 && a.behavior.hatch_notify {
                if let Some(item) = hatch_work::completion(&a.hatch_state.work, &work) {
                    a.hatch_state.completion = Some((item.clone(), std::time::Instant::now()));
                    self.dock.attention(&a.window,a.motion.reduced(),app_focused);
                } else if hatch_work::needs_attention(&a.hatch_state.work,&work) {
                    self.dock.attention(&a.window,a.motion.reduced(),app_focused);
                }
            }
            if a.hatch_state.work!=work {
                let selected=a.hatch_state.work.get(a.hatch_state.selected).map(|r|r.target);
                a.hatch_state.work=work.clone();
                a.hatch_state.selected=selected.and_then(|target|work.iter().position(|r|r.target==target)).unwrap_or(0);
                a.dirty=true;
            }
            a.hatch_state.badge_suppressed=newest.is_some();
            if i==0 {
                if newest.is_none() && a.hatch_state.badge.as_ref().is_none_or(|b| !b.window.has_focus()) { a.hatch_state.foreground = hatch_native::Foreground::capture(); }
                a.hatch_badge_frame();
            }
            if let Some(id)=a.hatch.as_ref().filter(|h|h.visible).map(|h|h.window.id()) {
                if let Some((_,adapter,_))=self.access.iter_mut().find(|(wid,_,_)|*wid==id){adapter.update_if_active(||a.hatch_access_tree());}
            }
        }
        if let Some(tray)=&mut self.tray {
            if let Some(a)=self.focused.and_then(|id|self.apps.iter().find(|a|a.window.id()==id)).or_else(||self.apps.first()) {
                tray.update(menu_drawer::Signal::collect(&work,&downloads::list()),&a.behavior.menu_drawer);
            }
        }
    }

    /// Open a window: the first as the prefs say, later ones next to the
    /// window that asked, with one shell and no session restore.
    fn spawn_window(&mut self, event_loop: &ActiveEventLoop, from: Option<usize>) {
        // The window comes up as the prefs say: last place, maximized,
        // fullscreen, or centred at 1440×900.
        let prefs = prefs::Prefs::load();
        let secondary = from.is_some();
        let start = if secondary { settings::WindowStart::Centered } else { prefs.behavior.as_ref().map(|b| b.window_start).unwrap_or(settings::WindowStart::Last) };
        // The icon from the first frame: Broadsheet ink and signal until
        // the app redraws it in the live colours.
        let icon = {
            let rgba = nus_render::dock_icon::render(64, nus_render::theme::hex(0xc8102e), nus_render::dock_icon::Face::Newsreader);
            winit::window::Icon::from_rgba(rgba, 64, 64).ok()
        };
        let mut attrs = Window::default_attributes()
            .with_title("nus")
            .with_window_icon(icon.clone())
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(false)
            // Photographing itself (NUS_SHOT): come up without taking the
            // focus, so whatever the user is typing keeps going where it was.
            .with_active(std::env::var_os("NUS_SHOT").is_none())
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        match start {
            settings::WindowStart::Last => {
                if let Some((x, y, w, h)) = prefs.window_rect {
                    attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(x, y)).with_inner_size(winit::dpi::PhysicalSize::new(w, h));
                }
            }
            settings::WindowStart::Maximized => attrs = attrs.with_maximized(true),
            settings::WindowStart::Fullscreen => attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None))),
            settings::WindowStart::Centered => {}
        }
        // NUS_SHOT_SIZE=1600x1000: the recorder's window, exactly, whatever
        // the prefs remember — every clip of every shot the same shape.
        if let Some(size) = std::env::var_os("NUS_SHOT_SIZE") {
            let size = size.to_string_lossy().to_lowercase();
            if let Some((w, h)) = size.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.trim().parse::<f64>(), h.trim().parse::<f64>()) {
                    attrs = attrs.with_maximized(false).with_fullscreen(None).with_inner_size(winit::dpi::LogicalSize::new(w, h));
                }
            }
        }
        if let Some(i) = from {
            // Cascade from the asking window.
            if let Some(a) = self.apps.get(i) {
                if let Ok(p) = a.window.outer_position() {
                    let s = a.window.inner_size();
                    attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(p.x + 40, p.y + 40)).with_inner_size(winit::dpi::PhysicalSize::new(s.width, s.height));
                }
            }
        }
        #[cfg(target_os="linux")]
        {
            // The same ID on Wayland and X11 links the window to its launcher.
            attrs=winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs,"dev.nus.app","nus");
        }
        let attrs = crate::macos::main_window_attributes(attrs);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("window: {e}");
                return;
            }
        };
        perf::startup(perf::StartupMark::WindowCreated);
        // The adapter must exist before the window is first shown.
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        crate::macos::prepare_window(&window);
        if let Some(menu)=&self.application_menu {menu.attach(&window);}
        // The folder the asking window works in, for a shell born here.
        let born_in = from.and_then(|i| self.apps.get(i)).and_then(|a| a.workspace.as_ref().map(|w| w.to_string_lossy().to_string()).or_else(|| a.focused_cwd()));
        match App::new(window.clone(), self.proxy.clone(), secondary, self.made, born_in) {
            Ok(mut a) => {
                perf::startup(perf::StartupMark::AppReady);
                a.inbound = Some(self.inbound.clone());
                a.phone_at_launch();
                a.fullscreen = start == settings::WindowStart::Fullscreen;
                if let Some(parent) = from.and_then(|i| self.apps.get(i)) {
                    a.container = parent.container.clone();
                    a.register_window();
                }
                // Prepare the complete shell before making the window visible.
                a.redraw();
                if self.made == 0 { self.dock.finish_launch(); }
                window.set_visible(true);
                a.dirty=true;
                window.request_redraw();
                self.access.push((window.id(), adapter, 0));
                self.apps.push(a);
                self.made += 1;
                self.focused = Some(window.id());
            }
            Err(e) => {
                tracing::error!("init: {e:#}");
                if self.apps.is_empty() {
                    event_loop.exit();
                }
            }
        }
    }

    /// Move tab `id` out of window `i`, to `dest` (send.rs). A point on
    /// screen lands in the nus window under it, or a new window there.
    fn send_tab(&mut self, event_loop: &ActiveEventLoop, i: usize, id: u64, dest: send::Dest) {
        use send::Dest;
        let dest = match dest {
            Dest::At(x, y) => {
                let under = self.apps.iter().position(|a| {
                    let Ok(p) = a.window.outer_position() else { return false };
                    let s = a.window.outer_size();
                    a.window.is_visible() != Some(false) && x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
                });
                match under {
                    Some(j) if j == i => return,
                    Some(j) => Dest::Window(u64::from(self.apps[j].window.id())),
                    None => Dest::At(x, y),
                }
            }
            d => d,
        };
        let Some(tab) = self.apps.get_mut(i).and_then(|a| a.take_tab(id)) else { return };
        match dest {
            Dest::Window(w) => match self.apps.iter().position(|a| u64::from(a.window.id()) == w) {
                Some(j) => {
                    self.apps[j].receive_tab(tab);
                    self.apps[j].window.focus_window();
                }
                None => self.apps[i].receive_tab(tab),
            },
            Dest::New | Dest::At(..) => {
                let before = self.apps.len();
                self.spawn_window(event_loop, Some(i));
                if self.apps.len() > before {
                    let a = self.apps.last_mut().expect("just made");
                    if let Dest::At(x, y) = dest {
                        a.window.set_outer_position(winit::dpi::PhysicalPosition::new(x - 60, y - 20));
                    }
                    a.receive_tab(tab);
                    // The window's own first tab gives way to the one it was made for.
                    a.drop_birth = true;
                } else {
                    self.apps[i].receive_tab(tab);
                }
            }
        }
    }

    /// Tell every app about every window.
    fn share_registry(&mut self) {
        let entries: Vec<windows::Entry> = self
            .apps
            .iter()
            .map(|a| windows::Entry { id: u64::from(a.window.id()), name: a.window_name(), tabs: a.tabs.len(), ordinal: a.ordinal, colour: a.container_colour() })
            .collect();
        // One shared snapshot: N windows used to each retain and compare N
        // entries on every loop, making both storage and comparisons quadratic.
        if self.apps.first().is_some_and(|a| a.windows.as_ref() == &entries) { return; }
        let entries = Arc::new(entries);
        for a in &mut self.apps {
            a.windows = entries.clone();
            a.dirty = true;
        }
    }
}

impl ApplicationHandler<UserEvent> for Host {
    fn exiting(&mut self, _: &ActiveEventLoop) {
        // Whatever Finish Work holds goes first: a quit is always the user's
        // (or the OS's) word, and nothing outlives it.
        self.release_finish_work();
        // AppKit's native Quit can terminate inside the event pump, before
        // main reaches its normal shutdown path.
        self.dock.prepare_quit();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.application_menu.is_none() { self.application_menu = Some(application_menu::NativeMenu::new(self.proxy.clone())); }
        if self.apps.is_empty() {
            self.spawn_window(event_loop, None);
        }
        if self.tray.is_none() && hatch_native::interactive() && !private::enabled() {
            self.tray = hatch_tray::Tray::new(self.proxy.clone());
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::ApplicationMenuCheck(command) => {
                let menu=self.application_menu.as_ref().expect("native application menu");
                assert_eq!(menu.labels(),["nus","File","Edit","View","Window","Help"]);
                menu.activate_for_check(command);
            }
            UserEvent::ApplicationCommand(command) => {
                if command == application_menu::Command::Quit { _el.exit(); return; }
                if self.apps.is_empty() { self.spawn_window(_el, None); }
                let i=self.focused.and_then(|id|self.app_index(id)).unwrap_or(0);
                if let Some(a)=self.apps.get_mut(i) {
                    if a.hatch.as_ref().is_some_and(|h|h.window.has_focus()) { a.in_hatch(|a|a.application_command(command)); }
                    else {a.application_command(command);}
                }
            }
            UserEvent::DockAttention => {
                if let Some(a)=self.apps.first(){self.dock.attention(&a.window,a.motion.reduced(),self.any_window_focused());}
            }
            UserEvent::WindowControl(id,action) => {
                if action==0 {self.window_event(_el,id,WindowEvent::CloseRequested);}
                else if let Some(a)=self.apps.iter_mut().find(|a|a.window.id()==id) {
                    if action==1 {a.window.set_minimized(true);} else {a.toggle_fullscreen();}
                }
            }
            UserEvent::HatchQuit => _el.exit(),
            UserEvent::MenuDrawer(anchor) => {
                let anchor=anchor.or_else(||self.tray.as_ref().and_then(|t|t.anchor()));
                let i=self.apps.iter().position(|a|a.menu_drawer.window.as_ref().is_some_and(|d|d.visible)).or_else(||self.focused.and_then(|id|self.apps.iter().position(|a|a.window.id()==id))).unwrap_or(0);
                if let Some(a)=self.apps.get_mut(i){a.toggle_menu_drawer(anchor);}
            }
            UserEvent::MenuSelect(target) => {
                if let Some(a)=self.apps.iter_mut().find(|a|u64::from(a.window.id())==target.window) {
                    if let Some(i)=a.tabs.iter().position(|t|t.id==target.tab&&(!target.right||t.right.is_some())) {
                        if a.tabs[i].hatch {a.hide_hatch_inner(false);a.tabs[i].hatch=false;}
                        a.tabs[i].focus_right=target.right;a.activate(i);a.hatch_state.main_hidden=false;
                        a.window.set_visible(true);a.window.set_minimized(false);if hatch_native::interactive(){a.window.focus_window();}a.layout();a.dirty=true;self.focused=Some(a.window.id());
                    }
                }
            }
            UserEvent::HatchResize(id,width,height) => {
                if let Some(a)=self.apps.iter_mut().find(|a|a.window.id()==id) {
                    if let Some(h)=&a.hatch {
                        let _=h.window.request_inner_size(winit::dpi::PhysicalSize::new(width,height));
                        a.hatch_resized(width,height);
                    }
                }
            }
            UserEvent::HatchMain => {
                let i=self.focused.and_then(|id|self.apps.iter().position(|a|a.window.id()==id)).unwrap_or(0);
                if let Some(a)=self.apps.get_mut(i) {
                    a.hatch_state.main_hidden=false;a.window.set_visible(true);a.window.set_minimized(false);a.window.focus_window();a.dirty=true;
                }
            }
            UserEvent::HatchWork => {
                let i=self.hatch_index();
                if let Some(a)=i.and_then(|i|self.apps.get_mut(i)){a.show_hatch_work();}
            }
            UserEvent::HatchSelect(target) => {
                if let Some(i)=self.apps.iter().position(|a|u64::from(a.window.id())==target.window) {
                    let valid=self.apps[i].tabs.iter().any(|t|t.id==target.tab && (!target.right || t.right.is_some()));
                    if valid {
                        let previous=self.apps.iter().find(|a|a.hatch.as_ref().is_some_and(|h|h.visible) || a.hatch_state.badge.as_ref().is_some_and(|b|b.window.has_focus())).map(|a|a.hatch_state.foreground.clone());
                        for (j,a) in self.apps.iter_mut().enumerate(){if i!=j{a.hide_hatch_inner(false);}}
                        self.apps[i].open_hatch_target(target);
                        if let Some(previous)=previous {self.apps[i].hatch_state.foreground=previous;}
                        self.focused=Some(self.apps[i].window.id());
                    }
                }
            }
            UserEvent::BrowserRedirect(id,from,to)=>{
                for app in &mut self.apps {for tab in &mut app.tabs {for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                    if let app::Pane::Web(w)=pane {if w.tab.browser.as_ref().is_some_and(|b|b.identifier()==id) {
                        w.tab.shared.borrow_mut().redirect(&from,&to);app.dirty=true;
                    }}
                }}}
            },
            UserEvent::BrowserWork => {},
            UserEvent::Wake => {
                for a in self.apps.iter_mut() {
                    a.dirty = true;
                }
            }
            UserEvent::Hatch | UserEvent::HatchShow => {
                let show=matches!(ev,UserEvent::HatchShow);
                let i=self.hatch_index();
                if let Some(a)=i.and_then(|i|self.apps.get_mut(i)) {if show {a.show_hatch();}else{a.toggle_hatch();}}
            }
            UserEvent::HatchHide => {
                for a in self.apps.iter_mut().filter(|a|a.hatch.as_ref().is_some_and(|h|h.visible)) {a.hide_hatch();}
            }
            UserEvent::Access(e) => {
                let Some(i) = self.app_index(e.window_id) else { return };
                let a = &mut self.apps[i];
                let hatch=a.hatch.as_ref().is_some_and(|h|h.window.id()==e.window_id);
                let pip=a.pip.as_ref().is_some_and(|p|p.window.id()==e.window_id);
                let drawer=a.menu_drawer.window.as_ref().is_some_and(|d|d.window.id()==e.window_id);
                match e.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        if let Some((_, ad, _)) = self.access.iter_mut().find(|(id, _, _)| *id == e.window_id) {
                            ad.update_if_active(|| if pip {a.pip_access_tree()} else if drawer {a.menu_drawer_access_tree()} else if hatch {a.hatch_access_tree()} else {a.library_access_tree()});
                        }
                    }
                    accesskit_winit::WindowEvent::ActionRequested(req) => if pip {a.pip_access_action(req)} else if drawer {a.menu_drawer_access_action(req)} else if hatch {a.hatch_access_action(req)} else {a.library_access_action(req)},
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(level) = memory_pressure::poll() {
            tracing::warn!(?level, "system memory pressure: reclaiming disposable caches");
            for app in &mut self.apps { app.reclaim_memory(level); }
        }
        if let Some(a)=self.focused.and_then(|id|self.apps.iter().find(|a|a.window.id()==id)).or_else(||self.apps.first()) {
            crate::app_icon::select(a.behavior.app_icon);
            self.dock.tick(a.surface.signal,a.motion.reduced(),self.any_window_focused());
            if let Some(tray)=&mut self.tray {tray.refresh_icon(a.surface.signal);}
        }
        self.refresh_hatch_work();
        self.refresh_finish_work();
        if let Some(menu)=&self.application_menu {if let Some(a)=self.focused.and_then(|id|self.app_index(id)).and_then(|i|self.apps.get(i)).or(self.apps.first()){menu.refresh(a);}}
        // Requests the apps can't answer themselves: new windows, fronting.
        let mut spawn_from: Vec<usize> = Vec::new();
        let mut front: Vec<u64> = Vec::new();
        let mut sends: Vec<(usize, u64, send::Dest)> = Vec::new();
        for (i, a) in self.apps.iter_mut().enumerate() {
            if let Some((id, dest)) = a.send_request.take() {
                sends.push((i, id, dest));
            }
            if a.new_window_request {
                a.new_window_request = false;
                spawn_from.push(i);
            }
            if let Some(id) = a.front_request.take() {
                front.push(id);
            }
        }
        // Windows a restore asked for: one per saved session, each restored.
        let mut sessions: Vec<start::Session> = Vec::new();
        for a in self.apps.iter_mut() {
            sessions.append(&mut a.spawn_sessions);
        }
        for s in sessions {
            self.spawn_window(event_loop, Some(0));
            if let Some(a) = self.apps.last_mut() {
                a.last_session = Some(s);
                a.restore_session_pub();
                a.drop_birth = true;
            }
        }
        for i in spawn_from {
            self.spawn_window(event_loop, Some(i));
        }
        for (i, id, dest) in sends {
            self.send_tab(event_loop, i, id, dest);
        }
        for id in front {
            if let Some(a) = self.apps.iter_mut().find(|a| u64::from(a.window.id()) == id) {
                a.hatch_state.main_hidden=false;
                a.window.set_visible(true);
                a.window.focus_window();
            }
        }
        // PiP belongs to the process: replace its source, even when requested
        // by another nus window, instead of accumulating floating windows.
        let request=self.apps.iter_mut().enumerate().filter_map(|(i,a)|a.pip_request.take().map(|(tab,right)|(i,tab,right))).last();
        if let Some((owner,tab,right))=request.filter(|(owner,tab,right)| !self.apps[*owner].retarget_pip(*tab,*right)) {
            let mut existing=None;
            for a in &mut self.apps {if let Some(p)=a.pip.take(){if existing.is_none(){existing=Some((p.window,p.cur));}}}
            let previous=existing.as_ref().map(|(_,r)|*r);
            let window=if let Some((window,_))=existing {Some(window)} else {
                let mut attrs=Window::default_attributes().with_title("nus · picture in picture")
                    .with_window_icon(icon_default()).with_decorations(false)
                    .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
                    .with_resizable(crate::hatch_native::wayland()).with_visible(false).with_active(false)
                    .with_inner_size(winit::dpi::LogicalSize::new(480.0,270.0));
                if let Ok(pos)=self.apps[owner].window.outer_position(){attrs=attrs.with_position(pos);}
                match event_loop.create_window(attrs) {
                    Ok(window)=>{let adapter=accesskit_winit::Adapter::with_event_loop_proxy(event_loop,&window,self.proxy.clone());self.access.push((window.id(),adapter,0));Some(Arc::new(window))},
                    Err(e)=>{tracing::warn!("PiP window: {e}");None}
                }
            };
            if let Some(window)=window {
                self.apps[owner].attach_pip(window,tab,right,previous);
            }
        }
        debug_assert!(self.apps.iter().filter(|a|a.pip.is_some()).count()<=1,"one PiP per process");
        let live_ids:std::collections::HashSet<_>=self.apps.iter().flat_map(|a|std::iter::once(a.window.id()).chain(a.pip.as_ref().map(|p|p.window.id())).chain(a.hatch.as_ref().map(|h|h.window.id())).chain(a.menu_drawer.window.as_ref().map(|d|d.window.id())).chain(a.little.as_ref().map(|l|l.window.id()))).collect();
        self.access.retain(|(id,_,_)|live_ids.contains(id));
        for (app_i,a) in self.apps.iter_mut().enumerate() {
        if a.menu_drawer.request {
            a.menu_drawer.request=false;
            let (anchor,monitor,scale)=a.menu_geometry();let size=((380.0*scale)as u32,(520.0*scale)as u32);let pos=menu_drawer::place(anchor,monitor,size,(5.0*scale)as i32);
            #[allow(unused_mut)]
            let mut attrs=Window::default_attributes().with_title("nus · menu drawer").with_window_icon(icon_default()).with_decorations(false).with_resizable(false).with_visible(false).with_active(false).with_window_level(winit::window::WindowLevel::AlwaysOnTop).with_position(winit::dpi::PhysicalPosition::new(pos.0,pos.1)).with_inner_size(winit::dpi::PhysicalSize::new(size.0,size.1));
            #[cfg(windows)] {use winit::platform::windows::WindowAttributesExtWindows;attrs=attrs.with_skip_taskbar(true);}
            match event_loop.create_window(attrs){Ok(w)=>{let adapter=accesskit_winit::Adapter::with_event_loop_proxy(event_loop,&w,self.proxy.clone());self.access.push((w.id(),adapter,0));a.attach_menu_drawer(Arc::new(w));},Err(e)=>tracing::warn!("Menu drawer: {e}")}
        }
        a.hatch_ride();
        if std::mem::take(&mut a.pointer_reset) {
            a.window.set_cursor(winit::window::CursorIcon::Default);
        }
        if let Some(url) = a.little_request.take() {
            let attrs = Window::default_attributes()
                .with_title("nus · little")
                .with_window_icon(icon_default())
                .with_decorations(false)
                .with_visible(false)
                .with_inner_size(winit::dpi::LogicalSize::new(little::LITTLE_W, little::LITTLE_H));
            match event_loop.create_window(attrs) {
                Ok(w) => a.attach_little(Arc::new(w), &url),
                Err(e) => tracing::warn!("little window: {e}"),
            }
        }
        for badge in [true,false] {
            let needed=if badge {app_i==0 && a.hatch_state.badge.is_none()} else {a.behavior.hatch_dim && a.hatch_state.shade.is_none()};
            if needed {
                #[allow(unused_mut)]
                let mut attrs=Window::default_attributes().with_title(if badge {"nus · ongoing work"} else {"nus · backdrop"}).with_decorations(false).with_resizable(false).with_transparent(true).with_active(false).with_visible(false).with_window_level(if badge {winit::window::WindowLevel::AlwaysOnTop} else {winit::window::WindowLevel::Normal}).with_inner_size(winit::dpi::LogicalSize::new(330.0,30.0));
                #[cfg(windows)] {use winit::platform::windows::WindowAttributesExtWindows;attrs=attrs.with_skip_taskbar(true);}
                match event_loop.create_window(attrs) {Ok(w)=>a.attach_hatch_overlay(Arc::new(w),badge),Err(e)=>tracing::warn!("Hatch overlay: {e}")}
            }
        }
        if let Some(((x, y), (w, h))) = a.hatch_request.take() {
            #[allow(unused_mut)]
            let mut attrs = Window::default_attributes()
                .with_title("nus · hatch")
                .with_active(false)
                .with_window_icon(icon_default())
                .with_decorations(false)
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
                .with_resizable(true)
                .with_transparent(true)
                .with_min_inner_size(winit::dpi::LogicalSize::new(360.0,200.0))
                .with_visible(false)
                .with_position(winit::dpi::PhysicalPosition::new(x, y))
                .with_inner_size(winit::dpi::PhysicalSize::new(w, h));
            #[cfg(windows)]
            {
                use winit::platform::windows::WindowAttributesExtWindows;
                attrs = attrs.with_skip_taskbar(true);
            }
            match event_loop.create_window(attrs) {
                Ok(w) => {
                    let adapter=accesskit_winit::Adapter::with_event_loop_proxy(event_loop,&w,self.proxy.clone());
                    self.access.push((w.id(),adapter,0));
                    a.attach_hatch(Arc::new(w));
                },
                Err(e) => tracing::warn!("hatch window: {e}"),
            }
        }

        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(i) = self.app_index(id) else { return };
        let a = &mut self.apps[i];
        if a.menu_drawer.window.as_ref().is_some_and(|d|d.window.id()==id){
            if let Some((_,adapter,_))=self.access.iter_mut().find(|(wid,_,_)|*wid==id){adapter.process_event(&a.menu_drawer.window.as_ref().unwrap().window,&event);}
            a.menu_drawer_event(event);
            if let Some((_,adapter,_))=self.access.iter_mut().find(|(wid,_,_)|*wid==id){adapter.update_if_active(||a.menu_drawer_access_tree());}
            return;
        }
        if a.window.id() == id {
            if let Some((_, ad, _)) = self.access.iter_mut().find(|(wid, _, _)| *wid == id) {
                ad.process_event(&a.window, &event);
            }
            if let WindowEvent::Focused(true) = event {
                self.focused = Some(id);
            }
        }
        if a.hatch_state.badge.as_ref().is_some_and(|b|b.window.id()==id) {
            match event {
                WindowEvent::MouseInput{state:ElementState::Released,button:MouseButton::Left,..}=>{
                    if let Some((item, _))=a.hatch_state.completion.take() {
                        a.hatch_click(hatch::Hit::Job(item.target));
                    } else {
                        let previous=a.hatch_state.foreground.clone();a.show_hatch_work();a.hatch_state.foreground=previous;
                    }
                },
                WindowEvent::RedrawRequested|WindowEvent::ScaleFactorChanged{..}=>{if let Some(b)=&mut a.hatch_state.badge{b.text.clear();}a.hatch_badge_frame();},
                _=>{}
            }
            return;
        }
        if a.hatch_state.shade.as_ref().is_some_and(|b|b.window.id()==id) {
            match event {WindowEvent::MouseInput{state:ElementState::Pressed,..}=>{if !a.hatch.as_ref().is_some_and(|h|h.pinned){a.hide_hatch();}},WindowEvent::RedrawRequested=>a.hatch_shade(),_=>{}}
            return;
        }
        if a.hatch.as_ref().is_some_and(|h| h.window.id() == id) {
            if let Some((_,adapter,_))=self.access.iter_mut().find(|(wid,_,_)|*wid==id){adapter.process_event(&a.hatch.as_ref().unwrap().window,&event);}
            match event {
                WindowEvent::CloseRequested => a.hide_hatch(),
                WindowEvent::Focused(f) => a.hatch_focus(f),
                WindowEvent::Resized(s) => a.hatch_resized(s.width, s.height),
                WindowEvent::ScaleFactorChanged { .. } => {let s=a.hatch.as_ref().unwrap().window.inner_size();a.hatch_resized(s.width,s.height);},
                WindowEvent::Ime(winit::event::Ime::Commit(text)) => a.hatch_ime(&text),
                WindowEvent::CursorLeft {..} => {if let Some(h)=&mut a.hatch{h.pos=(-1.0,-1.0);}},
                WindowEvent::ModifiersChanged(m) => a.hatch_modifiers(m.state()),
                WindowEvent::KeyboardInput { event, .. } => a.hatch_key(&event),
                WindowEvent::CursorMoved { position, .. } => a.hatch_cursor((position.x as f32, position.y as f32)),
                WindowEvent::MouseInput { state, button, .. } => {
                    let pos = a.hatch.as_ref().map(|h| h.pos).unwrap_or((0.0, 0.0));
                    a.hatch_mouse(button, state, pos);
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let pos = a.hatch.as_ref().map(|h| h.pos).unwrap_or((0.0, 0.0));
                    a.hatch_wheel(delta, pos);
                }
                WindowEvent::RedrawRequested => a.hatch_frame(),
                _ => {}
            }
            return;
        }
        if a.little.as_ref().is_some_and(|l| l.window.id() == id) {
            match event {
                WindowEvent::CloseRequested => a.close_little(),
                WindowEvent::Focused(f) => a.little_focus(f),
                WindowEvent::Resized(s) => a.little_resized(s.width, s.height),
                WindowEvent::Moved(p) => a.little_moved(p.x, p.y),
                WindowEvent::ModifiersChanged(m) => a.little_modifiers(m.state()),
                WindowEvent::KeyboardInput { event, .. } => a.little_key(&event),
                WindowEvent::CursorMoved { position, .. } => {
                    a.little_pos = (position.x as f32, position.y as f32);
                    a.little_cursor(a.little_pos);
                }
                WindowEvent::MouseInput { state, button, .. } => a.little_mouse(button, state, a.little_pos),
                WindowEvent::MouseWheel { delta, .. } => a.little_wheel(delta, a.little_pos),
                WindowEvent::RedrawRequested => a.little_frame(),
                _ => {}
            }
            return;
        }
        if a.pip.as_ref().is_some_and(|p| p.window.id() == id) {
            if let Some((_,ad,_))=self.access.iter_mut().find(|(wid,_,_)|*wid==id){ad.process_event(&a.pip.as_ref().unwrap().window,&event);}
            match event {
                WindowEvent::CloseRequested => a.close_pip(),
                WindowEvent::Focused(f) => a.pip_focus(f),
                WindowEvent::Resized(s) => a.pip_resized(s.width, s.height),
                WindowEvent::Moved(p) => a.pip_moved(p.x, p.y),
                WindowEvent::KeyboardInput { event, .. } => a.pip_key(&crate::app::KeyIn::from(&event)),
                WindowEvent::ModifiersChanged(m)=>{if let Some(p)=&mut a.pip{p.mods=m.state();}},
                WindowEvent::MouseInput { state, button, .. } => a.pip_mouse(button, state),
                WindowEvent::CursorEntered { .. } => a.pip_cursor_entered(),
                WindowEvent::CursorLeft { .. } => a.pip_cursor_left(),
                WindowEvent::CursorMoved { position, .. } => a.pip_cursor_moved(position.x, position.y),
                WindowEvent::MouseWheel { delta, .. } => a.pip_wheel(delta),
                WindowEvent::PinchGesture {delta,..} => a.pip_pinch(delta),
                WindowEvent::ScaleFactorChanged {..} => a.pip_scale_changed(),
                WindowEvent::RedrawRequested => {a.pip_frame();if let Some((_,ad,_))=self.access.iter_mut().find(|(wid,_,_)|*wid==id){ad.update_if_active(||a.pip_access_tree());}},
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                // Keep the owning App/PTYs alive; closing a window is not Quit.
                if a.behavior.hatch_background && !private::enabled() {
                    a.hatch_state.main_hidden=true;
                    a.window.set_visible(false);
                    a.save_session();
                    return;
                }
                if self.apps.len() == 1 {
                    event_loop.exit();
                } else {
                    let a = self.apps.remove(i);
                    self.access.retain(|(wid, _, _)| *wid != a.window.id());
                    drop(a);
                    if !private::enabled() && !self.apps.iter().any(|a|a.hotkey.is_some()) {
                        if let Some(a)=self.apps.first_mut() { a.hotkey=Some(hotkey::Hotkey::register(a.behavior.hatch_hotkey,self.proxy.clone())); }
                    }
                }
            }
            WindowEvent::Resized(s) => a.resize(s.width, s.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => a.set_scale(scale_factor as f32),
            WindowEvent::Moved(p) => a.window_moved(p.x, p.y),
            WindowEvent::ThemeChanged(t) => {
                if a.behavior.follow_os_theme && std::env::var_os("NUS_MODE").is_none() {
                    let mode = match t {
                        winit::window::Theme::Light => nus_render::Mode::Paper,
                        winit::window::Theme::Dark => nus_render::Mode::Ink,
                    };
                    a.set_mode(mode);
                }
            }
            WindowEvent::Focused(f) => a.focus_changed(f),
            WindowEvent::ModifiersChanged(m) => a.modifiers(m.state()),
            WindowEvent::KeyboardInput { event, .. } => {
                a.key(&event);
                a.dirty = true;
            }
            WindowEvent::CursorMoved { position, .. } => a.mouse_moved(position.x as f32, position.y as f32),
            WindowEvent::CursorEntered { .. } => a.cursor_entered(),
            WindowEvent::CursorLeft { .. } => a.cursor_left(),
            WindowEvent::MouseInput { state, button, .. } => {
                a.mouse_button(button, state);
                a.dirty = true;
            }
            WindowEvent::MouseWheel { delta,phase,.. } => {a.wheel(delta);if phase==winit::event::TouchPhase::Ended{a.timeline_detent();}},
            WindowEvent::Touch(t) => a.touch(t.id, t.phase, t.location.x as f32, t.location.y as f32),
            WindowEvent::RedrawRequested => a.redraw(),
            _ => {}
        }
    }
}

/// The Chromium version of the bundled CEF, from cef_version_info.
fn chromium_version() -> String {
    format!(
        "{}.{}.{}.{}",
        cef::sys::CHROME_VERSION_MAJOR,
        cef::sys::CHROME_VERSION_MINOR,
        cef::sys::CHROME_VERSION_BUILD,
        cef::sys::CHROME_VERSION_PATCH
    )
}

/// Launched as a macOS app (from Finder, the Dock, `open`), a process gets `/`
/// for a working directory and launchd's bare PATH. nus keeps its profile
/// relative to the working directory, so a bundle moves to
/// its installation under ~/Library/Application Support/nus/installs first;
/// and it takes PATH from the login
/// shell, as a terminal launched from a terminal would have it, so language
/// servers and tools installed with Homebrew are found.
#[cfg(target_os = "macos")]
fn settle_as_app(_dock: &mut dock::Dock) {
    if private::enabled() { return; }
    let in_bundle = std::env::current_exe().map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/")).unwrap_or(false);
    if !in_bundle {
        return;
    }
    if let Some(dir) = std::env::var_os("NUS_SHOT_DIR").filter(|_| std::env::var_os("NUS_SHOT").is_some()) {
        std::fs::create_dir_all(&dir).expect("create screenshot profile directory");
        std::env::set_current_dir(&dir).expect("use screenshot profile directory");
    } else if let Some(home) = std::env::var_os("HOME") {
        let base = std::path::Path::new(&home).join("Library/Application Support/nus");
        let exe = std::env::current_exe().expect("locate installed app");
        let bundle = exe.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).expect("app bundle");
        let dir = install::bundle_root(&base, bundle).expect("create isolated installation profile");
        std::env::set_current_dir(&dir).expect("use installation profile");
    }
}

#[cfg(target_os = "macos")]
fn prepare_app_environment() {
    if private::enabled() || !std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/")) { return; }
    // What Chromium is told on its command line comes from the prefs, and
    // has to be known before the browser process starts.
    // Only isolated native checks may deliberately hold startup open, proving
    // that a slow launch loops and a fast launch never waits for an animation.
    if std::env::var_os("NUS_SHOT").is_some() {
        if let Some(ms) = std::env::var("NUS_DOCK_LAUNCH_TEST_MS").ok().and_then(|s| s.parse::<u64>().ok()) {
            let until = std::time::Instant::now() + Duration::from_millis(ms.min(3000));
            while std::time::Instant::now() < until { dock::pump_launch(); }
        }
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = std::process::Command::new(&shell).args(["-l", "-c", "printf %s \"$PATH\""])
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().and_then(|mut child| {
            let started = std::time::Instant::now();
            loop {
                match child.try_wait()? {
                    Some(_) => return child.wait_with_output(),
                    None if crate::clock::since(started) < Duration::from_secs(5) => dock::pump_launch(),
                    None => { let _ = child.kill(); let _ = child.wait(); return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "login PATH lookup timed out")); }
                }
            }
        });
    if let Ok(out) = output {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !path.is_empty() {
            std::env::set_var("PATH", path);
        }
    }
    if let Ok(exe)=std::env::current_exe() {
        if let Some(contents)=exe.parent().and_then(|p|p.parent()) {
            let bin=contents.join("Resources/bin");
            let mut dirs=vec![bin];
            if let Some(home)=std::env::var_os("HOME") {dirs.push(std::path::PathBuf::from(home).join(".local/bin"));}
            dirs.extend(std::env::var_os("PATH").map(|p|std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
            if let Ok(path)=std::env::join_paths(dirs) {std::env::set_var("PATH",path);}
        }
    }

}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("nus {} ({})", env!("NUS_BUILD_VERSION"), env!("NUS_BUILD_REVISION"));
        return ExitCode::SUCCESS;
    }
    if std::env::args().nth(1).as_deref() == Some("--compatibility") {
        println!("{}", serde_json::to_string(&compatibility::contract()).unwrap());
        return ExitCode::SUCCESS;
    }
    let child_process = std::env::args().any(|a| a == "--type" || a.starts_with("--type="));
    let urls = little::urls_from_args();
    perf::start();
    let _private_root = match private::prepare() {
        Ok(root) => root,
        Err(_) => { eprintln!("Could not create an isolated incognito session."); return ExitCode::FAILURE; }
    };
    perf::startup(perf::StartupMark::PrivateReady);
    let mut dock = dock::Dock::bootstrap();
    #[cfg(target_os = "macos")]
    if !child_process { settle_as_app(&mut dock); }
    perf::startup(perf::StartupMark::DockReady);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    #[cfg(not(target_os = "macos"))]
    if !child_process {
        if let Err(e) = distribution::settle() {
            eprintln!("Could not open the nus data directory: {e}");
            return ExitCode::FAILURE;
        }
    }
    if !child_process && !private::enabled() && little::handoff(&urls) {
        return ExitCode::SUCCESS;
    }
    let profile_guard = if child_process { None } else {
        match compatibility::start() {
            Ok(guard) => Some(guard),
            Err(e) => {
                eprintln!("nus could not safely open this profile: {e}");
                if std::env::var_os("NUS_SHOT").is_none() {
                    rfd::MessageDialog::new().set_title("nus profile preserved")
                        .set_description(format!("nus could not safely open this profile. No settings were loaded.\n\n{e}"))
                        .set_level(rfd::MessageLevel::Error).show();
                }
                return ExitCode::FAILURE;
            }
        }
    };
    if !child_process {
        dock.begin_launch(prefs::Prefs::load().motion.unwrap_or_default().reduced());
        prefs::apply_start_switches();
        #[cfg(target_os = "macos")]
        prepare_app_environment();
    }

    if child_process {
        browser_runtime::load_library();
        let args = Args::new();
        let mut cef_app = browser::AppBuilder::new(browser::AppHandler);
        let ret = execute_process(Some(args.as_main_args()), Some(&mut cef_app), std::ptr::null_mut());
        return ExitCode::from(ret.max(0) as u8);
    }

    // The profile lock serializes ownership; a racing launch cannot overwrite instance credentials.
    let (urls_rx, port, inbound_tx) = match if private::enabled() {
        let (tx, rx) = std::sync::mpsc::channel();
        little::Claim::Primary(rx, 0, tx)
    } else { little::claim(&urls) } {
        little::Claim::Primary(rx, port, tx) => (rx, port, tx),
    };
    mercury::observe_installation();
    protected_state::migrate(&std::env::current_dir().unwrap_or_default().join("profile"));

    // Native windows can open before the browser engine is needed.
    perf::startup(perf::StartupMark::CefDeferred);

    let mut event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    // CEF's windows (native DevTools) ask NSApp about -sendEvent:; winit's
    // application class has to answer before CEF starts (cef_app_mac.rs).
    #[cfg(target_os = "macos")]
    cef_app_mac::install();
    perf::startup(perf::StartupMark::EventLoopReady);
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();
    browser_runtime::set_proxy(proxy.clone());
    let mut host = Host { proxy, apps: Vec::new(), access: Vec::new(), made: 0, focused: None, inbound: inbound_tx, tray: None, application_menu: None, work_at: None, hatch_owner: None, dock, finish: finish_work::FinishWork::new(finish_work_native::native()) };
    let mut urls_rx = Some(urls_rx);
    let _ = port;
    let mut launch_acknowledged = false;
    let code = loop {
        browser_runtime::pump();
        let background=!host.apps.is_empty() && host.apps.iter().all(|a| a.hatch_state.main_hidden && a.hatch.as_ref().is_none_or(|h|!h.visible) && a.little.is_none() && a.pip.is_none());
        // Input, CEF deadlines and worker completions wake idle maintenance.
        // Preserve the existing animated cadence; a positive pump timeout also
        // lets AppKit return control when redraw events are continuously queued.
        let animated=host.apps.iter().any(|a|a.dirty);
        let arrival=host.apps.iter().any(|a|a.arriving());
        let maintenance=Duration::from_millis(if arrival || (animated && !background) {2} else {50});
        let maintenance=host.apps.iter().filter_map(|a|a.browser_frame_wait()).fold(maintenance,Duration::min);
        let wait=browser_runtime::wait(maintenance).max(Duration::from_millis(1));
        event_loop.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now()+wait));
        let status = event_loop.pump_app_events(Some(wait), &mut host);
        if let PumpStatus::Exit(code) = status {
            host.release_finish_work();
            break code;
        }
        // A NUS_SHOT script that has run out: leave, the pictures are on disk.
        if host.apps.iter().any(|a| a.shot.as_ref().is_some_and(|s| s.done)) {
            break 0;
        }
        let _turn = perf::scope("ui_turn_work");
        host.share_registry();
        // URLs from other launches go to the window the user was last in.
        if let Some(rx) = urls_rx.as_ref() {
            let focused = host.focused;
            let idx = host.apps.iter().position(|a| Some(a.window.id()) == focused).or(if host.apps.is_empty() { None } else { Some(0) });
            let mut rest = Vec::new();
            for inbound in rx.try_iter() {
                match inbound {
                    // An assistant's hook: to whichever window has its pane.
                    little::Inbound::Request(req) if req.cmd == "agent" => {
                        let reply = if !req.origin.allows(&req.cmd) {
                            serde_json::json!({ "ok": false, "error": "agent is not available from the phone" })
                        } else if private::enabled() {
                            serde_json::json!({ "ok": false, "error": "unavailable in incognito" })
                        } else {
                            match agent::route(&mut host.apps, &req.args) {
                                Ok(v) => serde_json::json!({ "ok": true, "result": v }),
                                Err(e) => serde_json::json!({ "ok": false, "error": e }),
                            }
                        };
                        let _ = req.reply.send(reply);
                    }
                    other => rest.push(other),
                }
            }
            if let Some(a) = idx.and_then(|i| host.apps.get_mut(i)) {
                for inbound in rest {
                    match inbound {
                        little::Inbound::Url(url) => {
                            a.open_little(&url);
                        }
                        little::Inbound::Request(req) => a.remote_request(req),
                    }
                    a.dirty = true;
                }
            }
        }
        for a in host.apps.iter_mut() {
            if a.registered_tabs != a.tabs.len() {
                a.register_window();
            }
            a.tick();
            if !launch_acknowledged {
                if let Some(guard) = &profile_guard {
                    match guard.healthy() {
                        Ok(()) => launch_acknowledged = true,
                        Err(e) => tracing::warn!("Could not record launch health: {e}"),
                    }
                }
            }
            a.shot_tick();
            if !a.arriving() {
            a.tend_updates();
            a.poll_deferred();
            a.poll_page_menus();
            a.tend_tree();
            a.tend_touch();
            a.poll_loop();
            a.process_requests();
            a.apply_term_resizes(false);
            a.begin_frames();
            a.pip_frame();
            a.little_frame();
            a.hatch_frame();
            if a.dirty {
                a.hatch_redraw();
            }
            // pump() consumes the change it reports, so latch it into dirty
            // rather than letting redraw() pump a second time and see nothing.
            if a.pump() {
                a.dirty = true;
            }
            }
            if a.dirty && !a.hatch_state.main_hidden {
                a.redraw();
            }
            if let Some((_, ad, frame)) = host.access.iter_mut().find(|(id, _, _)| *id == a.window.id()) {
                if a.frames != *frame {
                    *frame = a.frames;
                    ad.update_if_active(|| a.library_access_tree());
                }
            }
        }
    };
    host.dock.prepare_quit();
    // Every window's tabs go in the file: the first window's session carries the rest.
    let others: Vec<start::Session> = host.apps.iter().skip(1).map(|a| a.session_snapshot()).collect();
    if let Some(a) = host.apps.first_mut() {
        a.other_sessions = others;
        a.save_session();
        a.sync_at_quit();
    }
    // Held shells: an idle prompt is let go, a running command is kept.
    // Our own shells go, with everything under them: no strays.
    for a in host.apps.iter_mut() {
        a.release_idle_held();
        a.reap_local_shells();
    }
    host.apps.clear();
    // CEF owns native browser references until its asynchronous close callback.
    let closing = std::time::Instant::now();
    while browser::live_count() > 0 && closing.elapsed() < Duration::from_secs(3) {
        cef::do_message_loop_work();
        std::thread::sleep(Duration::from_millis(5));
    }
    containers::shutdown();
    containers::release_private_context();
    if browser_runtime::ready() {
        cef::shutdown();
    }
    if !private::enabled() { browser_cache::maintain(std::path::Path::new("profile"), browser_cache::BUDGET); }
    if private::enabled() {
        // Windows cannot remove the current working directory.
        let _ = std::env::set_current_dir(std::env::temp_dir());
    }
    ExitCode::from(code as u8)
}

/// The bundled desktop mark for secondary windows, before theme recolouring.
fn icon_default() -> Option<winit::window::Icon> {
    let rgba = nus_render::dock_icon::render(64, nus_render::theme::hex(0xc8102e), nus_render::dock_icon::Face::Newsreader);
    winit::window::Icon::from_rgba(rgba, 64, 64).ok()
}
