//! nus-pty — shell profiles and PTY sessions on top of `portable-pty`
//! (ConPTY on Windows, openpty elsewhere).

pub mod ports;

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

pub mod hold;

/// A running shell attached to a PTY. Output arrives on a channel fed by a
/// reader thread; `on_output` is called from that thread so the host can
/// wake its event loop. The pty is either ours (`Local`) or a holder's
/// (`Held`): a `nus-hold` process that outlives us, reached over a socket.
pub struct Pty {
    inner: Inner,
    cols: u16,
    rows: u16,
    /// `kill` already went through; `drop` has nothing left to reap.
    reaped: bool,
}

enum Inner {
    Local {
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        child: Box<dyn Child + Send + Sync>,
        output: mpsc::Receiver<Vec<u8>>,
    },
    Held(hold::Client),
}

impl Pty {
    /// A shell in a holder process, so it survives this app.
    pub fn spawn_held(
        profile: &Profile,
        cols: u16,
        rows: u16,
        dir: &std::path::Path,
        on_output: impl Fn() + Send + 'static,
    ) -> Result<Pty> {
        let client = hold::spawn_held(profile, cols, rows, dir, on_output)?;
        Ok(Pty {
            inner: Inner::Held(client),
            cols,
            rows,
            reaped: false,
        })
    }

    /// Back to a holder that is already running; the ring replays first.
    pub fn attach(
        info: hold::Info,
        cols: u16,
        rows: u16,
        on_output: impl Fn() + Send + 'static,
    ) -> Result<Pty> {
        let client = hold::Client::attach(info, on_output)?;
        let mut pty = Pty {
            inner: Inner::Held(client),
            cols: 0,
            rows: 0,
            reaped: false,
        };
        pty.resize(cols, rows, (0, 0))?;
        Ok(pty)
    }

    /// The holder's id when the shell is held.
    pub fn held_id(&self) -> Option<&str> {
        match &self.inner {
            Inner::Held(c) => Some(&c.info.id),
            Inner::Local { .. } => None,
        }
    }

    /// Let go without killing: the holder keeps the shell. Only a held
    /// shell can be detached; a local one is simply dropped.
    pub fn detach(self) {
        drop(self);
    }

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
        if std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
        }
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
        // Apply backpressure while the UI is busy: at most 2 MiB queued.
        let (tx, rx) = mpsc::sync_channel(32);
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
            inner: Inner::Local {
                master: pair.master,
                writer,
                child,
                output: rx,
            },
            cols,
            rows,
            reaped: false,
        })
    }

    /// Drain everything the reader thread has delivered so far.
    pub fn take_output(&self) -> Vec<u8> {
        let rx = match &self.inner {
            Inner::Local { output, .. } => output,
            Inner::Held(c) => &c.output,
        };
        let mut out = Vec::new();
        while out.len() < 1024 * 1024 {
            let Ok(chunk) = rx.try_recv() else { break };
            out.extend_from_slice(&chunk);
        }
        out
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        match &mut self.inner {
            Inner::Local { writer, .. } => {
                writer.write_all(bytes).context("pty write")?;
                writer.flush().ok();
                Ok(())
            }
            Inner::Held(c) => c.write(bytes),
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) -> Result<()> {
        if cols == self.cols && rows == self.rows {
            return Ok(());
        }
        self.cols = cols;
        self.rows = rows;
        match &mut self.inner {
            Inner::Local { master, .. } => master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: cols * cell_px.0,
                    pixel_height: rows * cell_px.1,
                })
                .context("pty resize"),
            Inner::Held(c) => c.resize(cols, rows, cell_px),
        }
    }

    /// `Some(code)` once the child has exited.
    pub fn exit_code(&mut self) -> Option<u32> {
        match &mut self.inner {
            Inner::Local { child, .. } => child.try_wait().ok().flatten().map(|s| s.exit_code()),
            Inner::Held(c) => c.exit_code(),
        }
    }

    /// Stop the shell and everything it started. Killing the shell alone
    /// leaves its children behind — reparented to init, still holding
    /// their ports — so the tree is read first and signalled leaves up.
    pub fn kill(&mut self) {
        self.kill_with(None);
    }

    /// The same, over a process table already read: closing a stack of
    /// tabs reads it once and hands it to each shell.
    pub fn kill_with(&mut self, tree: Option<&std::collections::HashMap<u32, (u32, String)>>) {
        // Children first, while the shell is still their parent.
        if let Some(pid) = self.local_pid() {
            match tree {
                Some(t) => ports::reap_trees_in(t, &[pid]),
                None => ports::reap_trees(&[pid]),
            }
        }
        self.kill_reaped();
    }

    /// The pid whose tree goes when this shell is closed: a local
    /// shell's own. A held shell is the holder's, and the holder reaps
    /// it on its side, so there is nothing here to walk.
    pub fn local_pid(&self) -> Option<u32> {
        match &self.inner {
            Inner::Local { .. } => self.pid(),
            Inner::Held(_) => None,
        }
    }

    /// Stop the shell, its tree already reaped (`ports::reap_trees_in`).
    /// Closing several tabs reaps them together — one reading of the
    /// process table, one grace period — and then calls this on each.
    pub fn kill_reaped(&mut self) {
        match &mut self.inner {
            Inner::Local { child, .. } => {
                let _ = child.kill();
            }
            Inner::Held(c) => c.kill(),
        }
        self.reaped = true;
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // A local shell dies with us, and takes its children along; a
        // held one is the holder's to keep.
        if self.reaped {
            return;
        }
        if matches!(self.inner, Inner::Local { .. }) {
            self.kill_with(None);
        }
    }
}

