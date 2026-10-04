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
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
            .any(|p| homebrew_owns(root, &Path::new(p).join("Caskroom").join(token)));
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
            .is_some_and(|p| installed_under(root, &p));
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
    if data_home().is_some_and(|d| installed_under(root, &d.join("app"))) {
        Method::Account
    } else {
        Method::Folder
    }
}

fn installed_under(root: &Path, directory: &Path) -> bool {
    match (root.canonicalize(), directory.canonicalize()) {
        (Ok(root), Ok(directory)) => root.starts_with(directory),
        _ => false,
    }
}

fn homebrew_owns(root: &Path, cask: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(cask) = cask.canonicalize() else {
        return false;
    };
    if root.starts_with(&cask) {
        return true;
    }
    // Casks moved to /Applications leave an app link in their version folder.
    std::fs::read_dir(cask).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .join("nus.app")
                .canonicalize()
                .is_ok_and(|app| app == root)
        })
    })
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
                "  Type {} to also delete this channel's profiles, recovery copies, sign-ins, vault keys and logs: ",
                ui.bold(word)
            ));
            if typed != word {
                println!("  Nothing was removed.");
                return ExitCode::SUCCESS;
            }
        }
        println!();
    }
    if everything
        && cfg!(target_os = "linux")
        && matches!(i.method, Method::Account | Method::Folder)
    {
        if let Err(e) = remove_user_sandbox_rule(&i) {
            return fail(&ui, &e);
        }
    }
    // Keep the package available until credential/profile cleanup succeeds.
    // A busy profile or locked credential store must not strand an uninstall.
    if everything {
        if let Some(base) = data_home() {
            if let Err(e) = purge_channel_data(&base, i.channel, Some(&i.root)) {
                return fail(
                    &ui,
                    &format!("local data could not be removed: {e}; nus is still installed"),
                );
            }
            ui.row(
                "02",
                "Local data",
                "profiles, recovery copies, vault keys and logs removed",
                &ui.ok(),
                "",
            );
        }
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
            apt.args([if everything { "purge" } else { "remove" }, "-y", pkg]);
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
        if let Some(base) = data_home() {
            remove_empty_data_dirs(&base);
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

/// The signed Windows uninstaller calls this after closing nus, while its
/// packaged CLI is still present. No remote sync destination is touched.
pub fn uninstall_data() -> ExitCode {
    let ui = Ui::new();
    let i = match detect() {
        Ok(i) => i,
        Err(e) => return fail(&ui, &e),
    };
    if !cfg!(windows) || i.method != Method::Installer {
        return fail(
            &ui,
            "data cleanup requires the installed Windows uninstaller",
        );
    }
    if crate::running() {
        return fail(&ui, "quit nus before removing its local data");
    }
    match data_home()
        .ok_or_else(|| std::io::Error::other("user data directory unavailable"))
        .and_then(|base| purge_channel_data(&base, i.channel, Some(&i.root)))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&ui, &format!("local data could not be removed: {e}")),
    }
}

fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn collect_profiles(root: &Path, profiles: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for item in std::fs::read_dir(root)? {
        let path = item?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_dir() || is_link(&meta) {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "profile") {
            profiles.push(path);
        } else {
            collect_profiles(&path, profiles)?;
        }
    }
    Ok(())
}

