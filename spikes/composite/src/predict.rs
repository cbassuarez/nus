//! The command line, lit and predicted, terminal-side — nothing to
//! install in the shell. With prompt marks nus knows where the command
//! begins; it colours the tokens (command, flags, strings, numbers) and
//! offers the most recent history entry that continues what's typed as
//! ghost text after the caret. Right or End at the end of the line
//! accepts it. History persists per profile in profile/history.

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, Pane, TermPane};

/// Token classes on the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Command,
    Flag,
    Str,
    Num,
    Path,
    Plain,
    Op,
}

/// Split a command line into (start, len, class) over chars.
pub fn tokens(line: &str) -> Vec<(usize, usize, Tok)> {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut first = true;
    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let class;
        if c == '"' || c == '\'' {
            let q = c;
            i += 1;
            while i < n && chars[i] != q {
                i += 1;
            }
            i = (i + 1).min(n);
            class = Tok::Str;
        } else if "|&;<>()".contains(c) {
            while i < n && "|&;<>()".contains(chars[i]) {
                i += 1;
            }
            class = Tok::Op;
            // A new command follows a pipe or separator.
            out.push((start, i - start, class));
            first = true;
            continue;
        } else {
            while i < n && !chars[i].is_whitespace() && !"|&;<>()".contains(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            class = if first {
                Tok::Command
            } else if word.starts_with('-') && word.len() > 1 {
                Tok::Flag
            } else if word.chars().all(|c| c.is_ascii_digit() || c == '.') && word.chars().any(|c| c.is_ascii_digit()) {
                Tok::Num
            } else if word.contains('/') || word.contains('\\') || word.starts_with('~') || word.starts_with('.') {
                Tok::Path
            } else {
                Tok::Plain
            };
        }
        out.push((start, i - start, class));
        first = false;
    }
    out
}

fn history_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("history")
}

fn history_file(profile: &str) -> std::path::PathBuf {
    let safe: String = profile.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    history_dir().join(format!("{safe}.txt"))
}

/// The last 2000 commands run under this profile.
pub fn load_history(profile: &str) -> Vec<String> {
    std::fs::read_to_string(history_file(profile))
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect::<Vec<_>>())
        .map(|mut v| {
            if v.len() > 2000 {
                v.drain(..v.len() - 2000);
            }
            v
        })
        .unwrap_or_default()
}

pub fn append_history(profile: &str, cmd: &str) {
    let cmd = cmd.trim();
    if cmd.is_empty() || cmd.contains('\n') {
        return;
    }
    let _ = std::fs::create_dir_all(history_dir());
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(history_file(profile)) {
        let _ = writeln!(f, "{cmd}");
    }
}

impl TermPane {
    /// What's typed at the prompt right now, from the B mark to the caret,
    /// and where it starts (col), if the shell is at a prompt.
    pub fn typed(&self) -> Option<(usize, String)> {
        if !self.term.at_prompt() {
            return None;
        }
        let b = self.term.marks.last().filter(|m| m.kind == nus_vt::MarkKind::CommandStart)?;
        let grid = self.term.grid();
        let cur = self.term.cursor();
        if grid.abs_row(cur.row) != b.line || grid.display_offset != 0 {
            return None;
        }
        let row = grid.row(cur.row);
        let text: String = row.cells.iter().skip(b.col).take(cur.col.saturating_sub(b.col)).map(|c| c.ch).collect();
        Some((b.col, text))
    }

    /// The history entry that continues what's typed, and the rest of it.
    pub fn prediction(&self) -> Option<String> {
        let (_, typed) = self.typed()?;
        let typed = typed.trim_start();
        if typed.is_empty() {
            return None;
        }
        // Newest first: this pane's marks, then the file.
        let from_marks = self.term.marks.iter().rev().filter(|m| m.kind == nus_vt::MarkKind::CommandStart).map(|m| self.term.command_text(m));
        let hit = from_marks.chain(self.history.iter().rev().cloned()).find(|h| h.len() > typed.len() && h.starts_with(typed))?;
        Some(hit[typed.len()..].to_string())
    }
}

