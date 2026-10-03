//! The nus command's own lifecycle, none of it needing a running nus:
//!
//!   nus version                     this copy: version, channel, how and where
//!   nus update [--check]            the newest release for this channel, through
//!                                   whatever installed this copy
//!   nus uninstall [--everything] [--yes]
//!                                   this copy, the way it was installed; the
//!                                   profile stays unless --everything
//!   nus doctor                      what could keep nus from working here, each
//!                                   with the command that fixes it
//!
//! How a copy was installed is read from where it is and what it carries: the
//! Debian package says so in nus-package.json, the archive installed for this
//! account lives under the user data folder, nus.app may sit in Homebrew's
//! Caskroom, and the Windows installer has one fixed folder.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use serde_json::{json, Value};

use crate::ui::{pad, took, Ui};

const SITE: &str = "https://cbassuarez.com/nus.dev";
const RELEASES: &str = "https://api.github.com/repos/cbassuarez/nus/releases?per_page=30";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Method {
    /// The Debian package, under /opt: its name.
    Deb(String),
    /// The archive, installed for this account by its install-desktop.sh.
    Account,
    /// An extracted archive anywhere else.
    Folder,
    /// nus.app; the Homebrew cask's token when Homebrew put it there.
    App(Option<String>),
    /// The Windows installer's per-user copy.
    Installer,
    /// A build from source.
    Source,
}

impl Method {
    fn how(&self) -> String {
        match self {
            Method::Deb(p) => format!("Debian package {p}"),
            Method::Account => "installed for this account".into(),
            Method::Folder => "extracted folder".into(),
            Method::App(Some(t)) => format!("Homebrew cask {t}"),
            Method::App(None) => "nus.app".into(),
            Method::Installer => "Windows installer".into(),
            Method::Source => "built from source".into(),
        }
    }
}

pub struct Install {
    pub version: String,
    pub channel: nus_compat::Channel,
    pub method: Method,
    /// The package folder, or nus.app.
    pub root: PathBuf,
    /// What starts the app.
    pub launcher: PathBuf,
}

fn channel_word(c: nus_compat::Channel) -> &'static str {
    match c {
        nus_compat::Channel::Current => "stable",
        nus_compat::Channel::Preview => "preview",
        nus_compat::Channel::Development => "development",
    }
}

/// The user data folder nus keeps its profiles in.
fn data_home() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    Some(if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support/nus")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("nus")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home().map(|h| h.join(".local/share")))?
            .join("nus")
    })
}

