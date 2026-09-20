//! The macOS controls are drawn with the shell, so there is one first frame.
use crate::app::{App, CrumbHit, hover_key};
use nus_render::{Rect, Scene, Instance};

impl App {
    pub(crate) fn traffic_width(&self) -> f32 {
        if !cfg!(target_os="macos") || self.window.fullscreen().is_some() { return 0.0; }
        self.px(if self.width_class()==crate::app::Width::Narrow { 54.0 } else { 62.0 })
    }

    pub(crate) fn draw_traffic_lights(&mut self, scene:&mut Scene) {
        if self.traffic_width()==0.0 { return; }
        let strip=self.strip_rect();
        let small=self.width_class()==crate::app::Width::Narrow;
        let size=self.px(if small {10.0} else {12.0});
        let step=self.px(if small {17.0} else {20.0});
        let x=strip.x+self.px(16.0);
        let cy=(strip.y+strip.h*0.5).round();
        let group=Rect::new(x-self.px(4.0),strip.y,step*2.0+size+self.px(8.0),strip.h);
        let hover=group.contains(self.mouse.0,self.mouse.1);
        let active=self.window.has_focus();
        for (i,(hit,color,icon,words)) in [
            (CrumbHit::Close,[1.0,0.365,0.345,1.0],nus_render::text::icons::CLOSE,"Close window"),
            (CrumbHit::Minimize,[1.0,0.741,0.180,1.0],nus_render::text::icons::MINIMIZE,"Minimize window"),
            (CrumbHit::Maximize,[0.157,0.788,0.251,1.0],nus_render::text::icons::EXPAND,"Enter full screen"),
        ].into_iter().enumerate() {
            let r=Rect::new(x+i as f32*step,cy-size*0.5,size,size);
            let fill=if active||hover {color} else {crate::app::fade(self.theme.dim,0.35)};
            scene.push(Instance::rounded(r,size*0.5,crate::surface::mix(fill,self.theme.ink,0.16)));
            let inset=self.px(0.5).max(1.0);
            scene.push(Instance::rounded(Rect::new(r.x+inset,r.y+inset,r.w-2.0*inset,r.h-2.0*inset),size*0.5,fill));
            if hover {
                let glyph=self.px(if small {7.0} else {8.0});
                self.fonts.draw_icon(scene,icon,glyph,(r.x+(size-glyph)*0.5).round(),(r.y+(size-glyph)*0.5).round(),[0.16,0.13,0.10,0.9]);
            }
            let target=Rect::new(r.x-self.px(3.0),strip.y,step,strip.h);
            self.crumb_hits.push((target,hit));
            self.foot_tip(hover_key("traffic",i),target,words.into());
        }
    }
}

/// Make the native container transparent; the compositor supplies its corners.
#[cfg(target_os="macos")]
pub fn prepare_window(window:&winit::window::Window) {
    use std::ffi::{c_void,c_char};
    use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
    type Id=*mut c_void;
    #[link(name="objc")] extern "C" {fn objc_getClass(name:*const c_char)->Id;fn sel_registerName(name:*const c_char)->Id;fn objc_msgSend();}
    let Ok(handle)=window.window_handle() else{return;};
    let RawWindowHandle::AppKit(handle)=handle.as_raw() else{return;};
    unsafe {
        let get:unsafe extern "C" fn(Id,Id)->Id=std::mem::transmute(objc_msgSend as *const ());
        let flag:unsafe extern "C" fn(Id,Id,bool)=std::mem::transmute(objc_msgSend as *const ());
        let set:unsafe extern "C" fn(Id,Id,Id)=std::mem::transmute(objc_msgSend as *const ());
        let view=handle.ns_view.as_ptr();let win=get(view,sel_registerName(c"window".as_ptr()));
        if win.is_null(){return;}
        flag(win,sel_registerName(c"setOpaque:".as_ptr()),false);
        let color=get(objc_getClass(c"NSColor".as_ptr()),sel_registerName(c"clearColor".as_ptr()));
        set(win,sel_registerName(c"setBackgroundColor:".as_ptr()),color);
        let layer=get(view,sel_registerName(c"layer".as_ptr()));
        if !layer.is_null(){flag(layer,sel_registerName(c"setOpaque:".as_ptr()),false);}
    }
}
#[cfg(not(target_os="macos"))]
pub fn prepare_window(_: &winit::window::Window) {}
