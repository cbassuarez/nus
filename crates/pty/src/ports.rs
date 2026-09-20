//! What's on the wire: every socket on this machine with the process that
//! owns it, the process tree (so a port can be traced to the shell that
//! started it), and per-process facts (command line, executable, start
//! time). Gathered by shelling out — `netstat` and CIM on Windows, `ss`
//! and `ps` elsewhere — so it runs off the main thread.

use std::collections::HashMap;
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Listen,
    Established,
    /// UDP has no state; TCP handshakes and closes land here too.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Socket {
    pub proto: Proto,
    /// The local address as bound: `127.0.0.1`, `0.0.0.0`, `::`, `[::1]`…
    pub local_addr: String,
    pub port: u16,
    pub state: State,
    /// Remote host and port for connections.
    pub remote: Option<(String, u16)>,
    pub pid: u32,
}

impl Socket {
    /// Bound to every interface: reachable from the network.
    pub fn exposed(&self) -> bool {
        matches!(self.local_addr.as_str(), "0.0.0.0" | "::" | "[::]" | "*")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
    pub exe: String,
    pub cmdline: String,
    pub started: Option<SystemTime>,
}

/// Every socket, as the system reports it.
pub fn sockets() -> Vec<Socket> {
    #[cfg(windows)]
    {
        let Ok(o) = std::process::Command::new("netstat")
            .args(["-ano"])
            .output()
        else {
            return Vec::new();
        };
        parse_netstat(&String::from_utf8_lossy(&o.stdout))
    }
    #[cfg(not(windows))]
    {
        let text = std::process::Command::new("ss")
            .args(["-tunap"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
        match text {
            Some(t) => parse_ss(&t),
            None => {
                let o = std::process::Command::new("lsof")
                    .args(["-i", "-P", "-n"])
                    .output()
                    .ok();
                o.map(|o| parse_lsof(&String::from_utf8_lossy(&o.stdout)))
                    .unwrap_or_default()
            }
        }
    }
}

/// `netstat -ano`: `TCP 0.0.0.0:5173 0.0.0.0:0 LISTENING 1234`, and
/// `UDP [::]:5353 *:* 5678`.
pub fn parse_netstat(text: &str) -> Vec<Socket> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let proto = match f.first() {
            Some(&"TCP") => Proto::Tcp,
            Some(&"UDP") => Proto::Udp,
            _ => continue,
        };
        let Some((local_addr, port)) = f.get(1).and_then(|s| split_addr(s)) else {
            continue;
        };
        let (state, pid) = match proto {
            Proto::Tcp => {
                let state = match f.get(3) {
                    Some(&"LISTENING") => State::Listen,
                    Some(&"ESTABLISHED") => State::Established,
                    _ => State::Other,
                };
                (state, f.get(4).and_then(|p| p.parse().ok()).unwrap_or(0))
            }
            Proto::Udp => (
                State::Other,
                f.get(3).and_then(|p| p.parse().ok()).unwrap_or(0),
            ),
        };
        let remote = f
            .get(2)
            .and_then(|s| split_addr(s))
            .filter(|(h, p)| *p != 0 && h != "*" && h != "0.0.0.0");
        out.push(Socket {
            proto,
            local_addr,
            port,
            state,
            remote,
            pid,
        });
    }
    out
}

/// `ss -tunap`: `tcp LISTEN 0 128 0.0.0.0:5173 0.0.0.0:* users:(("node",pid=1234,fd=20))`.
pub fn parse_ss(text: &str) -> Vec<Socket> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        let proto = match f[0] {
            "tcp" => Proto::Tcp,
            "udp" => Proto::Udp,
            _ => continue,
        };
        let state = match f[1] {
            "LISTEN" => State::Listen,
            "ESTAB" => State::Established,
            _ => State::Other,
        };
        let Some((local_addr, port)) = split_addr(f[4]) else {
            continue;
        };
        let remote = split_addr(f[5]).filter(|(h, p)| *p != 0 && h != "*");
        let pid = line
            .split("pid=")
            .nth(1)
            .and_then(|s| s.split([',', ')']).next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        out.push(Socket {
            proto,
            local_addr,
            port,
            state,
            remote,
            pid,
        });
    }
    out
}

