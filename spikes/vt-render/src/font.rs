//! Font loading, metrics, shaping (rustybuzz) and rasterization (swash) into
//! a single R8 atlas.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use swash::FontRef;

pub const ATLAS_SIZE: u32 = 2048;

pub struct Metrics {
    pub cell_w: f32,
    pub cell_h: f32,
    /// Baseline offset from the top of the cell.
    pub baseline: f32,
    pub units_per_em: f32,
    pub px: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasGlyph {
    /// Normalized UV rect (u0, v0, u1, v1).
    pub uv: [f32; 4],
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

pub struct Font {
    data: &'static [u8],
    pub name: String,
    pub metrics: Metrics,
    scale_ctx: ScaleContext,
    glyphs: HashMap<u16, Option<AtlasGlyph>>,
    // Shelf packer state.
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    /// Pending atlas uploads: (x, y, w, h, data).
    pub uploads: Vec<(u32, u32, u32, u32, Vec<u8>)>,
}

impl Font {
    /// Find a monospace font by preference list and load it at `px` pixels.
    pub fn load(px: f32) -> Result<Font> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let candidates = [
            "JetBrains Mono",
            "Cascadia Code",
            "Cascadia Mono",
            "Fira Code",
            "Consolas",
            "Menlo",
            "SF Mono",
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Courier New",
        ];
        let families: Vec<fontdb::Family> = candidates.iter().map(|n| fontdb::Family::Name(n)).collect();
        let id = db
            .query(&fontdb::Query {
                families: &families,
                ..Default::default()
            })
            .ok_or_else(|| anyhow!("no monospace font found"))?;
        let face = db.face(id).ok_or_else(|| anyhow!("face missing"))?;
        let name = face.families.first().map(|f| f.0.clone()).unwrap_or_default();
        let index = face.index;
        let data: Vec<u8> = match &face.source {
            fontdb::Source::File(p) => std::fs::read(p)?,
            fontdb::Source::Binary(b) => b.as_ref().as_ref().to_vec(),
            fontdb::Source::SharedFile(_, b) => b.as_ref().as_ref().to_vec(),
        };
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        let font = FontRef::from_index(data, index as usize).ok_or_else(|| anyhow!("swash parse"))?;
        let m = font.metrics(&[]).scale(px);
        let advance = font.glyph_metrics(&[]).scale(px).advance_width(font.charmap().map('0'));
        let cell_w = advance.round().max(1.0);
        let cell_h = (m.ascent + m.descent + m.leading).round().max(1.0);
        tracing::info!(
            "font {name} @ {px}px: cell {cell_w}x{cell_h}, ascent {:.1} descent {:.1}",
            m.ascent,
            m.descent
        );
        Ok(Font {
            data,
            name,
            metrics: Metrics {
                cell_w,
                cell_h,
                baseline: (m.ascent + m.leading / 2.0).round(),
                units_per_em: font.metrics(&[]).units_per_em as f32,
                px,
            },
            scale_ctx: ScaleContext::new(),
            glyphs: HashMap::new(),
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            uploads: Vec::new(),
        })
    }

    fn swash(&self) -> FontRef<'static> {
        FontRef::from_index(self.data, 0).expect("font parsed once already")
    }

    pub fn hb_face(&self) -> rustybuzz::Face<'static> {
        rustybuzz::Face::from_slice(self.data, 0).expect("font parsed once already")
    }

    /// Shape one run of text. Returns (glyph id, cluster byte offset, x offset px, y offset px).
    pub fn shape(&self, face: &rustybuzz::Face<'static>, text: &str) -> Vec<(u16, u32, f32, f32)> {
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let features = [rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(b"calt"), 1, ..)];
        let out = rustybuzz::shape(face, &features, buf);
        let scale = self.metrics.px / self.metrics.units_per_em;
        out.glyph_infos()
            .iter()
            .zip(out.glyph_positions())
            .map(|(i, p)| {
                (
                    i.glyph_id as u16,
                    i.cluster,
                    p.x_offset as f32 * scale,
                    p.y_offset as f32 * scale,
                )
            })
            .collect()
    }

    /// Rasterize (or fetch) a glyph. `None` for empty glyphs (spaces).
    pub fn glyph(&mut self, id: u16) -> Option<AtlasGlyph> {
        if let Some(g) = self.glyphs.get(&id) {
            return *g;
        }
        let font = self.swash();
        let mut scaler = self.scale_ctx.builder(font).size(self.metrics.px).hint(true).build();
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
            // Color images come back RGBA; keep alpha only for this spike.
            let data: Vec<u8> = if img.data.len() == (w * h * 4) as usize {
                img.data.chunks(4).map(|p| p[3]).collect()
            } else {
                img.data.clone()
            };
            let (x, y) = self.pack(w, h)?;
            self.uploads.push((x, y, w, h, data));
            let s = ATLAS_SIZE as f32;
            Some(AtlasGlyph {
                uv: [x as f32 / s, y as f32 / s, (x + w) as f32 / s, (y + h) as f32 / s],
                left: img.placement.left,
                top: img.placement.top,
                width: w,
                height: h,
            })
        });
        self.glyphs.insert(id, entry);
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
}
