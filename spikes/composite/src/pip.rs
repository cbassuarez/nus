//! Picture-in-picture: our own always-on-top window showing a tab's texture
//! cropped to its video, with eased moves/resizes and transport that acts
//! on the element through the DevTools channel.
//!
//! The controls sit **over the picture**, the shape every other browser
//! uses, so nobody has to learn ours: a scrim, the tab's name and the way
//! back at the top, the transport in the middle, the scrubber along the
//! foot. They come up while the pointer is on the window (and stay up
//! while the video is paused, so a stopped window says how to start it)
//! and fade a beat after it leaves. Every one of them has a key too, and
//! the keys work whether or not the controls are showing.
//!
//! Two furnishings are named and can be turned off (BROWSER · PICTURE IN
//! PICTURE): **the band**, the signal stripe along the top that says the
//! window is ours and gives you somewhere to take hold, on by default;
//! and the **progress rule**, a hairline of played time along the foot
//! that is there whether or not the controls are, off by default — the
//! scrubber already tells you, and a window at rest should be a picture.

use std::sync::Arc;
use std::time::{Duration, Instant};

use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Target};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::Window;

use crate::app::App;
use crate::browser::Video;

#[path="pip_geometry.rs"] mod geometry;
#[path="pip_native.rs"] mod native;

/// Logical-pixel rectangle on the desktop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct Pip {
    pub window: Arc<Window>,
    pub target: Target,
    pub scene: Scene,
    /// Which tab and pane the video lives in.
    pub tab: usize,
    pub tab_id: u64,
    pub right: bool,
    pub focused: bool,
    pub cur: LRect,
    /// Authoritative stream ratio; native pixel rounding must not change it.
    pub aspect: f64,
    pub last_frame: Instant,
    pub opening: Option<Instant>,
    pub first_frame_ms: Option<f64>,
    pub dragging: bool,
    /// Left button is down (a drag starts once the cursor moves).
    pub pressed: bool,
    /// Desktop work area (logical) for snapping.
    pub area: LRect,
    /// The pointer inside this window, in the scene's own pixels.
    pub pos: (f32, f32),
    /// The pointer is over the window.
    pub inside: bool,
    /// When it left, for the fade out.
    pub left_at: Option<Instant>,
    /// The controls as last drawn: where each one is.
    pub hits: Vec<(Rect, Hit)>,
    /// What the press landed on, so a press on a control never drags the
    /// window out from under it.
    pub press_hit: Option<Hit>,
    /// Dragging the scrubber.
    pub scrubbing: bool,
    pub key_focus: Option<Hit>,
    pub mods: winit::keyboard::ModifiersState,
    track: Option<Rect>,
    gesture: Option<(LRect, (f64, f64), (i8, i8))>,
    press_origin: (f64, f64),
    area_checked: Instant,
}

/// One control on the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Play or pause, whichever it is not.
    Play,
    /// Ten seconds back, ten on.
    Back,
    Forward,
    Mute,
    /// Back to the tab it came from.
    ToTab,
    Close,
    Smaller,
    Larger,
    /// The scrubber: seek to where it was pressed, and follow the drag.
    Track,
}

/// How long the controls stay up after the pointer leaves, and how long
/// they take to go.
const LINGER: Duration = Duration::from_millis(1200);
const FADE: Duration = Duration::from_millis(220);

const MARGIN: f64 = 24.0;

/// Seconds as a player writes them: m:ss, or h:mm:ss past the hour.
fn clock(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "--:--".into();
    }
    let s = secs as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}


impl Pip {
    pub fn new(window: Arc<Window>, target: Target, tab: usize, right: bool, area: LRect, start: LRect) -> Pip {
        Pip {
            window,
            target,
            scene: Scene::new(),
            tab,
            tab_id: 0,
            right,
            focused: false,
            cur: start,
            aspect: start.w / start.h,
            last_frame: crate::clock::now() - Duration::from_millis(17),
            opening: Some(Instant::now()),
            first_frame_ms: None,
            dragging: false,
            pressed: false,
            area,
            pos: (0.0, 0.0),
            inside: false,
            left_at: Some(crate::clock::now()),
            hits: Vec::new(),
            press_hit: None,
            scrubbing: false,
            key_focus: None,
            mods: winit::keyboard::ModifiersState::empty(),
            track: None,
            gesture: None,
            press_origin: (0.0, 0.0),
            area_checked: crate::clock::now(),
        }
    }

    /// How present the controls are: all the way while the pointer is on
    /// the window or the video is stopped, then a beat, then gone.
    pub fn controls_alpha(&self, paused: bool, reduced: bool) -> f32 {
        if self.inside || self.dragging || self.scrubbing || self.key_focus.is_some() || paused {
            return 1.0;
        }
        let Some(left) = self.left_at else { return 0.0 };
        let since = crate::clock::since(left);
        if since < LINGER {
            return 1.0;
        }
        if reduced {
            return 0.0;
        }
        let t = (since - LINGER).as_secs_f32() / FADE.as_secs_f32();
        (1.0 - t).clamp(0.0, 1.0)
    }

