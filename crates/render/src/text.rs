//! Fonts, shaping and the glyph atlas. Bundled faces (IBM Plex Mono,
//! Newsreader Italic) plus any system font by family name; every
//! (font, size) pair shares one R8 atlas.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use swash::FontRef;

use crate::scene::{Color, Instance, Scene};

pub const ATLAS_SIZE: u32 = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(u16);

pub mod bundled {
    pub const PLEX_MONO: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf");
    pub const PLEX_MONO_MEDIUM: &[u8] =
        include_bytes!("../../../assets/fonts/IBMPlexMono-Medium.ttf");
    pub const PLEX_MONO_SEMIBOLD: &[u8] =
        include_bytes!("../../../assets/fonts/IBMPlexMono-SemiBold.ttf");
    pub const PLEX_MONO_BOLD: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Bold.ttf");
    pub const PLEX_MONO_ITALIC: &[u8] =
        include_bytes!("../../../assets/fonts/IBMPlexMono-Italic.ttf");
    pub const NEWSREADER_ITALIC: &[u8] =
        include_bytes!("../../../assets/fonts/Newsreader-Italic.ttf");
    pub const NEWSREADER: &[u8] = include_bytes!("../../../assets/fonts/Newsreader.ttf");
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// Advance of '0'; the terminal cell width.
    pub advance: f32,
    /// ascent + descent + leading, rounded; the terminal cell height.
    pub line_height: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Baseline offset from the top of a line box.
    pub baseline: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasGlyph {
    pub uv: [f32; 4],
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// How to draw a run of UI text.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub font: FontId,
    pub px: f32,
    pub color: Color,
    /// Extra advance per glyph, in px (caps labels use 0.08em).
    pub tracking: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ShapedGlyph {
    pub id: u16,
    /// The face that has this glyph: the requested one, or a fallback.
    pub font: FontId,
    /// Byte offset into the shaped text.
    pub cluster: u32,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

struct Face {
    data: &'static [u8],
    index: u32,
    hb: rustybuzz::Face<'static>,
    units_per_em: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: FontId,
    px_x64: u32,
    id: u16,
}

pub struct FontSystem {
    faces: Vec<Face>,
    scale: ScaleContext,
    glyphs: HashMap<GlyphKey, Option<AtlasGlyph>>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// Pending atlas uploads: (x, y, w, h, data).
    pub uploads: Vec<(u32, u32, u32, u32, Vec<u8>)>,
    db: std::cell::RefCell<Option<fontdb::Database>>,
    icons: HashMap<IconKey, Option<AtlasGlyph>>,
    /// System faces for what the bundled fonts lack — symbols, emoji,
    /// other scripts — loaded the first time a glyph is missing. They
    /// live behind a RefCell so shaping (and measuring) stays `&self`.
    extra: std::cell::RefCell<Vec<Face>>,
    fallbacks: std::cell::RefCell<Option<Vec<FontId>>>,
    /// Measured widths by (font, px, tracking, text): the sidebar measures
    /// the same strings every frame. Cleared when it grows large.
    widths: std::cell::RefCell<HashMap<(u16, u32, u32, String), f32>>,
}

/// Font ids at or above this index the `extra` (fallback) faces.
const EXTRA_BASE: u16 = 0x8000;

/// Families tried, in order, for characters the main font lacks.
#[cfg(target_os = "windows")]
const FALLBACK_FAMILIES: &[&str] = &[
    "Cascadia Mono",
    "Consolas",
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Segoe UI",
    "Segoe UI Historic",
    "Microsoft YaHei",
    "Yu Gothic UI",
    "Malgun Gothic",
    "Nirmala UI",
];
#[cfg(target_os = "macos")]
const FALLBACK_FAMILIES: &[&str] = &[
    "Menlo",
    "Apple Symbols",
    "Apple Color Emoji",
    "Helvetica Neue",
    "PingFang SC",
    "Hiragino Sans",
    "Apple SD Gothic Neo",
];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const FALLBACK_FAMILIES: &[&str] = &[
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Noto Sans Symbols2",
    "Noto Sans Symbols",
    "Noto Color Emoji",
    "DejaVu Sans",
    "Noto Sans CJK SC",
    "Noto Sans",
];

impl FontSystem {
    pub fn new() -> FontSystem {
        FontSystem {
            faces: Vec::new(),
            scale: ScaleContext::new(),
            glyphs: HashMap::new(),
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            uploads: Vec::new(),
            db: std::cell::RefCell::new(None),
            icons: HashMap::new(),
            extra: std::cell::RefCell::new(Vec::new()),
            fallbacks: std::cell::RefCell::new(None),
            widths: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// Run `f` on a face, primary or fallback.
    fn with_face<R>(&self, font: FontId, f: impl FnOnce(&Face) -> R) -> R {
        if font.0 >= EXTRA_BASE {
            let extra = self.extra.borrow();
            f(&extra[(font.0 - EXTRA_BASE) as usize])
        } else {
            f(&self.faces[font.0 as usize])
        }
    }

    /// Load a system family as a fallback face, if it exists.
    fn try_load_fallback(&self, family: &str) -> Option<FontId> {
        {
            let mut db = self.db.borrow_mut();
            if db.is_none() {
                let mut d = fontdb::Database::new();
                d.load_system_fonts();
                *db = Some(d);
            }
        }
        let db = self.db.borrow();
        let db = db.as_ref()?;
        let id = db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            ..Default::default()
        })?;
        let face = db.face(id)?;
        let index = face.index;
        let data = match &face.source {
            fontdb::Source::File(p) => std::fs::read(p).ok(),
            fontdb::Source::Binary(b) => Some(b.as_ref().as_ref().to_vec()),
            fontdb::Source::SharedFile(_, b) => Some(b.as_ref().as_ref().to_vec()),
        }?;
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        let hb = rustybuzz::Face::from_slice(data, index)?;
        let font = FontRef::from_index(data, index as usize)?;
        let units_per_em = font.metrics(&[]).units_per_em as f32;
        let mut extra = self.extra.borrow_mut();
        extra.push(Face {
            data,
            index,
            hb,
            units_per_em,
        });
        Some(FontId(EXTRA_BASE + (extra.len() - 1) as u16))
    }

    fn fallbacks(&self) -> Vec<FontId> {
        if let Some(f) = self.fallbacks.borrow().as_ref() {
            return f.clone();
        }
        let mut out = Vec::new();
        for fam in FALLBACK_FAMILIES {
            if let Some(id) = self.try_load_fallback(fam) {
                out.push(id);
            }
        }
        tracing::info!(
            "font fallbacks: {} of {} families",
            out.len(),
            FALLBACK_FAMILIES.len()
        );
        *self.fallbacks.borrow_mut() = Some(out.clone());
        out
    }

    /// A face among the fallbacks that has `ch`.
    fn fallback_for(&self, ch: char) -> Option<FontId> {
        self.fallbacks()
            .into_iter()
            .find(|&id| self.swash(id).charmap().map(ch) != 0)
    }

    pub fn load_bytes(&mut self, data: &'static [u8], index: u32) -> Result<FontId> {
        let hb = rustybuzz::Face::from_slice(data, index).ok_or_else(|| anyhow!("bad font"))?;
        let font = FontRef::from_index(data, index as usize).ok_or_else(|| anyhow!("bad font"))?;
        let units_per_em = font.metrics(&[]).units_per_em as f32;
        self.faces.push(Face {
            data,
            index,
            hb,
            units_per_em,
        });
        Ok(FontId((self.faces.len() - 1) as u16))
    }

    /// Load a system font by family name, or fall back to `fallback`.
    pub fn load_system(&mut self, family: &str, fallback: FontId) -> FontId {
        {
            let mut db = self.db.borrow_mut();
            if db.is_none() {
                let mut d = fontdb::Database::new();
                d.load_system_fonts();
                *db = Some(d);
            }
        }
        let guard = self.db.borrow();
        let Some(db) = guard.as_ref() else {
            return fallback;
        };
        let id = db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            ..Default::default()
        });
        let Some(id) = id else {
            tracing::warn!("font {family:?} not found; using fallback");
            return fallback;
        };
        let Some(face) = db.face(id) else {
            return fallback;
        };
        let index = face.index;
        let data = match &face.source {
            fontdb::Source::File(p) => std::fs::read(p).ok(),
            fontdb::Source::Binary(b) => Some(b.as_ref().as_ref().to_vec()),
            fontdb::Source::SharedFile(_, b) => Some(b.as_ref().as_ref().to_vec()),
        };
        let Some(data) = data else { return fallback };
        drop(guard);
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        match self.load_bytes(data, index) {
            Ok(f) => {
                tracing::info!("font {family:?} loaded");
                f
            }
            Err(_) => fallback,
        }
    }

    fn swash(&self, font: FontId) -> FontRef<'static> {
        self.with_face(font, |f| {
            FontRef::from_index(f.data, f.index as usize).expect("parsed at load")
        })
    }