/// Only recovery records naming this exact installation can authorize removal
/// beside it. Never delete a folder just because its name looks like nus's.
fn recovery_cleanup(
    installation: &Path,
    profiles: &[PathBuf],
    channel: nus_compat::Channel,
) -> std::io::Result<Vec<PathBuf>> {
    let installed = installation.canonicalize()?;
    let parent = installed
        .parent()
        .ok_or_else(|| std::io::Error::other("installation has no parent"))?;
    let mut records: Vec<PathBuf> = profiles
        .iter()
        .map(|p| p.parent().unwrap().join("update-recovery.json"))
        .collect();
    for entry in std::fs::read_dir(parent)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".nus-previous-"))
            && path.extension().is_some_and(|e| e == "json")
        {
            records.push(path);
        }
    }
    let mut paths = std::collections::BTreeSet::new();
    for record in records {
        let Ok(meta) = std::fs::symlink_metadata(&record) else {
            continue;
        };
        if !meta.is_file() || is_link(&meta) || meta.len() > 65536 {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&std::fs::read(&record)?) else {
            continue;
        };
        let Some(owner) = value["installation"].as_str().map(Path::new) else {
            continue;
        };
        let Some(version) = value["previous_version"].as_str() else {
            continue;
        };
        if value["schema"] != 1
            || owner.canonicalize().ok().as_ref() != Some(&installed)
            || nus_compat::Channel::for_version(version.trim_start_matches('v')) != channel
        {
            continue;
        }
        for (field, prefixes) in [
            ("package", &[".nus-previous-"][..]),
            ("staging", &[".nus-update-", ".nus-recovery-"][..]),
        ] {
            let Some(path) = value[field].as_str().map(PathBuf::from) else {
                continue;
            };
            if path.parent().and_then(|p| p.canonicalize().ok()).as_deref() != Some(parent)
                || !path.file_name().is_some_and(|n| {
                    prefixes
                        .iter()
                        .any(|prefix| n.to_string_lossy().starts_with(prefix))
                })
            {
                continue;
            }
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir() && !is_link(&m)) {
                paths.insert(path);
            }
        }
        if record.parent() == Some(parent) {
            paths.insert(record);
        }
    }
    // Remove ownership records last so a failed package deletion can be retried.
    let mut paths: Vec<_> = paths.into_iter().collect();
    paths.sort_by_key(|p| p.extension().is_some_and(|e| e == "json"));
    Ok(paths)
}

fn purge_channel_data(
    base: &Path,
    channel: nus_compat::Channel,
    installation: Option<&Path>,
) -> std::io::Result<()> {
    let root = base.join("installs").join(channel.directory());
    let mut recovery = Vec::new();
    if root.exists() {
        let _channel_guard = nus_compat::profile::Guard::acquire(&root)?;
        let mut profiles = Vec::new();
        collect_profiles(&root, &mut profiles)?;
        if let Some(installed) = installation {
            recovery = recovery_cleanup(installed, &profiles, channel)?;
        }
        // Imported/copied profiles can retain the same vault id. Deleting this
        // channel must not erase a key another channel or legacy profile uses.
        let mut others = Vec::new();
        for item in std::fs::read_dir(base.join("installs"))? {
            let path = item?.path();
            let meta = std::fs::symlink_metadata(&path)?;
            if path != root && meta.is_dir() && !is_link(&meta) {
                collect_profiles(&path, &mut others)?;
            }
        }
        if base.join("profile").is_dir() {
            others.push(base.join("profile"));
        }
        let retained = others
            .iter()
            .map(|p| nus_vault::key_identifier(p))
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>();
        // Acquire every lock before removing any credential or file.
        let _guards = profiles
            .iter()
            .map(|p| nus_compat::profile::Guard::acquire(p.parent().unwrap()))
            .collect::<std::io::Result<Vec<_>>>()?;
        for p in &profiles {
            if nus_vault::key_identifier(p)?.is_none_or(|id| !retained.contains(&id)) {
                nus_vault::erase_key(p)?;
            }
        }
        std::fs::remove_dir_all(&root)?;
    } else if let Some(installed) = installation {
        recovery = recovery_cleanup(installed, &[], channel)?;
    }
    for path in recovery {
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if is_link(&meta) {
            continue;
        }
        if meta.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if meta.is_file() {
            std::fs::remove_file(path)?;
        }
    }
    let logs = base.join("logs").join(channel.directory());
    match std::fs::remove_dir_all(&logs) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if cfg!(target_os = "linux") {
        remove_shared_linux_data(base, channel, Path::new("/opt"))?;
    }
    remove_empty_data_dirs(base);
    Ok(())
}

fn sandbox_rule_is_owned(text: &str, name: &str, root: &Path) -> bool {
    text.lines().any(|line| {
        line == format!(
            "profile {name} {}/nus-desktop flags=(unconfined) {{",
            root.display()
        )
    })
}

fn remove_user_sandbox_rule(i: &Install) -> Result<(), String> {
    let identity = Command::new("id")
        .arg("-un")
        .output()
        .map_err(|e| e.to_string())?;
    let user = String::from_utf8_lossy(&identity.stdout).trim().to_string();
    if !identity.status.success()
        || user.is_empty()
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Ok(());
    }
    let name = format!("nus-{}-{user}", i.channel.directory());
    let rule = Path::new("/etc/apparmor.d").join(&name);
    if !std::fs::symlink_metadata(&rule).is_ok_and(|m| m.is_file() && !is_link(&m)) {
        return Ok(());
    }
    let text = std::fs::read_to_string(&rule).map_err(|e| e.to_string())?;
    if !sandbox_rule_is_owned(&text, &name, &i.root) {
        return Ok(());
    }
    // The account installer creates this optional root-owned integration only
    // on explicit --allow-sandbox. Remove just its exact, verified rule.
    let script = r#"set -e
