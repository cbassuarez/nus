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
    let host = p.args.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_default();
    if host.is_empty() {
        return p;
    }
    let mut args: Vec<String> = p.args.iter().filter(|a| a.starts_with('-') && *a != "-t").cloned().collect();
    args.insert(0, "-t".into());
    args.push(host);
    args.push(bootstrap());
    p.args = args;
    p
}

#[cfg(test)]
mod tests {
    use super::*;

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
