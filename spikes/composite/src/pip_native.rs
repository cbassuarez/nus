//! Platform work areas and floating-window behavior for PiP.
use super::LRect;
use winit::window::Window;

pub fn configure(window: &Window) {
    window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    #[cfg(target_os="macos")]
    {
        use objc2_app_kit::{NSView, NSWindowCollectionBehavior as Behavior, NSFloatingWindowLevel};
        use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
        if let Ok(handle)=window.window_handle() {
            if let RawWindowHandle::AppKit(handle)=handle.as_raw() {
                let view=unsafe {&*handle.ns_view.as_ptr().cast::<NSView>()};
                if let Some(native)=view.window() {
                    native.setLevel(NSFloatingWindowLevel);
                    native.setAnimationBehavior(objc2_app_kit::NSWindowAnimationBehavior::None);
                    native.setHidesOnDeactivate(false);
                    native.setCollectionBehavior(Behavior::CanJoinAllSpaces | Behavior::FullScreenAuxiliary);
                }
            }
        }
    }
}

pub fn work_area(window: &Window) -> LRect {
    let scale=window.scale_factor();
    let monitor=window.current_monitor().or_else(||window.primary_monitor());
    let Some(monitor)=monitor else {return LRect{x:0.0,y:0.0,w:1280.0,h:720.0};};
    let pos=monitor.position();let size=monitor.size();
    let fallback=LRect{x:pos.x as f64/scale,y:pos.y as f64/scale,w:size.width as f64/scale,h:size.height as f64/scale};
    #[cfg(target_os="macos")]
    {
        use objc2_app_kit::NSScreen;
        if let Some(mtm)=objc2::MainThreadMarker::new() {
            let screens=NSScreen::screens(mtm);
            if let Some(first)=screens.firstObject() {
                let top=first.frame().size.height;
                for screen in screens.iter() {
                    let f=screen.frame();let s=screen.backingScaleFactor();
                    let x=(f.origin.x*s).round()as i32;
                    let y=((top-f.origin.y-f.size.height)*s).round()as i32;
                    if (x-pos.x).abs()<=2 && (y-pos.y).abs()<=2 {
                        // Re-read visibleFrame: Dock location and hiding can change.
                        let v=screen.visibleFrame();
                        return LRect{x:v.origin.x*s/scale,y:(top-v.origin.y-v.size.height)*s/scale,w:v.size.width*s/scale,h:v.size.height*s/scale};
                    }
                }
            }
        }
    }
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[repr(C)] struct Rect {left:i32,top:i32,right:i32,bottom:i32}
        #[repr(C)] struct Info {size:u32,monitor:Rect,work:Rect,flags:u32}
        #[link(name="user32")] extern "system" {
            fn MonitorFromRect(rect:*const Rect,flags:u32)->*mut c_void;
            fn GetMonitorInfoW(monitor:*mut c_void,info:*mut Info)->i32;
        }
        let rect=Rect{left:pos.x,top:pos.y,right:pos.x+size.width as i32,bottom:pos.y+size.height as i32};
        let mut info:Info=unsafe{std::mem::zeroed()};info.size=std::mem::size_of::<Info>()as u32;
        if unsafe{GetMonitorInfoW(MonitorFromRect(&rect,2),&mut info)}!=0 {
            return LRect{x:info.work.left as f64/scale,y:info.work.top as f64/scale,w:(info.work.right-info.work.left)as f64/scale,h:(info.work.bottom-info.work.top)as f64/scale};
        }
    }
    #[cfg(target_os="linux")]
    if !crate::hatch_native::wayland() {if let Some(area)=x11_work_area(fallback,scale){return area;}}
    // Wayland reserves placement and stacking to the compositor; winit's
    // supported native move/resize requests are used there.
    fallback
}
#[cfg(target_os="linux")]
fn x11_work_area(monitor:LRect,scale:f64)->Option<LRect> {
    use x11rb::{connection::Connection,protocol::xproto::{ConnectionExt,AtomEnum}};
    let (conn,screen)=x11rb::connect(None).ok()?;let root=conn.setup().roots.get(screen)?.root;
    let property=|name:&[u8]|->Option<Vec<u32>>{
        let atom=conn.intern_atom(true,name).ok()?.reply().ok()?.atom;
        Some(conn.get_property(false,root,atom,AtomEnum::CARDINAL,0,4096).ok()?.reply().ok()?.value32()?.collect())
    };
    let desktop=property(b"_NET_CURRENT_DESKTOP").and_then(|v|v.first().copied()).unwrap_or(0)as usize;
    let values=property(b"_NET_WORKAREA")?;let v=values.get(desktop*4..desktop*4+4)?;
    let (x,y)=(v[0]as i32 as f64/scale,v[1]as i32 as f64/scale);
    let left=monitor.x.max(x);let top=monitor.y.max(y);let right=(monitor.x+monitor.w).min(x+v[2]as f64/scale);let bottom=(monitor.y+monitor.h).min(y+v[3]as f64/scale);
    (right>left && bottom>top).then_some(LRect{x:left,y:top,w:right-left,h:bottom-top})
}

/// Bring PiP forward without taking keyboard focus from the browsing window.
pub fn show(window:&Window) {
    #[cfg(target_os="macos")] {
        use objc2_app_kit::NSView;
        use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
        if let Ok(handle)=window.window_handle() {if let RawWindowHandle::AppKit(handle)=handle.as_raw() {
            let view=unsafe{&*handle.ns_view.as_ptr().cast::<NSView>()};
            if let Some(native)=view.window() {native.orderFrontRegardless();return;}
        }}
    }
    window.set_visible(true);
}
