//! AppKit owns the macOS window buttons; nus still draws the entire shell.
use crate::app::App;

#[derive(Default)]
pub struct TrafficLights {
    #[cfg(target_os = "macos")]
    native: Option<native::Controls>,
}

impl TrafficLights {
    pub fn new(window: &winit::window::Window, proxy: &winit::event_loop::EventLoopProxy<crate::UserEvent>) -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                native: native::Controls::new(window, proxy),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (window, proxy);
            Self::default()
        }
    }
}

impl App {
    pub(crate) fn traffic_width(&self) -> f32 {
        if !cfg!(target_os = "macos") || self.window.fullscreen().is_some() {
            return 0.0;
        }
        // Keep the wordmark and everything after it in exactly the same place.
        self.px(if self.width_class() == crate::app::Width::Narrow {
            54.0
        } else {
            62.0
        })
    }

    /// Exercise AppKit's real hit testing in the native screenshot harness.
    pub(crate) fn check_traffic_lights(&self) {
        #[cfg(target_os = "macos")]
        native::check(self);
    }

    pub(crate) fn press_traffic_light(&self, index:usize) {
        #[cfg(target_os = "macos")]
        if let Some(controls)=&self.traffic_lights.native {
            controls.press(index);
        }
        #[cfg(not(target_os = "macos"))]
        let _=index;
    }

    pub(crate) fn traffic_hovered(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            native::hovered(self)
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    pub(crate) fn sync_traffic_lights(&self) {
        #[cfg(target_os = "macos")]
        native::layout(self);
    }

    /// Native green-button/menu actions do not pass through our keyboard handler.
    pub(crate) fn sync_native_fullscreen(&mut self) {
        #[cfg(target_os = "macos")]
        {
            let fullscreen = self.window.fullscreen().is_some();
            if self.fullscreen != fullscreen {
                self.fullscreen = fullscreen;
                self.sidebar_hover = false;
                self.layout();
            }
        }
    }
}

/// A transparent, full-content titlebar adds only the system window controls.
pub fn main_window_attributes(
    attrs: winit::window::WindowAttributes,
) -> winit::window::WindowAttributes {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attrs
            .with_decorations(true)
            .with_title_hidden(true)
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        attrs
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
    use objc2_app_kit::{NSAccessibility, NSEvent, NSEventModifierFlags, NSWindowCollectionBehavior};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSButton, NSTitlebarSeparatorStyle, NSView, NSWindowButton, NSWindowStyleMask,
    };
    use objc2_foundation::NSPoint;
    use objc2_foundation::{MainThreadMarker, NSArray, NSObjectProtocol};
    use std::cell::Cell;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    struct GroupState {
        hovered: Cell<bool>,
        proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
        window: winit::window::WindowId,
    }

