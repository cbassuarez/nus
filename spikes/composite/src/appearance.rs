//! System appearance selects complete looks. Render polarity is derived by
//! theme_edit::legible; it never selects which saved palette gets edited.
use crate::themes::StockTheme;
use nus_render::Mode;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SystemThemes {
    pub light: Option<StockTheme>,
    pub dark: Option<StockTheme>,
}

impl SystemThemes {
    pub fn remember(&mut self, theme: StockTheme) {
        if theme.resolved().mode == Mode::Ink { self.dark = Some(theme); }
        else { self.light = Some(theme); }
    }
    pub fn choice(&self, dark: bool) -> StockTheme {
        let saved = if dark { &self.dark } else { &self.light };
        saved.clone().unwrap_or_else(|| crate::themes::stock().into_iter()
            .find(|t| t.name == if dark { "blueprint" } else { "folio" }).unwrap())
    }
}

impl crate::app::App {
    pub(crate) fn system_theme_options(&self, dark: bool) -> Vec<(String, crate::settings::Hit, bool)> {
        let current = self.system_themes.choice(dark).name;
        crate::themes::all().into_iter().enumerate().filter_map(|(i, mut t)| {
            if t.port { t.prefers_ink = dark; }
            ((t.resolved().mode == Mode::Ink) == dark).then(|| {
                let selected = t.name == current;
                (t.name, crate::settings::Hit::SystemTheme(dark, i), selected)
            })
        }).collect()
    }
    /// Capture edits before leaving a look; the other system choice survives.
    pub(crate) fn remember_appearance(&mut self) {
        self.system_themes.remember(self.current_theme(&self.preset_name));
    }
    pub(crate) fn follow_system_appearance(&mut self, dark: bool) {
        // A pinned look may differ from an explicitly selected system look.
        // Preserve edits to the remembered look without replacing that choice.
        let slot = if self.theme.mode == Mode::Ink { &self.system_themes.dark } else { &self.system_themes.light };
        if slot.as_ref().is_none_or(|t| t.name == self.preset_name) {
            self.remember_appearance();
        }
        let theme = self.system_themes.choice(dark);
        self.install_theme(&theme);
        self.behavior.follow_os_theme = true;
        self.save_prefs();
    }
    pub(crate) fn choose_system_theme(&mut self, dark: bool, index: usize) {
        let Some(mut t) = crate::themes::all().get(index).cloned() else { return };
        if t.port { t.prefers_ink = dark; }
        if (t.resolved().mode == Mode::Ink) != dark { return; }
        if dark { self.system_themes.dark = Some(t.clone()); } else { self.system_themes.light = Some(t.clone()); }
        if self.behavior.follow_os_theme && self.window.theme().is_some_and(|v| (v == winit::window::Theme::Dark) == dark) {
            self.install_theme(&t);
        }
        self.save_prefs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_choices_preserve_custom_looks_and_have_opposite_polarities() {
        let mut s = SystemThemes::default();
        assert_eq!(s.choice(false).resolved().mode, Mode::Paper);
        assert_eq!(s.choice(true).resolved().mode, Mode::Ink);
        let mut custom = crate::themes::legacy().into_iter().find(|t| t.name == "blueprint").unwrap();
        custom.name = "mine-21".into();
        custom.prefers_ink = false;
        custom.surface.base = Some(nus_render::theme::hex(0x1f5fbf));
        custom.surface.tint = 1.0;
        s.remember(custom);
        let restored: SystemThemes = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(restored.choice(true).name, "mine-21");
        assert!(!restored.choice(true).prefers_ink, "saved source stays Paper even with light text");
        assert_eq!(restored.choice(false).name, "folio");
    }
}

/// A deliberately bounded visual preset. Never serialize all of Behavior here:
/// routing, accessibility overrides, startup, privacy and loading stay independent.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VisualStyle {
    pub ui: crate::fonts::Family,
    pub ui_weight: crate::fonts::Weight,
    pub terminal: crate::fonts::Family,
    pub terminal_weight: crate::fonts::Weight,
    pub typography: crate::fonts::Typography,
    pub home: crate::settings::HomeLook,
    pub artwork: String,
    pub masthead: bool,
    pub motion: f32,
    pub cursor_shape: crate::settings::CursorShapePref,
    pub cursor_weight: f32,
    pub cursor_motion: crate::settings::CursorMotion,
    pub cursor_blink: crate::settings::Blink,
    pub cursor_period: u32,
    pub cursor_glow: f32,
    pub cursor_smear: f32,
}

