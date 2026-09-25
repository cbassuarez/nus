//! Shell colors. Every shell stack wears a color from the theme's signal
//! family: fifteen colors, five hues around the signal (±30° in OKLCH) at
//! three lightnesses. On paper they are rich (light, clearly colored,
//! dark text); on ink they are tinted (near-black with a clear cast).
//!
//! SHELL COLORS in settings picks how a new shell gets one: RANDOM (a
//! color no open stack is wearing, when there is one), BY FOLDER (the
//! folder the shell opened in picks it, so a project keeps its color), or
//! NONE. A stack keeps its slot for its life and across restarts; the
//! color itself is worked out from the slot and the theme, so switching
//! paper and ink, or the signal, recolors every stack in step.
//!
//! The color reaches the tab through rules.luau: `new_tab` gets it as
//! `ctx.shell_color` and the default rule returns it, so a rule of your
//! own can use it, change it, or ignore it.

use crate::app::{App, Pane, Tab};
use crate::surface::Overrides;
use nus_render::oklch::{from_srgb, oklch};
use nus_render::Color;

/// How a new shell gets its color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShellTint {
    #[default]
    Random,
    Folder,
    None,
}

/// Colors in a family: five hues × three lightnesses.
pub const SLOTS: usize = 15;
const HUES: [f32; 5] = [-30.0, -15.0, 0.0, 15.0, 30.0];

/// The family's colors for this signal, on paper (`dark` false) or ink:
/// each a pane background and a signal for its row and rules.
pub fn family(signal: Color, dark: bool) -> [Overrides; SLOTS] {
    let s = from_srgb(signal);
    // A grey signal (onyx, contrast) makes a grey family: lightness only.
    let grey = s.c < 0.03;
    let (lights, chroma) = if dark { ([0.25, 0.28, 0.31], 0.05) } else { ([0.84, 0.80, 0.76], 0.10) };
    let accent_l = if dark { 0.72 } else { 0.56 };
    std::array::from_fn(|k| {
        let (hue, step) = (HUES[k / 3], k % 3);
        let h = s.h + if grey { 0.0 } else { hue };
        let c = if grey { s.c } else { chroma };
        Overrides { bg: Some(oklch(lights[step], c, h)), signal: Some(oklch(accent_l, if grey { s.c } else { s.c.clamp(0.10, 0.17) }, h)) }
    })
}

/// A slot for a folder: the same folder, the same slot, every time.
pub fn slot_for_folder(folder: &str) -> u8 {
    let key = folder.trim_end_matches(['/', '\\']).to_lowercase();
    // FNV-1a: stable across runs and builds, unlike the std hasher.
    let h = key.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
    (h % SLOTS as u32) as u8
}

/// A random slot, one no open stack is wearing when there is one.
pub fn slot_at_random(taken: &[u8]) -> u8 {
    let free: Vec<u8> = (0..SLOTS as u8).filter(|s| !taken.contains(s)).collect();
    let pool = if free.is_empty() { (0..SLOTS as u8).collect() } else { free };
    let mut b = [0u8; 4];
    let r = getrandom::fill(&mut b).map(|_| u32::from_le_bytes(b)).unwrap_or_else(|_| crate::clock::now().elapsed().subsec_nanos());
    pool[r as usize % pool.len()]
}

impl App {
    /// The slot a new tab's shell takes, per SHELL COLORS; None for
    /// anything but a shell, and for NONE.
    pub(crate) fn new_shell_slot(&self, left: &Pane) -> Option<u8> {
        let Pane::Term(t) = left else { return None };
        match self.behavior.shell_tint {
            ShellTint::None => None,
            ShellTint::Folder => {
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
                Some(slot_for_folder(t.opened_in.as_deref().unwrap_or(&home)))
            }
            ShellTint::Random => {
                let taken: Vec<u8> = self.tabs.iter().filter_map(|t| t.shell_slot).collect();
                Some(slot_at_random(&taken))
            }
        }
    }

