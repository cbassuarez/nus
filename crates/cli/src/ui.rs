//! How the lifecycle commands look: the installer's look (nus.dev's
//! install.sh), so `nus update` and `nus uninstall` read like the command
//! that installed nus. A terminal gets colour, lines redrawn in place and the
//! wordmark settling out of noise; a pipe or a log gets one plain line per
//! step. NO_COLOR is respected; outside a UTF-8 locale the symbols are ASCII.

use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BANNER: [&str; 4] = [
    "   _ __  _   _ ___ ",
    "  | '_ \\| | | / __|",
    "  | | | | |_| \\__ \\",
    "  |_| |_|\\__,_|___/",
];

pub struct Ui {
    pub tty: bool,
    color: bool,
    utf: bool,
    pub cols: usize,
}

pub struct Glyphs {
    pub ok: &'static str,
    pub bad: &'static str,
    pub warn: &'static str,
    pub dot: &'static str,
    pub arrow: &'static str,
    pub more: &'static str,
    rule: &'static str,
    gutter: &'static str,
    spin: &'static [&'static str],
    noise: &'static [&'static str],
}

const UTF: Glyphs = Glyphs {
    ok: "✓",
    bad: "✗",
    warn: "!",
    dot: "·",
    arrow: "→",
    more: "…",
    rule: "─",
    gutter: "│",
    spin: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    noise: &["░", "▒", "▓", "#", "%", "&", "*", "+", "=", "-", ":"],
};
const ASCII: Glyphs = Glyphs {
    ok: "+",
    bad: "x",
    warn: "!",
    dot: "-",
    arrow: "->",
    more: "...",
    rule: "-",
    gutter: "|",
    spin: &["|", "/", "-", "\\"],
    noise: &["#", "%", "&", "*", "+", "=", "-", ":"],
};

