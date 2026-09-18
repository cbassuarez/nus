//! Cut off: the command a restart killed comes back. Session restore lays
//! the scrollback down dimmed above the fresh prompt; a block with a start
//! and no finish at save time is drawn as a ruled line at the seam with one
//! icon chip on it, chosen by the command — resume for claude and codex,
//! run again for a server, reconnect for ssh — its tooltip saying what it
//! will do. TERMINAL · CUT OFF: CHIP · RUN AGAIN · OFF. Rules: `on_cutoff(b)
//! → { label, cmd }`. A held shell never has one: nothing was cut.

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, TermPane};
use nus_render::theme::metric as m;

/// How a cut-off command comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Resume,
    RunAgain,
    Reconnect,
}

#[derive(Clone, Debug)]
pub struct CutOff {
    pub cmd: String,
    /// Unix seconds when it started.
    pub at: u64,
    pub kind: Kind,
    /// What the chip types.
    pub resume: String,
    pub label: String,
    /// The absolute line the seam is drawn on: the first live line.
    pub line: u64,
}

/// The program a command line runs: `claude`, from `claude -p …` or
/// `C:\…\claude.cmd --resume x`.
pub fn program(cmd: &str) -> String {
    let first = cmd.trim().split_whitespace().next().unwrap_or("");
    let base = first.rsplit(['/', '\\']).next().unwrap_or(first);
    let base = base.trim_matches('"').trim_matches('\'');
    let lower = base.to_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".ps1"] {
        if let Some(s) = lower.strip_suffix(ext) {
            return s.to_string();
        }
    }
    lower
}

/// A command as one line: the grid wraps long ones, and `command_text`
/// keeps the wrap as a newline.
pub fn oneline(cmd: &str) -> String {
    cmd.chars().filter(|c| !matches!(c, '\n' | '\r')).collect::<String>().trim().to_string()
}

/// The assistants nus knows by name, for attention and resume.
pub fn is_assistant(program: &str) -> bool {
    matches!(program, "claude" | "codex" | "copilot" | "aider" | "ollama" | "opencode" | "gemini" | "amp" | "goose" | "cursor-agent")
}

/// Claude's session files live under `~/.claude/projects/<cwd slug>/`,
/// one `<uuid>.jsonl` each; the newest one that postdates the block is it.
fn claude_session(cwd: &str, at: u64) -> Option<String> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    let dir = std::path::Path::new(&home).join(".claude").join("projects").join(crate::journal::slug(cwd));
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_suffix(".jsonl") else { continue };
        let Ok(mtime) = e.metadata().and_then(|md| md.modified()) else { continue };
        let secs = mtime.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if secs + 5 < at {
            continue;
        }
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, id.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

/// What the chip should type for this command, and what to call it.
pub fn resume_for(cmd: &str, cwd: &str, at: u64) -> (Kind, String, String) {
    let cmd = oneline(cmd);
    let cmd = cmd.as_str();
    match program(cmd).as_str() {
        "claude" => match claude_session(cwd, at) {
            Some(id) => (Kind::Resume, format!("claude --resume {id}"), "resume claude".into()),
            None => (Kind::Resume, "claude --continue".into(), "resume claude".into()),
        },
        "codex" => (Kind::Resume, "codex resume --last".into(), "resume codex".into()),
        "ssh" | "mosh" | "et" => (Kind::Reconnect, cmd.to_string(), "reconnect".into()),
        _ => (Kind::RunAgain, cmd.to_string(), "run again".into()),
    }
}

impl TermPane {
    /// The last `max` lines of the screen and scrollback up to the cursor,
    /// trailing blank lines dropped: what restore lays down as history.
    pub fn snapshot_text(&self, max: usize) -> String {
        let grid = self.term.grid();
        let cur = self.term.cursor();
        let end = grid.abs_row(cur.row) + 1;
        let start = end.saturating_sub(max as u64).max(grid.oldest_abs());
        let mut lines: Vec<String> = (start..end).filter_map(|l| grid.row_abs(l)).map(|r| r.text().trim_end().to_string()).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }
}

