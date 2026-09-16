//! Spike 4: one compositor, terminal + browser panes, Broadsheet chrome.
//! See docs/SPIKES.md.

mod app;
mod browser;

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
}

struct Host {
    proxy: EventLoopProxy<UserEvent>,
    app: Option<App>,
}

impl ApplicationHandler<UserEvent> for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("nus")
            .with_decorations(false)
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        match App::new(window, self.proxy.clone()) {
            Ok(a) => self.app = Some(a),
            Err(e) => {
                tracing::error!("init: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, _ev: UserEvent) {
        if let Some(a) = &mut self.app {
            a.dirty = true;
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(a) = self.app.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => a.resize(s.width, s.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => a.set_scale(scale_factor as f32),
            WindowEvent::Moved(p) => a.window_moved(p.x, p.y),
            WindowEvent::ThemeChanged(t) => a.set_theme(match t {
                winit::window::Theme::Light => nus_render::Theme::paper(),
                winit::window::Theme::Dark => nus_render::Theme::ink(),
            }),
            WindowEvent::ModifiersChanged(m) => a.modifiers(m.state()),
            WindowEvent::KeyboardInput { event, .. } => {
                a.key(&event);
                a.dirty = true;
            }
            WindowEvent::CursorMoved { position, .. } => a.mouse_moved(position.x as f32, position.y as f32),
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
    let mut host = Host { proxy, app: None };
    let code = loop {
        do_message_loop_work();
        let status = event_loop.pump_app_events(Some(Duration::from_millis(2)), &mut host);
        if let PumpStatus::Exit(code) = status {
            break code;
        }
        if let Some(a) = host.app.as_mut() {
            a.apply_term_resizes(false);
            a.begin_frames();
            if a.dirty || a.pump() {
                a.redraw();
            }
        }
    };
    host.app = None;
    cef::shutdown();
    ExitCode::from(code as u8)
}
