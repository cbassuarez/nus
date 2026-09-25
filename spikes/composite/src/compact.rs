//! Compact mode: the sidebar is a 48px column of icons (favicon, emoji or
//! kind), the top strip stays, and
//! the content takes the rest. Ctrl+Shift+B, the palette, or LOOK ·
//! SIDEBAR. The rows are the same rows, so clicks, drags, selection and
//! the tab menu all work unchanged; titles show as a tooltip on hover.

use crate::app::{hover_key, App, IconMotion, SideHit};
use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};

/// The column's width, in logical px.
pub const COMPACT_W: f32 = 48.0;
/// The header: the window square, then NEW TAB.
pub const COMPACT_HEAD: f32 = 80.0;

impl App {
    pub(crate) fn compact(&self) -> bool {
        self.sidebar_rules.compact
    }

    /// The strip is there unless focus mode takes the chrome away. Compact
    /// narrows the sidebar; it never takes the strip with it.
    pub(crate) fn strip_shown(&self) -> bool {
        !self.focus
    }

    pub(crate) fn toggle_compact(&mut self) {
        self.sidebar_rules.compact = !self.sidebar_rules.compact;
        self.save_prefs();
        self.layout();
        self.dirty = true;
    }

    /// The compact column: window square, NEW TAB, icon rows, settings.
    pub(crate) fn draw_sidebar_compact(&mut self, scene: &mut Scene) {
        let t = self.theme.clone();
        let ink = t.ink;
        let sb = self.sidebar_rect();
        let (mx, my) = self.mouse;
        self.side_hits.clear();
        self.settle_sidebar_scroll();
        let g = self.sidebar_geometry();
        let isz = self.px(16.0);
        let cx = sb.x + ((sb.w - isz) / 2.0).round();
        let row = self.px(40.0);
        // Window square: the signal, a click lists the windows.
        let sq = self.px(14.0);
        let wr = Rect::new(sb.x, sb.y, sb.w, row);
        let wsq = Rect::new(sb.x + ((sb.w - sq) / 2.0).round(), sb.y + ((row - sq) / 2.0).round(), sq, sq);
        if wr.contains(mx, my) {
            scene.rect(wr, t.tint);
        }
        scene.rect(wsq, self.surface.signal);
        self.side_hits.push((wr, SideHit::Window));
        // NEW TAB: a plus; hold or right-click for the kinds.
        let nr = Rect::new(sb.x, sb.y + row, sb.w, row);
        self.icon_button(scene, icons::PLUS, isz, cx, nr.y + ((row - isz) / 2.0).round(), ink, nr, hover_key("compact-new", 0), IconMotion::Spin(90.0));
        self.side_hits.push((nr, SideHit::NewShell));
        scene.hline(sb.x, sb.y + COMPACT_HEAD * self.scale - self.px(m::STRUCTURE), sb.w, self.px(m::STRUCTURE), ink);
        if self.side_page==crate::files::SidePage::Files {
            self.draw_tree(scene,sb,sb.y+self.px(COMPACT_HEAD),g.foot_y);
            self.draw_responsive_footer(scene,sb,g.foot_y);self.draw_sidebar_menus(scene,sb);return;
        }
        self.draw_pins(scene,sb);
        // Pinned tabs first, as a block.
        let tiled_ids: Vec<u64> = self.tiling.as_ref().map(|t| t.ids()).unwrap_or_default();
        let tabs = std::mem::take(&mut self.tabs);
        let mut py = sb.y + COMPACT_HEAD * self.scale + self.pins_height();
        let pin_h = self.px(32.0);
        for &i in &g.pinned {
            let cell = Rect::new(sb.x, py, sb.w, pin_h);
            let active = i == self.active;
            if active {
                scene.rect(cell, ink);
            } else if cell.contains(mx, my) {
                scene.rect(cell, t.tint);
            }
            let color = if active { self.on_fill(ink) } else { t.dim };
            self.draw_tab_icon(scene, &tabs[i], cx, py + ((pin_h - isz) / 2.0).round(), isz, color);
            py += pin_h;
        }
        if !g.pinned.is_empty() {
            scene.hline(sb.x, py, sb.w, self.px(m::STRUCTURE), ink);
        }
        // Rows.
        let mut tip: Option<(usize, f32)> = None;
        for &(i, y, h) in &g.rows {
            // Rows out of the list's window are not drawn; one half out is
            // cut to it.
            let Some(cell) = (if h < 1.0 { None } else { g.clip(sb, y, h) }) else { continue };
            scene.layer(Some(cell));
            let tab = &tabs[i];
            let active = i == self.active;
            let hovered = cell.contains(mx, my);
            let selected = self.selected.contains(&i);
            if active {
                scene.rect(cell, crate::surface::mix(self.paper(), ink, t.tint[3]));
            } else if hovered {
                scene.rect(cell, t.tint);
            }
            // The signal bar on the left: active, or selected.
            if active || selected {
                let s = tab.look.signal.unwrap_or(self.surface.signal);
                scene.rect(Rect::new(sb.x, y, self.px(3.0), h), if selected && !active { s } else { s });
            }
            // Depth nudges the icon right a touch, so stacks read.
            let depth = crate::app::depth_of(&tabs, i).min(3) as f32;
            let ix = cx + depth * self.px(3.0);
            let color = if active { ink } else { t.dim };
            let iy = y + ((h - isz) / 2.0).round();
            scene.layer(Some(cell));
            self.draw_tab_icon(scene, tab, ix, iy, isz, color);
            scene.layer(Some(cell));
            // The corner: × while the pointer is on the row — closing a
            // tab is closing what it runs, so it is one click here too.
            // Otherwise a signal dot for waiting, or a tiny tiled mark.
            if hovered {
                let d = self.px(11.0);
                let cr = Rect::new(cell.right() - d - self.px(3.0), y + self.px(3.0), d, d);
                let reach = crate::touch::grown(cr, self.px(5.0));
                let hot = reach.contains(mx, my);
                if hot {
                    // A square under it, so the mark reads as a target.
                    scene.rect(crate::touch::grown(cr, self.px(3.0)), crate::surface::mix(self.paper(), ink, 0.14));
                }
                self.fonts.draw_icon(scene, icons::CLOSE, d, cr.x, cr.y, if hot { ink } else { crate::app::fade(ink, 0.6) });
                self.side_hits.push((reach, SideHit::Close(i)));
            } else if tab.waiting() {
                let d = self.px(6.0);
                scene.rect(Rect::new(cell.right() - d - self.px(4.0), y + self.px(4.0), d, d), self.surface.signal);
            } else if tiled_ids.contains(&tab.id) {
                let d = self.px(7.0);
                self.fonts.draw_icon(scene, icons::TILES, d, cell.right() - d - self.px(4.0), y + self.px(4.0), t.dim);
            }
            if hovered {
                tip = Some((i, y));
            }
            scene.layer(None);
        }
        self.draw_responsive_footer(scene,sb,g.foot_y);
        self.tabs = tabs;
        self.compact_tip = tip;
        self.draw_sidebar_menus(scene, sb);
    }

