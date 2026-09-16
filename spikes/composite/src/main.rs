//! Spike 4: one compositor, terminal + browser panes, Broadsheet chrome.
//! See docs/SPIKES.md.

mod access;
mod anim;
mod app;
mod browser;
mod little;
mod pip;
mod reader;
mod settings;
mod start;
mod surface;

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
    app: Option<App>,
    /// AccessKit adapter for the main window; the tree is rebuilt after
    /// every frame that changed something.
    access: Option<accesskit_winit::Adapter>,
    access_frame: u64,
}

impl ApplicationHandler<UserEvent> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("nus")
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        // The adapter must exist before the window is first shown.
        self.access = Some(accesskit_winit::Adapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone()));
        window.set_visible(true);
        match App::new(window, self.proxy.clone()) {
            Ok(a) => self.app = Some(a),
            Err(e) => {
                tracing::error!("init: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        let Some(a) = &mut self.app else { return };
        match ev {
            UserEvent::Wake => a.dirty = true,
            UserEvent::Access(e) => match e.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    if let Some(ad) = self.access.as_mut() {
                        ad.update_if_active(|| a.access_tree());
                    }
                }
                accesskit_winit::WindowEvent::ActionRequested(req) => a.access_action(req),
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
            },
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(a) = self.app.as_mut() else { return };
        if let Some(url) = a.little_request.take() {
            let attrs = Window::default_attributes()
                .with_title("nus · little")
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(a) = self.app.as_mut() else { return };
        if a.window.id() == id {
            if let Some(ad) = self.access.as_mut() {
                ad.process_event(&a.window, &event);
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => a.resize(s.width, s.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => a.set_scale(scale_factor as f32),
            WindowEvent::Moved(p) => a.window_moved(p.x, p.y),
            WindowEvent::ThemeChanged(t) => {
                if a.behavior.follow_os_theme {
                    a.set_theme(match t {
                        winit::window::Theme::Light => nus_render::Theme::paper(),
                        winit::window::Theme::Dark => nus_render::Theme::ink(),
                    })
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
    let urls_rx = match little::claim(&urls) {
        little::Claim::HandedOff => return ExitCode::SUCCESS,
        little::Claim::Primary(rx) => rx,
    };

    let profile = std::env::current_dir().unwrap().join("profile");
    let settings = Settings {
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        // Brands "Google Chrome" in Sec-CH-UA; sites treat bare "Chromium" as a bot.
        user_agent_product: format!("Chrome/{}", chromium_version()).as_str().into(),
        root_cache_path: profile.to_string_lossy().as_ref().into(),
        cache_path: profile.to_string_lossy().as_ref().into(),
        ..Default::default()
    };
    assert_eq!(
        initialize(Some(args.as_main_args()), Some(&settings), Some(&mut cef_app), std::ptr::null_mut()),
        1
    );

    let mut event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();
    let mut host = Host { proxy, app: None, access: None, access_frame: 0 };
    let mut urls_rx = Some(urls_rx);
    let code = loop {
        do_message_loop_work();
        let status = event_loop.pump_app_events(Some(Duration::from_millis(2)), &mut host);
        if let PumpStatus::Exit(code) = status {
            break code;
        }
        if let Some(a) = host.app.as_mut() {
            if a.urls_rx.is_none() {
                a.urls_rx = urls_rx.take();
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
            if a.frames != host.access_frame {
                host.access_frame = a.frames;
                if let Some(ad) = host.access.as_mut() {
                    ad.update_if_active(|| a.access_tree());
                }
            }
        }
    };
    if let Some(a) = host.app.as_ref() {
        a.save_session();
    }
    host.app = None;
    cef::shutdown();
    ExitCode::from(code as u8)
}