if [ -d /sys/kernel/security/apparmor ] && command -v apparmor_parser >/dev/null 2>&1; then apparmor_parser -R "$1"; fi
rm -f -- "$1"
"#;
    let result = Command::new("sudo")
        .args(["/bin/sh", "-c", script, "nus-uninstall"])
        .arg(&rule)
        .status()
        .map_err(|e| e.to_string())?;
    if result.success() {
        Ok(())
    } else {
        Err("could not remove this installation's sandbox rule; authenticate with sudo and retry; nus is still installed".into())
    }
}

fn remove_shared_linux_data(
    base: &Path,
    channel: nus_compat::Channel,
    opt: &Path,
) -> std::io::Result<()> {
    for other in [
        nus_compat::Channel::Current,
        nus_compat::Channel::Preview,
        nus_compat::Channel::Development,
    ] {
        if other == channel {
            continue;
        }
        let package = if other == nus_compat::Channel::Preview {
            "nus-preview"
        } else {
            "nus"
        };
        if base.join("installs").join(other.directory()).exists()
            || base.join("app").join(other.directory()).exists()
            || (other != nus_compat::Channel::Development && opt.join(package).exists())
        {
            return Ok(());
        }
    }
    let Some(data) = base.parent() else {
        return Ok(());
    };
    let extension = data.join("gnome-shell/extensions/dock-motion@nus.dev");
    let parents_are_local = extension
        .ancestors()
        .take_while(|p| *p != data)
        .all(|p| std::fs::symlink_metadata(p).is_ok_and(|m| !is_link(&m)));
    let owned = parents_are_local
        && std::fs::symlink_metadata(&extension).is_ok_and(|m| m.is_dir() && !is_link(&m))
        && std::fs::read(extension.join("metadata.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| {
                v["uuid"] == "dock-motion@nus.dev" && v["url"] == "https://cbassuarez.com/nus.dev/"
            });
    if owned {
        std::fs::remove_dir_all(&extension)?;
        if cfg!(target_os = "linux") {
            let _ = Command::new("gnome-extensions")
                .args(["disable", "dock-motion@nus.dev"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    match std::fs::remove_file(base.join("dock-motion-enabled")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(())
}

fn remove_empty_data_dirs(base: &Path) {
    for name in ["installs", "logs", "app", "uninstall"] {
        let _ = std::fs::remove_dir(base.join(name));
    }
    let _ = std::fs::remove_dir(base);
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

    // GNOME: nus's launch on its dock icon goes through its own small Shell
    // extension, which nus installs and enables; GNOME loads a new one at
    // the next login.
    let gnome = std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("GNOME")));
    if gnome {
        let installed = data_home()
            .and_then(|d| {
                d.parent()
                    .map(|p| p.join("gnome-shell/extensions/dock-motion@nus.dev/extension.js"))
            })
            .is_some_and(|p| p.is_file());
        let enabled = Command::new("gsettings")
            .args(["get", "org.gnome.shell", "enabled-extensions"])
            .output()
            .ok()
            .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("dock-motion@nus.dev"));
        let running = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell",
                "--object-path",
                "/org/gnome/Shell",
                "--method",
                "org.gnome.Shell.Extensions.GetExtensionInfo",
                "dock-motion@nus.dev",
            ])
            .output()
            .ok()
            .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("'state': <1.0>"));
        out.push(match (installed, enabled, running) {
            (_, _, true) => check(
                State::Good,
                "Dock",
                "nus's launch plays on its dock icon",
                None,
            ),
            (true, true, false) => check(
                State::Note,
                "Dock",
                "the dock motion loads at your next login",
                None,
            ),
            (true, false, _) => check(
                State::Note,
                "Dock",
                "dock motion is turned off",
                Some("gnome-extensions enable dock-motion@nus.dev".into()),
            ),
            (false, _, _) => check(
                State::Note,
                "Dock",
                "dock motion is installed when nus next starts",
                None,
            ),
        });
    }

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
    fn installation_ownership_compares_canonical_paths_on_both_sides() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("Programs/nus");
        let installed = directory.join("preview");
        let unrelated = temp.path().join("Programs/nus-other/preview");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(&unrelated).unwrap();
        assert!(installed_under(
            &installed.canonicalize().unwrap(),
            &directory
        ));
        assert!(!installed_under(
            &unrelated.canonicalize().unwrap(),
            &directory
        ));
    }

    #[test]
    fn sandbox_cleanup_requires_the_exact_installation_rule() {
        let rule = "profile nus-preview-seb /home/seb/.local/share/nus/app/preview/nus-desktop flags=(unconfined) {";
        assert!(sandbox_rule_is_owned(
            rule,
            "nus-preview-seb",
            Path::new("/home/seb/.local/share/nus/app/preview")
        ));
        assert!(!sandbox_rule_is_owned(
            rule,
            "nus-release-seb",
            Path::new("/home/seb/.local/share/nus/app/preview")
        ));
        assert!(!sandbox_rule_is_owned(
            rule,
            "nus-preview-seb",
            Path::new("/home/seb/project")
        ));
    }

    #[test]
    fn shared_dock_cleanup_waits_for_the_last_channel() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("nus");
        let other = base.join("installs/release/shared/profile");
        std::fs::create_dir_all(&other).unwrap();
        let extension = temp
            .path()
            .join("gnome-shell/extensions/dock-motion@nus.dev");
        std::fs::create_dir_all(&extension).unwrap();
        std::fs::write(
            extension.join("metadata.json"),
            r#"{"uuid":"dock-motion@nus.dev","url":"https://cbassuarez.com/nus.dev/"}"#,
        )
        .unwrap();
        std::fs::write(base.join("dock-motion-enabled"), "enabled").unwrap();
        let opt = temp.path().join("opt");
        remove_shared_linux_data(&base, nus_compat::Channel::Preview, &opt).unwrap();
        assert!(extension.exists());
        std::fs::remove_dir_all(&other).unwrap();
        std::fs::remove_dir_all(base.join("installs/release")).unwrap();
        std::fs::create_dir_all(opt.join("nus")).unwrap();
        remove_shared_linux_data(&base, nus_compat::Channel::Preview, &opt).unwrap();
        assert!(
            extension.exists(),
            "an installed channel with no profile still uses it"
        );
        std::fs::remove_dir_all(&opt).unwrap();
        remove_shared_linux_data(&base, nus_compat::Channel::Preview, &opt).unwrap();
        assert!(!extension.exists());
        assert!(!base.join("dock-motion-enabled").exists());
    }

    #[test]
    fn complete_cleanup_removes_only_recovery_owned_by_this_installation() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("data");
        let installed = temp.path().join("nus");
        let foreign = temp.path().join("other nus");
        let profile = base.join("installs/preview/shared/profile");
        for dir in [&installed, &foreign, &profile] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let owned = temp.path().join(".nus-previous-owned");
        let staging = temp.path().join(".nus-update-owned");
        let unrelated = temp.path().join(".nus-previous-foreign");
        let project = temp.path().join("project");
        for dir in [&owned, &staging, &unrelated, &project] {
            std::fs::create_dir(dir).unwrap();
        }
        let record = |owner: &Path, package: &Path, stage: &Path| {
            json!({
                "schema":1,"previous_version":"v0.0.2-preview.12", "installation":owner,
                "package":package,"staging":stage
            })
        };
        let owned_record = owned.with_extension("json");
        let foreign_record = unrelated.with_extension("json");
        std::fs::write(
            &owned_record,
            record(&installed, &owned, &staging).to_string(),
        )
        .unwrap();
        std::fs::write(
            &foreign_record,
            record(&foreign, &unrelated, &project).to_string(),
        )
        .unwrap();
        // Even a valid owner record cannot authorize a project path.
        std::fs::write(
            profile.parent().unwrap().join("update-recovery.json"),
            record(&installed, &project, &project).to_string(),
        )
        .unwrap();
        purge_channel_data(&base, nus_compat::Channel::Preview, Some(&installed)).unwrap();
        assert!(!owned.exists());
        assert!(!staging.exists());
        assert!(!owned_record.exists());
        assert!(unrelated.exists());
        assert!(foreign_record.exists());
        assert!(project.exists());
        // Companion ownership survives profile removal and allows cleanup retries.
        std::fs::create_dir(&owned).unwrap();
        std::fs::write(
            &owned_record,
            record(&installed, &owned, &staging).to_string(),
        )
        .unwrap();
        purge_channel_data(&base, nus_compat::Channel::Preview, Some(&installed)).unwrap();
        assert!(!owned.exists());
        assert!(!owned_record.exists());
    }

    #[cfg(unix)]
    #[test]
    fn recovery_cleanup_preserves_linked_and_outside_packages() {
        let temp = tempfile::tempdir().unwrap();
        let installed = temp.path().join("nus");
        let outside = temp.path().join("project/.nus-previous-outside");
        std::fs::create_dir(&installed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let linked = temp.path().join(".nus-previous-linked");
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        for (name, package) in [("linked", &linked), ("outside", &outside)] {
            std::fs::write(temp.path().join(format!(".nus-previous-{name}.json")), json!({
                "schema":1,"previous_version":"0.0.2-preview.12","installation":installed,"package":package
            }).to_string()).unwrap();
        }
        purge_channel_data(
            &temp.path().join("data"),
            nus_compat::Channel::Preview,
            Some(&installed),
        )
        .unwrap();
        assert!(outside.exists());
        assert!(linked.symlink_metadata().unwrap().file_type().is_symlink());
    }

    #[test]
    fn an_unrelated_homebrew_cask_does_not_own_this_copy() {
        let temp = tempfile::tempdir().unwrap();
        let cask = temp.path().join("Caskroom/nus/1.0/nus.app");
        let manual = temp.path().join("Downloads/nus.app");
        std::fs::create_dir_all(&cask).unwrap();
        std::fs::create_dir_all(&manual).unwrap();
        let cask_root = temp.path().join("Caskroom/nus");
        assert!(homebrew_owns(&cask, &cask_root));
        assert!(!homebrew_owns(&manual, &cask_root));
        #[cfg(unix)]
        {
            std::fs::remove_dir(&cask).unwrap();
            std::os::unix::fs::symlink(&manual, &cask).unwrap();
            assert!(homebrew_owns(&manual, &cask_root));
        }
    }

    #[test]
    fn complete_uninstall_removes_channel_data_and_logs_only() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("nus");
        for channel in ["preview", "release"] {
            for suffix in [
                "shared/profile",
                "old/profile",
                "shared/generations/saved/profile",
            ] {
                let p = base.join("installs").join(channel).join(suffix);
                std::fs::create_dir_all(&p).unwrap();
                std::fs::write(p.join("settings.json"), "my local profile").unwrap();
            }
            let logs = base.join("logs").join(channel);
            std::fs::create_dir_all(&logs).unwrap();
            std::fs::write(logs.join("nus.log"), "my log").unwrap();
        }
        purge_channel_data(&base, nus_compat::Channel::Preview, None).unwrap();
        assert!(!base.join("installs/preview").exists());
        assert!(!base.join("logs/preview").exists());
        assert!(base
            .join("installs/release/shared/profile/settings.json")
            .is_file());
        assert!(base.join("logs/release/nus.log").is_file());
        purge_channel_data(&base, nus_compat::Channel::Preview, None).unwrap();
    }

    #[test]
    fn complete_uninstall_refuses_a_busy_profile_before_deleting_anything() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("nus");
        let root = base.join("installs/preview/shared");
        std::fs::create_dir_all(root.join("profile")).unwrap();
        std::fs::write(root.join("profile/settings.json"), "keep me").unwrap();
        let _guard = nus_compat::profile::Guard::acquire(&root).unwrap();
        assert!(purge_channel_data(&base, nus_compat::Channel::Preview, None).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("profile/settings.json")).unwrap(),
            "keep me"
        );
    }

    #[test]
    fn complete_uninstall_keeps_a_key_another_channel_uses() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("nus");
        let id = "0123456789abcdef0123456789abcdef";
        for channel in ["preview", "release"] {
            let profile = base.join("installs").join(channel).join("shared/profile");
            std::fs::create_dir_all(&profile).unwrap();
            std::fs::write(profile.join(".vault-id"), id).unwrap();
        }
        // No credential store is needed: the shared id must not be erased.
        purge_channel_data(&base, nus_compat::Channel::Preview, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(base.join("installs/release/shared/profile/.vault-id"))
                .unwrap(),
            id
        );
    }

    #[cfg(unix)]
    #[test]
    fn complete_uninstall_does_not_follow_linked_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("nus");
        let root = base.join("installs/preview/shared");
        let outside = temp.path().join("project/profile");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("settings.json"), "project").unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("profile")).unwrap();
        purge_channel_data(&base, nus_compat::Channel::Preview, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.join("settings.json")).unwrap(),
            "project"
        );
    }

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

