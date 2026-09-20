//! Platform window behavior; the terminal and work model stay platform-neutral.
use winit::window::Window;

#[derive(Default, Clone)]
pub struct Foreground {
    #[cfg(target_os = "macos")]
    app: Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>,
    #[cfg(windows)]
    window: isize,
}

impl Foreground {
    pub fn capture() -> Self {
        #[cfg(target_os = "macos")]
        { Self { app: objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication() } }
        #[cfg(windows)]
        { Self { window: unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize } } }
        #[cfg(not(any(windows, target_os="macos")))]
        { Self::default() }
    }
    pub fn restore(&self) {
        #[cfg(target_os = "macos")]
        if let Some(app) = &self.app {
            #[allow(deprecated)]
            app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps);
        }
        #[cfg(windows)]
        if self.window != 0 { unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.window as _); } }
    }
}

pub fn configure(window: &Window) {
    #[cfg(target_os="macos")]
    {
        use objc2_app_kit::{NSView, NSWindowCollectionBehavior as B};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = window.window_handle() {
            if let RawWindowHandle::AppKit(handle) = handle.as_raw() {
                // Winit's view is alive for the lifetime of this window; UI thread only.
                let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
                if let Some(w) = view.window() {
                    w.setCollectionBehavior(B::CanJoinAllSpaces | B::FullScreenAuxiliary | B::IgnoresCycle);
                    w.setHidesOnDeactivate(false);
                }
            }
        }
    }
    #[cfg(not(target_os="macos"))]
    let _ = window;
}

/// Winit's set_visible(true) makes an AppKit window key. Passive status and
/// backdrop windows must be ordered forward without taking keyboard focus.
pub fn show_passive(window: &Window) {
    #[cfg(target_os="macos")]
    {
        use objc2_app_kit::NSView;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle)=window.window_handle() {
            if let RawWindowHandle::AppKit(handle)=handle.as_raw() {
                let view=unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
                if let Some(w)=view.window() { w.orderFrontRegardless(); }
            }
        }
    }
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle)=window.window_handle() {
            if let RawWindowHandle::Win32(handle)=handle.as_raw() {
                unsafe { windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(handle.hwnd.get() as _, windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE); }
            }
        }
    }
    #[cfg(not(any(windows,target_os="macos")))]
    window.set_visible(true);
}

/// Physical pixels relative to the selected display's top-left corner.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct TopArea { pub menu: i32, pub notch: Option<Notch> }
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Notch { pub left: i32, pub width: u32, pub height: u32 }
impl TopArea {
    pub fn content_top(self) -> i32 { self.notch.map(|n|n.height as i32).unwrap_or(self.menu) }
}

/// Winit scales each screen's logical origin. Match that screen, then use
/// NSScreen's actual camera exclusion and auxiliary menu-bar areas. visibleFrame
/// alone is wrong: its menu bar can be taller than the camera housing.
pub fn top_area(mx: i32, my: i32, scale: f32) -> TopArea {
    #[cfg(target_os="macos")]
    {
        use objc2::{MainThreadMarker, runtime::NSObjectProtocol};
        use objc2_app_kit::NSScreen;
        let Some(mtm) = MainThreadMarker::new() else { return TopArea::default() };
        let screens = NSScreen::screens(mtm);
        let Some(first) = screens.firstObject() else { return TopArea::default() };
        let top = first.frame().size.height;
        for screen in screens.iter() {
            let f = screen.frame();
            let s = screen.backingScaleFactor();
            let x = (f.origin.x * s).round() as i32;
            let y = ((top - f.origin.y - f.size.height) * s).round() as i32;
            if (x - mx).abs() <= 2 && (y - my).abs() <= 2 {
                let visible = screen.visibleFrame();
                let menu = (f.origin.y + f.size.height - visible.origin.y - visible.size.height).max(0.0);
                let mut area=TopArea{menu:(menu * scale as f64).round() as i32,notch:None};
                if screen.respondsToSelector(objc2::sel!(safeAreaInsets)) && screen.respondsToSelector(objc2::sel!(auxiliaryTopLeftArea)) {
                    let height=screen.safeAreaInsets().top;
                    if height>0.0 {
                        let left=screen.auxiliaryTopLeftArea();let right=screen.auxiliaryTopRightArea();
                        let start=left.origin.x+left.size.width;
                        let width=right.origin.x-start;
                        if width>0.0 && width<f.size.width && start>=f.origin.x {
                            area.notch=Some(Notch{left:((start-f.origin.x)*scale as f64).round() as i32,width:(width*scale as f64).round() as u32,height:(height*scale as f64).round() as u32});
                        }
                    }
                }
                return area;
            }
        }
    }
    let _ = (mx, my, scale);
    TopArea::default()
}

pub fn safe_top(mx:i32,my:i32,scale:f32)->i32 {top_area(mx,my,scale).content_top()}

/// Only the island and its attached dropdown sit at status-window level.
/// Modal and backdrop windows keep their ordinary floating/normal levels.
pub fn island_level(window:&Window, attached:bool) {
    #[cfg(target_os="macos")]
    {
        use objc2_app_kit::{NSView,NSStatusWindowLevel,NSFloatingWindowLevel};
        use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
        if let Ok(handle)=window.window_handle() {
            if let RawWindowHandle::AppKit(handle)=handle.as_raw() {
                let view=unsafe {&*handle.ns_view.as_ptr().cast::<NSView>()};
                if let Some(w)=view.window(){w.setLevel(if attached {NSStatusWindowLevel} else {NSFloatingWindowLevel});w.setHasShadow(!attached);}
            }
        }
    }
    #[cfg(not(target_os="macos"))] let _=(window,attached);
}

pub fn wayland() -> bool { cfg!(target_os="linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() }

/// Scripted captures normally avoid changing focus. Native QA can opt in
/// while retaining its isolated profile and bounded scripted lifetime.
pub fn interactive() -> bool {
    std::env::var_os("NUS_SHOT").is_none() || std::env::var_os("NUS_SHOT_INTERACTIVE").is_some()
}