/// `lsof -i -P -n`: `node 1234 seb 20u IPv4 … TCP 127.0.0.1:5173 (LISTEN)`.
pub fn parse_lsof(text: &str) -> Vec<Socket> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 9 {
            continue;
        }
        let proto = match f[7] {
            "TCP" => Proto::Tcp,
            "UDP" => Proto::Udp,
            _ => continue,
        };
        let pid = f[1].parse().unwrap_or(0);
        let (local, remote) = match f[8].split_once("->") {
            Some((l, r)) => (l, Some(r)),
            None => (f[8], None),
        };
        let Some((local_addr, port)) = split_addr(local) else {
            continue;
        };
        let state = match f.get(9) {
            Some(&"(LISTEN)") => State::Listen,
            Some(&"(ESTABLISHED)") => State::Established,
            _ => State::Other,
        };
        out.push(Socket {
            proto,
            local_addr,
            port,
            state,
            remote: remote.and_then(split_addr),
            pid,
        });
    }
    out
}

/// `host:port` → (host, port), with IPv6 brackets kept on the host.
fn split_addr(s: &str) -> Option<(String, u16)> {
    let (host, port) = s.rsplit_once(':')?;
    let port = port
        .parse::<u16>()
        .ok()
        .or(if port == "*" { Some(0) } else { None })?;
    Some((host.to_string(), port))
}

/// Every process: pid → (parent, name). Cheap; the tree for tracing ports
/// to shells.
pub fn process_tree() -> HashMap<u32, (u32, String)> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let mut map = HashMap::new();
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
                        (
                            entry.th32ParentProcessID,
                            name.trim_end_matches(".exe").to_string(),
                        ),
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
    #[cfg(not(windows))]
    {
        let o = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .output()
            .ok();
        let mut map = HashMap::new();
        if let Some(o) = o {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let mut f = line.split_whitespace();
                let (Some(pid), Some(ppid)) = (
                    f.next().and_then(|s| s.parse().ok()),
                    f.next().and_then(|s| s.parse().ok()),
                ) else {
                    continue;
                };
                let name = f
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                map.insert(pid, (ppid, name));
            }
        }
        map
    }
}

/// Walk parents from `pid` until one of `roots` is met; that root.
pub fn ancestor_in(tree: &HashMap<u32, (u32, String)>, pid: u32, roots: &[u32]) -> Option<u32> {
    let mut cur = pid;
    for _ in 0..64 {
        if roots.contains(&cur) {
            return Some(cur);
        }
        let (parent, _) = tree.get(&cur)?;
        if *parent == 0 || *parent == cur {
            return None;
        }
        cur = *parent;
    }
    None
}

/// Command line, executable and start time for a set of pids. One CIM
/// call on Windows (about a second — run it off-thread), `ps` elsewhere.
pub fn process_info(pids: &[u32]) -> HashMap<u32, Process> {
    let mut out = HashMap::new();
    if pids.is_empty() {
        return out;
    }
    #[cfg(windows)]
    {
        let filter = pids
            .iter()
            .map(|p| format!("ProcessId={p}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let script = format!(
            "Get-CimInstance Win32_Process -Filter \"{filter}\" | Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine,@{{n='Started';e={{[int64]([DateTimeOffset]$_.CreationDate).ToUnixTimeSeconds()}}}} | ConvertTo-Json -Compress"
        );
        let Ok(o) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
        else {
            return out;
        };
        let text = String::from_utf8_lossy(&o.stdout);
        let v: serde_json::Value = match serde_json::from_str(text.trim()) {
            Ok(v) => v,
            Err(_) => return out,
        };
        let items: Vec<serde_json::Value> = match v {
            serde_json::Value::Array(a) => a,
            serde_json::Value::Object(_) => vec![v],
            _ => Vec::new(),
        };
        for it in items {
            let pid = it.get("ProcessId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let s = |k: &str| it.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            out.insert(
                pid,
                Process {
                    pid,
                    parent: it
                        .get("ParentProcessId")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    name: s("Name").trim_end_matches(".exe").to_string(),
                    exe: s("ExecutablePath"),
                    cmdline: s("CommandLine"),
                    started: it
                        .get("Started")
                        .and_then(|v| v.as_i64())
                        .filter(|t| *t > 0)
                        .map(|t| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(t as u64)),
                },
            );
        }
    }
    #[cfg(not(windows))]
    {
        let list = pids
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let Ok(o) = std::process::Command::new("ps")
            .args(["-o", "pid=,ppid=,lstart=,args=", "-p", &list])
            .output()
        else {
            return out;
        };
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 8 {
                continue;
            }
            let pid: u32 = f[0].parse().unwrap_or(0);
            let parent: u32 = f[1].parse().unwrap_or(0);
            // lstart is five words: "Wed Sep 17 09:00:00 2026".
            let cmdline = f[7..].join(" ");
            let exe = f[7].to_string();
            let name = exe.rsplit('/').next().unwrap_or("").to_string();
            out.insert(
                pid,
                Process {
                    pid,
                    parent,
                    name,
                    exe,
                    cmdline,
                    started: None,
                },
            );
        }
    }
    out
}

