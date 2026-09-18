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
        Splash { started: Instant::now(), begun: false, tex: None, fade: Anim::at(1.0), leaving: false }
    }
}

impl App {
    /// The birth tab — the first window's shell, split or not — gives way
    /// to `pane` alone, when it is still the shell it was born with.
    pub(crate) fn replace_birth(&mut self, pane: crate::app::Pane) {
        if let Some(t) = self.tabs.first_mut() {
            if matches!(t.left, crate::app::Pane::Term(_)) {
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
            sp.started = Instant::now();
        }
        let elapsed = sp.started.elapsed().as_secs_f32();
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