// --- nus channel ----------------------------------------------------------------

/// What a channel change may carry into the new channel's profile: settings
/// and things you made, in plain files. Never the vault's (sessions, the
/// sync key, sign-ins, passwords), never history, and never over a file
/// the new profile already has.
const CARRY: &[&str] = &[
    "settings.json",
    "rules.luau",
    "me.json",
    "avatar.png",
    "themes",
    "layouts",
    "surfaces",
    "art",
    "fonts",
    "grammars",
    "blocklist.txt",
    "dangerous.txt",
    "sites.json",
    "shells.json",
    "folders.json",
    "containers.json",
    "assistants.json",
];

/// Copy `from` into `to` without replacing anything there; returns how
/// many files were copied. Links are not followed.
fn carry_tree(from: &Path, to: &Path) -> std::io::Result<usize> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        return Ok(0);
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        let mut n = 0;
        for e in std::fs::read_dir(from)?.flatten() {
            n += carry_tree(&e.path(), &to.join(e.file_name()))?;
        }
        return Ok(n);
    }
    if to.exists() {
        return Ok(0);
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::copy(from, to)?;
    Ok(1)
}

/// The shared profile of a channel's installs.
fn channel_profile(c: nus_compat::Channel) -> Option<PathBuf> {
    Some(
        data_home()?
            .join("installs")
            .join(c.directory())
            .join("shared")
            .join("profile"),
    )
}