    /// The control under `pos`, if any.
    pub fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }

    /// Where along the scrubber `x` falls, 0..1.
    pub fn track_fraction(&self, x: f32) -> f32 {
        let Some(r) = self.track else { return 0.0 };
        ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0)
    }

    /// Where a `w`×`h` window should land: the nearest corner to `near`.
    pub fn snap_target(&self, w: f64, h: f64, near: (f64, f64)) -> LRect {
        let a = self.area;
        let left = a.x + MARGIN;
        let right = a.x + a.w - w - MARGIN;
        let top = a.y + MARGIN;
        let bottom = a.y + a.h - h - MARGIN;
        let x = if near.0 < a.x + a.w / 2.0 { left } else { right };
        let y = if near.1 < a.y + a.h / 2.0 { top } else { bottom };
        geometry::fit(LRect { x, y, w, h }, a, MARGIN)
    }

    fn apply_rect(&mut self, rect: LRect) {
        let rect=geometry::fit(LRect{h:rect.w/self.aspect,..rect},self.area,16.0);
        let old=self.cur;
        self.cur=rect;
        if (old.x-rect.x).abs()>=0.25 || (old.y-rect.y).abs()>=0.25 {
            self.window.set_outer_position(LogicalPosition::new(rect.x,rect.y));
        }
        if (old.w-rect.w).abs()>=0.25 || (old.h-rect.h).abs()>=0.25 {
            let _=self.window.request_inner_size(LogicalSize::new(rect.w,rect.h));
        }
        self.window.request_redraw();
    }

    fn constrain_native_size(&mut self, w: u32, h: u32) {
        if self.pressed || w==0 || h==0 {return;}
        let scale=self.window.scale_factor();
        let (width,height)=(w as f64/scale,h as f64/scale);
        // Both dimensions round independently to physical pixels. Keep
        // exact logical geometry without chasing subpixel differences.
        if (w as f64-h as f64*self.aspect).abs()>(1.0+self.aspect)*0.5+0.01 {
            let corrected=geometry::native_resize(self.cur,width,height,self.aspect,self.area);
            self.cur.w=width;self.cur.h=height;
            self.apply_rect(corrected);
        } else if (self.cur.w*scale-w as f64).abs()>1.0 || (self.cur.h*scale-h as f64).abs()>1.0 {
            self.cur.w=width;self.cur.h=width/self.aspect;
        }
    }

    fn edge_at(&self,x:f32,y:f32)->(i8,i8) {
        let s=self.window.scale_factor()as f32;let d=7.0*s;
        let (w,h)=(self.target.size.0 as f32,self.target.size.1 as f32);
        (if x<d {-1} else if x>w-d {1}else{0},if y<d {-1}else if y>h-d {1}else{0})
    }


}

impl App {
    /// Ask the host to create a PiP window for `tab`'s web pane.
    pub(crate) fn tend_pip_focus(&mut self) {
        let minimized=self.window.is_minimized().unwrap_or(false);
        if self.pip_was_minimized && !minimized && self.behavior.pip_policy.restore_window {self.close_pip();}
        if !self.pip_was_minimized && minimized && self.behavior.pip_policy.leave_app {
            self.pip_away_pending=Some(crate::clock::now());
        }
        self.pip_was_minimized=minimized;
        let Some(since)=self.pip_away_pending else{return;};
        let elapsed=crate::clock::since(since).as_secs_f32();
        // Focus moves through native child windows too. Let it settle, and let
        // a late media report arrive after Alt-Tab before giving up.
        if elapsed<0.18{return;}
        let child_focused=self.pip.as_ref().is_some_and(|p|p.window.has_focus())
            ||self.hatch.as_ref().is_some_and(|p|p.window.has_focus())
            ||self.little.as_ref().is_some_and(|p|p.window.has_focus())
            ||self.menu_drawer.window.as_ref().is_some_and(|p|p.window.has_focus());
        if !self.behavior.pip_policy.leave_app || self.window_focused && !minimized || child_focused || self.pip.is_some() {
            self.pip_away_pending=None;return;
        }
        if let Some(right)=self.playing_video(self.active) {
            self.request_pip(self.active,right);self.pip_away_pending=None;
        } else if elapsed>3.0 {self.pip_away_pending=None;}
    }

    pub fn request_pip(&mut self, tab: usize, right: bool) {
        if self.pip_request.is_some() || self.web_tab(tab,right).is_none() {
            return;
        }
        self.pip_request = Some((tab, right));
    }

    /// Keep the native window and GPU surface warm when changing the source.
    pub fn retarget_pip(&mut self, tab: usize, right: bool) -> bool {
        let started=Instant::now();
        if self.web_tab(tab,right).is_none() {return false;}
        let Some(p)=self.pip.as_mut() else {return false;};
        p.opening=Some(started);p.first_frame_ms=None;
        p.tab=tab; p.tab_id=self.tabs[tab].id; p.right=right;
        p.track=None; p.hits.clear(); p.key_focus=None; p.scrubbing=false;
        p.pressed=false; p.dragging=false; p.gesture=None; p.press_hit=None;
        p.left_at=Some(crate::clock::now());
        p.last_frame=crate::clock::now()-Duration::from_millis(17);
        if let Some(t)=self.web_tab(tab,right) {t.prepare_pip();}
        self.pip_frame();
        true
    }

