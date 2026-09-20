//! Signal is the entry point; each drawer section independently chooses its
//! density. Compact work lists and expanded Desk cards share one live model.
use serde::{Deserialize, Serialize};
use crate::hatch_work::{Item, Status};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalStyle { #[default] Dot, Count, Text }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Module { Work, Downloads, Shortcuts }
impl Module {
    pub const ALL: [Self; 3] = [Self::Work, Self::Downloads, Self::Shortcuts];
    pub fn label(self) -> &'static str { match self { Self::Work=>"Work", Self::Downloads=>"Downloads", Self::Shortcuts=>"Quick actions" } }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Density { Hidden, #[default] Compact, Expanded }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section { pub module: Module, pub density: Density }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub signal: SignalStyle,
    pub sections: Vec<Section>,
    pub names: bool,
    pub recent: bool,
}
impl Default for Config {
    fn default()->Self {Self{enabled:true,signal:SignalStyle::Dot,sections:vec![Section{module:Module::Work,density:Density::Compact},Section{module:Module::Downloads,density:Density::Expanded},Section{module:Module::Shortcuts,density:Density::Expanded}],names:true,recent:true}}
}
impl Config {
    pub fn sections(&self)->Vec<Section>{
        let mut out=Vec::new();
        for section in &self.sections {if !out.iter().any(|s:&Section|s.module==section.module){out.push(*section);}}
        for module in Module::ALL {if !out.iter().any(|s|s.module==module){out.push(Section{module,density:Density::Hidden});}}
        out
    }
    pub fn set_density(&mut self,module:Module,density:Density){self.sections=self.sections();if let Some(s)=self.sections.iter_mut().find(|s|s.module==module){s.density=density;}}
    pub fn move_section(&mut self,module:Module,down:bool){self.sections=self.sections();if let Some(i)=self.sections.iter().position(|s|s.module==module){let j=if down{(i+1).min(self.sections.len()-1)}else{i.saturating_sub(1)};self.sections.swap(i,j);}}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signal {pub running:usize,pub attention:usize,pub finished:usize,pub downloads:usize}
impl Signal {
    pub fn collect(work:&[Item],downloads:&[crate::downloads::Download])->Self{Self{running:work.iter().filter(|i|i.status==Status::Running).count(),attention:work.iter().filter(|i|i.status.attention()&&(i.unread||matches!(i.status,Status::NeedsInput|Status::Attention))).count(),finished:work.iter().filter(|i|i.status==Status::Finished&&i.unread).count(),downloads:downloads.iter().filter(|d|d.active()).count()}}
    pub fn count(self)->usize{self.running+self.attention+self.downloads}
    pub fn text(self)->String{
        let mut parts=Vec::new();
        if self.attention>0{parts.push(format!("{} need attention",self.attention));}
        if self.running>0{parts.push(format!("{} running",self.running));}
        if self.downloads>0{parts.push(format!("{} active download{}",self.downloads,if self.downloads==1{""}else{"s"}));}
        if self.finished>0{parts.push(format!("{} finished",self.finished));}
        if parts.is_empty(){"All quiet".into()}else{parts.join(" · ")}
    }
    pub fn short(self)->String{if self.attention>0{format!("{} need you",self.attention)}else if self.count()>0{format!("{} active",self.count())}else{"Quiet".into()}}
}

/// Physical desktop pixels, matching winit and tray-icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {pub x:f64,pub y:f64,pub width:u32,pub height:u32}
pub fn place(a:Anchor,monitor:(i32,i32,u32,u32),size:(u32,u32),gap:i32)->(i32,i32){
    let (mx,my,mw,mh)=monitor;let (w,h)=(size.0.min(mw) as i32,size.1.min(mh) as i32);
    let x=(a.x as i32+a.width as i32-w).clamp(mx,mx+mw as i32-w);
    let below=a.y as i32+a.height as i32+gap;
    let y=if below+h<=my+mh as i32 {below}else{a.y as i32-h-gap};
    (x,y.clamp(my,my+mh as i32-h))
}
#[path="menu_drawer_ui.rs"] mod ui;
pub use ui::State;

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn mixed_modules_roundtrip(){let mut c=Config::default();c.set_density(Module::Work,Density::Expanded);c.set_density(Module::Downloads,Density::Hidden);c.move_section(Module::Shortcuts,false);c.names=false;assert_eq!(serde_json::from_slice::<Config>(&serde_json::to_vec(&c).unwrap()).unwrap(),c);assert_eq!(c.sections()[1].module,Module::Shortcuts);assert_eq!(c.sections()[2].density,Density::Hidden);assert_eq!(serde_json::from_str::<Config>("{}").unwrap(),Config::default());}
    #[test] fn module_list_is_bounded(){let mut c=Config::default();c.sections=vec![c.sections[0];500];assert_eq!(c.sections().len(),3);assert_eq!(c.sections()[1].density,Density::Hidden);c.move_section(Module::Work,false);assert_eq!(c.sections[0].module,Module::Work);}
    #[test] fn placement_handles_negative_origins_and_small_screens(){assert_eq!(place(Anchor{x:-2.0,y:840.0,width:20,height:30},(-1920,-200,1920,1080),(380,520),6),(-380,314));assert_eq!(place(Anchor{x:120.0,y:0.0,width:24,height:24},(0,0,320,240),(320,240),6),(0,0));}
    #[test] fn quiet_is_not_work(){let signal=Signal::collect(&[],&[]);assert_eq!(signal.count(),0);assert_eq!(signal.text(),"All quiet");}
    #[test] fn signal_ignores_idle_shells_and_seen_failures(){
        let item=|status,unread|Item{target:crate::hatch_work::Target{window:1,tab:2,right:false},title:String::new(),command:String::new(),space:String::new(),cwd:String::new(),status,exit:None,progress:None,unread};
        let work=[item(Status::Idle,true),item(Status::Failed,false),item(Status::Failed,true),item(Status::Running,false),item(Status::NeedsInput,false),item(Status::Finished,true)];
        let downloads=[crate::downloads::Download{live:true,paused:true,..Default::default()},crate::downloads::Download{done:true,..Default::default()}];
        let signal=Signal::collect(&work,&downloads);
        assert_eq!(signal,Signal{running:1,attention:2,finished:1,downloads:1});assert_eq!(signal.count(),4);
    }
}