impl App {
    /// Lay a saved snapshot down as dim history, and the seam after it.
    pub(crate) fn lay_snapshot(&mut self, t: &mut TermPane, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let mut bytes = Vec::with_capacity(text.len() + 64);
        bytes.extend_from_slice(b"\x1b[2m");
        for line in text.lines() {
            bytes.extend_from_slice(line.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"\x1b[0m");
        t.term.advance(&bytes);
    }

    /// The seam and its chip, drawn with the block layer.
    pub(crate) fn draw_cutoff(&mut self, scene: &mut Scene, p: &mut TermPane, r: Rect) {
        p.cutoff_hit = None;
        let Some(c) = p.cutoff.clone() else { return };
        let Some(row) = p.row_of_line(c.line) else { return };
        let t = self.theme.clone();
        let (_, ch) = p.grid.cell_size();
        let y = p.origin.1 + row as f32 * ch - self.px(3.0);
        let ink = t.ink;
        scene.hline(r.x + self.px(18.0), y, r.w - self.px(36.0), self.px(m::HAIRLINE), fade(ink, 0.35));
        // The chip: a small square at the seam's right end, the icon inside.
        let sz = self.px(18.0);
        let chip = Rect::new(r.right() - self.px(18.0) - sz, y - sz / 2.0, sz, sz);
        let (mx, my) = self.mouse;
        let hovered = chip.contains(mx, my);
        scene.rect(chip, self.paper());
        scene.outline(chip, self.px(m::HAIRLINE), fade(ink, if hovered { 0.9 } else { 0.5 }));
        let icon = match c.kind {
            Kind::Resume => nus_render::text::icons::UNDO,
            Kind::RunAgain => nus_render::text::icons::RELOAD,
            Kind::Reconnect => nus_render::text::icons::LINK,
        };
        let ipx = self.px(11.0);
        let color = if hovered { self.surface.signal } else { ink };
        self.fonts.draw_icon(scene, icon, ipx, chip.x + (sz - ipx) / 2.0, chip.y + (sz - ipx) / 2.0, color);
        if hovered {
            let words = format!("cut off at {} · {} · {}", crate::journal::when(c.at), c.label, c.resume);
            self.tip_words(chip, &words);
        }
        let _ = Style { color: ink, ..self.label() };
        p.cutoff_hit = Some(chip);
    }

    /// The chip was clicked: type what it promised, and the seam is done.
    pub(crate) fn cutoff_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
            let crate::app::Pane::Term(t) = p else { continue };
            if !t.cutoff_hit.is_some_and(|h| h.contains(x, y)) {
                continue;
            }
            if let Some(c) = t.cutoff.take() {
                let _ = t.pty.write(format!("{}\r", c.resume).as_bytes());
                self.play_event("toggle");
                self.dirty = true;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_and_resumes() {
        assert_eq!(program("claude -p 'hi'"), "claude");
        assert_eq!(program("C:\\Users\\x\\claude.cmd --resume 1"), "claude");
        assert_eq!(program("  npm run dev"), "npm");
        let (k, cmd, label) = resume_for("codex exec", "/x", 0);
        assert_eq!((k, cmd.as_str(), label.as_str()), (Kind::Resume, "codex resume --last", "resume codex"));
        let (k, cmd, _) = resume_for("ssh box", "/x", 0);
        assert_eq!((k, cmd.as_str()), (Kind::Reconnect, "ssh box"));
        let (k, cmd, _) = resume_for("npm run dev", "/x", 0);
        assert_eq!((k, cmd.as_str()), (Kind::RunAgain, "npm run dev"));
        let (k, cmd, _) = resume_for("claude", "/nowhere/at/all", u64::MAX);
        assert_eq!((k, cmd.as_str()), (Kind::Resume, "claude --continue"));
    }
}
