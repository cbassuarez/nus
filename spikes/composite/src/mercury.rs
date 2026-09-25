//! A local commemorative entitlement. It is not a worldwide user counter or
//! a server-enforced scarcity mechanism. Once earned, eligibility stays earned.
use nus_render::{Rect, Scene, text::Style};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
pub const CUTOFF: u64 = 1_798_761_600; // 2027-01-01 00:00:00 UTC, exclusive.
#[derive(Clone, Serialize, Deserialize)]
pub struct Claim {
    pub claimed_at: u64,
    pub version: String,
}
static CLAIM: LazyLock<Mutex<Option<Claim>>> = LazyLock::new(|| Mutex::new(load()));
// Process-wide so a claim/replay in any window reaches the Dock owner once.
static PRESENTATION_REVISION: AtomicU64 = AtomicU64::new(0);
pub(crate) fn presentation_revision() -> u64 {
    PRESENTATION_REVISION.load(Ordering::Relaxed)
}

fn path() -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("profile/mercury.json")
}
pub fn eligible(at: u64, version: &str) -> bool {
    at < CUTOFF
        && semver::Version::parse(version.trim_start_matches('v'))
            .is_ok_and(|v| v < semver::Version::new(1, 0, 0))
}
fn load() -> Option<Claim> {
    let claim: Claim = serde_json::from_slice(&std::fs::read(path()).ok()?).ok()?;
    eligible(claim.claimed_at, &claim.version).then_some(claim)
}
pub fn earned() -> bool {
    CLAIM.lock().unwrap().is_some()
}
pub fn observe_installation() {
    if crate::private::enabled() {
        return;
    }
    let _ = record_closure(
        &closed_path(),
        crate::journal::now(),
        crate::updates::CURRENT,
    );
}
fn record_closure(path: &std::path::Path, at: u64, version: &str) -> std::io::Result<()> {
    if !eligible(at, version) && !path.exists() {
        crate::store::write_json(
            path,
            &Claim {
                claimed_at: at,
                version: version.into(),
            },
        )?;
    }
    Ok(())
}
fn closed_path() -> std::path::PathBuf {
    path().with_file_name("mercury-closed.json")
}
pub fn can_claim() -> bool {
    observe_installation();
    !crate::private::enabled()
        && !earned()
        && !closed_path().exists()
        && eligible(crate::journal::now(), crate::updates::CURRENT)
}
pub fn label() -> String {
    let claim = CLAIM.lock().unwrap().clone();
    if let Some(c) = claim {
        format!("Earned {}", date(c.claimed_at))
    } else if can_claim() {
        "Claim Mercury".into()
    } else {
        if crate::private::enabled() {
            "Unavailable in private mode"
        } else {
            "Claim window closed"
        }
        .into()
    }
}
pub fn date(at: u64) -> String {
    let days = at / 86400;
    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", y + if m <= 2 { 1 } else { 0 })
}
pub fn claim() -> Result<(), String> {
    if earned() {
        return Ok(());
    }
    let at = crate::journal::now();
    if !can_claim() {
        return Err("The Mercury claim window has closed".into());
    }
    let value = Claim {
        claimed_at: at,
        version: crate::updates::CURRENT.into(),
    };
    crate::store::write_json(&path(), &value).map_err(|e| e.to_string())?;
    *CLAIM.lock().unwrap() = Some(value);
    Ok(())
}
/// The selected liquid-metal study, embedded so every platform uses the same
/// art. Resize in premultiplied space: transparent texels cannot darken silver
/// edges at small Dock sizes. The public icon remains straight-alpha RGBA.
static ART: LazyLock<image::RgbaImage> = LazyLock::new(|| {
    image::load_from_memory(include_bytes!("../../../assets/icon/mercury/tidal.png"))
        .expect("embedded Mercury artwork")
        .to_rgba8()
});
pub fn icon(size: u32) -> Vec<u8> {
    if size == 0 {
        return Vec::new();
    }
    let source = image::Rgba32FImage::from_fn(ART.width(), ART.height(), |x, y| {
        let p = ART.get_pixel(x, y).0;
        let a = p[3] as f32 / 255.0;
        image::Rgba([
            p[0] as f32 / 255.0 * a,
            p[1] as f32 / 255.0 * a,
            p[2] as f32 / 255.0 * a,
            a,
        ])
    });
    let resized =
        image::imageops::resize(&source, size, size, image::imageops::FilterType::Lanczos3);
    resized
        .pixels()
        .flat_map(|p| {
            let a = p[3].clamp(0.0, 1.0);
            let channel = |k: usize| {
                if a > 0.001 {
                    (p[k] / a * 255.0).clamp(0.0, 255.0).round() as u8
                } else {
                    0
                }
            };
            [
                channel(0),
                channel(1),
                channel(2),
                (a * 255.0).round() as u8,
            ]
        })
        .collect()
}

