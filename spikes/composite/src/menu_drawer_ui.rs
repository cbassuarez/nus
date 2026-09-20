use super::{Anchor, Config, Density, Module, Signal};
use crate::app::{App, Action};
use nus_render::{Rect, Scene, Style};
use nus_render::text::icons;
use std::{sync::Arc,time::Instant};
use winit::{window::Window,event::{WindowEvent,ElementState,MouseButton,MouseScrollDelta},keyboard::{Key,NamedKey}};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Hit { Close, Main, Customize, Quit, Work(crate::hatch_work::Target), AllWork, Downloads, Download(crate::downloads::Hit), NewTab, Terminal, NewWindow, Search }
#[derive(Default)]
pub struct State {
    pub window:Option<Drawer>,pub request:bool,pub anchor:Option<Anchor>,
    pub footer:Option<Rect>,pub dismissed:Option<Instant>,pub foreground:crate::hatch_native::Foreground,
}
pub struct Drawer {
    pub window:Arc<Window>,pub target:nus_render::gpu::Target,pub scene:Scene,pub visible:bool,
    pub hits:Vec<(Rect,Hit,String,bool)>,pub pos:(f32,f32),pub focus:Option<usize>,
    pub scroll:f32,pub reach:f32,pub viewport:Rect,pub max_height:f32,pub monitor:(i32,i32,u32,u32),pub anchor:Anchor,pub shift:bool,pub focused:bool,
}
struct Row {label:String,detail:String,hit:Option<Hit>,icon:(&'static str,&'static str),height:f32,progress:Option<f32>,tile:bool,secondary:Option<(Hit,&'static str)>}
impl Row {
    fn title(label:&str)->Self{Self{label:label.into(),detail:String::new(),hit:None,icon:icons::TERMINAL,height:29.0,progress:None,tile:false,secondary:None}}
    fn button(label:impl Into<String>,detail:impl Into<String>,hit:Hit,icon:(&'static str,&'static str),expanded:bool)->Self{Self{label:label.into(),detail:detail.into(),hit:Some(hit),icon,height:if expanded{60.0}else{34.0},progress:None,tile:false,secondary:None}}
}
fn rows(config:&Config,work:&[crate::hatch_work::Item],downloads:&[crate::downloads::Download])->Vec<Row>{
    let mut rows=Vec::new();
    for section in config.sections().into_iter().filter(|s|s.density!=Density::Hidden){
        let expanded=section.density==Density::Expanded;
        rows.push(Row::title(section.module.label()));
        match section.module {
            Module::Work=>{
                let items:Vec<_>=work.iter().filter(|i|!matches!(i.status,crate::hatch_work::Status::Idle)&& (config.recent||(i.status!=crate::hatch_work::Status::Finished&&(i.status!=crate::hatch_work::Status::Failed||i.unread)))).collect();
                if items.is_empty(){let mut row=Row::title("No active work");row.height=34.0;rows.push(row);}
                for (n,item) in items.iter().take(4).enumerate(){
                    let title=if config.names{item.title.clone()}else{format!("Task {}",n+1)};
                    let detail=if expanded&&config.names{format!("{} · {}",item.status.label(),item.space)}else{item.status.label().into()};
                    let mut row=Row::button(title,detail,Hit::Work(item.target),if item.status.attention(){icons::WARNING}else{icons::TERMINAL},expanded);
                    row.progress=item.progress.map(|p|p as f32/100.0);rows.push(row);
                }
                rows.push(Row::button("All work",format!("{} session{}",work.len(),if work.len()==1{""}else{"s"}),Hit::AllWork,icons::OPEN_EXTERNAL,false));
            }
            Module::Downloads=>{
                let mut items:Vec<_>=downloads.iter().filter(|d|d.active()||config.recent).collect();items.sort_by_key(|d|!d.active());
                if items.is_empty(){let mut row=Row::title("No downloads");row.height=34.0;rows.push(row);}
                for (n,d) in items.iter().take(3).enumerate(){
                    let title=if config.names{d.name.clone()}else{format!("Download {}",n+1)};
                    let mut row=Row::button(title,d.status(),Hit::Downloads,icons::DOWNLOAD,expanded);
                    if expanded&&d.active(){row.progress=Some(if d.total>0{(d.received as f32/d.total as f32).clamp(0.0,1.0)}else{0.0});row.secondary=Some(if d.paused{(Hit::Download(crate::downloads::Hit::Resume(d.key)),"Resume")}else{(Hit::Download(crate::downloads::Hit::Pause(d.key)),"Pause")});}
                    if expanded&&d.done{row.secondary=Some((Hit::Download(crate::downloads::Hit::Reveal(d.key)),"Show"));}
                    rows.push(row);
                }
                rows.push(Row::button("All downloads","",Hit::Downloads,icons::OPEN_EXTERNAL,false));
            }
            Module::Shortcuts=>{
                for (label,hit,icon) in [("New tab",Hit::NewTab,icons::PLUS),("Terminal",Hit::Terminal,icons::TERMINAL),("New window",Hit::NewWindow,icons::SQUARES),("Search",Hit::Search,icons::SEARCH)]{
                    let mut row=Row::button(label,"",hit,icon,false);row.tile=expanded;row.height=if expanded{48.0}else{34.0};rows.push(row);
                }
            }
        }
    }
    if rows.is_empty(){rows.push(Row::title("Your drawer is empty"));rows.push(Row::button("Choose sections in Settings","",Hit::Customize,icons::SLIDERS,false));}
    rows
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn mixed_sections_keep_real_targets_but_hide_names(){
        let target=crate::hatch_work::Target{window:7,tab:9,right:true};
        let work=vec![crate::hatch_work::Item{target,title:"Private project".into(),command:"secret command".into(),space:"Private space".into(),cwd:"/secret".into(),status:crate::hatch_work::Status::Running,exit:None,progress:Some(37),unread:false}];
        let downloads=vec![crate::downloads::Download{key:8,name:"Private file.pdf".into(),live:true,received:50,total:100,..Default::default()}];
        let mut config=Config::default();config.names=false;
        let mixed=rows(&config,&work,&downloads);
        assert!(mixed.iter().all(|r|!r.label.contains("Private")&&!r.detail.contains("Private")));
        assert!(mixed.iter().any(|r|r.hit==Some(Hit::Work(target))&&r.height==34.0));
        assert!(mixed.iter().any(|r|r.secondary==Some((Hit::Download(crate::downloads::Hit::Pause(8)),"Pause"))&&r.progress==Some(0.5)&&r.height==60.0));
        assert_eq!(mixed.iter().filter(|r|r.tile).count(),4);
        config.recent=false;
        let mut ended=work[0].clone();ended.status=crate::hatch_work::Status::Failed;
        assert!(!rows(&config,&[ended.clone()],&[]).iter().any(|r|r.hit==Some(Hit::Work(target))));
        ended.unread=true;
        assert!(rows(&config,&[ended.clone()],&[]).iter().any(|r|r.hit==Some(Hit::Work(target))));
        ended.status=crate::hatch_work::Status::Finished;
        assert!(!rows(&config,&[ended],&[]).iter().any(|r|r.hit==Some(Hit::Work(target))));
        config.set_density(Module::Downloads,Density::Hidden);
        assert!(!rows(&config,&work,&downloads).iter().any(|r|r.hit==Some(Hit::Downloads)));
        for module in Module::ALL{config.set_density(module,Density::Hidden);}
        assert_eq!(rows(&config,&work,&downloads)[1].hit,Some(Hit::Customize));
    }
}

impl App {
    pub(crate) fn toggle_menu_drawer(&mut self,anchor:Option<Anchor>){
        if self.menu_drawer.window.as_ref().is_some_and(|d|d.visible){self.hide_menu_drawer(true);return;}
        // Focus loss can arrive just before the status item's press event.
        if anchor.is_some()&&self.menu_drawer.dismissed.is_some_and(|t|crate::clock::since(t).as_millis()<180){return;}
        self.menu_drawer.anchor=anchor;
        self.menu_drawer.foreground=crate::hatch_native::Foreground::capture();
        if self.menu_drawer.window.is_none(){self.menu_drawer.request=true;return;}
        self.show_menu_drawer();
    }
    pub(crate) fn menu_geometry(&self)->(Anchor,(i32,i32,u32,u32),f32){
        let anchor=self.menu_drawer.anchor;
        let monitor=anchor.and_then(|a|self.window.available_monitors().find(|m|{let p=m.position();let s=m.size();a.x>=p.x as f64&&a.y>=p.y as f64&&a.x<(p.x+s.width as i32)as f64&&a.y<(p.y+s.height as i32)as f64})).or_else(||self.window.current_monitor()).or_else(||self.window.primary_monitor());
        let (bounds,scale)=monitor.map(|m|{let p=m.position();let s=m.size();((p.x,p.y,s.width,s.height),m.scale_factor()as f32)}).unwrap_or(((0,0,1280,800),1.0));
        let a=anchor.unwrap_or(Anchor{x:(bounds.0+bounds.2 as i32-20)as f64,y:bounds.1 as f64+32.0*scale as f64,width:0,height:0});
        (a,bounds,scale)
    }
    pub(crate) fn menu_footer_anchor(&self)->Option<Anchor>{let r=self.menu_drawer.footer?;let p=self.window.inner_position().ok()?;Some(Anchor{x:p.x as f64+r.x as f64,y:p.y as f64+r.y as f64,width:r.w as u32,height:r.h as u32})}
    pub(crate) fn attach_menu_drawer(&mut self,window:Arc<Window>){
        crate::hatch_native::configure(&window);crate::macos::prepare_window(&window);
        let Ok(target)=self.gpu.target(window.clone())else{return;};let (anchor,monitor,scale)=self.menu_geometry();
        self.menu_drawer.window=Some(Drawer{window,target,scene:Scene::new(),visible:false,hits:Vec::new(),pos:(-1.0,-1.0),focus:None,scroll:0.0,reach:0.0,viewport:Rect::new(0.0,0.0,1.0,1.0),max_height:(monitor.3 as f32/scale-80.0).clamp(160.0,720.0),monitor,anchor,shift:false,focused:false});
        self.show_menu_drawer();
    }
    fn show_menu_drawer(&mut self){
        let (anchor,monitor,scale)=self.menu_geometry();
        if let Some(d)=&mut self.menu_drawer.window{d.anchor=anchor;d.monitor=monitor;d.max_height=(monitor.3 as f32/scale-80.0).clamp(160.0,720.0);d.visible=true;d.scroll=0.0;d.focus=None;d.focused=false;}
        self.menu_drawer_frame();
        if let Some(d)=&self.menu_drawer.window{if crate::hatch_native::interactive(){d.window.set_visible(true);d.window.focus_window();}else{crate::hatch_native::show_passive(&d.window);}d.window.request_redraw();}
    }
    pub(crate) fn hide_menu_drawer(&mut self,restore:bool){
        self.menu_drawer.request=false;
        if let Some(d)=&mut self.menu_drawer.window{d.visible=false;d.window.set_visible(false);}
        if restore&&crate::hatch_native::interactive(){self.menu_drawer.foreground.restore();}
    }
    fn drawer_button(&mut self,d:&mut Drawer,r:Rect,label:&str,hit:Hit,content:bool,card:bool){
        let s=d.window.scale_factor()as f32;
        if card{d.scene.rect(r,crate::surface::mix(self.theme.paper,self.theme.ink,0.04));d.scene.outline(r,s,self.theme.dim);}
        if r.contains(d.pos.0,d.pos.1)&&(!content||d.viewport.contains(d.pos.0,d.pos.1)){d.scene.rect(r,crate::surface::mix(self.theme.paper,self.theme.ink,0.09));}
        if d.focus==Some(d.hits.len()){d.scene.outline(r.inset(s),2.0*s,self.surface.signal);}
        d.hits.push((r,hit,label.into(),content));
    }
    pub(crate) fn menu_drawer_frame(&mut self){
        let Some(mut d)=self.menu_drawer.window.take()else{return;};
        if !d.visible{self.menu_drawer.window=Some(d);return;}
        let scale=d.window.scale_factor()as f32;let px=|n:f32|(n*scale).round();
        let downloads=crate::downloads::list();let rows=rows(&self.behavior.menu_drawer,&self.hatch_state.work,&downloads);
        let mut content=12.0;let mut column=0;
        for row in &rows{if row.tile{if column==0{content+=row.height+6.0;}column=1-column;}else{column=0;content+=row.height+4.0;}}
        let height=(content+125.0).min(d.max_height).max(160.0).min(d.monitor.3 as f32/scale);let width=380.0f32.min(d.monitor.2 as f32/scale);
        let size=(px(width).max(1.0)as u32,px(height).max(1.0)as u32);
        if d.target.size!=size{let _=d.window.request_inner_size(winit::dpi::PhysicalSize::new(size.0,size.1));d.target.resize(&self.gpu.device,size.0,size.1);}
        let pos=super::place(d.anchor,d.monitor,size,px(5.0)as i32);
        if !crate::hatch_native::wayland(){d.window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.0,pos.1));}
        let (w,h)=(size.0 as f32,size.1 as f32);let ink=self.theme.ink;let paper=self.theme.paper;
        let label=Style{font:self.f.ui,px:px(12.0),color:ink,tracking:0.0};let small=Style{px:px(11.0),color:self.theme.dim,..label};
        d.scene.clear();d.scene.layer(None);d.hits.clear();d.scene.rect(Rect::new(0.0,0.0,w,h),paper);d.scene.outline(Rect::new(0.0,0.0,w,h),px(1.0),ink);
        if let Some(bind)=self.desktop_icon(){d.scene.texture(Rect::new(px(13.0),px(15.0),px(26.0),px(26.0)),bind,None);}
        self.fonts.draw(&mut d.scene,Style{font:self.f.wordmark,px:px(31.0),..label},px(49.0),px(38.0),"nus");
        let close=Rect::new(w-px(40.0),px(10.0),px(30.0),px(32.0));self.drawer_button(&mut d,close,"Close drawer",Hit::Close,false,false);self.fonts.draw_icon(&mut d.scene,icons::CLOSE,px(15.0),close.x+px(8.0),close.y+px(8.0),ink);
        let summary=self.fit(small,&Signal::collect(&self.hatch_state.work,&downloads).text(),w-px(28.0));self.fonts.draw(&mut d.scene,small,px(14.0),px(61.0),&summary);
        d.scene.hline(px(12.0),px(73.0),w-px(24.0),px(1.0),ink);
        d.viewport=Rect::new(px(8.0),px(79.0),w-px(16.0),h-px(125.0));d.reach=(px(content)-d.viewport.h).max(0.0);d.scroll=d.scroll.clamp(0.0,d.reach);d.scene.layer(Some(d.viewport));
        let mut y=d.viewport.y+px(4.0)-d.scroll;column=0;
        for row in rows{
            let tile_width=(d.viewport.w-px(14.0))/2.0;
            let r=Rect::new(d.viewport.x+px(3.0)+if row.tile{column as f32*(tile_width+px(8.0))}else{0.0},y,if row.tile{tile_width}else{d.viewport.w-px(6.0)},px(row.height));
            if let Some(hit)=row.hit{
                let accessible=if row.detail.is_empty(){row.label.clone()}else{format!("{}, {}",row.label,row.detail)};
                self.drawer_button(&mut d,r,&accessible,hit,true,row.tile||row.height>40.0);
                self.fonts.draw_icon(&mut d.scene,row.icon,px(14.0),r.x+px(8.0),r.y+px(if row.height>40.0&&!row.tile{11.0}else{(row.height-14.0)/2.0}),if matches!(row.hit,Some(Hit::Work(_))){self.surface.signal}else{ink});
                let reserved=if row.secondary.is_some(){px(65.0)}else{px(8.0)};
                let title_width=if row.height<=40.0&&!row.detail.is_empty(){r.w*0.55-px(30.0)}else{r.w-px(38.0)-reserved};
                let title=self.fit(label,&row.label,title_width);let baseline=r.y+px(if row.height>40.0&&!row.tile{23.0}else{row.height/2.0+4.0});
                self.fonts.draw(&mut d.scene,label,r.x+px(30.0),baseline,&title);
                if row.height>40.0&&!row.tile{let detail=self.fit(small,&row.detail,r.w-px(40.0)-reserved);self.fonts.draw(&mut d.scene,small,r.x+px(30.0),r.y+px(42.0),&detail);}
                else if !row.tile&&!row.detail.is_empty(){let status=self.fit(small,&row.detail,r.w*0.35);let available=r.w-self.fonts.measure(label,&title)-px(46.0);if self.fonts.measure(small,&status)<=available{let x=r.right()-px(8.0)-self.fonts.measure(small,&status);self.fonts.draw(&mut d.scene,small,x,baseline,&status);}}
                if let Some(progress)=row.progress.filter(|_|row.height>40.0){let bar=Rect::new(r.x+px(10.0),r.bottom()-px(6.0),r.w-px(20.0),px(2.0));d.scene.rect(bar,crate::surface::mix(paper,ink,0.12));d.scene.rect(Rect::new(bar.x,bar.y,bar.w*progress,bar.h),self.surface.signal);}
                if let Some((action,words))=row.secondary{let b=Rect::new(r.right()-px(64.0),r.y+px(10.0),px(58.0),px(34.0));self.drawer_button(&mut d,b,&format!("{words} {}",row.label),action,true,true);self.fonts.draw(&mut d.scene,small,b.x+px(8.0),b.y+px(22.0),words);}
            }else{self.fonts.draw(&mut d.scene,Style{font:self.f.strong,..small},r.x+px(7.0),r.y+px(21.0),&row.label);}
            if row.tile{column=1-column;if column==0{y+=r.h+px(6.0);}}else{column=0;y+=r.h+px(4.0);}
        }
        d.scene.layer(None);let fy=h-px(39.0);d.scene.hline(px(12.0),fy-px(4.0),w-px(24.0),px(1.0),ink);
        let cell=(w-px(24.0))/3.0;
        for (i,(words,hit)) in [("Open nus",Hit::Main),("Customize",Hit::Customize),("Quit nus",Hit::Quit)].into_iter().enumerate(){let r=Rect::new(px(12.0)+i as f32*cell,fy,cell,px(31.0));self.drawer_button(&mut d,r,words,hit,false,false);let x=r.x+(r.w-self.fonts.measure(small,words))/2.0;self.fonts.draw(&mut d.scene,small,x,r.y+px(20.0),words);}
        if d.reach>0.0{let track=d.viewport.h;let thumb=(track*track/(track+d.reach)).max(px(18.0));d.scene.rect(Rect::new(w-px(5.0),d.viewport.y+(track-thumb)*d.scroll/d.reach,px(2.0),thumb),self.theme.dim);}
        d.scene.finish();for(x,y,w,h,data)in self.fonts.uploads.drain(..){self.gpu.upload_glyph(x,y,w,h,&data);}self.gpu.render(&mut d.target,&d.scene,paper);self.menu_drawer.window=Some(d);
    }
    pub(crate) fn menu_drawer_action(&mut self,hit:Hit){
        match hit{
            Hit::Close=>self.hide_menu_drawer(true),
            Hit::Download(action)=>{self.download_action(action);self.menu_drawer_frame();},
            Hit::Work(target)=>{self.hide_menu_drawer(false);let _=self.proxy.send_event(crate::UserEvent::MenuSelect(target));},
            Hit::AllWork=>{self.hide_menu_drawer(false);self.show_hatch_work();},
            Hit::Quit=>{let _=self.proxy.send_event(crate::UserEvent::HatchQuit);},
            other=>{self.hide_menu_drawer(false);self.hatch_state.main_hidden=false;self.window.set_visible(true);self.window.set_minimized(false);if crate::hatch_native::interactive(){self.window.focus_window();}match other{Hit::Customize=>self.run(Action::SettingsAt(crate::settings::SEC_MENU,None)),Hit::Downloads=>self.open_downloads(),Hit::NewTab=>self.open_start_page(false),Hit::Terminal=>self.new_tab(self.behavior.default_profile),Hit::NewWindow=>self.new_window_request=true,Hit::Search=>self.run(Action::OpenPalette(crate::app::PaletteMode::Go)),_=>{}}self.dirty=true;}
        }
    }
    pub(crate) fn menu_drawer_event(&mut self,event:WindowEvent){
        match event{
            WindowEvent::CloseRequested=>self.hide_menu_drawer(true),
            WindowEvent::Focused(true)=>{if let Some(d)=&mut self.menu_drawer.window{d.focused=true;}},
            // Native windows can emit an initial unfocused event before their first focus.
            WindowEvent::Focused(false)=>{if self.menu_drawer.window.as_ref().is_some_and(|d|d.visible&&d.focused){self.menu_drawer.dismissed=Some(crate::clock::now());self.hide_menu_drawer(false);}},
            WindowEvent::RedrawRequested|WindowEvent::Resized(_)|WindowEvent::ScaleFactorChanged{..}=>self.menu_drawer_frame(),
            WindowEvent::CursorMoved{position,..}=>{if let Some(d)=&mut self.menu_drawer.window{d.pos=(position.x as f32,position.y as f32);d.window.request_redraw();}},
            WindowEvent::CursorLeft{..}=>{if let Some(d)=&mut self.menu_drawer.window{d.pos=(-1.0,-1.0);d.window.request_redraw();}},
            WindowEvent::MouseInput{button:MouseButton::Left,state:ElementState::Released,..}=>{let hit=self.menu_drawer.window.as_ref().and_then(|d|d.hits.iter().rev().find(|(r,_,_,content)|r.contains(d.pos.0,d.pos.1)&&(!content||d.viewport.contains(d.pos.0,d.pos.1))).map(|(_,h,_,_)|*h));if let Some(hit)=hit{self.menu_drawer_action(hit);}},
            WindowEvent::MouseWheel{delta,..}=>{if let Some(d)=&mut self.menu_drawer.window{let dy=match delta{MouseScrollDelta::LineDelta(_,y)=>y*34.0*d.window.scale_factor()as f32,MouseScrollDelta::PixelDelta(p)=>p.y as f32};d.scroll=(d.scroll-dy).clamp(0.0,d.reach);d.window.request_redraw();}},
            WindowEvent::ModifiersChanged(m)=>{if let Some(d)=&mut self.menu_drawer.window{d.shift=m.state().shift_key();}},
            WindowEvent::KeyboardInput{event,..} if event.state==ElementState::Pressed=>{
                if event.logical_key==Key::Named(NamedKey::Escape){self.hide_menu_drawer(true);return;}
                if matches!(event.logical_key,Key::Named(NamedKey::Enter|NamedKey::Space)){if let Some(hit)=self.menu_drawer.window.as_ref().and_then(|d|d.focus.and_then(|i|d.hits.get(i))).map(|(_,h,_,_)|*h){self.menu_drawer_action(hit);}return;}
                if let Some(d)=&mut self.menu_drawer.window{
                    match event.logical_key{
                        Key::Named(NamedKey::Tab|NamedKey::ArrowDown|NamedKey::ArrowUp)=>{if !d.hits.is_empty(){let prev=event.logical_key==Key::Named(NamedKey::ArrowUp)||(event.logical_key==Key::Named(NamedKey::Tab)&&d.shift);let i=match d.focus{Some(i) if prev=>(i+d.hits.len()-1)%d.hits.len(),Some(i)=>(i+1)%d.hits.len(),None if prev=>d.hits.len()-1,None=>0};let (r,_,_,content)=&d.hits[i];d.focus=Some(i);if *content{if r.y<d.viewport.y{d.scroll-=(d.viewport.y-r.y).min(d.scroll);}else if r.bottom()>d.viewport.bottom(){d.scroll=(d.scroll+r.bottom()-d.viewport.bottom()).min(d.reach);}}}},
                        Key::Named(NamedKey::PageDown)=>d.scroll=(d.scroll+d.viewport.h*0.8).min(d.reach),Key::Named(NamedKey::PageUp)=>d.scroll=(d.scroll-d.viewport.h*0.8).max(0.0),Key::Named(NamedKey::Home)=>d.scroll=0.0,Key::Named(NamedKey::End)=>d.scroll=d.reach,_=>{}
                    }d.window.request_redraw();
                }
            },_=>{}
        }
    }
    pub(crate) fn menu_drawer_access_tree(&self)->accesskit::TreeUpdate{
        use accesskit::{Node,NodeId,Role,TreeInfo,TreeId,TreeUpdate,Action};let mut children=Vec::new();let mut nodes=Vec::new();let mut focus=NodeId(1);
        if let Some(d)=&self.menu_drawer.window{for (n,(r,_,label,content)) in d.hits.iter().enumerate(){if *content&&(r.bottom()<=d.viewport.y||r.y>=d.viewport.bottom()){continue;}let id=NodeId(n as u64+10);let mut node=Node::new(Role::Button);node.set_label(label.clone());node.add_action(Action::Click);let (top,bottom)=if *content{(r.y.max(d.viewport.y),r.bottom().min(d.viewport.bottom()))}else{(r.y,r.bottom())};node.set_bounds(accesskit::Rect{x0:r.x as f64,y0:top as f64,x1:r.right()as f64,y1:bottom as f64});if d.focus==Some(n){focus=id;}nodes.push((id,node));children.push(id);}}
        let mut root=Node::new(Role::Window);root.set_label("nus menu drawer");root.set_children(children);nodes.push((NodeId(1),root));TreeUpdate{nodes,tree:Some(TreeInfo::new(NodeId(1))),tree_id:TreeId::ROOT,focus}
    }
    pub(crate) fn menu_drawer_access_action(&mut self,request:accesskit::ActionRequest){if request.action==accesskit::Action::Click{let hit=self.menu_drawer.window.as_ref().and_then(|d|d.hits.get(request.target_node.0.saturating_sub(10)as usize)).map(|(_,h,_,_)|*h);if let Some(hit)=hit{self.menu_drawer_action(hit);}}}
}
