//! Translate the window's real activity into the carapace's small, bounded gestures.
use std::collections::HashMap;
use std::time::Instant;

use crate::app::{App, Pane};
use crate::carapace_activity::{Activity, ActivityInput};
use crate::surface::{Material, Reaction, Shell, TextureKind, TextureOn};

#[derive(Default)]
struct Seen {
    command_done: Option<Instant>,
    agent_done: Option<Instant>,
    loading_since: Option<Instant>,
    progress: Option<(u8, u8)>,
}

#[derive(Default)]
pub(crate) struct Carapace {
    pub activity: Activity,
    pub output: u64,
    pub typed: u64,
    /// NUS_SHOT's deterministic visual fixture; ordinary windows leave it empty.
    pub fixture: Option<ActivityInput>,
    completion: u64,
    seen: HashMap<(u64, bool), Seen>,
}

impl App {
    pub(crate) fn carapace_tick(&mut self) {
        use crate::agent::Phase;
        let now = crate::clock::now();
        let mut input = ActivityInput::default();
        // Completion is an event, not the disappearance of work on a tab switch.
        // Only the focused pane's output contributes energy; background panes
        // can leave a held attention mark or announce a meaningful completion.
        self.carapace.seen.retain(|(id, _), _| self.tabs.iter().any(|t| t.id == *id && !t.hatch));
        for (index, tab) in self.tabs.iter().enumerate().filter(|(_, t)| !t.hatch) {
            for (right, pane) in std::iter::once((false, &tab.left)).chain(tab.right.as_ref().map(|p| (true, p))) {
                let focused = index == self.active && right == (tab.focus_right && tab.right.is_some());
                let mut state = Seen::default();
                let old = self.carapace.seen.get(&(tab.id, right));
                if focused { input.identity = tab.id.wrapping_mul(2) + u64::from(right); }
                match pane {
                    Pane::Term(t) => {
                        state.command_done = t.done.filter(|(exit, _)| exit.is_none_or(|code| code == 0)).map(|(_, at)| at);
                        state.agent_done = t.agent.as_ref().filter(|a| a.phase == Phase::Done).map(|a| a.since);
                        state.progress = t.progress;
                        input.attention |= t.waiting || t.agent.as_ref().is_some_and(|a| a.phase == Phase::Waiting)
                            || matches!(t.progress, Some((2 | 4, _)));
                        if focused {
                            input.working = t.agent.as_ref().map_or(t.running_since.is_some(), |a| a.phase == Phase::Working)
                                && !matches!(t.progress, Some((2 | 4, _)) | Some((1, 100)));
                            input.loading = matches!(t.progress, Some((3, _))) || matches!(t.progress, Some((1, p)) if p < 100);
                            input.progress = t.progress.filter(|(kind, _)| matches!(kind, 1 | 2 | 4)).map(|(_, p)| f32::from(p.min(100)) / 100.0);
                        }
                        if let Some(old) = old {
                            if (state.agent_done.is_some() && state.agent_done != old.agent_done)
                                || (t.agent.is_none() && state.command_done.is_some() && state.command_done != old.command_done)
                                || (state.progress == Some((1, 100)) && matches!(old.progress, Some((1, p)) if p < 100))
                            {
                                self.carapace.completion = self.carapace.completion.wrapping_add(1);
                            }
                        }
                    }
                    Pane::Web(w) => {
                        let page = w.tab.shared.borrow();
                        if page.loading {
                            state.loading_since = Some(old.and_then(|s| s.loading_since).unwrap_or(now));
                        } else if page.failed_url.is_none() && page.settled
                            && old.and_then(|s| s.loading_since).is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() > 0.5)
                        {
                            self.carapace.completion = self.carapace.completion.wrapping_add(1);
                        }
                        if focused {
                            input.loading = page.loading;
                            // WebKit's fallback currently reports a synthetic midpoint.
                            // Keep it indeterminate until it supplies real progress.
                            input.progress = (page.loading && page.native.is_none()).then_some(page.progress as f32);
                            input.attention |= page.failed_url.is_some();
                            input.media = w.tab.playing();
                        }
                    }
                    _ => {}
                }
                self.carapace.seen.insert((tab.id, right), state);
            }
        }
        input.output = self.carapace.output;
        input.typed = self.carapace.typed;
        input.completion = self.carapace.completion;
        if let Some(fixture) = self.carapace.fixture { input = fixture; }
        let reaction = if self.surface.material == Material::Plain { Reaction::Still } else { self.surface.reaction };
        let changed = self.carapace.activity.observe(now, input, reaction, self.surface.react_to, self.motion.reduced());
        if changed || self.carapace.activity.animating() {
            self.dirty = true;
        }
    }

    /// Used by the window, the settings proof, and the small PiP carapace.
    pub(crate) fn carapace_look(&self, width: f32, radius: f32) -> nus_render::CarapaceLook {
        let frame = self.carapace.activity.frame();
        nus_render::CarapaceLook {
            material: self.surface.material.render_id(), width, radius,
            signal: self.surface.signal, paper: self.paper(),
            phase: frame.phase, energy: frame.energy, progress: frame.progress,
            attention: frame.attention, band: self.surface.shell == Shell::Band,
            grain: if self.surface.texture_on == TextureOn::Carapace && self.surface.texture_kind == TextureKind::Grain { self.surface.texture } else { 0.0 },
            grain_scale: self.px(self.surface.texture_scale).max(0.25),
        }
    }
}
