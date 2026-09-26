//! Finding the shells on this machine, and the machines ssh knows.
//!
//! Each platform keeps its shells somewhere different, and an app started
//! from the Dock or Start menu sees a short PATH, so looking only at
//! `$SHELL` finds one. This looks where shells actually live: the login
//! shell from the user database, `/etc/shells`, Homebrew, Nix, cargo, the
//! PATH; on Windows cmd, Git Bash (and scoop's), MSYS2, Cygwin and shells
//! on the PATH. The same binary reached two ways (a symlink, usrmerge's
//! `/bin` and `/usr/bin`) is listed once. A second binary of a shell that
//! is already listed is named for where it came from: `bash (homebrew)`.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::Profile;

/// Shells worth offering, most wanted first. Legacy shells come last.
pub const KNOWN: &[&str] = &[
    "zsh", "bash", "fish", "nu", "pwsh", "xonsh", "elvish", "ksh", "mksh", "tcsh", "csh", "dash", "sh",
];

/// Shells found but kept out of the way unless asked for: they're on every
/// Unix and rarely what anyone opens a tab for.
pub const QUIET: &[&str] = &["ksh", "mksh", "tcsh", "csh", "dash", "sh"];

/// A shell's name without its folder or `.exe`.
pub fn base(program: &str) -> String {
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let lower = file.to_ascii_lowercase();
    lower.strip_suffix(".exe").map(str::to_string).unwrap_or(lower)
}

/// Where a binary came from, in one word, for its name and the list.
pub fn source_of(path: &Path, home: Option<&Path>) -> &'static str {
    let s = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let under = |p: &str| home.map(|h| h.join(p).to_string_lossy().replace('\\', "/").to_ascii_lowercase()).is_some_and(|h| s.starts_with(&h));
    if s.starts_with("/opt/homebrew/") || s.starts_with("/home/linuxbrew/") || s.contains("/cellar/") || (cfg!(target_os = "macos") && s.starts_with("/usr/local/")) {
        "homebrew"
    } else if s.starts_with("/nix/") || s.starts_with("/run/current-system/") || under(".nix-profile") {
        "nix"
    } else if under(".cargo") {
        "cargo"
    } else if under(".local") {
        "local"
    } else if s.starts_with("/snap/") {
        "snap"
    } else if s.contains("/msys64/") || s.contains("/msys2/") {
        "msys2"
    } else if s.contains("/cygwin") {
        "cygwin"
    } else if s.contains("/scoop/") {
        "scoop"
    } else if s.contains("/program files/git/") || s.contains("/program files (x86)/git/") {
        "git"
    } else if s.starts_with("/bin/") || s.starts_with("/usr/bin/") || s.starts_with("/sbin/") || s.starts_with("/usr/sbin/") || s.starts_with("c:/windows/") {
        "system"
    } else if s.starts_with("/usr/local/") {
        "local"
    } else {
        "path"
    }
}

/// A name no other entry has: the shell's own, else with where it came
/// from, else with its folder.
pub fn unique_name(base: &str, source: &str, path: &Path, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let with_source = format!("{base} ({source})");
    if !taken.contains(&with_source) {
        return with_source;
    }
    let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let with_dir = format!("{base} ({dir})");
    if !taken.contains(&with_dir) {
        return with_dir;
    }
    (2..).map(|n| format!("{base} ({source} {n})")).find(|n| !taken.contains(n)).unwrap_or_default()
}

/// The arguments that make each shell a login shell, as a terminal starts one.
pub fn login_args(base: &str) -> Vec<String> {
    match base {
        "zsh" | "bash" | "ksh" | "mksh" | "tcsh" | "csh" | "fish" | "nu" | "xonsh" | "dash" | "sh" => vec!["-l".into()],
        "pwsh" | "powershell" => vec!["-NoLogo".into()],
        _ => Vec::new(),
    }
}