pub fn channel(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    let want = match args.first().map(String::as_str) {
        Some("preview") => nus_compat::Channel::Preview,
        Some("stable") => nus_compat::Channel::Current,
        _ => return fail(&ui, "nus channel preview|stable [--carry] [--yes]"),
    };
    let carry = args.iter().any(|a| a == "--carry");
    let yes = args.iter().any(|a| a == "--yes" || a == "-y");
    let i = match detect() {
        Ok(i) => i,
        Err(e) => return fail(&ui, &e),
    };
    if i.channel == want {
        println!("  This copy is already on {}.", channel_word(want));
        return ExitCode::SUCCESS;
    }
    if i.method == Method::Source {
        return fail(
            &ui,
            "this nus is built from source: build the other channel from its branch",
        );
    }
    let start = Instant::now();
    // Stable only when there is one: the installer would otherwise fall
    // back to preview without a word.
    let version = match latest(want) {
        Ok(Some(v)) => v,
        Ok(None) => {
            return fail(
                &ui,
                &format!(
                    "there is no {} release for this platform yet",
                    channel_word(want)
                ),
            )
        }
        Err(e) => return fail(&ui, &e),
    };
    ui.row(
        "01",
        "Channel",
        &format!(
            "{} {} {} {version}",
            channel_word(i.channel),
            ui.g().arrow,
            channel_word(want)
        ),
        &ui.ok(),
        &took(start),
    );
    if !yes && std::io::stdin().is_terminal() {
        println!(
            "  {} installs beside this one, with its own profile{}.",
            channel_word(want),
            if carry {
                ", carrying your settings over"
            } else {
                ""
            }
        );
        if !ask("  Go on? [y/N] ").eq_ignore_ascii_case("y") {
            println!("  Nothing changed.");
            return ExitCode::SUCCESS;
        }
    }
    let tag = format!("v{version}");
    let status = match &i.method {
        Method::App(Some(_)) => {
            let token = if want == nus_compat::Channel::Preview {
                "nus@preview"
            } else {
                "nus"
            };
            let mut brew = Command::new("brew");
            brew.args(["install", "--cask", &format!("cbassuarez/tap/{token}")]);
            ui.stream("02", "Install", "with Homebrew", token, &mut brew)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        Method::Installer => {
            let mut ps = Command::new("powershell.exe");
            ps.args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-NoExit",
                "-Command",
                &format!("irm {SITE}/install.ps1 | iex"),
            ]);
            ps.env("NUS_VERSION", &tag);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                ps.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
            }
            ps.spawn()
                .map(|_| println!("  The install continues in a new window."))
                .map_err(|e| format!("could not start PowerShell: {e}"))
        }
        method => {
            let mut sh = Command::new("sh");
            sh.arg("-c")
                .arg(format!("curl -fsSL {SITE}/install.sh | sh"));
            // The exact release: never whichever the installer would pick.
            sh.env("NUS_VERSION", &tag);
            if matches!(method, Method::Account | Method::Folder) {
                sh.env("NUS_USER", "1");
            }
            match sh.status() {
                Ok(s) if s.success() => Ok(()),
                Ok(_) => Err("the installer stopped".to_string()),
                Err(e) => Err(format!("could not start the installer: {e}")),
            }
        }
    };
    if let Err(e) = status {
        return fail(&ui, &e);
    }
    if carry {
        let (Some(from), Some(to)) = (channel_profile(i.channel), channel_profile(want)) else {
            return fail(&ui, "no data folder to carry from");
        };
        let mut n = 0;
        for name in CARRY {
            if from.join(name).exists() {
                match carry_tree(&from.join(name), &to.join(name)) {
                    Ok(k) => n += k,
                    Err(e) => ui.hint(&format!("{name}: {e}")),
                }
            }
        }
        ui.row(
            "03",
            "Carried",
            &format!(
                "{n} files into the {} profile {} nothing replaced",
                channel_word(want),
                ui.g().dot
            ),
            &ui.ok(),
            &took(start),
        );
        ui.hint("sign-ins, sessions, the sync key and history stay with the old channel: pair it with nus sync pair");
    }
    ui.hint(&format!(
        "{} is still installed; its own nus uninstall removes it",
        channel_word(i.channel)
    ));
    ExitCode::SUCCESS
}