    /// Called by the host with the freshly created window.
    pub fn attach_pip(&mut self, window: Arc<Window>, tab: usize, right: bool, previous: Option<LRect>) {
        let started=Instant::now();
        if self.web_tab(tab,right).is_none() {window.set_visible(false);return;}
        native::configure(&window);
        let area=native::work_area(&self.window);
        let aspect = self.pane_video(tab, right).and_then(|v| geometry::stream_aspect(v.video_width,v.video_height)).unwrap_or(16.0 / 9.0);
        let w = 480.0;
        let h = w / aspect;
        let start=previous.unwrap_or(LRect{x:area.x+area.w-w-MARGIN,y:area.y+area.h-h-MARGIN,w,h});
        let to=geometry::with_aspect(start,aspect,area);
        window.set_outer_position(LogicalPosition::new(to.x,to.y));
        let _=window.request_inner_size(LogicalSize::new(to.w,to.h));
        let Ok(target)=self.gpu.target(window.clone()) else {window.set_visible(false);return;};
        let mut pip=Pip::new(window,target,tab,right,area,to);
        pip.opening=Some(started);
        pip.tab_id=self.tabs[tab].id;
        if let Some(t)=self.web_tab(tab,right) {t.prepare_pip();}
        self.pip=Some(pip);
        self.pip_frame();
        if let Some(p)=&self.pip {native::show(&p.window);}
    }

    pub fn close_pip(&mut self) {
        self.pip_away_pending = None;
        self.pip_request = None;
        self.pip = None;
    }

    fn pane_video(&self, tab: usize, right: bool) -> Option<Video> {
        self.web_tab(tab, right).and_then(|t| t.video())
    }

    fn web_tab(&self, tab: usize, right: bool) -> Option<&crate::browser::BrowserTab> {
        let t = self.tabs.get(tab)?;
        let pane = if right { t.right.as_ref()? } else { &t.left };
        match pane {
            crate::app::Pane::Web(w) => Some(&w.tab),
            _ => None,
        }
    }

    /// A playing video in `tab`, if any: (pane is right, video).
    pub fn playing_video(&self, tab: usize) -> Option<bool> {
        for right in [false, true] {
            if let Some(v) = self.pane_video(tab, right) {
                if !v.paused && !v.ended {
                    return Some(right);
                }
            }
        }
        None
    }

