//! Bundled families and their real supplied weights; no synthetic emboldening.
use crate::app::App;
use nus_render::FontId;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Family { #[default] Plex, Victor, JetBrains, Areal, ArealSemiMono, ArealMono }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Weight { #[default] Regular, Medium, Bold }
impl Weight {
    pub const ALL: [Self;3] = [Self::Regular,Self::Medium,Self::Bold];
    pub fn name(self)-> &'static str {match self {Self::Regular=>"Regular · 400",Self::Medium=>"Medium · 500",Self::Bold=>"Bold · 700"}}
}
impl Family {
    pub const ALL:[Self;6]=[Self::Plex,Self::Victor,Self::JetBrains,Self::Areal,Self::ArealSemiMono,Self::ArealMono];
    pub const MONO:[Self;4]=[Self::Plex,Self::Victor,Self::JetBrains,Self::ArealMono];
    pub fn name(self)-> &'static str {match self {Self::Plex=>"IBM Plex Mono",Self::Victor=>"Victor Mono",Self::JetBrains=>"JetBrains Mono",Self::Areal=>"ABC Areal",Self::ArealSemiMono=>"ABC Areal Semi Mono",Self::ArealMono=>"ABC Areal Mono"}}
    fn bytes(self,weight:Weight)-> &'static [u8] {
        match (self,weight) {
            (Self::Plex,Weight::Regular) => include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf"),
            (Self::Plex,Weight::Medium) => include_bytes!("../../../assets/fonts/IBMPlexMono-Medium.ttf"),
            (Self::Plex,Weight::Bold) => include_bytes!("../../../assets/fonts/IBMPlexMono-Bold.ttf"),
            (Self::Victor,Weight::Regular) => include_bytes!("../../../assets/fonts/VictorMono-Regular.ttf"),
            (Self::Victor,Weight::Medium) => include_bytes!("../../../assets/fonts/VictorMono-Medium.ttf"),
            (Self::Victor,Weight::Bold) => include_bytes!("../../../assets/fonts/VictorMono-Bold.ttf"),
            (Self::JetBrains,Weight::Regular) => include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
            (Self::JetBrains,Weight::Medium) => include_bytes!("../../../assets/fonts/JetBrainsMono-Medium.ttf"),
            (Self::JetBrains,Weight::Bold) => include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf"),
            (Self::Areal,Weight::Regular) => include_bytes!("../../../assets/fonts/ABCAreal-Regular.ttf"),
            (Self::Areal,Weight::Medium) => include_bytes!("../../../assets/fonts/ABCAreal-Medium.ttf"),
            (Self::Areal,Weight::Bold) => include_bytes!("../../../assets/fonts/ABCAreal-Bold.ttf"),
            (Self::ArealSemiMono,Weight::Regular) => include_bytes!("../../../assets/fonts/ABCArealSemiMono-Regular.ttf"),
            (Self::ArealSemiMono,Weight::Medium) => include_bytes!("../../../assets/fonts/ABCArealSemiMono-Medium.ttf"),
            (Self::ArealSemiMono,Weight::Bold) => include_bytes!("../../../assets/fonts/ABCArealSemiMono-Bold.ttf"),
            (Self::ArealMono,Weight::Regular) => include_bytes!("../../../assets/fonts/ABCArealMono-Regular.ttf"),
            (Self::ArealMono,Weight::Medium) => include_bytes!("../../../assets/fonts/ABCArealMono-Medium.ttf"),
            (Self::ArealMono,Weight::Bold) => include_bytes!("../../../assets/fonts/ABCArealMono-Bold.ttf"),
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Typography {
    pub system: [String;3],
    pub ui_scale: f32,
    pub terminal_size: f32,
    pub terminal_line: f32,
    pub terminal_spacing: f32,
    pub editor_family: Family,
    pub editor_weight: Weight,
    pub editor_size: f32,
    pub editor_line: f32,
}
impl Default for Typography { fn default()->Self { Self {system:Default::default(),ui_scale:1.0,terminal_size:13.0,terminal_line:1.0,terminal_spacing:0.0,editor_family:Family::Plex,editor_weight:Weight::Regular,editor_size:13.0,editor_line:1.25} } }
fn finite(v:f32,lo:f32,hi:f32,default:f32)->f32 {if v.is_finite(){v.clamp(lo,hi)}else{default}}
impl Typography {
    pub fn normalize(&mut self){self.ui_scale=finite(self.ui_scale,0.85,1.2,1.0);self.terminal_size=finite(self.terminal_size,9.0,24.0,13.0);self.terminal_line=finite(self.terminal_line,1.0,1.8,1.0);self.terminal_spacing=finite(self.terminal_spacing,0.0,3.0,0.0);self.editor_size=finite(self.editor_size,9.0,24.0,13.0);self.editor_line=finite(self.editor_line,1.0,1.8,1.25);}
}
impl App {
    pub(crate) fn font_face(&mut self,family:Family,weight:Weight,system:&str)->FontId {
        let fallback=self.bundled_face(family,weight);
        if system.is_empty(){return fallback;}
        let value=match weight{Weight::Regular=>400,Weight::Medium=>500,Weight::Bold=>700};
        if let Some((_,_,id))=self.system_font_cache.iter().find(|(f,w,_)|f==system && *w==value){return *id;}
        let id=self.fonts.load_system_weight(system,value,fallback);
        self.system_font_cache.push((system.into(),value,id));id
    }
    pub(crate) fn set_system_font(&mut self,role:u8,name:&str)->bool {
        if role>2{return false;}
        if !name.is_empty(){
            let found=self.fonts.system_families().into_iter().find(|(n,_)|n.eq_ignore_ascii_case(name));
            let Some((family,mono))=found else{self.notice(nus_render::text::icons::TEXT_AA,"Font Not Installed","choose a family from the list");return false;};
            if role>0 && !mono {self.notice(nus_render::text::icons::TEXT_AA,"Choose A Fixed-Width Font","for terminal and code columns");return false;}
            self.behavior.typography.system[role as usize]=family;
        } else {self.behavior.typography.system[role as usize].clear();}
        self.apply_fonts();true
    }
    pub(crate) fn terminal_px(&self)->f32 {self.behavior.typography.terminal_size * self.scale * 96.0/72.0}

    fn bundled_face(&mut self, family:Family, weight:Weight)->FontId {
        if let Some((_,_,id))=self.font_cache.iter().find(|(f,w,_)|*f==family && *w==weight) {return *id;}
        let id=self.fonts.load_bytes(family.bytes(weight),0).expect("validated bundled face");
        self.font_cache.push((family,weight,id));id
    }
    pub(crate) fn apply_fonts(&mut self) {
        self.behavior.typography.normalize();
        let config=self.behavior.typography.clone();
        self.f.ui=self.font_face(self.behavior.ui_font,self.behavior.ui_weight,&config.system[0]);
        self.f.strong=self.font_face(self.behavior.ui_font,Weight::Bold,&config.system[0]);
        self.f.term=self.font_face(self.behavior.term_font,self.behavior.term_weight,&config.system[1]);
        self.f.editor=self.font_face(config.editor_family,config.editor_weight,&config.system[2]);
        let px=self.terminal_px();
        for tab in &mut self.tabs {for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {if let crate::app::Pane::Term(t)=pane {
            t.grid.set_font(&self.fonts,self.f.term,px*t.zoom as f32/100.0);
            t.grid.set_spacing(&self.fonts,config.terminal_line,config.terminal_spacing*self.scale*t.zoom as f32/100.0);
        }}}
        if !self.tabs.is_empty() {self.layout();} self.dirty=true;
    }
    pub(crate) fn font_pairing(&mut self,k:u8) {
        self.behavior.typography=Typography::default();
        let (ui,term)=match k {1=>(Family::Areal,Family::JetBrains),2=>(Family::ArealSemiMono,Family::ArealMono),3=>(Family::Areal,Family::Victor),_=>(Family::Plex,Family::Plex)};
        self.behavior.ui_font=ui;self.behavior.term_font=term;self.behavior.ui_weight=Weight::Regular;self.behavior.term_weight=Weight::Regular;
        self.behavior.typography.editor_family=term;self.apply_fonts();
    }

}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn terminal_families_keep_fixed_columns_at_each_weight() {
        let mut fonts=nus_render::FontSystem::new();
        for family in Family::MONO {for weight in Weight::ALL {
            let id=fonts.load_bytes(family.bytes(weight),0).unwrap();
            let style=nus_render::Style{font:id,px:16.0,color:[1.0;4],tracking:0.0};
            let advance=fonts.measure(style,"0");
            for glyph in ["i","W","m"," ","."] {assert!((fonts.measure(style,glyph)-advance).abs()<0.01,"{} {} {glyph}",family.name(),weight.name());}
        }}
    }
    #[test] fn bundled_faces_load_at_every_exposed_weight() {
        let mut fonts=nus_render::FontSystem::new();
        for family in Family::ALL {for weight in Weight::ALL {
            assert!(fonts.load_bytes(family.bytes(weight),0).is_ok(),"{} {}",family.name(),weight.name());
        }}
    }
}