// --- nus logs -------------------------------------------------------------------

/// Text made safe to hand to someone: home paths shortened, the host's name
/// gone, and every secret-shaped thing removed — tokens, sync keys, bearer
/// and key=value secrets, URL queries and fragments, e-mail addresses,
/// IP addresses.
pub fn scrub(text: &str, home: Option<&str>, host: Option<&str>) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| {
        [
            (r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{8,}", "[token]"),
            (r"\bgithub_pat_[A-Za-z0-9_]{8,}", "[token]"),
            (r"\bglpat-[A-Za-z0-9_\-]{8,}", "[token]"),
            (r"\bnus5-[a-z2-7]{8,}", "nus5-[key]"),
            (r"(?i)\b(bearer|token|basic)\s+[A-Za-z0-9._~+/=\-]{12,}", "$1 [secret]"),
            (r"(?i)\b((?:access_|refresh_|id_)?token|password|passwd|secret|api_?key|auth(?:orization)?|client_secret|code)(\s*[=:]\s*)\S+", "$1$2[secret]"),
            (r"(https?://[^\s?#'\x22<>]+)[?#][^\s'\x22<>]*", "$1?[…]"),
            (r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}", "[email]"),
            (r"\b(?:\d{1,3}\.){3}\d{1,3}\b", "[ip]"),
            (r"\b(?:[0-9a-fA-F]{1,4}:){4,7}[0-9a-fA-F]{1,4}\b", "[ip]"),
        ]
        .into_iter()
        .map(|(p, r)| (Regex::new(p).expect("scrub pattern"), r))
        .collect()
    });
    let mut out = text.to_string();
    if let Some(h) = home.filter(|h| h.len() > 1) {
        out = out.replace(h, "~");
    }
    if let Some(h) = host.filter(|h| h.len() > 2) {
        out = out.replace(h, "[host]");
    }
    for (re, with) in rules {
        out = re.replace_all(&out, *with).into_owned();
    }
    out
}