impl VisualStyle {
    pub fn curated(name: &str) -> Self {
        use crate::fonts::{Family as F, Weight as W, Typography};
        use crate::settings::{HomeLook as H, CursorShapePref as C, CursorMotion as M, Blink};
        // UI, terminal, code and prose are authored separately; mono stays mono
        // where columns carry meaning. Working looks favor still, spacious type.
        let (ui, weight, terminal, editor, notes, size, line, measure, home, art, shape, motion) = match name {
            "blueprint" => (F::Victor,W::Medium,F::ArealMono,F::JetBrains,F::ArealMono,14.0,1.25,76,H::Art,"memphis",C::Underline,0.35),
            "canopy" => (F::Areal,W::Regular,F::ArealMono,F::JetBrains,F::Areal,15.0,1.5,70,H::Art,"pond",C::Beam,0.55),
            "carbon" => (F::Plex,W::Medium,F::JetBrains,F::JetBrains,F::Areal,15.0,1.5,72,H::Line,"memphis",C::Beam,0.15),
            "citron" => (F::ArealSemiMono,W::Bold,F::Plex,F::Plex,F::Areal,15.0,1.4,70,H::Art,"memphis",C::Block,0.25),
            "folio" => (F::Areal,W::Regular,F::Plex,F::JetBrains,F::Areal,16.0,1.6,66,H::Line,"pond",C::Beam,0.25),
            "indigo" => (F::Plex,W::Regular,F::JetBrains,F::JetBrains,F::Victor,15.0,1.5,74,H::Line,"brain",C::Underline,0.2),
            "iris" => (F::Victor,W::Medium,F::Victor,F::Victor,F::Areal,15.0,1.5,68,H::Art,"brain",C::Beam,0.55),
            "lagoon" => (F::Areal,W::Medium,F::ArealMono,F::JetBrains,F::Areal,15.0,1.5,72,H::Art,"pond",C::Beam,0.45),
            "ledger" => (F::ArealSemiMono,W::Medium,F::Plex,F::Plex,F::ArealSemiMono,14.0,1.45,80,H::Line,"memphis",C::Underline,0.15),
            _ => (F::ArealSemiMono,W::Bold,F::ArealMono,F::JetBrains,F::Areal,15.0,1.4,70,H::Art,"memphis",C::Block,0.3),
        };
        let quiet = matches!(name,"carbon"|"folio"|"indigo"|"ledger");
        Self { ui, ui_weight:weight, terminal, terminal_weight:if name=="blueprint" {W::Medium} else {W::Regular},
            typography: Typography { terminal_size:14.0, terminal_line:if name=="blueprint" {1.25} else if quiet {1.3} else {1.2},
                terminal_spacing:if name=="blueprint" {0.25} else {0.0}, editor_family:editor, editor_size:14.0,
                editor_line:if quiet {1.45} else {1.3}, notes_family:notes, notes_size:size, notes_line:line,
                notes_spacing:if name=="blueprint" {0.25} else {0.0}, notes_measure:measure, ..Default::default() },
            home, artwork:art.into(), masthead:matches!(name,"folio"|"vermilion"), motion,
            cursor_shape:shape, cursor_weight:if name=="blueprint" {3.0} else {2.0},
            cursor_motion:if quiet {M::Jump} else {M::Glide}, cursor_blink:Blink::Never,
            cursor_period:1200, cursor_glow:if name=="iris" {0.12} else {0.0}, cursor_smear:0.0 }
    }
    pub fn capture(app: &crate::app::App) -> Self {
        Self {ui:app.behavior.ui_font, ui_weight:app.behavior.ui_weight, terminal:app.behavior.term_font,
            terminal_weight:app.behavior.term_weight, typography:app.behavior.typography.clone(),
            home:app.behavior.home_look, artwork:app.behavior.home_art.clone(), masthead:app.header.masthead,
            motion:app.motion.register, cursor_shape:app.cursor.shape, cursor_weight:app.cursor.weight,
            cursor_motion:app.cursor.motion, cursor_blink:app.cursor.blink, cursor_period:app.cursor.period,
            cursor_glow:app.cursor.glow, cursor_smear:app.cursor.smear}
    }
    pub fn apply(&self, app: &mut crate::app::App) {
        self.apply_behavior_visuals(&mut app.behavior);
        app.header.masthead=self.masthead;
        app.motion.register=self.motion.clamp(0.0,1.0);
        self.apply_cursor(&mut app.cursor);
        app.apply_fonts();
    }
    pub fn apply_cursor(&self, cursor: &mut crate::settings::CursorPrefs) {
        cursor.shape=self.cursor_shape; cursor.weight=self.cursor_weight.clamp(1.0,6.0);
        cursor.motion=self.cursor_motion; cursor.blink=self.cursor_blink;
        cursor.period=self.cursor_period.clamp(400,2400);
        cursor.glow=self.cursor_glow.clamp(0.0,1.0); cursor.smear=self.cursor_smear.clamp(0.0,2.0);
    }
    pub fn apply_behavior_visuals(&self, b: &mut crate::settings::Behavior) {
        b.ui_font=self.ui; b.ui_weight=self.ui_weight; b.term_font=self.terminal; b.term_weight=self.terminal_weight;
        b.typography=self.typography.clone(); b.typography.normalize();
        b.home_look=self.home; b.home_art=self.artwork.clone();
    }
}
