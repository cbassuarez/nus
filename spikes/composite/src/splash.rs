//! The splash: the icon, exactly as the taskbar has it, on paper. The
//! swoosh draws itself in as the window comes up, holds until the first
//! tab is ready, then the whole thing fades. Nothing else on it. With
//! HOME · THE PLATE and THEN · THE PROMPT it draws the icon where the
//! plate keeps it, at the plate's size, and hands the prompt over with the
//! icon in place: only the paper over the chrome fades (plate.rs).

use std::sync::Arc;
use std::time::Instant;

use nus_render::Rect;

use crate::anim::Anim;
use crate::app::App;

pub struct Splash {
    pub arrival: bool,
    /// When the first frame was drawn: the clock starts there, not at
    /// App::new (the window comes up a good while after).
    pub started: Instant,
    pub begun: bool,
    /// The icon texture at the progress it was rendered for.
    pub tex: Option<(f32, Arc<wgpu::BindGroup>)>,
    pub fade: Anim,
    pub leaving: bool,
}

/// Seconds the swoosh takes to draw (before the motion register).
const DRAW: f32 = crate::plate::DRAW;
/// Never shorter than this; never longer than the max, ready or not.
const MAX: f32 = 2.6;
const SIZE: u32 = 256;

impl Splash {
    pub fn new() -> Splash {
        Splash { arrival: false, started: crate::clock::now(), begun: false, tex: None, fade: Anim::at(1.0), leaving: false }
    }
}

impl App {
    /// The birth tab — the first window's shell, split or not — gives way
    /// to `pane` alone, when it is still the shell it was born with.
    pub(crate) fn replace_birth(&mut self, pane: crate::app::Pane) {
        if let Some(t) = self.tabs.first_mut() {
            if matches!(t.left, crate::app::Pane::Term(_) | crate::app::Pane::Home(_)) {
                t.left = pane;
                t.right = None;
                t.focus_right = false;
                t.solo = false;
            }
        }
        self.layout();
        self.dirty = true;
    }

    /// Whether the first tab has something to show.
    fn splash_ready(&self) -> bool {
        self.tabs.iter().any(|t| {
            std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| match p {
                crate::app::Pane::Term(tp) => tp.term.grid().is_damaged() || tp.title != "shell",
                crate::app::Pane::Web(w) => w.tab.shared.borrow().paints > 0,
                _ => true,
            })
        })
    }

    pub fn draw_splash(&mut self, scene: &mut nus_render::Scene) {
        let Some(sp) = self.splash.as_mut() else { return };
        if !sp.begun {
            sp.begun = true;
            sp.started = crate::clock::now();
        }
        let elapsed = crate::clock::since(sp.started).as_secs_f32();
        if self.splash.as_ref().is_some_and(|sp|sp.arrival) {self.draw_arrival(scene,elapsed);return;}
        let k = self.plate_k();
        let draw_secs = if self.motion.reduced() || self.behavior.splash != crate::settings::SplashMode::Draw { 0.0 } else { DRAW * k };
        if self.behavior.splash == crate::settings::SplashMode::None {
            self.splash = None;
            return;
        }
        let progress = if draw_secs <= 0.0 { 1.0 } else { crate::plate::swoosh((elapsed / draw_secs).clamp(0.0, 1.0)) };
        let plate = self.plate_continues();
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        // The icon: where the plate keeps it, from the plate's own field;
        // or 160 px in the middle, from the same field at 256.
        let (rect, bind) = if plate {
            let pane = self.tabs.first().map(|t| t.left.rect()).unwrap_or(Rect::new(0.0, 0.0, w, h));
            let rect = crate::plate::icon_rect(pane);
            let size = (rect.w.round() as u32).clamp(64, 1024);
            (rect, Some(self.plate_texture(size, progress)))
        } else {
            let bind = self.plate_texture(SIZE, progress);
            let size = (160.0 * self.scale).round();
            let rect = Rect::new(((w - size) / 2.0).round(), ((h - size) / 2.0).round() - self.px(12.0), size, size);
            (rect, Some(bind))
        };
        // The plate needs nothing of the first tab: it is the first tab.
        let ready = plate || self.splash_ready();
        let paper = self.paper();
        let reduced = self.motion.reduced();
        let hold = self.behavior.splash_hold.max(0.2);
        let Some(sp) = self.splash.as_mut() else { return };
        let mut hand_over = false;
        if !sp.leaving && elapsed >= hold.max(draw_secs) && (ready || elapsed >= MAX.max(hold)) {
            sp.leaving = true;
            let out = if reduced { 0.0 } else { 0.26 * k };
            sp.fade.replay(1.0, 0.0, out);
            hand_over = plate;
        }
        let alpha = sp.fade.value();
        if sp.leaving && alpha <= 0.001 && !sp.fade.active() {
            self.splash = None;
            self.dirty = true;
            return;
        }
        if hand_over {
            // The prompt comes up under the same icon while the paper over
            // the chrome fades; the tick's THEN step has nothing left to do.
            self.then_done = true;
            let mut home = crate::home::HomePane::new();
            home.handed = true;
            self.replace_birth(crate::app::Pane::Home(home));
        }
        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), [paper[0], paper[1], paper[2], alpha]);
        if let Some(bind) = bind {
            // Under the plate the icon beneath is the same pixels: full alpha.
            scene.texture_alpha(rect, bind, if plate { 1.0 } else { alpha });
            scene.layer(None);
        }
        // Keep drawing while it is up.
        self.dirty = true;
    }
}