/// This command's own copy of nus.
pub fn detect() -> Result<Install, String> {
    let app = crate::app().ok_or("this nus command is not part of an installed nus")?;
    let exe = if cfg!(target_os = "macos") {
        app.join("Contents/MacOS/nus")
    } else {
        app.clone()
    };
    let out = Command::new(&exe)
        .arg("--version")
        .output()
        .map_err(|e| format!("could not ask {} its version: {e}", exe.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text
        .split_whitespace()
        .nth(1)
        .ok_or("the installed nus did not say its version")?
        .to_string();
    let channel = nus_compat::Channel::for_version(&version);
    let root = if cfg!(target_os = "macos") {
        app.clone()
    } else {
        app.parent().ok_or("no package folder")?.to_path_buf()
    };
    let method = method_of(&root, channel);
    Ok(Install {
        version,
        channel,
        method,
        root,
        launcher: app,
    })
}

fn method_of(root: &Path, channel: nus_compat::Channel) -> Method {
    if cfg!(target_os = "macos") {
        let token = if channel == nus_compat::Channel::Preview {
            "nus@preview"
        } else {
            "nus"
        };
        let brew = ["/opt/homebrew", "/usr/local"]
            .iter()
            .any(|p| Path::new(p).join("Caskroom").join(token).is_dir());
        return Method::App(brew.then(|| token.to_string()));
    }
    let record: Option<Value> = std::fs::read(root.join("nus-package.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let Some(record) = record else {
        return Method::Source;
    };
    if cfg!(windows) {
        let fixed = data_home()
            .and_then(|d| d.parent().map(|p| p.join("Programs").join("nus")))
            .is_some_and(|p| root.starts_with(p));
        return if fixed {
            Method::Installer
        } else {
            Method::Folder
        };
    }
    if record["managed"] == "deb" {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "nus".into());
        return Method::Deb(name);
    }
    if data_home().is_some_and(|d| root.starts_with(d.join("app"))) {
        Method::Account
    } else {
        Method::Folder
    }
}

/// This platform's release target.
fn target() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos-arm64"
    } else if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// Order a version: x.y.z, and a preview before the release it leads to.
fn version_key(v: &str) -> Option<(u64, u64, u64, u64)> {
    let v = v.trim_start_matches('v');
    let (core, pre) = v
        .split_once("-preview.")
        .map_or((v, None), |(c, p)| (c, Some(p)));
    let mut n = core.split('.').map(|p| p.parse::<u64>().ok());
    let key = (n.next()??, n.next()??, n.next()??);
    if n.next().is_some() {
        return None;
    }
    let pre = match pre {
        Some(p) => p.parse().ok()?,
        None => u64::MAX,
    };
    Some((key.0, key.1, key.2, pre))
}

/// The newest published release of this channel with a package for this
/// platform, from GitHub.
fn latest(channel: nus_compat::Channel) -> Result<Option<String>, String> {
    let out = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            RELEASES,
        ])
        .output()
        .map_err(|_| "could not start curl")?;
    if !out.status.success() {
        return Err("GitHub could not be reached".into());
    }
    let releases: Value =
        serde_json::from_slice(&out.stdout).map_err(|_| "GitHub answered with something else")?;
    Ok(newest(
        &releases,
        channel == nus_compat::Channel::Preview,
        target(),
    ))
}

fn newest(releases: &Value, preview: bool, target: &str) -> Option<String> {
    releases
        .as_array()?
        .iter()
        .filter(|r| r["draft"] == false)
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let key = version_key(tag)?;
            let has = r["assets"]
                .as_array()?
                .iter()
                .any(|a| a["name"].as_str().is_some_and(|n| n.contains(target)));
            (has && tag.contains("-preview.") == preview)
                .then(|| (key, tag.trim_start_matches('v').to_string()))
        })
        .max_by_key(|(key, _)| *key)
        .map(|(_, tag)| tag)
}

fn fail(ui: &Ui, why: &str) -> ExitCode {
    ui.fail(why);
    ExitCode::FAILURE
}

// --- nus version ----------------------------------------------------------------

pub fn version(json: bool) -> ExitCode {
    let ui = Ui::new();
    let dot = ui.g().dot;
    let running = crate::call("version", json!({}))
        .ok()
        .and_then(|v| v.get("nus").and_then(Value::as_str).map(String::from));
    match detect() {
        Ok(i) => {
            if json {
                println!(
                    "{}",
                    json!({"version": i.version, "channel": channel_word(i.channel), "method": i.method.how(), "path": i.root, "running": running})
                );
            } else {
                println!(
                    "nus {} {dot} {} {dot} {} {dot} {}",
                    i.version,
                    channel_word(i.channel),
                    i.method.how(),
                    i.root.display()
                );
                if let Some(r) = running.filter(|r| *r != i.version) {
                    println!(
                        "{}",
                        ui.grey(&format!(
                            "running: nus {r} (restart it to use {})",
                            i.version
                        ))
                    );
                }
            }
        }
        Err(_) => {
            if json {
                println!(
                    "{}",
                    json!({"version": env!("CARGO_PKG_VERSION"), "method": "command only", "running": running})
                );
            } else {
                println!(
                    "nus {} {dot} command only, not part of an installed nus",
                    env!("CARGO_PKG_VERSION")
                );
            }
        }
    }
    ExitCode::SUCCESS
}

// --- nus update -----------------------------------------------------------------

