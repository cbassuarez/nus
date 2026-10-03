//! Resizing the window the way each system does it, and the top strip's
//! double click.
//!
//! While you drag a window's edge the system holds the event loop —
//! AppKit's live resize, Windows' modal sizing loop — so nus's own turn
//! doesn't come until you let go. The frame is drawn in the resize event
//! itself instead (`App::live_resize_frame`), and on macOS the GPU layer
//! presents inside the window's own transaction for as long as the resize
//! lasts, so the picture and the frame arrive together rather than the
//! content wobbling a frame behind the edge.
//!
//! A double click on the strip does what the system's title bar does:
//! on macOS what Desktop & Dock says (zoom, which Stage Manager's stage
//! already bounds, or minimise, or nothing); on Windows maximise and
//! restore; on Linux what the desktop's setting says, maximise by default.

use std::time::Duration;
use winit::window::Window;

/// What a double click on the strip does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripAction {
    /// Zoom (macOS) or maximise, and back.
    Zoom,
    Minimize,
    Nothing,
}

/// macOS's `AppleActionOnDoubleClick`, and the older boolean before it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn mac_action(action: Option<&str>, miniaturize: bool) -> StripAction {
    match action {
        Some("Minimize") => StripAction::Minimize,
        Some("None") | Some("Do Nothing") => StripAction::Nothing,
        Some(_) => StripAction::Zoom,
        None if miniaturize => StripAction::Minimize,
        None => StripAction::Zoom,
    }
}

/// GNOME's `action-double-click-titlebar` (KDE and others follow it or
/// maximise, which is also the default here).
#[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
pub fn gnome_action(value: &str) -> StripAction {
    match value.trim().trim_matches('\'') {
        "minimize" => StripAction::Minimize,
        "none" | "lower" | "menu" => StripAction::Nothing,
        _ => StripAction::Zoom,
    }
}

pub use imp::*;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, NSObject, NSObjectProtocol};
    use objc2::msg_send;
    use objc2_app_kit::{NSEvent, NSView};
    use objc2_foundation::{NSString, NSUserDefaults};

    fn view_of(window: &Window) -> Option<Retained<NSView>> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        Some(objc2::Message::retain(view))
    }

    /// AppKit is resizing the window from an edge right now.
    pub fn in_live_resize(window: &Window) -> bool {
        view_of(window).is_some_and(|v| v.inLiveResize())
    }

    /// Present the window's GPU layer inside Core Animation's transaction
    /// (on) — in step with the window's frame, at the cost of waiting for
    /// the GPU — or on its own (off), as between resizes.
    pub fn present_in_transaction(window: &Window, on: bool) {
        let Some(view) = view_of(window) else { return };
        let Some(metal) = AnyClass::get(c"CAMetalLayer") else { return };
        unsafe {
            let root: Option<Retained<NSObject>> = msg_send![&*view, layer];
            let Some(root) = root else { return };
            let set = |layer: &NSObject| {
                if layer.isKindOfClass(metal) {
                    let _: () = msg_send![layer, setPresentsWithTransaction: on];
                }
            };
            set(&root);
            let sublayers: Option<Retained<objc2_foundation::NSArray<NSObject>>> = msg_send![&*root, sublayers];
            for layer in sublayers.iter().flat_map(|a| a.iter()) {
                set(&layer);
            }
        }
    }

    pub fn double_click_interval() -> Duration {
        Duration::from_secs_f64(NSEvent::doubleClickInterval().clamp(0.1, 2.0))
    }

    pub fn strip_action() -> StripAction {
        let defaults = NSUserDefaults::standardUserDefaults();
        let action = defaults.stringForKey(&NSString::from_str("AppleActionOnDoubleClick")).map(|s| s.to_string());
        let miniaturize = defaults.boolForKey(&NSString::from_str("AppleMiniaturizeOnDoubleClick"));
        mac_action(action.as_deref(), miniaturize)
    }

    /// The system's zoom, as the title bar's double click does it: the
    /// window fills the space the system gives it — under Stage Manager,
    /// the stage beside the strip — and a second zoom puts it back.
    pub fn zoom(window: &Window) {
        if let Some(w) = view_of(window).and_then(|v| v.window()) {
            w.zoom(None);
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    pub fn in_live_resize(_window: &Window) -> bool {
        false
    }

    pub fn present_in_transaction(_window: &Window, _on: bool) {}

    pub fn double_click_interval() -> Duration {
        let ms = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() };
        Duration::from_millis(u64::from(ms).clamp(100, 2000))
    }

    pub fn strip_action() -> StripAction {
        StripAction::Zoom
    }

    pub fn zoom(window: &Window) {
        window.set_maximized(!window.is_maximized());
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::*;

    pub fn in_live_resize(_window: &Window) -> bool {
        false
    }

    pub fn present_in_transaction(_window: &Window, _on: bool) {}

    pub fn double_click_interval() -> Duration {
        Duration::from_millis(400)
    }

    /// The desktop's own setting, read once: a process per double click
    /// would be noticed.
    pub fn strip_action() -> StripAction {
        static ACTION: std::sync::OnceLock<StripAction> = std::sync::OnceLock::new();
        *ACTION.get_or_init(|| {
            nus_compat::command("gsettings")
                .args(["get", "org.gnome.desktop.wm.preferences", "action-double-click-titlebar"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| gnome_action(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or(StripAction::Zoom)
        })
    }

    pub fn zoom(window: &Window) {
        window.set_maximized(!window.is_maximized());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktops_own_double_click() {
        assert_eq!(mac_action(None, false), StripAction::Zoom);
        assert_eq!(mac_action(None, true), StripAction::Minimize);
        assert_eq!(mac_action(Some("Maximize"), true), StripAction::Zoom);
        assert_eq!(mac_action(Some("Fill"), false), StripAction::Zoom);
        assert_eq!(mac_action(Some("Minimize"), false), StripAction::Minimize);
        assert_eq!(mac_action(Some("None"), false), StripAction::Nothing);
        assert_eq!(gnome_action("'toggle-maximize'\n"), StripAction::Zoom);
        assert_eq!(gnome_action("'minimize'"), StripAction::Minimize);
        assert_eq!(gnome_action("'none'"), StripAction::Nothing);
        assert_eq!(gnome_action("'toggle-maximize-vertically'"), StripAction::Zoom);
    }
}