fn host_name() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .or_else(|| std::env::var("COMPUTERNAME").ok())
}

pub fn logs(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    let Some(base) = data_home().map(|d| d.join("logs")) else {
        return fail(&ui, "no data folder");
    };
    let found: Vec<PathBuf> = ["preview", "release", "development"]
        .iter()
        .map(|c| base.join(c))
        .filter(|d| d.join("nus.log").is_file())
        .collect();
    if found.is_empty() {
        println!(
            "  No logs yet: nus writes {} once it has run.",
            base.join("<channel>").join("nus.log").display()
        );
        return ExitCode::SUCCESS;
    }
    if !args.iter().any(|a| a == "--bundle") {
        for dir in &found {
            for name in ["nus.log", "nus.1.log"] {
                let p = dir.join(name);
                if let Ok(m) = std::fs::metadata(&p) {
                    println!(
                        "  {}  {}",
                        p.display(),
                        ui.grey(&format!("{} KB", m.len() / 1024))
                    );
                }
            }
        }
        // The last of the newest log, as it is (it is yours, here).
        let newest = found
            .iter()
            .map(|d| d.join("nus.log"))
            .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        if let Some(p) = newest {
            println!();
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            for l in &lines[lines.len().saturating_sub(40)..] {
                println!("  {}", ui.grey(l));
            }
        }
        println!();
        ui.hint(&format!(
            "{} packs these, version and doctor, with secrets and addresses removed",
            ui.bold("nus logs --bundle")
        ));
        return ExitCode::SUCCESS;
    }
    let start = Instant::now();
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok());
    let host = host_name();
    let clean = |t: &str| scrub(t, home.as_deref(), host.as_deref());
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("nus-report-{stamp}");
    let dir = std::env::temp_dir().join(&name);
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(&ui, &format!("{}: {e}", dir.display()));
    }
    let me = std::env::current_exe().ok();
    let run = |arg: &str| -> String {
        me.as_ref()
            .and_then(|exe| {
                Command::new(exe)
                    .arg(arg)
                    .env("NO_COLOR", "1")
                    .env("TERM", "dumb")
                    .output()
                    .ok()
            })
            .map(|o| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                )
            })
            .unwrap_or_default()
    };
    let system = format!(
        "{} {} · {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("XDG_SESSION_TYPE").unwrap_or_default()
    );
    let mut files: Vec<(String, String)> = vec![
        ("version.txt".into(), run("version")),
        ("doctor.txt".into(), run("doctor")),
        ("system.txt".into(), system),
    ];
    for d in &found {
        let ch = d
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for log in ["nus.log", "nus.1.log"] {
            if let Ok(t) = std::fs::read_to_string(d.join(log)) {
                files.push((format!("{ch}-{log}"), t));
            }
        }
    }
    for (file, text) in &files {
        if let Err(e) = std::fs::write(dir.join(file), clean(text)) {
            return fail(&ui, &format!("{file}: {e}"));
        }
    }
    let out = std::env::current_dir()
        .unwrap_or_default()
        .join(format!("{name}.tar.gz"));
    let tar = Command::new(if cfg!(windows) { "tar.exe" } else { "tar" })
        .arg("-czf")
        .arg(&out)
        .arg("-C")
        .arg(std::env::temp_dir())
        .arg(&name)
        .status();
    let _ = std::fs::remove_dir_all(&dir);
    match tar {
        Ok(s) if s.success() => {
            ui.row(
                "01",
                "Report",
                &out.display().to_string(),
                &ui.ok(),
                &took(start),
            );
            ui.hint("removed: tokens, keys, secrets, URL queries, e-mail and IP addresses, this machine's name; home is ~");
            ui.hint("read it before you share it");
            ExitCode::SUCCESS
        }
        _ => fail(&ui, "could not pack the report (is tar installed?)"),
    }
}