static LIQUID_EPOCH: LazyLock<Instant> = LazyLock::new(crate::clock::now);
fn liquid_seconds() -> f32 {
    crate::clock::since(*LIQUID_EPOCH).as_secs_f32()
}

pub struct Reveal {
    started: Instant,
    texture: Arc<wgpu::BindGroup>,
    pub keyboard_focus: bool,
    trail: Vec<[f32; 3]>,
    last_mouse: (f32, f32),
}

const PAPER: [f32; 4] = [0.976, 0.972, 0.963, 1.0];
const INK: [f32; 4] = [0.13, 0.14, 0.15, 1.0];
const DIM: [f32; 4] = [0.40, 0.41, 0.42, 1.0];

/// Physical pixels; one composition shared by the renderer and bounds checks.
struct Layout {
    scale: f32,
    art: Rect,
    title: f32,
    title_px: f32,
    button: Rect,
}
impl Layout {
    fn new(w: f32, h: f32, scale: f32) -> Self {
        let s = scale.min(w / 320.0).min(h / 360.0);
        let title_px = (h * 0.12).clamp(32.0 * s, 60.0 * s);
        // Lay out the artwork's visible alpha extent (18%..84%), not its
        // transparent square, and reserve the heading's ascent above baseline.
        let size = (w * 0.88)
            .min(((h - title_px - 210.0 * s) / 0.66).max(64.0 * s))
            .min(590.0 * s);
        let used = size * 0.66 + title_px + 210.0 * s;
        let top = (((h - used) * 0.5).max(0.0) + 56.0 * s - size * 0.18).max(8.0 * s);
        let title = top + size * 0.84 + title_px * 0.82 + 18.0 * s;
        Self {
            scale: s,
            art: Rect::new((w - size) * 0.5, top, size, size),
            title,
            title_px,
            button: Rect::new(w * 0.5 - 90.0 * s, title + 76.0 * s, 180.0 * s, 44.0 * s),
        }
    }
}
fn ease(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v * (3.0 - 2.0 * v)
}

