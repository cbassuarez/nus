//! Blueprint is the appearance of a fresh profile, and an explicit preset
//! for an existing profile, through the same editable settings.
//!
//! This is not a migration: saved profiles keep their choices until they
//! select it. Each value remains editable afterwards.

use crate::app::App;
use crate::fonts::{Family as FontFamily, Weight};
use crate::settings::{Behavior, Blink, CursorColor, CursorMotion, CursorPrefs, CursorShapePref};
use crate::shell_colors::ShellTint;
use crate::surface::{Surface, TextureKind, TextureOn};
use crate::theme_edit::ThemeEdit;
#[cfg(test)]
use nus_render::theme::hex;

fn configure(
    behavior: &mut Behavior,
    cursor: &mut CursorPrefs,
    surface: &mut Surface,
    palette: &mut ThemeEdit,
) {
    // Use the bundled palette rather than a user's saved theme with the same
    // name. The caller installs the complete appearance after these explicit
    // terminal defaults; ordinary theme selection applies the authored visual pairing.
    let blueprint = crate::themes::stock()
        .into_iter()
        .find(|theme| theme.name == "blueprint")
        .expect("the bundled Blueprint palette exists");
    *palette = blueprint.palette();

    behavior.term_font = FontFamily::ArealMono;
    behavior.term_weight = Weight::Medium;
    behavior.typography.system[1].clear();
    behavior.typography.terminal_size = 14.0;
    behavior.typography.terminal_line = 1.25;
    behavior.typography.terminal_spacing = 0.25;
    // Notes are set the same way.
    behavior.typography.notes_family = FontFamily::ArealMono;
    behavior.typography.notes_weight = Weight::Medium;
    behavior.typography.notes_size = 14.0;
    behavior.typography.notes_line = 1.25;
    behavior.typography.notes_spacing = 0.25;
    behavior.shell_tint = ShellTint::None;
    behavior.highlight = true;
    behavior.blocks = true;

    cursor.shape = CursorShapePref::Underline;
    cursor.weight = 3.0;
    cursor.motion = CursorMotion::Glide;
    cursor.blink = Blink::Never;
    cursor.color = CursorColor::Theme;

    surface.signal = blueprint.surface.signal;
    surface.base = None;
    surface.tint = 0.0;
    surface.texture_kind = TextureKind::None;
    surface.texture = 0.0;
    surface.texture_scale = 5.0;
    surface.texture_on = TextureOn::Panes;
    surface.texture_motion = false;
}

/// Data-only startup defaults. Keep deserialization defaults unchanged so
/// older, partial and salvaged settings retain their existing meaning.
pub(crate) fn fresh_preferences() -> crate::prefs::Prefs {
    let mut behavior = Behavior::default();
    let mut cursor = CursorPrefs::default();
    let mut surface = Surface::default();
    let mut palette = ThemeEdit::default();
    configure(&mut behavior, &mut cursor, &mut surface, &mut palette);
    let theme = crate::themes::stock().into_iter().find(|t| t.name == "blueprint").unwrap();
    surface = theme.surface.clone();
    let visual=theme.visual.as_ref().unwrap();
    visual.apply_behavior_visuals(&mut behavior);
    visual.apply_cursor(&mut cursor);
    behavior.follow_os_theme = false;
    crate::prefs::Prefs {
        schema: crate::prefs::SCHEMA,
        behavior: Some(behavior),
        cursor: Some(cursor),
        motion: Some(crate::anim::Motion { register:visual.motion, ..Default::default() }),
        header: Some(crate::settings::HeaderPrefs { masthead:visual.masthead, ..Default::default() }),
        surface: Some(surface),
        theme: Some(palette),
        load_bar: Some(crate::anim::LoadBar { style: theme.bar, color: theme.bar_color, ..Default::default() }),
        tab_colours: Some(theme.tab_colours.clone()),
        preset_name: Some("blueprint".into()),
        ..Default::default()
    }
}