    /// Offer the visible row to the shared tooltip controller; it is drawn
    /// after the panes and follows the same dwell/dismissal rules as chrome.
    pub(crate) fn draw_compact_tip(&mut self, _scene: &mut Scene) {
        let Some((i, _)) = self.compact_tip.take() else { return; };
        if !self.sidebar_visible() { return; }
        let Some(tab) = self.tabs.get(i) else { return; };
        let id = tab.id;
        let (title, detail) = tab.row_text();
        let words = if detail.is_empty() { title } else { format!("{title} · {detail}") };
        let sb = self.sidebar_rect();
        let geometry = self.sidebar_geometry();
        if let Some((_, y, height)) = geometry.rows.iter().find(|(index, _, _)| *index == i) {
            if let Some(hit) = geometry.clip(sb, *y, *height) {
                self.offer_tip(hover_key("compact-tab", id as usize), hit, words);
            }
        }
    }

    /// A tab's icon: its emoji, its favicon, or its kind.
    fn draw_tab_icon(&mut self, scene: &mut Scene, tab: &crate::app::Tab, x: f32, y: f32, isz: f32, color: nus_render::Color) {
        if self.draw_small_tab(scene,tab,x,y,isz,color){return;}
        if let Some(e) = &tab.emoji {
            let st = Style { font: self.f.ui, px: self.px(14.0), color, tracking: 0.0 };
            let ew = self.fonts.measure(st, e);
            self.fonts.draw(scene, st, x + ((isz - ew) / 2.0).max(0.0), y + isz - self.px(2.0), e);
            return;
        }
        let (main, other) = tab.panes();
        self.draw_pane_icon(scene, main, x, y, isz, color, None);
        if let Some(o) = other {
            let bsz = self.px(8.0);
            let br = Rect::new(x + isz - bsz + self.px(2.0), y + isz - bsz + self.px(2.0), bsz, bsz);
            scene.rect(Rect::new(br.x - self.px(1.5), br.y - self.px(1.5), bsz + self.px(3.0), bsz + self.px(3.0)), self.paper());
            self.draw_pane_icon(scene, o, br.x, br.y, bsz, color, None);
        }
    }
}
