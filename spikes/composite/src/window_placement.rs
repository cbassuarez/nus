//! One pre-show placement pass. Never called when startup finishes or the app
//! regains focus. A saved normal window keeps its geometry when its complete frame fits.
use crate::settings::WindowStart;
use crate::startup_policy::{fitted, fully_inside, centered, Area};
use winit::window::Window;

pub fn place(window:&Window, mode:WindowStart, saved:bool, secondary:bool) {
    if std::env::var_os("NUS_SHOT_SIZE").is_some()
        || matches!(mode,WindowStart::Fullscreen|WindowStart::Maximized) { return; }
    platform_place(window,mode,saved,secondary);
}

#[cfg(target_os="macos")]
fn platform_place(window:&Window, mode:WindowStart, saved:bool, secondary:bool) {
    use objc2_foundation::{MainThreadMarker,NSPoint,NSRect,NSSize};
    use objc2_app_kit::{NSView,NSScreen};
    use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
    let Some(mtm)=MainThreadMarker::new() else {return};
    let Ok(handle)=window.window_handle() else {return};
    let RawWindowHandle::AppKit(handle)=handle.as_raw() else {return};
    let view=unsafe {&*handle.ns_view.as_ptr().cast::<NSView>()};
    let Some(native)=view.window() else {return};
    let Some(screen)=native.screen().or_else(||NSScreen::mainScreen(mtm))
        .or_else(||NSScreen::screens(mtm).firstObject()) else {return};
    // Invert AppKit's y axis, but do not scale global screen origins.
    let area=|r:NSRect|Area{x:r.origin.x,y:-(r.origin.y+r.size.height),w:r.size.width,h:r.size.height};
    let work=area(screen.visibleFrame());
    let old=area(native.frame());
    let screens=NSScreen::screens(mtm);
    let areas:Vec<_>=screens.iter().map(|s|area(s.visibleFrame())).collect();
    let restore=secondary || (matches!(mode,WindowStart::Last) && saved);
    if restore && areas.iter().any(|&work| fully_inside(old,work)) {return;}
    let preferred=if restore {(old.w,old.h)} else {(1440.0,900.0)};
    let Some(r)=(if restore {fitted(work,old)} else {centered(work,preferred)}) else {return};
    native.setFrame_display(NSRect::new(NSPoint::new(r.x,-r.y-r.h),NSSize::new(r.w,r.h)),false);
}

#[cfg(windows)]
fn platform_place(window:&Window, mode:WindowStart, saved:bool, secondary:bool) {
    use windows_sys::Win32::{Graphics::Gdi::*,UI::WindowsAndMessaging::*};
    use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
    let Ok(handle)=window.window_handle() else {return};
    let RawWindowHandle::Win32(handle)=handle.as_raw() else {return};
    let hwnd=handle.hwnd.get() as _;
    let mut info:MONITORINFO=unsafe {std::mem::zeroed()};
    info.cbSize=std::mem::size_of::<MONITORINFO>() as u32;
    let mut outer=unsafe {std::mem::zeroed()};
    let mut client=unsafe {std::mem::zeroed()};
    unsafe {
        let mon=MonitorFromWindow(hwnd,MONITOR_DEFAULTTONEAREST);
        if GetMonitorInfoW(mon,&mut info)==0 || GetWindowRect(hwnd,&mut outer)==0
            || GetClientRect(hwnd,&mut client)==0 {return;}
    }
    let work=Area{x:info.rcWork.left as f64,y:info.rcWork.top as f64,
        w:(info.rcWork.right-info.rcWork.left)as f64,h:(info.rcWork.bottom-info.rcWork.top)as f64};
    let old=Area{x:outer.left as f64,y:outer.top as f64,w:(outer.right-outer.left)as f64,h:(outer.bottom-outer.top)as f64};
    let scale=window.scale_factor();
    let restore=secondary || (matches!(mode,WindowStart::Last)&&saved);
    if restore && fully_inside(old,work) {return;}
    let inset=((old.w-(client.right-client.left)as f64).max(0.0),(old.h-(client.bottom-client.top)as f64).max(0.0));
    let desired=if restore {(old.w,old.h)} else {(1440.0*scale+inset.0,900.0*scale+inset.1)};
    if let Some(r)=if restore {fitted(work,old)} else {centered(work,desired)} {
        let _=window.request_inner_size(winit::dpi::PhysicalSize::new((r.w-inset.0).max(1.0).round()as u32,(r.h-inset.1).max(1.0).round()as u32));
        window.set_outer_position(winit::dpi::PhysicalPosition::new(r.x.round()as i32,r.y.round()as i32));
    }
}

