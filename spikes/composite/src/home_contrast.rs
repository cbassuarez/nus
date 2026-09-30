//! Home's text tokens are opaque and independent of the reading veil.
//! `new` resolves a protected local surface. `for_art` keeps that text polarity
//! but uses a deliberately faint veil: its bounds are artwork design hints,
//! NOT a per-pixel contrast guarantee over arbitrary imagery or HDR radiance.
use nus_render::{Color, Rect};
use nus_render::policy::{contrast, luminance};
use crate::art::Backdrop;

/// Artwork stays visible beneath the native prompt. These values govern only
/// the background material, never glyphs, selection, caret or error screens.
pub(crate) const ART_READING_ALPHA: f32 = 0.10;
pub(crate) const ART_FOOTER_ALPHA: f32 = 0.03;
pub(crate) const ART_FOOTER_WEIGHT: f32 = ART_FOOTER_ALPHA / ART_READING_ALPHA;

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

    /// Artwork's authored Dark/Light polarity selects text; it does not
    /// authorize hiding the art to manufacture a guaranteed contrast ratio.
    /// Dark art gets a black veil, light art gets white. Theme-following art
    /// keeps its resolved local-paper tint, at the same deliberately low alpha.
    /// Plain Home still uses `new` and keeps its opaque-paper guarantee.
    pub fn for_art(backdrop: Backdrop, paper: Color, ink: Color, dim: Color, signal: Color) -> Self {
        let mut palette = Self::new(backdrop, paper, ink, dim, signal);
        palette.veil[3] = ART_READING_ALPHA;
        palette
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
    #[test] fn art_is_faint_without_changing_text_caret_or_selection() {
        for backdrop in [Backdrop::Theme, Backdrop::Light, Backdrop::Dark] {
            let args = ([0.3,0.2,0.1,1.0], [0.1,0.1,0.1,1.0], [0.5,0.4,0.3,1.0], [0.8,0.1,0.2,1.0]);
            let protected = Palette::new(backdrop,args.0,args.1,args.2,args.3);
            let art = Palette::for_art(backdrop,args.0,args.1,args.2,args.3);
            assert_eq!(art.veil[3], ART_READING_ALPHA);
            assert_eq!(art.primary,protected.primary);
            assert_eq!(art.secondary,protected.secondary);
            assert_eq!(art.accent,protected.accent);
            assert_eq!(art.selection,protected.selection);
            assert_eq!(art.selected,protected.selected);
            assert_eq!(art.caret(args.3,false),protected.caret(args.3,false));
            assert_eq!(art.caret(args.3,true),protected.caret(args.3,true));
            for token in [art.primary,art.secondary,art.accent,art.selection,art.selected] {
                assert_eq!(token[3],1.0);
            }
            assert!((art.veil[3] * ART_FOOTER_WEIGHT - ART_FOOTER_ALPHA).abs() < 0.000001);
        }
    }

    #[test] fn veil_polarity_and_plain_home_are_preserved() {
        let create=|backdrop|Palette::for_art(backdrop,[1.0;4],[0.0,0.0,0.0,1.0],[0.4,0.4,0.4,1.0],[1.0,0.0,0.0,1.0]);
        assert_eq!(&create(Backdrop::Dark).veil[..3], &[0.0;3]);
        assert_eq!(&create(Backdrop::Light).veil[..3], &[1.0;3]);
        let plain=Palette::new(Backdrop::Theme,[0.48,0.48,0.48,1.0],[0.0,0.0,0.0,1.0],[0.5;4],[1.0,0.0,0.0,1.0]);
        assert_eq!(plain.veil[3],1.0);
        assert!(contrast(plain.primary,plain.surface)>=7.0);
    }

    #[test] fn transparent_artwork_is_not_claimed_to_protect_against_every_pixel() {
        let art=Palette::for_art(Backdrop::Dark,[0.0,0.0,0.0,1.0],[1.0;4],[1.0;4],[1.0;4]);
        let bright_pixel=opaque(art.veil,[1.0;4]);
        // Intentional counterexample: low-alpha art is visual tuning, not AAA.
        assert!(contrast(art.primary,bright_pixel)<7.0);
        assert!(ART_READING_ALPHA>0.0 && ART_READING_ALPHA<=0.12);
        assert!(ART_FOOTER_ALPHA>=0.0 && ART_FOOTER_ALPHA<=0.04);
    }

    #[test] fn new_results_and_news_gap_are_in_this_frames_geometry() {
        let (_,empty)=visible_rows(&[],0,0,10.0,50.0,400.0,240.0,0,8.0); assert!(empty.is_empty());
        let (_,rows)=visible_rows(&[34.0,54.0,34.0],2,0,10.0,50.0,400.0,240.0,1,8.0);
        assert_eq!(rows.last().unwrap().0.bottom(),180.0);
        let (start,rows)=visible_rows(&[54.0;8],8,0,10.0,50.0,400.0,180.0,0,8.0);
        assert_eq!(start,6);assert_eq!(rows.last().unwrap().1,7);assert!(rows.iter().all(|(r,_)|r.bottom()<=180.0));
    }
}