/// Ask a process to stop: Ctrl+C-ish first (`taskkill` / SIGTERM), then
/// `force` kills outright. The whole tree goes, not just the named
/// process — a dev server is a shell with a node under it, and killing
/// the shell alone leaves the node holding the port.
pub fn kill(pid: u32, force: bool) -> bool {
    kill_tree(pid, force)
}

/// Every process under `pid`, deepest first, from a tree already read.
/// Deepest first so a signal reaches the leaves before the parent that
/// would otherwise outlive them as orphans.
pub fn descendants_in(tree: &HashMap<u32, (u32, String)>, pid: u32) -> Vec<u32> {
    // Never turn a reserved/root pid into a request for the entire system tree.
    if pid <= 1 { return Vec::new(); }
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&child, &(parent, _)) in tree {
        if child != parent {
            kids.entry(parent).or_default().push(child);
        }
    }
    // Breadth first from the root; reversing it puts the leaves in front.
    let mut order = Vec::new();
    let mut queue = vec![pid];
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    seen.insert(pid);
    while let Some(p) = queue.pop() {
        for &c in kids.get(&p).into_iter().flatten() {
            if seen.insert(c) {
                order.push(c);
                queue.push(c);
            }
        }
    }
    order.reverse();
    order
}

/// Every process under `pid`, deepest first.
pub fn descendants(pid: u32) -> Vec<u32> {
    descendants_in(&process_tree(), pid)
}

/// What a shell is running: the name of its own first child, by pid, from
/// a tree already read. `None` is a shell with nothing under it — a bare
/// prompt, nothing to lose by closing.
pub fn child_name_in(tree: &HashMap<u32, (u32, String)>, pid: u32) -> Option<String> {
    let mut kids: Vec<(u32, &str)> = tree
        .iter()
        .filter(|(&child, &(parent, _))| parent == pid && child != pid)
        .map(|(&child, (_, name))| (child, name.as_str()))
        // conhost is ConPTY's own helper, not anyone's work.
        .filter(|(_, name)| !name.is_empty() && !name.eq_ignore_ascii_case("conhost"))
        .collect();
    kids.sort_unstable();
    kids.first().map(|(_, name)| name.to_string())
}

/// `pid` and everything under it. The tree is read before the first
/// signal goes out, so a child that gets reparented to init on the way
/// down is still on the list.
pub fn kill_tree(pid: u32, force: bool) -> bool {
    kill_trees_in(&process_tree(), &[pid], force)
}

/// Several trees from one reading of the process table — closing a stack
/// of tabs is one `ps`, not one per tab.
pub fn kill_trees(pids: &[u32], force: bool) -> bool {
    if pids.is_empty() {
        return true;
    }
    kill_trees_in(&process_tree(), pids, force)
}