    /// The color for a slot under the current theme.
    pub(crate) fn shell_color(&self, slot: Option<u8>) -> Option<Overrides> {
        let dark = self.theme.mode == nus_render::Mode::Ink;
        slot.map(|s| family(self.surface.signal, dark)[s as usize % SLOTS].clone())
    }

    /// A shell's terminal reads its colors against the pane it is drawn
    /// on: the stack's color, when it has one, is its background.
    pub(crate) fn fit_palette(theme: &nus_render::Theme, tab: &mut Tab) {
        let bg = tab.look.bg.unwrap_or(theme.paper);
        for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            if let Pane::Term(t) = pane {
                theme.apply(&mut t.term.palette);
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                t.term.palette.set_base(nus_vt::palette::BG, nus_vt::Rgb { r: q(bg[0]), g: q(bg[1]), b: q(bg[2]) });
                t.term.grid_mut().damage_all();
            }
        }
    }

    /// Every shell stack in its color again: after SHELL COLORS changes,
    /// a theme or its face changes, or the signal moves. A color the user
    /// chose for a tab stays theirs.
    pub(crate) fn recolor_shells(&mut self) {
        for i in 0..self.tabs.len() {
            if self.tabs[i].tint.is_some() || self.tabs[i].parent.is_some() || !matches!(self.tabs[i].left, Pane::Term(_)) {
                continue;
            }
            if self.behavior.shell_tint == ShellTint::None {
                self.tabs[i].shell_slot = None;
            } else if self.tabs[i].shell_slot.is_none() {
                self.tabs[i].shell_slot = self.new_shell_slot(&self.tabs[i].left);
            }
            let look = self.look_with(&self.tabs[i].left, None, self.tabs[i].shell_slot);
            self.tabs[i].look = look.clone();
            let id = self.tabs[i].id;
            for k in 0..self.tabs.len() {
                if self.tabs[k].parent == Some(id) && self.tabs[k].tint.is_none() {
                    self.tabs[k].look = self.look_for(&self.tabs[k].left, Some(&look));
                }
            }
        }
        let theme = self.theme.clone();
        for tab in &mut self.tabs {
            Self::fit_palette(&theme, tab);
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nus_render::policy::contrast;
    use nus_render::theme::hex;

    #[test]
    fn the_family_is_rich_on_paper_and_tinted_on_ink() {
        let red = hex(0xc8102e);
        let paper = family(red, false);
        let ink = family(red, true);
        assert_eq!(paper.len(), SLOTS);
        for (p, i) in paper.iter().zip(&ink) {
            let (pb, ib) = (p.bg.unwrap(), i.bg.unwrap());
            // Dark text reads on the rich paper colors, light text on the ink ones.
            assert!(contrast(pb, hex(0x141414)) >= 7.0, "{pb:?}");
            assert!(contrast(ib, hex(0xe8e6e1)) >= 7.0, "{ib:?}");
            let (l, c) = (from_srgb(pb), from_srgb(ib));
            assert!(l.l > 0.7 && l.c > 0.05, "rich, not pale: {l:?}");
            assert!(c.l < 0.35, "tinted, not grey-lifted: {c:?}");
        }
        // Distinct: no two slots alike.
        for a in 0..SLOTS {
            for b in a + 1..SLOTS {
                assert_ne!(paper[a].bg, paper[b].bg);
            }
        }
    }

    #[test]
    fn a_grey_signal_makes_a_grey_family() {
        for o in family(hex(0x777777), false) {
            assert!(from_srgb(o.bg.unwrap()).c < 0.02);
        }
    }

    #[test]
    fn folders_keep_their_slot_and_random_avoids_taken_ones() {
        assert_eq!(slot_for_folder("/Users/me/nus"), slot_for_folder("/Users/me/nus/"));
        assert!((slot_for_folder("/x") as usize) < SLOTS);
        let taken: Vec<u8> = (0..SLOTS as u8).filter(|&s| s != 7).collect();
        for _ in 0..20 {
            assert_eq!(slot_at_random(&taken), 7);
        }
        let all: Vec<u8> = (0..SLOTS as u8).collect();
        assert!((slot_at_random(&all) as usize) < SLOTS);
    }
}