impl App {
    /// Colour the command line's tokens and draw the ghost prediction.
    pub(crate) fn draw_prompt_line(&mut self, scene: &mut Scene, p: &TermPane, paper: nus_render::Color) {
        if !self.behavior.shell_integration || (!self.behavior.highlight && !self.behavior.predict && self.behavior.prompt_lsp == crate::settings::PromptLsp::Off) {
            return;
        }
        let Some((col0, typed)) = p.typed() else { return };
        let (cw, ch) = p.grid.cell_size();
        let cur = p.term.cursor();
        let y = p.origin.1 + cur.row as f32 * ch;
        let base = y + p.grid.metrics.baseline;
        let font = p.grid.font;
        let px = p.grid.px;
        let t = self.theme.clone();
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        // The shell's own grammar (tree-sitter) colours the line; the regex
        // tokens fill in flags and paths, and stand in for shells without one.
        let lang = self.profiles.get(p.profile).map(|pr| crate::shell::kind_of(&pr.program)).map(|k| match k {
            crate::shell::Kind::PowerShell => "powershell",
            crate::shell::Kind::Bash | crate::shell::Kind::Zsh | crate::shell::Kind::Fish | crate::shell::Kind::Other => "bash",
            _ => "",
        }).unwrap_or("");
        if self.behavior.highlight && !typed.trim().is_empty() {
            let spans = if lang.is_empty() { tokens(&typed) } else { crate::syntax::command_line(lang, &typed) };
            for (start, len, class) in spans {
                let color = match class {
                    Tok::Command => ansi(4),
                    Tok::Flag => ansi(6),
                    Tok::Str => ansi(2),
                    Tok::Num => ansi(5),
                    Tok::Op => ansi(3),
                    Tok::Path | Tok::Plain => continue,
                };
                let text: String = typed.chars().skip(start).take(len).collect();
                let x = p.origin.0 + (col0 + start) as f32 * cw;
                // Paint over the ink glyphs: paper first, then the coloured run.
                scene.rect(Rect::new(x, y, len as f32 * cw, ch), paper);
                self.fonts.draw(scene, Style { font, px, color, tracking: 0.0 }, x, base, &text);
            }
        }
        let mut history_ghost = false;
        if self.behavior.predict {
            if let Some(rest) = p.prediction() {
                let x = p.origin.0 + cur.col as f32 * cw;
                let cols_left = p.term.cols().saturating_sub(cur.col);
                let rest: String = rest.chars().take(cols_left).collect();
                if !rest.is_empty() {
                    self.fonts.draw(scene, Style { font, px, color: fade(t.ink, 0.38), tracking: 0.0 }, x, base, &rest);
                    history_ghost = true;
                }
            }
        }
        self.draw_prompt_lsp(scene, p, col0, &typed, history_ghost);
    }

    /// Right or End at the end of the line accepts the prediction. Returns
    /// true when it did (and the key must not reach the shell).
    pub(crate) fn accept_prediction(&mut self, key: nus_vt::input::Key) -> bool {
        if !self.behavior.predict || !self.behavior.shell_integration {
            return false;
        }
        if !matches!(key, nus_vt::input::Key::Right | nus_vt::input::Key::End) {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        let Some(rest) = t.prediction() else { return false };
        // Only when the caret is at the end of what's typed.
        let grid = t.term.grid();
        let cur = t.term.cursor();
        let row = grid.row(cur.row);
        let tail: String = row.cells.iter().skip(cur.col).map(|c| c.ch).collect();
        if !tail.trim().is_empty() {
            return false;
        }
        let _ = t.pty.write(rest.as_bytes());
        self.dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        let t = tokens("git commit -m \"fix it\" --amend | wc -l 42 ./src");
        let classes: Vec<Tok> = t.iter().map(|x| x.2).collect();
        assert_eq!(classes, vec![Tok::Command, Tok::Plain, Tok::Flag, Tok::Str, Tok::Flag, Tok::Op, Tok::Command, Tok::Flag, Tok::Num, Tok::Path]);
    }
}
