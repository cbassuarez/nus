//! Independent PiP entry and return preferences. No focus event owns the window.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    LeaveApp,
    LeaveTab,
    FocusApp,
    ClickApp,
    RestoreWindow,
    FocusTab,
}
impl Event {
    pub fn label(self) -> &'static str {
        match self {
            Self::LeaveApp => "open when leaving the app",
            Self::LeaveTab => "open when leaving a video tab",
            Self::FocusApp => "close on app focus",
            Self::ClickApp => "close on window click",
            Self::RestoreWindow => "close on window restore",
            Self::FocusTab => "close on video tab return",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub leave_app: bool,
    pub leave_tab: bool,
    pub focus_app: bool,
    pub click_app: bool,
    pub restore_window: bool,
    pub focus_tab: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            leave_app: true,
            leave_tab: true,
            focus_app: false,
            click_app: false,
            restore_window: false,
            focus_tab: false,
        }
    }
}
impl Policy {
    pub fn get(&self, event: Event) -> bool {
        match event {
            Event::LeaveApp => self.leave_app,
            Event::LeaveTab => self.leave_tab,
            Event::FocusApp => self.focus_app,
            Event::ClickApp => self.click_app,
            Event::RestoreWindow => self.restore_window,
            Event::FocusTab => self.focus_tab,
        }
    }
    pub fn set(&mut self, event: Event, on: bool) {
        *match event {
            Event::LeaveApp => &mut self.leave_app,
            Event::LeaveTab => &mut self.leave_tab,
            Event::FocusApp => &mut self.focus_app,
            Event::ClickApp => &mut self.click_app,
            Event::RestoreWindow => &mut self.restore_window,
            Event::FocusTab => &mut self.focus_tab,
        } = on;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_profiles_keep_pip_on_return() {
        let p: Policy = serde_json::from_str("{}").unwrap();
        assert!(p.leave_app && p.leave_tab);
        assert!(!p.focus_app && !p.click_app && !p.restore_window && !p.focus_tab);
    }
    #[test]
    fn return_controls_are_independent_and_persist() {
        for event in [
            Event::FocusApp,
            Event::ClickApp,
            Event::RestoreWindow,
            Event::FocusTab,
        ] {
            let mut p = Policy::default();
            p.set(event, true);
            let p: Policy = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
            assert!(p.get(event));
            assert!(p.leave_app);
            assert_eq!(
                [p.focus_app, p.click_app, p.restore_window, p.focus_tab]
                    .into_iter()
                    .filter(|v| *v)
                    .count(),
                1
            );
        }
    }
}