pub fn update(check: bool) -> ExitCode {
    let ui = Ui::new();
    let i = match detect() {
        Ok(i) => i,
        Err(e) => return fail(&ui, &e),
    };
    if i.method == Method::Source {
        return fail(
            &ui,
            "this nus is built from source: pull and rebuild it to update",
        );
    }
    if check {
        let start = Instant::now();
        let dot = ui.g().dot;
        return match latest(i.channel) {
            Ok(Some(v)) if version_key(&v) > version_key(&i.version) => {
                ui.row(
                    "01",
                    "Update",
                    &format!("{} {} {v}", i.version, ui.g().arrow),
                    &ui.warn(),
                    &took(start),
                );
                ui.hint(&format!("{} to install it", ui.bold("nus update")));
                ExitCode::SUCCESS
            }
            Ok(_) => {
                ui.row(
                    "01",
                    "Update",
                    &format!("{} {dot} up to date", i.version),
                    &ui.ok(),
                    &took(start),
                );
                ExitCode::SUCCESS
            }
            Err(e) => fail(&ui, &e),
        };
    }
    let preview = i.channel == nus_compat::Channel::Preview;
    let status = match &i.method {
        Method::App(Some(token)) => {
            ui.banner(
                &format!("nus unified environment {} updater", ui.g().dot),
                &format!("v{} {} {}", i.version, ui.g().dot, channel_word(i.channel)),
            );
            let mut brew = Command::new("brew");
            brew.args(["upgrade", "--cask", &format!("cbassuarez/tap/{token}")]);
            return match ui.stream(
                "01",
                "Update",
                "with Homebrew",
                &i.root.display().to_string(),
                &mut brew,
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(&ui, &e),
            };
        }
        Method::Installer => {
            // The installer replaces this very command, which Windows will not
            // let it do while it runs: it continues in a window of its own.
            let mut ps = Command::new("powershell.exe");
            ps.args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-NoExit",
                "-Command",
                &format!("irm {SITE}/install.ps1 | iex"),
            ]);
            if preview {
                ps.env("NUS_CHANNEL", "preview");
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                ps.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
            }
            return match ps.spawn() {
                Ok(_) => {
                    println!("  The update continues in a new window.");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&ui, &format!("could not start PowerShell: {e}")),
            };
        }
        method => {
            // The installer itself: it finds this copy, says what it is doing,
            // and updates it in place (or says it is up to date).
            let mut sh = Command::new("sh");
            sh.arg("-c")
                .arg(format!("curl -fsSL {SITE}/install.sh | sh"));
            if preview {
                sh.env("NUS_CHANNEL", "preview");
            }
            if matches!(method, Method::Account | Method::Folder) {
                sh.env("NUS_USER", "1");
            }
            if *method == Method::Folder {
                println!(
                    "  {}",
                    ui.grey(&format!("This copy is a folder; the update installs nus for this account, and {} can go.", i.root.display()))
                );
            }
            sh.status()
        }
    };
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => fail(&ui, &format!("could not start the installer: {e}")),
    }
}

// --- nus uninstall --------------------------------------------------------------

fn ask(prompt: &str) -> String {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

/// The `nus` link in ~/.local/bin, or a folder's menu entry, when they point
/// into `root`.
fn remove_pointers(root: &Path, channel: nus_compat::Channel) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(link) = home.as_ref().map(|h| h.join(".local/bin/nus")) {
        if std::fs::read_link(&link).is_ok_and(|t| t.starts_with(root)) {
            let _ = std::fs::remove_file(&link);
        }
    }
    if cfg!(target_os = "linux") {
        let id = if channel == nus_compat::Channel::Preview {
            "dev.nus.app.preview.desktop"
        } else {
            "dev.nus.app.desktop"
        };
        if let Some(entry) =
            data_home().and_then(|d| d.parent().map(|p| p.join("applications").join(id)))
        {
            if std::fs::read_to_string(&entry)
                .is_ok_and(|t| t.contains(&format!("Exec=\"{}", root.display())))
            {
                let _ = std::fs::remove_file(&entry);
            }
        }
    }
}

