//! Settings · Profile as a map of where the profile lives: you at the
//! centre; this device, the profile folder and the encryption key on the
//! inner orbit (on this machine, always); the synced folder and the private
//! repository on the outer one, reached only through the key. A line is
//! solid where something is set up and dashed where it could be; set-up
//! sync lines carry a slow tick of motion, so it reads as live. Every card
//! is the control for what it shows.
use crate::app::{fade, hover_key, App};
use crate::settings::Hit;
use nus_render::{Instance, Rect, Scene, Style};

type P = (f32, f32);

fn at(c: P, r: f32, deg: f32) -> P {
    let a = deg.to_radians();
    (c.0 + r * a.cos(), c.1 + r * a.sin())
}

/// A straight line `w` wide from `a` to `b`.
fn line(scene: &mut Scene, a: P, b: P, w: f32, color: nus_render::Color) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (nx, ny) = (-dy / len * w * 0.5, dx / len * w * 0.5);
    scene.push(Instance::quad([[a.0 + nx, a.1 + ny], [b.0 + nx, b.1 + ny], [b.0 - nx, b.1 - ny], [a.0 - nx, a.1 - ny]], color));
}

/// Dashes along `a`→`b`, shifted by `phase` (0..1 of one dash+gap).
#[allow(clippy::too_many_arguments)]
fn dashed(scene: &mut Scene, a: P, b: P, w: f32, on: f32, off: f32, phase: f32, color: nus_render::Color) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let step = on + off;
    let mut s = -step + (phase.fract() * step);
    while s < len {
        let s0 = s.max(0.0);
        let s1 = (s + on).min(len);
        if s1 > s0 {
            line(scene, (a.0 + ux * s0, a.1 + uy * s0), (a.0 + ux * s1, a.1 + uy * s1), w, color);
        }
        s += step;
    }
}

/// A ring of short arcs (straight chords at this size).
fn dashed_ring(scene: &mut Scene, c: P, r: f32, w: f32, n: usize, color: nus_render::Color) {
    for k in 0..n {
        let a0 = k as f32 / n as f32 * 360.0;
        let a1 = a0 + 360.0 / n as f32 * 0.45;
        line(scene, at(c, r, a0), at(c, r, a1), w, color);
    }
}

struct Node {
    at: P,
    label: &'static str,
    value: String,
    set: bool,
    hit: Hit,
    tip: String,
}

impl App {
    pub(crate) fn profile_orbit_height(&self, w: f32) -> f32 {
        if w >= self.px(620.0) { self.px(430.0) } else { self.px(520.0) }
    }

