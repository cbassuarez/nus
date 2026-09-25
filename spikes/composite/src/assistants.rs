//! Honest connection checks and explicit routing into persistent terminal sessions.
use crate::app::{Action, App, PaletteMode, PaletteRow, Pane};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
/// Preserve every character of commands and prompts, including long paths.
pub(crate) fn review_lines(
    fonts: &nus_render::FontSystem,
    style: nus_render::Style,
    text: &str,
    width: f32,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut line));
            continue;
        }
        let mut next = line.clone();
        next.push(ch);
        if !line.is_empty() && fonts.measure(style, &next) > width.max(style.px) {
            lines.push(std::mem::take(&mut line));
        }
        line.push(ch);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}
pub const NAMES: [&str; 3] = ["Claude", "Codex", "Ollama"];
pub const BINS: [&str; 3] = ["claude", "codex", "ollama"];
pub fn index(name: &str) -> Option<u8> {
    BINS.iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| i as u8)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub executable: String,
    pub model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub providers: [Provider; 3],
    /// Instant 0 … Max 4; see `intelligence`.
    pub intelligence: u8,
}
impl Default for Config {
    fn default() -> Self {
        Config { providers: Default::default(), intelligence: crate::intelligence::DEFAULT }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Model(u8),
    Executable(u8),
    Font(u8),
}
#[derive(Clone, Debug, Default)]
pub struct Connection {
    pub path: Option<PathBuf>,
    pub status: String,
    pub version: String,
    pub checked: bool,
    pub tools: bool,
    pub models: Vec<String>,
}
#[derive(Default)]
pub struct State {
    pub entries: [Connection; 3],
    pub pending: Option<Receiver<[Connection; 3]>>,
    queued: Option<Config>,
    pub tab: usize,
    pub checked_at: Option<Instant>,
}
impl State {
    pub fn refresh(&mut self, config: Config) {
        if self.pending.is_some() {
            self.queued = Some(config);
            return;
        }
        self.entries = Default::default();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let result = std::array::from_fn(|i| check(i, &config.providers[i]));
            let _ = tx.send(result);
        });
    }
    pub fn poll(&mut self) -> bool {
        let result = self.pending.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(entries) = result {
            self.pending = None;
            if let Some(config) = self.queued.take() {
                // Executable/model settings may change during a check. Never
                // publish the old provider result as the new configuration.
                self.refresh(config);
            } else {
                self.entries = entries;
                self.checked_at = Some(crate::clock::now());
            }
            true
        } else {
            false
        }
    }
}
fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return path
            .metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    }
    #[cfg(not(unix))]
    {
        true
    }
}
pub fn resolve(bin: &str, custom: &str) -> Option<PathBuf> {
    if !custom.trim().is_empty() {
        let p = PathBuf::from(custom.trim());
        return executable(&p).then_some(p);
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let h = PathBuf::from(home);
        dirs.extend([h.join(".local/bin"), h.join(".cargo/bin"), h.join("bin")]);
    }
    #[cfg(target_os = "macos")]
    dirs.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/Applications/Ollama.app/Contents/Resources"),
    ]);
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(p).join("npm"));
    }
    let names = if cfg!(windows) {
        vec![
            format!("{bin}.exe"),
            format!("{bin}.cmd"),
            format!("{bin}.bat"),
            bin.into(),
        ]
    } else {
        vec![bin.into()]
    };
    dirs.into_iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| executable(p))
}
/// Bound output and runtime. stderr is discarded and never shown in
/// connection cards because authentication output may contain account data.
fn capture(path: &Path, args: &[&str]) -> Result<(bool, String), String> {
    capture_with_timeout(path, args, Duration::from_secs(8))
}
fn capture_with_timeout(path: &Path, args: &[&str], timeout: Duration) -> Result<(bool, String), String> {
    use std::io::Read;
    let mut c = Command::new(path);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let mut child = c
        .spawn()
        .map_err(|_| "Could not start the CLI".to_string())?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = stdout;
        let _ = reader.by_ref().take(128 * 1024).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
        let _ = tx.send(String::from_utf8_lossy(&bytes).to_string());
    });
    let start = crate::clock::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A spawned descendant can keep stdout open after its parent
                // exits. The output read must share the process deadline.
                let remaining = timeout.saturating_sub(crate::clock::since(start));
                return rx.recv_timeout(remaining).map(|text| (status.success(), text))
                    .map_err(|_| "Check timed out while reading CLI output.".into());
            }
            Ok(None) if crate::clock::since(start) < timeout => {
                std::thread::sleep(Duration::from_millis(40))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Check timed out. Open the CLI to diagnose.".into());
            }
        }
    }
}
fn check(i: usize, config: &Provider) -> Connection {
    let Some(path) = resolve(BINS[i], &config.executable) else {
        return Connection {
            status: "Not found · choose an executable or install the CLI".into(),
            checked: true,
            ..Default::default()
        };
    };
    let mut result = Connection {
        path: Some(path.clone()),
        status: "Installed · not checked".into(),
        checked: true,
        ..Default::default()
    };
    match capture(&path, &["--version"]) {
        Ok((true, text)) => {
            result.version = text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(100)
                .collect()
        }
        _ => {
            result.status = "CLI could not start · check the executable".into();
            return result;
        }
    }
    let args: &[&str] = match i {
        0 => &["auth", "status", "--json"],
        1 => &["login", "status"],
        _ => &["list"],
    };
    match capture(&path, args) {
        Ok((ok, text)) if i == 2 => {
            if ok {
                result.models = text
                    .lines()
                    .skip(1)
                    .filter_map(|l| l.split_whitespace().next())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                result.status = format!(
                    "Service reachable · {} models available",
                    result.models.len()
                );
            } else {
                result.status = "CLI installed · service is not reachable".into();
            }
        }
        Ok((ok, text)) => {
            let signed = if i == 0 {
                serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v.get("loggedIn").and_then(|x| x.as_bool()))
                    .unwrap_or(false)
            } else {
                ok
            };
            result.status = if signed {
                "Signed in · ready to start a session"
            } else {
                "CLI installed · sign in or check authentication"
            }
            .into();
        }
        Err(e) => result.status = e,
    }
    if i < 2 {
        result.tools = capture(&path, &["mcp", "get", "nus"]).is_ok_and(|(ok, _)| ok);
    }
    result
}
/// Shell arguments, never interpolation. The default Windows shell is PowerShell.
pub fn quote(value: &str, powershell: bool) -> String {
    if powershell {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn packaged_cli(exe: &Path, platform: &str) -> Option<PathBuf> {
    let directory = exe.parent()?;
    Some(match platform {
        "macos" => directory.parent()?.join("Resources/bin/nus"),
        "windows" => directory.join("bin/nus.exe"),
        _ => directory.join("bin/nus"),
    })
}
/// The `nus` command-line companion: the bundle's own, else one on PATH.
pub(crate) fn nus_cli() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| packaged_cli(&p, std::env::consts::OS))
        .filter(|p| p.is_file())
        .or_else(|| resolve("nus", ""))
}