/// Camera distance is integrated acceleration, not an expanding radial drawing.
/// Each star keeps a fixed world x/y while the camera moves through its z plane.
fn arrival_distance(seconds: f32) -> f32 {
    let t = seconds.max(0.0);
    0.035 * t + 1.1 * (t - 0.4).max(0.0).powi(3)
}

fn arrival_star(index: u32, seconds: f32) -> ([f32; 2], [f32; 2], f32) {
    let random = |salt: u32| {
        let mut n = index.wrapping_mul(747796405).wrapping_add(salt);
        n = (n ^ (n >> 16)).wrapping_mul(2246822519);
        n = (n ^ (n >> 13)).wrapping_mul(3266489917);
        (n ^ (n >> 16)) as f32 / u32::MAX as f32
    };
    let world = [(random(17) - 0.5) * 5.2, (random(79) - 0.5) * 3.2];
    let distance = arrival_distance(seconds);
    let depth = (random(131) * 4.0 - distance).rem_euclid(4.0) + 0.12;
    // Project both ends of a short camera exposure: near stars naturally streak
    // faster and longer than distant ones. Recycled stars enter at the far plane.
    let previous_depth = (depth + distance - arrival_distance(seconds - 0.035)).min(4.12);
    let head = [world[0] / depth, world[1] / depth];
    let tail = [world[0] / previous_depth, world[1] / previous_depth];
    (tail, head, depth)
}

impl App {
    pub(crate) fn finish_arrival(&mut self) {
        if self.splash.as_ref().is_some_and(|s|s.arrival) {
            let _=crate::store::write_atomic(std::path::Path::new("profile/arrival-seen"),b"1");
        }
    }

