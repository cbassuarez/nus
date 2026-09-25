//! The Ledger: assistants, where the tabs already are. A tab whose shell
//! runs claude or codex grows a line or two under its title —
//!
//!   WORKING 4:12          edit vt/src/term.rs
//!   [WAITING 0:04] BASH   run cargo test --workspace?   ALLOW  DENY  ALWAYS
//!   ✓ DONE · 3 FILES 12M       (the check is the CHECK icon)
//!
//! — and the top strip counts them: [1 WAITING] 2 WORKING. What they say
//! comes from agent.rs; the answers are keystrokes into the pane, so the
//! assistant's own prompt and the sidebar are the same question.
//! SIDEBAR · ASSISTANTS IN TAB ROWS turns the lines off (the dot stays).

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::agent::{Agent, Answer, Phase};
use crate::app::{fade, App, SideHit, Tab};
use nus_render::theme::metric as m;

impl Tab {
    /// The assistant this tab shows: one that waits before one that works
    /// before one that is done, either half of a split.
    pub(crate) fn agent(&self) -> Option<&Agent> {
        let rank = |a: &Agent| match a.phase {
            Phase::Waiting => 3,
            Phase::Working => 2,
            Phase::Done => 1,
            Phase::Idle => 0,
        };
        fn one(p: &crate::app::Pane) -> Option<&Agent> {
            match p {
                crate::app::Pane::Term(t) => t.agent.as_ref().filter(|a| a.phase != Phase::Idle),
                _ => None,
            }
        }
        let l = one(&self.left);
        let r = self.right.as_ref().and_then(one);
        match (l, r) {
            (Some(a), Some(b)) => Some(if rank(b) > rank(a) { b } else { a }),
            (a, b) => a.or(b),
        }
    }
}

/// What the Ledger draws for one assistant, so height and drawing agree.
struct Lines {
    words: Option<String>,
    buttons: bool,
}

fn lines(a: &Agent) -> Lines {
    Lines { words: a.line(), buttons: a.phase == Phase::Waiting && a.ask.is_some() && crate::agent::keys(&a.name, Answer::Allow).is_some() }
}

impl App {
    fn ledger_on(&self) -> bool {
        self.behavior.ledger && !self.sidebar_icons()
    }

    /// How much taller a tab's row is for its assistant's lines.
    pub(crate) fn ledger_h(&self, tab: &Tab) -> f32 {
        if !self.ledger_on() {
            return 0.0;
        }
        let Some(a) = tab.agent() else { return 0.0 };
        let l = lines(a);
        let mut h = self.px(18.0);
        if l.words.is_some() {
            h += self.px(19.0);
        }
        if l.buttons {
            h += self.px(32.0);
        }
        h + self.px(8.0)
    }

    /// True when the row's assistant is in the Ledger (the dot gives way).
    pub(crate) fn ledger_shows(&self, tab: &Tab) -> bool {
        self.ledger_on() && tab.agent().is_some()
    }

