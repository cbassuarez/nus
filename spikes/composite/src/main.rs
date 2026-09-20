//! Spike 4: one compositor, terminal + browser panes, Broadsheet chrome.
//! See docs/SPIKES.md.

mod access;
mod shell;
mod termui;
mod predict;
mod webui;
mod welcome;
mod themes;
mod macos;
mod dock;
mod downloads;
mod sidebar;
mod fonts;
mod prompt;
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
mod editor;
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
mod app;
mod browser;
mod little;
mod pip;
mod prefs;
mod reader;
mod settings;
mod sound;
mod shot;
mod splash;
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
    Wake,
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
    work_at: Option<std::time::Instant>,
    hatch_owner: Option<WindowId>,
    dock: dock::Dock,
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

    fn refresh_hatch_work(&mut self) {
        let owner=self.apps.iter().filter_map(|a|a.hatch.as_ref()).find(|h|h.visible&&!h.hiding).map(|h|h.window.id());
        if self.hatch_owner==owner && self.work_at.is_some_and(|at|at.elapsed().as_millis()<200){return;}
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
                    self.dock.attention(a.motion.reduced());
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
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("window: {e}");
                return;
            }
        };
        // The adapter must exist before the window is first shown.
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        crate::macos::prepare_window(&window);
        // The folder the asking window works in, for a shell born here.
        let born_in = from.and_then(|i| self.apps.get(i)).and_then(|a| a.workspace.as_ref().map(|w| w.to_string_lossy().to_string()).or_else(|| a.focused_cwd()));
        match App::new(window.clone(), self.proxy.clone(), secondary, self.made, born_in) {
            Ok(mut a) => {
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

    /// Tell every app about every window.
    fn share_registry(&mut self) {
        let entries: Vec<windows::Entry> = self
            .apps
            .iter()
            .map(|a| windows::Entry { id: u64::from(a.window.id()), name: a.window_name(), tabs: a.tabs.len(), ordinal: a.ordinal, colour: a.container_colour() })
            .collect();
        for a in self.apps.iter_mut() {
            if a.windows != entries {
                a.windows = entries.clone();
                a.dirty = true;
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for Host {
    fn exiting(&mut self, _: &ActiveEventLoop) {
        // AppKit's native Quit can terminate inside the event pump, before
        // main reaches its normal shutdown path.
        self.dock.prepare_quit();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.apps.is_empty() {
            self.spawn_window(event_loop, None);
        }
        if self.tray.is_none() && hatch_native::interactive() {
            self.tray = hatch_tray::Tray::new(self.proxy.clone());
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::DockAttention => {
                if let Some(a)=self.apps.first(){self.dock.attention(a.motion.reduced());}
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
                let drawer=a.menu_drawer.window.as_ref().is_some_and(|d|d.window.id()==e.window_id);
                match e.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        if let Some((_, ad, _)) = self.access.iter_mut().find(|(id, _, _)| *id == e.window_id) {
                            ad.update_if_active(|| if drawer {a.menu_drawer_access_tree()} else if hatch {a.hatch_access_tree()} else {a.access_tree()});
                        }
                    }
                    accesskit_winit::WindowEvent::ActionRequested(req) => if drawer {a.menu_drawer_access_action(req)} else if hatch {a.hatch_access_action(req)} else {a.access_action(req)},
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(a)=self.focused.and_then(|id|self.apps.iter().find(|a|a.window.id()==id)).or_else(||self.apps.first()) {
            self.dock.tick(a.surface.signal,a.motion.reduced());
            if let Some(tray)=&mut self.tray {tray.refresh_icon(a.surface.signal);}
        }
        self.refresh_hatch_work();
        // Requests the apps can't answer themselves: new windows, fronting.
        let mut spawn_from: Vec<usize> = Vec::new();
        let mut front: Vec<u64> = Vec::new();
        for (i, a) in self.apps.iter_mut().enumerate() {
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
        for id in front {
            if let Some(a) = self.apps.iter_mut().find(|a| u64::from(a.window.id()) == id) {
                a.hatch_state.main_hidden=false;
                a.window.set_visible(true);
                a.window.focus_window();
            }
        }
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
        if let Some(p) = a.pointer_request.take() {
            let cursor = match p {
                settings::Pointer::System => winit::window::Cursor::Icon(winit::window::CursorIcon::Default),
                settings::Pointer::InkArrow | settings::Pointer::SignalDot => {
                    let (rgba, hot) = a.pointer_image(p);
                    match winit::window::CustomCursor::from_rgba(rgba, 32, 32, hot.0, hot.1) {
                        Ok(src) => winit::window::Cursor::Custom(event_loop.create_custom_cursor(src)),
                        Err(_) => winit::window::Cursor::Icon(winit::window::CursorIcon::Default),
                    }
                }
            };
            a.window.set_cursor(cursor);
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
        if let Some((tab, right)) = a.pip_request.take() {
            let attrs = Window::default_attributes()
                .with_title("nus · pip")
                .with_window_icon(icon_default())
                .with_decorations(false)
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
                .with_resizable(false)
                .with_visible(false)
                .with_inner_size(winit::dpi::LogicalSize::new(240.0, 135.0));
            match event_loop.create_window(attrs) {
                Ok(w) => a.attach_pip(Arc::new(w), tab, right),
                Err(e) => tracing::warn!("pip window: {e}"),
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
            match event {
                WindowEvent::CloseRequested => a.close_pip(),
                WindowEvent::Focused(f) => a.pip_focus(f),
                WindowEvent::Resized(s) => a.pip_resized(s.width, s.height),
                WindowEvent::Moved(p) => a.pip_moved(p.x, p.y),
                WindowEvent::KeyboardInput { event, .. } => a.pip_key(&event),
                WindowEvent::MouseInput { state, button, .. } => a.pip_mouse(button, state),
                WindowEvent::CursorEntered { .. } => a.pip_cursor_entered(),
                WindowEvent::CursorMoved { .. } => a.pip_cursor_moved(),
                WindowEvent::MouseWheel { delta, .. } => a.pip_wheel(delta),
                WindowEvent::RedrawRequested => a.pip_frame(),
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                // Keep the owning App/PTYs alive; closing a window is not Quit.
                if a.behavior.hatch_background {
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
                    if !self.apps.iter().any(|a|a.hotkey.is_some()) {
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
            WindowEvent::CursorLeft { .. } => a.cursor_left(),
            WindowEvent::MouseInput { state, button, .. } => {
                a.mouse_button(button, state);
                a.dirty = true;
            }
            WindowEvent::MouseWheel { delta, .. } => a.wheel(delta),
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
/// ~/Library/Application Support/nus first; and it takes PATH from the login
/// shell, as a terminal launched from a terminal would have it, so language
/// servers and tools installed with Homebrew are found.
#[cfg(target_os = "macos")]
fn settle_as_app(dock: &mut dock::Dock) {
    let in_bundle = std::env::current_exe().map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/")).unwrap_or(false);
    if !in_bundle {
        return;
    }
    if let Some(dir) = std::env::var_os("NUS_SHOT_DIR").filter(|_| std::env::var_os("NUS_SHOT").is_some()) {
        std::fs::create_dir_all(&dir).expect("create screenshot profile directory");
        std::env::set_current_dir(&dir).expect("use screenshot profile directory");
    } else if let Some(home) = std::env::var_os("HOME") {
        let dir = std::path::Path::new(&home).join("Library/Application Support/nus");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::env::set_current_dir(&dir);
    }
    dock.begin_launch(prefs::Prefs::load().motion.unwrap_or_default().reduced());
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
                    None if started.elapsed() < Duration::from_secs(5) => dock::pump_launch(),
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
    let mut dock = dock::Dock::bootstrap();
    #[cfg(target_os = "macos")]
    settle_as_app(&mut dock);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // macOS links CEF as a framework inside the bundle rather than a dylib on
    // the search path, so it has to be loaded before any CEF call — and the
    // loader has to outlive all of them.
    #[cfg(target_os = "macos")]
    let _cef_library = {
        let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), false);
        assert!(loader.load(), "could not load the CEF framework from the bundle");
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let cmd = args.as_cmd_line().unwrap();
    let is_browser_process = cmd.has_switch(Some(&"type".into())) != 1;
    let mut cef_app = browser::AppBuilder::new(browser::AppHandler);
    let ret = execute_process(Some(args.as_main_args()), Some(&mut cef_app), std::ptr::null_mut());
    if !is_browser_process {
        return ExitCode::from(ret.max(0) as u8);
    }
    assert_eq!(ret, -1, "browser process must not be executed here");

    // One instance: a second launch hands its URLs to the first and exits.
    let urls = little::urls_from_args();
    let (urls_rx, port, inbound_tx) = match little::claim(&urls) {
        little::Claim::HandedOff => return ExitCode::SUCCESS,
        little::Claim::Primary(rx, port, tx) => (rx, port, tx),
    };

    let profile = std::env::current_dir().unwrap().join("profile");
    let cache = profile.clone();
    let root = cache.clone();
    let settings = Settings {
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        // Brands "Google Chrome" in Sec-CH-UA; sites treat bare "Chromium" as a bot.
        user_agent_product: format!("Chrome/{}", chromium_version()).as_str().into(),
        root_cache_path: root.to_string_lossy().as_ref().into(),
        cache_path: cache.to_string_lossy().as_ref().into(),
        ..Default::default()
    };
    assert_eq!(
        initialize(Some(args.as_main_args()), Some(&settings), Some(&mut cef_app), std::ptr::null_mut()),
        1
    );

    let mut event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();
    let mut host = Host { proxy, apps: Vec::new(), access: Vec::new(), made: 0, focused: None, inbound: inbound_tx, tray: None, work_at: None, hatch_owner: None, dock };
    let mut urls_rx = Some(urls_rx);
    let _ = port;
    let code = loop {
        do_message_loop_work();
        let background=!host.apps.is_empty() && host.apps.iter().all(|a| a.hatch_state.main_hidden && a.hatch.as_ref().is_none_or(|h|!h.visible) && a.little.is_none() && a.pip.is_none());
        let wait=Duration::from_millis(if background {50} else {2});
        event_loop.set_control_flow(if background {ControlFlow::WaitUntil(std::time::Instant::now()+wait)} else {ControlFlow::Poll});
        let status = event_loop.pump_app_events(Some(wait), &mut host);
        if let PumpStatus::Exit(code) = status {
            break code;
        }
        // A NUS_SHOT script that has run out: leave, the pictures are on disk.
        if host.apps.iter().any(|a| a.shot.as_ref().is_some_and(|s| s.done)) {
            break 0;
        }
        host.share_registry();
        // URLs from other launches go to the window the user was last in.
        if let Some(rx) = urls_rx.as_ref() {
            let focused = host.focused;
            let idx = host.apps.iter().position(|a| Some(a.window.id()) == focused).or(if host.apps.is_empty() { None } else { Some(0) });
            if let Some(a) = idx.and_then(|i| host.apps.get_mut(i)) {
                for inbound in rx.try_iter() {
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
            a.shot_tick();
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
            if a.dirty && !a.hatch_state.main_hidden {
                a.redraw();
            }
            if let Some((_, ad, frame)) = host.access.iter_mut().find(|(id, _, _)| *id == a.window.id()) {
                if a.frames != *frame {
                    *frame = a.frames;
                    ad.update_if_active(|| a.access_tree());
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
    for a in host.apps.iter_mut() {
        a.release_idle_held();
    }
    host.apps.clear();
    cef::shutdown();
    ExitCode::from(code as u8)
}

/// The bundled desktop mark for secondary windows, before theme recolouring.
fn icon_default() -> Option<winit::window::Icon> {
    let rgba = nus_render::dock_icon::render(64, nus_render::theme::hex(0xc8102e), nus_render::dock_icon::Face::Newsreader);
    winit::window::Icon::from_rgba(rgba, 64, 64).ok()
}
