//! Shell integration: the bundled scripts, written to profile/shell at
//! startup, and how each shell is launched so it sources them. Ghostty's
//! way — detect the shell by its program name and inject; a "none"
//! switch in settings turns it off. The scripts emit OSC 133 prompt
//! marks, OSC 7 for the working directory, and keep the user's own
//! prompt and rc files.

use std::path::PathBuf;

const PS1: &str = include_str!("../assets/shell/nus.ps1");
const BASH: &str = include_str!("../assets/shell/nus.bash");
const ZSH: &str = include_str!("../assets/shell/nus.zsh");
const FISH: &str = include_str!("../assets/shell/nus.fish");

fn dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("shell")
}

/// Write the scripts out (idempotent; overwrites so upgrades take).
pub fn install() -> PathBuf {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    for (name, body) in [("nus.ps1", PS1), ("nus.bash", BASH), (".zshrc", ZSH), ("nus.fish", FISH)] {
        let p = d.join(name);
        if std::fs::read_to_string(&p).ok().as_deref() != Some(body) {
            let _ = std::fs::write(&p, body);
        }
    }
    d
}

/// Which shell a profile runs, by program name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    PowerShell,
    Cmd,
    Bash,
    Zsh,
    Fish,
    Nu,
    Other,
}

pub fn kind_of(program: &str) -> Kind {
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    let base = base.trim_end_matches(".exe");
    match base {
        "pwsh" | "powershell" => Kind::PowerShell,
        "cmd" => Kind::Cmd,
        // /bin/sh is commonly dash on Linux. Bash's --rcfile option makes
        // those shells exit immediately; do not assume a compatible binary.
        "bash" => Kind::Bash,
        "zsh" => Kind::Zsh,
        "fish" => Kind::Fish,
        "nu" | "nushell" => Kind::Nu,
        _ => Kind::Other,
    }
}

/// What integration a shell gets, for the welcome page and settings.
pub fn describe(kind: Kind) -> &'static str {
    match kind {
        Kind::PowerShell => "prompt marks, cwd, exit codes · via -NoExit -Command",
        Kind::Cmd => "prompt marks and cwd · via PROMPT",
        Kind::Bash => "prompt marks, cwd, exit codes · via --rcfile (loads your .bashrc)",
        Kind::Zsh => "prompt marks, cwd, exit codes · via ZDOTDIR (loads your .zshrc)",
        Kind::Fish => "cwd (fish 4 already marks prompts) · via -C",
        Kind::Nu => "built in: nushell marks prompts and reports cwd itself",
        Kind::Other => "none: this shell isn't known",
    }
}

/// The profile, rewritten so its shell sources the integration.
pub fn integrate(p: nus_pty::Profile, on: bool) -> nus_pty::Profile {
    let d = install();
    integrate_at(p, on, &d)
}

fn integrate_at(mut p: nus_pty::Profile, on: bool, d: &std::path::Path) -> nus_pty::Profile {
    p.env.push(("NUS_SHELL_INTEGRATION".into(), if on { "on".into() } else { "off".into() }));
    if !on {
        return p;
    }
    // ssh: the integration goes over the wire (see ssh.rs).
    if kind_of(&p.program) == Kind::Other && p.program.rsplit(['/', '\\']).next().unwrap_or("").trim_end_matches(".exe") == "ssh" {
        return crate::ssh::integrate(p);
    }
    let path = |name: &str| d.join(name).to_string_lossy().replace('\\', "/");
    match kind_of(&p.program) {
        Kind::PowerShell => {
            // -NoExit -EncodedCommand runs after the user's profile, stays
            // interactive, and isn't subject to the execution policy the way
            // dot-sourcing a file is.
            if !p.args.iter().any(|a| a.eq_ignore_ascii_case("-command") || a.eq_ignore_ascii_case("-c") || a.eq_ignore_ascii_case("-file") || a.eq_ignore_ascii_case("-encodedcommand")) {
                let utf16: Vec<u8> = PS1.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
                p.args.push("-NoExit".into());
                p.args.push("-EncodedCommand".into());
                p.args.push(base64(&utf16));
            }
        }
        Kind::Cmd => {
            // cmd's PROMPT: $E is ESC, $P the path, $G '>'.
            p.env.push(("PROMPT".into(), "$E]133;D$E\\$E]7;file:///$P$E\\$E]133;A$E\\$P$G$E]133;B$E\\".into()));
        }
        Kind::Bash => {
            if !p.args.iter().any(|a| a == "--rcfile" || a == "-c") {
                p.args.retain(|a| a != "-l");
                p.args.push("--rcfile".into());
                p.args.push(path("nus.bash"));
                p.args.push("-i".into());
            }
        }
        Kind::Zsh => {
            let home = std::env::var("ZDOTDIR").or_else(|_| std::env::var("HOME")).unwrap_or_default();
            p.env.push(("NUS_USER_ZDOTDIR".into(), home));
            p.env.push(("ZDOTDIR".into(), d.to_string_lossy().to_string()));
        }
        Kind::Fish => {
            if !p.args.iter().any(|a| a == "-c") {
                p.args.push("-C".into());
                p.args.push(format!("source {}", fish_quote(&path("nus.fish"))));
            }
        }
        Kind::Nu | Kind::Other => {}
    }
    p
}

fn fish_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(program: &str, args: &[&str]) -> nus_pty::Profile {
        nus_pty::Profile {name:"fixture".into(),program:program.into(),args:args.iter().map(|s|s.to_string()).collect(),cwd:None,env:vec![]}
    }
    #[test]
    fn posix_sh_does_not_receive_bash_only_startup_flags() {
        for shell in ["/bin/sh", "/bin/dash", "/bin/ksh"] {
            let p = integrate_at(profile(shell, &["-l"]), true, std::path::Path::new("/fixture"));
            assert_eq!(p.args, ["-l"]);
            assert_eq!(p.program, shell);
        }
        let p = integrate_at(profile("/bin/bash", &["-l"]), true, std::path::Path::new("/fixture"));
        assert!(p.args.iter().any(|arg| arg == "--rcfile"));
    }
    #[test]
    fn fish_script_path_is_quoted_as_one_argument() {
        let p = integrate_at(profile("fish", &[]), true, std::path::Path::new("/home/O'Brien/nus"));
        assert_eq!(p.args[1], "source '/home/O\\'Brien/nus/nus.fish'");
        assert_eq!(fish_quote("a\\b'c"), "'a\\\\b\\'c'");
    }
    #[test]
    fn windows_shell_names_are_case_insensitive_and_paths_are_supported() {
        assert_eq!(kind_of(r"C:\Program Files\PowerShell\7\PWSH.EXE"), Kind::PowerShell);
        let p = integrate_at(profile("powershell.exe", &["-File", "work.ps1"]), true, std::path::Path::new("/fixture"));
        assert_eq!(p.args, ["-File", "work.ps1"]);
    }
}

/// Standard base64, for -EncodedCommand.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
