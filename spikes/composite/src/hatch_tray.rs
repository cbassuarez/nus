//! Signal icons are independent of the modular drawer's contents and density.
use crate::menu_drawer::{Config,Signal,SignalStyle};
#[cfg(any(target_os="linux",all(test,target_os="macos")))]
#[path="menu_tray_linux.rs"] mod linux;
pub struct Tray {
    #[cfg(any(windows,target_os="macos"))] icon:tray_icon::TrayIcon,
    #[cfg(any(windows,target_os="macos"))] work:tray_icon::menu::MenuItem,
    #[cfg(target_os="linux")] linux:linux::Tray,
    state:Signal,style:SignalStyle,enabled:bool,signal:nus_render::Color,
}
impl Tray {
    pub fn new(proxy:winit::event_loop::EventLoopProxy<crate::UserEvent>)->Option<Self>{
        #[cfg(any(windows,target_os="macos"))]
        {
            use tray_icon::{TrayIconBuilder,TrayIconEvent,MouseButton,MouseButtonState,menu::{Menu,MenuItem}};
            let menu=Menu::new();let work=MenuItem::with_id("nus-drawer","Open nus drawer",true,None);
            let hatch=MenuItem::with_id("hatch-show","Show / hide Hatch",true,None);let main=MenuItem::with_id("hatch-main","Open nus window",true,None);let quit=MenuItem::with_id("hatch-quit","Quit nus",true,None);
            menu.append_items(&[&work,&hatch,&main,&quit]).ok()?;let click_proxy=proxy.clone();
            TrayIconEvent::set_event_handler(Some(move |event|{if let TrayIconEvent::Click{rect,button:MouseButton::Left,button_state:MouseButtonState::Down,..}=event{let _=click_proxy.send_event(crate::UserEvent::MenuDrawer(Some(crate::menu_drawer::Anchor{x:rect.position.x,y:rect.position.y,width:rect.size.width,height:rect.size.height})));}}));
            let signal=nus_render::theme::signal::RED;let icon=tray_icon::Icon::from_rgba(nus_render::dock_icon::render(36,signal,nus_render::dock_icon::Face::Newsreader),36,36).ok()?;
            match TrayIconBuilder::new().with_menu(Box::new(menu)).with_menu_on_left_click(false).with_icon(icon).with_tooltip("nus · All quiet").build(){Ok(icon)=>{let tray=Self{icon,work,state:Signal::default(),style:SignalStyle::Dot,enabled:true,signal};tray.trace_icon();Some(tray)},Err(e)=>{tracing::warn!("nus tray: {e}");None}}
        }
        #[cfg(target_os="linux")] {
            let signal=nus_render::theme::signal::RED;
            let linux=linux::Tray::new(proxy,linux::State{signal,work:Signal::default(),style:SignalStyle::Dot,enabled:true});
            Some(Self{linux,state:Signal::default(),style:SignalStyle::Dot,enabled:true,signal})
        }
        #[cfg(not(any(windows,target_os="macos",target_os="linux")))] {let _=proxy;None}
    }
    pub fn refresh_icon(&mut self,signal:nus_render::Color){if self.signal!=signal{self.signal=signal;self.paint();}}
    pub fn anchor(&self)->Option<crate::menu_drawer::Anchor>{
        #[cfg(any(windows,target_os="macos"))] {self.icon.rect().map(|r|crate::menu_drawer::Anchor{x:r.position.x,y:r.position.y,width:r.size.width,height:r.size.height})}
        #[cfg(not(any(windows,target_os="macos")))] {None}
    }
    pub fn update(&mut self,state:Signal,config:&Config){
        let changed=self.state!=state||self.style!=config.signal||self.enabled!=config.enabled;self.state=state;self.style=config.signal;
        #[cfg(any(windows,target_os="macos"))]
        {if self.enabled!=config.enabled{let _=self.icon.set_visible(config.enabled);}if changed{let _=self.icon.set_tooltip(Some(format!("nus · {}",state.text())));self.work.set_text(format!("Open nus drawer · {}",state.short()));}}
        self.enabled=config.enabled;if changed{self.paint();}
    }
    fn paint(&self){
        #[cfg(any(windows,target_os="macos"))]
        {
            let badge=badge(self.state,self.style,cfg!(target_os="macos"));
            let rgba=nus_render::dock_icon::tray(36,self.signal,badge.as_deref());if let Ok(icon)=tray_icon::Icon::from_rgba(rgba,36,36){let _=self.icon.set_icon(Some(icon));}
            #[cfg(target_os="macos")] self.icon.set_title(match self.style{SignalStyle::Dot=>None,SignalStyle::Count=>Some(self.state.count().to_string()),SignalStyle::Text=>Some(self.state.short())});
        }
        #[cfg(target_os="linux")] self.linux.update(linux::State{signal:self.signal,work:self.state,style:self.style,enabled:self.enabled});
        self.trace_icon();
    }
    fn trace_icon(&self){
        #[cfg(target_os="macos")]
        if std::env::var_os("NUS_SHOT").is_some(){let Some(mtm)=objc2::MainThreadMarker::new()else{return;};if let Some(dir)=std::env::var_os("NUS_DOCK_TRACE").map(std::path::PathBuf::from){if let Some(data)=self.icon.ns_status_item().and_then(|i|i.button(mtm)).and_then(|b|b.image()).and_then(|i|i.TIFFRepresentation()){let rgb=self.signal.map(|v|(v*255.0).round()as u8);let _=std::fs::create_dir_all(&dir);let _=std::fs::write(dir.join(format!("menu-{:02x}{:02x}{:02x}.tiff",rgb[0],rgb[1],rgb[2])),data.to_vec());let _=std::fs::write(dir.join("signal.txt"),format!("{:?}\n{}\n{}",self.style,self.state.text(),self.enabled));}}}
    }
}

pub(crate) fn badge(state:Signal,style:SignalStyle,native_text:bool)->Option<String>{
    if native_text&&style!=SignalStyle::Dot{return None;}
    if state.attention>0{return Some("!".into());}
    if state.count()>0{return Some(if style==SignalStyle::Count{if state.count()>9{"9+".into()}else{state.count().to_string()}}else{String::new()});}
    (state.finished>0).then(String::new)
}