    /// The lines under a tab's title: from `x`, at `y` (the row's foot), `w` wide.
    pub(crate) fn draw_ledger(&mut self, scene: &mut Scene, tab: &Tab, i: usize, x: f32, y: f32, w: f32) {
        if !self.ledger_on() {
            return;
        }
        let Some(a) = tab.agent() else { return };
        let t = self.theme.clone();
        let ink = t.ink;
        // Attention wears the Space's own signal (DESIGN.md), not the tab's.
        let signal = self.surface.signal;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let l = lines(a);
        let now = crate::clock::now();
        let secs = |at: std::time::Instant| now.saturating_duration_since(at).as_secs();
        // The label line: the phase, filled when it needs you.
        let base = y + self.px(12.0);
        let mut lx = x;
        match a.phase {
            Phase::Waiting => {
                self.want_beat(1000);
                let words = format!("WAITING {}", crate::agent::clock(secs(a.since)));
                let lw = self.fonts.measure(strong, &words);
                let pad = self.px(5.0);
                scene.rect(Rect::new(lx, base - self.px(10.0), lw + 2.0 * pad, self.px(14.0)), signal);
                self.fonts.draw(scene, Style { color: self.on_fill(signal), ..strong }, lx + pad, base, &words);
                lx += lw + 2.0 * pad + self.px(8.0);
                if let Some(ask) = &a.ask {
                    self.fonts.draw(scene, Style { color: t.dim, ..label }, lx, base, &ask.tool.to_uppercase());
                }
            }
            Phase::Working => {
                // The square breathes, as the dot always did.
                let d = self.px(7.0);
                let breath = self.breath();
                scene.rect(Rect::new(lx, base - self.px(7.0), d, d), fade(ink, breath));
                lx += d + self.px(7.0);
                let words = format!("WORKING {}", crate::agent::clock(secs(a.turn.unwrap_or(a.since))));
                self.fonts.draw(scene, Style { color: ink, ..strong }, lx, base, &words);
            }
            Phase::Done => {
                let n = a.touched.len();
                let isz = self.px(11.0);
                self.fonts.draw_icon(scene, nus_render::text::icons::CHECK, isz, lx, base - isz + self.px(1.0), ink);
                lx += isz + self.px(6.0);
                let words = if n == 0 { "DONE".to_string() } else { format!("DONE · {n} FILE{}", if n == 1 { "" } else { "S" }) };
                lx += self.fonts.draw(scene, Style { color: ink, ..strong }, lx, base, &words) + self.px(8.0);
                self.fonts.draw(scene, Style { color: t.dim, ..label }, lx, base, &crate::agent::ago(secs(a.since)).to_uppercase());
            }
            Phase::Idle => {}
        }
        let mut cy = y + self.px(18.0);
        // The words: the question in ink, what it is doing in dim.
        if let Some(words) = &l.words {
            let st = Style { color: if a.phase == Phase::Waiting { ink } else { t.dim }, ..ui };
            let text = self.fit(st, words, w);
            self.fonts.draw(scene, st, x, cy + self.px(14.0), &text);
            cy += self.px(19.0);
        }
        // The answers, as the assistant's own prompt offers them.
        if l.buttons {
            let bh = self.px(24.0);
            let by = cy + self.px(4.0);
            let mut bx = x;
            let (mx, my) = self.mouse;
            for (k, (word, answer)) in [("ALLOW", Answer::Allow), ("DENY", Answer::Deny), ("ALWAYS", Answer::Always)].into_iter().enumerate() {
                let tw = self.fonts.measure(strong, word);
                let r = Rect::new(bx, by, tw + self.px(16.0), bh);
                if r.right() > x + w {
                    break;
                }
                let hot = r.contains(mx, my);
                let first = k == 0;
                if first {
                    scene.rect(r, if hot { signal } else { ink });
                } else if hot {
                    scene.rect(r, t.tint);
                }
                scene.outline(r, self.px(m::HAIRLINE), ink);
                let color = if first { self.on_fill(if hot { signal } else { ink }) } else { ink };
                self.fonts.draw(scene, Style { color, ..strong }, r.x + self.px(8.0), r.y + bh / 2.0 + self.px(4.0), word);
                self.side_hits.push((r, SideHit::Answer(i, answer)));
                bx = r.right() + self.px(6.0);
            }
        }
    }

    /// The strip's count: (waiting, working) assistants in this window.
    pub(crate) fn agent_counts(&self) -> (usize, usize) {
        self.tabs.iter().filter_map(|t| t.agent()).fold((0, 0), |(w, k), a| match a.phase {
            Phase::Waiting => (w + 1, k),
            Phase::Working => (w, k + 1),
            _ => (w, k),
        })
    }

    /// The strip's count, drawn leftward from `rx`: [1 WAITING] 2 WORKING.
    /// Returns where it ends and where it was, for the click.
    pub(crate) fn draw_agent_counts(&mut self, scene: &mut Scene, rx: f32, strip: Rect, lbase: f32) -> Option<(f32, Rect)> {
        let (waiting, working) = self.agent_counts();
        if waiting + working == 0 {
            return None;
        }
        let t = self.theme.clone();
        let strong = self.label_strong();
        let label = self.label();
        let signal = self.surface.signal;
        let mut x = rx;
        let right = rx;
        if working > 0 {
            let words = format!("{working} WORKING");
            x -= self.fonts.measure(label, &words);
            self.fonts.draw(scene, Style { color: t.ink, ..label }, x, lbase, &words);
            x -= self.px(12.0);
        }
        if waiting > 0 {
            let words = format!("{waiting} WAITING");
            let pad = self.px(5.0);
            let lw = self.fonts.measure(strong, &words);
            x -= lw + 2.0 * pad;
            scene.rect(Rect::new(x, lbase - self.px(10.0), lw + 2.0 * pad, self.px(14.0)), signal);
            self.fonts.draw(scene, Style { color: self.on_fill(signal), ..strong }, x + pad, lbase, &words);
        }
        let hit = Rect::new(x - self.px(4.0), strip.y, right - x + self.px(8.0), strip.h);
        if hit.contains(self.mouse.0, self.mouse.1) {
            scene.outline(Rect::new(hit.x, strip.y + self.px(5.0), hit.w, strip.h - self.px(10.0)), self.px(m::HAIRLINE), t.ink);
        }
        Some((x, hit))
    }

    /// The count, clicked: to the assistant that waits, else one at work.
    pub(crate) fn goto_agent(&mut self) {
        let pick = |p: Phase| self.tabs.iter().position(|t| t.agent().is_some_and(|a| a.phase == p));
        if let Some(i) = pick(Phase::Waiting).or_else(|| pick(Phase::Working)) {
            self.activate(i);
            self.dirty = true;
        }
    }
}