    define_class!(
        #[unsafe(super = NSView)]
        #[thread_kind = MainThreadOnly]
        #[name = "NusTrafficLightGroup"]
        #[ivars = GroupState]
        struct Group;
        unsafe impl NSObjectProtocol for Group {}
        impl Group {
            // AppKit's standard window widgets query this selector on their
            // host to draw their own grouped hover glyphs. Keep this one small
            // compatibility hook isolated; never draw replacement symbols.
            // Also used by Chromium's CustomWindowControlsView.
            #[unsafe(method(_mouseInGroup:))]
            fn mouse_in_group(&self, _button: &NSButton) -> bool { self.ivars().hovered.get() }

            #[unsafe(method(nusToggleFullscreen:))]
            fn toggle_fullscreen(&self, sender: &NSButton) {
                if NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Option) {
                    if let Some(window)=sender.window() { window.zoom(None); }
                    return;
                }
                // A native widget on a borderless host must enter through
                // winit: it temporarily installs the style AppKit needs and
                // restores it on exit. Direct toggleFullScreen skips that save.
                let _ = self.ivars().proxy.send_event(crate::UserEvent::WindowControl(self.ivars().window,2));
            }

            #[unsafe(method(nusClose:))]
            fn close_window(&self, _sender: &NSButton) {
                let _ = self.ivars().proxy.send_event(crate::UserEvent::WindowControl(self.ivars().window,0));
            }

            #[unsafe(method(nusMinimize:))]
            fn minimize_window(&self, _sender: &NSButton) {
                let _ = self.ivars().proxy.send_event(crate::UserEvent::WindowControl(self.ivars().window,1));
            }

            #[unsafe(method_id(hitTest:))]
            fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
                let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
                hit.filter(|v| !std::ptr::eq(&**v, &**self))
            }
        }
    );

    pub(super) struct Controls {
        titlebar: Retained<Group>,
        buttons: Vec<Retained<NSButton>>,
        accessible: Cell<Option<bool>>,
    }

    impl Controls {
        pub(super) fn press(&self,index:usize) {unsafe {self.buttons[index].performClick(None);}}
        pub(super) fn new(window: &winit::window::Window, proxy: &winit::event_loop::EventLoopProxy<crate::UserEvent>) -> Option<Self> {
            let handle = window.window_handle().ok()?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return None;
            };
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            let native = view.window()?;
            let buttons: Vec<_> = [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ]
            .into_iter()
            .filter_map(|kind| native.standardWindowButton(kind))
            .collect();
            if buttons.len() != 3 {
                return None;
            }
            let allocated = Group::alloc(MainThreadMarker::new()?).set_ivars(GroupState { hovered: Cell::new(false), proxy: proxy.clone(), window: window.id() });
            let titlebar: Retained<Group> = unsafe { msg_send![super(allocated), init] };
            for button in &buttons {
                titlebar.addSubview(button);
            }
            unsafe {
                for (button, action) in buttons.iter().zip([sel!(nusClose:),sel!(nusMinimize:),sel!(nusToggleFullscreen:)]) {
                    button.setTarget(Some(&titlebar));
                    button.setAction(Some(action));
                }
            }
            // The original borderless frame preserves nus's exact shell/corners.
            // Retain AppKit's controls and their tracking host inside our content.
            window.set_decorations(false);
            native.setStyleMask(
                native.styleMask()
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
            );
            native.setCollectionBehavior(
                native.collectionBehavior() | NSWindowCollectionBehavior::FullScreenPrimary,
            );
            view.addSubview(&titlebar);
            Some(Self {
                titlebar,
                buttons,
                accessible: Cell::new(None),
            })
        }
    }

    pub(super) fn hovered(app: &App) -> bool {
        let Ok(handle) = app.window.window_handle() else {
            return false;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return false;
        };
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let Some(window) = view.window() else {
            return false;
        };
        let Some(controls) = &app.traffic_lights.native else {
            return false;
        };
        let (Some(first), Some(last)) = (controls.buttons.first(), controls.buttons.last()) else {
            return false;
        };
        if first.isHidden() {
            return false;
        }
        // Crossing from winit into an AppKit button sends CursorLeft. Keep a
        // compact header open while the pointer is inside the native group.
        let first = first.convertRect_toView(first.bounds(), None);
        let last = last.convertRect_toView(last.bounds(), None);
        let point = window.mouseLocationOutsideOfEventStream();
        point.x >= first.origin.x - 2.0
            && point.x <= last.origin.x + last.size.width + 2.0
            && point.y >= first.origin.y - 2.0
            && point.y <= first.origin.y + first.size.height + 2.0
    }

    pub(super) fn check(app: &App) {
        let handle = app.window.window_handle().unwrap();
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            unreachable!()
        };
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let window = view.window().unwrap();
        if app.window.fullscreen().is_none() {
            assert!(!window.styleMask().contains(NSWindowStyleMask::Titled), "native frame changed nus corners");
        }
        let root = unsafe { view.superview() }.unwrap();
        let bounds = view.bounds();
        assert!((bounds.size.width * app.scale as f64 - app.target.size.0 as f64).abs() < 1.0);
        assert!((bounds.size.height * app.scale as f64 - app.target.size.1 as f64).abs() < 1.0);
        for kind in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            let button = &app.traffic_lights.native.as_ref().unwrap().buttons[kind.0 as usize];
            let rect = button.convertRect_toView(button.bounds(), Some(view));
            eprintln!(
                "TRAFFIC {:?} content {:?} button {:?} parent {:?}",
                kind,
                rect,
                button.frame(),
                unsafe { button.superview() }.unwrap().frame()
            );
            let center = NSPoint::new(
                rect.origin.x + rect.size.width / 2.0,
                rect.origin.y + rect.size.height / 2.0,
            );
            let at = root.convertPoint_fromView(center, Some(view));
            let hit = root.hitTest(at);
            eprintln!("TRAFFIC HIT {:?}", hit.as_ref().map(|v| v.class().name()));
            if !button.isHidden() && app.window.fullscreen().is_none() {
                assert!(
                    hit.as_deref()
                        .is_some_and(|v| std::ptr::eq(v, &***button as &NSView)),
                    "native button center is not clickable"
                );
                for (dx,dy) in [(-4.0,0.0),(4.0,0.0),(0.0,-4.0),(0.0,4.0)] {
                    let point = NSPoint::new(center.x+dx,center.y+dy);
                    let hit = root.hitTest(root.convertPoint_fromView(point,Some(view)));
                    assert!(hit.as_deref().is_some_and(|v| std::ptr::eq(v, &***button as &NSView)), "visible circle has a dead hit region");
                }
            }
        }
        if app.window.fullscreen().is_none() && app.strip_shown() {
            let strip = app.strip_rect();
            let small = app.width_class() == crate::app::Width::Narrow;
            let cx = strip.x + app.px(if small { 21.0 } else { 22.0 });
            let cy = (strip.y + strip.h * 0.5).round();
            for (x, y) in [
                (cx, strip.y + app.px(1.0)),
                (cx, strip.bottom() - app.px(1.0)),
                (cx - app.px(11.0), cy),
            ] {
                let point = NSPoint::new(
                    x as f64 / app.scale as f64,
                    if view.isFlipped() {
                        y as f64 / app.scale as f64
                    } else {
                        bounds.size.height - y as f64 / app.scale as f64
                    },
                );
                let hit = root.hitTest(root.convertPoint_fromView(point, Some(view)));
                eprintln!(
                    "TRAFFIC OUTSIDE {x},{y}: {:?}",
                    hit.as_ref().map(|v| v.class().name())
                );
                assert!(
                    hit.as_deref().is_some_and(|v| std::ptr::eq(v, view)),
                    "space around red must reach the existing header drag handler"
                );
            }
            for (rect, _) in &app.crumb_hits {
                let point = NSPoint::new(
                    (rect.x + rect.w * 0.5) as f64 / app.scale as f64,
                    (rect.y + rect.h * 0.5) as f64 / app.scale as f64,
                );
                let hit = root.hitTest(root.convertPoint_fromView(point, Some(view)));
                assert!(
                    hit.as_deref().is_some_and(|v| std::ptr::eq(v, view)),
                    "native titlebar intercepts an existing header action"
                );
            }
        }
        assert!(
            !app.crumb_hits.iter().any(|(_, h)| matches!(
                h,
                crate::app::CrumbHit::Close
                    | crate::app::CrumbHit::Minimize
                    | crate::app::CrumbHit::Maximize
            )),
            "custom traffic-light hit targets remain"
        );
        eprintln!("TRAFFIC CHECK PASSED");
    }

    pub(super) fn layout(app: &App) {
        let Ok(handle) = app.window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        // The winit-owned view outlives this call; all callers run on the UI thread.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let Some(window) = view.window() else {
            return;
        };
        if window.titlebarSeparatorStyle() != NSTitlebarSeparatorStyle::None {
            window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
        }
        let Some(controls) = &app.traffic_lights.native else {
            return;
        };
        let buttons = &controls.buttons;
        let titlebar = &controls.titlebar;
        let hovered = hovered(app);
        if titlebar.ivars().hovered.replace(hovered) != hovered {
            for button in buttons {
                NSView::setNeedsDisplay(button, true);
            }
        }
        // Match the shell's appearance without changing application-wide appearance.
        let name = unsafe {
            if app.theme.mode == nus_render::Mode::Ink {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        if titlebar.appearance().is_none_or(|a| &*a.name() != name) {
            titlebar.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
        }
        // Match nus's existing fullscreen behavior: its strip omits the lights.
        let fullscreen = app.window.fullscreen().is_some();
        for button in buttons {
            let hidden = fullscreen || !app.strip_shown() || app.arriving();
            if button.isHidden() != hidden {
                button.setHidden(hidden);
            }
        }
        let visible = !fullscreen && app.strip_shown() && !app.arriving();
        if controls.accessible.replace(Some(visible)) != Some(visible) {
            // AccessKit owns the content view's children. Expose the real native
            // widgets alongside that view instead of creating duplicate AX nodes.
            let mut children: Vec<&AnyObject> = vec![view.as_ref()];
            if visible {
                children.extend(buttons.iter().map(|b| -> &AnyObject { (&**b).as_ref() }));
            }
            unsafe {
                window.setAccessibilityChildren(Some(&NSArray::from_slice(&children)));
            }
        }
        if titlebar.isHidden() == visible {
            titlebar.setHidden(!visible);
        }
        if fullscreen {
            return;
        }

        let strip = app.strip_rect();
        let scale = app.scale as f64;
        let height = strip.bottom() as f64 / scale;
        // Host only the existing header. Native buttons keep their own sizes.
        let mut frame = titlebar.frame();
        frame.origin.x = 0.0;
        frame.origin.y = if view.isFlipped() {
            0.0
        } else {
            view.bounds().size.height - height
        };
        frame.size.width = view.bounds().size.width;
        frame.size.height = height;
        if titlebar.frame() != frame {
            titlebar.setFrame(frame);
        }

        let small = app.width_class() == crate::app::Width::Narrow;
        let first_center = strip.x + app.px(if small { 21.0 } else { 22.0 });
        let step = app.px(if small { 17.0 } else { 20.0 });
        let y = (strip.y + strip.h * 0.5).round() as f64 / scale;
        for (i, button) in buttons.iter().enumerate() {
            let size = button.frame().size;
            let content_center = NSPoint::new(
                (first_center + i as f32 * step) as f64 / scale,
                if view.isFlipped() {
                    y
                } else {
                    view.bounds().size.height - y
                },
            );
            let center = titlebar.convertPoint_fromView(content_center, Some(view));
            let origin = NSPoint::new(center.x - size.width * 0.5, center.y - size.height * 0.5);
            if button.frame().origin != origin {
                button.setFrameOrigin(origin);
            }
        }
    }
}