    pub fn metrics(&self, font: FontId, px: f32) -> Metrics {
        let f = self.swash(font);
        let m = f.metrics(&[]).scale(px);
        let advance = f
            .glyph_metrics(&[])
            .scale(px)
            .advance_width(f.charmap().map('0'));
        let line_height = (m.ascent + m.descent + m.leading).round().max(1.0);
        Metrics {
            advance: advance.round().max(1.0),
            line_height,
            ascent: m.ascent,
            descent: m.descent,
            baseline: (m.ascent + m.leading / 2.0).round(),
        }
    }

    fn shape_one(&self, font: FontId, px: f32, text: &str, cluster_base: u32) -> Vec<ShapedGlyph> {
        self.with_face(font, |face| {
            let mut buf = rustybuzz::UnicodeBuffer::new();
            buf.push_str(text);
            buf.guess_segment_properties();
            let features = [rustybuzz::Feature::new(
                rustybuzz::ttf_parser::Tag::from_bytes(b"calt"),
                1,
                ..,
            )];
            let out = rustybuzz::shape(&face.hb, &features, buf);
            let s = px / face.units_per_em;
            out.glyph_infos()
                .iter()
                .zip(out.glyph_positions())
                .map(|(i, p)| ShapedGlyph {
                    id: i.glyph_id as u16,
                    font,
                    cluster: i.cluster + cluster_base,
                    x_advance: p.x_advance as f32 * s,
                    x_offset: p.x_offset as f32 * s,
                    y_offset: p.y_offset as f32 * s,
                })
                .collect()
        })
    }

