//! One native replacement for `space`: recovered Earth/Limb and Darkroom.
//! Resource decoding is lazy, shared per App, and never asks CEF or the network.
use std::sync::Arc;
use std::time::{Duration, Instant};
use nus_render::{Gpu, Rect, Scene, Style};
use nus_render::space::{SpaceImage, SpaceParams, SpaceRenderer, SpaceResources, SpaceStats};
pub use nus_render::space_motion::{Action, Motion};
use crate::app::{App, Pane};

const MAX_VIEWS: usize = 4;
const ATTRIBUTION: &str = include_str!("../assets/art/space/NOTICE.md");
pub(crate) const MAPS: [(&str, &[u8], (u32,u32));4] = [
    ("Earth",include_bytes!("../assets/art/space/nus_earth_texture.jpg"),(2048,1024)),
    ("Earth detail",include_bytes!("../assets/art/space/nus_earth_detail.jpg"),(2048,1024)),
    ("Clouds",include_bytes!("../assets/art/space/nus_earth_clouds.jpg"),(512,256)),
    ("Cloud detail",include_bytes!("../assets/art/space/nus_earth_cloud_detail.jpg"),(1024,512)),
];
fn images()->Result<[SpaceImage;4],String> {
    let mut images=Vec::with_capacity(4);
    for (name,bytes,expected) in MAPS {
        let image=image::load_from_memory_with_format(bytes,image::ImageFormat::Jpeg)
            .map_err(|e|format!("Could not decode Space {name}: {e}"))?.into_rgba8();
        if image.dimensions()!=expected {return Err(format!("Space {name} dimensions do not match the packaged asset"));}
        images.push(SpaceImage{width:image.width(),height:image.height(),rgba:image.into_raw()});
    }
    images.try_into().map_err(|_|"Space image set is incomplete".into())
}
struct View { key:(u64,usize), used:Instant, renderer:SpaceRenderer }
#[derive(Default)]
pub(crate) struct Spaces { resources:Option<Arc<SpaceResources>>, error:Option<String>, views:Vec<View> }
impl Spaces {
    fn draw(&mut self,gpu:&Gpu,key:(u64,usize),size:(u32,u32),p:SpaceParams)->Result<(Arc<wgpu::BindGroup>,SpaceStats),String> {
        if let Some(error)=&self.error {return Err(error.clone());}
        if self.resources.is_none() {
            match images().and_then(|images|SpaceResources::new(gpu,images).map_err(|e|e.to_string())) {
                Ok(resources)=>self.resources=Some(resources),
                Err(error)=>{self.error=Some(error.clone());return Err(error);}
            }
        }
        let i=match self.views.iter().position(|v|v.key==key) {Some(i)=>i,None=>{
            if self.views.len()>=MAX_VIEWS {let i=self.views.iter().enumerate().min_by_key(|(_,v)|v.used).unwrap().0;self.views.swap_remove(i);}
            self.views.push(View{key,used:crate::clock::now(),renderer:SpaceRenderer::new(self.resources.as_ref().unwrap().clone())});self.views.len()-1
        }};
        let v=&mut self.views[i];v.used=crate::clock::now();let bind=v.renderer.render(size,p);Ok((bind,v.renderer.stats()))
    }
    pub(crate) fn touch(&mut self,id:u64) {for v in &mut self.views {if v.key.0==id {v.used=crate::clock::now();}}}
    pub(crate) fn trim(&mut self) {
        let now=crate::clock::now();self.views.retain(|v|now.saturating_duration_since(v.used)<Duration::from_secs(60));
        if self.views.is_empty() {self.resources=None;self.views.shrink_to_fit();}
    }
    pub(crate) fn stats(&self,id:u64)->Option<SpaceStats> {self.views.iter().find(|v|v.key.0==id).map(|v|v.renderer.stats())}
}
impl App {
    pub(crate) fn space_controls_available(&self)->bool {
        self.behavior.home_look==crate::settings::HomeLook::Art && self.behavior.home_art=="space"
            && self.palette.is_none() && self.start.is_none() && !self.me_card.open
            && self.splash.is_none() && !self.board.open && !self.scm.open && self.timeline.is_none()
            && self.page_menu.is_none() && !self.dl_menu && !self.tidy.open && !self.pane_mode
    }
    /// Also called when a Home is no longer drawn: do not accrue hidden time.
    pub(crate) fn tend_space_visibility(&mut self) {
        let now=crate::clock::since(self.started).as_secs_f64();
        let live=self.space_controls_available() && self.art_budget()!=crate::power::Budget::Still
            && !self.hatch_state.main_hidden && self.window.is_minimized()!=Some(true);
        let reduced=self.motion.reduced();let composing=self.prompt_composing;let frame=self.frames;
        for tab in &mut self.tabs {for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            if let Pane::Home(h)=p {
                let drawn=h.space_drawn.is_some_and(|at|at==frame || at.saturating_add(1)==frame);
                let eligible=live && drawn && !h.library;
                h.space_motion.environment(now,eligible,composing,reduced);
                if eligible {self.spaces.touch(h.art_id);}
            }
        }}
    }
    pub(crate) fn draw_space_cmd(&mut self,scene:&mut Scene,r:Rect,p:SpaceParams,key:(u64,usize)) {
        if r.w<=0.0 || r.h<=0.0 || ![r.x,r.y,r.w,r.h].iter().all(|x|x.is_finite()) {return;}
        let clip=scene.clip();
        if clip.is_some_and(|c| {let i=r.intersect(&c);i.w<=0.0||i.h<=0.0}) {return;}
        let size=(r.w.ceil().max(1.0) as u32,r.h.ceil().max(1.0) as u32);
        match self.spaces.draw(&self.gpu,key,size,p) {
            Ok((bind,stats))=>{
                scene.texture(r,bind,clip);scene.layer(clip);
                let (w,h)=(stats.output_size.0 as f32,stats.output_size.1 as f32);
                let scale=r.h/h;
                let style=Style{font:self.f.ui,px:(h/800.0).clamp(0.72,0.85)*20.0*scale,color:[0.68,0.75,0.82,1.0],tracking:0.0};
                let reading=Rect::new(r.x+p.reading_rect[0]*r.w,r.y+p.reading_rect[1]*r.h,p.reading_rect[2]*r.w,p.reading_rect[3]*r.h);
                let mut accepted=0;
                for (name,at,alpha) in nus_render::space::label_candidates(stats.output_size,p) {
                    let (x,y)=(r.x+at[0]/w*r.w,r.y+at[1]/h*r.h);
                    let bounds=Rect::new(x,y,self.fonts.measure(style,name),style.px*1.5);
                    let overlap=bounds.intersect(&reading.inset(-self.px(8.0)));
                    if bounds.right()>r.right()-r.w*0.03 || bounds.bottom()>r.bottom()-r.h*0.12
                        || (reading.w>0.0 && reading.h>0.0 && overlap.w>0.0 && overlap.h>0.0) {continue;}
                    // A label is omitted if any corner is occluded by Earth.
                    if [[bounds.x,bounds.y],[bounds.right(),bounds.y],[bounds.x,bounds.bottom()],[bounds.right(),bounds.bottom()]].iter().any(|q|
                        nus_render::space::visibility([(q[0]-r.x)/r.w*w,(q[1]-r.y)/r.h*h],stats.output_size,p)<0.1) {continue;}
                    self.fonts.draw(scene,Style{color:[0.68,0.75,0.82,alpha],..style},x,y+style.px,name);
                    accepted+=1;if accepted==3 {break;}
                }
            }
            Err(error)=>{
                scene.rect(r,[0.012,0.02,0.03,1.0]);
                let label=Style{color:self.surface.signal,..self.label()};
                let text=self.fit(label,format!("SPACE · {error}"),(r.w-self.px(32.0)).max(1.0));
                self.fonts.draw(scene,label,r.x+self.px(16.0),r.bottom()-self.px(18.0),&text);
            }
        }
    }
    pub(crate) fn space_focus_control(&mut self,id:u64,action:Option<Action>)->bool {
        if !self.space_controls_available() {return false;}
        let Some(tab)=self.tabs.get_mut(self.active) else{return false;};
        let right=match (&tab.left,tab.right.as_ref()) {
            (Pane::Home(h),_) if h.art_id==id=>false,
            (_,Some(Pane::Home(h))) if h.art_id==id=>true,
            _=>return false,
        };
        tab.focus_right=right;
        let Pane::Home(h)=tab.focused() else{return false;};
        if h.library||h.space_hits.is_empty() || !h.space_drawn.is_some_and(|at|at==self.frames || at.saturating_add(1)==self.frames) {return false;}
        h.space_focus=action.map(|a|usize::from(a==Action::Hold));self.dirty=true;true
    }
    pub(crate) fn space_action(&mut self,id:u64,action:Action)->bool {
        if !self.space_focus_control(id,Some(action)) {return false;}
        let now=crate::clock::since(self.started).as_secs_f64();
        let Some(Pane::Home(h))=self.tabs.get_mut(self.active).map(|t|t.focused()) else{return false;};
        h.space_motion.act(action,now);self.dirty=true;true
    }
    /// F6 enters the two art controls; Tab/Shift-Tab leave them for the prompt.
    pub(crate) fn space_key(&mut self,ev:&crate::app::KeyIn)->bool {
        use winit::keyboard::{Key,NamedKey};
        if !self.space_controls_available() || ev.state!=winit::event::ElementState::Pressed {return false;}
        if self.mods.control_key()||self.mods.super_key()||self.mods.alt_key() {return false;}
        let now=crate::clock::since(self.started).as_secs_f64();let shift=self.mods.shift_key();
        let Some(Pane::Home(h))=self.tabs.get_mut(self.active).map(|t|t.focused()) else{return false;};
        if h.library || h.space_hits.is_empty() {return false;}
        if ev.logical_key==Key::Named(NamedKey::F6) {h.space_focus=if h.space_focus.is_some(){None}else{Some(0)};self.dirty=true;return true;}
        let Some(i)=h.space_focus else{return false;};
        match ev.logical_key {
            Key::Named(NamedKey::Tab)=>h.space_focus=match(i,shift){(0,false)=>Some(1),(1,true)=>Some(0),_=>None},
            Key::Named(NamedKey::ArrowLeft)=>h.space_focus=Some(0),
            Key::Named(NamedKey::ArrowRight)=>h.space_focus=Some(1),
            Key::Named(NamedKey::Enter|NamedKey::Space)=>h.space_motion.act(if i==0{Action::Turn}else{Action::Hold},now),
            Key::Named(NamedKey::Escape)=>h.space_focus=None,
            _=>{h.space_focus=None;return false;}
        }self.dirty=true;true
    }
    pub(crate) fn space_footer_height(&self,r:Rect)->f32 {self.px(if r.w<self.px(650.0){116.0}else{88.0}).min(r.h*0.45)}
    pub(crate) fn draw_space_controls(&mut self,scene:&mut Scene,h:&mut crate::home::HomePane,ink:nus_render::Color) {
        if h.space_drawn!=Some(self.frames) {return;}
        let r=h.rect;let pad=self.px(18.0).min(r.w*0.06);let available=(r.w-2.0*pad).max(0.0);
        if available<self.px(90.0) || r.h<self.px(90.0) {return;}
        let route=if self.behavior.prompt.hints {self.px(40.0)}else{0.0};
        let y=r.bottom()-route-self.px(44.0);let narrow=r.w<self.px(650.0);
        let control_w=if narrow {available}else{self.px(290.0).min(available)};
        let bx=r.right()-pad-control_w;let gap=self.px(14.0);let bw=(control_w-gap)/2.0;
        let style=Style{font:self.f.ui,px:self.px(11.0),color:ink,tracking:0.0};
        let old=scene.clip();let clip=old.map_or(r,|c|r.intersect(&c));scene.layer(Some(clip));
        let lx=r.x+pad;let title_y=if narrow {y-self.px(27.0)}else{y+self.px(11.0)};
        let name=Style{px:self.px(10.0),tracking:self.px(1.4),..style};
        let label_w=if narrow {available}else{(bx-lx-gap).max(0.0)};
        if label_w>self.px(95.0) && title_y>r.y+self.px(15.0) {
            let text=self.fit(name,"LIMB / DARKROOM",label_w);
            self.fonts.draw(scene,name,lx,title_y,&text);
            let caption=h.space_motion.caption(crate::clock::since(self.started).as_secs_f64());
            let text=self.fit(style,caption,label_w);
            self.fonts.draw(scene,Style{color:crate::app::fade(ink,0.80),..style},lx,title_y+self.px(17.0),&text);
        }
        for (i,action) in [Action::Turn,Action::Hold].into_iter().enumerate() {
            let rect=Rect::new(bx+i as f32*(bw+gap),y,bw,self.px(32.0));let hit=rect.intersect(&clip);
            if hit.w<=0.0||hit.h<=0.0 {continue;}
            if rect.contains(self.mouse.0,self.mouse.1)||h.space_focus==Some(i) {scene.outline(rect,self.px(1.0),crate::app::fade(ink,0.55));}
            let text=self.fit(style,h.space_motion.action_label(action),(bw-self.px(10.0)).max(1.0));
            self.fonts.draw(scene,style,rect.x+self.px(5.0),rect.y+self.px(21.0),&text);
            h.space_hits.push((hit,action));
        }
        scene.layer(old);
        // Keep the actual source attribution in the shipped application, not
        // only in the repository. It is available over the quiet identity line.
        self.offer_tip(h.art_id ^ 0x5350414345000000,Rect::new(lx,title_y-self.px(12.0),label_w,self.px(32.0)),ATTRIBUTION.into());
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]fn recovered_images_decode_at_the_expected_size(){let set=images().unwrap();assert_eq!(set.len(),4);for(image,(_,_,size))in set.iter().zip(MAPS){assert_eq!((image.width,image.height),size);assert_eq!(image.rgba.len(),(image.width*image.height*4)as usize);}}
}

