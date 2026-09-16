//! nus-render — the compositor. One wgpu surface, layers of instanced quads
//! (solid rects, atlas glyphs, external textures), fonts and shaping, the
//! terminal grid renderer, and the Broadsheet theme tokens.

pub mod gpu;
pub mod grid;
pub mod scene;
pub mod text;
pub mod theme;

pub use gpu::{Gpu, TextureBinder};
pub use grid::GridRenderer;
pub use scene::{Bind, Color, Instance, Layer, Rect, Scene};
pub use text::{FontId, FontSystem, Metrics, Style};
pub use theme::{Mode, Theme};