    /// Shape with `font`; runs it has no glyphs for are reshaped with the
    /// first fallback face that has them.
    pub fn shape(&self, font: FontId, px: f32, text: &str) -> Vec<ShapedGlyph> {
        let glyphs = self.shape_one(font, px, text, 0);
        if !glyphs.iter().any(|g| g.id == 0) {
            return glyphs;
        }
        // Byte ranges of missing clusters, merged when adjacent.
        let bytes: Vec<u32> = {
            let mut b: Vec<u32> = text.char_indices().map(|(i, _)| i as u32).collect();
            b.push(text.len() as u32);
            b
        };
        let cluster_end = |c: u32| {
            bytes
                .iter()
                .copied()
                .find(|&x| x > c)
                .unwrap_or(text.len() as u32)
        };
        let mut out: Vec<ShapedGlyph> = Vec::with_capacity(glyphs.len());
        let mut i = 0;
        while i < glyphs.len() {
            if glyphs[i].id != 0 {
                out.push(glyphs[i]);
                i += 1;
                continue;
            }
            let start = glyphs[i].cluster;
            let mut end = cluster_end(start);
            let mut j = i + 1;
            while j < glyphs.len() && glyphs[j].id == 0 {
                end = end.max(cluster_end(glyphs[j].cluster));
                j += 1;
            }
            let (s, e) = (start as usize, end as usize);
            let run = &text[s..e];
            let first = run.chars().next().unwrap_or(' ');
            match self.fallback_for(first) {
                Some(fb) => out.extend(self.shape_one(fb, px, run, start)),
                None => out.extend(glyphs[i..j].iter().copied()),
            }
            i = j;
        }
        out
    }