/// Whether an assistant's config carries nus's hooks (`nus hook install`).
/// Settings asks every frame it is up; the files are read every two seconds.
pub(crate) fn hooks_connected(id: u8) -> bool {
    use std::sync::Mutex;
    static SEEN: Mutex<Option<(Instant, [bool; 2])>> = Mutex::new(None);
    if id > 1 {
        return false;
    }
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    match *seen {
        Some((at, v)) if crate::clock::since(at) < Duration::from_secs(2) => v[id as usize],
        _ => {
            let v = [read_hooks(0), read_hooks(1)];
            *seen = Some((crate::clock::now(), v));
            v[id as usize]
        }
    }
}

fn read_hooks(id: u8) -> bool {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let (path, mark) = match id {
        0 => (std::env::var_os("CLAUDE_CONFIG_DIR").map(|d| PathBuf::from(d).join("settings.json")).or_else(|| home.map(|h| h.join(".claude/settings.json"))), "hook claude"),
        1 => (std::env::var_os("CODEX_HOME").map(|d| PathBuf::from(d).join("config.toml")).or_else(|| home.map(|h| h.join(".codex/config.toml"))), "hook codex"),
        _ => return false,
    };
    path.and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|t| t.contains(mark) && t.contains("NUS_CLI"))
}

impl App {
    /// Connect or disconnect an assistant's hooks: the command, typed and
    /// not run, in a new shell — you read it, you press Enter.
    pub(crate) fn review_assistant_hooks(&mut self, id: u8, on: bool) {
        if id > 1 {
            return;
        }
        let Some(cli) = nus_cli() else {
            self.notice_problem("Companion Missing", "the nus command-line companion · reinstall the app bundle or add nus to PATH");
            return;
        };
        let shell = self.profiles.get(self.behavior.default_profile).map(|p| crate::shell::kind_of(&p.program));
        let ps = shell == Some(crate::shell::Kind::PowerShell);
        let command = format!("{}{} hook {} {}", if ps { "& " } else { "" }, quote(&cli.display().to_string(), ps), if on { "install" } else { "uninstall" }, BINS[id as usize]);
        match self.new_term_pane_at(false, self.behavior.default_profile, None) {
            Ok(mut term) => {
                term.type_at_prompt = Some(command);
                let mut tab = self.make_tab(Pane::Term(term), None);
                tab.name = Some(format!("{} · {} status", NAMES[id as usize], if on { "connect" } else { "disconnect" }));
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
                self.layout();
                self.notice(nus_render::text::icons::ENTER, "Review The Command", if on { "then press Enter · a backup of the config is kept beside it" } else { "then press Enter · only nus's hooks are removed" });
            }
            Err(e) => self.notice_problem("Could Not Open Terminal", e.to_string()),
        }
    }

