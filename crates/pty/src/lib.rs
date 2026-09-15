//! nus-pty — shell profiles and PTY sessions on top of `portable-pty`
//! (ConPTY on Windows, openpty elsewhere).

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

/// What to run in a new tab.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

impl Profile {
    /// The platform's sensible default: pwsh (falling back to Windows
    /// PowerShell) on Windows, `$SHELL` elsewhere.
    pub fn default_shell() -> Profile {
        #[cfg(windows)]
        {
            let program = if which("pwsh.exe") {
                "pwsh.exe"
            } else {
                "powershell.exe"
            };
            Profile {
                name: program.trim_end_matches(".exe").to_string(),
                program: program.to_string(),
                args: vec!["-NoLogo".into()],
                cwd: None,
                env: Vec::new(),
            }
        }
        #[cfg(not(windows))]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            Profile {
                name: shell.rsplit('/').next().unwrap_or("sh").to_string(),
                program: shell,
                args: vec!["-l".into()],
                cwd: None,
                env: Vec::new(),
            }
        }
    }

    /// A WSL distribution as a peer profile (Windows only; harmless elsewhere).
    pub fn wsl(distro: &str) -> Profile {
        Profile {
            name: format!("wsl:{distro}"),
            program: "wsl.exe".into(),
            args: vec!["-d".into(), distro.into()],
            cwd: None,
            env: Vec::new(),
        }
    }

    /// `ssh <host>` using the system ssh, so keys/agent/config are inherited.
    pub fn ssh(host: &str) -> Profile {
        Profile {
            name: format!("ssh:{host}"),
            program: "ssh".into(),
            args: vec![host.into()],
            cwd: None,
            env: Vec::new(),
        }
    }
}

#[cfg(windows)]
fn which(exe: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(exe).is_file()))
        .unwrap_or(false)
}

/// A running shell attached to a PTY. Output arrives on a channel fed by a
/// reader thread; `on_output` is called from that thread so the host can
/// wake its event loop.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: mpsc::Receiver<Vec<u8>>,
    cols: u16,
    rows: u16,
}

impl Pty {
    pub fn spawn(
        profile: &Profile,
        cols: u16,
        rows: u16,
        on_output: impl Fn() + Send + 'static,
    ) -> Result<Pty> {
        let system = native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(&profile.program);
        cmd.args(&profile.args);
        if let Some(cwd) = &profile.cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "nus");
        for (k, v) in &profile.env {
            cmd.env(k, v);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("spawn {}", profile.program))?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("clone reader")?;
        let writer = pair.master.take_writer().context("take writer")?;
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                            on_output();
                        }
                    }
                }
                // EOF: tell the host once more so it notices the exit.
                on_output();
            })
            .context("spawn reader thread")?;

        Ok(Pty {
            master: pair.master,
            writer,
            child,
            output: rx,
            cols,
            rows,
        })
    }

    /// Drain everything the reader thread has delivered so far.
    pub fn take_output(&self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.output.try_recv() {
            out.extend_from_slice(&chunk);
        }
        out
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.writer.write_all(bytes).context("pty write")?;
        self.writer.flush().ok();
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) -> Result<()> {
        if cols == self.cols && rows == self.rows {
            return Ok(());
        }
        self.cols = cols;
        self.rows = rows;
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: cols * cell_px.0,
                pixel_height: rows * cell_px.1,
            })
            .context("pty resize")
    }

    /// `Some(code)` once the child has exited.
    pub fn exit_code(&mut self) -> Option<u32> {
        self.child.try_wait().ok().flatten().map(|s| s.exit_code())
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.kill();
    }
}