    pub fn glyph(&mut self, font: FontId, px: f32, id: u16) -> Option<AtlasGlyph> {
        let key = GlyphKey {
            font,
            px_x64: (px * 64.0) as u32,
            id,
        };
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        let fref = self.swash(font);
        let mut scaler = self.scale.builder(fref).size(px).hint(true).build();
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, id);
        let entry = image.and_then(|img| {
            let (w, h) = (img.placement.width, img.placement.height);
            if w == 0 || h == 0 {
                return None;
            }
            let data: Vec<u8> = if img.data.len() == (w * h * 4) as usize {
                img.data.chunks(4).map(|p| p[3]).collect()
            } else {
                img.data.clone()
            };
            let (x, y) = self.pack(w, h)?;
            self.uploads.push((x, y, w, h, data));
            let s = ATLAS_SIZE as f32;
            Some(AtlasGlyph {
                uv: [
                    x as f32 / s,
                    y as f32 / s,
                    (x + w) as f32 / s,
                    (y + h) as f32 / s,
                ],
                left: img.placement.left,
                top: img.placement.top,
                width: w,
                height: h,
            })
        });
        self.glyphs.insert(key, entry);
        entry
    }

    fn pack(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w1, h1) = (w + 1, h + 1);
        if self.shelf_x + w1 > ATLAS_SIZE {
            self.shelf_x = 0;
            self.shelf_y += self.shelf_h;
            self.shelf_h = 0;
        }
        if self.shelf_y + h1 > ATLAS_SIZE {
            tracing::warn!("glyph atlas full");
            return None;
        }
        let pos = (self.shelf_x, self.shelf_y);
        self.shelf_x += w1;
        self.shelf_h = self.shelf_h.max(h1);
        Some(pos)
    }

    /// Lay out `text` at (x, baseline) and push its glyphs. Returns the
    /// advance width.
    pub fn draw(&mut self, scene: &mut Scene, s: Style, x: f32, baseline: f32, text: &str) -> f32 {
        let Style {
            font,
            px,
            color,
            tracking,
        } = s;
        // Labels (the tracked style) read in Caps, never ALLCAPS.
        let text = if tracking > 0.0 {
            caps(text)
        } else {
            text.to_string()
        };
        let mut pen = x;
        for g in self.shape(font, px, &text) {
            if let Some(a) = self.glyph(g.font, px, g.id) {
                scene.push(Instance::glyph(
                    (pen + g.x_offset + a.left as f32).round(),
                    (baseline - g.y_offset - a.top as f32).round(),
                    a.width as f32,
                    a.height as f32,
                    a.uv,
                    color,
                ));
            }
            pen += g.x_advance + tracking;
        }
        pen - x
    }

    /// Width of `text` without drawing it.
    pub fn measure(&self, s: Style, text: &str) -> f32 {
        let capped;
        let text = if s.tracking > 0.0 {
            capped = caps(text);
            capped.as_str()
        } else {
            text
        };
        let key = (
            s.font.0,
            s.px.to_bits(),
            s.tracking.to_bits(),
            text.to_string(),
        );
        if let Some(w) = self.widths.borrow().get(&key) {
            return *w;
        }
        let w = self
            .shape(s.font, s.px, text)
            .iter()
            .map(|g| g.x_advance + s.tracking)
            .sum();
        let mut cache = self.widths.borrow_mut();
        if cache.len() > 8192 {
            cache.clear();
        }
        cache.insert(key, w);
        w
    }
}

impl Default for FontSystem {
    fn default() -> Self {
        FontSystem::new()
    }
}

