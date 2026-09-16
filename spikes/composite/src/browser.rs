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
    /// Logical size CEF should render at; app sets it, view_rect reads it.
    pub size: (f32, f32),
    pub scale: f32,
    /// Where the browser pane sits in the window (physical px), for
    /// screen_point → popup placement.
    pub origin: (f32, f32),
    pub window_pos: (i32, i32),
}

pub type SharedRef = StdRc<RefCell<Shared>>;

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
            cl.append_switch_with_value(Some(&"remote-debugging-port".into()), Some(&"9229".into()));
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

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            let Some(info) = info else { return };
            if type_ != PaintElementType::default() {
                return; // popups (select dropdowns) not composited in this spike
            }
            use cef::osr_texture_import::shared_texture_handle::SharedTextureHandle;
            let handle = SharedTextureHandle::new(info);
            match handle.import_texture(&self.osr.device) {
                Ok(texture) => {
                    let bind = (self.osr.bind_texture)(&texture);
                    self.osr.shared.borrow_mut().bind = Some(bind);
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
                self.d.shared.borrow_mut().url = u.to_string();
            }
        }

        fn on_loading_progress_change(&self, _browser: Option<&mut Browser>, progress: f64) {
            self.d.shared.borrow_mut().loading = progress < 1.0;
        }
    }
}

wrap_life_span_handler! {
    pub struct LifeBuilder {
        _unit: (),
    }

    impl LifeSpanHandler {
        // Popups would be separate native windows; this spike opens them in place.
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
            if let (Some(b), Some(url)) = (browser, target_url) {
                if let Some(f) = b.main_frame() {
                    f.load_url(Some(url));
                }
            }
            1
        }
    }
}

wrap_client! {
    pub struct ClientBuilder {
        render: RenderHandler,
        display: DisplayHandler,
        life: LifeSpanHandler,
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
    }
}

pub struct BrowserTab {
    pub browser: Browser,
    pub shared: SharedRef,
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
            LifeBuilder::new(()),
        );
        let mut context = request_context_create_context(
            Some(&RequestContextSettings::default()),
            None,
        );
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url.into()),
            Some(&settings),
            None,
            context.as_mut(),
        )?;
        Some(BrowserTab { browser, shared })
    }

    pub fn host(&self) -> Option<BrowserHost> {
        self.browser.host()
    }

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
}
