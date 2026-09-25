//! Shell integration over SSH, kitty's way: the integration rides along
//! the same connection. `ssh -t host <bootstrap>` writes nus's bash and
//! zsh scripts to `~/.cache/nus/` on the remote and execs the login shell
//! with them, so the remote prompt has marks, cwd (with the hostname, so
//! OSC 7 says where you are), exit codes and progress. Nothing to install
//! on the far side; the files are a few KB and rewritten each time.
//! PowerShell remotes get `pwsh -NoExit -Command` the same way.

const BASH: &str = include_str!("../assets/shell/nus.bash");
const ZSH: &str = include_str!("../assets/shell/nus.zsh");

/// The remote command: write the scripts, then exec the user's shell
/// with them. Quoted for `ssh -t host '<this>'`.
pub fn bootstrap() -> String {
    // A heredoc per script, with a delimiter that can't appear in them.
    let mut s = String::from("mkdir -p \"$HOME/.cache/nus\" && ");
    s.push_str(&format!("cat > \"$HOME/.cache/nus/nus.bash\" <<'NUS_EOF_1'\n{BASH}\nNUS_EOF_1\n"));
    s.push_str(&format!("cat > \"$HOME/.cache/nus/.zshrc\" <<'NUS_EOF_2'\n{ZSH}\nNUS_EOF_2\n"));
    // A terminfo entry for TERM=nus is not there yet; xterm-256color is what
    // every remote knows, and our answers to XTGETTCAP fill the gaps.
    s.push_str(
        "export NUS_SHELL_INTEGRATION=on TERM=xterm-256color COLORTERM=truecolor TERM_PROGRAM=nus; \
         case \"$(basename \"${SHELL:-/bin/sh}\")\" in \
           zsh) ZDOTDIR=\"$HOME/.cache/nus\" exec zsh -l ;; \
           bash) exec bash --rcfile \"$HOME/.cache/nus/nus.bash\" -i ;; \
           *) exec \"${SHELL:-/bin/sh}\" -l ;; \
         esac",
    );
    s
}

/// An `ssh <host>` profile, rewritten so the remote shell sources the
/// integration: the whole bootstrap rides as ssh's remote command.
pub fn integrate(mut p: nus_pty::Profile) -> nus_pty::Profile {
    if p.args.iter().any(|a| a.contains("NUS_EOF_1")) {
        return p;
    }
    let Some(host) = destination(&p.args) else {
        return p;
    };
    // Explicit remote commands and noninteractive sessions belong to the user.
    if host + 1 != p.args.len() || p.args.iter().any(|a| a == "-T" || a == "-N") { return p; }
    if !p.args.iter().any(|a| a == "-t" || a == "-tt") { p.args.insert(0, "-t".into()); }
    p.args.push(bootstrap());
    p
}

/// The host an ssh command line goes to (`user@` and the port dropped):
/// `ssh -p 2222 me@box.lan` → `box.lan`.
pub fn host_of(args: &[String]) -> Option<String> {
    let arg = &args[destination(args)?];
    let host = arg.strip_prefix("ssh://").unwrap_or(arg);
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.split([':', '/']).next().unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

/// A typed command that opens a tunnel (`ssh host`, `mosh host`, `et host`),
/// and where to: the words after the program, read as ssh reads them.
pub fn command_host(cmd: &str) -> Option<String> {
    let mut words = cmd.split_whitespace().map(|w| w.trim_matches(|c| c == '"' || c == '\'').to_string());
    let program = crate::blocks::program_of(cmd);
    if !TUNNELS.contains(&program.as_str()) {
        return None;
    }
    words.find(|w| w.rsplit(['/', '\\']).next().is_some_and(|b| b.trim_end_matches(".exe") == program))?;
    let rest: Vec<String> = words.collect();
    host_of(&rest).or_else(|| Some(program.clone()))
}

/// Programs whose session is somewhere else.
pub const TUNNELS: [&str; 4] = ["ssh", "mosh", "et", "autossh"];

/// Whether an OSC 7 host is this machine: its name, its short name, or
/// localhost. Compared without case; `.local` and domain tails ignored.
pub fn is_local(host: &str) -> bool {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let me = NAME.get_or_init(local_name);
    let short = |h: &str| h.split('.').next().unwrap_or(h).to_ascii_lowercase();
    let h = short(host);
    h.is_empty() || h == "localhost" || (!me.is_empty() && h == short(me))
}

#[cfg(unix)]
fn local_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most buf.len() bytes into a local buffer.
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

#[cfg(not(unix))]
fn local_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

impl crate::app::App {
    /// A tunnel's colour on this paper: the theme's, when it names one, or
    /// a hue of the host's own (the same host, the same hue, every time),
    /// at a lightness and chroma set in OKLCH so every host reads alike.
    /// Always at least 3:1 on the paper, as a rule or a chip must be.
    pub(crate) fn tunnel_color(&self, host: &str) -> nus_render::Color {
        let paper = self.paper();
        let want = self.surface.tunnel.unwrap_or_else(|| {
            // FNV-1a: stable across runs and builds, unlike the std hasher.
            let h = host.to_ascii_lowercase().bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193));
            let l = if nus_render::policy::luminance(paper) < 0.18 { 0.74 } else { 0.56 };
            nus_render::oklch::oklch(l, 0.15, (h % 360) as f32)
        });
        nus_render::oklch::readable(want, paper, 3.0)
    }

    /// The focused shell's tunnel, for the strip above it.
    pub(crate) fn active_tunnel(&self) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        match tab.focused_ref() {
            crate::app::Pane::Term(t) => t.tunnel(),
            _ => None,
        }
    }

    /// The tag a tunnelled shell wears: `SSH · HOST` on the tunnel's colour.
    /// Returns its width.
    pub(crate) fn draw_tunnel_tag(&mut self, scene: &mut nus_render::Scene, host: &str, x: f32, base: f32) -> f32 {
        let c = self.tunnel_color(host);
        let style = nus_render::Style { color: self.on_fill(c), ..self.label_strong() };
        let text = format!("SSH · {}", host.to_uppercase());
        let pad = self.px(7.0);
        let w = self.fonts.measure(style, &text) + pad * 2.0;
        let h = self.px(nus_render::theme::metric::LABEL_PX) + self.px(8.0);
        scene.rect(nus_render::Rect::new(x, base - h + self.px(4.0), w, h), c);
        self.fonts.draw(scene, style, x + pad, base, &text);
        w
    }
}