    /// Per-frame: draw the cropped video, ease the window, retire PiP when
    /// the tab is gone.
    pub fn pip_frame(&mut self) {
        let Some(pip) = self.pip.as_mut() else { return };
        let Some(index)=self.tabs.iter().position(|t|t.id==pip.tab_id) else {self.pip=None;return;};
        pip.tab=index;
        // Native resize callbacks can be coalesced (or omitted during a
        // programmatic change). Render against the actual drawable size.
        let size=pip.window.inner_size();
        pip.constrain_native_size(size.width,size.height);
        let size=pip.window.inner_size();
        if size.width>0 && size.height>0 && (size.width,size.height)!=pip.target.size {pip.target.resize(&self.gpu.device,size.width,size.height);}
        if crate::clock::since(pip.area_checked)>Duration::from_millis(350) {
            pip.area_checked=crate::clock::now();
            let area=native::work_area(&pip.window);
            if area!=pip.area {pip.area=area;pip.apply_rect(geometry::fit(pip.cur,area,16.0));}
        }
        if crate::clock::since(pip.last_frame) < Duration::from_millis(16) {
            return;
        }
        pip.last_frame = crate::clock::now();
        let (tab, right) = (pip.tab, pip.right);
        let Some(t) = self.web_tab(tab, right) else {
            self.pip = None;
            return;
        };
        let shared = t.shared.borrow();
        let (bind, video) = (shared.bind.clone(), shared.video.clone());
        drop(shared);
        let pip = self.pip.as_mut().unwrap();
        if let Some(aspect)=video.as_ref().and_then(|v|geometry::stream_aspect(v.video_width,v.video_height)) {
            if (aspect-pip.aspect).abs()>f64::EPSILON*aspect {
                pip.aspect=aspect;
                // A resize begun for the old source must not restore its ratio.
                pip.gesture=None;pip.pressed=false;pip.dragging=false;
                pip.apply_rect(geometry::with_aspect(pip.cur,aspect,pip.area));
                let size=pip.window.inner_size();
                if size.width>0 && size.height>0 {pip.target.resize(&self.gpu.device,size.width,size.height);}
            }
        }
        let theme = self.theme.clone();
        let scale = pip.window.scale_factor() as f32;
        let (w, h) = (pip.target.size.0 as f32, pip.target.size.1 as f32);
        // The scene comes out while we draw, so the whole of App is free
        // to be asked for fonts, metrics and the theme.
        let mut scene = std::mem::replace(&mut pip.scene, Scene::new());
        scene.clear();
        scene.layer(None);
        let band = if self.behavior.pip_band { (4.0 * scale).round() } else { 0.0 };
        // Decorations overlay the stream; subtracting the band would squeeze it.
        let picture = Rect::new(0.0, 0.0, w, h);
        let picture_ready=bind.is_some() && video.is_some();
        if let (Some(bind), Some(v)) = (bind, video.clone()) {
            scene.rect(picture,[0.0,0.0,0.0,1.0]);
            // Missing page pixels (object-fit:cover or an offscreen edge)
            // remain blank in their original position, never stretched.
            if v.w>0.0 && v.h>0.0 && v.vw>0.0 && v.vh>0.0 {
                let x=v.x.max(0.0);let y=v.y.max(0.0);
                let cw=(v.x+v.w).min(v.vw)-x;let ch=(v.y+v.h).min(v.vh)-y;
                if cw>0.0 && ch>0.0 {
                    let [dx,dy,dw,dh]=v.picture;
                    let dest=Rect::new((dx+(x-v.x)/v.w*dw)*w,(dy+(y-v.y)/v.h*dh)*h,cw/v.w*dw*w,ch/v.h*dh*h);
                    scene.texture_uv(dest,[x/v.vw,y/v.vh,(x+cw)/v.vw,(y+ch)/v.vh],bind,None);
                }
            }
            scene.layer(None);
            // The progress rule: played time along the foot, there
            // whether or not the controls are. Off by default.
            if self.behavior.pip_progress && v.dur > 0.0 {
                let p = (v.t / v.dur).clamp(0.0, 1.0) as f32;
                let t = (2.0 * scale).round();
                scene.rect(Rect::new(0.0, h - t, w * p, t), self.surface.signal);
            }
        }
        // The controls, over the picture.
        let reduced = self.motion.reduced();
        let alpha = {
            let pip = self.pip.as_ref().unwrap();
            pip.controls_alpha(video.as_ref().is_some_and(|v| v.paused), reduced)
        };
        let hits = if alpha > 0.01 {
            self.draw_pip_controls(&mut scene, picture, scale, alpha, video.as_ref())
        } else {
            Vec::new()
        };
        let pip = self.pip.as_mut().unwrap();
        pip.hits = hits;
        // Texture lives on the carapace only; the video stays clean.
        if band > 0.0 {
            scene.rect(Rect::new(0.0, 0.0, w, band), self.surface.signal);
            if let (Some(kind), true) = (self.surface.texture_kind.shader_kind(), self.surface.texture > 0.0) {
                scene.push(nus_render::Instance::texture_kind(Rect::new(0.0, 0.0, w, band), kind, [1.0, 1.0, 1.0, self.surface.texture], self.surface.texture_scale * scale, 0.0));
            }
        }
        if pip.focused {
            let t = (m::FLOATING * scale).round();
            scene.push(nus_render::Instance::stroke(Rect::new(0.0, 0.0, w, h), 0.0, t, theme.ink, None, 0.0));
        }
        scene.finish();
        pip.scene = scene;
        let clear = theme.paper;
        let pip = self.pip.as_mut().unwrap();
        pip.window.pre_present_notify();
        let Pip { target, scene, .. } = pip;
        for (x,y,w,h,data) in self.fonts.uploads.drain(..) {self.gpu.upload_glyph(x,y,w,h,&data);}
        let presented=self.gpu.render(target, scene, clear);
        if presented && picture_ready {
            let p=self.pip.as_mut().unwrap();
            if let Some(started)=p.opening.take() {
                let ms=started.elapsed().as_secs_f64()*1000.0;p.first_frame_ms=Some(ms);
                tracing::info!(tab=p.tab_id,milliseconds=ms,"PiP first picture presented");
            }
        }
        // While the controls are fading, keep the frames coming.
        if alpha > 0.0 && alpha < 1.0 {
            self.pip.as_ref().unwrap().window.request_redraw();
        }
    }