    /// A special first-open Atlas: one mark, one acceleration, then your workspace.
    /// No new window, assets, timers or sound. Work is bounded to 192 projected stars; reduced motion uses a still mark.
    fn draw_arrival(&mut self, scene:&mut nus_render::Scene, elapsed:f32) {
        use nus_render::{Instance,text::Style};
        use crate::app::fade;
        let reduced=self.motion.reduced();
        let duration=if reduced {0.55}else{3.1};
        if elapsed>=duration || self.behavior.splash==crate::settings::SplashMode::None {
            self.finish_arrival();self.splash=None;self.dirty=true;return;
        }
        let (w,h)=(self.target.size.0 as f32,self.target.size.1 as f32);
        let ease=|v:f32|crate::plate::swoosh(v.clamp(0.0,1.0));
        let leave=if reduced{ease((elapsed-0.25)/0.3)}else{ease((elapsed-2.5)/0.6)};
        let alpha=1.0-leave;
        let scale=self.scale;let px=|v:f32|v*scale;
        scene.layer(None);
        scene.rect(Rect::new(0.0,0.0,w,h),fade(self.paper(),alpha));
        let cw=px(560.0).min(w-px(32.0)).max(1.0);
        let ch=px(300.0).min(h-px(40.0)).max(1.0);
        let r=Rect::new((w-cw)/2.0,(h-ch)*0.38,cw,ch);
        self.draw_atlas_frame(scene,r,alpha);
        let inside=Rect::new(r.x+px(3.0),r.y+px(3.0),r.w-px(6.0),r.h-px(6.0));
        scene.layer(Some(inside));
        let center=[r.x+r.w/2.0,r.y+r.h*0.43];
        if !reduced {
            let focal=cw*0.65;
            let appear=ease(elapsed/0.22);
            for i in 0..192 {
                let (tail,head,depth)=arrival_star(i,elapsed);
                let a=[center[0]+tail[0]*focal,center[1]+tail[1]*focal];
                let b=[center[0]+head[0]*focal,center[1]+head[1]*focal];
                let dx=b[0]-a[0];let dy=b[1]-a[1];let length=dx.hypot(dy);
                let far_fade=((4.12-depth)/0.35).clamp(0.0,1.0);
                let brightness=(0.25+0.65*(1.0-depth/4.12))*appear*far_fade*alpha;
                let color=fade(self.theme.ink,brightness);
                let radius=px((0.6+0.3/depth).min(1.65));
                if length>radius {
                    let nx=-dy/length;let ny=dx/length;
                    // Taper from a distant point toward the approaching head.
                    scene.push(Instance::quad([
                        [a[0]-nx*radius*0.18,a[1]-ny*radius*0.18],
                        [b[0]-nx*radius,b[1]-ny*radius],
                        [b[0]+nx*radius,b[1]+ny*radius],
                        [a[0]+nx*radius*0.18,a[1]+ny*radius*0.18],
                    ],color));
                }
                scene.push(Instance::rounded(Rect::new(b[0]-radius,b[1]-radius,radius*2.0,radius*2.0),radius,color));
            }
        }
        let grow=if reduced{1.0}else{ease((elapsed-0.3)/0.7)};
        let word=Style{font:self.f.wordmark,px:px(58.0+18.0*grow),color:fade(self.theme.ink,alpha),tracking:0.0};
        let nw=self.fonts.measure(word,"n");let full=self.fonts.measure(word,"nus");
        let x=center[0]-(nw+(full-nw)*grow)/2.0;
        // A paper bed keeps the mark legible while streaks pass behind it.
        scene.rect(Rect::new(x-px(12.0),center[1]-word.px*0.8,full+px(24.0),word.px*1.1),fade(self.paper(),alpha));
        self.fonts.draw(scene,word,x,center[1]+word.px*0.22,"n");
        self.fonts.draw(scene,Style{color:fade(word.color,grow),..word},x+nw,center[1]+word.px*0.22,"us");
        let label=Style{color:fade(self.theme.dim,alpha*grow),..self.label()};
        let caption=if self.previous_install.is_some(){"Welcome back."}else{"Make yourself at home."};
        let width=self.fonts.measure(label,caption);
        self.fonts.draw(scene,label,center[0]-width/2.0,r.y+r.h-px(53.0),caption);
        let hint=Style{color:fade(self.theme.dim,alpha*0.65),..self.label()};
        self.fonts.draw(scene,hint,r.x+px(20.0),r.bottom()-px(18.0),"ESC · CONTINUE");
        scene.layer(None);self.dirty=true;
    }
}

#[cfg(test)]
mod arrival_tests {
    use super::*;
    #[test]
    fn points_become_perspective_streaks_as_camera_accelerates() {
        let extent = |t| {
            (0..192).map(|i| {let (a,b,_)=arrival_star(i,t);(b[0]-a[0]).hypot(b[1]-a[1])}).sum::<f32>()
        };
        assert!(extent(1.8)>extent(0.2)*20.0);
        for i in 0..192 {for tick in 0..310 {
            let (a,b,z)=arrival_star(i,tick as f32/100.0);
            assert!(a.into_iter().chain(b).all(f32::is_finite));
            assert!((0.12..=4.12).contains(&z));
            // Both projections lie on one perspective ray, with the head nearer.
            assert!((a[0]*b[1]-a[1]*b[0]).abs()<0.0001);
            assert!(b[0].hypot(b[1])+0.00001>=a[0].hypot(a[1]));
        }}
    }
}