/// The absolute paths listed in an `/etc/shells`.
pub fn parse_etc_shells(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && l.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// A glob with `*` and `?` against one file name (ssh's Include patterns).
fn glob_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

/// The concrete hosts in an ssh config, following `Include` (relative to
/// `~/.ssh`, with `*` and `?`), a few levels deep. Patterns (`*`, `?`, `!`)
/// aren't hosts one can open; each host is listed once, in file order.
pub fn ssh_hosts(text: &str, ssh_dir: &Path, read: &dyn Fn(&Path) -> Option<String>, list: &dyn Fn(&Path) -> Vec<PathBuf>, depth: usize, out: &mut Vec<String>) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `Keyword value` or `Keyword=value`, keyword in any case.
        let (key, rest) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => (&line[..i], line[i + 1..].trim_start_matches(|c: char| c.is_whitespace() || c == '=')),
            None => continue,
        };
        if key.eq_ignore_ascii_case("host") {
            for host in rest.split_whitespace() {
                let host = host.trim_matches('"');
                if !host.is_empty() && !host.contains(['*', '?', '!']) && !out.iter().any(|h| h == host) {
                    out.push(host.to_string());
                }
            }
        } else if key.eq_ignore_ascii_case("include") && depth > 0 {
            for pattern in rest.split_whitespace() {
                let pattern = pattern.trim_matches('"');
                let expanded = pattern.strip_prefix("~/").map(|p| ssh_dir.parent().unwrap_or(ssh_dir).join(p)).unwrap_or_else(|| {
                    let p = Path::new(pattern);
                    if p.is_absolute() { p.to_path_buf() } else { ssh_dir.join(p) }
                });
                let name = expanded.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let files: Vec<PathBuf> = if name.contains(['*', '?']) {
                    let dir = expanded.parent().map(Path::to_path_buf).unwrap_or_default();
                    let mut v: Vec<PathBuf> = list(&dir).into_iter().filter(|f| f.file_name().is_some_and(|n| glob_match(&name, &n.to_string_lossy()))).collect();
                    v.sort();
                    v
                } else {
                    vec![expanded]
                };
                for f in files {
                    if let Some(t) = read(&f) {
                        ssh_hosts(&t, ssh_dir, read, list, depth - 1, out);
                    }
                }
            }
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The login shell: `$SHELL`, else the user database's (an app opened
/// from the Dock may have no `$SHELL`), else `/bin/sh`.
#[cfg(unix)]
pub fn login_shell() -> String {
    if let Some(s) = std::env::var("SHELL").ok().filter(|s| !s.is_empty() && Path::new(s).is_file()) {
        return s;
    }
    // SAFETY: getpwuid returns a pointer into static storage or null; the
    // shell string is copied out at once, on this thread.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_shell.is_null() {
            let s = std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy().to_string();
            if !s.is_empty() && Path::new(&s).is_file() {
                return s;
            }
        }
    }
    "/bin/sh".into()
}