    /// The overlay: a scrim, the name and the way back at the top, the
    /// transport in the middle, the scrubber along the foot. Everything
    /// is drawn in the window's own pixels, so it is returned as hit
    /// rectangles in those same pixels.
    fn draw_pip_controls(&mut self, scene: &mut Scene, r: Rect, scale: f32, alpha: f32, v: Option<&Video>) -> Vec<(Rect, Hit)> {
        use nus_render::text::{icons, Style};
        let mut hits: Vec<(Rect, Hit)> = Vec::new();
        let px = |n: f32| (n * scale).round();
        let a = |c: nus_render::Color, k: f32| crate::app::fade(c, k * alpha);
        // A video is a video whatever the app's theme is: the marks are
        // white and the scrim is black, the way every player does it, so
        // they read over a bright picture as well as a dark one.
        let paper: nus_render::Color = [1.0, 1.0, 1.0, 1.0];
        scene.rect(r, a([0.0, 0.0, 0.0, 1.0], 0.5));
        let (mx, my) = self.pip.as_ref().map(|p| p.pos).unwrap_or((-1.0, -1.0));
        let narrow = r.w < px(320.0);
        let seekable=v.is_some_and(|v|v.dur.is_finite()&&v.dur>0.0);

        // One mark, centred on (cx, cy), with a lit square behind it when
        // the pointer is on it.
        let mark = |app: &mut App, scene: &mut Scene, icon: (&'static str, &'static str), size: f32, cx: f32, cy: f32, hit: Hit, hits: &mut Vec<(Rect, Hit)>| {
            let b = Rect::new(cx - size / 2.0, cy - size / 2.0, size, size);
            let reach = crate::touch::grown(b, px(10.0));
            let focused=app.pip.as_ref().is_some_and(|p|p.key_focus==Some(hit));
            let hot = reach.contains(mx, my) || focused;
            if hot {
                scene.rect(reach, a(paper, 0.16));
                if focused {scene.push(nus_render::Instance::stroke(reach,0.0,px(1.0),a(paper,0.9),None,0.0));}
            }
            app.fonts.draw_icon(scene, icon, size, b.x, b.y, a(paper, if hot { 1.0 } else { 0.86 }));
            hits.push((reach, hit));
        };

        // Top: what it is on the left, the way out on the right.
        let top = r.y + px(16.0);
        let isz = px(15.0);
        let label = Style { font: self.f.ui, px: px(11.0), color: a(paper, 0.8), tracking: 0.0 };
        let title = self.pip.as_ref().and_then(|p| self.tabs.get(p.tab)).map(|tab| tab.title()).unwrap_or_default();
        let name_w = r.w - px(174.0);
        if !narrow && name_w > px(40.0) {
            let fit = self.fit(label, &title, name_w);
            self.fonts.draw(scene, label, r.x + px(14.0), top + px(4.0), &fit);
        }
        let mut rx = r.right() - px(14.0) - isz;
        mark(self, scene, icons::CLOSE, isz, rx + isz / 2.0, top, Hit::Close, &mut hits);
        rx -= isz + px(16.0);
        mark(self, scene, icons::TO_TAB, isz, rx + isz / 2.0, top, Hit::ToTab, &mut hits);

        rx -= isz + px(20.0);
        mark(self,scene,icons::PLUS,isz,rx+isz/2.0,top,Hit::Larger,&mut hits);
        rx -= isz + px(16.0);
        mark(self,scene,icons::MINUS,isz,rx+isz/2.0,top,Hit::Smaller,&mut hits);

        // Middle: ten back, play or pause, ten on.
        let paused = v.map(|v| v.paused).unwrap_or(true);
        let cy = r.y + r.h / 2.0;
        let big = px(if narrow { 26.0 } else { 34.0 });
        let small = px(if narrow { 17.0 } else { 21.0 });
        let gap = px(if narrow { 34.0 } else { 46.0 });
        let cx = r.x + r.w / 2.0;
        mark(self, scene, if paused { icons::PLAY_FILL } else { icons::PAUSE_FILL }, big, cx, cy, Hit::Play, &mut hits);
        mark(self, scene, icons::BACK_10, small, cx - gap, cy, Hit::Back, &mut hits);
        mark(self, scene, icons::FORWARD_10, small, cx + gap, cy, Hit::Forward, &mut hits);
        // The ten, inside each arrow, the way every other player writes it.
        {
            let ten = Style { font: self.f.ui, px: px(7.5), color: a(paper, 0.9), tracking: 0.0 };
            let amount=self.behavior.pip_skip_seconds.clamp(1,120).to_string();
            let tw = self.fonts.measure(ten, &amount);
            for x in [cx - gap, cx + gap] {
                self.fonts.draw(scene, ten, (x - tw / 2.0).round(), (cy + px(3.0)).round(), &amount);
            }
        }

        // Foot: elapsed, the scrubber, the run time, and the speaker.
        let foot = r.bottom() - px(18.0);
        let msz = px(14.0);
        let mut x0 = r.x + px(14.0);
        let mut x1 = r.right() - px(14.0) - msz;
        mark(self, scene, if v.map(|v| v.muted).unwrap_or(false) { icons::SPEAKER_OFF } else { icons::SPEAKER }, msz, x1 + msz / 2.0, foot, Hit::Mute, &mut hits);
        x1 -= px(14.0);
        let time = Style { font: self.f.ui, px: px(10.0), color: a(paper, 0.78), tracking: 0.0 };
        if !seekable {
            if let Some(p)=self.pip.as_mut(){p.track=None;}return hits;
        }
        if !narrow {
            if let Some(v) = v {
                let (l, r2) = (clock(v.t), clock(v.dur));
                self.fonts.draw(scene, time, x0, foot + px(3.5), &l);
                x0 += self.fonts.measure(time, &l) + px(10.0);
                let rw = self.fonts.measure(time, &r2);
                x1 -= rw + px(10.0);
                self.fonts.draw(scene, time, x1 + px(10.0), foot + px(3.5), &r2);
            }
        }
        // The track: a hairline, the played part in the signal, a head.
        let tw = (x1 - x0).max(px(30.0));
        let th = px(3.0);
        let track = Rect::new(x0, foot - th / 2.0, tw, th);
        scene.rect(track, a(paper, 0.3));
        let played = v.filter(|v| v.dur > 0.0).map(|v| (v.t / v.dur).clamp(0.0, 1.0) as f32).unwrap_or(0.0);
        scene.rect(Rect::new(track.x, track.y, tw * played, th), a(self.surface.signal, 1.0));
        let head = px(9.0);
        scene.rect(Rect::new(track.x + tw * played - head / 2.0, foot - head / 2.0, head, head), a(paper, 1.0));
        let seekable=v.is_some_and(|v|v.dur.is_finite() && v.dur>0.0);
        if let Some(p)=self.pip.as_mut(){p.track=seekable.then_some(track);}
        if seekable {hits.push((Rect::new(track.x,foot-px(10.0),track.w,px(20.0)),Hit::Track));}
        // A visible grip makes the border resize affordance discoverable.
        for d in [4.0,8.0,12.0] {scene.rect(Rect::new(r.right()-px(d),r.bottom()-px(3.0),px(2.0),px(2.0)),a(paper,0.7));}
        hits
    }

