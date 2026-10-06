//! Window-only input stays live above nus-owned blocking content. The native
//! macOS controls remain NSButtons; the GPU strip below is not a replacement.
use crate::app::{App, CrumbHit, Pane};
use nus_render::{Rect, Scene};
use std::time::Instant;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{Key, NamedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button { Close, Minimize, Zoom, Skip, DefaultBrowser, NotNow }

#[derive(Default)]
pub(crate) struct State {
    capture: Option<(Button, Rect)>,
    focused: Option<Button>,
    swallow_release: bool,
    caption_press: Option<(Instant, (f32, f32))>,
    pub default_revision: u64,
}

impl App {
    pub(crate) fn window_shell_active(&self) -> bool {
        self.splash.is_some() || self.me_card.open || self.start.is_some()
            || self.palette.is_some() || self.timeline.is_some()
            || self.tabs.get(self.active).is_some_and(|t| {
                std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| {
                    matches!(p, Pane::Web(w) if w.tab.shared.borrow().dialog.is_some())
                })
            })
    }

    pub(crate) fn window_shell_rect(&self) -> Rect {
        let r = self.strip_rect();
        if self.window_shell_active() && r.h < self.px(28.0) {
            Rect::new(0.0, 0.0, self.target.size.0 as f32, self.px(34.0))
        } else { r }
    }

    fn shell_buttons(&self) -> Vec<(Button, Rect)> {
        let r = self.window_shell_rect();
        let mut right = r.right();
        let cell = self.px(42.0).min(r.w / 3.0);
        let mut out = Vec::with_capacity(6);
        if !cfg!(target_os = "macos") {
            for b in [Button::Close, Button::Zoom, Button::Minimize] {
                right -= cell;
                out.push((b, Rect::new(right,r.y,cell,r.h)));
            }
        }
        if self.splash.is_some() && right > r.x + self.px(180.0) {
            let w=self.px(58.0); right -= w;
            out.push((Button::Skip, Rect::new(right,r.y,w,r.h)));
        }
        if (self.arriving() || self.me_card.open) && crate::default_browser::offer_visible()
            && right-r.x > self.px(440.0) {
            let w=self.px(76.0); right-=w;
            out.push((Button::NotNow,Rect::new(right,r.y,w,r.h)));
            let w=self.px(160.0); right-=w;
            out.push((Button::DefaultBrowser,Rect::new(right,r.y,w,r.h)));
        }
        out
    }

    fn shell_invoke(&mut self, b: Button) {
        match b {
            Button::Close | Button::Minimize | Button::Zoom => {
                let action=match b {Button::Close=>0,Button::Minimize=>1,_=>3};
                // Host executes window effects after this App borrow is gone.
                let _=self.proxy.send_event(crate::UserEvent::WindowControl(self.window.id(),action));
            }
            Button::Skip => { self.finish_arrival(); self.splash=None; }
            Button::DefaultBrowser => { crate::default_browser::request(); }
            Button::NotNow => crate::default_browser::dismiss_offer(),
        }
        self.dirty=true;
    }

    /// Called before modal/arrival handlers, but only for the main window.
    /// Caption dragging is intentionally performed in the native press callback.
    pub(crate) fn route_window_shell(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::Focused(false) | WindowEvent::Destroyed => {
                self.traffic_lights.shell.capture=None;
                self.traffic_lights.shell.swallow_release=false;
                self.traffic_lights.shell.caption_press=None;
                return false;
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged {..} => {
                if self.traffic_lights.shell.capture.take().is_some() {
                    self.traffic_lights.shell.swallow_release=true;
                }
                self.traffic_lights.shell.caption_press=None;
                return false;
            }
            _=>{}
        }
        if let WindowEvent::MouseInput {button:MouseButton::Left,state:ElementState::Released,..}=event {
            let captured=self.traffic_lights.shell.capture.take();
            let swallow=std::mem::take(&mut self.traffic_lights.shell.swallow_release);
            if let Some((button,was))=captured {
                if self.window_shell_active() && self.shell_buttons().iter().any(|(b,r)|
                    *b==button && *r==was && r.contains(self.mouse.0,self.mouse.1)) {
                    self.shell_invoke(button);
                }
                return true;
            }
            if swallow { return true; }
        }
        if !self.window_shell_active() { return false; }
        if let WindowEvent::Ime(_)=event {
            // Decoration has no text field. Other modals retain their own IME.
            if self.splash.is_some() {
                // An input method can hand Enter or Space over as text.
                if let WindowEvent::Ime(winit::event::Ime::Commit(text))=event {
                    if matches!(text.as_str(),"\n"|"\r"|" ") { self.finish_arrival(); self.splash=None; self.dirty=true; }
                }
                return true;
            }
        }
        if let WindowEvent::KeyboardInput {event,..}=event {
            let command=if cfg!(target_os="macos") {self.mods.super_key()} else {self.mods.control_key()};
            if event.state==ElementState::Pressed {
                if self.splash.is_some() && !command && !self.mods.alt_key() {
                    if event.logical_key==Key::Named(NamedKey::Tab) {
                        let choices:Vec<_>=self.shell_buttons().into_iter().map(|(b,_)|b)
                            .filter(|b|matches!(b,Button::Skip|Button::DefaultBrowser|Button::NotNow)).collect();
                        if !choices.is_empty() {
                            let previous=self.traffic_lights.shell.focused.and_then(|b|choices.iter().position(|v|*v==b));
                            let next=match previous {
                                None=>if self.mods.shift_key(){choices.len()-1}else{0},
                                Some(i)=>(i+if self.mods.shift_key(){choices.len()-1}else{1})%choices.len(),
                            };
                            self.traffic_lights.shell.focused=Some(choices[next]);
                            self.dirty=true;
                        }
                        return true;
                    }
                    if event.logical_key==Key::Named(NamedKey::Enter) {
                        if let Some(b)=self.traffic_lights.shell.focused {
                            if self.shell_buttons().iter().any(|(v,_)|*v==b) {self.shell_invoke(b);return true;}
                        }
                    }
                }
                if (cfg!(target_os="macos") && command && matches!(&event.logical_key,Key::Character(k) if k.eq_ignore_ascii_case("w")))
                    || (!cfg!(target_os="macos") && self.mods.alt_key() && event.logical_key==Key::Named(NamedKey::F4)) {
                    self.shell_invoke(Button::Close); return true;
                }
                if cfg!(target_os="macos") && command && matches!(&event.logical_key,Key::Character(k) if k.eq_ignore_ascii_case("m")) {
                    self.shell_invoke(Button::Minimize); return true;
                }
            }
        }
        if self.window.fullscreen().is_some() { return false; }
        let r=self.window_shell_rect();
        match event {
            WindowEvent::MouseWheel {..} if r.contains(self.mouse.0,self.mouse.1)=>true,
            WindowEvent::MouseInput {button,state:ElementState::Pressed,..}=>{
                // A new press also clears a macOS drag whose release was owned by AppKit.
                self.traffic_lights.shell.capture=None;
                self.traffic_lights.shell.swallow_release=false;
                if !cfg!(target_os="macos") && *button==MouseButton::Left
                    && self.window.is_resizable() && !self.window.is_maximized()
                    && !self.shell_buttons().iter().any(|(_,hit)|hit.contains(self.mouse.0,self.mouse.1)) {
                    if let Some(direction)=edge(self.mouse,self.target.size,self.px(6.0)) {
                        self.traffic_lights.shell.swallow_release=true;
                        let _=self.window.drag_resize_window(direction);
                        return true;
                    }
                }
                if !r.contains(self.mouse.0,self.mouse.1) { return false; }
                if *button!=MouseButton::Left { return true; }
                if let Some((b,hit))=self.shell_buttons().into_iter().find(|(_,h)|h.contains(self.mouse.0,self.mouse.1)) {
                    self.traffic_lights.shell.capture=Some((b,hit));
                    self.dirty=true; return true;
                }
                // AppKit owns its real buttons. Do not use their area as a caption.
                if cfg!(target_os="macos") && self.mouse.0 < r.x+self.px(84.0) { return true; }
                let now=Instant::now();
                let twice=self.traffic_lights.shell.caption_press.is_some_and(|(at,p)| {
                    now.saturating_duration_since(at)<=crate::window_resize::double_click_interval()
                        && (p.0-self.mouse.0).abs()<self.px(5.0) && (p.1-self.mouse.1).abs()<self.px(5.0)
                });
                self.traffic_lights.shell.swallow_release=true;
                if twice {
                    self.traffic_lights.shell.caption_press=None;
                    match crate::window_resize::strip_action() {
                        crate::window_resize::StripAction::Zoom=>self.shell_invoke(Button::Zoom),
                        crate::window_resize::StripAction::Minimize=>self.shell_invoke(Button::Minimize),
                        crate::window_resize::StripAction::Nothing=>{},
                    }
                } else {
                    self.traffic_lights.shell.caption_press=Some((now,self.mouse));
                    // Must remain in this press callback, not a future user event.
                    let _=self.window.drag_window();
                }
                true
            }
            _=>false,
        }
    }

    pub(crate) fn poll_default_browser_ui(&mut self) {
        let files=crate::default_browser::files::status().0;
        if files!=self.default_files_seen { self.default_files_seen=files; self.dirty=true; }
        let (revision,note)=crate::default_browser::status();
        if self.traffic_lights.shell.default_revision!=revision {
            self.traffic_lights.shell.default_revision=revision;
            self.register_note=note;
            self.dirty=true;
        }
    }

    /// Runs after build(), including the cached-arrival branch. No cached
    /// scene may supply the live window controls' hit geometry.
    pub(crate) fn draw_window_shell(&mut self) {
        if !self.window_shell_active() || self.window.fullscreen().is_some() { return; }
        // Over the splash the controls wait out of sight: they come up when
        // the pointer reaches the top edge or Tab reaches them.
        if self.splash.is_some() && self.traffic_lights.shell.focused.is_none()
            && !self.window_shell_rect().contains(self.mouse.0,self.mouse.1) { return; }
        let mut scene=std::mem::take(&mut self.scene);
        self.draw_window_shell_into(&mut scene);
        self.scene=scene;
    }
    fn draw_window_shell_into(&mut self, scene:&mut Scene) {
        let r=self.window_shell_rect();
        let saved=scene.clip();
        // This is a top-level native-shell layer, not a child of the modal clip.
        scene.layer(Some(r));
        let mut paper=self.paper(); paper[3]=1.0;
        scene.rect(r,paper);
        self.crumb_hits.retain(|(_,h)|!matches!(h,CrumbHit::Close|CrumbHit::Minimize|CrumbHit::Maximize));
        let ui=self.ui();
        let baseline=r.y+(r.h+ui.px)*0.5-self.px(2.0);
        for (b,hit) in self.shell_buttons() {
            let hot=hit.contains(self.mouse.0,self.mouse.1);
            if hot || self.traffic_lights.shell.focused==Some(b) {
                scene.outline(hit.inset(self.px(3.0)),self.px(1.0),self.theme.ink);
            }
            let text=match b {Button::Close=>"×",Button::Minimize=>"−",Button::Zoom=>"□",Button::Skip=>"Skip",Button::DefaultBrowser=>"Make Default…",Button::NotNow=>"Not Now"};
            let width=self.fonts.measure(ui,text);
            self.fonts.draw(scene,ui,hit.x+(hit.w-width)*0.5,baseline,text);
            let h=match b {Button::Close=>Some(CrumbHit::Close),Button::Minimize=>Some(CrumbHit::Minimize),Button::Zoom=>Some(CrumbHit::Maximize),_=>None};
            if let Some(h)=h { self.crumb_hits.push((hit,h)); }
        }
        scene.layer(saved);
    }
}

fn edge((x,y):(f32,f32),(w,h):(u32,u32),pad:f32)->Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection::*;
    if x<0.0 || y<0.0 || x>=w as f32 || y>=h as f32 {return None;}
    let (l,r,t,b)=(x<pad,x>w as f32-pad,y<pad,y>h as f32-pad);
    match (l,r,t,b) {
        (true,_,true,_)=>Some(NorthWest),(_,true,true,_)=>Some(NorthEast),
        (true,_,_,true)=>Some(SouthWest),(_,true,_,true)=>Some(SouthEast),
        (true,_,_,_)=>Some(West),(_,true,_,_)=>Some(East),
        (_,_,true,_)=>Some(North),(_,_,_,true)=>Some(South),_=>None,
    }
}
