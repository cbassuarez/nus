//! The ports board — airport control. What's listening, who owns it (down
//! to the nus shell that started it, since we own the pty tree), and what
//! to do about it. A centred overlay sheet in the departures-board manner:
//! monospace ruled rows that split-flap in and out as ports arrive and
//! depart, a lamp per row, a detail that opens in place with the action
//! strip. EXPAND makes it a full page (a pane); Esc closes. The facts are
//! gathered off-thread on a clock — faster while the board is open — and
//! a new port lights the status icon with a line beside it for six
//! seconds.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use nus_pty::ports::{Probe, Process, Proto, Socket, State};
use nus_render::text::Style;
use nus_render::{Rect, Scene};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, NamedKey};

use crate::app::{fade, App, Pane};
use crate::settings::{KillConfirm, PortsGrouping, PortsOpen, Tunnel};
use nus_render::theme::metric as m;

/// One poll's worth of facts.
pub struct Snapshot {
    pub sockets: Vec<Socket>,
    pub tree: HashMap<u32, (u32, String)>,
    pub info: HashMap<u32, Process>,
    pub docker: Vec<(u16, String, String)>,
    pub at: Instant,
}

/// What the worker is told.
#[derive(Clone)]
pub struct Wants {
    pub interval: Duration,
    pub docker: bool,
    pub probe: bool,
    /// Ports to probe now (new ones); cleared by the worker.
    pub probe_ports: Vec<u16>,
}

pub enum Update {
    Snapshot(Snapshot),
    Probe(u16, Option<Probe>),
}

/// Rows are keyed by what they are, not where they sit.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Port { proto: Proto, port: u16, pid: u32 },
    Conn { pid: u32 },
    Docker { port: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Mine,
    Others,
    System,
    Connections,
    Docker,
    /// Ports that remember: a dev server one of your shells started, gone
    /// now — start again brings it back where it ran.
    Remembered,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Mine => "NUS TERMINALS",
            Group::Others => "OTHER PROCESSES",
            Group::System => "RESERVED / FILTERED",
            Group::Connections => "CONNECTIONS",
            Group::Docker => "DOCKER",
            Group::Remembered => "REMEMBERED",
        }
    }
}

/// A departed port with a known command and folder, kept in ports.json.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Remembered {
    pub port: u16,
    pub process: String,
    pub command: String,
    pub cwd: String,
    pub last_seen: u64,
}

impl Remembered {
    /// A row for the board: pid 0 marks it as a memory, not a socket.
    pub fn row(&self) -> Row {
        Row {
            key: Key::Port { proto: Proto::Tcp, port: self.port, pid: 0 },
            group: Group::Remembered,
            proto: Proto::Tcp,
            port: self.port,
            pid: 0,
            process: self.process.clone(),
            exe: String::new(),
            cmdline: self.command.clone(),
            bound: String::new(),
            exposed: false,
            started: None,
            identity: None,
            tab: None,
            cwd: Some(self.cwd.clone()),
            command: Some(self.command.clone()),
            probe: None,
            conns: 0,
            remotes: Vec::new(),
            container: None,
            name: None,
            rule: Rule::default(),
            seen: crate::clock::now(),
            dying: None,
            tunnel: None,
            watch: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lamp {
    Up,
    Exposed,
    Dying,
    Gone,
}

/// A `ports` rule's answer for one row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rule {
    pub name: Option<String>,
    pub tint: Option<nus_render::Color>,
    pub open: Option<String>,
    pub tunnel: bool,
    pub hide: bool,
    pub watch: bool,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub key: Key,
    pub group: Group,
    pub proto: Proto,
    pub port: u16,
    pub pid: u32,
    pub process: String,
    pub exe: String,
    pub cmdline: String,
    pub bound: String,
    pub exposed: bool,
    pub started: Option<SystemTime>,
    pub identity: Option<u64>,
    /// The nus tab whose shell is an ancestor, and that shell's cwd and
    /// last command.
    pub tab: Option<u64>,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub probe: Option<Probe>,
    /// CONNECTIONS: how many, and the remote hosts, most common first.
    pub conns: usize,
    pub remotes: Vec<String>,
    /// DOCKER: container and image.
    pub container: Option<(String, String)>,
    pub name: Option<String>,
    pub rule: Rule,
    pub seen: Instant,
    pub dying: Option<Instant>,
    pub tunnel: Option<TunnelState>,
    pub watch: bool,
}

#[derive(Clone, Debug)]
pub struct TunnelState {
    pub tab: u64,
    pub url: Option<String>,
}

impl Row {
    pub fn status(&self)->&'static str {
        if self.group==Group::Remembered {"No current listener observed"}
        else if self.dying.is_some(){"Stop requested"}
        else if matches!(self.key, Key::Conn { .. }) {"Established connections observed"}
        else if matches!(self.key, Key::Docker { .. }) {"Docker port mapping observed"}
        else if self.proto==Proto::Udp {"UDP socket bound"}
        else {"TCP listener observed"}
    }
    pub fn lamp(&self) -> Lamp {
        if self.group == Group::Remembered {
            Lamp::Gone
        } else if self.dying.is_some() {
            Lamp::Dying
        } else if self.exposed {
            Lamp::Exposed
        } else {
            Lamp::Up
        }
    }

    pub fn title(&self) -> String {
        if let Some(n) = self.rule.name.as_ref().or(self.name.as_ref()) {
            return n.clone();
        }
        match &self.key {
            Key::Conn { .. } => self.process.clone(),
            Key::Docker { .. } => self.container.as_ref().map(|c| c.0.clone()).unwrap_or_default(),
            Key::Port { .. } => {
                if let Some(p) = &self.probe {
                    if !p.framework.is_empty() {
                        return p.framework.clone();
                    }
                    if !p.title.is_empty() {
                        return p.title.clone();
                    }
                }
                if self.process.is_empty() {
                    "Unknown process".into()
                } else {
                    self.process.clone()
                }
            }
        }
    }

    pub fn url(&self) -> String {
        format!("http://localhost:{}/", self.port)
    }

    pub fn uptime(&self) -> String {
        let Some(s) = self.started else { return String::new() };
        let Ok(d) = SystemTime::now().duration_since(s) else { return String::new() };
        let secs = d.as_secs();
        if secs < 60 {
            format!("{secs}s")
        } else if secs < 3600 {
            format!("{}m", secs / 60)
        } else if secs < 86400 {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        } else {
            format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
        }
    }
}

/// A row on its way out: drawn flapping up, then dropped.
pub struct Departed {
    pub row: Row,
    pub at: Instant,
}

/// What the board has under the pointer or the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Row(Key),
    Name(Key),
    Action(Key, Act),
    Expand,
    Close,
    Grouping,
    Head(Group),
    Confirm(Key, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Open,
    Copy,
    Jump,
    Kill,
    Again,
    Tunnel,
    Watch,
    Name,
}

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::Open => "OPEN IN BROWSER",
            Act::Copy => "COPY LOCAL URL",
            Act::Jump => "SHOW TERMINAL",
            Act::Kill => "STOP PROCESS",
            Act::Again => "RUN SAVED COMMAND",
            Act::Tunnel => "CREATE PUBLIC LINK",
            Act::Watch => "NOTIFY ON CHANGE",
            Act::Name => "RENAME",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Act::Open => "O",
            Act::Copy => "C",
            Act::Jump => "J",
            Act::Kill => "K",
            Act::Again => "R",
            Act::Tunnel => "T",
            Act::Watch => "W",
            Act::Name => "N",
        }
    }
}

pub struct Board {
    pub flaps: HashMap<Key, crate::split_flap::RowChange>,
    pub open: bool,
    pub rows: Vec<Row>,
    pub departed: Vec<Departed>,
    pub sel: Option<Key>,
    pub expanded: Option<Key>,
    pub rename: Option<(Key, String)>,
    pub confirm: Option<Key>,
    pub filter: Option<String>,
    pub scroll: f32,
    pub reach: f32,
    pub viewport: Rect,
    pub reveal: bool,
    pub hits: Vec<(Rect, Hit)>,
    pub rect: Rect,
    /// The worker.
    pub rx: Receiver<Update>,
    pub wants: Arc<Mutex<Wants>>,
    pub last: Option<Instant>,
    pub polls: u32,
    /// Persisted names, by `process:port`.
    pub names: HashMap<String, String>,
    pub watched: HashSet<String>,
    /// Ports that remember, and their rows for the board (ports not live now).
    pub remembered: Vec<Remembered>,
    pub ghosts: Vec<Row>,
    /// A new port's line beside the status icon: text, when, which.
    pub toast: Option<(Instant, Key)>,
    pub rise: crate::anim::Anim,
    pub probed: HashSet<u16>,
    /// Kill: pids with a graceful ask out, and when.
    pub killing: HashMap<u32, (Instant, u64)>,
}

impl Board {
    pub fn new() -> Board {
        let wants = Arc::new(Mutex::new(Wants { interval: Duration::from_secs(10), docker: true, probe: true, probe_ports: Vec::new() }));
        let (tx, rx) = channel();
        if !(std::env::var_os("NUS_SHOT").is_some() && std::env::var_os("NUS_PORTS_FIXTURE").is_some()) {spawn_worker(tx, wants.clone());}
        let (names, watched) = load_names();
        let remembered = load_remembered();
        let ghosts = remembered.iter().map(Remembered::row).collect();
        Board {
            flaps: HashMap::new(),
            open: false,
            rows: Vec::new(),
            departed: Vec::new(),
            sel: None,
            expanded: None,
            rename: None,
            confirm: None,
            filter: None,
            scroll: 0.0,
            reach: 0.0,
            viewport: Rect::new(0.0,0.0,0.0,0.0),
            reveal: false,
            hits: Vec::new(),
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            rx,
            wants,
            last: None,
            polls: 0,
            names,
            watched,
            remembered,
            ghosts,
            toast: None,
            rise: crate::anim::Anim::at(0.0),
            probed: HashSet::new(),
            killing: HashMap::new(),
        }
    }

