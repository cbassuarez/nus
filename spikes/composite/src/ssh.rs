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
