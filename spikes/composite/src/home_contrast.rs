//! Home's text contract is against the final reading surface, not theme names.
//! All tokens are opaque. Artwork receives the same bounds used to resolve them.
use nus_render::{Color, Rect};
use nus_render::policy::{contrast, luminance};
use crate::art::Backdrop;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub selection: Color,
    pub selected: Color,
    pub surface: Color,
    pub bounds: [f32; 2],
    pub veil: Color,
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    [a[0]+(b[0]-a[0])*t, a[1]+(b[1]-a[1])*t, a[2]+(b[2]-a[2])*t, 1.0]
}

pub(crate) fn opaque(c: Color, bg: Color) -> Color { mix(bg, c, c[3].clamp(0.0,1.0)) }

fn encode(v: f32) -> f32 {
    if v <= 0.0031308 {v*12.92} else {1.055*v.powf(1.0/2.4)-0.055}
}

fn neutral(v: f32) -> Color { let c=encode(v); [c,c,c,1.0] }

/// Worst contrast anywhere in a luminance interval; an average is insufficient.
fn against(c: Color, bounds: [f32;2]) -> f32 {
    let l=luminance(c);
    if l<bounds[0] {(bounds[0]+0.05)/(l+0.05)}
    else if l>bounds[1] {(l+0.05)/(bounds[1]+0.05)} else {1.0}
}

fn resolve(c: Color, surface: Color, bounds: [f32;2], minimum: f32) -> Color {
    let c=opaque(c,surface);
    if against(c,bounds)>=minimum {return c;}
    let black=[0.0,0.0,0.0,1.0]; let white=[1.0;4];
    let end=if against(black,bounds)>against(white,bounds) {black} else {white};
    let(mut lo,mut hi)=(0.0,1.0);
    for _ in 0..18 {let m=(lo+hi)*0.5;if against(mix(c,end,m),bounds)>=minimum {hi=m;}else{lo=m;}}
    mix(c,end,hi)
}

impl Palette {
    pub fn new(backdrop: Backdrop, paper: Color, ink: Color, dim: Color, signal: Color) -> Self {
        let paper=opaque(paper,[0.0,0.0,0.0,1.0]);
        let (surface,bounds,veil)=match backdrop {
            Backdrop::Dark => {
                let cap=0.07;
                (neutral(cap),[0.0,cap],[0.0,0.0,0.0,1.0-encode(cap)])
            }
            Backdrop::Light => {
                let floor=0.42;
                (neutral(floor),[floor,1.0],[1.0,1.0,1.0,encode(floor)])
            }
            Backdrop::Theme => {
                // Rare mid-luminance custom papers cannot support 7:1 with
                // either black or white. Move just the local paper enough.
                let black=[0.0,0.0,0.0,1.0]; let white=[1.0;4];
                let fg=if contrast(black,paper)>contrast(white,paper) {black} else {white};
                let end=if fg==black {white} else {black};
                let mut surface=paper;
                if contrast(fg,paper)<7.25 {
                    let(mut lo,mut hi)=(0.0,1.0);
                    for _ in 0..18 {let m=(lo+hi)*0.5;if contrast(fg,mix(paper,end,m))>=7.25 {hi=m;}else{lo=m;}}
                    surface=mix(paper,end,hi);
                }
                let l=luminance(surface); (surface,[l,l],surface)
            }
        };
        let primary=resolve(ink,surface,bounds,7.1);
        let secondary=resolve(dim,surface,bounds,4.6);
        let accent=resolve(signal,surface,bounds,4.6);
        // Reversed, opaque selection keeps both its edge and text legible.
        let selected=resolve(surface,primary,[luminance(primary);2],7.1);
        Self {primary,secondary,accent,selection:primary,selected,surface,bounds,veil}
    }

    pub fn caret(&self, candidate: Color, selected: bool) -> Color {
        if selected {resolve(candidate,self.selection,[luminance(self.selection);2],3.1)}
        else {resolve(candidate,self.surface,self.bounds,3.1)}
    }
}

/// Layout once, then use these same rectangles for protection and hit testing.
pub(crate) fn visible_rows(heights: &[f32], selected: usize, start: usize, x: f32, mut y: f32, width: f32, bottom: f32, news: usize, gap: f32) -> (usize, Vec<(Rect,usize)>) {
    let selected=selected.saturating_sub(1).min(heights.len().saturating_sub(1));
    let mut start=start.min(selected);
    let reach=|start:usize| heights[start..=selected].iter().sum::<f32>() + if news>0 && start<=news && selected>=news {gap} else {0.0};
    if !heights.is_empty() {while start<selected && y+reach(start)>bottom {start+=1;}}
    let mut rows=Vec::new();
    for (k,h) in heights.iter().copied().enumerate().skip(start) {
        let space=if news>0 && k==news {gap} else {0.0};
        if y+space+h>bottom {break;}
        y+=space;rows.push((Rect::new(x,y,width,h),k));y+=h;
    }
    (start,rows)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn text_and_selection_survive_extreme_and_saturated_palettes() {
        let colors=[[0.0,0.0,0.0,1.0],[1.0;4],[0.1,0.3,1.0,1.0],[1.0,0.1,0.5,0.3],[0.48,0.48,0.48,1.0]];
        for backdrop in [Backdrop::Theme,Backdrop::Light,Backdrop::Dark] {for paper in colors {for ink in colors {
            let p=Palette::new(backdrop,paper,ink,[0.5,0.2,0.8,0.67],[1.0,0.1,0.25,0.5]);
            assert!(against(p.primary,p.bounds)>=7.0);
            assert!(against(p.secondary,p.bounds)>=4.5);
            assert!(against(p.accent,p.bounds)>=4.5);
            assert!(contrast(p.selected,p.selection)>=7.0);
            assert!(against(p.caret(ink,false),p.bounds)>=3.0);
            assert!(contrast(p.caret(ink,true),p.selection)>=3.0);
            assert_eq!(p.primary[3],1.0); assert_eq!(p.secondary[3],1.0);
            // The renderer blends ordinary colors in encoded sRGB. Check
            // the fallback over saturated artwork as well as black/white.
            for background in colors {let bg=opaque(p.veil,background);assert!(against(p.primary,[luminance(bg);2])>=7.0);}
        }}}
    }
    #[test] fn new_results_and_news_gap_are_in_this_frames_geometry() {
        let (_,empty)=visible_rows(&[],0,0,10.0,50.0,400.0,240.0,0,8.0); assert!(empty.is_empty());
        let (_,rows)=visible_rows(&[34.0,54.0,34.0],2,0,10.0,50.0,400.0,240.0,1,8.0);
        assert_eq!(rows.last().unwrap().0.bottom(),180.0);
        let (start,rows)=visible_rows(&[54.0;8],8,0,10.0,50.0,400.0,180.0,0,8.0);
        assert_eq!(start,6);assert_eq!(rows.last().unwrap().1,7);assert!(rows.iter().all(|(r,_)|r.bottom()<=180.0));
    }
}