#[cfg(not(any(windows,target_os="macos")))]
fn platform_place(window:&Window, mode:WindowStart, saved:bool, secondary:bool) {
    use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
    let wayland=window.window_handle().is_ok_and(|h|matches!(h.as_raw(),RawWindowHandle::Wayland(_)));
    let Some(monitor)=window.current_monitor().or_else(||window.primary_monitor()).or_else(||window.available_monitors().next()) else {return};
    let size=monitor.size(); let p=monitor.position(); let scale=monitor.scale_factor();
    // X11 has no bounded portable work-area query in this adapter. Use a
    // conservative monitor fit instead of blocking startup on X property IO.
    let work=Area{x:p.x as f64,y:p.y as f64,w:size.width as f64,h:size.height as f64};
    let restore=secondary || (matches!(mode,WindowStart::Last)&&saved);
    if !wayland && restore {
        if let Ok(p)=window.outer_position() {
            let s=window.outer_size();
            if fully_inside(Area{x:p.x as f64,y:p.y as f64,w:s.width as f64,h:s.height as f64},work) {return;}
        }
    }
    let desired=if restore {let s=window.outer_size();(s.width as f64,s.height as f64)} else {(1440.0*scale,900.0*scale)};
    let p=window.outer_position().unwrap_or(p);
    let old=Area{x:p.x as f64,y:p.y as f64,w:desired.0,h:desired.1};
    if let Some(r)=if restore {fitted(work,old)} else {centered(work,desired)} {
        let outer=window.outer_size(); let inner=window.inner_size();
        let dx=outer.width.saturating_sub(inner.width) as f64;
        let dy=outer.height.saturating_sub(inner.height) as f64;
        let _=window.request_inner_size(winit::dpi::PhysicalSize::new((r.w-dx).max(1.0).round()as u32,(r.h-dy).max(1.0).round()as u32));
        if !wayland {window.set_outer_position(winit::dpi::PhysicalPosition::new(r.x.round()as i32,r.y.round()as i32));}
    }
}

/// Fit a newly requested auxiliary window without changing its preferred size.
pub fn keep_inside(window: &Window) { platform_place(window,WindowStart::Last,true,false); }

/// Native frame and usable display area, for isolated placement checks.
#[cfg(target_os="macos")]
pub fn desktop_geometry(window: &Window) -> Option<(Area, Vec<Area>)> {
    use objc2_foundation::{MainThreadMarker, NSRect};
    use objc2_app_kit::{NSView, NSScreen};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let mtm=MainThreadMarker::new()?;
    let handle=window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle)=handle.as_raw() else {return None};
    let view=unsafe {&*handle.ns_view.as_ptr().cast::<NSView>()};
    let native=view.window()?;
    let area=|r:NSRect|Area{x:r.origin.x,y:-(r.origin.y+r.size.height),w:r.size.width,h:r.size.height};
    Some((area(native.frame()),NSScreen::screens(mtm).iter().map(|s|area(s.visibleFrame())).collect()))
}
#[cfg(not(target_os="macos"))]
pub fn desktop_geometry(window: &Window) -> Option<(Area, Vec<Area>)> {
    let p=window.outer_position().ok()?;let size=window.outer_size();
    let areas=window.available_monitors().map(|m|{let p=m.position();let s=m.size();Area{x:p.x as f64,y:p.y as f64,w:s.width as f64,h:s.height as f64}}).collect();
    Some((Area{x:p.x as f64,y:p.y as f64,w:size.width as f64,h:size.height as f64},areas))
}