    pub fn row(&self, key: &Key) -> Option<&Row> {
        self.rows.iter().find(|r| &r.key == key).or_else(|| self.ghosts.iter().find(|r| &r.key == key))
    }

    /// The memories as rows, for ports that are not listening right now.
    pub fn refresh_ghosts(&mut self) {
        let live: HashSet<u16> = self.rows.iter().map(|r| r.port).collect();
        self.ghosts = self.remembered.iter().filter(|m| !live.contains(&m.port)).map(Remembered::row).collect();
    }

    /// A departed row worth remembering: one of your shells started it.
    pub fn remember(&mut self, r: &Row) {
        let (Some(cmd), Some(cwd)) = (r.command.clone(), r.cwd.clone()) else { return };
        if cmd.trim().is_empty() || r.port == 0 {
            return;
        }
        let now = crate::journal::now();
        self.remembered.retain(|m| m.port != r.port);
        self.remembered.insert(0, Remembered { port: r.port, process: r.process.clone(), command: crate::cutoff::oneline(&cmd), cwd, last_seen: now });
        self.remembered.truncate(24);
    }

    pub fn row_mut(&mut self, key: &Key) -> Option<&mut Row> {
        self.rows.iter_mut().find(|r| &r.key == key)
    }

    /// The rows in display order with their group heads, filtered.
    pub fn listing(&self, grouping: PortsGrouping) -> Vec<Entry> {
        listing(&self.rows, &self.ghosts, self.filter.as_deref(), grouping)
    }
}

fn listing(live: &[Row], ghosts: &[Row], filter: Option<&str>, grouping: PortsGrouping) -> Vec<Entry> {
        let q = filter.unwrap_or("").to_lowercase();
        let mut rows: Vec<&Row> = live.iter()
            .chain(ghosts.iter())
            .filter(|r| !r.rule.hide)
            .filter(|r| {
                q.is_empty()
                    || r.port.to_string().contains(&q)
                    || r.process.to_lowercase().contains(&q)
                    || r.title().to_lowercase().contains(&q)
                    || r.cmdline.to_lowercase().contains(&q)
            })
            .collect();
        let mut out = Vec::new();
        match grouping {
            PortsGrouping::Origin => {
                for g in [Group::Mine, Group::Others, Group::System, Group::Connections, Group::Docker, Group::Remembered] {
                    let mut in_g: Vec<&Row> = rows.iter().copied().filter(|r| r.group == g).collect();
                    if in_g.is_empty() {
                        continue;
                    }
                    in_g.sort_by_key(|r| (r.port, r.pid));
                    out.push(Entry::Head(g, in_g.len()));
                    out.extend(in_g.into_iter().map(|r| Entry::Row(r.key.clone())));
                }

            }
            PortsGrouping::Port => {
                rows.sort_by_key(|r| (r.port, r.pid));
                out.extend(rows.into_iter().map(|r| Entry::Row(r.key.clone())));
            }
            PortsGrouping::Process => {
                rows.sort_by(|a, b| a.process.to_lowercase().cmp(&b.process.to_lowercase()).then(a.process.cmp(&b.process)).then(a.port.cmp(&b.port)));
                let mut last: Option<&str> = None;
                for r in &rows {
                    if last != Some(r.process.as_str()) {
                        last = Some(r.process.as_str());
                        let n = rows.iter().filter(|x| x.process == r.process).count();
                        out.push(Entry::Process(r.process.clone(), n));
                    }
                    out.push(Entry::Row(r.key.clone()));
                }
            }
        }
        out
}

#[derive(Clone, Debug)]
pub enum Entry {
    Head(Group, usize),
    Process(String, usize),
    Row(Key),
}

fn names_path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("ports.json")
}

