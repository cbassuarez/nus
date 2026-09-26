//! The command line, lit and predicted, terminal-side — nothing to
//! install in the shell. With prompt marks nus knows where the command
//! begins; it colours the tokens (command, flags, strings, numbers) and
//! offers the most recent history entry that continues what's typed as
//! ghost text after the caret. Right or Ctrl+F accepts the full suggestion;
//! Option+Right accepts the next shell token. History persists per profile in profile/history.

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, App, TermPane};

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
            } else if word.chars().all(|c| c.is_ascii_digit() || c == '.')
                && word.chars().any(|c| c.is_ascii_digit())
            {
                Tok::Num
            } else if word.contains('/')
                || word.contains('\\')
                || word.starts_with('~')
                || word.starts_with('.')
            {
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
    std::env::current_dir()
        .unwrap_or_default()
        .join("profile")
        .join("history")
}

fn history_file(profile: &str) -> std::path::PathBuf {
    let safe: String = profile
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    history_dir().join(format!("{safe}.txt"))
}

/// The last 2000 commands run under this profile.
pub fn load_history(profile: &str) -> Vec<String> {
    crate::storage::tail(&history_file(profile), crate::storage::HISTORY_FILE)
        .map(|s| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        })
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
    let _ = crate::storage::append_line(
        &history_file(profile),
        cmd,
        crate::storage::HISTORY_FILE,
        crate::storage::HISTORY_LINES,
    );
}

impl TermPane {
    /// The prompt prefix in bytes; complete command and grid positions live
    /// in PromptLine so callers do not confuse characters with columns.
    pub fn typed(&self) -> Option<(usize, String)> {
        let line = self.prompt_line()?;
        Some((line.start.1, line.prefix().to_string()))
    }

    pub fn prediction(&self) -> Option<String> {
        let line = self.prompt_line()?;
        if !line.at_end() || line.text.trim().is_empty() {
            return None;
        }
        self.prompt_history_entries().find_map(|h| {
            if !crate::prompt_code::safe_text(&h) {
                return None;
            }
            h.strip_prefix(&line.text)
                .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                .map(str::to_string)
        })
    }
}

impl App {
    pub(crate) fn draw_prompt_line(
        &mut self,
        scene: &mut Scene,
        p: &TermPane,
        paper: nus_render::Color,
        focused: bool,
    ) {
        if !self.behavior.shell_integration {
            return;
        }
        let Some(line) = p.prompt_line() else { return };
        let (cw, ch) = p.grid.cell_size();
        let theme = self.theme.clone();
        let ansi = |i: usize| crate::theme_edit::from_rgb(theme.ansi[i]);
        let lang = self
            .profiles
            .get(p.profile)
            .map(|pr| crate::shell::kind_of(&pr.program))
            .map(|kind| match kind {
                crate::shell::Kind::PowerShell => "powershell",
                crate::shell::Kind::Bash
                | crate::shell::Kind::Zsh
                | crate::shell::Kind::Fish
                | crate::shell::Kind::Other => "bash",
                _ => "",
            })
            .unwrap_or("");
        if self.behavior.highlight && !line.text.trim().is_empty() {
            let spans = if lang.is_empty() {
                tokens(&line.text)
            } else {
                crate::syntax::command_line(lang, &line.text)
            };
            let mut colors = vec![None; line.cells.len()];
            let mut underlines = vec![false; line.cells.len()];
            for (start, len, class) in spans {
                let color = match class {
                    Tok::Command => ansi(4),
                    Tok::Flag => ansi(6),
                    Tok::Str => ansi(2),
                    Tok::Num => ansi(5),
                    Tok::Op => ansi(3),
                    Tok::Path | Tok::Plain => continue,
                };
                for color_at in colors.iter_mut().skip(start).take(len) {
                    *color_at = Some(color);
                }
            }
            for (start, len, kind) in crate::git_complete::spans(&line.text, p.cwd.as_deref()) {
                use crate::git_complete::Kind;
                let color = match kind {
                    Kind::Sub => ansi(5),
                    Kind::Ref => ansi(2),
                    Kind::Remote => ansi(3),
                    Kind::File => theme.ink,
                };
                for i in start..(start + len).min(colors.len()) {
                    colors[i] = Some(color);
                    underlines[i] = kind == Kind::File;
                }
            }
            // Cell-by-cell placement follows wide glyphs, wraps, and the
            // configured tracking instead of shaping a run at zero tracking.
            for (i, cell) in line.cells.iter().enumerate() {
                let Some(color) = colors[i] else { continue };
                let x = p.origin.0 + cell.col as f32 * cw;
                let y = p.origin.1 + cell.row as f32 * ch;
                let glyph = line.text[cell.byte..].chars().next().unwrap().to_string();
                scene.rect(Rect::new(x, y, cell.width as f32 * cw, ch), paper);
                self.fonts.draw(
                    scene,
                    Style {
                        font: p.grid.font,
                        px: p.grid.px,
                        color,
                        tracking: 0.0,
                    },
                    x,
                    y + p.grid.metrics.baseline,
                    &glyph,
                );
                if underlines[i] {
                    scene.hline(
                        x,
                        y + ch - self.px(2.0),
                        cell.width as f32 * cw,
                        self.px(1.0),
                        fade(theme.ink, 0.5),
                    );
                }
            }
        }
        if focused && !self.prompt_composing {
            self.draw_prompt_ghost(scene, p, &line);
            self.draw_prompt_lsp(scene, p, &line);
            self.draw_prompt_code(scene, p, &line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        let t = tokens("git commit -m \"fix it\" --amend | wc -l 42 ./src");
        let classes: Vec<Tok> = t.iter().map(|x| x.2).collect();
        assert_eq!(
            classes,
            vec![
                Tok::Command,
                Tok::Plain,
                Tok::Flag,
                Tok::Str,
                Tok::Flag,
                Tok::Op,
                Tok::Command,
                Tok::Flag,
                Tok::Num,
                Tok::Path
            ]
        );
    }
}