    // --- PiP window events -------------------------------------------------

    pub fn pip_focus(&mut self, focused: bool) {
        if let Some(p) = self.pip.as_mut() {
            p.focused = focused;
            if !focused {p.pressed=false;p.dragging=false;p.gesture=None;p.scrubbing=false;p.press_hit=None;p.key_focus=None;p.mods=winit::keyboard::ModifiersState::empty();}
        }
    }

    pub fn pip_resized(&mut self, w: u32, h: u32) {
        if let Some(p) = self.pip.as_mut() {
            if w==0 || h==0 {return;}
            p.target.resize(&self.gpu.device, w, h);
            p.constrain_native_size(w,h);
        }
    }

    pub fn pip_key(&mut self, ev: &crate::app::KeyIn) {
        if ev.state != ElementState::Pressed {
            return;
        }
        let Some(pip) = self.pip.as_ref() else { return };
        if !pip.focused {
            return;
        }
        if ev.logical_key==WKey::Named(NamedKey::Tab) {
            let order:Vec<Hit>=[Hit::Play,Hit::Back,Hit::Forward,Hit::Mute,Hit::Track,Hit::Smaller,Hit::Larger,Hit::ToTab,Hit::Close].into_iter().filter(|h|!matches!(h,Hit::Track) || pip.track.is_some()).collect();
            let back=pip.mods.shift_key();let n=order.len();
            let next=pip.key_focus.and_then(|h|order.iter().position(|v|*v==h)).map(|i|if back {(i+n-1)%n}else{(i+1)%n}).unwrap_or(if back{n-1}else{0});
            let p=self.pip.as_mut().unwrap();p.key_focus=Some(order[next]);p.window.request_redraw();return;
        }
        if ev.logical_key==WKey::Named(NamedKey::Enter) || ev.logical_key==WKey::Named(NamedKey::Space) {
            if let Some(hit)=pip.key_focus {self.pip_act(hit);return;}
        }
        let (tab, right) = (pip.tab, pip.right);
        let back=format!("__nus.seek(-{})",self.behavior.pip_skip_seconds.clamp(1,120));
        let forward=format!("__nus.seek({})",self.behavior.pip_skip_seconds.clamp(1,120));
        let cmd = match &ev.logical_key {
            WKey::Named(NamedKey::ArrowLeft) => back.as_str(),
            WKey::Named(NamedKey::ArrowRight) => forward.as_str(),
            WKey::Named(NamedKey::ArrowUp) => "__nus.vol(0.1)",
            WKey::Named(NamedKey::ArrowDown) => "__nus.vol(-0.1)",
            WKey::Named(NamedKey::Space) => "__nus.toggle()",
            WKey::Named(NamedKey::Escape) => {
                self.return_from_pip();
                return;
            }
            WKey::Character(c) => match c.to_lowercase().as_str() {
                "k" => "__nus.toggle()",
                "j" => back.as_str(),
                "l" => forward.as_str(),
                "," => "__nus.step(-1)",
                "." => "__nus.step(1)",
                "m" => "__nus.mute()",
                "+" | "=" => {self.pip_zoom(1.12);return;},
                "-" => {self.pip_zoom(1.0/1.12);return;},
                _ => return,
            },
            _ => return,
        };
        if let Some(t) = self.web_tab(tab, right) {
            t.eval(cmd);
        }
    }

    pub fn pip_mouse(&mut self, button: MouseButton, state: ElementState) {
        if button!=MouseButton::Left {return;}
        let Some(p)=self.pip.as_mut() else {return;};
        match state {
            ElementState::Pressed=>{
                p.window.focus_window();p.focused=true;p.key_focus=None;
                let scale=p.window.scale_factor();
                let (x,y)=p.pos;let edge=p.edge_at(x,y);
                let pos=p.window.outer_position().map(|p|(p.x as f64/scale,p.y as f64/scale)).unwrap_or((p.cur.x,p.cur.y));
                p.press_origin=(pos.0+x as f64/scale,pos.1+y as f64/scale);
                p.area=native::work_area(&p.window);
                if edge!=(0,0) {
                    if crate::hatch_native::wayland(){use winit::window::ResizeDirection as D;let direction=match edge{(-1,-1)=>D::NorthWest,(1,-1)=>D::NorthEast,(-1,1)=>D::SouthWest,(1,1)=>D::SouthEast,(-1,0)=>D::West,(1,0)=>D::East,(0,-1)=>D::North,_=>D::South};let _=p.window.drag_resize_window(direction);return;}
                    p.gesture=Some((p.cur,p.press_origin,edge));p.pressed=true;return;}
                if let Some(hit)=p.hit_at(x,y) {
                    p.press_hit=Some(hit);
                    if hit==Hit::Track {p.scrubbing=true;let f=p.track_fraction(x);self.pip_seek_to(f);}
                } else {p.pressed=true;}
            }
            ElementState::Released=>{
                p.pressed=false;p.dragging=false;p.gesture=None;
                if let Some(hit)=p.press_hit.take() {
                    let scrubbed=std::mem::take(&mut p.scrubbing);
                    if !scrubbed && p.hit_at(p.pos.0,p.pos.1)==Some(hit) {self.pip_act(hit);}
                } else {p.apply_rect(geometry::fit(p.cur,p.area,16.0));}
            }
        }
    }