impl Ui {
    pub fn new() -> Self {
        let tty = std::io::stdout().is_terminal()
            && std::env::var("TERM").map_or(!cfg!(unix), |t| t != "dumb");
        // A Windows console draws escape codes and these glyphs reliably only in
        // Windows Terminal; elsewhere the locale says.
        let vt = !cfg!(windows) || std::env::var_os("WT_SESSION").is_some();
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_default()
            .to_lowercase();
        let utf = if cfg!(windows) {
            vt
        } else {
            locale.contains("utf-8") || locale.contains("utf8")
        };
        let cols = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse().ok())
            .unwrap_or(100);
        Ui {
            tty: tty && vt,
            color: tty && vt && std::env::var_os("NO_COLOR").is_none(),
            utf,
            cols,
        }
    }

    pub fn g(&self) -> &'static Glyphs {
        if self.utf {
            &UTF
        } else {
            &ASCII
        }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    pub fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    pub fn grey(&self, s: &str) -> String {
        self.paint("90", s)
    }
    pub fn faint(&self, s: &str) -> String {
        self.paint("2;90", s)
    }
    pub fn red(&self, s: &str) -> String {
        let truecolor = std::env::var("COLORTERM").is_ok_and(|c| c == "truecolor" || c == "24bit")
            || std::env::var_os("WT_SESSION").is_some();
        self.paint(
            if truecolor {
                "1;38;2;224;87;76"
            } else {
                "1;31"
            },
            s,
        )
    }
    pub fn ok(&self) -> String {
        self.red(self.g().ok)
    }
    pub fn bad(&self) -> String {
        self.red(self.g().bad)
    }
    pub fn warn(&self) -> String {
        self.bold(self.g().warn)
    }
    pub fn spinner(&self, i: usize) -> String {
        let s = self.g().spin;
        self.grey(s[i % s.len()])
    }

    fn clear(&self) {
        if self.tty {
            print!("\r\x1b[2K");
        }
    }

    /// One step: number, name, detail, mark, time. A path in the home
    /// folder reads from `~`.
    pub fn row(&self, n: &str, name: &str, detail: &str, mark: &str, time: &str) {
        let home = std::env::var("HOME").ok().filter(|h| h.len() > 1);
        let detail = match home.as_deref().and_then(|h| detail.strip_prefix(h)) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => detail.to_string(),
        };
        let detail = detail.as_str();
        self.clear();
        println!(
            "  {}  {}{}  {}{}",
            self.grey(n),
            self.bold(&pad(name, 10)),
            self.grey(&pad(&fit(detail, 52, self.g().more), 52)),
            mark,
            self.grey(&format!("{time:>7}"))
        );
    }

    /// A line under a step: what to do about it.
    pub fn hint(&self, text: &str) {
        println!("        {} {}", self.grey(self.g().arrow), text);
    }

    pub fn rule(&self) {
        println!("  {}", self.faint(&self.g().rule.repeat(81)));
    }

    /// The wordmark, settling out of noise left to right, the tagline and
    /// version beside it once it has.
    pub fn banner(&self, tag: &str, version: &str) {
        if !self.tty || !self.color {
            println!();
            for (k, line) in BANNER.iter().enumerate() {
                let side = match k {
                    1 => format!("   {tag}"),
                    2 => format!("   {version}"),
                    _ => String::new(),
                };
                println!("  {line}{side}");
            }
            println!();
            return;
        }
        let noise = self.g().noise;
        print!("\n\n\n\n\n");
        let mut out = std::io::stdout();
        for f in 0..=42usize {
            let t = f * 35;
            let mut frame = String::from("\x1b[4A");
            for (k, line) in BANNER.iter().enumerate() {
                frame.push_str("\r  ");
                for (c, ch) in line.chars().enumerate() {
                    let at = 150 + (c + 1) * 55 + (k + 1) * 25;
                    if ch == ' ' {
                        frame.push(' ');
                    } else if t < at {
                        frame.push_str(&self.faint(noise[(f + k * 7 + c * 13) % noise.len()]));
                    } else if t < at + 160 {
                        frame.push_str(&self.red(&ch.to_string()));
                    } else {
                        frame.push_str(&self.bold(&ch.to_string()));
                    }
                }
                if f == 42 && k == 1 {
                    frame.push_str(&format!("   {}", self.bold(tag)));
                }
                if f == 42 && k == 2 {
                    frame.push_str(&format!("   {}", self.grey(version)));
                }
                frame.push_str("\x1b[K\n");
            }
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
            if f < 42 {
                std::thread::sleep(Duration::from_millis(35));
            }
        }
        println!();
    }

    /// Run `command` with its output in a dimmed window of its last four
    /// lines under the step; on success the window folds into the step's
    /// line, on failure all of it stays, with the log's path.
    pub fn stream(
        &self,
        n: &str,
        name: &str,
        doing: &str,
        done: &str,
        command: &mut Command,
    ) -> Result<(), String> {
        let start = Instant::now();
        let log = std::env::temp_dir().join("nus-lifecycle.log");
        let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
        let err = file.try_clone().map_err(|e| e.to_string())?;
        let mut child = command
            .stdin(Stdio::inherit())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| format!("could not start {name}: {e}"))?;
        let mut shown = 0;
        let mut i = 0;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            if self.tty {
                if shown > 0 {
                    print!("\x1b[{shown}A");
                }
                self.row(n, name, doing, &self.spinner(i), &took(start));
                let text = std::fs::read_to_string(&log).unwrap_or_default();
                let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                let tail = &lines[lines.len().saturating_sub(4)..];
                for l in tail {
                    let l: String = l.chars().take(self.cols.saturating_sub(12)).collect();
                    println!(
                        "\r\x1b[2K      {}",
                        self.faint(&format!("{} {l}", self.g().gutter))
                    );
                }
                shown = tail.len() + 1;
                let _ = std::io::stdout().flush();
            }
            i += 1;
            std::thread::sleep(Duration::from_millis(100));
        };
        if self.tty && shown > 0 {
            print!("\x1b[{shown}A\x1b[J");
        }
        if !status.success() {
            self.row(n, name, doing, &self.bad(), &took(start));
            for l in std::fs::read_to_string(&log).unwrap_or_default().lines() {
                println!("      {} {l}", self.faint(self.g().gutter));
            }
            return Err(format!("{name} failed; the full log is {}", log.display()));
        }
        self.row(n, name, done, &self.ok(), &took(start));
        Ok(())
    }

    pub fn fail(&self, why: &str) {
        self.clear();
        eprintln!("  {} {why}", self.bad());
    }
}

pub fn took(since: Instant) -> String {
    format!("{:.1}s", since.elapsed().as_secs_f64())
}

/// At most `n` characters, the cut marked.
pub fn fit(s: &str, n: usize, more: &str) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let keep = n.saturating_sub(more.chars().count());
    format!("{}{more}", s.chars().take(keep).collect::<String>())
}

/// Pad to `n` characters (not bytes: `·` is one).
pub fn pad(s: &str, n: usize) -> String {
    let len = s.chars().count();
    if len >= n {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(n - len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_counts_characters() {
        assert_eq!(pad("a · b", 7), "a · b  ");
        assert_eq!(pad("longer than", 4), "longer than");
        assert_eq!(fit("a · b · c", 6, "…"), "a · b…");
        assert_eq!(fit("short", 6, "…"), "short");
    }
}
