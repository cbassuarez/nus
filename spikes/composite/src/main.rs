//! Spike 4: one compositor, terminal + browser panes, Broadsheet chrome.
//! See docs/SPIKES.md.

mod access;
mod shell;
mod termui;
mod predict;
mod webui;
mod welcome;
mod themes;
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
mod anim;
mod app;
mod browser;
mod little;
mod pip;
mod prefs;
mod reader;
mod settings;
mod sound;
mod splash;
mod start;
mod surface;
mod theme_edit;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use cef::args::Args;
use cef::*;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Window, WindowId};

use app::App;

#[derive(Debug)]
pub enum UserEvent {
    Wake,
    Access(accesskit_winit::Event),
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
}

impl Host {
    fn app_index(&self, id: WindowId) -> Option<usize> {
        self.apps.iter().position(|a| a.window.id() == id || a.little.as_ref().is_some_and(|l| l.window.id() == id) || a.pip.as_ref().is_some_and(|p| p.window.id() == id))
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
            let rgba = nus_render::icon::app_icon(64, nus_render::theme::hex(0x141414), nus_render::theme::hex(0xc8102e));
            winit::window::Icon::from_rgba(rgba, 64, 64).ok()
        };
        let mut attrs = Window::default_attributes()
            .with_title("nus")
            .with_window_icon(icon.clone())
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(false)
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
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("window: {e}");
                return;
            }
        };
        // The adapter must exist before the window is first shown.
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        window.set_visible(true);
        match App::new(window.clone(), self.proxy.clone(), secondary, self.made) {
            Ok(mut a) => {
                a.fullscreen = start == settings::WindowStart::Fullscreen;
                if let Some(parent) = from.and_then(|i| self.apps.get(i)) {
                    a.container = parent.container.clone();
                    a.register_window();
                }
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
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.apps.is_empty() {
            self.spawn_window(event_loop, None);
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wake => {
                for a in self.apps.iter_mut() {
                    a.dirty = true;
                }
            }
            UserEvent::Access(e) => {
                let Some(i) = self.apps.iter().position(|a| a.window.id() == e.window_id) else { return };
                let a = &mut self.apps[i];
                match e.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        if let Some((_, ad, _)) = self.access.iter_mut().find(|(id, _, _)| *id == e.window_id) {
                            ad.update_if_active(|| a.access_tree());
                        }
                    }
                    accesskit_winit::WindowEvent::ActionRequested(req) => a.access_action(req),
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
        for i in spawn_from {
            self.spawn_window(event_loop, Some(i));
        }
        for id in front {
            if let Some(a) = self.apps.iter().find(|a| u64::from(a.window.id()) == id) {
                a.window.focus_window();
            }
        }
        for a in self.apps.iter_mut() {
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
        if a.window.id() == id {
            if let Some((_, ad, _)) = self.access.iter_mut().find(|(wid, _, _)| *wid == id) {
                ad.process_event(&a.window, &event);
            }
            if let WindowEvent::Focused(true) = event {
                self.focused = Some(id);
            }
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
                // The last window closing ends the app and saves the session;
                // any other just goes.
                if self.apps.len() == 1 {
                    event_loop.exit();
                } else {
                    let a = self.apps.remove(i);
                    self.access.retain(|(wid, _, _)| *wid != a.window.id());
                    drop(a);
                }
            }
            WindowEvent::Resized(s) => a.resize(s.width, s.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => a.set_scale(scale_factor as f32),
            WindowEvent::Moved(p) => a.window_moved(p.x, p.y),
            WindowEvent::ThemeChanged(t) => {
                if a.behavior.follow_os_theme {
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

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

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
    let (urls_rx, port) = match little::claim(&urls) {
        little::Claim::HandedOff => return ExitCode::SUCCESS,
        little::Claim::Primary(rx, port) => (rx, port),
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
    let mut host = Host { proxy, apps: Vec::new(), access: Vec::new(), made: 0, focused: None };
    let mut urls_rx = Some(urls_rx);
    let _ = port;
    let code = loop {
        do_message_loop_work();
        let status = event_loop.pump_app_events(Some(Duration::from_millis(2)), &mut host);
        if let PumpStatus::Exit(code) = status {
            break code;
        }
        host.share_registry();
        // URLs from other launches go to the window the user was last in.
        if let Some(rx) = urls_rx.as_ref() {
            let focused = host.focused;
            let idx = host.apps.iter().position(|a| Some(a.window.id()) == focused).or(if host.apps.is_empty() { None } else { Some(0) });
            if let Some(a) = idx.and_then(|i| host.apps.get_mut(i)) {
                for url in rx.try_iter() {
                    a.open_little(&url);
                    a.dirty = true;
                }
            }
        }
        for a in host.apps.iter_mut() {
            if a.registered_tabs != a.tabs.len() {
                a.register_window();
            }
            a.tick();
            a.process_requests();
            a.apply_term_resizes(false);
            a.begin_frames();
            a.pip_frame();
            a.little_frame();
            // pump() consumes the change it reports, so latch it into dirty
            // rather than letting redraw() pump a second time and see nothing.
            if a.pump() {
                a.dirty = true;
            }
            if a.dirty {
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
    if let Some(a) = host.apps.first() {
        a.save_session();
    }
    host.apps.clear();
    cef::shutdown();
    ExitCode::from(code as u8)
}

/// The bundled icon for secondary windows, before the app recolours it.
fn icon_default() -> Option<winit::window::Icon> {
    let rgba = nus_render::icon::app_icon(64, nus_render::theme::hex(0x141414), nus_render::theme::hex(0xc8102e));
    winit::window::Icon::from_rgba(rgba, 64, 64).ok()
}
