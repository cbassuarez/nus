//! While you were away: what happened in this window's shells while
//! nobody was looking — what an agent (or a long command) ran, what
//! failed, what is still running, what is asking for hands. It shows as
//! a block of rows above the prompt's usual rows, each row opening the
//! shell it names, and goes when you act on one or dismiss it. "Away"
//! is the window unfocused for five minutes, or the time since the last
//! session was saved when the app comes up.

use std::time::{Duration, Instant};

use crate::app::{Action, App, PaletteRow};

/// Away this long, and what happened meanwhile is news.
pub const AWAY: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct News {
    /// Unix seconds: everything since is news. None: nothing to tell.
    pub since: Option<u64>,
    /// When the window lost focus, to know how long it was away.
    pub unfocused_at: Option<Instant>,
    /// The rows, made when `since` changes and every so often after.
    pub rows: Vec<PaletteRow>,
    pub made: Option<Instant>,
}

impl App {
    /// The window's focus changed: leaving starts the clock; coming back
    /// after AWAY sets the news.
    pub(crate) fn news_focus(&mut self, focused: bool) {
        if !focused {
            self.news.unfocused_at = Some(Instant::now());
            return;
        }
        if let Some(at) = self.news.unfocused_at.take() {
            if at.elapsed() >= AWAY {
                let then = crate::journal::now().saturating_sub(at.elapsed().as_secs());
                self.news.since = Some(self.news.since.map(|s| s.min(then)).unwrap_or(then));
                self.news.made = None;
            }
        }
    }

    /// At launch: since the last session was saved.
    pub(crate) fn news_at_launch(&mut self) {
        if let Some(saved) = crate::start::last_saved() {
            self.news.since = Some(saved);
            self.news.made = None;
        }
    }

    pub(crate) fn dismiss_news(&mut self) {
        self.news.since = None;
        self.news.rows.clear();
        self.dirty = true;
    }

    /// The rows, fresh enough: failures first, then what ran long, then
    /// what still runs, then hands waiting. At most six.
    pub(crate) fn news_rows(&mut self) -> Vec<PaletteRow> {
        let Some(since) = self.news.since else { return Vec::new() };
        if self.news.made.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
            return self.news.rows.clone();
        }
        let mut out: Vec<PaletteRow> = Vec::new();
        let entries = crate::journal::since(since);
        let tab_of = |tab: &str, tabs: &[crate::app::Tab]| tabs.iter().position(|t| t.id.to_string() == tab);
        // What failed.
        for e in entries.iter().filter(|e| e.exit.is_some_and(|x| x != 0)) {
            if out.len() >= 6 {
                break;
            }
            let go = tab_of(&e.tab, &self.tabs).map(Action::SwitchTab).unwrap_or_else(|| Action::ShellAt(e.cwd.clone()));
            out.push(PaletteRow { num: "✗".into(), text: format!("{} · exit {} · {} · {}", crate::journal::oneline(&e.cmd), e.exit.unwrap_or(0), crate::journal::when(e.start), crate::plate::tail(&e.cwd)), action: go });
        }
        // What ran long and finished.
        for e in entries.iter().filter(|e| e.exit == Some(0) && e.ms >= 30_000) {
            if out.len() >= 6 {
                break;
            }
            let go = tab_of(&e.tab, &self.tabs).map(Action::SwitchTab).unwrap_or_else(|| Action::ShellAt(e.cwd.clone()));
            out.push(PaletteRow { num: "✓".into(), text: format!("{} · {} · {} · {}", crate::journal::oneline(&e.cmd), crate::journal::took(e.ms), crate::journal::when(e.start), crate::plate::tail(&e.cwd)), action: go });
        }
        // What still runs, here and in holders.
        let running: Vec<(usize, String)> = self
            .tabs
            .iter()
            .enumerate()
            .filter_map(|(i, t)| match &t.left {
                crate::app::Pane::Term(tp) if tp.running_at.is_some() => Some((i, tp.title.clone())),
                _ => None,
            })
            .collect();
        for (i, title) in running {
            if out.len() >= 6 {
                break;
            }
            out.push(PaletteRow { num: "▶".into(), text: format!("{title} · still running"), action: Action::SwitchTab(i) });
        }
        for info in self.held_loose() {
            if out.len() >= 6 {
                break;
            }
            out.push(PaletteRow { num: "▶".into(), text: format!("{} · held · still running", info.program), action: Action::AttachHeld(info.id.clone()) });
        }
        // Hands waiting for an answer.
        for (i, t) in self.tabs.iter().enumerate() {
            if out.len() >= 6 {
                break;
            }
            let asks = std::iter::once(&t.left).chain(t.right.as_ref()).find_map(|p| match p {
                crate::app::Pane::Web(w) => w.hands.ask.as_ref().map(|a| a.who.clone()),
                _ => None,
            });
            if let Some(who) = asks {
                out.push(PaletteRow { num: "✋".into(), text: format!("{who} asks for hands · {}", t.title()), action: Action::SwitchTab(i) });
            }
        }
        self.news.rows = out.clone();
        self.news.made = Some(Instant::now());
        out
    }
}