pub fn uninstall(everything: bool, yes: bool) -> ExitCode {
    let ui = Ui::new();
    let i = match detect() {
        Ok(i) => i,
        Err(e) => return fail(&ui, &e),
    };
    if i.method == Method::Source {
        return fail(
            &ui,
            "this nus is built from source: there is no installed copy to remove",
        );
    }
    if crate::running() {
        return fail(&ui, "nus is running: quit it, then run nus uninstall again");
    }
    let dot = ui.g().dot;
    let word = channel_word(i.channel);
    let profile = data_home().map(|d| d.join("installs").join(i.channel.directory()));
    ui.banner(
        &format!("nus unified environment {dot} uninstaller"),
        &format!("v{} {dot} {word}", i.version),
    );
    let start = Instant::now();
    ui.row(
        "01",
        "Found",
        &format!("{} {dot} {}", i.version, i.method.how()),
        &ui.ok(),
        &took(start),
    );
    if !yes {
        if !std::io::stdin().is_terminal() {
            return fail(&ui, "pass --yes to remove nus without being asked");
        }
        println!();
        let answer = ask(&format!(
            "  Remove nus {} from {}? [y/N] ",
            i.version,
            i.root.display()
        ));
        if !matches!(answer.to_lowercase().as_str(), "y" | "yes") {
            println!("  Nothing was removed.");
            return ExitCode::SUCCESS;
        }
        if everything {
            let typed = ask(&format!(
                "  Type {} to also delete this channel's profile (settings, sessions, sign-ins, the sync key): ",
                ui.bold(word)
            ));
            if typed != word {
                println!("  Nothing was removed.");
                return ExitCode::SUCCESS;
            }
        }
        println!();
    }
    let root = i.root.display().to_string();
    let removed = match &i.method {
        Method::Deb(pkg) => {
            let root_user = Command::new("id")
                .arg("-u")
                .output()
                .is_ok_and(|o| o.stdout.starts_with(b"0"));
            if !root_user
                && !Command::new("sudo")
                    .args(["-n", "true"])
                    .status()
                    .is_ok_and(|s| s.success())
            {
                ui.row("02", "Remove", "with apt (sudo)", &ui.spinner(0), "");
                if !Command::new("sudo")
                    .arg("-v")
                    .status()
                    .is_ok_and(|s| s.success())
                {
                    return fail(&ui, "sudo was refused; nothing was removed");
                }
                if ui.tty {
                    print!("\x1b[2A\x1b[J");
                }
            }
            let mut apt = if root_user {
                Command::new("apt-get")
            } else {
                let mut c = Command::new("sudo");
                c.arg("apt-get");
                c
            };
            apt.args(["remove", "-y", pkg]);
            ui.stream(
                "02",
                "Remove",
                "with apt (sudo)",
                &format!("{pkg} removed"),
                &mut apt,
            )
        }
        Method::Account => {
            let mut sh = Command::new("sh");
            sh.arg(i.root.join("install-desktop.sh")).arg("--uninstall");
            ui.stream("02", "Remove", "for this account", &root, &mut sh)
        }
        Method::App(Some(token)) => {
            let mut brew = Command::new("brew");
            brew.args(["uninstall", "--cask", &format!("cbassuarez/tap/{token}")]);
            ui.stream("02", "Remove", "with Homebrew", &root, &mut brew)
        }
        Method::Folder | Method::App(None) => {
            let start = Instant::now();
            remove_pointers(&i.root, i.channel);
            match std::fs::remove_dir_all(&i.root) {
                Ok(()) => {
                    ui.row("02", "Remove", &root, &ui.ok(), &took(start));
                    Ok(())
                }
                Err(e) => Err(format!("could not remove {root}: {e}")),
            }
        }
        Method::Installer => {
            // The uninstaller removes this very command: it runs on its own
            // once this one has exited.
            let uninstaller = data_home().and_then(|d| {
                std::fs::read_dir(d.join("uninstall").join(i.channel.directory()))
                    .ok()?
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| {
                        p.file_name().is_some_and(|n| {
                            n.to_string_lossy().starts_with("unins")
                                && n.to_string_lossy().ends_with(".exe")
                        })
                    })
            });
            match uninstaller {
                Some(u) => match Command::new(&u)
                    .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
                    .spawn()
                {
                    Ok(_) => {
                        ui.row(
                            "02",
                            "Remove",
                            "the Windows uninstaller finishes in a moment",
                            &ui.ok(),
                            "",
                        );
                        Ok(())
                    }
                    Err(e) => Err(format!("could not start the uninstaller: {e}")),
                },
                None => Err("the Windows uninstaller is missing; use Settings › Apps".into()),
            }
        }
        Method::Source => unreachable!(),
    };
    if let Err(e) = removed {
        return fail(&ui, &e);
    }
    if everything {
        if let Some(p) = profile.as_ref().filter(|p| p.exists()) {
            let start = Instant::now();
            match std::fs::remove_dir_all(p) {
                Ok(()) => ui.row(
                    "03",
                    "Profile",
                    &format!("{} removed", p.display()),
                    &ui.ok(),
                    &took(start),
                ),
                Err(e) => return fail(&ui, &format!("could not remove {}: {e}", p.display())),
            }
        }
    }
    ui.rule();
    println!("  nus is gone.");
    println!();
    if !everything {
        if let Some(p) = profile.filter(|p| p.exists()) {
            println!(
                "  {}{}",
                pad("settings", 16),
                ui.grey(&format!("stay in {}", p.display()))
            );
        }
    }
    println!(
        "  {}{}",
        pad("to return", 16),
        ui.grey(&format!("curl -fsSL {SITE}/install.sh | sh"))
    );
    println!();
    ExitCode::SUCCESS
}