    pub(crate) fn draw_profile_orbit(&mut self, scene: &mut Scene, r: Rect) {
        let t = self.theme.clone();
        let ink = t.ink;
        let signal = self.surface.signal;
        let label = self.label();
        let small = Style { px: self.px(9.0), color: t.dim, tracking: 0.08, ..label };
        let wide = r.w >= self.px(620.0);
        let c = (r.x + r.w * if wide { 0.40 } else { 0.5 }, r.y + r.h * if wide { 0.50 } else { 0.46 });
        let outer = (r.h * 0.43).min(r.w * if wide { 0.40 } else { 0.44 });
        let inner = outer * 0.56;
        let hair = self.px(1.0);

        // The orbits: inner is this machine, outer is optional.
        scene.push(Instance::stroke(Rect::new(c.0 - inner, c.1 - inner, inner * 2.0, inner * 2.0), inner, hair, fade(ink, 0.35), None, 0.0));
        dashed_ring(scene, c, outer, hair, 72, fade(ink, 0.3));

        let b = &self.behavior;
        let key = crate::syncui::key().is_some();
        let folder = !b.sync_folder.is_empty();
        let repo = !b.sync_git.is_empty();
        let (name, face, days) = match &self.me {
            Some(me) => (me.name.clone(), me.face.clone(), me.day_word()),
            None => (crate::me::os_user(), crate::me::Face::Initial, "not set up".into()),
        };
        let short = |s: &str| crate::app::fit_cmd(s, 22);
        let nodes = [
            Node { at: at(c, inner, 270.0), label: "THIS DEVICE", value: crate::me::device(), set: true, hit: Hit::MeEdit(2), tip: "Rename this device · it marks the changes it syncs".into() },
            Node { at: at(c, inner, 150.0), label: "PROFILE/", value: "settings · rules · journal".into(), set: true, hit: Hit::MeFolder, tip: "Open the profile folder · everything nus keeps is in it".into() },
            Node { at: at(c, inner, 30.0), label: "YOUR KEY", value: if key { "on this device".into() } else { "none yet".into() }, set: key, hit: Hit::SyncKey, tip: if key { "Show and copy your key · paste it on another device".into() } else { "Create the key that seals what leaves this device".into() } },
            Node { at: at(c, outer, if wide { 340.0 } else { 320.0 }), label: "SYNCED FOLDER", value: if folder { short(&b.sync_folder) } else { "connect →".into() }, set: folder, hit: Hit::SyncEdit(0), tip: "A folder your system already syncs: iCloud Drive, Dropbox, Syncthing".into() },
            Node { at: at(c, outer, if wide { 62.0 } else { 70.0 }), label: "PRIVATE REPO", value: if repo { short(&b.sync_git) } else { "connect →".into() }, set: repo, hit: Hit::SyncEdit(1), tip: "A private git repository: GitHub, GitLab, Forgejo or Gitea".into() },
        ];

        // Lines: you to the inner three; the key out to each destination.
        let moving = !self.motion.reduced() && (folder || repo) && key;
        let phase = if moving { crate::clock::since(self.started).as_secs_f32() * 0.35 } else { 0.0 };
        let w = self.px(2.0);
        for n in &nodes[..3] {
            if n.set {
                line(scene, c, n.at, w, ink);
            } else {
                dashed(scene, c, n.at, w, self.px(6.0), self.px(6.0), 0.0, fade(ink, 0.45));
            }
        }
        for n in &nodes[3..] {
            if n.set && key {
                line(scene, nodes[2].at, n.at, w, fade(signal, 0.35));
                dashed(scene, nodes[2].at, n.at, w, self.px(4.0), self.px(14.0), phase, signal);
            } else {
                dashed(scene, nodes[2].at, n.at, hair * 1.5, self.px(6.0), self.px(6.0), 0.0, fade(ink, 0.35));
            }
        }
        if moving {
            self.dirty = true;
        }

        // You: the face, square as everything in nus, on a hard shadow.
        let fs = self.px(if wide { 92.0 } else { 80.0 });
        let fr = Rect::new(c.0 - fs / 2.0, c.1 - fs / 2.0 - self.px(10.0), fs, fs);
        let hot = fr.contains(self.mouse.0, self.mouse.1);
        let lift = if hot { self.px(2.0) } else { 0.0 };
        let fr = Rect::new(fr.x - lift, fr.y - lift, fr.w, fr.h);
        scene.rect(Rect::new(fr.x + self.px(5.0) + lift, fr.y + self.px(5.0) + lift, fr.w, fr.h), ink);
        scene.rect(fr, t.paper);
        self.draw_face(scene, fr, &face, &name);
        scene.outline(fr, self.px(2.0), ink);
        let face_hit = if self.me.is_some() { Hit::MeEdit(1) } else { Hit::MeCard };
        self.settings_hits.push((fr, face_hit));
        self.offer_tip(hover_key("orbit-face", 0), fr, if self.me.is_some() { "Change your face · a picture, an initial or an emoji".into() } else { "Set up your profile · a name and a face, kept on this device".into() });
        let serif = Style { font: self.f.serif, px: self.px(22.0), color: ink, tracking: 0.0 };
        let nw = self.fonts.measure(serif, &name);
        self.fonts.draw(scene, serif, c.0 - nw / 2.0, fr.bottom() + self.px(30.0), &name);
        let dw = self.fonts.measure(small, &days);
        self.fonts.draw(scene, small, c.0 - dw / 2.0, fr.bottom() + self.px(46.0), &days);

        // The cards: each is the control for what it shows.
        let value = Style { px: self.px(11.0), color: ink, ..label };
        for (i, n) in nodes.iter().enumerate() {
            let vw = self.fonts.measure(value, &n.value).max(self.fonts.measure(small, n.label));
            let cw = vw + self.px(24.0);
            let ch = self.px(44.0);
            let mut cr = Rect::new(n.at.0 - cw / 2.0, n.at.1 - ch / 2.0, cw, ch);
            cr.x = cr.x.clamp(r.x, r.right() - cw);
            cr.y = cr.y.clamp(r.y, r.bottom() - ch);
            let hot = cr.contains(self.mouse.0, self.mouse.1);
            let lift = if hot { self.px(2.0) } else { 0.0 };
            let card = Rect::new(cr.x - lift, cr.y - lift, cr.w, cr.h);
            if n.set {
                scene.rect(Rect::new(card.x + self.px(3.0) + lift, card.y + self.px(3.0) + lift, card.w, card.h), ink);
                scene.rect(card, t.paper);
                scene.outline(card, self.px(2.0), ink);
            } else {
                scene.rect(card, t.paper);
                scene.push(Instance::hazard(card, hair, fade(ink, 0.5), [0.0, 0.0, 0.0, 0.0], self.px(6.0)));
                scene.outline(card.inset(hair * 2.0), hair, fade(ink, 0.5));
                if hot {
                    scene.outline(card, self.px(2.0), ink);
                }
            }
            self.fonts.draw(scene, small, card.x + self.px(12.0), card.y + self.px(17.0), n.label);
            let vc = if n.set { ink } else if hot { signal } else { t.dim };
            self.fonts.draw(scene, Style { color: vc, ..value }, card.x + self.px(12.0), card.y + self.px(33.0), &n.value);
            self.settings_hits.push((cr, n.hit));
            self.offer_tip(hover_key("orbit-node", i), cr, n.tip.clone());
        }

        // What the lines mean, and what never goes anywhere.
        let ly = r.bottom() - self.px(10.0);
        let x0 = r.x;
        line(scene, (x0, ly - self.px(4.0)), (x0 + self.px(22.0), ly - self.px(4.0)), w, ink);
        let mut x = x0 + self.px(30.0);
        x += self.fonts.draw(scene, small, x, ly, "ON THIS DEVICE");
        x += self.px(22.0);
        dashed(scene, (x, ly - self.px(4.0)), (x + self.px(22.0), ly - self.px(4.0)), w, self.px(5.0), self.px(4.0), 0.0, fade(ink, 0.5));
        x += self.px(30.0);
        self.fonts.draw(scene, small, x, ly, "OPTIONAL · SEALED WITH YOUR KEY FIRST");
        let never = "NEVER LEAVES: COOKIES · CACHES · DOWNLOADS · SHELL HISTORY";
        let nw = self.fonts.measure(small, never);
        if wide {
            self.fonts.draw(scene, small, r.right() - nw, r.y + self.px(12.0), never);
        } else {
            self.fonts.draw(scene, small, x0, ly - self.px(18.0), never);
        }
    }
}
