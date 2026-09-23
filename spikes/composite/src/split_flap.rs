//! Station: one physical hinge per character. Each row cascades independently, one flap following the previous flap.
use nus_render::{Color, FontSystem, Rect, Scene, Style};

pub const TURN: f32 = 0.31;
pub const STAGGER:f32=0.023;

pub fn duration(cells:usize)->f32{TURN+cells.saturating_sub(1) as f32*STAGGER}

#[derive(Clone, Copy, PartialEq)]
pub enum Face { Letter(char), Icon((&'static str, &'static str)) }

fn face(fonts: &mut FontSystem, scene: &mut Scene, style: Style, r: Rect, content: Face) {
    match content {
        Face::Letter(ch) => {
            let text = ch.to_string();
            let width = fonts.measure(style, &text);
            fonts.draw(scene, style, r.x + (r.w-width)*0.5, r.y+(r.h+style.px)*0.5-style.px*0.16, &text);
        }
        Face::Icon(icon) => {
            let side = style.px.min(r.w*0.75);
            fonts.draw_icon(scene, icon, side, r.x+(r.w-side)*0.5, r.y+(r.h-side)*0.5, style.color);
        }
    }
}

/// Project clipped glyph/paint strips about the centre hinge, preserving UVs.
fn leaf(scene: &mut Scene, source: &Scene, r: Rect, upper: bool, angle: f32) {
    let cy = r.y+r.h*0.5;
    let bounds = if upper {Rect::new(r.x,r.y,r.w,r.h*0.5)} else {Rect::new(r.x,cy,r.w,r.h*0.5)};
    let (sin,cos) = angle.sin_cos();
    if cos < 0.001 { return; }
    let project = |y:f32| {let dy=y-cy;let k=r.h*8.0/(r.h*8.0-dy*sin);(cy+dy*cos*k,k)};
    for source in source.instances() {
        let box_ = Rect::new(source.pos[0],source.pos[1],source.size[0],source.size[1]);
        let visible=box_.intersect(&bounds);
        if visible.h<=0.0 || visible.w<=0.0 {continue;}
        let bands=visible.h.ceil().max(1.0) as usize;
        for band in 0..bands {
            let a=visible.y+visible.h*band as f32/bands as f32;
            let b=visible.y+visible.h*(band+1) as f32/bands as f32;
            let (y0,k0)=project(a);let(y1,k1)=project(b);let k=(k0+k1)*0.5;
            let mut i=*source;
            i.pos=[r.x+r.w*0.5+(visible.x-r.x-r.w*0.5)*k,y0];i.size=[visible.w*k,y1-y0];
            if i.kind==1 {
                let uv=source.uv;
                i.uv=[uv[0]+(uv[2]-uv[0])*(visible.x-box_.x)/box_.w,uv[1]+(uv[3]-uv[1])*(a-box_.y)/box_.h,
                    uv[0]+(uv[2]-uv[0])*(visible.right()-box_.x)/box_.w,uv[1]+(uv[3]-uv[1])*(b-box_.y)/box_.h];
            }
            for c in &mut i.color[..3] {*c*=0.68+0.32*cos;}
            scene.push(i);
        }
    }
}

pub fn cell(fonts:&mut FontSystem, scene:&mut Scene, style:Style, r:Rect, old:Face, new:Face, progress:f32, paper:Color, pin:Color, scale:f32) {
    let clip=scene.clip();scene.layer(Some(clip.map_or(r,|c|c.intersect(&r))));
    let top=crate::surface::mix(paper,style.color,0.08);
    let bottom=crate::surface::mix(paper,style.color,0.13);
    scene.rect(r,bottom);scene.rect(Rect::new(r.x,r.y,r.w,r.h*0.5),top);
    let p=progress.clamp(0.0,1.0);
    if old==new || p<=0.0 || p>=1.0 {
        scene.hline(r.x,r.y+r.h*0.5,r.w,(0.5*scale).max(0.5),paper);
        face(fonts,scene,style,r,if p>=1.0{new}else{old});
    } else {
        let mut from=Scene::new();from.rect(r,top);face(fonts,&mut from,style,r,old);
        let mut to=Scene::new();to.rect(r,bottom);face(fonts,&mut to,style,r,new);
        leaf(scene,&to,r,true,0.0);leaf(scene,&from,r,false,0.0);
        if p<0.4 {
            let t=p/0.4;leaf(scene,&from,r,true,-t*t*std::f32::consts::FRAC_PI_2);
        } else {
            let t=(p-0.4)/0.6;leaf(scene,&to,r,false,(1.0-t).powi(3)*std::f32::consts::FRAC_PI_2);
        }
        scene.hline(r.x,r.y+r.h*0.5,r.w,(0.5*scale).max(0.5),paper);
    }
    let hinge=(scale*0.65).max(0.65);
    for x in [r.x,r.right()-hinge] {scene.rect(Rect::new(x,r.y+r.h*0.5-scale,hinge,scale*2.0),pin);}
    scene.layer(clip);
}

pub fn text(fonts:&mut FontSystem,scene:&mut Scene,style:Style,r:Rect,old:&str,new:&str,cw:f32,elapsed:f32,paper:Color,pin:Color,scale:f32) {
    let before:Vec<_>=old.chars().collect();let after:Vec<_>=new.chars().collect();
    let n=(r.w/cw).floor().max(0.0) as usize;
    for i in 0..n {
        let cell_rect=Rect::new(r.x+i as f32*cw,r.y,(cw-2.0*scale).max(1.0),r.h);
        cell(fonts,scene,style,cell_rect,Face::Letter(*before.get(i).unwrap_or(&' ')),Face::Letter(*after.get(i).unwrap_or(&' ')),(elapsed-i as f32*STAGGER)/TURN,paper,pin,scale);
    }
}

#[derive(Clone)]
pub struct RowChange {pub before:[String;4],pub after:[String;4],pub at:std::time::Instant,pub cells:usize}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn row_duration_includes_following_flaps(){assert_eq!(duration(1),TURN);assert_eq!(duration(10),TURN+9.0*STAGGER);}

}