/// Bundled Phosphor icons (MIT), regular weight unless noted.
pub mod icons {
    macro_rules! icon {
        ($name:ident, $file:literal) => {
            pub const $name: (&str, &str) = (
                $file,
                include_str!(concat!("../../../assets/icons/", $file, ".svg")),
            );
        };
    }
    icon!(SEARCH, "magnifying-glass");
    icon!(COMMAND, "command");
    icon!(BACK, "arrow-left");
    icon!(FORWARD, "arrow-right");
    icon!(RELOAD, "arrows-clockwise");
    icon!(SIDEBAR, "sidebar-simple");
    icon!(TERMINAL, "terminal-window");
    icon!(GLOBE, "globe");
    icon!(SETTINGS, "gear-six");
    icon!(PIP, "picture-in-picture");
    icon!(BELL, "bell");
    icon!(PORTS, "plugs-connected");
    icon!(ASSISTANT, "sparkle");
    icon!(MINIMIZE, "minus");
    icon!(MAXIMIZE, "square");
    icon!(CLOSE, "x");
    icon!(PIN, "push-pin");
    icon!(CARET_RIGHT, "caret-right");
    icon!(CARET_DOWN, "caret-down");
    icon!(STACK, "stack");
    icon!(LINK, "link");
    icon!(SHARE, "share-network");
    icon!(PLUS, "plus");
    icon!(MINUS, "minus");
    icon!(ARROWS_OUT, "arrows-out-cardinal");
    icon!(SWAP, "arrows-left-right");
    icon!(SOLO, "corners-out");
    icon!(TO_TAB, "arrow-square-out");
    icon!(ENTER, "key-return");
    icon!(HASH, "hash");
    icon!(MORE, "dots-three");
    icon!(HARD_HAT, "hard-hat");
    icon!(COPY, "copy");
    icon!(CHECK, "check");
    icon!(PENCIL, "pencil-simple");
    icon!(SMILEY, "smiley");
    icon!(TILES, "squares-four");
    icon!(TAG, "tag");
    icon!(SHUFFLE, "shuffle");
    icon!(SUN, "sun");
    icon!(MOON, "moon");
    icon!(UNDO, "arrow-counter-clockwise");
    icon!(SLIDERS_H, "sliders-horizontal");
    icon!(APP_WINDOW, "app-window");
    icon!(WARNING, "warning");
    icon!(HOME, "house");
    icon!(GLOBE_BOLD, "globe-bold");
    icon!(TERMINAL_BOLD, "terminal-window-bold");
    icon!(BELL_BOLD, "bell-bold");
    icon!(BUG, "bug");
    icon!(BROADCAST, "broadcast");
    icon!(KEYBOARD, "keyboard");
    icon!(PALETTE, "palette");
    icon!(SQUARES, "squares-four");
    icon!(BRUSH, "paint-brush");
    icon!(CODE, "code");
    icon!(CURSOR, "cursor-click");
    icon!(EXPAND, "arrows-out-simple");
    icon!(USER, "user-circle");
    icon!(CHAT, "chat-circle-dots");
    icon!(COOKIE, "cookie");
    icon!(FOLDER, "folder-open");
    icon!(FOLDER_SIMPLE, "folder-simple");
    icon!(GITHUB, "github-logo");
    icon!(SHIELD, "shield-check");
    icon!(DOWNLOAD, "download-simple");
    icon!(OPEN_EXTERNAL, "arrow-square-out");
    icon!(SLIDERS, "sliders-horizontal");
    icon!(CIRCLE, "circle");
    icon!(BOOK, "book-open");
    icon!(BOOK_TEXT, "book-open-text");
    icon!(CONSOLE, "terminal");
    icon!(NETWORK, "network");
    icon!(PLANET, "planet");
    icon!(ROCKET, "rocket-launch");
    icon!(HISTORY, "clock-counter-clockwise");
    icon!(SPEAKER, "speaker-high");
    icon!(SPEAKER_OFF, "speaker-slash");
    icon!(CALENDAR, "calendar-blank");
    icon!(DESKTOP, "desktop");
    icon!(EYE_SLASH, "eye-slash");
    icon!(LOCK_KEY, "lock-key");
    icon!(IMAGE, "image");
    icon!(TEXT_AA, "text-aa");
    icon!(HAND_WAVING, "hand-waving");
}