    pub(crate) fn default_assistant(&self) -> u8 {
        index(&self.behavior.ask_backend).unwrap_or_else(|| {
            (0..3)
                .find(|&i| {
                    resolve(BINS[i], &self.behavior.assistants.providers[i].executable).is_some()
                })
                .unwrap_or(0) as u8
        })
    }
    pub(crate) fn review_assistant_tools(&mut self, id: u8) {
        if id > 1 {
            return;
        }
        let Some(cli) = nus_cli() else {
            self.notice_problem("Companion Missing", "the nus command-line companion · reinstall the app bundle or add nus to PATH");
            return;
        };
        let Some(provider) = resolve(
            BINS[id as usize],
            &self.behavior.assistants.providers[id as usize].executable,
        ) else {
            self.notice(nus_render::text::icons::ASSISTANT, "Assistant Not Set Up", "set it up first");
            return;
        };
        let shell = self
            .profiles
            .get(self.behavior.default_profile)
            .map(|p| crate::shell::kind_of(&p.program));
        if shell == Some(crate::shell::Kind::Cmd) {
            self.notice(nus_render::text::icons::TERMINAL, "Choose Another Shell", "PowerShell or a POSIX shell can configure assistant tools");
            return;
        }
        let ps = shell == Some(crate::shell::Kind::PowerShell);
        let command = format!(
            "{}{} mcp add nus -- {} mcp",
            if ps { "& " } else { "" },
            quote(&provider.display().to_string(), ps),
            quote(&cli.display().to_string(), ps)
        );
        match self.new_term_pane_at(
            false,
            self.behavior.default_profile,
            Some(self.assistant_folder()),
        ) {
            Ok(mut term) => {
                term.type_at_prompt = Some(command);
                let mut tab = self.make_tab(Pane::Term(term), None);
                tab.name = Some(format!("{} · connect nus tools", NAMES[id as usize]));
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
                self.layout();
                self.notice(nus_render::text::icons::ENTER, "Review The Command", "then press Enter to register nus tools with this assistant");
            }
            Err(e) => self.notice_problem("Could Not Set Up Tools", e.to_string()),
        }
    }
    pub(crate) fn assistant_folder(&self) -> String {
        self.workspace
            .as_ref()
            .map(|p| p.display().to_string())
            .or_else(|| self.focused_cwd())
            .or_else(|| {
                self.tabs.iter().rev().find_map(|t| {
                    if let Pane::Term(p) = &t.left {
                        p.cwd.clone()
                    } else {
                        None
                    }
                })
            })
            .or_else(|| std::env::var("HOME").ok())
            .or_else(|| std::env::var("USERPROFILE").ok())
            .unwrap_or_else(|| ".".into())
    }
    pub(crate) fn assistant_command(&self, id: u8, prompt: &str) -> Result<String, String> {
        let i = id as usize;
        if i >= 3 {
            return Err("Unknown assistant".into());
        }
        let config = &self.behavior.assistants.providers[i];
        let path = resolve(BINS[i], &config.executable).ok_or_else(|| {
            format!(
                "{} was not found. Choose its executable in Assistants.",
                NAMES[i]
            )
        })?;
        let program = self
            .profiles
            .get(self.behavior.default_profile)
            .map(|p| p.program.to_lowercase())
            .unwrap_or_default();
        let ps = program.contains("pwsh") || program.contains("powershell");
        if program.ends_with("cmd.exe") || program == "cmd" {
            return Err("Choose PowerShell or a POSIX shell for assistant routing.".into());
        }
        let q = |s: &str| quote(s, ps);
        let mut args = vec![q(&path.display().to_string())];
        if i == 2 {
            if config.model.trim().is_empty() {
                return Err("Choose an Ollama model in Assistants before starting.".into());
            }
            args.extend(["run".into(), q(config.model.trim())]);
        } else {
            // The intelligence level, as flags this CLI accepts.
            let level = self.behavior.assistants.intelligence;
            args.extend(crate::intelligence::flags(i, level, &config.model).iter().map(|a| q(a)));
        }
        if !prompt.trim().is_empty() {
            // A prompt beginning with '-' is still prompt text, never a CLI
            // option that could change the provider's permissions or behavior.
            args.push("--".into());
            args.push(q(prompt.trim()));
        }
        Ok(format!("{}{}", if ps { "& " } else { "" }, args.join(" ")))
    }
    pub(crate) fn draft_assistant(&mut self, id: u8, prompt: &str) {
        if id >= 3 {
            return;
        }
        self.open_palette(PaletteMode::Assistant(id));
        if let Some((_, q)) = self.palette.as_mut() {
            *q = prompt.into();
        }
    }
    pub(crate) fn assistant_prompt_rows(&self, id: u8, input: &str) -> Vec<PaletteRow> {
        if id >= 3 {
            return vec![];
        }
        let info = |text: String| PaletteRow {
            num: "·".into(),
            text,
            action: Action::Noop,
        };
        let mut rows = vec![];
        match self.assistant_command(id, input) {
            Ok(command) => {
                rows.push(PaletteRow {
                    num: "*".into(),
                    text: format!(
                        "Start {} in a new terminal · {}",
                        NAMES[id as usize],
                        if input.trim().is_empty() {
                            "interactive session"
                        } else {
                            "send this prompt"
                        }
                    ),
                    action: Action::AssistantStart(id, input.trim().into()),
                });
                rows.push(info(format!("Command · {command}")));
            }
            Err(e) => rows.push(PaletteRow {
                num: "!".into(),
                text: e,
                action: Action::SettingsAt(9, None),
            }),
        }
        rows.push(info(format!("Folder · {}", self.assistant_folder())));
        if id == 2 && self.assistants.entries[2].checked {
            let model = &self.behavior.assistants.providers[2].model;
            if !model.is_empty()
                && !self.assistants.entries[2]
                    .models
                    .iter()
                    .any(|m| m == model || m == &format!("{model}:latest"))
            {
                rows.push(info(
                    "This model is not listed locally. Ollama may download it on first run.".into(),
                ));
            }
        }
        rows.push(info(
            "Uses the CLI's own account, instructions, tools and approval settings.".into(),
        ));
        rows.push(info(
            "No terminal output, browser pages or nus memory are attached to this launch.".into(),
        ));
        rows.push(PaletteRow {
            num: "⚙".into(),
            text: "Configure assistants, models and connections".into(),
            action: Action::SettingsAt(9, None),
        });
        rows
    }
    pub(crate) fn start_assistant(&mut self, id: u8, prompt: &str) {
        let command = match self.assistant_command(id, prompt) {
            Ok(c) => c,
            Err(e) => {
                self.notice_problem("Could Not Start Assistant", e);
                return;
            }
        };
        let cwd = self.assistant_folder();
        match self.new_term_pane_at(false, self.behavior.default_profile, Some(cwd)) {
            Ok(mut term) => {
                term.type_at_prompt = Some(format!("{command}\r"));
                term.type_origin = Some(crate::finish_work::Origin::NusAction);
                let mut tab = self.make_tab(Pane::Term(term), None);
                tab.name = Some(format!("{} · session", NAMES[id as usize]));
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
                self.layout();
                self.dirty = true;
            }
            Err(e) => self.notice_problem("Could Not Open Terminal", format!("for the assistant · {e}")),
        }
    }
    pub(crate) fn assistant_setup(&mut self, id: u8, kind: u8) {
        if id >= 3 {
            return;
        }
        let i = id as usize;
        if kind == 3 && id == 2 {
            self.open_url("https://ollama.com/library", true);
            return;
        }
        if kind == 0 {
            let url = match id {
                0 => "https://code.claude.com/docs/en/setup",
                1 => "https://developers.openai.com/codex/cli/",
                _ => "https://ollama.com/download",
            };
            self.open_url(url, true);
            return;
        }
        let Some(path) = resolve(BINS[i], &self.behavior.assistants.providers[i].executable) else {
            self.notice(nus_render::text::icons::ASSISTANT, "CLI Not Found", "install it or choose its executable first");
            return;
        };
        let ps = self
            .profiles
            .get(self.behavior.default_profile)
            .is_some_and(|p| p.program.contains("pwsh") || p.program.contains("powershell"));
        let program = quote(&path.display().to_string(), ps);
        let args = match (kind, id) {
            (1, 0) => "auth login",
            (1, 1) => "login",
            (1, _) => "serve",
            (_, 0) => "auth status --text",
            (_, 1) => "login status",
            _ => "list",
        };
        self.open_prompt_shell(&format!("{}{program} {args}", if ps { "& " } else { "" }));
    }
    pub(crate) fn preference_rows(&self, field: Field, input: &str) -> Vec<PaletteRow> {
        let title = match field {
            Field::Model(id) => format!("{} model", NAMES[id as usize]),
            Field::Executable(id) => format!("{} executable path", NAMES[id as usize]),
            Field::Font(role) => format!(
                "{} installed font",
                ["Interface", "Terminal", "Editor"][role as usize]
            ),
        };
        let mut rows = vec![PaletteRow {
            num: "✓".into(),
            text: format!(
                "Save {title} · {}",
                if input.trim().is_empty() {
                    "use default"
                } else {
                    input.trim()
                }
            ),
            action: Action::Preference(field, input.trim().into()),
        }];
        match field {
            Field::Model(2) => {
                for model in &self.assistants.entries[2].models {
                    if input.is_empty() || model.to_lowercase().contains(&input.to_lowercase()) {
                        rows.push(PaletteRow {
                            num: "·".into(),
                            text: format!("Use {model}"),
                            action: Action::Preference(field, model.clone()),
                        });
                    }
                }
            }
            Field::Font(role) => {
                for (family, mono) in self.fonts.system_families() {
                    if (role == 0 || mono)
                        && (input.is_empty()
                            || family.to_lowercase().contains(&input.to_lowercase()))
                    {
                        rows.push(PaletteRow {
                            num: "Aa".into(),
                            text: family.clone(),
                            action: Action::Preference(field, family),
                        });
                        if rows.len() >= 10 {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
        rows
    }
    pub(crate) fn set_preference(&mut self, field: Field, value: &str) {
        match field {
            Field::Model(id) => {
                self.behavior.assistants.providers[id as usize].model = value.into()
            }
            Field::Executable(id) => {
                if !value.is_empty() && !executable(Path::new(value)) {
                    self.notice(nus_render::text::icons::TERMINAL, "Not An Executable", "choose the full path to an executable file");
                    return;
                }
                self.behavior.assistants.providers[id as usize].executable = value.into();
                self.assistants.refresh(self.behavior.assistants.clone());
            }
            Field::Font(role) => {
                if !self.set_system_font(role, value) {
                    return;
                }
            }
        }
        self.save_prefs();
        self.dirty = true;
    }
    pub(crate) fn edit_preference(&mut self, field: Field) {
        let value = match field {
            Field::Model(id) => self.behavior.assistants.providers[id as usize]
                .model
                .clone(),
            Field::Executable(id) => self.behavior.assistants.providers[id as usize]
                .executable
                .clone(),
            Field::Font(role) => self.behavior.typography.system[role as usize].clone(),
        };
        self.open_palette(PaletteMode::Preference(field));
        if let Some((_, q)) = self.palette.as_mut() {
            *q = value;
        }
    }
    pub(crate) fn assistant_memory(&mut self) {
        let path = std::env::current_dir()
            .unwrap_or_default()
            .join("profile/memory.md");
        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = crate::protected_state::write(&path, b"");
        }
        self.open_file(&path, false);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn companion_cli_matches_every_shipping_package_layout() {
        for (exe, platform, expected) in [
            ("/Apps/nus.app/Contents/MacOS/nus", "macos", "/Apps/nus.app/Contents/Resources/bin/nus"),
            ("/package/nus.exe", "windows", "/package/bin/nus.exe"),
            ("/package/nus-desktop", "linux", "/package/bin/nus"),
        ] {
            assert_eq!(packaged_cli(Path::new(exe), platform), Some(PathBuf::from(expected)));
        }
    }
    #[cfg(unix)]
    #[test]
    fn cli_checks_timeout_when_a_descendant_keeps_stdout_open() {
        let start = Instant::now();
        let result = capture_with_timeout(Path::new("/bin/sh"), &["-c", "sleep 1 & exit 0"], Duration::from_millis(100));
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_millis(800));
    }
    #[test]
    fn long_review_preserves_every_character_and_fits() {
        let mut fonts = nus_render::FontSystem::new();
        let font = fonts
            .load_bytes(
                include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf"),
                0,
            )
            .unwrap();
        let style = nus_render::Style {
            font,
            px: 14.0,
            color: [1.0; 4],
            tracking: 0.0,
        };
        let text = "Command · '/a/very/long/path/without/spaces/claude' 'what is café's $(value)?'";
        let lines = review_lines(&fonts, style, text, 96.0);
        assert_eq!(lines.concat(), text);
        assert!(lines.len() > 3);
        assert!(lines.iter().all(|line| fonts.measure(style, line) <= 96.01));
    }
    #[test]
    fn quotes_keep_prompts_literal() {
        assert_eq!(quote("what's $(secret)", false), "'what'\\''s $(secret)'");
        assert_eq!(quote("what's $env:KEY", true), "'what''s $env:KEY'");
    }
    #[test]
    fn explicit_missing_executable_never_falls_back() {
        assert!(resolve("sh", "/definitely/missing/nus-test-cli").is_none());
    }
}