// --- nus doctor -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Good,
    Note,
    Bad,
}

struct Check {
    state: State,
    name: &'static str,
    detail: String,
    fix: Option<String>,
}

fn check(
    state: State,
    name: &'static str,
    detail: impl Into<String>,
    fix: Option<String>,
) -> Check {
    Check {
        state,
        name,
        detail: detail.into(),
        fix,
    }
}

/// The first `nus` on PATH.
fn on_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "nus.exe" } else { "nus" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// The program a desktop entry runs.
fn exec_target(text: &str) -> Option<String> {
    let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?.trim();
    match exec.strip_prefix('"') {
        Some(rest) => rest.split('"').next().map(|s| s.replace("\\\\", "\\")),
        None => exec.split_whitespace().next().map(String::from),
    }
}

#[cfg(unix)]
fn setuid_root(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).is_ok_and(|m| m.uid() == 0 && m.mode() & 0o4777 == 0o4755)
}
#[cfg(not(unix))]
fn setuid_root(_: &Path) -> bool {
    false
}

fn linux_checks(i: &Install, out: &mut Vec<Check>) {
    let display = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        check(State::Good, "Display", "Wayland", None)
    } else if std::env::var_os("DISPLAY").is_some() {
        check(State::Good, "Display", "X11", None)
    } else {
        check(
            State::Note,
            "Display",
            "none in this shell (over SSH, or a console)",
            None,
        )
    };
    out.push(display);

    let fix = match &i.method {
        Method::Deb(p) => Some(format!("sudo apt install --reinstall {p}")),
        _ => Some(format!(
            "sudo sh {}/install-desktop.sh --allow-sandbox",
            i.root.display()
        )),
    };
    let userns = Command::new(&i.launcher)
        .arg("--nus-check-userns")
        .status()
        .is_ok_and(|s| s.success());
    out.push(if userns {
        check(
            State::Good,
            "Sandbox",
            "Chromium can create its user namespaces",
            None,
        )
    } else if setuid_root(&i.root.join("chrome-sandbox")) {
        check(
            State::Good,
            "Sandbox",
            "Chromium's setuid sandbox helper",
            None,
        )
    } else {
        check(
            State::Bad,
            "Sandbox",
            "blocked: web pages will not open",
            fix,
        )
    });

    let id = if i.channel == nus_compat::Channel::Preview {
        "dev.nus.app.preview.desktop"
    } else {
        "dev.nus.app.desktop"
    };
    let user = data_home().and_then(|d| d.parent().map(|p| p.join("applications").join(id)));
    let system = Path::new("/usr/share/applications").join(id);
    let entry = user
        .clone()
        .filter(|u| u.is_file())
        .or_else(|| system.is_file().then_some(system));
    let own = i.launcher.canonicalize().ok();
    out.push(match entry {
        None => check(
            State::Note,
            "Menu",
            "nus is not in your applications",
            match &i.method {
                Method::Deb(p) => Some(format!("sudo apt install --reinstall {p}")),
                _ => Some(format!("sh {}/install-desktop.sh", i.root.display())),
            },
        ),
        Some(entry) => {
            // Only a user entry is ours to suggest removing; a system one
            // belongs to its package.
            let mine = user.as_ref() == Some(&entry);
            let text = std::fs::read_to_string(&entry).unwrap_or_default();
            match exec_target(&text) {
                Some(t) if !Path::new(&t).exists() => check(
                    State::Bad,
                    "Menu",
                    format!("opens {t}, which is gone"),
                    mine.then(|| format!("rm '{}'", entry.display())),
                ),
                Some(t) if Path::new(&t).canonicalize().ok() != own => check(
                    State::Note,
                    "Menu",
                    format!("opens another copy: {t}"),
                    mine.then(|| format!("rm '{}'", entry.display())),
                ),
                _ => check(State::Good, "Menu", "opens this copy", None),
            }
        }
    });

    let scoped = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .is_ok_and(|o| o.status.success());
    out.push(if scoped {
        check(
            State::Good,
            "Limits",
            "scoped: reclaimed past 30% of RAM, capped at 50%",
            None,
        )
    } else {
        check(
            State::Note,
            "Limits",
            "no user systemd: nus runs without resource limits",
            None,
        )
    });

    let icd: Vec<String> = ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"]
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name().to_str().map(|n| {
                n.trim_end_matches(".json")
                    .trim_end_matches("_icd.x86_64")
                    .trim_end_matches("_icd")
                    .to_string()
            })
        })
        .collect();
    out.push(if icd.is_empty() {
        check(
            State::Note,
            "Graphics",
            "no Vulkan driver: drawing falls back to software",
            Some("sudo apt install mesa-vulkan-drivers".into()),
        )
    } else {
        check(
            State::Good,
            "Graphics",
            format!(
                "Vulkan {} {} driver{}",
                "·",
                icd.len(),
                if icd.len() == 1 { "" } else { "s" }
            ),
            None,
        )
    });

    // Extracted archives left in Downloads: harmless now, but easy to start by mistake.
    let old: Vec<PathBuf> = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name().is_some_and(|n| {
                    n.to_string_lossy().starts_with("nus-")
                        && n.to_string_lossy().ends_with("-linux-x86_64")
                })
        })
        .filter(|p| *p != i.root)
        .collect();
    if !old.is_empty() {
        let list = old
            .iter()
            .map(|p| format!("'{}'", p.display()))
            .collect::<Vec<_>>()
            .join(" ");
        out.push(check(
            State::Note,
            "Leftovers",
            format!("{} extracted in Downloads", old.len()),
            Some(format!("rm -r {list}")),
        ));
    }
}