impl FontSystem {
    /// Rasterize an SVG icon at `px` (square) into the atlas; cached by
    /// name and size.
    pub fn icon(&mut self, icon: (&'static str, &'static str), px: f32) -> Option<AtlasGlyph> {
        let (name, svg) = icon;
        let key = IconKey {
            name,
            px_x64: (px * 64.0) as u32,
        };
        if let Some(g) = self.icons.get(&key) {
            return *g;
        }
        let entry = (|| {
            let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
            let side = px.round().max(1.0) as u32;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
            let s = side as f32 / tree.size().width().max(1.0);
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(s, s),
                &mut pixmap.as_mut(),
            );
            let data: Vec<u8> = pixmap.pixels().iter().map(|p| p.alpha()).collect();
            let (x, y) = self.pack(side, side)?;
            self.uploads.push((x, y, side, side, data));
            let a = ATLAS_SIZE as f32;
            Some(AtlasGlyph {
                uv: [
                    x as f32 / a,
                    y as f32 / a,
                    (x + side) as f32 / a,
                    (y + side) as f32 / a,
                ],
                left: 0,
                top: side as i32,
                width: side,
                height: side,
            })
        })();
        self.icons.insert(key, entry);
        entry
    }

    /// Draw an icon with its top-left at (x, y).
    /// An icon spun by `angle` radians about its centre and scaled by
    /// `scale` (about the centre too): hover motion.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_icon_moved(
        &mut self,
        scene: &mut Scene,
        icon: (&'static str, &'static str),
        px: f32,
        x: f32,
        y: f32,
        color: Color,
        angle: f32,
        scale: f32,
    ) -> f32 {
        if let Some(g) = self.icon(icon, px) {
            let (w, h) = (g.width as f32 * scale, g.height as f32 * scale);
            let dx = (g.width as f32 - w) / 2.0;
            let dy = (g.height as f32 - h) / 2.0;
            let mut i = Instance::glyph(x.round() + dx, y.round() + dy, w, h, g.uv, color);
            i.phase = angle;
            scene.push(i);
        }
        px
    }

    pub fn draw_icon(
        &mut self,
        scene: &mut Scene,
        icon: (&'static str, &'static str),
        px: f32,
        x: f32,
        y: f32,
        color: Color,
    ) -> f32 {
        if let Some(g) = self.icon(icon, px) {
            scene.push(Instance::glyph(
                x.round(),
                y.round(),
                g.width as f32,
                g.height as f32,
                g.uv,
                color,
            ));
        }
        px
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct IconKey {
    name: &'static str,
    px_x64: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_glyphs_come_from_a_fallback_face() {
        let mut fonts = FontSystem::new();
        let plex = fonts.load_bytes(bundled::PLEX_MONO, 0).unwrap();
        // Plex Mono has the letters; the command sign and the emoji it does not.
        let glyphs = fonts.shape(plex, 13.0, "a \u{2318} b \u{1F600}");
        assert!(
            glyphs.iter().all(|g| g.id != 0 || g.font != plex),
            "every glyph resolved or reshaped: {glyphs:?}"
        );
        let resolved = glyphs
            .iter()
            .filter(|g| g.font != plex && g.id != 0)
            .count();
        // On a machine with system fonts, both symbols resolve; on a bare CI box at least the code runs.
        if fonts.fallbacks().is_empty() {
            return;
        }
        assert!(
            resolved >= 1,
            "a fallback face supplied the symbol: {glyphs:?}"
        );
        // Measuring still works through fallbacks and stays &self.
        let w = fonts.measure(
            Style {
                font: plex,
                px: 13.0,
                color: [0.0; 4],
                tracking: 0.0,
            },
            "\u{2318}",
        );
        assert!(w > 0.0);
    }
}

/// ALLCAPS words become Caps: "NEW TAB" → "New Tab", "CTRL+SHIFT+D" →
/// "Ctrl+Shift+D", "OSC 52" → "Osc 52". A word with any lowercase letter
/// is left alone (names, paths, domains); so are digits and symbols.
pub fn caps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_inclusive(char::is_whitespace) {
        let body = word.trim_end_matches(char::is_whitespace);
        let tail = &word[body.len()..];
        let letters: Vec<char> = body.chars().filter(|c| c.is_alphabetic()).collect();
        let all_caps = !letters.is_empty() && letters.iter().all(|c| c.is_uppercase());
        if !all_caps {
            out.push_str(word);
            continue;
        }
        // Sub-words at + / - · keep their own first letter up (key chords).
        let mut first = true;
        for ch in body.chars() {
            if ch.is_alphabetic() {
                if first {
                    out.push(ch);
                    first = false;
                } else {
                    out.extend(ch.to_lowercase());
                }
            } else {
                out.push(ch);
                if matches!(ch, '+' | '/' | '-' | '·') {
                    first = true;
                }
            }
        }
        out.push_str(tail);
    }
    out
}

#[cfg(test)]
mod caps_tests {
    use super::caps;

    #[test]
    fn allcaps_words_become_caps() {
        assert_eq!(caps("NEW TAB"), "New Tab");
        assert_eq!(caps("CTRL+SHIFT+D"), "Ctrl+Shift+D");
        assert_eq!(caps("OSC 52 · F2"), "Osc 52 · F2");
        assert_eq!(caps("rules.luau"), "rules.luau");
        assert_eq!(caps("std - Rust"), "std - Rust");
        assert_eq!(caps("×"), "×");
        assert_eq!(caps("POWERSHELL  70×34"), "Powershell  70×34");
    }
}
