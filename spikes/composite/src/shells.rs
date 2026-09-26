//! The shell list as people meet it. Discovery (nus-pty) finds what is on
//! the machine; this sorts it into shells, your own and other machines,
//! says how much of nus's shell integration each gets, keeps the rarely
//! wanted (sh, dash, csh…) out of the way until asked for, remembers which
//! machines you use, and offers the good shells that are missing with the
//! command that installs them, typed into a shell for you to run.
//!
//! Your own shells live in profile/shells.json:
//! `[{ "name": "xonsh", "program": "/usr/local/bin/xonsh", "args": ["--login"],
//!     "cwd": "~/src", "env": { "XONSH_COLOR_STYLE": "default" } }]`
use nus_pty::discover::{base, source_of, QUIET};
use nus_pty::Profile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// On this machine.
    Shell,
    /// Added by you (profile/shells.json).
    Yours,
    /// Elsewhere: an ssh host, a WSL distribution.
    Machine,
}

/// How much of nus's shell integration a shell gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Prompt marks, folder, exit codes: blocks, jumps, run again, the journal.
    Full,
    /// Prompt marks and folder, no exit codes.
    Partial,
    /// Carried over ssh by `nus ssh` (SSH · BRING THE INTEGRATION).
    Remote,
    /// Plain terminal.
    None,
}