impl crate::app::TermPane {
    /// Where this shell really is, when that isn't here: the host its
    /// remote shell reports (OSC 7), the one an ssh typed at the prompt is
    /// still connected to, or the ssh profile's own. None on this machine.
    pub(crate) fn tunnel(&self) -> Option<String> {
        if let Some(h) = self.term.cwd_host.as_deref().filter(|h| !is_local(h)) {
            return Some(h.to_string());
        }
        if TUNNELS.contains(&self.program.as_str()) {
            if let Some(h) = self.blocks().last().filter(|b| b.running).and_then(|b| command_host(&b.cmd)) {
                return Some(h);
            }
        }
        self.ssh_host.clone()
    }
}

fn destination(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" { return (i+1 < args.len()).then_some(i+1); }
        if !arg.starts_with('-') { return (!arg.is_empty()).then_some(i); }
        if arg.len() == 2 && "BbcDEeFIiJLlmOopQRSWw".contains(arg.as_bytes()[1] as char) {
            i += 1; // This option's argument is not the destination.
            if i >= args.len() { return None; }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_preserves_option_values_and_explicit_remote_commands() {
        for args in [vec!["-p", "2222", "-i", "/keys/key with space", "user@host"], vec!["-o", "ProxyJump=jump", "-p2222", "host"]] {
            let mut p = nus_pty::Profile::ssh("unused");
            p.args = args.iter().map(|s|s.to_string()).collect();
            let integrated = integrate(p.clone());
            assert_eq!(&integrated.args[1..integrated.args.len()-1], &p.args);
            assert_eq!(integrated.args[0], "-t");
        }
        for args in [vec!["host", "echo hello"], vec!["-T", "host"], vec!["-N", "-L", "3000:localhost:3000", "host"], vec!["-p"]] {
            let mut p = nus_pty::Profile::ssh("unused");
            p.args = args.iter().map(|s|s.to_string()).collect();
            assert_eq!(integrate(p.clone()).args, p.args);
        }
    }

    #[test]
    fn hosts_from_command_lines() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(host_of(&a(&["-p", "2222", "me@box.lan"])).as_deref(), Some("box.lan"));
        assert_eq!(host_of(&a(&["ssh://me@box:22"])).as_deref(), Some("box"));
        assert_eq!(command_host("ssh -i key prod-1").as_deref(), Some("prod-1"));
        assert_eq!(command_host("mosh me@pi").as_deref(), Some("pi"));
        assert_eq!(command_host("ls -la"), None);
        assert!(is_local("localhost") && is_local(""));
        assert!(!is_local("definitely-not-this-machine.example"));
    }

    #[test]
    fn bootstrap_shape() {
        let b = bootstrap();
        assert!(b.starts_with("mkdir -p"));
        assert!(b.contains("NUS_EOF_1\n") && b.contains("\nNUS_EOF_1\n"));
        assert!(b.contains("exec bash --rcfile"));
        assert!(b.contains("ZDOTDIR="));
        // The scripts must not contain their own delimiters.
        assert!(!BASH.contains("NUS_EOF_1") && !ZSH.contains("NUS_EOF_2"));
    }
}
