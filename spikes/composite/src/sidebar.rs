//! Resizable sidebar and footer; the same controls stay reachable at every width.
use crate::app::{App,SideHit,IconMotion,hover_key,Pane};
use nus_render::{Rect,Scene};
use nus_render::text::icons;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,serde::Serialize,serde::Deserialize)]
pub enum SmallTabs { Icons, #[default] Favicons, Preview }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Resize {Width,Footer}
pub fn default_width()->f32{248.0}
pub fn default_footer()->f32{32.0}
pub fn width(value:f32,available:f32)->f32 {if !value.is_finite(){return default_width().min(available.max(48.0));}value.clamp(48.0,480.0).min((available*0.72).max(48.0)).round()}
impl App {
    pub(crate) fn sidebar_icons(&self)->bool {self.compact()||self.sidebar_w()<=self.px(104.0)}
    pub(crate) fn sidebar_footer_h(&self)->f32{
        let count=if self.sidebar_icons(){6}else{8};let cols=((self.list_rect().w/self.px(30.0)).floor() as usize).clamp(1,count);
        let rows=count.div_ceil(cols);let row=self.sidebar_rules.footer_row.clamp(28.0,64.0);
        self.px(row*rows as f32).min(self.sidebar_rect().h*0.45).round()
    }
    pub(crate) fn sidebar_resize_at(&self,x:f32,y:f32)->Option<Resize>{
        if !self.sidebar_visible()||self.dl_menu{return None;}
        let r=self.sidebar_rect();let grab=self.px(4.0);let edge=if self.sidebar_right(){r.x}else{r.right()};
        if (x-edge).abs()<=grab&&y>=r.y&&y<=r.bottom(){return Some(Resize::Width);}
        if x>=r.x&&x<=r.right()&&(y-(r.bottom()-self.sidebar_footer_h())).abs()<=grab{return Some(Resize::Footer);}
        None
    }
    pub(crate) fn sidebar_resize_to(&mut self,x:f32,y:f32){
        let Some(kind)=self.sidebar_resize else{return;};let r=self.sidebar_rect();
        match kind {
            Resize::Width=>{let w=if self.sidebar_right(){r.right()-x}else{x-r.x};self.sidebar_rules.compact=false;self.sidebar_rules.width=width(w/self.scale,self.target.size.0 as f32/self.scale);},
            Resize::Footer=>{let count=if self.sidebar_icons(){6usize}else{8};let cols=((self.list_rect().w/self.px(30.0)).floor() as usize).clamp(1,count);self.sidebar_rules.footer_row=((r.bottom()-y)/self.scale/count.div_ceil(cols) as f32).clamp(28.0,64.0).round();}
        }
        self.sidebar_leave=None;self.layout();self.dirty=true;
    }
    pub(crate) fn draw_responsive_footer(&mut self,scene:&mut Scene,sb:Rect,fy:f32){
        let count=if self.sidebar_icons(){6usize}else{8};
        let cols=((sb.w/self.px(30.0)).floor() as usize).clamp(1,count);let rows=count.div_ceil(cols);let h=sb.bottom()-fy;let cellh=(h/rows as f32).floor();
        scene.layer(Some(Rect::new(sb.x,fy,sb.w,h)));scene.rect(Rect::new(sb.x,fy,sb.w,h),self.paper());scene.hline(sb.x,fy,sb.w,self.px(1.0),if self.sidebar_resize_at(self.mouse.0,self.mouse.1)==Some(Resize::Footer){self.surface.signal}else{self.theme.ink});
        let items=if self.sidebar_icons(){vec![SideHit::Profile,SideHit::Look,SideHit::Files,SideHit::Downloads,SideHit::MenuDrawer,SideHit::Settings]}else{vec![SideHit::Profile,SideHit::NewTab,SideHit::Look,SideHit::Files,SideHit::Closed,SideHit::Downloads,SideHit::MenuDrawer,SideHit::Settings]};
        for (i,hit) in items.into_iter().enumerate(){
            let col=i%cols;let row=i/cols;let x=(sb.x+sb.w*col as f32/cols as f32).round();let right=(sb.x+sb.w*(col+1) as f32/cols as f32).round();let r=Rect::new(x,fy+row as f32*cellh,right-x,cellh);
            let size=self.px(16.0).min(cellh-self.px(8.0)).max(self.px(10.0));let ix=(r.x+(r.w-size)*0.5).round();let iy=(r.y+(r.h-size)*0.5).round();
            let (icon,words)=match hit{SideHit::Profile=>(icons::USER,"Your profile"),SideHit::NewTab=>(icons::PLUS,"New tab"),SideHit::Look=>(icons::SETTINGS,"Themes"),SideHit::Files=>(icons::FOLDER_SIMPLE,"Files and folders"),SideHit::Closed=>(icons::HISTORY,"Recently closed"),SideHit::Downloads=>(icons::DOWNLOAD,"Downloads"),_=>(icons::SETTINGS,"Settings")};
            if hit==SideHit::Profile {let face=self.me.as_ref().map(|m|m.face.clone()).unwrap_or(crate::me::Face::Initial);let name=self.user_name.clone();self.draw_face(scene,Rect::new(ix,iy,size,size),&face,&name);}
            else if hit==SideHit::MenuDrawer {self.menu_drawer.footer=Some(r);if let Some(bind)=self.desktop_icon(){scene.texture(Rect::new(ix,iy,size,size),bind,None);}let state=crate::menu_drawer::Signal::collect(&self.hatch_state.work,&crate::downloads::list());if state.count()>0{scene.push(nus_render::Instance::rounded(Rect::new(ix+size-self.px(4.0),iy+size-self.px(4.0),self.px(5.0),self.px(5.0)),self.px(2.5),self.surface.signal));}self.foot_tip(hover_key("drawer",i),r,format!("Menu drawer · {}",state.text()));}
            else if hit==SideHit::Look {self.draw_look_chip(scene,r.x+self.px(4.0),r.y,r.h,r.w-self.px(8.0));self.side_hits.pop();}
            else {let active=hit==SideHit::Downloads&&crate::downloads::list().iter().any(|d|d.active());self.icon_button(scene,icon,size,ix,iy,if active{self.surface.signal}else{self.theme.ink},r,hover_key("foot",i),IconMotion::Still);if active{scene.rect(Rect::new(ix,r.bottom()-self.px(4.0),size,self.px(2.0)),self.surface.signal);}}
            if hit==SideHit::Downloads{self.download_ui.anchor=Some(r);}else if hit!=SideHit::MenuDrawer{self.foot_tip(hover_key("foot-tip",i),r,words.into());}
            self.side_hits.push((r,hit));
        }
        scene.layer(None);
    }
    pub(crate) fn draw_small_tab(&mut self,scene:&mut Scene,tab:&crate::app::Tab,x:f32,y:f32,size:f32,color:nus_render::Color)->bool{
        let (pane,_)=tab.panes();
        match self.sidebar_rules.small_tabs{
            SmallTabs::Favicons=>false,
            SmallTabs::Icons=>{let icon=match pane{Pane::Web(_)=>icons::GLOBE,Pane::Term(_)|Pane::Home(_)=>icons::TERMINAL,Pane::Settings(_)=>icons::SETTINGS,Pane::Hints(_)=>icons::HOME,Pane::Editor(_)=>icons::CODE,Pane::Ports(_)=>icons::PORTS,Pane::Downloads(_)=>icons::DOWNLOAD};self.fonts.draw_icon(scene,icon,size,x,y,color);true},
            SmallTabs::Preview=>{if let Pane::Web(w)=pane{if let Some(bind)=w.still.clone().or_else(||w.tab.shared.borrow().bind.clone()){
                let width=(self.sidebar_w()-self.px(12.0)).min(self.px(72.0));let h=(width*0.62).round();let r=Rect::new((x+size*0.5-width*0.5).round(),(y+size*0.5-h*0.5).round(),width,h);scene.texture(r,bind,None);scene.layer(None);scene.outline(r,self.px(1.0),color);return true;
            }}false}
        }
    }
}
#[cfg(test)]mod tests{use super::*;#[test]fn sidebar_width_is_bounded(){assert_eq!(width(10.0,1400.0),48.0);assert_eq!(width(900.0,1400.0),480.0);assert!(width(480.0,320.0)<=231.0);assert!(width(f32::NAN,800.0).is_finite());}}
