//! Persisted desktop icon selection; Mercury ownership is independent of wearing it.
use nus_render::{dock_icon::{self,Face},Color,Rect,Scene,Style};
use std::sync::{Arc,atomic::{AtomicU8,Ordering}};
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,serde::Serialize,serde::Deserialize)]
pub enum Choice {#[default] Automatic,Newsreader,Plex,Silkscreen,PlexItalic,Bungee,Rubik,Mercury}
impl Choice {
    pub const ALL:[Self;8]=[Self::Automatic,Self::Newsreader,Self::Plex,Self::Silkscreen,Self::PlexItalic,Self::Bungee,Self::Rubik,Self::Mercury];
    pub fn name(self)->&'static str {match self{Self::Automatic=>"Automatic",Self::Newsreader=>"Newsreader",Self::Plex=>"Plex",Self::Silkscreen=>"Silkscreen",Self::PlexItalic=>"Plex Italic",Self::Bungee=>"Bungee",Self::Rubik=>"Rubik Mono",Self::Mercury=>"Mercury"}}
    pub fn face(self)->Face {match self{Self::Plex=>Face::Plex,Self::Silkscreen=>Face::Silkscreen,Self::PlexItalic=>Face::PlexItalic,Self::Bungee=>Face::Bungee,Self::Rubik=>Face::Rubik,_=>Face::Newsreader}}
    pub fn mercury(self)->bool {matches!(self,Self::Automatic|Self::Mercury)&&crate::mercury::earned()}
    fn render(self,size:u32,signal:Color)->Vec<u8>{if self.mercury(){crate::mercury::icon(size)}else{dock_icon::render(size,signal,self.face())}}
}
/// The tiles Settings offers: Mercury only while it is yours or can be claimed.
pub fn offered()->Vec<Choice>{let m=crate::mercury::earned()||crate::mercury::can_claim();Choice::ALL.into_iter().filter(|c|*c!=Choice::Mercury||m).collect()}
static SELECTED:AtomicU8=AtomicU8::new(0);
pub fn select(choice:Choice){SELECTED.store(choice as u8,Ordering::Relaxed);}
pub fn selected()->Choice {Choice::ALL[usize::from(SELECTED.load(Ordering::Relaxed)).min(7)]}
pub fn mercury()->bool {selected().mercury()}
pub fn face()->Face {selected().face()}
pub fn render(size:u32,signal:Color)->Vec<u8>{selected().render(size,signal)}
pub struct Preview {choice:Choice,signal:Color,earned:bool,bind:Arc<wgpu::BindGroup>}
impl crate::app::App {
    pub(crate) fn icon_choices_height(&self,width:f32)->f32 {let cols=((width/self.px(130.0)).floor()as usize).clamp(1,4);self.px(134.0)*offered().len().div_ceil(cols)as f32}
    pub(crate) fn draw_icon_choices(&mut self,scene:&mut Scene,r:Rect){
        let signal=self.surface.signal;let earned=crate::mercury::earned();
        self.icon_previews.retain(|p|p.signal==signal&&p.earned==earned);
        let cols=((r.w/self.px(130.0)).floor()as usize).clamp(1,4);let cell=r.w/cols as f32;
        let mercury_here=offered().contains(&Choice::Mercury);
        for (i,choice) in offered().into_iter().enumerate(){
            let tile=Rect::new(r.x+(i%cols)as f32*cell,r.y+(i/cols)as f32*self.px(134.0),cell-self.px(8.0),self.px(126.0));
            let selected=self.behavior.app_icon==choice;let locked=choice==Choice::Mercury&&!earned;
            let color=if selected{signal}else{self.theme.dim};
            scene.outline(tile,self.px(if selected{2.0}else{1.0}),crate::app::fade(color,if selected{1.0}else{0.35}));
            let bind=if let Some(p)=self.icon_previews.iter().find(|p|p.choice==choice){p.bind.clone()}else{
                let size=128;let rgba=if choice==Choice::Mercury{crate::mercury::icon(size)}else{choice.render(size,signal)};
                let bgra:Vec<u8>=rgba.as_chunks::<4>().0.iter().flat_map(|p|[p[2],p[1],p[0],p[3]]).collect();
                let tex=self.device.create_texture(&wgpu::TextureDescriptor{label:Some("app icon choice"),size:wgpu::Extent3d{width:size,height:size,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Bgra8Unorm,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
                self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo{texture:&tex,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},&bgra,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(size*4),rows_per_image:Some(size)},wgpu::Extent3d{width:size,height:size,depth_or_array_layers:1});
                let bind=(self.bind_texture)(&tex);self.icon_previews.push(Preview{choice,signal,earned,bind:bind.clone()});bind
            };
            let size=self.px(72.0);scene.texture(Rect::new(tile.x+(tile.w-size)/2.0,tile.y+self.px(8.0),size,size),bind,Some(r));scene.layer(Some(r));
            let style=Style{color:self.theme.ink,..self.label()};let title=choice.name();let tw=self.fonts.measure(style,title);
            self.fonts.draw(scene,style,tile.x+(tile.w-tw)/2.0,tile.y+self.px(98.0),title);
            let detail=if locked{"Claim · free until 2027"}else if selected{"Selected"}else if choice==Choice::Automatic&&mercury_here{"Mercury once claimed"}else if choice==Choice::Automatic{"The default"}else{"Choose"};
            let style=Style{px:self.px(9.0),color,..self.label()};let tw=self.fonts.measure(style,detail);self.fonts.draw(scene,style,tile.x+(tile.w-tw)/2.0,tile.y+self.px(115.0),detail);
            // Mercury's tile claims it, then wears it.
            self.settings_hits.push((tile,crate::settings::Hit::AppIcon(choice)));
        }
    }
}
#[cfg(test)]mod tests{use super::*;#[test]fn icon_choices_roundtrip_and_remain_distinct(){for choice in Choice::ALL{assert_eq!(serde_json::from_str::<Choice>(&serde_json::to_string(&choice).unwrap()).unwrap(),choice);}let a=Choice::Plex.render(32,[1.,0.,0.,1.]);assert_ne!(a,Choice::Newsreader.render(32,[1.,0.,0.,1.]));assert!(!Choice::Plex.mercury());}}