impl crate::app::App {
    /// The modal replaces the underlying accessibility tree as well as the
    /// pixels and input targets, so hidden Settings actions cannot be invoked.
    pub(crate) fn mercury_access_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        self.me_card.mercury_reveal.as_ref()?;
        let mut children = Vec::new();
        let mut nodes = Vec::new();
        let mut focus = NodeId(1);
        self.access_map.clear();
        for (i, (rect, hit)) in self.me_card.hits.iter().enumerate() {
            let id = NodeId(100 + i as u64);
            let mut node = Node::new(Role::Button);
            node.set_label("Continue");
            node.set_bounds(accesskit::Rect {
                x0: rect.x as f64,
                y0: rect.y as f64,
                x1: rect.right() as f64,
                y1: rect.bottom() as f64,
            });
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
            self.access_map
                .insert(id.0, crate::access::Target::Mercury(*hit));
            focus = id;
            nodes.push((id, node));
            children.push(id);
        }
        let mut root = Node::new(Role::Dialog);
        root.set_modal();
        root.set_label("Mercury. You were here at the beginning. Your silver n is yours to keep.");
        root.set_children(children);
        nodes.push((NodeId(1), root));
        Some(TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus,
        })
    }

    pub(crate) fn mercury_texture(&mut self) -> Arc<wgpu::BindGroup> {
        if let Some(texture) = &self.me_card.mercury_art {
            return texture.clone();
        }
        // Keep the full material study on the GPU; do not run a large CPU
        // resample on the UI thread just to open Settings.
        let texture = self.bind_rgba(ART.as_raw(), ART.width(), ART.height());
        self.me_card.mercury_art = Some(texture.clone());
        texture
    }

    pub(crate) fn claim_mercury(&mut self) {
        if let Err(e) = claim() {
            self.notice_problem("Could Not Claim", e);
            return;
        }
        let texture = self.mercury_texture();
        // This uses the modal input layer, without opening the profile/setup walk.
        self.me_card.open = true;
        self.me_card.mercury_reveal = Some(Reveal {
            started: crate::clock::now(),
            texture,
            keyboard_focus: false,
            trail: Vec::new(),
            last_mouse: self.mouse,
        });
        PRESENTATION_REVISION.fetch_add(1, Ordering::Relaxed);
        self.refresh_icon();
        self.play_event("mercury.claim");
        self.dirty = true;
    }

    pub(crate) fn mercury_action(&mut self, hit: crate::me::CardHit) {
        use crate::me::CardHit;
        match hit {
            CardHit::MercuryDone => {
                self.close_me_card();
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub(crate) fn mercury_key(&mut self, key: &winit::keyboard::Key) {
        use crate::me::CardHit;
        use winit::keyboard::{Key, NamedKey};
        match key {
            Key::Named(NamedKey::Escape) => self.mercury_action(CardHit::MercuryDone),
            Key::Named(NamedKey::Tab) => {
                if let Some(r) = &mut self.me_card.mercury_reveal {
                    r.keyboard_focus = true;
                }
                self.dirty = true;
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                self.mercury_action(CardHit::MercuryDone);
            }
            _ => {}
        }
    }

    pub(crate) fn mercury_settings_height(&self, width: f32) -> f32 {
        self.px(if width >= self.px(480.0) {
            316.0
        } else {
            406.0
        })
    }

    /// Settings owns the invitation. The metal stays alive while visible;
    /// opening the page never claims an award or starts the launch morph.
    pub(crate) fn draw_mercury_settings(&mut self, scene: &mut Scene, rect: Rect) {
        let outer = scene.clip();
        let clip = outer.map_or(rect, |c| c.intersect(&rect));
        if clip.w <= 0.0 || clip.h <= 0.0 {
            return;
        }
        let texture = self.mercury_texture();
        scene.layer(Some(clip));
        scene.push(nus_render::Instance::rounded(rect, self.px(14.0), PAPER));
        let wide = rect.w >= self.px(480.0);
        let size = if wide {
            (rect.w * 0.48).min(self.px(308.0))
        } else {
            self.px(230.0).min(rect.w * 0.85)
        };
        let art = Rect::new(
            if wide {
                rect.x + self.px(8.0)
            } else {
                rect.x + (rect.w - size) * 0.5
            },
            rect.y + self.px(8.0),
            size,
            size,
        );
        let seconds = if self.motion.reduced() {
            0.0
        } else {
            liquid_seconds()
        };
        scene.liquid_texture(
            art,
            texture,
            1.0,
            seconds,
            if self.motion.reduced() { 0.0 } else { 1.0 },
        );
        if !self.motion.reduced() {
            self.dirty = true;
        }
        scene.layer(Some(clip));
        let cx = if wide {
            rect.x + rect.w * 0.72
        } else {
            rect.x + rect.w * 0.5
        };
        let top = rect.y + self.px(if wide { 90.0 } else { 249.0 });
        let title = Style {
            font: self.f.wordmark,
            px: self.px(38.0),
            color: INK,
            tracking: -self.px(0.6),
        };
        self.mercury_centered(scene, title, cx, top, "Mercury");
        let detail = Style {
            color: DIM,
            px: self.px(12.0),
            ..self.ui()
        };
        let copy_width = if wide {
            rect.w * 0.5 - self.px(24.0)
        } else {
            rect.w - self.px(32.0)
        };
        let lines = crate::reader::wrap(
            &self.fonts,
            detail,
            "A silver n for being here early.",
            copy_width,
        );
        for (i, line) in lines.iter().enumerate() {
            self.mercury_centered(
                scene,
                detail,
                cx,
                top + self.px(26.0 + i as f32 * 19.0),
                line,
            );
        }
        let extra = lines.len().saturating_sub(1) as f32 * self.px(19.0);
        let status = if earned() {
            label()
        } else {
            "The first edition of nus.".into()
        };
        self.mercury_centered(scene, detail, cx, top + self.px(45.0) + extra, &status);
        let available = earned() || can_claim();
        let button = Rect::new(
            cx - self.px(84.0),
            top + self.px(68.0) + extra,
            self.px(168.0),
            self.px(40.0),
        );
        let text = if earned() {
            "Replay Mercury".into()
        } else if available {
            "Claim Mercury".into()
        } else {
            label()
        };
        if available {
            let ink = self.theme.ink;
            let hot = button.contains(self.mouse.0, self.mouse.1);
            let shadow = self.px(if hot { 4.0 } else { 3.0 });
            scene.rect(
                Rect::new(button.x + shadow, button.y + shadow, button.w, button.h),
                ink,
            );
            scene.rect(
                button,
                if hot {
                    self.theme.tint
                } else {
                    self.theme.paper
                },
            );
            scene.outline(button, self.px(nus_render::theme::metric::STRUCTURE), ink);
            let style = Style {
                color: ink,
                ..self.label_strong()
            };
            self.mercury_centered(
                scene,
                style,
                cx,
                button.y + button.h * 0.5 + self.px(4.0),
                &text,
            );
            self.settings_hits
                .push((button.intersect(&clip), crate::settings::Hit::Mercury));
        } else {
            self.mercury_centered(scene, detail, cx, button.y + self.px(25.0), &text);
        }
        scene.layer(outer);
    }

    pub(crate) fn draw_mercury(&mut self, scene: &mut Scene) -> bool {
        let reduced = self.motion.reduced();
        let (w, h) = (self.target.size.0 as f32, self.target.size.1 as f32);
        let Some(reveal) = &mut self.me_card.mercury_reveal else {
            return false;
        };
        let elapsed = crate::clock::since(reveal.started).as_secs_f32();
        let t = if reduced { 5.0 } else { elapsed };
        let seconds = if reduced { 5.0 } else { elapsed };
        let keyboard_focus = reveal.keyboard_focus;
        let texture = reveal.texture.clone();
        reveal.trail.retain(|p| elapsed - p[2] < 2.3);
        if !reduced
            && self.mouse != reveal.last_mouse
            && self.mouse.0 >= 0.0
            && self.mouse.1 >= 0.0
            && self.mouse.0 < w
            && self.mouse.1 < h
        {
            let sample = [self.mouse.0 / w, self.mouse.1 / h, elapsed];
            if reveal.trail.last().is_some_and(|p| elapsed - p[2] < 0.07) {
                if let Some(p) = reveal.trail.last_mut() {
                    p[0] = sample[0];
                    p[1] = sample[1];
                }
            } else {
                reveal.trail.push(sample);
            }
            if reveal.trail.len() > 28 {
                reveal.trail.remove(0);
            }
        }
        reveal.last_mouse = self.mouse;
        let trail: Vec<_> = if reduced {
            Vec::new()
        } else {
            reveal
                .trail
                .iter()
                .map(|p| [p[0], p[1], elapsed - p[2]])
                .collect()
        };
        let layout = Layout::new(w, h, self.scale);
        let px = |v: f32| v * layout.scale;
        let cx = w * 0.5;
        let appearance = ease((t - 0.15) / 1.35);
        let text_alpha = ease((t - 1.05) / 0.9);
        let fade = |mut color: [f32; 4], alpha: f32| {
            color[3] *= alpha;
            color
        };

        scene.layer(None);
        // Seamless white cyclorama: no horizon, frame, or visible gradient edge.
        scene.push(nus_render::Instance::rounded_stops(
            Rect::new(0.0, 0.0, w, h),
            0.0,
            &[
                [0.996, 0.994, 0.990, 1.0],
                PAPER,
                [0.955, 0.953, 0.948, 1.0],
            ],
            90.0,
            0.0,
            false,
        ));
        scene.soft_ellipse(
            Rect::new(cx - w * 0.46, h * 0.01, w * 0.92, h * 0.80),
            [1.0, 1.0, 1.0, 0.8],
        );

        // Full procedural star field. Ordered pixel geometry appears only
        // along the pointer's recent path, then dissolves back into the field.
        scene.mercury_field(
            Rect::new(0.0, 0.0, w, h),
            seconds,
            appearance,
            px(1.0),
            &trail,
        );

        let eyebrow = Style {
            color: fade(DIM, ease(t / 0.65)),
            px: px(10.0),
            tracking: px(2.6),
            ..self.label()
        };
        self.mercury_centered(
            scene,
            eyebrow,
            cx,
            (layout.art.y - px(3.0)).max(px(28.0)),
            "THE FIRST EDITION",
        );

        // Wide feathered contact shadow. It tightens as the silver settles.
        let ground = layout.art.y + layout.art.h * 0.827;
        scene.soft_ellipse(
            Rect::new(
                cx - layout.art.w * 0.44,
                ground - px(13.0),
                layout.art.w * 0.88,
                px(54.0),
            ),
            [0.28, 0.29, 0.30, 0.16 * appearance],
        );
        scene.soft_ellipse(
            Rect::new(
                cx - layout.art.w * 0.31,
                ground + px(2.0),
                layout.art.w * 0.62,
                px(18.0),
            ),
            [0.22, 0.23, 0.24, 0.11 * appearance],
        );

        let settle = 1.0 - ease(t / 2.1);
        let mut art = layout.art;
        art.y -= px(16.0) * settle;
        scene.liquid_texture(
            art,
            texture,
            appearance,
            seconds,
            if reduced { 0.0 } else { ease(t / 2.0) },
        );

        let title = Style {
            font: self.f.wordmark,
            px: layout.title_px.min(w * 0.16),
            color: fade(INK, text_alpha),
            tracking: -px(1.3),
        };
        self.mercury_centered(scene, title, cx, layout.title, "Mercury");
        let detail = Style {
            color: fade(DIM, text_alpha),
            px: px(14.0).min(w / 26.0),
            ..self.ui()
        };
        self.mercury_centered(
            scene,
            detail,
            cx,
            layout.title + px(32.0),
            "You were here at the beginning.",
        );
        self.mercury_centered(
            scene,
            detail,
            cx,
            layout.title + px(53.0),
            "Your silver n is yours to keep.",
        );

        self.me_card.rect = Rect::new(0.0, 0.0, w, h);
        self.me_card.hits.clear();
        // Continue is available from the first frame; the reveal never traps
        // the user behind an animation timer.
        let button = layout.button;
        let hovered = button.contains(self.mouse.0, self.mouse.1);
        scene.push(nus_render::Instance::rounded(
            button,
            button.h * 0.5,
            if hovered {
                [0.26, 0.27, 0.28, 1.0]
            } else {
                INK
            },
        ));
        let button_style = Style {
            color: [1.0; 4],
            px: px(13.0),
            ..self.ui_strong()
        };
        self.mercury_centered(scene, button_style, cx, button.y + px(27.0), "Continue");
        self.me_card
            .hits
            .push((button, crate::me::CardHit::MercuryDone));
        if keyboard_focus {
            let rect = button;
            scene.push(nus_render::Instance::stroke(
                rect.inset(-px(4.0)),
                rect.h * 0.5 + px(4.0),
                px(1.3),
                [0.36, 0.40, 0.45, 1.0],
                None,
                0.0,
            ));
        }
        if !reduced {
            self.dirty = true;
        }
        true
    }

    fn mercury_centered(
        &mut self,
        scene: &mut Scene,
        style: Style,
        cx: f32,
        baseline: f32,
        text: &str,
    ) {
        let width = self.fonts.measure(style, text);
        self.fonts
            .draw(scene, style, cx - width * 0.5, baseline, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_has_transparent_surround_and_readable_silver_at_dock_sizes() {
        for size in [16, 32, 64, 256] {
            let pixels = icon(size);
            assert_eq!(pixels.len(), (size * size * 4) as usize);
            let (mut coverage, mut dark, mut light) = (0, 0, 0);
            for p in pixels.chunks_exact(4) {
                if p[3] > 200 {
                    coverage += 1;
                    dark += usize::from(p[0] < 100);
                    light += usize::from(p[0] > 200);
                }
            }
            assert!(coverage > (size * size / 10) as usize);
            assert!(coverage < (size * size / 2) as usize);
            assert!(
                dark > 0 && light > 0,
                "silver lost its contrast at {size}px"
            );
            for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
                assert_eq!(pixels[((y * size + x) * 4 + 3) as usize], 0);
            }
        }
        assert!(icon(0).is_empty());
    }

    #[test]
    fn presentation_fits_short_narrow_and_retina_windows() {
        for (w, h) in [
            (240.0, 210.0),
            (240.0, 320.0),
            (320.0, 360.0),
            (480.0, 640.0),
            (640.0, 360.0),
            (1100.0, 900.0),
            (1600.0, 1000.0),
        ] {
            for scale in [1.0, 2.0] {
                let (w, h) = (w * scale, h * scale);
                let l = Layout::new(w, h, scale);
                for r in [l.art, l.button] {
                    assert!(
                        r.x >= 0.0 && r.y >= 0.0 && r.right() <= w && r.bottom() <= h,
                        "{w}x{h}: {r:?}"
                    );
                }
                assert!(l.title > l.art.y + l.art.h * 0.84);
                assert!(l.button.y > l.title + 53.0 * l.scale);
            }
        }
    }
    #[test]
    fn first_limit_closes_permanently_even_after_downgrade() {
        for (at, version) in [(CUTOFF - 1, "1.0.0"), (CUTOFF, "0.9.9")] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("closed.json");
            record_closure(&path, CUTOFF - 2, "0.9.9").unwrap();
            assert!(!path.exists());
            record_closure(&path, at, version).unwrap();
            let receipt = std::fs::read(&path).unwrap();
            record_closure(&path, CUTOFF - 2, "0.9.9").unwrap();
            assert_eq!(std::fs::read(path).unwrap(), receipt);
        }
    }
    #[test]
    fn claim_window_and_permanent_entitlement() {
        assert_eq!(date(CUTOFF), "2027-01-01");
        assert!(eligible(CUTOFF - 1, "0.9.9"));
        assert!(!eligible(CUTOFF - 1, "1.0.0"));
        assert!(!eligible(CUTOFF, "1.0.0"));
        assert!(!eligible(CUTOFF - 1, "1.0.1"));
        assert!(eligible(CUTOFF - 1, "0.0.1-preview.12"));
        assert!(!eligible(CUTOFF - 1, "nonsense"));
        let claim = Claim {
            claimed_at: CUTOFF - 1,
            version: "0.9.9".into(),
        };
        assert!(
            eligible(claim.claimed_at, &claim.version),
            "future current version never revokes the original receipt"
        );
    }
}
