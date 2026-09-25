//! Keyboard and menus share the focused surface's zoom, separate from DPI.
use crate::app::{App, KeyIn, Pane};
use winit::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};

pub fn shortcut(ev: &KeyIn, mods: ModifiersState) -> Option<i32> {
    let primary = if cfg!(target_os="macos") { mods.super_key() } else { mods.control_key() };
    if !primary || mods.alt_key() { return None; }
    match &ev.logical_key {
        Key::Character(c) if c == "+" || c == "=" => Some(1),
        Key::Character(c) if c == "-" && !mods.shift_key() => Some(-1),
        Key::Character(c) if c == "0" && !mods.shift_key() => Some(0),
        _ => match ev.physical_key {
            PhysicalKey::Code(KeyCode::Equal | KeyCode::NumpadAdd) => Some(1),
            PhysicalKey::Code(KeyCode::Minus | KeyCode::NumpadSubtract) if !mods.shift_key() => Some(-1),
            PhysicalKey::Code(KeyCode::Digit0 | KeyCode::Numpad0) if !mods.shift_key() => Some(0),
            _ => None,
        }
    }
}

pub fn next(current: u32, step: i32, reset: u32, min: u32, max: u32) -> u32 {
    const STEPS: &[u32] = &[25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500];
    if step == 0 { return reset.clamp(min, max); }
    if step > 0 { STEPS.iter().copied().find(|n| *n > current).unwrap_or(max).clamp(min, max) }
    else { STEPS.iter().copied().rev().find(|n| *n < current).unwrap_or(min).clamp(min, max) }
}

impl App {
    pub(crate) fn tend_zoom(&mut self) {
        let reduced = self.motion.reduced();
        for tab in &self.tabs {
            for pane in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = pane {
                    self.dirty |= w.tab.tick_zoom(reduced);
                    if let Some(d) = &w.devtools { self.dirty |= d.tick_zoom(reduced); }
                }
            }
        }
        if let Some(l) = &self.little { self.dirty |= l.pane.tab.tick_zoom(reduced); }
    }

    pub(crate) fn zoom_focused(&mut self, step: i32) {
        let term_px = self.terminal_px();
        let duration = self.motion.dur(160.0);
        let Some(pane) = self.tabs.get_mut(self.active).map(|t|t.focused()) else { return; };
        let percent = match pane {
            Pane::Web(w) => {
                let target = if w.focus_devtools { w.devtools.as_ref().unwrap_or(&w.tab) } else { &w.tab };
                let current = target.zoom_percent();
                let value = next(current, step, crate::sites::default_zoom(), 25, 500);
                target.zoom_to(value, duration);
                if !w.focus_devtools {
                    let name = crate::sites::host_of(&w.tab.shared.borrow().url);
                    if !name.is_empty() { let mut prefs = crate::sites::prefs(&name); prefs.zoom = value; crate::sites::set(&name, prefs); }
                }
                value
            }
            Pane::Term(t) => {
                t.zoom = next(t.zoom, step, 100, 50, 300);
                t.grid.set_font(&self.fonts, self.f.term, term_px * t.zoom as f32 / 100.0);
                t.grid.set_spacing(&self.fonts, self.behavior.typography.terminal_line, self.behavior.typography.terminal_spacing * self.scale * t.zoom as f32 / 100.0);
                t.view_key = None;
                t.zoom
            }
            Pane::Editor(e) => { e.zoom = next(e.zoom, step, 100, 50, 300); e.zoom }
            _ => { self.ui_zoom = next(self.ui_zoom, step, 100, 75, 200); self.ui_zoom }
        };
        self.layout(); self.apply_term_resizes(true); self.dirty = true;
        self.notice(nus_render::text::icons::SEARCH, "Zoom", format!("{percent}%"));
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn steps_clamp_and_reset_to_user_default() {
        assert_eq!(next(110, 1, 100, 25, 500), 125);
        assert_eq!(next(125, -1, 100, 25, 500), 110);
        assert_eq!(next(500, 1, 100, 25, 500), 500);
        assert_eq!(next(75, -1, 100, 75, 200), 75);
        assert_eq!(next(300, 0, 125, 25, 500), 125);
    }
    #[test] fn physical_and_logical_keys_include_plus_and_numpad() {
        let primary=if cfg!(target_os="macos") {ModifiersState::SUPER} else {ModifiersState::CONTROL};
        for (code,text,shift,expected) in [(KeyCode::Equal,"=",false,Some(1)),(KeyCode::Equal,"+",true,Some(1)),(KeyCode::Minus,"-",false,Some(-1)),(KeyCode::Digit0,"0",false,Some(0)),(KeyCode::NumpadAdd,"",false,Some(1)),(KeyCode::Minus,"_",true,None)] {
            let ev=KeyIn{physical_key:PhysicalKey::Code(code),logical_key:Key::Character(text.into()),text:None,state:winit::event::ElementState::Pressed,repeat:false};
            let mods=primary|if shift{ModifiersState::SHIFT}else{ModifiersState::empty()};
            assert_eq!(shortcut(&ev,mods),expected);assert_eq!(shortcut(&ev,mods|ModifiersState::ALT),None);
        }
    }
}