impl Level {
    pub fn word(self) -> &'static str {
        match self {
            Level::Full => "FULL",
            Level::Partial => "PARTIAL",
            Level::Remote => "VIA SSH",
            Level::None => "PLAIN",
        }
    }
    pub fn says(self) -> &'static str {
        match self {
            Level::Full => "blocks, prompt jumps, folders, exit codes, run again, the journal",
            Level::Partial => "prompt marks and folders; no exit codes",
            Level::Remote => "nus's scripts go over the same connection: marks, folders, exit codes",
            Level::None => "a plain terminal: nus can't see prompts or folders here",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Custom {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Custom {
    pub fn profile(&self) -> Profile {
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
        let cwd = self.cwd.as_ref().map(|c| if let Some(rest) = c.strip_prefix("~/") { format!("{home}/{rest}") } else { c.clone() });
        Profile { name: self.name.clone(), program: self.program.clone(), args: self.args.clone(), cwd, env: self.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect() }
    }
}

fn custom_path() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("shells.json")
}

pub fn load_custom() -> Vec<Custom> {
    if crate::private::enabled() {
        return Vec::new();
    }
    std::fs::read_to_string(custom_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_custom(list: &[Custom]) -> std::io::Result<()> {
    let path = custom_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(list)?)
}

pub fn machine(p: &Profile) -> bool {
    let b = base(&p.program);
    b == "ssh" || b == "wsl"
}

pub fn level(p: &Profile, ssh_integration: bool) -> Level {
    use crate::shell::Kind;
    if base(&p.program) == "ssh" {
        return if ssh_integration { Level::Remote } else { Level::None };
    }
    match crate::shell::kind_of(&p.program) {
        Kind::Zsh | Kind::Bash | Kind::PowerShell | Kind::Fish | Kind::Nu => Level::Full,
        Kind::Cmd => Level::Partial,
        Kind::Other => Level::None,
    }
}

/// Found, but out of the way until you show it.
pub fn quiet(p: &Profile) -> bool {
    QUIET.contains(&base(&p.program).as_str())
}

/// Split a typed command line into program and arguments: spaces, with
/// "double" or 'single' quotes keeping a part whole.
pub fn split_command(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A shell worth having that isn't here, and the command that gets it.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub shell: &'static str,
    pub why: &'static str,
    pub command: String,
}

/// Which package managers are here, by the tools on the PATH (and where
/// Homebrew lives, since an app from the Dock has a short PATH).
fn have(tool: &str) -> bool {
    let exe = if cfg!(windows) { format!("{tool}.exe") } else { tool.to_string() };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(unix) {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin", "/snap/bin"].iter().map(PathBuf::from));
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Microsoft\WindowsApps"));
        }
    }
    dirs.iter().any(|d| d.join(&exe).is_file() || (cfg!(windows) && d.join(format!("{tool}.cmd")).is_file()))
}

/// Offers for the shells in `wanted` that `present` doesn't have, with the
/// first package manager here that has each.
pub fn offers(present: &[String]) -> Vec<Offer> {
    let has = |b: &str| present.iter().any(|p| p == b);
    let mut out = Vec::new();
    let mut offer = |shell: &'static str, why: &'static str, cmds: &[(&str, String)]| {
        if has(shell) {
            return;
        }
        if let Some((_, c)) = cmds.iter().find(|(tool, _)| have(tool)) {
            out.push(Offer { shell, why, command: c.clone() });
        }
    };
    if cfg!(windows) {
        offer("pwsh", "PowerShell 7: the modern, cross-platform PowerShell", &[("winget", "winget install --id Microsoft.PowerShell -e".into()), ("scoop", "scoop install pwsh".into())]);
        offer("git bash", "bash and the Unix tools, from Git for Windows", &[("winget", "winget install --id Git.Git -e".into()), ("scoop", "scoop install git".into())]);
        offer("nu", "Nushell: structured data in the pipeline", &[("winget", "winget install --id Nushell.Nushell -e".into()), ("scoop", "scoop install nu".into()), ("cargo", "cargo install nu --locked".into())]);
    } else {
        offer("fish", "friendly out of the box: suggestions and colour with no setup", &[("brew", "brew install fish".into()), ("apt", "sudo apt install fish".into()), ("dnf", "sudo dnf install fish".into()), ("pacman", "sudo pacman -S fish".into()), ("zypper", "sudo zypper install fish".into())]);
        offer("nu", "Nushell: structured data in the pipeline", &[("brew", "brew install nushell".into()), ("pacman", "sudo pacman -S nushell".into()), ("cargo", "cargo install nu --locked".into())]);
        offer("pwsh", "PowerShell 7 on this machine too", &[("brew", "brew install --cask powershell".into()), ("snap", "sudo snap install powershell --classic".into())]);
        if cfg!(target_os = "linux") {
            offer("zsh", "zsh: what macOS starts with", &[("apt", "sudo apt install zsh".into()), ("dnf", "sudo dnf install zsh".into()), ("pacman", "sudo pacman -S zsh".into()), ("zypper", "sudo zypper install zsh".into())]);
        }
    }
    out
}

impl App {
    /// After discovery: your own shells, then the default by its name (a
    /// default kept by position alone would move as shells come and go).
    pub(crate) fn load_shells(&mut self) {
        for c in load_custom() {
            let mut p = c.profile();
            if self.profiles.iter().any(|o| o.name == p.name) {
                p.name = format!("{} (yours)", p.name);
            }
            self.custom_shells.push(p.name.clone());
            self.profiles.push(p);
        }
        self.resolve_default_shell();
    }

    pub(crate) fn resolve_default_shell(&mut self) {
        let name = self.behavior.default_shell_name.clone();
        if let Some(i) = (!name.is_empty()).then(|| self.profiles.iter().position(|p| p.name == name)).flatten() {
            self.behavior.default_profile = i;
        } else if let Some(p) = self.profiles.get(self.behavior.default_profile) {
            self.behavior.default_shell_name = p.name.clone();
        } else {
            self.behavior.default_profile = 0;
        }
    }

    /// Look again (after installing one): new shells join the end, so the
    /// shells open now keep their places.
    pub(crate) fn rescan_shells(&mut self) {
        let before = self.profiles.len();
        for p in Profile::discover() {
            if !self.profiles.iter().any(|o| o.name == p.name) {
                self.profiles.push(p);
            }
        }
        for c in load_custom() {
            let p = c.profile();
            match self.profiles.iter().position(|o| o.name == p.name) {
                Some(i) if self.custom_shells.contains(&p.name) => self.profiles[i] = p,
                Some(_) => {}
                None => {
                    self.custom_shells.push(p.name.clone());
                    self.profiles.push(p);
                }
            }
        }
        let n = self.profiles.len() - before;
        self.notice(nus_render::text::icons::TERMINAL, "Shells", if n == 0 { "nothing new on this machine".to_string() } else { format!("{n} new · in Terminal settings and NEW TAB") });
        self.dirty = true;
    }

    pub(crate) fn shell_group(&self, i: usize) -> Group {
        let Some(p) = self.profiles.get(i) else { return Group::Shell };
        if self.custom_shells.contains(&p.name) {
            Group::Yours
        } else if machine(p) {
            Group::Machine
        } else {
            Group::Shell
        }
    }

    pub(crate) fn shell_hidden(&self, i: usize) -> bool {
        let Some(p) = self.profiles.get(i) else { return true };
        if i == self.behavior.default_profile {
            return false;
        }
        self.behavior.shells_hidden.contains(&p.name) || (quiet(p) && !self.behavior.shells_shown.contains(&p.name))
    }

    /// Where a shell came from, in a word: login, homebrew, ssh…
    pub(crate) fn shell_source(&self, i: usize) -> String {
        let Some(p) = self.profiles.get(i) else { return String::new() };
        match self.shell_group(i) {
            Group::Yours => "yours".into(),
            Group::Machine => if base(&p.program) == "ssh" { "ssh".into() } else { "wsl".into() },
            Group::Shell if i == 0 => "login".into(),
            Group::Shell => {
                let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
                source_of(std::path::Path::new(&p.program), home.as_deref()).into()
            }
        }
    }

    /// Every entry for the list: the default first, then shells, your own
    /// and other machines (the ones you use most recently first).
    pub(crate) fn shell_order(&self, with_hidden: bool) -> Vec<usize> {
        let d = self.behavior.default_profile;
        let mut out: Vec<usize> = Vec::new();
        if d < self.profiles.len() {
            out.push(d);
        }
        for g in [Group::Shell, Group::Yours] {
            out.extend((0..self.profiles.len()).filter(|&i| i != d && self.shell_group(i) == g && (with_hidden || !self.shell_hidden(i))));
        }
        let mut machines: Vec<usize> = (0..self.profiles.len()).filter(|&i| i != d && self.shell_group(i) == Group::Machine && (with_hidden || !self.shell_hidden(i))).collect();
        let used = |i: usize| self.profiles.get(i).and_then(|p| self.behavior.shells_used.get(&p.name)).copied().unwrap_or(0);
        machines.sort_by_key(|&i| std::cmp::Reverse(used(i)));
        out.extend(machines);
        out
    }

    /// NEW TAB's fan-out: shells and your own, then the four machines used
    /// last; how many more there are (they're in the palette).
    pub(crate) fn menu_shells(&self) -> (Vec<usize>, usize) {
        let all = self.shell_order(false);
        let mut out = Vec::new();
        let mut machines = 0;
        let mut more = 0;
        for i in all {
            if self.shell_group(i) == Group::Machine && i != self.behavior.default_profile {
                if machines >= 4 {
                    more += 1;
                    continue;
                }
                machines += 1;
            }
            out.push(i);
        }
        more += (0..self.profiles.len()).filter(|&i| self.shell_hidden(i)).count();
        (out, more)
    }

    pub(crate) fn note_shell_used(&mut self, i: usize) {
        if let Some(p) = self.profiles.get(i) {
            self.behavior.shells_used.insert(p.name.clone(), crate::journal::now());
            if self.behavior.shells_used.len() > 64 {
                if let Some(oldest) = self.behavior.shells_used.iter().min_by_key(|(_, t)| **t).map(|(k, _)| k.clone()) {
                    self.behavior.shells_used.remove(&oldest);
                }
            }
        }
    }

    pub(crate) fn set_default_shell(&mut self, i: usize) {
        if let Some(p) = self.profiles.get(i) {
            self.behavior.default_profile = i;
            self.behavior.default_shell_name = p.name.clone();
            self.behavior.shells_hidden.retain(|n| n != &p.name);
        }
    }

    pub(crate) fn toggle_shell_hidden(&mut self, i: usize) {
        let Some(p) = self.profiles.get(i).cloned() else { return };
        if i == self.behavior.default_profile {
            self.notice(nus_render::text::icons::TERMINAL, "Default Shell", "choose another default before hiding this one");
            return;
        }
        if self.shell_hidden(i) {
            self.behavior.shells_hidden.retain(|n| n != &p.name);
            if quiet(&p) && !self.behavior.shells_shown.contains(&p.name) {
                self.behavior.shells_shown.push(p.name.clone());
            }
        } else {
            self.behavior.shells_shown.retain(|n| n != &p.name);
            if !quiet(&p) {
                self.behavior.shells_hidden.push(p.name.clone());
            }
        }
    }

    /// The shells here, by the names install offers know them by.
    fn present_shells(&self) -> Vec<String> {
        let mut v: Vec<String> = self.profiles.iter().map(|p| base(&p.program)).collect();
        if self.profiles.iter().any(|p| p.name.starts_with("git bash")) {
            v.push("git bash".into());
        }
        v
    }

    pub(crate) fn shell_offers(&self) -> Vec<Offer> {
        if crate::private::enabled() {
            return Vec::new();
        }
        offers(&self.present_shells())
    }

    /// Open the default shell with the install command typed, not run:
    /// you read it, then press Enter (some ask for your password).
    pub(crate) fn get_shell(&mut self, k: usize) {
        let Some(o) = self.shell_offers().into_iter().nth(k) else { return };
        match self.new_term_pane(false, self.behavior.default_profile) {
            Ok(mut t) => {
                t.type_at_prompt = Some(o.command.clone());
                let tab = self.make_tab(crate::app::Pane::Term(t), None);
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
                self.layout();
                self.notice(nus_render::text::icons::DOWNLOAD, format!("Get {}", o.shell), "press Enter to run it · then LOOK AGAIN in Terminal settings");
            }
            Err(e) => self.notice_problem("Could Not Open Terminal", e.to_string()),
        }
    }

    /// `add shell …` from the palette: a program and its arguments.
    pub(crate) fn add_custom_shell(&mut self, typed: &str) {
        let parts = split_command(typed.trim());
        let Some(program) = parts.first().cloned() else { return };
        let b = base(&program);
        let mut name = if b.is_empty() { "shell".to_string() } else { b };
        let mut list = load_custom();
        let taken = |n: &str, list: &[Custom], profiles: &[Profile]| list.iter().any(|c| c.name == n) || profiles.iter().any(|p| p.name == n);
        if taken(&name, &list, &self.profiles) {
            name = (2..).map(|n| format!("{name} {n}")).find(|n| !taken(n, &list, &self.profiles)).unwrap_or(name);
        }
        let c = Custom { name: name.clone(), program, args: parts[1..].to_vec(), cwd: None, env: BTreeMap::new() };
        list.push(c.clone());
        if let Err(e) = save_custom(&list) {
            self.notice_problem("Could Not Save Shell", e.to_string());
            return;
        }
        self.custom_shells.push(name.clone());
        self.profiles.push(c.profile());
        self.notice(nus_render::text::icons::TERMINAL, "Shell Added", format!("{name} · edit its folder and variables in profile/shells.json"));
        self.save_prefs();
        self.dirty = true;
    }

    /// Take one of your own out of shells.json. It stays usable by the tabs
    /// already running it until nus restarts.
    pub(crate) fn remove_custom_shell(&mut self, i: usize) {
        let Some(p) = self.profiles.get(i).cloned() else { return };
        if self.shell_group(i) != Group::Yours {
            return;
        }
        if i == self.behavior.default_profile {
            self.set_default_shell(0);
        }
        let mut list = load_custom();
        list.retain(|c| c.name != p.name && format!("{} (yours)", c.name) != p.name);
        if let Err(e) = save_custom(&list) {
            self.notice_problem("Could Not Save Shells", e.to_string());
            return;
        }
        self.custom_shells.retain(|n| n != &p.name);
        if !self.behavior.shells_hidden.contains(&p.name) {
            self.behavior.shells_hidden.push(p.name.clone());
        }
        self.notice(nus_render::text::icons::TERMINAL, "Shell Removed", p.name);
        self.save_prefs();
        self.dirty = true;
    }

    /// profile/shells.json in the editor, with an example when it's new.
    pub(crate) fn open_shells_file(&mut self) {
        let path = custom_path();
        if !path.exists() {
            let example = vec![Custom { name: "example".into(), program: if cfg!(windows) { r"C:\path\to\shell.exe".into() } else { "/path/to/shell".into() }, args: vec!["--login".into()], cwd: Some("~".into()), env: BTreeMap::from([("NAME".to_string(), "value".to_string())]) }];
            if let Err(e) = save_custom(&example) {
                self.notice_problem("Could Not Create shells.json", e.to_string());
                return;
            }
        }
        self.open_file(&path, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_split_like_a_shell() {
        assert_eq!(split_command(r#"/usr/local/bin/xonsh --login"#), ["/usr/local/bin/xonsh", "--login"]);
        assert_eq!(split_command(r#""C:\Program Files\x\sh.exe" -i ''"#), [r"C:\Program Files\x\sh.exe", "-i", ""]);
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn levels_are_honest() {
        let p = |program: &str| Profile { name: "x".into(), program: program.into(), args: vec![], cwd: None, env: vec![] };
        assert_eq!(level(&p("/bin/zsh"), false), Level::Full);
        assert_eq!(level(&p(r"C:\Windows\System32\cmd.exe"), false), Level::Partial);
        assert_eq!(level(&p("ssh"), true), Level::Remote);
        assert_eq!(level(&p("ssh"), false), Level::None);
        assert_eq!(level(&p("/usr/bin/xonsh"), true), Level::None);
        assert!(quiet(&p("/bin/dash")));
        assert!(!quiet(&p("/bin/zsh")));
        assert!(machine(&p("wsl.exe")));
    }

    #[test]
    fn nothing_offered_twice_or_when_present() {
        let all = ["fish", "nu", "pwsh", "zsh", "git bash"].map(String::from).to_vec();
        assert!(offers(&all).is_empty());
    }
}

/// One line of Settings' shell list.
#[derive(Clone, Debug)]
pub enum Line {
    Head,
    Caption(&'static str),
    Row(usize),
    More(usize),
    Buttons,
    OfferCaption,
    Offer(usize, Offer),
}

impl App {
    /// The list, top to bottom: each group, the buttons, the offers.
    pub(crate) fn shell_lines(&self) -> Vec<Line> {
        let mut out = vec![Line::Head];
        let all = self.shell_order(true);
        for (g, word) in [(Group::Shell, "ON THIS MACHINE"), (Group::Yours, "YOURS"), (Group::Machine, "OTHER MACHINES · SSH AND WSL")] {
            let rows: Vec<usize> = all.iter().copied().filter(|&i| self.shell_group(i) == g).collect();
            if rows.is_empty() {
                continue;
            }
            out.push(Line::Caption(word));
            let cap = if g == Group::Machine { 8 } else { usize::MAX };
            out.extend(rows.iter().take(cap).map(|&i| Line::Row(i)));
            if rows.len() > cap {
                out.push(Line::More(rows.len() - cap));
            }
        }
        out.push(Line::Buttons);
        let offers = self.shell_offers();
        if !offers.is_empty() {
            out.push(Line::OfferCaption);
            out.extend(offers.into_iter().enumerate().map(|(k, o)| Line::Offer(k, o)));
        }
        out
    }

    fn shell_line_h(&self, l: &Line) -> f32 {
        self.px(match l {
            Line::Head => 30.0,
            Line::Caption(_) | Line::OfferCaption => 30.0,
            Line::Row(_) => 36.0,
            Line::More(_) => 26.0,
            Line::Buttons => 52.0,
            Line::Offer(..) => 52.0,
        })
    }

    pub(crate) fn shell_list_height(&self) -> f32 {
        self.shell_lines().iter().map(|l| self.shell_line_h(l)).sum::<f32>() + self.px(6.0)
    }

    /// A small outlined button with a word; pushes its hit. Returns its width.
    fn shell_button(&mut self, scene: &mut nus_render::Scene, right: f32, mid: f32, word: &str, hit: crate::settings::Hit, strong: bool) -> f32 {
        use nus_render::{Rect, Style};
        let t = self.theme.clone();
        let st = if strong { self.label_strong() } else { self.label() };
        let w = self.fonts.measure(st, word) + self.px(16.0);
        let r = Rect::new(right - w, mid - self.px(11.0), w, self.px(22.0));
        let hot = r.contains(self.mouse.0, self.mouse.1);
        if hot {
            scene.rect(r, t.ink);
        }
        scene.outline(r, self.px(nus_render::theme::metric::HAIRLINE), t.ink);
        let c = if hot { self.on_fill(t.ink) } else { t.ink };
        self.fonts.draw(scene, Style { color: c, ..st }, r.x + self.px(8.0), mid + self.px(4.0), word);
        self.settings_hits.push((r, hit));
        w
    }

    pub(crate) fn draw_shell_list(&mut self, scene: &mut nus_render::Scene, r: nus_render::Rect) {
        use crate::settings::Hit;
        use nus_render::text::icons;
        use nus_render::theme::metric as m;
        use nus_render::{Rect, Style};
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let head = Style { color: t.dim, px: self.px(9.5), tracking: self.px(1.0), ..label };
        let dim = Style { color: t.dim, ..ui };
        let cols = [r.x, r.x + r.w * 0.36, r.x + r.w * 0.52];
        let mut y = r.y;
        for line in self.shell_lines() {
            let h = self.shell_line_h(&line);
            let mid = y + h / 2.0;
            match &line {
                Line::Head => {
                    for (x, w) in cols.iter().zip(["SHELL", "FROM", "NUS INTEGRATION"]) {
                        self.fonts.draw(scene, head, *x, y + self.px(18.0), w);
                    }
                    scene.hline(r.x, y + h - self.px(4.0), r.w, self.px(m::HAIRLINE), ink);
                }
                Line::Caption(w) => {
                    self.fonts.draw(scene, Style { color: ink, ..head }, r.x, y + self.px(21.0), w);
                }
                Line::OfferCaption => {
                    scene.hline(r.x, y + self.px(4.0), r.w, self.px(m::HAIRLINE), ink);
                    self.fonts.draw(scene, Style { color: ink, ..head }, r.x, y + self.px(23.0), "GET A GOOD SHELL · TYPED INTO A SHELL FOR YOU TO RUN");
                }
                Line::Row(i) => {
                    let i = *i;
                    let Some(p) = self.profiles.get(i).cloned() else { continue };
                    let hidden = self.shell_hidden(i);
                    let default = i == self.behavior.default_profile;
                    let group = self.shell_group(i);
                    let row = Rect::new(r.x - self.px(8.0), y, r.w + self.px(16.0), h);
                    if default {
                        scene.rect(row, crate::app::fade(self.surface.signal, 0.08));
                        scene.rect(Rect::new(row.x, row.y, self.px(3.0), row.h), self.surface.signal);
                    }
                    let icon = match group {
                        Group::Shell => icons::TERMINAL,
                        Group::Yours => icons::USER,
                        Group::Machine => if base(&p.program) == "ssh" { icons::GLOBE } else { icons::SQUARES },
                    };
                    let c = if hidden { t.dim } else { ink };
                    let isz = self.px(12.0);
                    self.fonts.draw_icon(scene, icon, isz, cols[0], mid - isz / 2.0, c);
                    let name_w = cols[1] - cols[0] - self.px(30.0);
                    let name = self.fit(strong, &p.name, name_w);
                    let nx = cols[0] + self.px(20.0);
                    let drawn = self.fonts.draw(scene, Style { color: c, ..if default { strong } else { ui } }, nx, mid + self.px(4.0), &name);
                    if default {
                        self.fonts.draw(scene, Style { color: self.surface.signal, px: self.px(9.0), ..label }, nx + drawn + self.px(8.0), mid + self.px(3.5), "DEFAULT");
                    }
                    let source = self.shell_source(i);
                    let src = self.fit(dim, &source, cols[2] - cols[1] - self.px(12.0));
                    self.fonts.draw(scene, dim, cols[1], mid + self.px(4.0), &src);
                    // The path, on hover over the name or where it's from.
                    let info = Rect::new(cols[0], y, cols[2] - cols[0], h);
                    self.offer_tip(crate::app::hover_key("shell-path", i), info, format!("{} {}", p.program, p.args.join(" ")));
                    // Integration: a badge, and what it means on hover.
                    let lv = level(&p, self.behavior.ssh_integration);
                    let bw = self.fonts.measure(Style { px: self.px(9.0), ..label }, lv.word()) + self.px(12.0);
                    let badge = Rect::new(cols[2], mid - self.px(9.0), bw, self.px(18.0));
                    match lv {
                        Level::Full => scene.rect(badge, ink),
                        Level::Remote => scene.outline(badge, self.px(1.5), ink),
                        Level::Partial | Level::None => scene.outline(badge, self.px(m::HAIRLINE), crate::app::fade(ink, 0.5)),
                    }
                    let bc = match lv { Level::Full => self.on_fill(ink), Level::None => t.dim, _ => ink };
                    self.fonts.draw(scene, Style { color: bc, px: self.px(9.0), ..label }, badge.x + self.px(6.0), badge.y + self.px(12.5), lv.word());
                    self.offer_tip(crate::app::hover_key("shell-level", i), badge, lv.says().to_string());
                    // What you can do with it, from the right.
                    let mut right = r.right();
                    if group == Group::Yours {
                        right -= self.shell_button(scene, right, mid, "REMOVE", Hit::ShellRemove(i), false) + self.px(6.0);
                    }
                    if !default {
                        right -= self.shell_button(scene, right, mid, if hidden { "SHOW" } else { "HIDE" }, Hit::ShellHide(i), false) + self.px(6.0);
                        right -= self.shell_button(scene, right, mid, "MAKE DEFAULT", Hit::DefaultProfile(i), false) + self.px(6.0);
                    }
                    let _ = self.shell_button(scene, right, mid, "OPEN", Hit::ShellOpen(i), true);
                    scene.hline(r.x, y + h - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), t.tint);
                }
                Line::More(n) => {
                    self.fonts.draw(scene, dim, cols[0] + self.px(20.0), mid + self.px(4.0), &format!("and {n} more · type its name after NEW TAB, or in the palette"));
                }
                Line::Buttons => {
                    let mut x = r.x;
                    for (word, icon, hit) in [("ADD YOUR OWN SHELL", icons::PLUS, Hit::ShellAdd), ("EDIT SHELLS.JSON", icons::CODE, Hit::ShellEdit), ("LOOK AGAIN", icons::RELOAD, Hit::ShellRescan)] {
                        let w = self.fonts.measure(strong, word) + self.px(40.0);
                        let b = Rect::new(x, mid - self.px(15.0), w, self.px(30.0));
                        let hot = b.contains(self.mouse.0, self.mouse.1);
                        scene.rect(Rect::new(b.x + self.px(3.0), b.y + self.px(3.0), b.w, b.h), ink);
                        scene.rect(b, if hot { ink } else { t.paper });
                        scene.outline(b, self.px(m::STRUCTURE), ink);
                        let c = if hot { self.on_fill(ink) } else { ink };
                        self.fonts.draw_icon(scene, icon, self.px(12.0), b.x + self.px(10.0), b.y + self.px(9.0), c);
                        self.fonts.draw(scene, Style { color: c, ..strong }, b.x + self.px(28.0), b.y + self.px(19.5), word);
                        self.settings_hits.push((b, hit));
                        let tip = match hit {
                            Hit::ShellAdd => "A program and its arguments; nus runs it in a new tab like any shell",
                            Hit::ShellEdit => "profile/shells.json: your shells' names, folders and environment variables",
                            _ => "Find shells installed since nus started",
                        };
                        self.offer_tip(crate::app::hover_key("shell-btn", x as usize), b, tip.into());
                        x += w + self.px(14.0);
                    }
                }
                Line::Offer(k, o) => {
                    self.fonts.draw(scene, strong, r.x, y + self.px(20.0), o.shell);
                    let sw = self.fonts.measure(strong, o.shell);
                    let why = self.fit(dim, o.why, r.w * 0.62 - sw);
                    self.fonts.draw(scene, dim, r.x + sw + self.px(10.0), y + self.px(20.0), &why);
                    let mono = Style { font: self.f.term, px: self.px(11.0), color: ink, tracking: 0.0 };
                    let cmd = self.fit(mono, &o.command, r.w - self.px(90.0));
                    self.fonts.draw(scene, mono, r.x, y + self.px(39.0), &cmd);
                    let _ = self.shell_button(scene, r.right(), y + self.px(26.0), "GET", Hit::ShellGet(*k), true);
                    scene.hline(r.x, y + h - self.px(m::HAIRLINE), r.w, self.px(m::HAIRLINE), t.tint);
                }
            }
            y += h;
        }
    }
}