    pub fn pip_cursor_entered(&mut self) {
        if let Some(p) = self.pip.as_mut() {
            p.inside = true;
            p.left_at = None;
            p.window.request_redraw();
        }
    }

    /// The pointer left: the controls stay a beat, then go.
    pub fn pip_cursor_left(&mut self) {
        if let Some(p) = self.pip.as_mut() {
            p.inside = false;
            p.left_at = Some(crate::clock::now());
            if !p.pressed && !p.scrubbing {p.pos = (-1.0, -1.0);}
            p.window.request_redraw();
        }
    }

    /// `x`/`y` are the window's own pixels, the same the scene is drawn in.
    pub fn pip_cursor_moved(&mut self, x: f64, y: f64) {
        let Some(p) = self.pip.as_mut() else { return };
        p.pos = (x as f32, y as f32);
        p.inside = true;
        p.left_at = None;
        if p.scrubbing {
            let f = p.track_fraction(x as f32);
            self.pip_seek_to(f);
            return;
        }
        let scale=p.window.scale_factor();
        let pos=p.window.outer_position().map(|v|(v.x as f64/scale,v.y as f64/scale)).unwrap_or((p.cur.x,p.cur.y));
        let global=(pos.0+x/scale,pos.1+y/scale);
        if let Some((start,origin,edge))=p.gesture {
            p.apply_rect(geometry::resize(start,(global.0-origin.0,global.1-origin.1),edge,p.area));
            return;
        }
        if p.pressed {
            let delta=(global.0-p.press_origin.0,global.1-p.press_origin.1);
            if p.dragging || delta.0.hypot(delta.1)>3.0 {
                p.dragging=true;
                if crate::hatch_native::wayland() {let _=p.window.drag_window();p.pressed=false;p.dragging=false;return;}
                let next=geometry::fit(LRect{x:p.cur.x+delta.0,y:p.cur.y+delta.1,..p.cur},p.area,16.0);
                p.press_origin=global;p.apply_rect(next);
            }
            return;
        }
        let edge=p.edge_at(x as f32,y as f32);
        let cursor=match edge {(-1,-1)|(1,1)=>winit::window::CursorIcon::NwseResize,(1,-1)|(-1,1)=>winit::window::CursorIcon::NeswResize,(_,0) if edge.0!=0=>winit::window::CursorIcon::EwResize,(0,_) if edge.1!=0=>winit::window::CursorIcon::NsResize,_=>winit::window::CursorIcon::Default};
        p.window.set_cursor(cursor);
        // The marks light under the pointer, so it has to redraw.
        p.window.request_redraw();
    }

    /// Seek to a fraction of the run time.
    fn pip_seek_to(&mut self, f: f32) {
        let Some(pip) = self.pip.as_ref() else { return };
        let (tab, right) = (pip.tab, pip.right);
        if let Some(t) = self.web_tab(tab, right) {
            t.eval(&format!("__nus.seekTo({f})"));
        }
        if let Some(p) = self.pip.as_ref() {
            p.window.request_redraw();
        }
    }

    /// What a control does. The keys call the same things.
    fn pip_act(&mut self, hit: Hit) {
        let Some(pip) = self.pip.as_ref() else { return };
        let (tab, right) = (pip.tab, pip.right);
        let back=format!("__nus.seek(-{})",self.behavior.pip_skip_seconds.clamp(1,120));
        let forward=format!("__nus.seek({})",self.behavior.pip_skip_seconds.clamp(1,120));
        let cmd = match hit {
            Hit::Play => "__nus.toggle()",
            Hit::Back => back.as_str(),
            Hit::Forward => forward.as_str(),
            Hit::Mute => "__nus.mute()",
            Hit::ToTab => return self.return_from_pip(),
            Hit::Close => return self.close_pip(),
            Hit::Smaller => return self.pip_zoom(1.0/1.12),
            Hit::Larger => return self.pip_zoom(1.12),
            // Handled on the press, and followed by the drag.
            Hit::Track => return,
        };
        if let Some(t) = self.web_tab(tab, right) {
            t.eval(cmd);
        }
        if let Some(p) = self.pip.as_ref() {
            p.window.request_redraw();
        }
    }

    /// Drag ended by the OS (the window moved): keep `cur` honest.
    pub fn pip_moved(&mut self, x: i32, y: i32) {
        if let Some(p) = self.pip.as_mut() {
            if p.dragging {
                let scale = p.window.scale_factor();
                p.cur.x = x as f64 / scale;
                p.cur.y = y as f64 / scale;
            }
        }
    }

    pub fn pip_wheel(&mut self, delta: MouseScrollDelta) {
        let dy=match delta {MouseScrollDelta::LineDelta(_,y)=>y as f64*0.08,MouseScrollDelta::PixelDelta(p)=>p.y*0.002};
        if dy==0.0 || !dy.is_finite() {return;}
        self.pip_zoom(dy.clamp(-0.5,0.5).exp());
    }

    pub fn pip_pinch(&mut self,delta:f64) {
        if delta==0.0 || !delta.is_finite() {return;}
        self.pip_zoom((1.0+delta).clamp(0.5,2.0));
    }