impl App {
    pub(crate) fn apply_blueprint_terminal(&mut self) {
        configure(
            &mut self.behavior,
            &mut self.cursor,
            &mut self.surface,
            &mut self.theme_edit,
        );
        let theme = crate::themes::stock().into_iter().find(|t| t.name == "blueprint").unwrap();
        self.apply_theme(&theme);
        self.apply_fonts();
        // Refresh existing terminals after applying the explicit font defaults.
        self.rebuild_theme();
        self.save_prefs();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nus_render::theme::Mode;

    #[test]
    fn blueprint_applies_the_selected_terminal_settings_in_both_faces() {
        let mut behavior = Behavior::default();
        let mut cursor = CursorPrefs::default();
        let mut surface = Surface::default();
        let mut palette = ThemeEdit::default();
        behavior.typography.system[1] = "Previously installed font".into();
        surface.base = Some(hex(0xff0000));
        surface.tint = 0.5;
        configure(&mut behavior, &mut cursor, &mut surface, &mut palette);

        assert_eq!(behavior.term_font, FontFamily::ArealMono);
        assert_eq!(behavior.term_weight, Weight::Medium);
        assert!(behavior.typography.system[1].is_empty());
        assert_eq!(behavior.typography.terminal_size, 14.0);
        assert_eq!(behavior.typography.terminal_line, 1.25);
        assert_eq!(behavior.typography.terminal_spacing, 0.25);
        assert_eq!(behavior.shell_tint, ShellTint::None);
        assert!(behavior.highlight && behavior.blocks);
        assert_eq!(cursor.shape, CursorShapePref::Underline);
        assert_eq!(cursor.weight, 3.0);
        assert_eq!(cursor.motion, CursorMotion::Glide);
        assert_eq!(cursor.blink, Blink::Never);
        assert_eq!(cursor.color, CursorColor::Theme);
        assert_eq!(surface.texture_kind, TextureKind::None);
        assert_eq!(surface.texture, 0.0);
        assert_eq!(surface.texture_scale, 5.0);
        assert_eq!(surface.texture_on, TextureOn::Panes);
        assert!(!surface.texture_motion);
        for (mode, paper, ink, caret) in [
            (Mode::Paper, 0x1f5fbf, 0xffffff, 0xffffff),
            (Mode::Ink, 0x1f5fbf, 0xffffff, 0xffffff),
        ] {
            let theme = palette.build(mode, surface.signal);
            assert_eq!(surface.paper(theme.paper), hex(paper));
            assert_eq!(theme.ink, hex(ink));
            assert_eq!(theme.caret, hex(caret));
        }
    }

    #[test]
    fn blueprint_preserves_interface_editor_and_unrelated_preferences() {
        let mut behavior = Behavior::default();
        behavior.follow_os_theme = false;
        behavior.default_profile = 3;
        behavior.ui_font = FontFamily::Victor;
        behavior.ui_weight = Weight::Bold;
        behavior.typography.system = [
            "Interface font".into(),
            "Terminal font".into(),
            "Editor font".into(),
        ];
        behavior.typography.ui_scale = 1.1;
        behavior.typography.editor_family = FontFamily::JetBrains;
        behavior.typography.editor_weight = Weight::Bold;
        behavior.typography.editor_size = 17.0;
        behavior.typography.editor_line = 1.5;
        behavior.predict = false;
        behavior.fold_over = 200;
        let mut cursor = CursorPrefs {
            period: 720,
            hollow_unfocused: false,
            hide_while_typing: false,
            smear: 0.4,
            ..CursorPrefs::default()
        };
        let mut surface = Surface {
            shell_radius: 12.0,
            shell_width: 8.0,
            opacity: 0.8,
            drift: 0.2,
            breath: 0.4,
            ..Surface::default()
        };
        let before_behavior = serde_json::to_value(&behavior).unwrap();
        let before_cursor = serde_json::to_value(&cursor).unwrap();
        let before_surface = serde_json::to_value(&surface).unwrap();
        configure(
            &mut behavior,
            &mut cursor,
            &mut surface,
            &mut ThemeEdit::default(),
        );

        let mut expected_behavior = before_behavior;
        let actual_behavior = serde_json::to_value(&behavior).unwrap();
        for field in [
            "term_font",
            "term_weight",
            "shell_tint",
            "highlight",
            "blocks",
        ] {
            expected_behavior[field] = actual_behavior[field].clone();
        }
        for field in ["terminal_size", "terminal_line", "terminal_spacing"] {
            expected_behavior["typography"][field] = actual_behavior["typography"][field].clone();
        }
        expected_behavior["typography"]["system"][1] = serde_json::json!("");
        assert_eq!(actual_behavior, expected_behavior);

        let mut expected_cursor = before_cursor;
        let actual_cursor = serde_json::to_value(&cursor).unwrap();
        for field in ["shape", "weight", "motion", "blink", "color"] {
            expected_cursor[field] = actual_cursor[field].clone();
        }
        assert_eq!(actual_cursor, expected_cursor);

        let mut expected_surface = before_surface;
        let actual_surface = serde_json::to_value(&surface).unwrap();
        for field in [
            "signal",
            "base",
            "tint",
            "texture_kind",
            "texture",
            "texture_scale",
            "texture_on",
            "texture_motion",
        ] {
            expected_surface[field] = actual_surface[field].clone();
        }
        assert_eq!(actual_surface, expected_surface);
    }

    #[test]
    fn blueprint_uses_existing_serializable_settings_and_is_idempotent() {
        let mut behavior = Behavior::default();
        let mut cursor = CursorPrefs::default();
        let mut surface = Surface::default();
        let mut palette = ThemeEdit::default();
        configure(&mut behavior, &mut cursor, &mut surface, &mut palette);
        let saved = serde_json::to_value((&behavior, &cursor, &surface, &palette)).unwrap();
        (behavior, cursor, surface, palette) = serde_json::from_value(saved.clone()).unwrap();
        configure(&mut behavior, &mut cursor, &mut surface, &mut palette);
        assert_eq!(
            serde_json::to_value((&behavior, &cursor, &surface, &palette)).unwrap(),
            saved
        );
    }
}