#[cfg(test)]
mod native_gpu_tests {
    use super::*;
    use std::future::Future;
    use std::task::{Context, Poll, Wake, Waker};

    // Test-only executor: callbacks unpark this test thread. It never runs on
    // the native UI thread and carries a bounded request-device deadline.
    fn wait<F: Future>(future: F) -> F::Output {
        struct Thread(std::thread::Thread);
        impl Wake for Thread {fn wake(self: Arc<Self>) {self.0.unpark();}}
        let waker:Waker=Arc::new(Thread(std::thread::current())).into();
        let mut cx=Context::from_waker(&waker);let mut future=std::pin::pin!(future);
        let until=Instant::now()+Duration::from_secs(30);
        loop {if let Poll::Ready(value)=future.as_mut().poll(&mut cx) {return value;}
            assert!(Instant::now()<until,"native Space device request timed out");
            std::thread::park_timeout(Duration::from_millis(10));}
    }
    fn pixels(device:&wgpu::Device,queue:&wgpu::Queue,renderer:&SpaceRenderer)->Vec<u8> {
        let size=renderer.stats().output_size;let stride=(size.0*4).div_ceil(256)*256;
        let buffer=device.create_buffer(&wgpu::BufferDescriptor {label:Some("Space QA readback"),size:u64::from(stride)*u64::from(size.1),
            usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        let mut encoder=device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo {texture:renderer.output_texture().unwrap(),mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
            wgpu::TexelCopyBufferInfo {buffer:&buffer,layout:wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(stride),rows_per_image:Some(size.1)}},
            wgpu::Extent3d{width:size.0,height:size.1,depth_or_array_layers:1});
        queue.submit([encoder.finish()]);let (tx,rx)=std::sync::mpsc::channel();
        let slice=buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read,move|r|{let _=tx.send(r);});
        // Poll nonblocking so a lost device cannot trap this harness forever.
        let until=Instant::now()+Duration::from_secs(30);
        loop {device.poll(wgpu::PollType::Poll).unwrap();
            match rx.try_recv() {Ok(result)=>{result.unwrap();break;},Err(std::sync::mpsc::TryRecvError::Disconnected)=>panic!("Space readback callback disappeared"),_=>{}}
            assert!(Instant::now()<until,"Space GPU readback timed out");std::thread::sleep(Duration::from_millis(2));}
        let data=slice.get_mapped_range().unwrap();let mut result=Vec::new();
        for row in data.chunks(stride as usize).take(size.1 as usize) {result.extend_from_slice(&row[..size.0 as usize*4]);}
        drop(data);buffer.unmap();result
    }
    fn capture(name:&str,rgba:&[u8],size:(u32,u32)) {
        if let Some(dir)=std::env::var_os("NUS_SPACE_CAPTURE_DIR") {
            let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();
            image::save_buffer_with_format(dir.join(format!("{name}.png")),rgba,size.0,size.1,image::ColorType::Rgba8,image::ImageFormat::Png).unwrap();
        }
    }
    #[test]
    #[ignore = "requires a native GPU; set NUS_SPACE_CAPTURE_DIR for review images"]
    fn native_space_gpu_real_assets_and_cache() {
        let backends=if cfg!(target_os="macos"){wgpu::Backends::METAL}else if cfg!(windows){wgpu::Backends::DX12}else{wgpu::Backends::VULKAN};
        let instance=wgpu::Instance::new(wgpu::InstanceDescriptor{backends,..wgpu::InstanceDescriptor::new_without_display_handle()});
        let adapter=wait(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("native Space adapter");
        let (device,queue)=wait(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let resources=SpaceResources::new_offscreen(&device,&queue,images().unwrap()).unwrap();
        let mut renderer=SpaceRenderer::new(resources.clone());let size=(1100,570);let p=SpaceParams::default();
        renderer.render(size,p);let earth=pixels(&device,&queue,&renderer);
        assert!(earth.chunks_exact(4).all(|p|p[3]==255));
        let colors:std::collections::HashSet<_>=earth.chunks_exact(4).map(|p|[p[0],p[1],p[2]]).collect();
        assert!(colors.len()>256,"Earth is blank/solid instead of the textured recovered scene");
        capture("space-earth",&earth,size);
        renderer.render(size,p);assert_eq!(renderer.stats().scene_draws,1);assert_eq!(renderer.stats().reuses,1);
        renderer.render(size,SpaceParams{blend:1.0,..p});let dark=pixels(&device,&queue,&renderer);capture("space-darkroom",&dark,size);
        assert_ne!(earth,dark);assert!(dark.chunks_exact(4).all(|p|p[3]==255));assert_eq!(renderer.stats().dust_draws,1);
        renderer.render(size,SpaceParams{blend:0.5,..p});let middle=pixels(&device,&queue,&renderer);capture("space-mid-turn",&middle,size);
        assert_ne!(middle,earth);assert_ne!(middle,dark);assert_eq!(renderer.stats().dust_draws,1);
        let narrow=(540,780);renderer.render(narrow,p);let image=pixels(&device,&queue,&renderer);capture("space-earth-narrow",&image,narrow);
        assert_eq!(renderer.stats().dust_draws,2);assert_eq!(renderer.stats().output_size,narrow);
        let mut other=SpaceRenderer::new(resources);other.render((300,180),p);pixels(&device,&queue,&other);
        assert_eq!(other.stats().scene_draws,1);assert_eq!(renderer.stats().scene_draws,4);
        eprintln!("SPACE GPU PASS {:?}: {:?}",adapter.get_info(),renderer.stats());
    }
}
