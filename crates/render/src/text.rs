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
    db: Option<fontdb::Database>,
}

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
            db: None,
        }
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
        let db = self.db.get_or_insert_with(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        });
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
        let f = &self.faces[font.0 as usize];
        FontRef::from_index(f.data, f.index as usize).expect("parsed at load")
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

    pub fn shape(&self, font: FontId, px: f32, text: &str) -> Vec<ShapedGlyph> {
        let face = &self.faces[font.0 as usize];
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
                cluster: i.cluster,
                x_advance: p.x_advance as f32 * s,
                x_offset: p.x_offset as f32 * s,
                y_offset: p.y_offset as f32 * s,
            })
            .collect()
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
        let mut pen = x;
        for g in self.shape(font, px, text) {
            if let Some(a) = self.glyph(font, px, g.id) {
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
        self.shape(s.font, s.px, text)
            .iter()
            .map(|g| g.x_advance + s.tracking)
            .sum()
    }
}

impl Default for FontSystem {
    fn default() -> Self {
        FontSystem::new()
    }
}