/// Make the native container transparent; the compositor supplies its corners.
#[cfg(target_os = "macos")]
pub fn prepare_window(window: &winit::window::Window) {
    use std::ffi::{c_char, c_void};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    type Id = *mut c_void;
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Id;
        fn objc_msgSend();
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    unsafe {
        let get: unsafe extern "C" fn(Id, Id) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        let flag: unsafe extern "C" fn(Id, Id, bool) =
            std::mem::transmute(objc_msgSend as *const ());
        let set: unsafe extern "C" fn(Id, Id, Id) = std::mem::transmute(objc_msgSend as *const ());
        let view = handle.ns_view.as_ptr();
        let win = get(view, sel_registerName(c"window".as_ptr()));
        if win.is_null() {
            return;
        }
        flag(win, sel_registerName(c"setOpaque:".as_ptr()), false);
        let color = get(
            objc_getClass(c"NSColor".as_ptr()),
            sel_registerName(c"clearColor".as_ptr()),
        );
        set(
            win,
            sel_registerName(c"setBackgroundColor:".as_ptr()),
            color,
        );
        let layer = get(view, sel_registerName(c"layer".as_ptr()));
        if !layer.is_null() {
            flag(layer, sel_registerName(c"setOpaque:".as_ptr()), false);
        }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn prepare_window(_: &winit::window::Window) {}