#[cfg(test)]
mod scrub_tests {
    use super::*;

    #[test]
    fn secrets_and_addresses_do_not_leave() {
        let raw = "auth Bearer abcdefghijklmnopqrstuv for seb@example.com at 192.168.1.20:51807 \
                   ghp_ABCDEFGH12345678 key nus5-abcdefghijklmnop token=hunter2hunter2 \
                   https://github.com/login/oauth?code=XYZ123&state=1 /home/seb/nus/profile on seb-laptop";
        let s = scrub(raw, Some("/home/seb"), Some("seb-laptop"));
        for gone in [
            "abcdefghijklmnopqrstuv",
            "seb@example.com",
            "192.168.1.20",
            "ghp_ABCD",
            "abcdefghijklmnop",
            "hunter2",
            "XYZ123",
            "/home/seb",
            "seb-laptop",
        ] {
            assert!(!s.contains(gone), "{gone} leaked: {s}");
        }
        assert!(s.contains("https://github.com/login/oauth?[…]"), "{s}");
        assert!(s.contains("~/nus/profile"), "{s}");
    }

    #[test]
    fn carrying_never_replaces() {
        let base = std::env::temp_dir().join(format!("nus-carry-{}", std::process::id()));
        let (from, to) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(from.join("themes")).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("themes/dusk.theme"), "new").unwrap();
        std::fs::write(from.join("settings.json"), "new").unwrap();
        std::fs::write(to.join("settings.json"), "mine").unwrap();
        assert_eq!(
            carry_tree(&from.join("themes"), &to.join("themes")).unwrap(),
            1
        );
        assert_eq!(
            carry_tree(&from.join("settings.json"), &to.join("settings.json")).unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(to.join("settings.json")).unwrap(),
            "mine"
        );
        let _ = std::fs::remove_dir_all(base);
    }
}