fn load_names() -> (HashMap<String, String>, HashSet<String>) {
    let Ok(text) = std::fs::read_to_string(names_path()) else { return Default::default() };
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let names = v
        .get("names")
        .and_then(|n| n.as_object())
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default();
    let watched = v.get("watched").and_then(|w| w.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
    (names, watched)
}

fn load_remembered() -> Vec<Remembered> {
    let Ok(text) = std::fs::read_to_string(names_path()) else { return Vec::new() };
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    v.get("remembered").and_then(|r| serde_json::from_value(r.clone()).ok()).unwrap_or_default()
}

fn save_names(names: &HashMap<String, String>, watched: &HashSet<String>) {
    // Keep what else the file holds.
    let remembered = load_remembered();
    save_file(names, watched, &remembered);
}

fn save_file(names: &HashMap<String, String>, watched: &HashSet<String>, remembered: &[Remembered]) {
    let v = serde_json::json!({ "names": names, "watched": watched.iter().collect::<Vec<_>>(), "remembered": remembered });
    let _ = std::fs::create_dir_all(names_path().parent().unwrap());
    let _ = std::fs::write(names_path(), serde_json::to_string_pretty(&v).unwrap_or_default());
}

/// The worker: sockets + tree every interval, process facts for the pids
/// that changed, docker when wanted, probes when asked.
fn spawn_worker(tx: Sender<Update>, wants: Arc<Mutex<Wants>>) {
    std::thread::Builder::new()
        .name("ports".into())
        .spawn(move || {
            let mut known: HashMap<u32, Process> = HashMap::new();
            let mut last_docker = crate::clock::now() - Duration::from_secs(60);
            let mut docker: Vec<(u16, String, String)> = Vec::new();
            loop {
                let w = wants.lock().map(|w| w.clone()).unwrap_or(Wants { interval: Duration::from_secs(10), docker: false, probe: false, probe_ports: Vec::new() });
                if !w.probe_ports.is_empty() {
                    if let Ok(mut g) = wants.lock() {
                        g.probe_ports.clear();
                    }
                    for port in w.probe_ports {
                        let p = nus_pty::ports::probe(port, Duration::from_millis(400));
                        if tx.send(Update::Probe(port, p)).is_err() {
                            return;
                        }
                    }
                }
                let sockets = nus_pty::ports::sockets();
                let tree = nus_pty::ports::process_tree();
                // Refresh process birth identities too: a PID can be reused between polls.
                let pids: Vec<u32> = sockets
                    .iter()
                    .map(|s| s.pid)
                    .filter(|p| *p != 0)
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                known.clear();
                if !pids.is_empty() {
                    for (pid, info) in nus_pty::ports::process_info(&pids) {
                        known.insert(pid, info);
                    }
                    // Pids we asked about and got nothing for: remember as empty, so we don't ask every poll.
                    for p in &pids {
                        known.entry(*p).or_insert_with(|| Process { pid: *p, ..Default::default() });
                    }
                }
                // Forget pids that are gone, so a reused pid gets fresh facts.
                known.retain(|pid, _| tree.contains_key(pid));
                if w.docker && crate::clock::since(last_docker) >= Duration::from_secs(15) {
                    last_docker = crate::clock::now();
                    docker = nus_pty::ports::docker_ports();
                }
                let snap = Snapshot {
                    sockets,
                    tree,
                    info: known.clone(),
                    docker: if w.docker { docker.clone() } else { Vec::new() },
                    at: crate::clock::now(),
                };
                if tx.send(Update::Snapshot(snap)).is_err() {
                    return;
                }
                // Sleep in small steps so a shorter interval takes hold soon.
                let mut slept = Duration::ZERO;
                while slept < w.interval {
                    std::thread::sleep(Duration::from_millis(250));
                    slept += Duration::from_millis(250);
                    let now = wants.lock().map(|w| w.interval).unwrap_or(w.interval);
                    if now < w.interval && slept >= now {
                        break;
                    }
                    if wants.lock().map(|w| !w.probe_ports.is_empty()).unwrap_or(false) {
                        break;
                    }
                }
            }
        })
        .expect("ports worker");
}

impl App {
    /// Once a loop: take the worker's news, tend the flaps, the toast, the kills.
    pub(crate) fn ports_tick(&mut self) {
        // What the worker should be doing.
        let interval = if self.board.open || self.ports_page_open() { Duration::from_secs(self.behavior.ports_poll.max(1) as u64) } else { Duration::from_secs(10) };
        if let Ok(mut w) = self.board.wants.lock() {
            w.interval = interval;
            w.docker = self.behavior.ports_show_docker;
            w.probe = self.behavior.ports_probe;
        }
        let mut updates = Vec::new();
        while let Ok(u) = self.board.rx.try_recv() {
            updates.push(u);
        }
        for u in updates {
            match u {
                Update::Snapshot(s) => self.ports_merge(s),
                Update::Probe(port, p) => {
                    for r in self.board.rows.iter_mut().filter(|r| r.port == port && matches!(r.key, Key::Port { .. })) {
                        r.probe = p.clone();
                    }
                    self.dirty = true;
                }
            }
        }
        // Each row runs independently; its flaps follow their preceding neighbor.
        if (self.board.open || self.ports_page_open()) && !self.motion.reduced()
            && self.board.flaps.values().any(|c|c.before!=c.after && crate::clock::now()<c.at+Duration::from_secs_f32(crate::split_flap::duration(c.cells))) {self.dirty=true;}
        // Flaps and the toast expire.
        let before = self.board.departed.len();
        self.board.departed.retain(|d| crate::clock::since(d.at).as_millis() < 450);
        if self.board.departed.len() != before || (!self.board.departed.is_empty() && (self.board.open || self.ports_page_open())) {
            self.dirty = true;
        }
        if self.board.rows.iter().any(|r| crate::clock::since(r.seen).as_millis() < 450) && (self.board.open || self.ports_page_open()) {
            self.dirty = true;
        }
        if self.board.toast.as_ref().is_some_and(|(at, _)| crate::clock::since(at).as_secs_f32() > 6.0) {
            self.board.toast = None;
            self.dirty = true;
        }
        if self.board.rise.active() {
            self.dirty = true;
        }
        // Kills: force after three seconds if still there.
        let due: Vec<u32> = self.board.killing.iter().filter(|(_, (at, _))| crate::clock::since(at).as_secs() >= 3).map(|(p, _)| *p).collect();
        for pid in due {
            if let Some((_, identity)) = self.board.killing.remove(&pid) {
                nus_pty::ports::kill_identified(pid, identity, true);
            }
        }
        // Tunnels: the public URL shows up in the tunnel tab's grid.
        let mut found: Vec<(Key, String)> = Vec::new();
        for r in &self.board.rows {
            let Some(t) = &r.tunnel else { continue };
            if t.url.is_some() {
                continue;
            }
            if let Some(tab) = self.tabs.iter().find(|x| x.id == t.tab) {
                for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                    if let Pane::Term(term) = p {
                        let text = term.term.grid().text();
                        if let Some(url) = text.split_whitespace().find(|w| w.starts_with("https://") && (w.contains("trycloudflare.com") || w.contains("ngrok"))) {
                            found.push((r.key.clone(), url.trim_end_matches(['.', ',']).to_string()));
                        }
                    }
                }
            }
        }
        for (k, url) in found {
            if let Some(r) = self.board.row_mut(&k) {
                if let Some(t) = r.tunnel.as_mut() {
                    t.url = Some(url);
                }
            }
            self.dirty = true;
        }
    }

    fn ports_page_open(&self) -> bool {
        self.tabs.get(self.active).is_some_and(|t| matches!(t.left, Pane::Ports(_)) || matches!(t.right, Some(Pane::Ports(_))))
    }

    /// Fold a snapshot into the rows: arrivals flap in, departures out,
    /// facts refresh, the toast fires.
    fn ports_merge(&mut self, s: Snapshot) {
        let shells: Vec<(u64, u32, Option<String>, Option<String>)> = self
            .tabs
            .iter()
            .flat_map(|t| {
                std::iter::once(&t.left).chain(t.right.as_ref()).filter_map(move |p| match p {
                    Pane::Term(term) => term.pty.pid().map(|pid| {
                        let cmd = term.term.marks.iter().rev().find(|m| m.kind == nus_vt::MarkKind::CommandStart).map(|m| term.term.command_text(m)).filter(|c| !c.trim().is_empty());
                        (t.id, pid, term.term.cwd.clone(), cmd)
                    }),
                    _ => None,
                })
            })
            .collect();
        let shell_pids: Vec<u32> = shells.iter().map(|s| s.1).collect();
        let hidden: Vec<String> = self.behavior.ports_hidden.iter().map(|h| h.to_lowercase()).collect();
        let show_system = self.behavior.ports_show_system;
        let show_udp = self.behavior.ports_show_udp;
        let show_conn = self.behavior.ports_show_connections;
        let me = std::process::id();

        let mut fresh: Vec<Row> = Vec::new();
        // Listening sockets, one row per (proto, port, pid).
        for sock in s.sockets.iter().filter(|k| k.state == State::Listen || (k.proto == Proto::Udp && show_udp)) {
            if sock.proto == Proto::Udp && !show_udp {
                continue;
            }
            let key = Key::Port { proto: sock.proto, port: sock.port, pid: sock.pid };
            if fresh.iter().any(|r| r.key == key) {
                continue;
            }
            let info = s.info.get(&sock.pid);
            let name = info.map(|i| i.name.clone()).filter(|n| !n.is_empty()).or_else(|| s.tree.get(&sock.pid).map(|t| t.1.clone())).unwrap_or_default();
            let mine = nus_pty::ports::ancestor_in(&s.tree, sock.pid, &shell_pids);
            let system = sock.pid == 0 || sock.pid == 4 || sock.port < 1024 || hidden.contains(&name.to_lowercase()) || sock.pid == me || name.eq_ignore_ascii_case("nus-hold");
            if system && !show_system {
                continue;
            }
            let group = if mine.is_some() { Group::Mine } else if system { Group::System } else { Group::Others };
            let shell = mine.and_then(|pid| shells.iter().find(|s| s.1 == pid));
            fresh.push(Row {
                key,
                group,
                proto: sock.proto,
                port: sock.port,
                pid: sock.pid,
                process: name,
                exe: info.map(|i| i.exe.clone()).unwrap_or_default(),
                cmdline: info.map(|i| i.cmdline.clone()).unwrap_or_default(),
                bound: sock.local_addr.clone(),
                exposed: sock.exposed(),
                started: info.and_then(|i| i.started),
                identity: info.and_then(|i| i.identity),
                tab: shell.map(|s| s.0),
                cwd: shell.and_then(|s| s.2.clone()),
                command: shell.and_then(|s| s.3.clone()),
                probe: None,
                conns: 0,
                remotes: Vec::new(),
                container: None,
                name: None,
                rule: Rule::default(),
                seen: crate::clock::now(),
                dying: None,
                tunnel: None,
                watch: false,
            });
        }
        // Established connections, one row per process.
        if show_conn {
            let mut by_pid: HashMap<u32, Vec<&Socket>> = HashMap::new();
            for sock in s.sockets.iter().filter(|k| k.state == State::Established && k.remote.is_some()) {
                by_pid.entry(sock.pid).or_default().push(sock);
            }
            for (pid, socks) in by_pid {
                let info = s.info.get(&pid);
                let name = info.map(|i| i.name.clone()).filter(|n| !n.is_empty()).or_else(|| s.tree.get(&pid).map(|t| t.1.clone())).unwrap_or_default();
                if pid == 0 || pid == 4 || hidden.contains(&name.to_lowercase()) || name.eq_ignore_ascii_case("nus-hold") {
                    continue;
                }
                let mut counts: HashMap<String, usize> = HashMap::new();
                for k in &socks {
                    if let Some((h, _)) = &k.remote {
                        if h == "127.0.0.1" || h == "[::1]" || h == "::1" {
                            continue;
                        }
                        *counts.entry(h.clone()).or_default() += 1;
                    }
                }
                if counts.is_empty() {
                    continue;
                }
                let mut remotes: Vec<(String, usize)> = counts.into_iter().collect();
                remotes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                let mine = nus_pty::ports::ancestor_in(&s.tree, pid, &shell_pids);
                let shell = mine.and_then(|p| shells.iter().find(|s| s.1 == p));
                fresh.push(Row {
                    key: Key::Conn { pid },
                    group: Group::Connections,
                    proto: Proto::Tcp,
                    port: 0,
                    pid,
                    process: name,
                    exe: info.map(|i| i.exe.clone()).unwrap_or_default(),
                    cmdline: info.map(|i| i.cmdline.clone()).unwrap_or_default(),
                    bound: String::new(),
                    exposed: false,
                    started: info.and_then(|i| i.started),
                identity: info.and_then(|i| i.identity),
                    tab: shell.map(|s| s.0),
                    cwd: shell.and_then(|s| s.2.clone()),
                    command: shell.and_then(|s| s.3.clone()),
                    probe: None,
                    conns: remotes.iter().map(|r| r.1).sum(),
                    remotes: remotes.into_iter().take(6).map(|r| r.0).collect(),
                    container: None,
                    name: None,
                    rule: Rule::default(),
                    seen: crate::clock::now(),
                    dying: None,
                    tunnel: None,
                    watch: false,
                });
            }
        }
        // Docker's published ports.
        for (port, container, image) in &s.docker {
            fresh.push(Row {
                key: Key::Docker { port: *port },
                group: Group::Docker,
                proto: Proto::Tcp,
                port: *port,
                pid: 0,
                process: "docker".into(),
                exe: String::new(),
                cmdline: format!("{container} · {image}"),
                bound: "0.0.0.0".into(),
                exposed: false,
                started: None,
            identity: None,
                tab: None,
                cwd: None,
                command: None,
                probe: None,
                conns: 0,
                remotes: Vec::new(),
                container: Some((container.clone(), image.clone())),
                name: None,
                rule: Rule::default(),
                seen: crate::clock::now(),
                dying: None,
                tunnel: None,
                watch: false,
            });
        }

        // Merge: keep what was there (its seen time, probe, tunnel), add
        // what's new, flap out what's gone.
        let mut merged: Vec<Row> = Vec::new();
        let mut arrived: Vec<Key> = Vec::new();
        for mut f in fresh {
            let name_key = format!("{}:{}", f.process.to_lowercase(), f.port);
            f.name = self.board.names.get(&name_key).cloned();
            f.watch = self.board.watched.contains(&name_key);
            f.rule = self.rules.ports(&f);
            if f.rule.watch {
                f.watch = true;
            }
            if let Some(old) = self.board.rows.iter().find(|r| r.key == f.key) {
                f.seen = old.seen;
                f.probe = old.probe.clone();
                f.dying = old.dying;
                f.tunnel = old.tunnel.clone();
                if f.dying.is_some() && !self.board.killing.contains_key(&f.pid) {
                    // Asked to die but still here past the force: leave the lamp.
                }
            } else if self.board.polls > 0 {
                arrived.push(f.key.clone());
            }
            merged.push(f);
        }
        let gone: Vec<Row> = self.board.rows.iter().filter(|r| !merged.iter().any(|m| m.key == r.key)).cloned().collect();
        let remember_on = self.behavior.ports_remember;
        for r in gone {
            if remember_on && r.group == Group::Mine {
                self.board.remember(&r);
                save_file(&self.board.names, &self.board.watched, &self.board.remembered);
            }
            self.board.killing.remove(&r.pid);
            if r.watch && self.board.polls > 0 {
                self.ports_toast(nus_render::text::icons::PORTS, format!("Port {} Closed", r.port), r.title(), None);
            }
            if self.board.expanded.as_ref() == Some(&r.key) {
                self.board.expanded = None;
            }
            self.board.departed.push(Departed { row: r, at: crate::clock::now() });
        }
        self.board.rows = merged;
        self.board.refresh_ghosts();
        self.board.polls += 1;
        self.board.last = Some(s.at);
        // Arrivals: a probe, the toast, the rule's auto-open and tunnel.
        let mut probe_now = Vec::new();
        let mut auto_open: Vec<(Key, String)> = Vec::new();
        let mut auto_tunnel: Vec<Key> = Vec::new();
        for k in &arrived {
            let Some(r) = self.board.row(k).cloned() else { continue };
            if matches!(r.key, Key::Port { proto: Proto::Tcp, .. }) && self.behavior.ports_probe && !self.board.probed.contains(&r.port) && r.pid != me {
                probe_now.push(r.port);
            }
            if matches!(r.group, Group::Mine | Group::Others) && matches!(r.key, Key::Port { proto: Proto::Tcp, .. }) {
                if self.behavior.ports_toast && !r.rule.hide {
                    self.ports_arrived(&r);
                }
                if let Some(how) = r.rule.open.clone() {
                    auto_open.push((k.clone(), how));
                }
                if r.rule.tunnel {
                    auto_tunnel.push(k.clone());
                }
            }
        }
        if !probe_now.is_empty() {
            for p in &probe_now {
                self.board.probed.insert(*p);
            }
            if let Ok(mut w) = self.board.wants.lock() {
                w.probe_ports.extend(probe_now);
            }
        }
        for (k, how) in auto_open {
            let open = match how.as_str() {
                "tab" => PortsOpen::Tab,
                "peek" => PortsOpen::Peek,
                _ => PortsOpen::Split,
            };
            self.ports_open_row(&k, open);
        }
        for k in auto_tunnel {
            self.ports_act(&k, Act::Tunnel);
        }
        // Probes for ports we already had when the board first woke.
        if self.board.polls == 1 && self.behavior.ports_probe {
            let first: Vec<u16> = self.board.rows.iter().filter(|r| matches!(r.key, Key::Port { proto: Proto::Tcp, .. }) && r.group != Group::System && r.pid != me).map(|r| r.port).collect();
            for p in &first {
                self.board.probed.insert(*p);
            }
            if let Ok(mut w) = self.board.wants.lock() {
                w.probe_ports.extend(first);
            }
        }
        // The sidebar's PORTS folder and the crumb count follow the board.
        self.ports = self
            .board
            .rows
            .iter()
            .filter(|r| matches!(r.key, Key::Port { proto: Proto::Tcp, .. }) && r.group != Group::System)
            .map(|r| nus_pty::ListeningPort { port: r.port, pid: r.pid, process: r.process.clone() })
            .collect();
        self.dirty = true;
    }

    /// A port's news, on the slip at the foot of the content.
    fn ports_toast(&mut self, icon: crate::toast::Icon, text: impl Into<String>, tail: impl Into<String>, act: Option<crate::toast::Act>) {
        self.play_event("toggle");
        self.toast(icon, text, tail, act);
    }

    /// A new port: the slip offers to open it, and while it shows the
    /// ports icon glows and O opens it too.
    fn ports_arrived(&mut self, r: &Row) {
        self.board.toast = Some((crate::clock::now(), r.key.clone()));
        self.ports_toast(nus_render::text::icons::PORTS, format!("New Port {}", r.port), r.title(), Some(crate::toast::Act::OpenPort(r.key.clone())));
    }

    pub(crate) fn open_board(&mut self) {
        if self.board.open {
            return;
        }
        self.board.open = true;
        self.board.filter = None;
        self.board.confirm = None;
        self.board.rename = None;
        let d = self.motion.dur(crate::anim::base::PALETTE);
        self.board.rise.replay(0.0, 1.0, d);
        if self.board.sel.is_none() {
            let first = self.board.listing(self.behavior.ports_grouping).into_iter().find_map(|e| match e {
                Entry::Row(k) => Some(k),
                _ => None,
            });
            self.board.sel = first;
        }
        // Wake the worker now rather than on its clock.
        if let Ok(mut w) = self.board.wants.lock() {
            w.interval = Duration::from_millis(100);
        }
        self.dirty = true;
    }

    pub(crate) fn close_board(&mut self) {
        self.board.open = false;
        self.board.rename = None;
        self.board.confirm = None;
        self.board.filter = None;
        self.dirty = true;
    }

    /// The board as a page: a tab with a Ports pane.
    pub(crate) fn expand_board(&mut self) {
        self.board.open = false;
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Ports(_))) {
            self.activate(i);
            return;
        }
        let tab = self.make_tab(Pane::Ports(PortsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0) }), None);
        self.tabs.push(tab);
        let n = self.tabs.len() - 1;
        self.activate(n);
        self.dirty = true;
    }

    /// Open a row's URL where the setting says.
    fn ports_open_row(&mut self, key: &Key, how: PortsOpen) {
        let Some(r) = self.board.row(key) else { return };
        if r.port == 0 {
            return;
        }
        let url = r.url();
        match how {
            PortsOpen::Tab => self.open_url(&url, true),
            PortsOpen::Split => {
                // Beside the shell that owns it when we know it.
                if let Some(tab) = r.tab.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
                    self.activate(tab);
                }
                self.open_url(&url, false);
            }
            PortsOpen::Peek => {
                let src = self.active;
                self.open_peek(src, &url);
            }
        }
    }

    /// One of the strip's actions on a row.
    pub(crate) fn ports_act(&mut self, key: &Key, act: Act) {
        let Some(r) = self.board.row(key).cloned() else { return };
        match act {
            Act::Open => {
                let how = self.behavior.ports_open;
                self.ports_open_row(key, how);
                self.close_board();
            }
            Act::Copy => {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(r.url());
                }
                self.ports_toast(nus_render::text::icons::COPY, "Copied", r.url(), None);
            }
            Act::Jump => {
                if let Some(i) = r.tab.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
                    self.close_board();
                    self.activate(i);
                }
            }
            Act::Kill => {
                let needs_ask = match self.behavior.ports_kill_confirm {
                    KillConfirm::Always => true,
                    KillConfirm::Never => false,
                    KillConfirm::System => r.group != Group::Mine,
                };
                if needs_ask && self.board.confirm.as_ref() != Some(key) {
                    // The question is drawn in the open row; the hover ×
                    // asks from a closed one, so open it or nothing shows.
                    self.board.confirm = Some(key.clone());
                    self.board.expanded = Some(key.clone());
                    self.dirty = true;
                    return;
                }
                self.board.confirm = None;
                self.ports_kill(key);
            }
            Act::Again => {
                // Never type into an existing shell: it may now be running a
                // different foreground program, or the owner may be a split pane.
                if let Some(cmd) = r.command.clone() {
                    let profile = self.behavior.default_profile;
                    match self.new_term_pane_at(false, profile, r.cwd.clone()) {
                        Ok(mut t) => {
                            t.type_at_prompt = Some(format!("{cmd}\r"));
                            t.type_origin = Some(crate::finish_work::Origin::NusAction);
                            let tab = self.make_tab(Pane::Term(t), None);
                            self.tabs.push(tab);
                            self.close_board();
                            self.activate(self.tabs.len() - 1);
                        }
                        Err(_) => self.toast_problem("Could Not Open Terminal", format!("for port {}", r.port), None),
                    }
                }
            }

            Act::Tunnel => {
                if r.port == 0 || r.tunnel.is_some() {
                    return;
                }
                let cmd = match self.behavior.ports_tunnel {
                    Tunnel::Cloudflared => format!("cloudflared tunnel --url http://localhost:{}", r.port),
                    Tunnel::Ngrok => format!("ngrok http {}", r.port),
                };
                let owner = r.tab.and_then(|id| self.tabs.iter().position(|t| t.id == id))
                    .filter(|&i| self.tabs[i].right.is_none());
                let profile = self.behavior.default_profile;
                let mut t = match self.new_term_pane_at(owner.is_some(), profile, r.cwd.clone()) {
                    Ok(t) => t,
                    Err(_) => {
                        self.toast_problem("Could Not Open Terminal", format!("for the tunnel to port {}", r.port), None);
                        return;
                    }
                };
                t.type_at_prompt = Some(format!("{cmd}\r"));
                t.type_origin = Some(crate::finish_work::Origin::NusAction);
                let i = if let Some(i) = owner {
                    self.tabs[i].right = Some(Pane::Term(t));
                    self.tabs[i].focus_right = true;
                    i
                } else {
                    let tab = self.make_tab(Pane::Term(t), None);
                    self.tabs.push(tab);
                    self.tabs.len() - 1
                };
                self.activate(i);
                let id = self.tabs[i].id;
                if let Some(row) = self.board.row_mut(key) {
                    row.tunnel = Some(TunnelState { tab: id, url: None });
                }
                self.apply_term_resizes(false);
                self.close_board();
            }
            Act::Watch => {
                let name_key = format!("{}:{}", r.process.to_lowercase(), r.port);
                let on = !self.board.watched.contains(&name_key);
                if on {
                    self.board.watched.insert(name_key);
                } else {
                    self.board.watched.remove(&name_key);
                }
                if let Some(row) = self.board.row_mut(key) {
                    row.watch = on;
                }
                save_names(&self.board.names, &self.board.watched);
            }
            Act::Name => {
                let cur = r.name.clone().unwrap_or_default();
                self.board.rename = Some((key.clone(), cur));
            }
        }
        self.dirty = true;
    }

    fn ports_kill(&mut self, key: &Key) {
        let Some(r) = self.board.row(key).cloned() else { return };
        if r.pid == 0 {
            return;
        }
        // Signal the selected process, never an unrelated foreground job in
        // its terminal. Unknown or stale process identity fails closed.
        let Some(identity) = r.identity else {
            self.toast_problem("Could Not Verify Process", "refresh and try again", None);
            return;
        };
        if !nus_pty::ports::kill_identified(r.pid, identity, false) {
            self.toast_problem("Could Not Stop Process", "it changed or exited, or refused to stop", None);
            return;
        }
        self.board.killing.insert(r.pid, (crate::clock::now(), identity));
        if let Some(row) = self.board.row_mut(key) {
            row.dying = Some(crate::clock::now());
        }
        self.play_event("tab.close");
        self.dirty = true;
    }

    /// Keys while the board is up (overlay or page). Returns true when consumed.
    pub(crate) fn board_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if !self.board.open && !self.ports_page_open() {
            return false;
        }
        if ev.state != ElementState::Pressed {
            return true;
        }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let mods = self.mods;
        let key = ev.logical_key.clone();
        // Rename in progress.
        if let Some((k, s)) = self.board.rename.as_mut() {
            // The name's own editing: typing, erasing, paste (field.rs).
            if crate::field::edit(s, ev, mods, 80).taken() {
                self.dirty = true;
                return true;
            }
            match &key {
                WKey::Named(NamedKey::Escape) => self.board.rename = None,
                WKey::Named(NamedKey::Enter) => {
                    let (k, s) = (k.clone(), s.trim().to_string());
                    self.board.rename = None;
                    if let Some(r) = self.board.row_mut(&k) {
                        let name_key = format!("{}:{}", r.process.to_lowercase(), r.port);
                        if s.is_empty() {
                            r.name = None;
                            self.board.names.remove(&name_key);
                        } else {
                            r.name = Some(s.clone());
                            self.board.names.insert(name_key, s);
                        }
                    }
                    save_names(&self.board.names, &self.board.watched);
                }
                _ => {}
            }
            self.dirty = true;
            return true;
        }
        // A kill waiting on yes.
        if let Some(k) = self.board.confirm.clone() {
            match &key {
                WKey::Named(NamedKey::Enter) | WKey::Character(_) if matches!(key.to_text(), Some("y") | Some("Y") | None) => {
                    self.board.confirm = None;
                    self.ports_kill(&k);
                }
                _ => self.board.confirm = None,
            }
            self.dirty = true;
            return true;
        }
        // The filter line.
        if let Some(f) = self.board.filter.as_mut() {
            // Backspace on nothing closes it; the rest is the line's own
            // editing: typing, erasing, paste (field.rs).
            if f.is_empty() && matches!(key, WKey::Named(NamedKey::Backspace)) {
                self.board.filter = None;
                self.dirty = true;
                return true;
            }
            if crate::field::edit(f, ev, mods, 200).taken() {
                self.dirty = true;
                return true;
            }
            match &key {
                WKey::Named(NamedKey::Escape) => self.board.filter = None,
                WKey::Named(NamedKey::Enter) | WKey::Named(NamedKey::ArrowDown) | WKey::Named(NamedKey::ArrowUp) => {
                    // Fall through to selection with the filter kept.
                    return self.board_nav(&key, ctrl, shift);
                }
                _ => {}
            }
            self.dirty = true;
            return true;
        }
        if let WKey::Character(c) = &key {
            if !ctrl {
                let c = c.to_string();
                if c == "/" {
                    self.board.filter = Some(String::new());
                    self.dirty = true;
                    return true;
                }
                if c.eq_ignore_ascii_case("g") {
                    self.behavior.ports_grouping = self.behavior.ports_grouping.next();
                    self.save_prefs();
                    self.dirty = true;
                    return true;
                }
                if let Some(sel) = self.board.sel.clone() {
                    let act = [Act::Open, Act::Copy, Act::Jump, Act::Kill, Act::Again, Act::Tunnel, Act::Watch, Act::Name].into_iter().find(|a| a.key().eq_ignore_ascii_case(&c));
                    if let Some(a) = act {
                        self.ports_act(&sel, a);
                        return true;
                    }
                }
                if c.chars().all(|ch| ch.is_ascii_digit()) {
                    self.board.filter = Some(c);
                    self.dirty = true;
                    return true;
                }
            }
        }
        self.board_nav(&key, ctrl, shift)
    }

    fn board_nav(&mut self, key: &WKey, ctrl: bool, _shift: bool) -> bool {
        let listing = self.board.listing(self.behavior.ports_grouping);
        let keys: Vec<Key> = listing.iter().filter_map(|e| match e {
            Entry::Row(k) => Some(k.clone()),
            _ => None,
        }).collect();
        let pos = self.board.sel.as_ref().and_then(|s| keys.iter().position(|k| k == s));
        match key {
            WKey::Named(NamedKey::Escape) => {
                if self.board.expanded.is_some() {
                    self.board.expanded = None;
                } else if self.board.open {
                    self.close_board();
                } else {
                    return false;
                }
            }
            WKey::Named(NamedKey::ArrowDown) => {
                let next = pos.map(|p| (p + 1).min(keys.len().saturating_sub(1))).unwrap_or(0);
                self.board.sel = keys.get(next).cloned();
            }
            WKey::Named(NamedKey::ArrowUp) => {
                let next = pos.map(|p| p.saturating_sub(1)).unwrap_or(0);
                self.board.sel = keys.get(next).cloned();
            }
            WKey::Named(NamedKey::Home) => self.board.sel = keys.first().cloned(),
            WKey::Named(NamedKey::End) => self.board.sel = keys.last().cloned(),
            WKey::Named(NamedKey::Enter) if ctrl => self.expand_board(),
            WKey::Named(NamedKey::Enter) => {
                if let Some(s) = self.board.sel.clone() {
                    self.board.expanded = if self.board.expanded.as_ref() == Some(&s) { None } else { Some(s) };
                }
            }
            _ => return self.board.open,
        }
        self.board.reveal=true;
        self.dirty = true;
        true
    }

    /// Mouse on the board. Returns true when consumed.
    pub(crate) fn board_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        let page = self.ports_page_open();
        if !self.board.open && !page {
            return false;
        }
        if state != ElementState::Pressed {
            return self.board.open;
        }
        let hit = self.board.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| h.clone());
        let Some(hit) = hit else {
            if self.board.open && !self.board.rect.contains(x, y) && button == MouseButton::Left {
                self.close_board();
                return true;
            }
            return self.board.open;
        };
        if button != MouseButton::Left {
            return true;
        }
        match hit {
            Hit::Row(k) => {
                self.board.sel = Some(k.clone());
                self.board.expanded = if self.board.expanded.as_ref() == Some(&k) { None } else { Some(k) };
                self.board.confirm = None;
            }
            Hit::Name(k) => {
                self.board.sel = Some(k.clone());
                self.ports_act(&k, Act::Name);
            }
            Hit::Action(k, a) => {
                self.board.sel = Some(k.clone());
                self.ports_act(&k, a);
            }
            Hit::Confirm(k, yes) => {
                self.board.confirm = None;
                if yes {
                    self.ports_kill(&k);
                }
            }
            Hit::Expand => self.expand_board(),
            Hit::Close => self.close_board(),
            Hit::Grouping => {
                self.behavior.ports_grouping = self.behavior.ports_grouping.next();
                self.save_prefs();
            }
            Hit::Head(_) => {}
        }
        self.dirty = true;
        true
    }

    pub(crate) fn board_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !(self.board.open || self.ports_page_open()) || !self.board.rect.contains(x, y) {
            return false;
        }
        let max = (self.board.reach - self.board.viewport.h).max(0.0);
        self.board.scroll = (self.board.scroll - dy).clamp(0.0, max);
        self.dirty = true;
        true
    }

    // --- drawing ---

    /// The overlay: scrim, the sheet, the board inside.
    pub(crate) fn draw_board_overlay(&mut self, scene: &mut Scene, w: f32, h: f32) {
        if !self.board.open {
            return;
        }
        let t = self.theme.clone();
        let rise = self.board.rise.value();
        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), nus_render::theme::Theme::with_alpha(t.scrim, t.scrim[3] * rise));
        let bw = (w * 0.7).max(self.px(640.0)).min(w - 2.0 * self.px(16.0));
        let bh = (h * 0.78).min(h - self.px(80.0));
        let bx = ((w - bw) / 2.0).round();
        let by = ((h - bh) / 2.0 * 0.8).round() + (1.0 - rise) * self.px(10.0);
        let r = Rect::new(bx, by, bw, bh);
        scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), t.ink);
        scene.rect(r, t.paper);
        scene.outline(r, self.px(m::STRUCTURE), t.ink);
        self.board.rect = r;
        self.draw_board(scene, r, true);
    }

    fn draw_flap_text(&mut self,scene:&mut Scene,style:Style,r:Rect,old:&str,text:&str,cw:f32,paper:nus_render::Color,elapsed:f32) {
        let pin=crate::surface::mix(paper,style.color,0.5);
        crate::split_flap::text(&mut self.fonts,scene,style,r,old,text,cw,elapsed,paper,pin,self.scale);
    }

    /// The board itself, in `r` (the overlay sheet or a page).
    pub(crate) fn draw_board(&mut self, scene: &mut Scene, r: Rect, overlay: bool) {
        let t = self.theme.clone();
        let outer = scene.clip();
        let r = outer.map_or(r, |clip| r.intersect(&clip));
        // A dark instrument panel in both appearances, using the current theme.
        let paper = crate::surface::mix(t.paper, t.ink, if t.mode == nus_render::Mode::Paper {0.94} else {0.03});
        let ink = if t.mode == nus_render::Mode::Paper {t.paper} else {t.ink};
        let label = Style { color: ink, tracking:0.0, ..self.label() };
        let strong = Style { color: ink, tracking:0.0, ..self.label_strong() };
        let dim = Style { color: crate::surface::mix(paper, ink, 0.62), ..label };
        let mono = Style { font: self.f.term, px: self.px(15.0), color: ink, tracking: 0.0 };
        let mono_dim = Style { color: dim.color, ..mono };
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        let signal = self.surface.signal;
        let (mx, my) = self.mouse;
        let pad = self.px(18.0).min(r.w*0.04);
        let hair = self.px(1.0);
        let row_h = self.px(40.0);
        let head_h = self.px(30.0);
        let reduced = self.motion.reduced();
        let grouping = self.behavior.ports_grouping;
        let cw = self.fonts.measure(mono, "M").max(1.0) + self.px(3.0);
        let live:HashSet<_>=self.board.rows.iter().chain(&self.board.ghosts).map(|r|r.key.clone()).collect();
        self.board.flaps.retain(|key,_|live.contains(key));
        self.board.hits.clear();
        scene.layer(Some(r));
        scene.rect(r, paper);
        let mut y = r.y + self.px(16.0);
        self.fonts.draw_icon(scene,nus_render::text::icons::PORTS,self.px(20.0),r.x+pad,y,signal);
        let title = Style {px:self.px(20.0),..strong};
        self.fonts.draw(scene,title,r.x+pad+self.px(30.0),y+self.px(17.0),"PORT CONTROL");
        if overlay && r.w<self.px(440.0) {y+=self.px(31.0);}
        let mut rx = r.right()-pad;
        let chips: Vec<(&str,Hit)> = if overlay {vec![("CLOSE",Hit::Close),("EXPAND",Hit::Expand)]} else {vec![]};
        for (word,hit) in chips {
            let ww=self.fonts.measure(label,word)+self.px(16.0);rx-=ww;
            let hr=Rect::new(rx,y,ww,self.px(24.0));
            scene.outline(hr,hair,dim.color);
            self.fonts.draw(scene,label,rx+self.px(8.0),y+self.px(17.0),word);
            self.board.hits.push((hr,hit));rx-=self.px(8.0);
        }
        y+=self.px(37.0);
        let n_listen=self.board.rows.iter().filter(|row|matches!(row.key,Key::Port{proto:Proto::Tcp,..})).count();
        let n_udp=self.board.rows.iter().filter(|row|matches!(row.key,Key::Port{proto:Proto::Udp,..})).count();
        let n_exposed=self.board.rows.iter().filter(|row|row.exposed&&matches!(row.key,Key::Port{..})).count();
        let stats=format!("{n_listen} TCP listeners · {n_udp} UDP bindings · {n_exposed} all-interface bindings");
        for line in crate::reader::wrap(&self.fonts,dim,&stats,r.w-pad*2.0){self.fonts.draw(scene,dim,r.x+pad,y,&line);y+=self.px(18.0);}
        y-=self.px(4.0);
        let word=format!("GROUP: {}  ·  G",grouping.name().to_uppercase());
        let gw=self.fonts.measure(label,&word)+self.px(14.0);
        let group=Rect::new(r.x+pad,y,gw.min(r.w-pad*2.0),self.px(25.0));
        scene.rect(group,crate::surface::mix(paper,ink,0.08));
        self.fonts.draw(scene,label,group.x+self.px(7.0),y+self.px(17.0),&self.fit(label,&word,group.w-self.px(14.0)));
        self.board.hits.push((group,Hit::Grouping));
        if let Some(at)=self.board.last {
            let text=format!("UPDATED {}S AGO",crate::clock::since(at).as_secs());
            let tw=self.fonts.measure(dim,&text);
            if group.right()+self.px(20.0)<r.right()-pad-tw {self.fonts.draw(scene,dim,r.right()-pad-tw,y+self.px(17.0),&text);}
        }
        y+=self.px(35.0);
        if let Some(f)=&self.board.filter {
            let text=self.fit(mono,&format!("/ {f}_"),r.w-2.0*pad);
            self.fonts.draw(scene,mono,r.x+pad,y+self.px(14.0),&text);y+=self.px(26.0);
        }
        scene.hline(r.x+pad,y,r.w-2.0*pad,hair,dim.color);y+=self.px(7.0);
        let col_port=r.x+pad+self.px(16.0);
        let col_name=col_port+cw*6.0+self.px(10.0);
        let right=r.right()-pad-self.px(22.0);
        let up_w=if r.w>=self.px(420.0){cw*7.0}else{0.0};
        let owner_w=if r.w>=self.px(1000.0){cw*10.0}else{0.0};
        let proc_w=if r.w>=self.px(700.0){cw*16.0}else{0.0};
        let col_up=right-up_w;
        let col_owner=col_up-owner_w;
        let col_proc=col_owner-proc_w;
        for (x,width,word) in [(col_port,col_name-col_port,"PORT"),(col_name,col_proc-col_name,"SERVICE"),(col_proc,proc_w,"PROCESS"),(col_owner,owner_w,"OWNER"),(col_up,up_w,"PROCESS AGE")] {
            if width>0.0 {self.fonts.draw(scene,dim,x,y+self.px(14.0),word);}
        }
        y+=self.px(25.0);
        // Rows.
        let top = y;
        let bottom = (r.bottom() - self.px(40.0)).max(top);
        let body = Rect::new(r.x,top,r.w,(bottom-top).max(0.0)).intersect(&r);
        self.board.viewport=body;
        self.board.scroll=self.board.scroll.clamp(0.0,(self.board.reach-body.h).max(0.0));
        let body_hits=self.board.hits.len();
        scene.layer(Some(body));
        let mut y = top - self.board.scroll;
        let listing = self.board.listing(grouping);
        let expanded = self.board.expanded.clone();
        let mut selected_bounds=None;
        let sel = self.board.sel.clone();
        let confirm = self.board.confirm.clone();
        let rename = self.board.rename.clone();
        let mut departed: Vec<(Row, f32)> = self.board.departed.iter().map(|d| (d.row.clone(), crate::clock::since(d.at).as_secs_f32())).collect();
        if listing.is_empty() && departed.is_empty() {
            let msg = if self.board.polls == 0 { "Checking local ports…" } else if self.board.filter.is_some() { "No matching entries" } else { "No entries match your port settings" };
            self.fonts.draw(scene, dim, r.x + pad, y + self.px(30.0), msg);
            y += self.px(50.0);
        }
        for e in &listing {
            match e {
                Entry::Head(g, n) => {
                    let base = y + self.px(20.0);
                    let color = match g {
                        Group::Mine => signal,
                        Group::System => t.dim,
                        _ => ink,
                    };
                    self.fonts.draw(scene, Style { color, ..strong }, r.x + pad, base, g.name());
                    let count = format!("{n}");
                    self.fonts.draw(scene, dim, r.x + pad + self.fonts.measure(strong, g.name()) + self.px(8.0), base, &count);
                    if *g == Group::Mine {
                        self.fonts.draw(scene, dim, r.x + pad + self.fonts.measure(strong, g.name()) + self.px(8.0) + self.fonts.measure(label, &count) + self.px(10.0), base, &self.fit(dim,"· STARTED FROM YOUR TERMINALS",(r.w-self.px(120.0)).max(0.0)));
                    }
                    self.board.hits.push((Rect::new(r.x, y, r.w, head_h), Hit::Head(*g)));
                    y += head_h;
                }
                Entry::Process(p, n) => {
                    let base = y + self.px(20.0);
                    self.fonts.draw(scene, strong, r.x + pad, base, &self.fit(strong,&p.to_uppercase(),r.w-pad*2.0-self.px(30.0)));
                    self.fonts.draw(scene, dim, r.x + pad + self.fonts.measure(strong, &p.to_uppercase()) + self.px(8.0), base, &n.to_string());
                    y += head_h;
                }
                Entry::Row(k) => {
                    let Some(row) = self.board.row(k).cloned() else { continue };
                    let is_sel = sel.as_ref() == Some(k);
                    let is_open = expanded.as_ref() == Some(k);
                    let rr = Rect::new(r.x, y, r.w, row_h);
                    let hot = rr.contains(mx, my);
                    // Split-flap on arrival: the row's text drops in from the flap line.

                    let flap = 1.0;
                    if is_sel {
                        scene.rect(rr, fade(signal, 0.12));
                    } else if hot {
                        scene.rect(rr, crate::surface::mix(paper, ink, 0.04));
                    }
                    if let Some(tint) = row.rule.tint {
                        scene.rect(Rect::new(r.x + self.px(4.0), y + self.px(6.0), self.px(3.0), row_h - self.px(12.0)), tint);
                    }
                    // Exposed: a warning stripe down the left.
                    if row.exposed && matches!(row.key, Key::Port { .. }) {
                        let mut sy = y;
                        while sy < y + row_h {
                            scene.rect(Rect::new(r.x + self.px(10.0), sy, self.px(4.0), self.px(4.0)), ansi(3));
                            sy += self.px(8.0);
                        }
                    }
                    scene.layer(Some(Rect::new(r.x, y, r.w, row_h).intersect(&body)));
                    let drop = (1.0 - flap) * row_h;
                    let base = y + self.px(22.0) + drop;
                    // Lamp.
                    let lamp = row.lamp();
                    let lc = match lamp {
                        Lamp::Up => ansi(2),
                        Lamp::Exposed => ansi(3),
                        Lamp::Dying => {
                            let blink = ((row.dying.map(|d| crate::clock::since(d).as_secs_f32()).unwrap_or(0.0) * 4.0).sin() * 0.5 + 0.5) as f32;
                            fade(ansi(1), 0.4 + 0.6 * blink)
                        }
                        Lamp::Gone => t.dim,
                    };
                    let ld = self.px(7.0);
                    scene.rect(Rect::new(r.x + pad + self.px(4.0), base - ld + self.px(1.0), ld, ld), lc);
                    // Port · name · process · owner · uptime.
                    let port_s = match &row.key {
                        Key::Conn { .. } => format!("{:>5}", format!("×{}", row.conns)),
                        _ => format!("{:>5}{}", row.port, if row.proto == Proto::Udp { "u" } else { " " }),
                    };
                    let name = match &rename {
                        Some((rk, s)) if rk == k => format!("{s}_"),
                        _ => row.title(),
                    };
                    let name_w = col_proc - col_name - cw;
                    let name_fit = self.fit(mono, &name, name_w);


                    self.board.hits.push((Rect::new(col_name, y, name_w, row_h), Hit::Name(k.clone())));
                    let proc_s = match &row.key {
                        Key::Conn { .. } => row.remotes.first().cloned().unwrap_or_default(),
                        Key::Docker { .. } => row.container.as_ref().map(|c| c.1.clone()).unwrap_or_default(),
                        _ => {
                            let mut s = row.process.clone();
                            if row.pid > 0 {
                                s.push_str(&format!(" {}", row.pid));
                            }
                            s
                        }
                    };
                    let fields=[port_s.clone(),name_fit.to_uppercase(),proc_s.to_uppercase(),row.uptime()];
                    let widths=[cw*6.0,name_w,(proc_w-cw).max(0.0),up_w];
                    let cells:usize=widths.iter().map(|w|(w/cw).floor().max(0.0)as usize).sum();
                    let visible=rr.intersect(&body).h>0.0;
                    let now=crate::clock::now();
                    let change=self.board.flaps.entry(k.clone()).or_insert_with(||crate::split_flap::RowChange{before:if visible&&!reduced{Default::default()}else{fields.clone()},after:fields.clone(),at:now,cells});
                    if change.after!=fields {change.before=change.after.clone();change.after=fields.clone();change.at=now;}
                    change.cells=cells;
                    let editing=rename.as_ref().is_some_and(|(key,_)| key==k);
                    if !visible||reduced||editing {change.before=fields.clone();}
                    let change=change.clone();
                    let elapsed=if reduced||!visible{f32::INFINITY}else{crate::clock::since(change.at).as_secs_f32()};
                    let xs=[col_port,col_name,col_proc,col_up];let mut offset=0usize;
                    for field in 0..4 {
                        if widths[field]>0.0 {self.draw_flap_text(scene,if field<2{mono}else{mono_dim},Rect::new(xs[field],y+self.px(5.0),widths[field],row_h-self.px(10.0)),&change.before[field],&fields[field],cw,paper,elapsed-offset as f32*crate::split_flap::STAGGER);}
                        offset+=(widths[field]/cw).floor().max(0.0)as usize;
                    }
                    let owner = match row.group {
                        Group::Mine => row.tab.and_then(|id| self.tabs.iter().position(|t| t.id == id)).map(|i| format!("tab {}", i + 1)).unwrap_or_else(|| "shell".into()),
                        Group::Docker => "docker".into(),
                        Group::Remembered => "remembered".into(),
                        Group::System => "system".into(),
                        Group::Connections => if row.tab.is_some() { "mine".into() } else { String::new() },
                        Group::Others => "outside nus".into(),
                    };
                    if owner_w>0.0 {self.fonts.draw(scene,mono_dim,col_owner,base,&self.fit(mono_dim,&owner,col_up-col_owner-cw));}
                    // A fixed slot at the right edge for the × that comes
                    // up on hover, so nothing shifts under the pointer.

                    // ×: stop this process, and everything under it,
                    // without opening the row first. Only where KILL is
                    // on offer at all — docker rows and dead ones have
                    // nothing to stop.
                    let killable = !matches!(row.key, Key::Docker { .. }) && row.pid > 0 && row.lamp() != Lamp::Gone;
                    if killable && rr.contains(mx, my) && confirm.as_ref() != Some(k) {
                        let isz = self.px(12.0);
                        let kr = Rect::new(r.right() - pad - isz, base - isz + self.px(1.0), isz, isz);
                        let reach = crate::touch::grown(kr, self.px(6.0));
                        let hot = reach.contains(mx, my);
                        self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, isz, kr.x, kr.y, if hot { ansi(1) } else { t.dim });
                        if hot {
                            self.tip_words(reach, &format!("stop {} · and everything under it", row.process));
                        }
                        self.board.hits.push((reach, Hit::Action(k.clone(), Act::Kill)));
                    }
                    if row.watch {
                        self.fonts.draw_icon(scene, nus_render::text::icons::BELL, self.px(12.0), r.x + pad + self.px(4.0), y + self.px(4.0), t.dim);
                    }
                    // The flap line, mid-row, while it drops.
                    if flap < 1.0 {
                        scene.hline(r.x + pad, y + row_h * 0.5, r.w - 2.0 * pad, hair, fade(ink, 1.0 - flap));
                    }
                    scene.layer(Some(body));
                    scene.hline(r.x + pad, y + row_h - hair, r.w - 2.0 * pad, hair, fade(ink, 0.10));
                    self.board.hits.push((rr, Hit::Row(k.clone())));
                    y += row_h;
                    // The detail, opened in place.
                    if is_open {
                        let lines: Vec<(String, String)> = {
                            let mut v = Vec::new();
                            if matches!(row.key, Key::Port { .. }) {
                                v.push(("STATUS".into(), row.status().into()));
                                v.push(("BOUND ADDRESS".into(), format!("{} · {}", if row.bound.is_empty(){"unknown"}else{&row.bound}, bind_scope(&row.bound))));
                            }
                            if !row.cmdline.is_empty() {
                                v.push(("COMMAND".into(), row.cmdline.clone()));
                            } else if !row.exe.is_empty() {
                                v.push(("EXE".into(), row.exe.clone()));
                            }
                            if let Some(c) = &row.command {
                                v.push(("SAVED COMMAND".into(), c.clone()));
                            }
                            if let Some(c) = &row.cwd {
                                v.push(("IN".into(), c.clone()));
                            }
                            if let Some(p) = &row.probe {
                                let mut s = if p.status > 0 { format!("HTTP {}", p.status) } else { "No HTTP response observed".into() };
                                if !p.title.is_empty() {
                                    s.push_str(&format!(" · {}", p.title));
                                }
                                if !p.framework.is_empty() {
                                    s.push_str(&format!(" · {}", p.framework));
                                }
                                if !p.server.is_empty() {
                                    s.push_str(&format!(" · {}", p.server));
                                }
                                v.push(("HTTP CHECK".into(), s));
                            }
                            if !row.remotes.is_empty() {
                                v.push(("REMOTE ADDRESSES".into(), row.remotes.join(" · ")));
                            }
                            if let Some((c, i)) = &row.container {
                                v.push(("CONTAINER".into(), format!("{c} · {i}")));
                            }
                            if let Some(t) = &row.tunnel {
                                v.push(("TUNNEL".into(), t.url.clone().unwrap_or_else(|| "starting…".into())));
                            }
                            v
                        };
                        let lx=col_port;
                        let stacked=r.w<self.px(480.0);
                        let kw=if stacked{0.0}else{(cw*12.0).min((r.w-pad*2.0)*0.38)};
                        let width=(r.right()-pad-lx-kw).max(self.px(30.0));
                        let wrapped:Vec<_>=lines.iter().map(|(label,value)|(label,wrap_detail(&self.fonts,mono,value,width))).collect();
                        let label_h=if stacked{self.px(19.0)}else{0.0};
                        let dh=self.px(52.0)+wrapped.iter().map(|(_,v)|label_h+v.len()as f32*self.px(22.0)+self.px(4.0)).sum::<f32>();
                        scene.rect(Rect::new(r.x,y,r.w,dh),crate::surface::mix(paper,ink,0.03));
                        let mut ly=y+self.px(10.0);
                        for (label,values) in &wrapped {
                            self.fonts.draw(scene,dim,lx,ly+self.px(15.0),label);
                            ly+=label_h;
                            for value in values {self.fonts.draw(scene,mono,lx+kw,ly+self.px(15.0),value);ly+=self.px(22.0);}
                            ly+=self.px(4.0);
                        }
                        // The action strip.
                        ly += self.px(6.0);
                        let mut ax = lx;
                        let acts: Vec<Act> = match &row.key {
                            Key::Conn { .. } => vec![Act::Jump, Act::Kill, Act::Name],
                            Key::Docker { .. } => vec![Act::Open, Act::Copy, Act::Name],
                            Key::Port { .. } => {
                                let mut v = vec![Act::Open, Act::Copy];
                                if row.tab.is_some() {
                                    v.push(Act::Jump);
                                }
                                v.push(Act::Kill);
                                if row.command.is_some() {
                                    v.push(Act::Again);
                                }
                                v.push(Act::Tunnel);
                                v.push(Act::Watch);
                                v.push(Act::Name);
                                v
                            }
                        };
                        if confirm.as_ref() == Some(k) {
                            let q = self.fit(strong,&format!("Stop {} ({})?", row.process, row.pid),r.w-pad*2.0);
                            self.fonts.draw(scene,Style{color:ansi(1),..strong},ax,ly+self.px(16.0),&q);ly+=self.px(24.0);
                            for line in crate::reader::wrap(&self.fonts,dim,"Requests a stop. After 3 seconds, force-stops the same process if it is still running.",r.w-pad*2.0){self.fonts.draw(scene,dim,lx,ly+self.px(16.0),&line);ly+=self.px(20.0);}
                            ax=lx;
                            for (word, yes) in [("STOP · ENTER", true), ("CANCEL · ESC", false)] {
                                let ww = self.fonts.measure(label, word);
                                if ax+ww+self.px(6.0)>r.right()-pad {
                                    ax=lx;ly+=self.px(30.0);
                                    scene.rect(Rect::new(r.x,ly-self.px(6.0),r.w,self.px(42.0)),crate::surface::mix(paper,ink,0.03));
                                }
                                let hr = Rect::new(ax - self.px(6.0), ly, ww + self.px(12.0), self.px(26.0));
                                let hot = hr.contains(mx, my);
                                self.fonts.draw(scene, Style { color: if hot || yes { ink } else { t.dim }, ..strong }, ax, ly + self.px(16.0), word);
                                self.board.hits.push((hr, Hit::Confirm(k.clone(), yes)));
                                ax += ww + self.px(18.0);
                            }
                        } else {
                            for a in acts {
                                let on = match a {
                                    Act::Watch => row.watch,
                                    Act::Tunnel => row.tunnel.is_some(),
                                    _ => false,
                                };
                                let word = format!("{} · {}", a.label(), a.key());
                                let ww = self.fonts.measure(label, &word);
                                if ax+ww+self.px(6.0)>r.right()-pad {
                                    ax=lx;ly+=self.px(30.0);
                                    scene.rect(Rect::new(r.x,ly-self.px(6.0),r.w,self.px(42.0)),crate::surface::mix(paper,ink,0.03));
                                }
                                let hr = Rect::new(ax - self.px(6.0), ly, ww + self.px(12.0), self.px(26.0));
                                let hot = hr.contains(mx, my);
                                let color = if a == Act::Kill && hot { ansi(1) } else if hot || on { signal } else { ink };
                                if hot {
                                    scene.rect(hr, crate::surface::mix(paper, ink, 0.06));
                                }
                                self.fonts.draw(scene, Style { color, ..strong }, ax, ly + self.px(16.0), &word);
                                self.board.hits.push((hr, Hit::Action(k.clone(), a)));
                                ax += ww + self.px(18.0);
                            }
                        }
                        y = (y+dh).max(ly+self.px(36.0));
                        scene.hline(r.x + pad, y - hair, r.w - 2.0 * pad, hair, ink);
                    }
                    if is_sel {selected_bounds=Some((rr.y,y));}
                }
            }
        }
        // Departures: flap up and fade, in the place they had (at the end, simply).
        departed.retain(|(_, age)| *age < 0.45);
        for (row, age) in departed {
            let k = if reduced { 1.0 } else { ease_out(age / 0.45) };
            let ry = y - k * row_h;
            let rr = Rect::new(r.x, ry, r.w, row_h);
            scene.layer(Some(Rect::new(r.x, y.max(top), r.w, (bottom - y.max(top)).max(0.0))));
            let base = ry + self.px(22.0);
            let c = fade(t.dim, 1.0 - k);
            self.fonts.draw(scene, Style { color: c, ..mono }, col_port, base, &format!("{:>5}", row.port));
            self.fonts.draw(scene, Style { color: c, ..mono }, col_name, base, &self.fit(mono, &row.title(), col_proc - col_name - cw));
            self.fonts.draw(scene, Style { color: c, ..mono }, col_proc, base, "departed");
            let _ = rr;
            scene.layer(Some(body));
        }
        self.board.reach = (y + self.board.scroll - top) + self.px(8.0);
        if std::mem::take(&mut self.board.reveal) {
            if let Some((start,end))=selected_bounds {
                let shift=if start<body.y {start-body.y}else if end>body.bottom(){(end-body.bottom()).min(start-body.y)}else{0.0};
                self.board.scroll=(self.board.scroll+shift).clamp(0.0,(self.board.reach-body.h).max(0.0));
                if shift.abs()>0.5 {self.dirty=true;}
            }
        }
        for (hit,_) in &mut self.board.hits[body_hits..] {*hit=hit.intersect(&body);}
        self.board.hits.retain(|(hit,_)|hit.w>0.0 && hit.h>0.0);
        scene.layer(Some(r));
        // Foot: the keys.
        let fy = r.bottom() - self.px(34.0);
        scene.hline(r.x + pad, fy, r.w - 2.0 * pad, hair, ink);
        let foot = if r.w<self.px(480.0){"ENTER DETAILS · / FILTER · G GROUP"}else if self.board.sel.is_some(){"ENTER DETAILS · O OPEN · K STOP · / FILTER"}else{"↑↓ PICK · ENTER DETAILS · / FILTER · G GROUP"};
        self.fonts.draw(scene, dim, r.x + pad, fy + self.px(20.0), &self.fit(dim, foot, r.w - 2.0 * pad));
        scene.layer(outer);
    }

}

fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x)
}

fn bind_scope(address:&str)->&'static str {
    if matches!(address,"0.0.0.0"|"::"|"[::]"|"*"){return "All interfaces; network reachability not checked";}
    match address.trim_matches(['[',']']).parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_loopback()=>"Loopback interface",
        Ok(_)=>"Specific interface; network reachability not checked",
        Err(_)=>"Interface unknown",
    }
}

fn wrap_detail(fonts:&nus_render::FontSystem,style:Style,text:&str,width:f32)->Vec<String>{
    crate::reader::wrap(fonts,style,text,width).into_iter().flat_map(|line|{
        let mut lines=Vec::new();let mut part=String::new();
        for ch in line.chars(){let mut next=part.clone();next.push(ch);if !part.is_empty()&&fonts.measure(style,&next)>width{lines.push(std::mem::take(&mut part));}part.push(ch);}
        if !part.is_empty(){lines.push(part);}lines
    }).collect()
}

#[cfg(test)] mod label_tests {
    use super::*;
    fn remembered(port: u16) -> Row {
        Remembered {port, process:"node".into(),command:"npm start".into(),cwd:"/tmp".into(),last_seen:0}.row()
    }
    #[test]
    fn remembered_entries_survive_every_grouping_and_filter() {
        let mut r = remembered(3000);
        r.name = Some("My saved server".into());
        for grouping in [PortsGrouping::Origin, PortsGrouping::Port, PortsGrouping::Process] {
            let entries = listing(&[], &[r.clone()], Some("saved server"), grouping);
            assert!(entries.iter().any(|e| matches!(e, Entry::Row(k) if k == &r.key)));
        }
    }
    #[test]
    fn process_counts_include_only_visible_filtered_entries() {
        let mut live = remembered(3000);
        live.group = Group::Mine;
        let mut hidden = remembered(3001);
        hidden.rule.hide = true;
        let entries = listing(&[live, hidden], &[remembered(3002)], None, PortsGrouping::Process);
        assert!(matches!(&entries[0], Entry::Process(name, 2) if name == "node"));
        let entries = listing(&[remembered(3000)], &[remembered(3002)], Some("3002"), PortsGrouping::Process);
        assert!(matches!(&entries[0], Entry::Process(_, 1)));
    }
    #[test] fn binding_does_not_claim_reachability(){assert!(bind_scope("0.0.0.0").contains("not checked"));assert!(bind_scope("192.168.1.8").starts_with("Specific"));assert_eq!(bind_scope("[::1]"),"Loopback interface");assert_eq!(bind_scope("127.0.0.2"),"Loopback interface");assert_eq!(bind_scope(""),"Interface unknown");}
    #[test] fn udp_and_stop_requests_are_not_reported_as_live_tcp(){let mut r=Remembered{port:53,process:"dns".into(),command:String::new(),cwd:String::new(),last_seen:0}.row();assert_eq!(r.status(),"No current listener observed");r.group=Group::Others;r.proto=Proto::Udp;assert_eq!(r.status(),"UDP socket bound");r.dying=Some(crate::clock::now());assert_eq!(r.status(),"Stop requested");}
}

/// The board as a page.
pub struct PortsPane {
    pub rect: Rect,
}