/// Where to look beyond `/etc/shells` and the PATH an app was given.
#[cfg(unix)]
fn shell_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin", "/run/current-system/sw/bin", "/nix/var/nix/profiles/default/bin", "/snap/bin", "/usr/bin", "/bin"].iter().map(PathBuf::from).collect();
    if let Some(h) = home {
        for d in [".nix-profile/bin", ".cargo/bin", ".local/bin"] {
            dirs.push(h.join(d));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

/// Local shells beyond the ones already in `have`, most wanted first.
#[cfg(unix)]
pub fn local_shells(have: &[Profile]) -> Vec<Profile> {
    let home = home();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut taken: HashSet<String> = have.iter().map(|p| p.name.clone()).collect();
    for p in have {
        if let Ok(c) = std::fs::canonicalize(&p.program) {
            seen.insert(c);
        }
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_to_string("/etc/shells").map(|t| parse_etc_shells(&t)).unwrap_or_default();
    for dir in shell_dirs(home.as_deref()) {
        for name in KNOWN {
            candidates.push(dir.join(name));
        }
    }
    let mut found: Vec<(usize, PathBuf)> = Vec::new();
    for path in candidates {
        let b = base(&path.to_string_lossy());
        let Some(rank) = KNOWN.iter().position(|k| *k == b) else { continue };
        if !executable(&path) {
            continue;
        }
        let Ok(canon) = std::fs::canonicalize(&path) else { continue };
        if !seen.insert(canon) {
            continue;
        }
        found.push((rank, path));
    }
    // Stable: most wanted first, then as found (/etc/shells before brew).
    found.sort_by_key(|(rank, _)| *rank);
    found
        .into_iter()
        .map(|(_, path)| {
            let b = base(&path.to_string_lossy());
            let name = unique_name(&b, source_of(&path, home.as_deref()), &path, &taken);
            taken.insert(name.clone());
            Profile { name, program: path.display().to_string(), args: login_args(&b), cwd: None, env: Vec::new() }
        })
        .collect()
}

/// Windows: cmd, Git Bash from scoop when Git's own isn't there, MSYS2,
/// Cygwin, and shells on the PATH (nu, fish, elvish, xonsh, bash).
#[cfg(windows)]
pub fn local_shells(have: &[Profile]) -> Vec<Profile> {
    struct Found {
        home: Option<PathBuf>,
        taken: HashSet<String>,
        seen: HashSet<String>,
        out: Vec<Profile>,
    }
    impl Found {
        fn add(&mut self, name: &str, path: PathBuf, args: Vec<String>, env: Vec<(String, String)>) {
            let key = path.display().to_string().to_ascii_lowercase();
            if !path.is_file() || self.seen.contains(&key) {
                return;
            }
            self.seen.insert(key);
            let n = unique_name(name, source_of(&path, self.home.as_deref()), &path, &self.taken);
            self.taken.insert(n.clone());
            self.out.push(Profile { name: n, program: path.display().to_string(), args, cwd: None, env });
        }
    }
    let mut f = Found {
        home: home(),
        taken: have.iter().map(|p| p.name.clone()).collect(),
        seen: have.iter().map(|p| p.program.to_ascii_lowercase()).collect(),
        out: Vec::new(),
    };
    let comspec = std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
    f.add("cmd", comspec, Vec::new(), Vec::new());
    if !f.taken.contains("git bash") {
        if let Some(h) = f.home.clone() {
            f.add("git bash", h.join(r"scoop\apps\git\current\bin\bash.exe"), vec!["--login".into(), "-i".into()], Vec::new());
        }
    }
    // MSYS2 and Cygwin start in the folder they're opened in with CHERE_INVOKING.
    let chere = || vec![("CHERE_INVOKING".to_string(), "1".to_string())];
    for root in [r"C:\msys64", r"C:\tools\msys64"] {
        let mut env = chere();
        env.push(("MSYSTEM".into(), "UCRT64".into()));
        f.add("msys2", Path::new(root).join(r"usr\bin\bash.exe"), vec!["--login".into(), "-i".into()], env);
    }
    for root in [r"C:\cygwin64", r"C:\cygwin"] {
        f.add("cygwin", Path::new(root).join(r"bin\bash.exe"), vec!["--login".into(), "-i".into()], chere());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.push(PathBuf::from(r"C:\Program Files\nu\bin"));
    if let Some(h) = f.home.clone() {
        dirs.push(h.join(r"scoop\shims"));
        dirs.push(h.join(r".cargo\bin"));
    }
    for name in ["nu", "fish", "elvish", "xonsh", "bash"] {
        for dir in &dirs {
            // System32's bash.exe is WSL's launcher, already listed per distro.
            if name == "bash" && dir.to_string_lossy().to_ascii_lowercase().contains(r"\windows\") {
                continue;
            }
            let path = dir.join(format!("{name}.exe"));
            if path.is_file() {
                f.add(name, path, login_args(name), Vec::new());
                break;
            }
        }
    }
    f.out
}

#[cfg(not(any(unix, windows)))]
pub fn local_shells(_have: &[Profile]) -> Vec<Profile> {
    Vec::new()
}

/// Hosts from `~/.ssh/config` and what it includes.
pub fn ssh_config_hosts() -> Vec<String> {
    let Some(home) = home() else { return Vec::new() };
    let dir = home.join(".ssh");
    let Ok(text) = std::fs::read_to_string(dir.join("config")) else { return Vec::new() };
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    let list = |d: &Path| std::fs::read_dir(d).map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file()).collect()).unwrap_or_default();
    let mut out = Vec::new();
    ssh_hosts(&text, &dir, &read, &list, 4, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn names_stay_unique_and_say_where_from() {
        let mut taken: HashSet<String> = ["zsh".to_string()].into();
        let p = Path::new("/opt/homebrew/bin/zsh");
        assert_eq!(unique_name("zsh", source_of(p, None), p, &taken), "zsh (homebrew)");
        taken.insert("zsh (homebrew)".into());
        assert_eq!(unique_name("zsh", "homebrew", Path::new("/opt/homebrew/opt/zsh/bin/zsh"), &taken), "zsh (/opt/homebrew/opt/zsh/bin)");
        assert_eq!(unique_name("fish", "homebrew", Path::new("/opt/homebrew/bin/fish"), &taken), "fish");
    }

    #[test]
    fn sources() {
        let home = Path::new("/Users/seb");
        assert_eq!(source_of(Path::new("/bin/bash"), Some(home)), "system");
        assert_eq!(source_of(Path::new("/nix/store/x-bash/bin/bash"), Some(home)), "nix");
        assert_eq!(source_of(Path::new("/Users/seb/.cargo/bin/nu"), Some(home)), "cargo");
        assert_eq!(source_of(Path::new(r"C:\msys64\usr\bin\bash.exe"), None), "msys2");
        assert_eq!(source_of(Path::new(r"C:\Program Files\Git\bin\bash.exe"), None), "git");
    }

    #[test]
    fn etc_shells_and_bases() {
        let t = "# comment\n/bin/zsh\n\n/opt/homebrew/bin/fish\nnot/absolute\n";
        assert_eq!(parse_etc_shells(t), vec![PathBuf::from("/bin/zsh"), PathBuf::from("/opt/homebrew/bin/fish")]);
        assert_eq!(base(r"C:\Program Files\PowerShell\7\pwsh.exe"), "pwsh");
        assert_eq!(base("/usr/bin/zsh"), "zsh");
        assert_eq!(login_args("elvish"), Vec::<String>::new());
    }

    #[test]
    fn ssh_hosts_follow_includes_and_skip_patterns() {
        let dir = PathBuf::from("/h/.ssh");
        let files: HashMap<PathBuf, &str> = [
            (dir.join("conf.d/work"), "Host bastion prod-*\n  HostName 10.0.0.1\nHost=db"),
            (dir.join("conf.d/home"), "host nas\nInclude deeper"),
            (dir.join("deeper"), "Host deep"),
        ]
        .into();
        let read = |p: &Path| files.get(p).map(|s| s.to_string());
        let list = |d: &Path| files.keys().filter(|p| p.parent() == Some(d)).cloned().collect::<Vec<_>>();
        let mut out = Vec::new();
        ssh_hosts("Host dev dev\nInclude conf.d/*\nHost * !bad\nMatch host x", &dir, &read, &list, 4, &mut out);
        assert_eq!(out, ["dev", "nas", "deep", "bastion", "db"]);
    }
}