/// Close these trees for good: a word first (SIGTERM / `taskkill`), a
/// breath for anything that cleans up after itself, then the rest go
/// outright. Only waits when the shells had children — an idle prompt
/// closes at once.
pub fn reap_trees_in(tree: &HashMap<u32, (u32, String)>, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    let had_children = pids.iter().any(|&p| !descendants_in(tree, p).is_empty());
    kill_trees_in(tree, pids, false);
    if had_children {
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    kill_trees_in(tree, pids, true);
}

/// The same, reading the table itself.
pub fn reap_trees(pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    reap_trees_in(&process_tree(), pids);
}

/// The signalling itself, over a tree already read.
pub fn kill_trees_in(tree: &HashMap<u32, (u32, String)>, pids: &[u32], force: bool) -> bool {
    let mut targets: Vec<u32> = Vec::new();
    for &pid in pids {
        for d in descendants_in(tree, pid) {
            if !targets.contains(&d) {
                targets.push(d);
            }
        }
    }
    // The roots last: their children first, so nothing is orphaned.
    for &pid in pids {
        if !targets.contains(&pid) {
            targets.push(pid);
        }
    }
    let mut any = false;
    for pid in targets {
        any |= kill_one(pid, force);
    }
    any
}

/// One process, and on Windows whatever Windows counts as its tree.
pub fn kill_one(pid: u32, force: bool) -> bool {
    if pid <= 1 {
        return false;
    }
    #[cfg(windows)]
    {
        let mut args = vec!["/PID".to_string(), pid.to_string(), "/T".to_string()];
        if force {
            args.push("/F".into());
        }
        std::process::Command::new("taskkill")
            .args(&args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args([if force { "-KILL" } else { "-TERM" }, &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Docker's published ports, when docker is on PATH: (host port, container
/// name, image). Empty otherwise.
pub fn docker_ports() -> Vec<(u16, String, String)> {
    let Ok(o) = std::process::Command::new("docker")
        .args(["ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Ports}}"])
        .output()
    else {
        return Vec::new();
    };
    if !o.status.success() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        let mut f = line.split('\t');
        let (Some(name), Some(image), Some(ports)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        // "0.0.0.0:8080->80/tcp, :::8080->80/tcp"
        for p in ports.split(',') {
            let Some((host, _)) = p.trim().split_once("->") else {
                continue;
            };
            if let Some((_, port)) = host.rsplit_once(':') {
                if let Ok(port) = port.parse::<u16>() {
                    if !out.iter().any(|(q, _, _)| *q == port) {
                        out.push((port, name.to_string(), image.to_string()));
                    }
                }
            }
        }
    }
    out
}

/// A quick look at what answers on a port: HTTP status, `<title>`, and
/// a guess at the framework. Sends one `GET /`; bounded by `timeout`.
pub fn probe(port: u16, timeout: std::time::Duration) -> Option<Probe> {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, timeout).ok()?;
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    s.write_all(
        format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\nUser-Agent: nus\r\nAccept: text/html\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let deadline = std::time::Instant::now() + timeout;
    while buf.len() < 64 * 1024 && std::time::Instant::now() < deadline {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    if !text.starts_with("HTTP/") {
        return Some(Probe {
            status: 0,
            title: String::new(),
            server: String::new(),
            framework: "not http".into(),
        });
    }
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let header = |name: &str| -> String {
        head.lines()
            .find(|l| l.to_ascii_lowercase().starts_with(&format!("{name}:")))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };
    let server = header("server");
    let powered = header("x-powered-by");
    let lower = body.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|i| {
            let rest = &body[i..];
            let start = rest.find('>')? + 1;
            let end = rest[start..].find("</title>")? + start;
            Some(rest[start..end].trim().to_string())
        })
        .unwrap_or_default();
    let framework = if lower.contains("/@vite/client") || lower.contains("vite/client") {
        "vite"
    } else if lower.contains("/_next/") || powered.to_lowercase().contains("next") {
        "next.js"
    } else if lower.contains("__nuxt") {
        "nuxt"
    } else if lower.contains("/_app/immutable/") || lower.contains("sveltekit") {
        "sveltekit"
    } else if lower.contains("astro") && lower.contains("astro-island") {
        "astro"
    } else if powered.to_lowercase().contains("express") {
        "express"
    } else if server.to_lowercase().contains("uvicorn") {
        "uvicorn"
    } else if server.to_lowercase().contains("werkzeug") {
        "flask"
    } else if server.to_lowercase().contains("gunicorn") {
        "gunicorn"
    } else if server.to_lowercase().contains("simplehttp") {
        "python http.server"
    } else if lower.contains("webpack") && lower.contains("hot") {
        "webpack dev server"
    } else if lower.contains("storybook") {
        "storybook"
    } else if lower.contains("jupyter") {
        "jupyter"
    } else {
        ""
    };
    Some(Probe {
        status,
        title,
        server,
        framework: framework.to_string(),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    pub status: u16,
    pub title: String,
    pub server: String,
    pub framework: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pid → (ppid, name), the shape `process_tree` returns.
    fn tree(rows: &[(u32, u32, &str)]) -> HashMap<u32, (u32, String)> {
        rows.iter().map(|&(pid, ppid, n)| (pid, (ppid, n.to_string()))).collect()
    }

    #[test]
    fn descendants_come_back_deepest_first() {
        // 10 → 20 → 30, and 10 → 21. The leaves have to be signalled
        // before their parents, or they are orphaned on the way down.
        let t = tree(&[(1, 0, "init"), (10, 1, "sh"), (20, 10, "npm"), (21, 10, "tail"), (30, 20, "node")]);
        let d = descendants_in(&t, 10);
        assert_eq!(d.len(), 3, "{d:?}");
        let at = |pid: u32| d.iter().position(|&p| p == pid).unwrap();
        assert!(at(30) < at(20), "the node has to go before the npm that holds it: {d:?}");
        assert!(d.contains(&21));
        assert!(!d.contains(&10), "the root is not its own descendant");
    }

    #[test]
    fn a_cycle_does_not_hang() {
        let t = tree(&[(10, 11, "a"), (11, 10, "b")]);
        assert_eq!(descendants_in(&t, 10), vec![11]);
    }

    #[test]
    fn a_shell_at_its_prompt_is_running_nothing() {
        let t = tree(&[(1, 0, "init"), (10, 1, "zsh")]);
        assert_eq!(child_name_in(&t, 10), None);
    }

    #[test]
    fn a_shell_running_something_names_it() {
        // Lowest pid wins, so the same shell always reads the same way.
        let t = tree(&[(10, 1, "zsh"), (33, 10, "vim"), (22, 10, "npm"), (44, 22, "node")]);
        assert_eq!(child_name_in(&t, 10).as_deref(), Some("npm"));
    }

    #[test]
    fn conpty_s_own_helper_is_not_your_work() {
        let t = tree(&[(10, 1, "pwsh"), (11, 10, "conhost")]);
        assert_eq!(child_name_in(&t, 10), None);
    }

    #[test]
    fn nothing_signals_init() {
        assert!(!kill_one(1, true));
        assert!(!kill_one(0, false));
        let t=tree(&[(1,0,"init"),(10,1,"sh"),(20,10,"node")]);
        assert!(descendants_in(&t,0).is_empty());
        assert!(descendants_in(&t,1).is_empty());
    }

    #[test]
    fn netstat_lines() {
        let text = "\
  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:5173           0.0.0.0:0              LISTENING       1234
  TCP    127.0.0.1:9229         0.0.0.0:0              LISTENING       99
  TCP    192.168.1.5:52000      142.250.72.14:443      ESTABLISHED     4321
  TCP    [::1]:8765             [::]:0                 LISTENING       77
  UDP    0.0.0.0:5353           *:*                                    555
";
        let v = parse_netstat(text);
        assert_eq!(v.len(), 5);
        assert_eq!(
            v[0],
            Socket {
                proto: Proto::Tcp,
                local_addr: "0.0.0.0".into(),
                port: 5173,
                state: State::Listen,
                remote: None,
                pid: 1234
            }
        );
        assert!(v[0].exposed());
        assert!(!v[1].exposed());
        assert_eq!(v[2].state, State::Established);
        assert_eq!(v[2].remote, Some(("142.250.72.14".into(), 443)));
        assert_eq!(v[3].local_addr, "[::1]");
        assert_eq!(v[4].proto, Proto::Udp);
        assert_eq!(v[4].pid, 555);
    }

    #[test]
    fn ss_lines() {
        let text = "\
Netid State  Recv-Q Send-Q Local Address:Port  Peer Address:Port Process
tcp   LISTEN 0      128    0.0.0.0:5173        0.0.0.0:*         users:((\"node\",pid=1234,fd=20))
tcp   ESTAB  0      0      10.0.0.2:44000      1.1.1.1:443       users:((\"curl\",pid=7,fd=3))
udp   UNCONN 0      0      127.0.0.1:323       0.0.0.0:*
";
        let v = parse_ss(text);
        assert_eq!(v.len(), 3);
        assert_eq!(
            (v[0].port, v[0].pid, v[0].state),
            (5173, 1234, State::Listen)
        );
        assert_eq!(v[1].remote, Some(("1.1.1.1".into(), 443)));
        assert_eq!(v[2].proto, Proto::Udp);
    }

    #[test]
    fn ancestors() {
        let mut t = HashMap::new();
        t.insert(10, (1, "shell".to_string()));
        t.insert(20, (10, "node".to_string()));
        t.insert(30, (20, "esbuild".to_string()));
        assert_eq!(ancestor_in(&t, 30, &[10]), Some(10));
        assert_eq!(ancestor_in(&t, 30, &[99]), None);
    }
}
