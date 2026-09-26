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
    pub preview:Preview,
}
pub struct Drawer {
    pub window:Arc<Window>,pub target:nus_render::gpu::Target,pub scene:Scene,pub visible:bool,
    pub p:Paint,pub max_height:f32,pub monitor:(i32,i32,u32,u32),pub anchor:Anchor,pub shift:bool,pub focused:bool,
}
/// What one drawing of the drawer needs besides a scene: its targets, the
/// pointer, focus and scroll. The drawer window has one; Settings' live
/// preview has its own, so both draw the same drawer.
pub struct Paint {pub hits:Vec<(Rect,Hit,String,bool)>,pub pos:(f32,f32),pub focus:Option<usize>,pub scroll:f32,pub reach:f32,pub viewport:Rect}
impl Default for Paint {fn default()->Self{Self{hits:Vec::new(),pos:(-1.0,-1.0),focus:None,scroll:0.0,reach:0.0,viewport:Rect::new(0.0,0.0,1.0,1.0)}}}
/// Settings' live preview: the drawer drawn offscreen, and its pointer.
pub struct Preview {pub paint:Paint,pub texture:Option<(wgpu::Texture,Arc<wgpu::BindGroup>,(u32,u32))>,pub tray:Option<(String,Arc<wgpu::BindGroup>)>,pub rect:Rect}
impl Default for Preview {fn default()->Self{Self{paint:Paint::default(),texture:None,tray:None,rect:Rect::new(0.0,0.0,0.0,0.0)}}}
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
        self.menu_drawer.window=Some(Drawer{window,target,scene:Scene::new(),visible:false,p:Paint::default(),max_height:(monitor.3 as f32/scale-80.0).clamp(160.0,720.0),monitor,anchor,shift:false,focused:false});
        self.show_menu_drawer();
    }
    fn show_menu_drawer(&mut self){
        let (anchor,monitor,scale)=self.menu_geometry();
        if let Some(d)=&mut self.menu_drawer.window{d.anchor=anchor;d.monitor=monitor;d.max_height=(monitor.3 as f32/scale-80.0).clamp(160.0,720.0);d.visible=true;d.p.scroll=0.0;d.p.focus=None;d.focused=false;}
        self.menu_drawer_frame();
        if let Some(d)=&self.menu_drawer.window{if crate::hatch_native::interactive(){d.window.set_visible(true);d.window.focus_window();}else{crate::hatch_native::show_passive(&d.window);}d.window.request_redraw();}
    }
    pub(crate) fn hide_menu_drawer(&mut self,restore:bool){
        self.menu_drawer.request=false;
        if let Some(d)=&mut self.menu_drawer.window{d.visible=false;d.window.set_visible(false);}
        if restore&&crate::hatch_native::interactive(){self.menu_drawer.foreground.restore();}
    }
    #[allow(clippy::too_many_arguments)]
    fn drawer_button(&mut self,scene:&mut Scene,p:&mut Paint,s:f32,r:Rect,label:&str,hit:Hit,content:bool,card:bool){
        if card{scene.rect(r,crate::surface::mix(self.theme.paper,self.theme.ink,0.04));scene.outline(r,s,self.theme.dim);}
        if r.contains(p.pos.0,p.pos.1)&&(!content||p.viewport.contains(p.pos.0,p.pos.1)){scene.rect(r,crate::surface::mix(self.theme.paper,self.theme.ink,0.09));}
        if p.focus==Some(p.hits.len()){scene.outline(r.inset(s),2.0*s,self.surface.signal);}
        p.hits.push((r,hit,label.into(),content));
    }
    /// The drawer's rows and size in logical px, for the space it has.
    fn drawer_layout(&self,max_w:f32,max_h:f32)->(Vec<Row>,f32,f32,f32){
        let downloads=crate::downloads::list();let rows=rows(&self.behavior.menu_drawer,&self.hatch_state.work,&downloads);
        let mut content=12.0;let mut column=0;
        for row in &rows{if row.tile{if column==0{content+=row.height+6.0;}column=1-column;}else{column=0;content+=row.height+4.0;}}
        let height=(content+125.0).min(max_h).max(160.0);let width=380.0f32.min(max_w);
        (rows,content,width,height)
    }
    pub(crate) fn menu_drawer_frame(&mut self){
        let Some(mut d)=self.menu_drawer.window.take()else{return;};
        if !d.visible{self.menu_drawer.window=Some(d);return;}
        let scale=d.window.scale_factor()as f32;let px=|n:f32|(n*scale).round();
        let (rows,content,width,height)=self.drawer_layout(d.monitor.2 as f32/scale,d.max_height.min(d.monitor.3 as f32/scale));
        let size=(px(width).max(1.0)as u32,px(height).max(1.0)as u32);
        if d.target.size!=size{let _=d.window.request_inner_size(winit::dpi::PhysicalSize::new(size.0,size.1));d.target.resize(&self.gpu.device,size.0,size.1);}
        let pos=super::place(d.anchor,d.monitor,size,px(5.0)as i32);
        if !crate::hatch_native::wayland(){d.window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.0,pos.1));}
        let mut scene=std::mem::take(&mut d.scene);
        self.paint_drawer(&mut scene,&mut d.p,rows,content,(size.0 as f32,size.1 as f32),scale);
        scene.finish();for(x,y,w,h,data)in self.fonts.uploads.drain(..){self.gpu.upload_glyph(x,y,w,h,&data);}
        let paper=self.theme.paper;self.gpu.render(&mut d.target,&scene,paper);d.scene=scene;self.menu_drawer.window=Some(d);
    }
    /// The drawer, into `scene` at the origin, `size` physical px.
    fn paint_drawer(&mut self,scene:&mut Scene,p:&mut Paint,rows:Vec<Row>,content:f32,size:(f32,f32),scale:f32){
        let px=|n:f32|(n*scale).round();let (w,h)=size;let mut column=0;
        let downloads=crate::downloads::list();
        let ink=self.theme.ink;let paper=self.theme.paper;
        let label=Style{font:self.f.ui,px:px(12.0),color:ink,tracking:0.0};let small=Style{px:px(11.0),color:self.theme.dim,..label};
        scene.clear();scene.layer(None);p.hits.clear();scene.rect(Rect::new(0.0,0.0,w,h),paper);scene.outline(Rect::new(0.0,0.0,w,h),px(1.0),ink);
        if let Some(bind)=self.desktop_icon(){scene.texture(Rect::new(px(13.0),px(15.0),px(26.0),px(26.0)),bind,None);}
        self.fonts.draw(scene,Style{font:self.f.wordmark,px:px(31.0),..label},px(49.0),px(38.0),"nus");
        let close=Rect::new(w-px(40.0),px(10.0),px(30.0),px(32.0));self.drawer_button(scene,p,scale,close,"Close drawer",Hit::Close,false,false);self.fonts.draw_icon(scene,icons::CLOSE,px(15.0),close.x+px(8.0),close.y+px(8.0),ink);
        let summary=self.fit(small,&Signal::collect(&self.hatch_state.work,&downloads).text(),w-px(28.0));self.fonts.draw(scene,small,px(14.0),px(61.0),&summary);
        scene.hline(px(12.0),px(73.0),w-px(24.0),px(1.0),ink);
        p.viewport=Rect::new(px(8.0),px(79.0),w-px(16.0),h-px(125.0));p.reach=(px(content)-p.viewport.h).max(0.0);p.scroll=p.scroll.clamp(0.0,p.reach);scene.layer(Some(p.viewport));
        let mut y=p.viewport.y+px(4.0)-p.scroll;
        for row in rows{
            let tile_width=(p.viewport.w-px(14.0))/2.0;
            let r=Rect::new(p.viewport.x+px(3.0)+if row.tile{column as f32*(tile_width+px(8.0))}else{0.0},y,if row.tile{tile_width}else{p.viewport.w-px(6.0)},px(row.height));
            if let Some(hit)=row.hit{
                let accessible=if row.detail.is_empty(){row.label.clone()}else{format!("{}, {}",row.label,row.detail)};
                self.drawer_button(scene,p,scale,r,&accessible,hit,true,row.tile||row.height>40.0);
                self.fonts.draw_icon(scene,row.icon,px(14.0),r.x+px(8.0),r.y+px(if row.height>40.0&&!row.tile{11.0}else{(row.height-14.0)/2.0}),if matches!(row.hit,Some(Hit::Work(_))){self.surface.signal}else{ink});
                let reserved=if row.secondary.is_some(){px(65.0)}else{px(8.0)};
                let title_width=if row.height<=40.0&&!row.detail.is_empty(){r.w*0.55-px(30.0)}else{r.w-px(38.0)-reserved};
                let title=self.fit(label,&row.label,title_width);let baseline=r.y+px(if row.height>40.0&&!row.tile{23.0}else{row.height/2.0+4.0});
                self.fonts.draw(scene,label,r.x+px(30.0),baseline,&title);
                if row.height>40.0&&!row.tile{let detail=self.fit(small,&row.detail,r.w-px(40.0)-reserved);self.fonts.draw(scene,small,r.x+px(30.0),r.y+px(42.0),&detail);}
                else if !row.tile&&!row.detail.is_empty(){let status=self.fit(small,&row.detail,r.w*0.35);let available=r.w-self.fonts.measure(label,&title)-px(46.0);if self.fonts.measure(small,&status)<=available{let x=r.right()-px(8.0)-self.fonts.measure(small,&status);self.fonts.draw(scene,small,x,baseline,&status);}}
                if let Some(progress)=row.progress.filter(|_|row.height>40.0){let bar=Rect::new(r.x+px(10.0),r.bottom()-px(6.0),r.w-px(20.0),px(2.0));scene.rect(bar,crate::surface::mix(paper,ink,0.12));scene.rect(Rect::new(bar.x,bar.y,bar.w*progress,bar.h),self.surface.signal);}
                if let Some((action,words))=row.secondary{let b=Rect::new(r.right()-px(64.0),r.y+px(10.0),px(58.0),px(34.0));self.drawer_button(scene,p,scale,b,&format!("{words} {}",row.label),action,true,true);self.fonts.draw(scene,small,b.x+px(8.0),b.y+px(22.0),words);}
            }else{self.fonts.draw(scene,Style{font:self.f.strong,..small},r.x+px(7.0),r.y+px(21.0),&row.label);}
            if row.tile{column=1-column;if column==0{y+=r.h+px(6.0);}}else{column=0;y+=r.h+px(4.0);}
        }
        scene.layer(None);let fy=h-px(39.0);scene.hline(px(12.0),fy-px(4.0),w-px(24.0),px(1.0),ink);
        let cell=(w-px(24.0))/3.0;
        for (i,(words,hit)) in [("Open nus",Hit::Main),("Customize",Hit::Customize),("Quit nus",Hit::Quit)].into_iter().enumerate(){let r=Rect::new(px(12.0)+i as f32*cell,fy,cell,px(31.0));self.drawer_button(scene,p,scale,r,words,hit,false,false);let x=r.x+(r.w-self.fonts.measure(small,words))/2.0;self.fonts.draw(scene,small,x,r.y+px(20.0),words);}
        if p.reach>0.0{let track=p.viewport.h;let thumb=(track*track/(track+p.reach)).max(px(18.0));scene.rect(Rect::new(w-px(5.0),p.viewport.y+(track-thumb)*p.scroll/p.reach,px(2.0),thumb),self.theme.dim);}
    }
    /// Settings' live preview's height at this scale, in physical px.
    pub(crate) fn drawer_preview_height(&self)->f32{let (_,_,_,h)=self.drawer_layout(380.0,560.0);(h*self.scale).round()+self.px(64.0)}
    /// Settings · Menu: the real drawer, drawn offscreen as the drawer
    /// window draws it, and beside it the real tray icon as it is now.
    pub(crate) fn draw_drawer_preview(&mut self,scene:&mut Scene,r:Rect){
        let scale=self.scale;let (rows,content,width,height)=self.drawer_layout(380.0,560.0);
        let size=(((width*scale).round()as u32).max(1),((height*scale).round()as u32).max(1));
        let at=Rect::new(r.x,r.y,size.0 as f32,size.1 as f32);
        let mut p=std::mem::take(&mut self.menu_drawer.preview.paint);
        p.pos=if at.contains(self.mouse.0,self.mouse.1){(self.mouse.0-at.x,self.mouse.1-at.y)}else{(-1.0,-1.0)};
        let mut own=Scene::new();self.paint_drawer(&mut own,&mut p,rows,content,(size.0 as f32,size.1 as f32),scale);own.finish();
        self.menu_drawer.preview.paint=p;self.menu_drawer.preview.rect=at;
        for(x,y,w,h,data)in self.fonts.uploads.drain(..){self.gpu.upload_glyph(x,y,w,h,&data);}
        if self.menu_drawer.preview.texture.as_ref().is_none_or(|t|t.2!=size){
            let tex=self.gpu.offscreen_texture(size);let bind=(self.bind_texture)(&tex);self.menu_drawer.preview.texture=Some((tex,bind,size));
        }
        let paper=self.theme.paper;
        let Some((tex,bind,_))=self.menu_drawer.preview.texture.as_ref().map(|(t,b,s)|(t.clone(),b.clone(),*s))else{return;};
        self.gpu.render_into(&tex,&own,paper);
        scene.texture(at,bind,scene.clip());scene.layer(scene.clip());
        // The tray icon, as the menu bar shows it now and while work runs.
        let x=at.right()+self.px(28.0);
        if x+self.px(150.0)>r.right(){return;}
        let cfg=self.behavior.menu_drawer.clone();
        let now=Signal::collect(&self.hatch_state.work,&crate::downloads::list());
        let busy=Signal{running:2,..Signal::default()};
        let cap=Style{font:self.f.ui,px:self.px(9.0),color:self.theme.dim,tracking:0.08};
        for (i,(words,state)) in [("MENU BAR · NOW",now),("WHILE TWO THINGS RUN",busy)].into_iter().enumerate(){
            let y=at.y+i as f32*self.px(74.0);
            self.fonts.draw(scene,cap,x,y+self.px(10.0),words);
            let bar=Rect::new(x,y+self.px(18.0),self.px(150.0),self.px(30.0));
            scene.rect(bar,crate::surface::mix(self.theme.paper,self.theme.ink,0.06));scene.outline(bar,self.px(1.0),crate::app::fade(self.theme.ink,0.25));
            if !cfg.enabled{self.fonts.draw(scene,Style{px:self.px(11.0),..cap},bar.x+self.px(10.0),bar.y+self.px(19.0),"off · no icon");continue;}
            let (bind,title)=self.tray_preview(state,&cfg);
            let s=self.px(18.0);scene.texture(Rect::new(bar.x+self.px(10.0),bar.y+(bar.h-s)/2.0,s,s),bind,scene.clip());scene.layer(scene.clip());
            if let Some(t)=title{self.fonts.draw(scene,Style{font:self.f.ui,px:self.px(12.0),color:self.theme.ink,tracking:0.0},bar.x+self.px(34.0),bar.y+self.px(19.0),&t);}
        }
    }
    /// The tray icon's own pixels for `state`, and the text macOS puts beside it.
    fn tray_preview(&mut self,state:Signal,cfg:&Config)->(Arc<wgpu::BindGroup>,Option<String>){
        let mac=cfg!(target_os="macos");
        let badge=crate::hatch_tray::badge(state,cfg.signal,mac);
        let title=if mac{match cfg.signal{super::SignalStyle::Dot=>None,super::SignalStyle::Count=>Some(state.count().to_string()),super::SignalStyle::Text=>Some(state.short())}}else{None};
        let key=format!("{:?}{:?}",badge,self.surface.signal);
        if let Some((k,b))=&self.menu_drawer.preview.tray{if *k==key{return (b.clone(),title);}}
        let size=36u32;let rgba=nus_render::dock_icon::tray(size,self.surface.signal,badge.as_deref());
        let bgra:Vec<u8>=rgba.as_chunks::<4>().0.iter().flat_map(|p|[p[2],p[1],p[0],p[3]]).collect();
        let tex=self.device.create_texture(&wgpu::TextureDescriptor{label:Some("tray preview"),size:wgpu::Extent3d{width:size,height:size,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Bgra8Unorm,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
        self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo{texture:&tex,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},&bgra,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(size*4),rows_per_image:Some(size)},wgpu::Extent3d{width:size,height:size,depth_or_array_layers:1});
        let b=(self.bind_texture)(&tex);self.menu_drawer.preview.tray=Some((key,b.clone()));(b,title)
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
            WindowEvent::CursorMoved{position,..}=>{if let Some(d)=&mut self.menu_drawer.window{d.p.pos=(position.x as f32,position.y as f32);d.window.request_redraw();}},
            WindowEvent::CursorLeft{..}=>{if let Some(d)=&mut self.menu_drawer.window{d.p.pos=(-1.0,-1.0);d.window.request_redraw();}},
            WindowEvent::MouseInput{button:MouseButton::Left,state:ElementState::Released,..}=>{let hit=self.menu_drawer.window.as_ref().and_then(|d|d.p.hits.iter().rev().find(|(r,_,_,content)|r.contains(d.p.pos.0,d.p.pos.1)&&(!content||d.p.viewport.contains(d.p.pos.0,d.p.pos.1))).map(|(_,h,_,_)|*h));if let Some(hit)=hit{self.menu_drawer_action(hit);}},
            WindowEvent::MouseWheel{delta,..}=>{if let Some(d)=&mut self.menu_drawer.window{let dy=match delta{MouseScrollDelta::LineDelta(_,y)=>y*34.0*d.window.scale_factor()as f32,MouseScrollDelta::PixelDelta(p)=>p.y as f32};d.p.scroll=(d.p.scroll-dy).clamp(0.0,d.p.reach);d.window.request_redraw();}},
            WindowEvent::ModifiersChanged(m)=>{if let Some(d)=&mut self.menu_drawer.window{d.shift=m.state().shift_key();}},
            WindowEvent::KeyboardInput{event,..} if event.state==ElementState::Pressed=>{
                if event.logical_key==Key::Named(NamedKey::Escape){self.hide_menu_drawer(true);return;}
                if matches!(event.logical_key,Key::Named(NamedKey::Enter|NamedKey::Space)){if let Some(hit)=self.menu_drawer.window.as_ref().and_then(|d|d.p.focus.and_then(|i|d.p.hits.get(i))).map(|(_,h,_,_)|*h){self.menu_drawer_action(hit);}return;}
                if let Some(d)=&mut self.menu_drawer.window{
                    match event.logical_key{
                        Key::Named(NamedKey::Tab|NamedKey::ArrowDown|NamedKey::ArrowUp)=>{if !d.p.hits.is_empty(){let prev=event.logical_key==Key::Named(NamedKey::ArrowUp)||(event.logical_key==Key::Named(NamedKey::Tab)&&d.shift);let i=match d.p.focus{Some(i) if prev=>(i+d.p.hits.len()-1)%d.p.hits.len(),Some(i)=>(i+1)%d.p.hits.len(),None if prev=>d.p.hits.len()-1,None=>0};let (r,_,_,content)=&d.p.hits[i];d.p.focus=Some(i);if *content{if r.y<d.p.viewport.y{d.p.scroll-=(d.p.viewport.y-r.y).min(d.p.scroll);}else if r.bottom()>d.p.viewport.bottom(){d.p.scroll=(d.p.scroll+r.bottom()-d.p.viewport.bottom()).min(d.p.reach);}}}},
                        Key::Named(NamedKey::PageDown)=>d.p.scroll=(d.p.scroll+d.p.viewport.h*0.8).min(d.p.reach),Key::Named(NamedKey::PageUp)=>d.p.scroll=(d.p.scroll-d.p.viewport.h*0.8).max(0.0),Key::Named(NamedKey::Home)=>d.p.scroll=0.0,Key::Named(NamedKey::End)=>d.p.scroll=d.p.reach,_=>{}
                    }d.window.request_redraw();
                }
            },_=>{}
        }
    }
    pub(crate) fn menu_drawer_access_tree(&self)->accesskit::TreeUpdate{
        use accesskit::{Node,NodeId,Role,TreeInfo,TreeId,TreeUpdate,Action};let mut children=Vec::new();let mut nodes=Vec::new();let mut focus=NodeId(1);
        if let Some(d)=&self.menu_drawer.window{for (n,(r,_,label,content)) in d.p.hits.iter().enumerate(){if *content&&(r.bottom()<=d.p.viewport.y||r.y>=d.p.viewport.bottom()){continue;}let id=NodeId(n as u64+10);let mut node=Node::new(Role::Button);node.set_label(label.clone());node.add_action(Action::Click);let (top,bottom)=if *content{(r.y.max(d.p.viewport.y),r.bottom().min(d.p.viewport.bottom()))}else{(r.y,r.bottom())};node.set_bounds(accesskit::Rect{x0:r.x as f64,y0:top as f64,x1:r.right()as f64,y1:bottom as f64});if d.p.focus==Some(n){focus=id;}nodes.push((id,node));children.push(id);}}
        let mut root=Node::new(Role::Window);root.set_label("nus menu drawer");root.set_children(children);nodes.push((NodeId(1),root));TreeUpdate{nodes,tree:Some(TreeInfo::new(NodeId(1))),tree_id:TreeId::ROOT,focus}
    }
    pub(crate) fn menu_drawer_access_action(&mut self,request:accesskit::ActionRequest){if request.action==accesskit::Action::Click{let hit=self.menu_drawer.window.as_ref().and_then(|d|d.p.hits.get(request.target_node.0.saturating_sub(10)as usize)).map(|(_,h,_,_)|*h);if let Some(hit)=hit{self.menu_drawer_action(hit);}}}
}