    fn pip_zoom(&mut self,factor:f64) {
        let Some(p)=self.pip.as_mut() else {return;};
        if p.scrubbing || p.pressed {return;}
        p.area=native::work_area(&p.window);
        p.apply_rect(geometry::zoom(p.cur,factor,p.area,(0.5,0.5)));
    }

    pub fn pip_scale_changed(&mut self) {
        if let Some(p)=self.pip.as_mut() {
            let scale=p.window.scale_factor();let size=p.window.inner_size();
            let pos=p.window.outer_position().ok();
            let rect=LRect{x:pos.map(|v|v.x as f64/scale).unwrap_or(p.cur.x),y:pos.map(|v|v.y as f64/scale).unwrap_or(p.cur.y),w:size.width as f64/scale,h:size.height as f64/scale};
            p.area=native::work_area(&p.window);p.cur=rect;p.apply_rect(geometry::with_aspect(rect,p.aspect,p.area));
            p.target.resize(&self.gpu.device,size.width.max(1),size.height.max(1));
        }
    }

    pub fn pip_place(&mut self, previous:LRect) {
        if let Some(p)=self.pip.as_mut() {
            let rect=LRect{h:previous.w/p.aspect,..previous};
            p.apply_rect(geometry::fit(rect,p.area,16.0));
        }
    }

    /// Bring the video's tab back and close PiP.
    pub fn return_from_pip(&mut self) {
        if let Some(p) = self.pip.take() {
            let tab = p.tab;
            drop(p);
            self.hatch_state.main_hidden=false;
            self.window.set_visible(true);self.window.set_minimized(false);
            self.window.focus_window();
            self.activate(tab);
        }
    }
}

impl Hit {
    fn label(self)-> &'static str {match self {
        Self::Play=>"Play or pause",Self::Back=>"Back ten seconds",Self::Forward=>"Forward ten seconds",Self::Mute=>"Mute or unmute",Self::ToTab=>"Return to tab",Self::Close=>"Close picture in picture",Self::Smaller=>"Make smaller",Self::Larger=>"Make larger",Self::Track=>"Playback position",
    }}
}
impl App {
    pub(crate) fn pip_access_tree(&self)->accesskit::TreeUpdate {
        use accesskit::{Action,Node,NodeId,Role,TreeInfo,TreeId,TreeUpdate};
        let mut nodes=Vec::new();let mut children=Vec::new();let mut focus=NodeId(1);
        if let Some(p)=&self.pip {
            for hit in [Hit::Play,Hit::Back,Hit::Forward,Hit::Mute,Hit::Smaller,Hit::Larger,Hit::ToTab,Hit::Close,Hit::Track] {
                if matches!(hit,Hit::Track) && p.track.is_none(){continue;}
                let id=NodeId(10+hit as u64);let mut n=Node::new(if hit==Hit::Track {Role::Slider}else{Role::Button});n.set_label(match hit {Hit::Back=>format!("Back {} seconds",self.behavior.pip_skip_seconds.clamp(1,120)),Hit::Forward=>format!("Forward {} seconds",self.behavior.pip_skip_seconds.clamp(1,120)),_=>hit.label().into()});n.add_action(Action::Focus);
                if hit==Hit::Track {n.add_action(Action::Increment);n.add_action(Action::Decrement);n.add_action(Action::SetValue);n.set_min_numeric_value(0.0);n.set_max_numeric_value(100.0);if let Some(v)=self.pane_video(p.tab,p.right){n.set_numeric_value((v.t/v.dur*100.0).clamp(0.0,100.0));}}
                else {n.add_action(Action::Click);}
                if let Some((r,_))=p.hits.iter().find(|(_,h)|*h==hit){n.set_bounds(accesskit::Rect{x0:r.x as f64,y0:r.y as f64,x1:r.right()as f64,y1:r.bottom()as f64});}
                if p.key_focus==Some(hit){focus=id;}children.push(id);nodes.push((id,n));
            }
        }
        let mut root=Node::new(Role::Window);root.set_label("Picture in picture");root.set_children(children);nodes.push((NodeId(1),root));
        TreeUpdate{nodes,tree:Some(TreeInfo::new(NodeId(1))),tree_id:TreeId::ROOT,focus}
    }
    pub(crate) fn pip_access_action(&mut self,req:accesskit::ActionRequest) {
        use accesskit::{Action,ActionData};
        let Some(hit)=[Hit::Play,Hit::Back,Hit::Forward,Hit::Mute,Hit::Smaller,Hit::Larger,Hit::ToTab,Hit::Close,Hit::Track].into_iter().find(|h|10+*h as u64==req.target_node.0) else {return;};
        match req.action {
            Action::Click=>self.pip_act(hit),
            Action::Focus=>{if let Some(p)=&mut self.pip {p.key_focus=Some(hit);p.window.focus_window();p.window.request_redraw();}},
            Action::Increment if hit==Hit::Track=>self.pip_act(Hit::Forward),
            Action::Decrement if hit==Hit::Track=>self.pip_act(Hit::Back),
            Action::SetValue if hit==Hit::Track=>{if let Some(ActionData::NumericValue(v))=req.data{if v.is_finite(){self.pip_seek_to((v/100.0).clamp(0.0,1.0)as f32);}}},
            _=>{}
        }
    }
}