impl Profile {
    /// Everything a new tab could run: the default shell, WSL distros
    /// (Windows), and every concrete `Host` in ~/.ssh/config.
    pub fn discover() -> Vec<Profile> {
        let mut out = vec![Profile::default_shell()];
        #[cfg(windows)]
        {
            if which("pwsh.exe") && which("powershell.exe") {
                out.push(Profile {
                    name: "powershell".into(),
                    program: "powershell.exe".into(),
                    args: vec!["-NoLogo".into()],
                    cwd: None,
                    env: Vec::new(),
                });
            }
            // Git for Windows brings a bash; offer it when it's there.
            for dir in [
                r"C:\Program Files\Git\bin",
                r"C:\Program Files (x86)\Git\bin",
            ] {
                let bash = std::path::Path::new(dir).join("bash.exe");
                if bash.is_file() {
                    out.push(Profile {
                        name: "git bash".into(),
                        program: bash.display().to_string(),
                        args: vec!["--login".into(), "-i".into()],
                        cwd: None,
                        env: Vec::new(),
                    });
                    break;
                }
            }
            if let Ok(o) = std::process::Command::new("wsl.exe")
                .args(["-l", "-q"])
                .output()
            {
                // wsl.exe prints UTF-16LE.
                let u16s: Vec<u16> = o
                    .stdout
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                for line in String::from_utf16_lossy(&u16s).lines() {
                    let d = line.trim().trim_matches('\0');
                    if !d.is_empty() {
                        out.push(Profile::wsl(d));
                    }
                }
            }
        }
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let cfg = std::path::Path::new(&home).join(".ssh").join("config");
            if let Ok(text) = std::fs::read_to_string(cfg) {
                for line in text.lines() {
                    let line = line.trim();
                    if let Some(rest) = line
                        .strip_prefix("Host ")
                        .or_else(|| line.strip_prefix("host "))
                    {
                        for host in rest.split_whitespace() {
                            if !host.contains(['*', '?', '!']) {
                                out.push(Profile::ssh(host));
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

impl Pty {
    /// The shell's process id.
    pub fn pid(&self) -> Option<u32> {
        match &self.inner {
            Inner::Local { child, .. } => child.process_id(),
            Inner::Held(c) => Some(c.info.pid),
        }
    }

    /// Name of a process the shell is currently running (its first child),
    /// if any — the signal that closing this tab would kill real work.
    pub fn foreground_process(&self) -> Option<String> {
        let pid = self.pid()?;
        child_process_name(pid)
    }
}

#[cfg(windows)]
fn child_process_name(parent: u32) -> Option<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ParentProcessID == parent {
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    // conhost is ConPTY's own helper, not user work.
                    if !name.eq_ignore_ascii_case("conhost.exe") {
                        found = Some(name.trim_end_matches(".exe").to_string());
                        break;
                    }
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
fn child_process_name(parent: u32) -> Option<String> {
    // `pgrep -P` is on every Linux and macOS box; a proper /proc walk is v1.
    let out = std::process::Command::new("pgrep")
        .args(["-P", &parent.to_string(), "-l"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|s| s.to_string())
}

/// A TCP port something on this machine is listening on, with its process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListeningPort {
    pub port: u16,
    pub pid: u32,
    pub process: String,
}

/// Listening TCP ports (loopback and all-interfaces), deduplicated by port.
pub fn listening_ports() -> Vec<ListeningPort> {
    let mut out: Vec<ListeningPort> = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(o) = std::process::Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() >= 5 && f[0] == "TCP" && f[3] == "LISTENING" {
                    let port = f[1].rsplit(':').next().and_then(|p| p.parse::<u16>().ok());
                    let pid = f[4].parse::<u32>().ok();
                    if let (Some(port), Some(pid)) = (port, pid) {
                        if !out.iter().any(|p| p.port == port) {
                            out.push(ListeningPort {
                                port,
                                pid,
                                process: String::new(),
                            });
                        }
                    }
                }
            }
        }
        let names = process_names();
        for p in out.iter_mut() {
            p.process = names.get(&p.pid).cloned().unwrap_or_default();
        }
    }
    #[cfg(not(windows))]
    {
        // ss (Linux) then lsof (macOS); both print "pid=" / "(PID)" forms we can mine.
        let text = std::process::Command::new("ss")
            .args(["-ltnp"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .or_else(|| {
                std::process::Command::new("lsof")
                    .args(["-iTCP", "-sTCP:LISTEN", "-P", "-n"])
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            })
            .unwrap_or_default();
        for line in text.lines() {
            let port = line.split_whitespace().find_map(|tok| {
                tok.rsplit(':')
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .filter(|_| tok.contains(':'))
            });
            let name = line
                .split("users:((\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(|s| s.to_string())
                .or_else(|| line.split_whitespace().next().map(|s| s.to_string()));
            let pid = line
                .split("pid=")
                .nth(1)
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if let Some(port) = port {
                if !out.iter().any(|p| p.port == port) {
                    out.push(ListeningPort {
                        port,
                        pid,
                        process: name.unwrap_or_default(),
                    });
                }
            }
        }
    }
    out.sort_by_key(|p| p.port);
    out
}

#[cfg(windows)]
fn process_names() -> std::collections::HashMap<u32, String> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut map = std::collections::HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                map.insert(
                    entry.th32ProcessID,
                    name.trim_end_matches(".exe").to_string(),
                );
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
    }
    map
}