pub fn doctor() -> ExitCode {
    let ui = Ui::new();
    let start = Instant::now();
    let mut checks = Vec::new();
    let install = detect();
    match &install {
        Ok(i) if i.method == Method::Source => checks.push(check(
            State::Note,
            "Installed",
            format!("{} {} built from source", i.version, ui.g().dot),
            None,
        )),
        Ok(i) => checks.push(check(
            State::Good,
            "Installed",
            format!("{} {} {}", i.version, ui.g().dot, i.method.how()),
            None,
        )),
        Err(e) => checks.push(check(
            State::Bad,
            "Installed",
            e.clone(),
            Some(format!("curl -fsSL {SITE}/install.sh | sh")),
        )),
    }
    let me = std::env::current_exe()
        .ok()
        .and_then(|e| e.canonicalize().ok());
    checks.push(match on_path() {
        Some(p) if p.canonicalize().ok() == me => {
            check(State::Good, "Command", "nus on PATH is this copy", None)
        }
        Some(p) => check(
            State::Note,
            "Command",
            format!("nus on PATH is {}", p.display()),
            None,
        ),
        None => check(
            State::Note,
            "Command",
            "nus is not on PATH",
            me.as_ref()
                .and_then(|m| m.parent())
                .map(|d| format!("add {} to PATH", d.display())),
        ),
    });
    checks.push(if crate::running() {
        check(State::Good, "Running", "nus is open and answering", None)
    } else {
        check(State::Good, "Running", "nus is not open", None)
    });
    if let Ok(i) = &install {
        if cfg!(target_os = "linux") && i.method != Method::Source {
            linux_checks(i, &mut checks);
        }
        if i.method != Method::Source {
            checks.push(match latest(i.channel) {
                Ok(Some(v)) if version_key(&v) > version_key(&i.version) => check(
                    State::Note,
                    "Updates",
                    format!("{v} is available"),
                    Some("nus update".into()),
                ),
                Ok(_) => check(State::Good, "Updates", "up to date", None),
                Err(e) => check(State::Note, "Updates", e, None),
            });
        }
    }
    let version = install
        .as_ref()
        .map(|i| format!("v{} {} {}", i.version, ui.g().dot, channel_word(i.channel)))
        .unwrap_or_default();
    println!();
    println!("  {}   {}", ui.bold("nus doctor"), ui.grey(&version));
    println!();
    for (n, c) in checks.iter().enumerate() {
        let mark = match c.state {
            State::Good => ui.ok(),
            State::Note => ui.warn(),
            State::Bad => ui.bad(),
        };
        ui.row(&format!("{:02}", n + 1), c.name, &c.detail, &mark, "");
        if let Some(fix) = &c.fix {
            ui.hint(fix);
        }
    }
    let bad = checks.iter().filter(|c| c.state == State::Bad).count();
    ui.rule();
    if bad == 0 {
        println!(
            "  {}",
            ui.grey(&format!(
                "nothing in the way {} {}",
                ui.g().dot,
                took(start)
            ))
        );
        ExitCode::SUCCESS
    } else {
        println!(
            "  {}",
            if bad == 1 {
                "1 thing keeps nus from working here.".to_string()
            } else {
                format!("{bad} things keep nus from working here.")
            }
        );
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_come_before_their_release() {
        assert!(version_key("0.0.2-preview.11") > version_key("0.0.2-preview.9"));
        assert!(version_key("v0.0.2") > version_key("v0.0.2-preview.11"));
        assert!(version_key("0.1.0-preview.1") > version_key("0.0.2"));
        assert_eq!(version_key("0.0.2-dev"), None);
        assert_eq!(version_key("1.2"), None);
    }

    #[test]
    fn newest_release_of_the_channel_with_this_platform() {
        let r = json!([
            {"tag_name": "v0.0.2-preview.11", "draft": true, "assets": [{"name": "nus-0.0.2-preview.11-linux-x86_64.tar.gz"}]},
            {"tag_name": "v0.0.2-preview.10", "draft": false, "assets": [{"name": "nus-0.0.2-preview.10-linux-x86_64.tar.gz"}]},
            {"tag_name": "v0.0.2-preview.9", "draft": false, "assets": [{"name": "nus-0.0.2-preview.9-linux-x86_64.tar.gz"}]},
            {"tag_name": "v0.0.1", "draft": false, "assets": [{"name": "nus-0.0.1-macos-arm64.zip"}]},
            {"tag_name": "apt-preview", "draft": false, "assets": [{"name": "nus-preview_0.0.2-preview.10_amd64.deb"}]}
        ]);
        assert_eq!(
            newest(&r, true, "linux-x86_64").as_deref(),
            Some("0.0.2-preview.10")
        );
        assert_eq!(newest(&r, false, "linux-x86_64"), None);
        assert_eq!(newest(&r, false, "macos-arm64").as_deref(), Some("0.0.1"));
    }

    #[test]
    fn a_menu_entry_names_its_program() {
        assert_eq!(
            exec_target("Exec=\"/opt/nus-preview/nus\" --open-external -- %U").as_deref(),
            Some("/opt/nus-preview/nus")
        );
        assert_eq!(
            exec_target("Exec=/usr/bin/nus %U").as_deref(),
            Some("/usr/bin/nus")
        );
    }
}
