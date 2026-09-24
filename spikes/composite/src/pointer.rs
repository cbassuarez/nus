//! Sample client coordinates only when focus/entry events omit a motion event.
pub fn in_window(window:&winit::window::Window)->Option<(f32,f32)> {
    #[cfg(target_os="macos")] {
        use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
        use objc2_app_kit::NSView;
        let RawWindowHandle::AppKit(handle)=window.window_handle().ok()?.as_raw() else{return None};
        let view=unsafe{&*handle.ns_view.as_ptr().cast::<NSView>()};
        let native=view.window()?;
        let p=view.convertPoint_fromView(native.mouseLocationOutsideOfEventStream(),None);
        let bounds=view.bounds();let scale=window.scale_factor();
        let y=if view.isFlipped(){p.y-bounds.origin.y}else{bounds.size.height-(p.y-bounds.origin.y)};
        Some((((p.x-bounds.origin.x)*scale)as f32,(y*scale)as f32))
    }
    #[cfg(windows)] {
        use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
        use windows_sys::Win32::{Foundation::POINT,UI::WindowsAndMessaging::GetCursorPos,Graphics::Gdi::ScreenToClient};
        let RawWindowHandle::Win32(handle)=window.window_handle().ok()?.as_raw() else{return None};
        let mut point=POINT{x:0,y:0};
        unsafe{if GetCursorPos(&mut point)==0 || ScreenToClient(handle.hwnd.get() as _,&mut point)==0{return None;}}
        Some((point.x as f32,point.y as f32))
    }
    #[cfg(target_os="linux")] {
        use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
        use x11rb::protocol::xproto::ConnectionExt;
        let id=match window.window_handle().ok()?.as_raw(){RawWindowHandle::Xlib(h)=>h.window as u32,RawWindowHandle::Xcb(h)=>h.window.get(),_=>return None};
        let (conn,_)=x11rb::connect(None).ok()?;
        let p=conn.query_pointer(id).ok()?.reply().ok()?;
        p.same_screen.then_some((p.win_x as f32,p.win_y as f32))
    }
    #[cfg(not(any(target_os="macos",windows,target_os="linux")))] {let _=window;None}
}
