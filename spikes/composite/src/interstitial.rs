//! The pages nus shows in place of a site, or over one: a connection
//! that isn't private, a reported site, a clock that's off, a page that
//! crashed or ran out of memory, a form that would be sent again, a Wi-Fi
//! network that wants a sign-in, a site that can't be reached; and over a
//! live page, a hung renderer, a tab waking from sleep, a microphone or
//! camera macOS won't give, a download that was blocked.
//!
//! All of them read as a shell transcript: what nus tried, what happened,
//! then the next commands. Where nus watched it happen, the page carries a
//! trace (the "Trace" direction, 2026-10-01): each step it can vouch for,
//! with how long it took, and the step that failed marked by the rule —
//! signal for danger, ink for a problem. Pages without a trace keep the
//! rule down the left. ↵ always takes the safe command; going ahead anyway
//! is a dim command at the end, never the default.
//!
//! Pages that stand in for a site are HTML, written over Chromium's own
//! error document (or a blank one), so the address bar keeps the address
//! you asked for. Their commands come back through the `nusInterstitial`
//! binding with a token only that document knows. Overlays (`Page::native`)
//! are drawn by nus over the page, which stays alive underneath.
//!
//! `nus://interstitials` lists every page and the `nus://` commands that
//! make the real thing happen (`nus://crash`, `nus://hang`, …).

use std::collections::HashSet;
use std::sync::{Mutex, RwLock};

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cert,
    Malware,
    Clock,
    Oom,
    Crash,
    Resubmit,
    Portal,
    Unreachable,
    Index,
    // Overlays, over a page that is still there.
    /// A load with no answer yet: said, never stopped.
    Slow,
    Hung,
    Sleep,
    Permission,
    File,
    /// A page's own question: alert, confirm, prompt, leave-page, sign-in.
    Dialog,
}

impl Kind {
    pub const ALL: [Kind; 15] = [Kind::Cert, Kind::Malware, Kind::Clock, Kind::Oom, Kind::Crash, Kind::Permission, Kind::File, Kind::Resubmit, Kind::Sleep, Kind::Portal, Kind::Unreachable, Kind::Slow, Kind::Hung, Kind::Dialog, Kind::Index];
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Cert => "cert",
            Kind::Malware => "malware",
            Kind::Clock => "clock",
            Kind::Oom => "oom",
            Kind::Crash => "crash",
            Kind::Resubmit => "resubmit",
            Kind::Portal => "portal",
            Kind::Unreachable => "unreachable",
            Kind::Index => "index",
            Kind::Slow => "slow",
            Kind::Hung => "hung",
            Kind::Sleep => "sleep",
            Kind::Permission => "permission",
            Kind::File => "file",
            Kind::Dialog => "dialog",
        }
    }
    pub fn from_slug(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.slug() == s)
    }
    /// Drawn by nus over the page rather than in place of it.
    pub fn native(self) -> bool {
        matches!(self, Kind::Slow | Kind::Hung | Kind::Sleep | Kind::Permission | Kind::File | Kind::Dialog)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    Danger,
    Problem,
    Rest,
}

/// One next command: the verb nus runs, what it does in words, its key.
#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub verb: String,
    pub label: String,
    pub key: &'static str,
    /// Going ahead against the advice: dim, last, never the default.
    pub unsafe_: bool,
}

fn act(verb: &str, label: &str, key: &'static str) -> Act {
    Act { verb: verb.into(), label: label.into(), key, unsafe_: false }
}
fn risky(verb: &str, label: &str) -> Act {
    Act { verb: verb.into(), label: label.into(), key: "", unsafe_: true }
}

/// How a step of the trace went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Ok,
    Fail,
    /// Still going: nus is waiting on it.
    Wait,
    /// Not reached: an earlier step failed.
    Skip,
    /// A fact, not a step that passes or fails.
    Fact,
}

impl Mark {
    pub fn glyph(self) -> &'static str {
        match self {
            Mark::Ok => "✓",
            Mark::Fail => "✕",
            Mark::Wait => "…",
            Mark::Skip => "—",
            Mark::Fact => "",
        }
    }
}

/// One line of the trace: a step nus watched, what happened, how long.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub name: String,
    /// The first line says what happened; the rest are detail.
    pub what: Vec<String>,
    pub time: String,
    pub mark: Mark,
}

fn step(name: &str, what: Vec<String>, time: String, mark: Mark) -> Step {
    Step { name: name.into(), what, time, mark }
}

/// Facts under a label of their own, after the trace ("Last on :5173").
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub label: String,
    pub lines: Vec<String>,
}

/// A duration as the trace writes it: 0.4 ms, 12 ms, 1.2 s, 42 min.
pub fn took(d: std::time::Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{ms:.1} ms")
    } else if ms < 1000.0 {
        format!("{} ms", ms.round() as u64)
    } else if ms < 10_000.0 {
        format!("{:.1} s", ms / 1000.0)
    } else if ms < 120_000.0 {
        format!("{} s", (ms / 1000.0).round() as u64)
    } else if ms < 7_200_000.0 {
        format!("{} min", (ms / 60_000.0).round() as u64)
    } else {
        format!("{} h", (ms / 3_600_000.0).round() as u64)
    }
}

/// How a page failed, as its route shows it at the station where it did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// No address came back.
    Unknown,
    /// Turned away: a connection refused, credentials rejected.
    Wall,
    /// Nothing came back: a timeout, no route.
    Gap,
    /// The connection broke on the way.
    Cut,
    /// The network sent the request somewhere else (a sign-in page).
    Detour,
    /// Sent back round: a redirect loop.
    Loop,
    /// Still waiting for it.
    Glass,
    /// It came back empty or unreadable.
    Empty,
    /// The certificate failed.
    Seal,
    /// nus refused it: a listed dangerous site.
    Hazard,
    /// nus blocked it before asking.
    Ban,
    /// A download that nus blocked.
    File,
    /// The page's main thread is stuck.
    Stuck,
    /// The renderer crashed.
    Split,
    /// The renderer ran out of memory.
    Overflow,
    /// The renderer was terminated.
    Stop,
    /// The tab is asleep.
    Moon,
    /// This device's clock is wrong.
    Clock,
    /// This device refused the page a camera or microphone.
    Device,
    /// The request would be a form sent again.
    Uturn,
}

/// What is still happening at the break: drawn as a loop while it lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    Still,
    /// A request out, with no response yet.
    InFlight,
    /// A script running without a pause.
    Spinning,
    /// A renderer being terminated.
    Ending,
    /// A port being tried until it accepts connections.
    Polling,
}

/// The way a page travels, nus to page, and where it broke.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub stations: Vec<&'static str>,
    pub at: usize,
    pub mark: Break,
    pub live: Live,
    /// Already walked on this page: written again, it stands in place.
    pub settled: bool,
}

impl Route {
    /// The stations for `url` (tls only over https), broken at `station`.
    pub fn new(url: &str, station: &str, mark: Break) -> Route {
        let mut stations = vec!["nus", "dns", "connect"];
        if url.starts_with("https:") {
            stations.push("tls");
        }
        stations.extend(["request", "response", "page"]);
        let at = stations.iter().position(|s| *s == station).or_else(|| stations.iter().position(|s| *s == "connect")).unwrap_or(0);
        Route { stations, at, mark, live: Live::Still, settled: false }
    }

    pub fn live(mut self, live: Live) -> Route {
        self.live = live;
        self
    }

    fn mark_slug(&self) -> &'static str {
        match self.mark {
            Break::Unknown => "unknown", Break::Wall => "wall", Break::Gap => "gap", Break::Cut => "cut", Break::Detour => "detour",
            Break::Loop => "loop", Break::Glass => "glass", Break::Empty => "empty", Break::Seal => "seal", Break::Hazard => "hazard",
            Break::Ban => "ban", Break::File => "file", Break::Stuck => "stuck", Break::Split => "split", Break::Overflow => "overflow",
            Break::Stop => "stop", Break::Moon => "moon", Break::Clock => "clock", Break::Device => "device", Break::Uturn => "uturn",
        }
    }

    fn live_slug(&self) -> &'static str {
        match self.live { Live::Still => "", Live::InFlight => "flight", Live::Spinning => "spin", Live::Ending => "pulse", Live::Polling => "poll" }
    }
}

/// How fast the route walks, from the window's motion register: ms for a
/// segment, the break arriving, a live loop; all 0 when motion is reduced.
#[derive(Clone, Copy, Debug)]
pub struct Pace {
    pub seg: f32,
    pub pop: f32,
    pub loop_: f32,
}

static PACE: RwLock<Pace> = RwLock::new(Pace { seg: 120.0, pop: 220.0, loop_: 1400.0 });

/// The window's motion, for pages written from Chromium's callbacks.
pub fn set_pace(p: Pace) {
    *PACE.write().unwrap_or_else(|e| e.into_inner()) = p;
}

/// What nus saw of a page before it stopped answering.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// The document's HTTP status, and how long the server took to answer.
    pub server: Option<(i64, std::time::Duration)>,
    /// From asking for the page to its document loaded.
    pub loaded: Option<std::time::Duration>,
}

/// How a page's process ended, and what else nus knows about it.
#[derive(Clone, Debug, Default)]
pub struct Ended {
    pub oom: bool,
    /// You stopped it, from the page that wasn't responding.
    pub yours: bool,
    /// Something outside the page ended it: the system, another program.
    pub killed: bool,
    /// Chromium's code for it, and what that means: ("SIGSEGV (11)", "bad memory access").
    pub code: Option<(String, String)>,
    /// How long the page had been open.
    pub open: Option<std::time::Duration>,
    /// Other pages that ended with it: they shared its process.
    pub shared: Vec<String>,
    /// Earlier ends of this site in this session, oldest first: how long ago.
    pub before: Vec<std::time::Duration>,
}

/// A line to type into, on an overlay that asks for words (a prompt, a
/// sign-in). A secret one shows dots.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub secret: bool,
}

/// A page, ready to show either way.
#[derive(Clone, Debug)]
pub struct Page {
    pub kind: Kind,
    pub sev: Sev,
    /// The address this is about: the one you asked for.
    pub url: String,
    /// The first line of the transcript, after `»`.
    pub command: String,
    /// What happened, a line at a time; the first is the verdict. Pages
    /// with a trace say it there instead.
    pub log: Vec<String>,
    /// What nus watched happen, a step at a time.
    pub trace: Vec<Step>,
    /// Facts under their own labels, after the trace.
    pub notes: Vec<Note>,
    /// Where on its way the page failed: the glyph at the head of the page.
    pub route: Option<Route>,
    pub head: String,
    pub body: String,
    pub acts: Vec<Act>,
    /// Proves a command came from this document and not from a site.
    pub token: String,
    /// What the overlay asks you to type, and which line has the caret.
    pub fields: Vec<Field>,
    pub field: usize,
}

impl Page {
    fn new(kind: Kind, sev: Sev, url: &str, log: Vec<String>, head: String, body: String, mut acts: Vec<Act>) -> Page {
        // ↵ belongs to the first safe command alone.
        let default = acts.iter().position(|a| !a.unsafe_);
        for (i, a) in acts.iter_mut().enumerate() {
            if a.key == "↵" && Some(i) != default {
                a.key = "";
            }
        }
        Page { kind, sev, url: url.into(), command: format!("open {url}"), log, trace: Vec::new(), notes: Vec::new(), route: None, head, body, acts, token: crate::remote::new_token(), fields: Vec::new(), field: 0 }
    }

    /// The safe way out: back, or when there is nowhere to go back to,
    /// closing the tab.
    fn away(can_back: bool) -> Act {
        if can_back { act("back", "Back", "↵") } else { act("close", "Close tab", "↵") }
    }

    pub fn cert(url: &str, code: &str, can_back: bool) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Cert, Sev::Danger, url, Vec::new(),
            format!("Certificate not trusted · {host}"),
            format!("The connection to {host} is not private. Data sent to it, such as passwords or card numbers, can be read or altered in transit."),
            vec![Page::away(can_back), act("retry", "Retry", "⌘R"), risky("proceed", &format!("Continue to {host} (unsafe)"))]);
        p.trace = load_trace(url, code, &Failure { stage: Stage::Secure, only: None, head: String::new(), body: String::new(), what: cert_words(code).into() });
        p.route = Some(Route::new(url, "tls", Break::Seal));
        p
    }

    /// The clock, when it explains a certificate that isn't valid yet or
    /// any more. `behind` is how far, in days (negative: ahead).
    pub fn clock(url: &str, behind_days: i64, can_back: bool) -> Page {
        let host = host(url);
        let (way, n) = if behind_days >= 0 { ("behind", behind_days) } else { ("ahead", -behind_days) };
        let days = if n == 1 { "1 day".to_string() } else { format!("{n} days") };
        let mut p = Page::new(Kind::Clock, Sev::Problem, url,
            vec![format!("✕ system clock {days} {way}"), format!("  clock   {}", clock_now()), "  error   NET::ERR_CERT_DATE_INVALID".into()],
            format!("System clock is {days} {way}"),
            format!("Certificates for {host} cannot be validated until the date and time are correct."),
            vec![act("settings:date-time", "Date & time settings", "↵"), act("retry", "Retry", "⌘R"), Page::away(can_back)]);
        p.route = Some(Route::new(url, "tls", Break::Clock));
        p
    }

    pub fn malware(url: &str, list: &str, can_back: bool) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Malware, Sev::Danger, url,
            vec![format!("✕ {host} is listed as dangerous"), format!("  list    {list}"), "  no request was sent".into()],
            format!("Listed as dangerous · {host}"),
            format!("{host} appears in {list}, the list of sites that phish or distribute malware. No request was sent."),
            vec![Page::away(can_back), risky("proceed", &format!("Visit {host} anyway (unsafe)"))]);
        p.route = Some(Route::new(url, "nus", Break::Hazard));
        p
    }

    /// The page's process ended. `e` says how, and what nus knows besides.
    pub fn crashed(url: &str, e: &Ended) -> Page {
        let host = host(url);
        let after = e.open.filter(|_| !e.yours).map(|d| format!(" after {}", took(d))).unwrap_or_default();
        let (kind, head, mut body, status) = if e.oom {
            (Kind::Oom, format!("Renderer out of memory{after}"), "Memory limit exceeded. Other tabs are unaffected. Sleeping idle tabs frees memory before a reload.".to_string(), "out of memory".to_string())
        } else if e.yours {
            (Kind::Crash, "Page terminated".to_string(), "The renderer was terminated on request.".to_string(), "terminated on request".to_string())
        } else if e.killed {
            (Kind::Crash, format!("Renderer terminated{after}"), "Terminated by an external signal, for example the OS reclaiming memory. Other tabs are unaffected.".to_string(), "terminated externally".to_string())
        } else {
            let how = e.code.as_ref().map(|(_, words)| words.clone()).unwrap_or_else(|| "renderer error".into());
            (Kind::Crash, format!("Renderer crashed{after}"), format!("Exit: {how}. Other tabs are unaffected."), e.code.as_ref().map(|(c, _)| c.clone()).unwrap_or_else(|| "crashed".into()))
        };
        if !e.before.is_empty() && !e.yours {
            body.push_str(&format!(" {} on this site this session.", capitalized(&ordinal(e.before.len() + 1))));
        }
        let mut acts = Vec::new();
        if e.shared.is_empty() {
            acts.push(act("retry", "Reload", "↵"));
        } else {
            acts.push(act("retry-all", &format!("Reload all {} tabs", e.shared.len() + 1), "↵"));
            acts.push(act("retry", "Reload this tab", "⌘R"));
        }
        if e.oom {
            acts.push(act("sleep-idle", "Sleep idle tabs", "S"));
        }
        if !e.yours {
            acts.push(act("details", "Copy diagnostics", "C"));
        }
        let mut p = Page::new(kind, Sev::Problem, url, Vec::new(), head, body, acts);
        p.command = format!("page {url}");
        p.route = Some(Route::new(url, "page", if e.oom { Break::Overflow } else if e.yours { Break::Stop } else { Break::Split }));
        p.trace.push(step("process", vec![format!("renderer · {host}")], e.open.map(took).unwrap_or_default(), Mark::Fact));
        p.trace.push(step("exit", vec![status], String::new(), Mark::Fail));
        if !e.shared.is_empty() {
            p.trace.push(step("same process", vec![format!("{} also exited", e.shared.join(", "))], String::new(), Mark::Fact));
        }
        if !e.before.is_empty() {
            let ago: Vec<String> = e.before.iter().map(|d| format!("{} ago", took(*d))).collect();
            p.trace.push(step("history", vec![format!("exited {} on this site", ago.join(", ")), format!("{} this session", ordinal(e.before.len() + 1))], String::new(), Mark::Fact));
        }
        p
    }

    /// You chose to stop it; Chromium hasn't said it's gone yet.
    pub fn stopping(url: &str) -> Page {
        let mut p = Page::new(Kind::Crash, Sev::Problem, url, Vec::new(), "Terminating renderer".into(),
            "Reload runs after the process exits.".into(),
            vec![act("retry", "Reload", "↵")]);
        p.command = format!("stop {url}");
        p.route = Some(Route::new(url, "page", Break::Stop).live(Live::Ending));
        p.trace.push(step("process", vec!["terminating".into()], String::new(), Mark::Wait));
        p
    }

    /// Browser setup and presentation failures cannot rely on a web renderer
    /// to draw their explanation. The native transcript uses these same acts.
    pub fn browser_failed(url: &str, detail: &str) -> Page {
        let mut p = Page::new(Kind::Crash, Sev::Problem, url, Vec::new(),
            "Page can't be displayed".into(), detail.into(),
            vec![act("retry", "Retry", "↵"), act("close", "Close tab", "")]);
        p.trace.push(step("render", vec!["no frame displayed".into()], String::new(), Mark::Fail));
        p.route = Some(Route::new(url, "page", Break::Empty));
        p
    }

    pub fn resubmit(url: &str, can_back: bool) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Resubmit, Sev::Problem, url,
            vec!["✕ page is the result of a form submission · ERR_CACHE_MISS".into(), format!("  form    to {host}")],
            "Resend form data?".into(),
            format!("Reloading resubmits the form to {host}, which can repeat an action such as a purchase."),
            vec![Page::away(can_back), act("resubmit", "Resend", "⌘↵")]);
        p.route = Some(Route::new(url, "request", Break::Uturn));
        p
    }

    pub fn portal(url: &str, network: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Portal, Sev::Problem, url,
            vec!["✕ request intercepted by the network".into(), format!("  wi-fi   {network} requires sign-in")],
            "Network sign-in required".into(),
            format!("{network} intercepts requests until sign-in. {host} loads after sign-in."),
            vec![act("open-portal", "Open sign-in page", "↵"), act("retry", "Retry", "⌘R")]);
        p.route = Some(Route::new(url, "connect", Break::Detour));
        p
    }

    pub fn unreachable(url: &str, code: &str, can_back: bool) -> Page {
        let f = failure(url, code);
        let mut p = Page::new(Kind::Unreachable, Sev::Problem, url, Vec::new(), f.head.clone(), f.body.clone(),
            vec![act("retry", "Retry", "↵"), Page::away(can_back)]);
        p.trace = load_trace(url, code, &f);
        p.route = Some(load_route(url, code, &f));
        p
    }

    /// The same address failing the same way: a try of this page again.
    pub fn same_failure(&self, other: &Page) -> bool {
        let code = |p: &Page| p.trace.iter().find(|s| s.mark == Mark::Fail).and_then(|s| s.what.get(1).cloned());
        self.url == other.url && self.kind == other.kind && code(self).is_some() && code(self) == code(other)
    }

    /// Tried again and failed the same way: the newer time, and the count.
    pub fn tried_again(&mut self, again: &Page) {
        let time = again.trace.iter().find(|s| s.mark == Mark::Fail).map(|s| s.time.clone()).unwrap_or_default();
        if let Some(s) = self.trace.iter_mut().find(|s| s.mark == Mark::Fail) {
            if !time.is_empty() {
                s.time = time;
            }
            let tries = s.what.iter().find_map(|l| l.strip_prefix("tried ")?.strip_suffix(" times")?.parse::<u32>().ok()).unwrap_or(1) + 1;
            s.what.retain(|l| !l.starts_with("tried "));
            s.what.push(format!("tried {tries} times"));
        }
    }

    /// How long the failing step took: from asking to the error.
    pub fn took_to_fail(&mut self, d: std::time::Duration) {
        if let Some(s) = self.trace.iter_mut().find(|s| s.mark == Mark::Fail) {
            s.time = took(d);
        }
    }

    /// The first line of the failing step, said better (an HTTP status).
    pub fn failed_as(&mut self, what: String) {
        if let Some(s) = self.trace.iter_mut().find(|s| s.mark == Mark::Fail) {
            if let Some(first) = s.what.first_mut() {
                *first = what;
            }
        }
    }

    /// What last served this refused local port (from ports that remember):
    /// said under its own label, with its command offered to run again and
    /// a watch that loads the page when the port answers.
    pub fn last_on_port(&mut self, last: Option<(&str, &str, &str, std::time::Duration)>) {
        let port = port_of(&self.url);
        let mut first = Vec::new();
        if let Some((process, command, cwd, ago)) = last {
            self.notes.push(Note { label: format!("Last process on :{port}"), lines: vec![format!("{process} · {cwd}"), format!("command  {command}"), format!("last seen {} ago", took(ago))] });
            first.push(act("start", &format!("Run {command} in a new shell"), "↵"));
        }
        first.push(act("watch", &format!("Reload when :{port} accepts connections"), "W"));
        self.acts.splice(0..0, first);
        self.default_enter();
    }

    /// Watching the port: said, and the commands that would start a watch go.
    pub fn watching(&mut self, said: Option<String>) {
        let port = port_of(&self.url);
        self.notes.retain(|n| n.label != "Watching");
        self.notes.push(Note { label: "Watching".into(), lines: std::iter::once(format!(":{port} · reloads when it accepts connections")).chain(said).collect() });
        self.acts.retain(|a| a.verb != "watch" && a.verb != "start");
        if let Some(r) = self.route.as_mut() {
            r.live = Live::Polling;
        }
        self.default_enter();
    }

    /// ↵ goes to the first safe command again, after the commands changed.
    fn default_enter(&mut self) {
        let default = self.acts.iter().position(|a| !a.unsafe_);
        for (i, a) in self.acts.iter_mut().enumerate() {
            if Some(i) == default {
                a.key = "↵";
            } else if a.key == "↵" {
                a.key = if a.verb == "retry" { "⌘R" } else { "" };
            }
        }
    }

    /// What happened, as plain text for a bug report.
    pub fn details(&self, version: &str) -> String {
        let mut out = format!("{}\n{}\n{}\n", self.url, self.head, self.body);
        for s in &self.trace {
            out.push_str(&format!("{:<10}{}{}\n", s.name, s.what.join(" · "), if s.time.is_empty() { String::new() } else { format!(" · {}", s.time) }));
        }
        out.push_str(&format!("nus {version}\n"));
        out
    }

    /// The page's script hasn't answered for `secs`.
    pub fn hung(url: &str, secs: u64, seen: &Seen) -> Page {
        let host = host(url);
        let server = seen.server.map(|(_, d)| format!("The server responded in {}; the delay is in the page's JavaScript. ", took(d))).unwrap_or_else(|| "The delay is in the page's JavaScript. ".into());
        let mut p = Page::new(Kind::Hung, Sev::Problem, url, Vec::new(),
            format!("Page unresponsive · main thread blocked {secs} s"),
            format!("{server}Rendering and input on {host} are blocked."),
            vec![act("wait", "Wait 10 s", "↵"), act("stop", "Terminate renderer · unsaved input is lost", "S")]);
        p.command = format!("watch {url}");
        p.route = Some(Route::new(url, "page", Break::Stuck).live(Live::Spinning));
        if let Some((status, d)) = seen.server {
            p.trace.push(step("server", vec![format!("HTTP {status}")], took(d), Mark::Ok));
        }
        if let Some(d) = seen.loaded {
            p.trace.push(step("document", vec!["loaded".into()], took(d), Mark::Ok));
        }
        p.trace.push(step("main thread", vec!["blocked".into(), "no response to heartbeat".into()], format!("{secs} s"), Mark::Fail));
        p
    }

    /// A load with no answer after `waited`: nus says so, and stops nothing.
    pub fn slow(url: &str, waited: std::time::Duration) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Slow, Sev::Rest, url, Vec::new(),
            format!("No response after {} · {host}", took(waited)),
            "The request is still open. Nothing has been cancelled.".into(),
            vec![act("wait", "Wait", "↵"), act("retry", "Retry", "⌘R"), act("stop", "Stop loading", "Esc")]);
        p.route = Some(Route::new(url, "response", Break::Glass).live(Live::InFlight));
        p.trace = vec![
            step("request", vec![format!("sent to {host}")], String::new(), Mark::Ok),
            step("response", vec!["none yet".into()], took(waited), Mark::Wait),
        ];
        p
    }

    pub fn sleep(url: &str, asleep_for: std::time::Duration, waking: bool) -> Page {
        let host = host(url);
        let mins = (asleep_for.as_secs() / 60).max(1);
        let mut p = Page::new(Kind::Sleep, Sev::Rest, url,
            vec![format!("· asleep {} · memory released", if mins >= 60 { format!("{}h {}m", mins / 60, mins % 60) } else { format!("{mins} min") }), "  scroll  kept".into()],
            format!("{host} · asleep"),
            if waking { "Restoring.".into() } else { "Suspended after inactivity. Scroll position is kept.".into() },
            if waking { vec![] } else { vec![act("wake", "Wake", "↵")] });
        p.command = format!("wake {url}");
        p.route = Some(Route::new(url, "page", Break::Moon));
        p
    }

    pub fn permission(url: &str, what: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Permission, Sev::Problem, url,
            vec![format!("✕ macOS denied {what} access to nus"), format!("  site    {host}")],
            format!("{} access denied by macOS", capitalized(what)),
            "Allow nus in System Settings › Privacy & Security, then restart nus.".into(),
            vec![act("settings:privacy", "Open System Settings", "↵"), act("dismiss", "Back to page", "Esc")]);
        p.route = Some(Route::new(url, "page", Break::Device));
        p
    }

    pub fn file(url: &str, name: &str, why: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::File, Sev::Danger, url,
            vec![format!("✕ blocked {name}"), format!("  reason  {why}"), "  saved   nothing".into()],
            format!("Download blocked · {name}"),
            format!("Source: {host}. The file was not saved."),
            vec![act("dismiss", "Back to page", "↵"), act("downloads", "Downloads", "⌘J"), risky("keep", "Keep file (unsafe)")]);
        p.command = format!("download {url}");
        p.route = Some(Route::new(url, "response", Break::File));
        p
    }

    /// A page's `alert()`: what it says, and a way on.
    pub fn alert(url: &str, message: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Dialog, Sev::Rest, url, vec![format!("· {host} says")], String::new(), message.into(), vec![act("ok", "OK", "↵")]);
        p.command = format!("alert · {host}");
        p
    }

    /// A page's `confirm()`. Cancel is the safe answer, so it takes ↵;
    /// saying yes is a deliberate ⌘↵.
    pub fn confirm(url: &str, message: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Dialog, Sev::Rest, url, vec![format!("· {host} asks")], String::new(), message.into(), vec![act("cancel", "Cancel", "↵"), act("ok", "OK", "⌘↵")]);
        p.command = format!("confirm · {host}");
        p
    }

    /// A page's `prompt()`: the question, a line to answer on.
    pub fn prompt(url: &str, message: &str, default: &str) -> Page {
        let host = host(url);
        let mut p = Page::new(Kind::Dialog, Sev::Rest, url, vec![format!("· {host} asks")], String::new(), message.into(), vec![act("ok", "OK", "↵"), act("cancel", "Cancel", "Esc")]);
        p.command = format!("prompt · {host}");
        p.fields = vec![Field { label: "answer".into(), value: default.into(), secret: false }];
        p
    }

    /// Leaving (or reloading) a page that says it holds unsaved work.
    pub fn leave(url: &str, reload: bool) -> Page {
        let host = host(url);
        let (verb, what) = if reload { ("reload", "Reload") } else { ("leave", "Leave") };
        let mut p = Page::new(Kind::Dialog, Sev::Problem, url,
            vec![format!("✕ {host} has changes that may not be saved")],
            format!("{what} this page?"),
            "Changes you made may not be saved.".into(),
            vec![act("stay", "Stay on the page", "↵"), act(verb, what, "⌘↵")]);
        p.command = format!("{verb} {url}");
        p
    }

    /// A site (or a proxy) wants a username and password. Over plain HTTP
    /// they travel readable, and the page says so.
    pub fn signin(url: &str, host_name: &str, realm: &str, proxy: bool) -> Page {
        let plain = url.starts_with("http:");
        let who = if proxy { format!("the proxy {host_name}") } else { host_name.to_string() };
        let mut log = vec![format!("✕ {who} wants a sign-in")];
        if !realm.is_empty() {
            log.push(format!("  realm   {realm}"));
        }
        let body = if plain {
            format!("Your username and password go only to {who}, but this connection isn't private, so anyone on the network can read them.")
        } else {
            format!("Your username and password go only to {who}.")
        };
        let mut p = Page::new(Kind::Dialog, if plain { Sev::Danger } else { Sev::Problem }, url, log,
            format!("Sign in to {who}"), body,
            vec![act("signin", "Sign in", "↵"), act("cancel", "Cancel", "Esc")]);
        p.command = format!("sign in · {host_name}");
        p.fields = vec![
            Field { label: "username".into(), value: String::new(), secret: false },
            Field { label: "password".into(), value: String::new(), secret: true },
        ];
        p
    }

    /// `nus://interstitials`: every page, and the commands that cause the real thing.
    pub fn index() -> Page {
        let mut acts: Vec<Act> = Kind::ALL.iter().filter(|k| **k != Kind::Index).map(|k| act(&format!("open:nus://interstitial/{}", k.slug()), &format!("the {} page", k.slug()), "")).collect();
        for (url, what) in DEBUG_URLS {
            acts.push(act(&format!("open:{url}"), what, ""));
        }
        let mut p = Page::new(Kind::Index, Sev::Rest, "nus://interstitials",
            vec![format!("· {} pages, {} commands", Kind::ALL.len() - 1, DEBUG_URLS.len())],
            "Pages nus shows when a site can't".into(),
            "Each nus://interstitial/… address shows a page with sample facts. The commands below it make the real thing happen to the page you're on.".into(),
            acts);
        p.command = "ls nus://interstitials".into();
        p
    }

    /// A page with sample facts, for `nus://interstitial/<kind>`.
    pub fn sample(kind: Kind) -> Page {
        match kind {
            Kind::Cert => Page::cert("https://expired.badssl.com/", "NET::ERR_CERT_DATE_INVALID", true),
            Kind::Malware => Page::malware("http://login-paypa1-secure.example/", "profile/dangerous.txt", true),
            Kind::Clock => Page::clock("https://nus.dev/", 3, true),
            Kind::Oom => Page::crashed("https://figma.com/file/8Hq2/Plot", &Ended { oom: true, open: Some(std::time::Duration::from_secs(25 * 60)), ..Ended::default() }),
            Kind::Crash => Page::crashed("https://docs.example.com/editor", &Ended {
                code: Some(("SIGSEGV (11)".into(), "bad memory access".into())),
                open: Some(std::time::Duration::from_secs(42 * 60)),
                shared: vec!["docs.example.com/sheet".into()],
                before: vec![std::time::Duration::from_secs(11 * 60)],
                ..Ended::default()
            }),
            Kind::Resubmit => Page::resubmit("https://shop.example.com/checkout", true),
            Kind::Portal => Page::portal("https://news.ycombinator.com/", "this network"),
            Kind::Unreachable => Page::unreachable("http://localhost:3000/", "ERR_CONNECTION_REFUSED", true),
            Kind::Index => Page::index(),
            Kind::Slow => Page::slow("https://reports.example.com/q3", std::time::Duration::from_secs(30)),
            Kind::Hung => Page::hung("https://maps.example.com/route", 12, &Seen {
                server: Some((200, std::time::Duration::from_millis(300))),
                loaded: Some(std::time::Duration::from_millis(1100)),
            }),
            Kind::Sleep => Page::sleep("https://docs.rs/tokio/latest/tokio/", std::time::Duration::from_secs(14 * 60), false),
            Kind::Permission => Page::permission("https://meet.example.com/abc-defg", "microphone"),
            Kind::File => Page::file("https://files.example.net/invoice.pdf.exe", "invoice.pdf.exe", "a program named like a document"),
            Kind::Dialog => Page::confirm("https://mail.example.com/", "Delete 3 conversations?"),
        }
    }

    /// Written over the current document, built node by node: Chrome's
    /// own error pages enforce Trusted Types, which refuse any HTML string
    /// (document.write, innerHTML, DOMParser), but not DOM made by hand.
    /// The listeners come from this script, which the page's CSP does not
    /// govern either.
    pub fn script(&self) -> String {
        let c = *COLORS.read().unwrap_or_else(|e| e.into_inner());
        let pace = { let p = *PACE.read().unwrap_or_else(|e| e.into_inner()); serde_json::json!({ "seg": p.seg, "pop": p.pop, "loop": p.loop_ }) };
        let data = serde_json::json!({
            "title": self.head,
            "sev": match self.sev { Sev::Danger => "danger", Sev::Problem => "", Sev::Rest => "rest" },
            "command": self.command,
            "log": self.log,
            "head": self.head,
            "body": self.body,
            "trace": self.trace.iter().map(|s| serde_json::json!({ "name": s.name, "what": s.what, "time": s.time, "mark": s.mark.glyph(), "fail": s.mark == Mark::Fail })).collect::<Vec<_>>(),
            "notes": self.notes.iter().map(|n| serde_json::json!({ "label": n.label, "lines": n.lines })).collect::<Vec<_>>(),
            "route": self.route.as_ref().map(|r| serde_json::json!({ "stations": r.stations, "at": r.at, "mark": r.mark_slug(), "live": r.live_slug(), "settled": r.settled, "label": format!("{} at {}", self.head, r.stations[r.at]) })),
            "pace": pace,
            "acts": self.acts.iter().map(|a| serde_json::json!({ "verb": a.verb, "shown": shown_verb(&a.verb), "what": if a.key.is_empty() { a.label.clone() } else { format!("{} · {}", a.label, a.key) }, "unsafe": a.unsafe_ })).collect::<Vec<_>>(),
            "token": self.token,
            "css": self.css(c),
            "scheme": if c.dark { "dark" } else { "light" },
        });
        format!("({})({},{});", BUILD_JS, data, ROUTE_JS)
    }

    fn css(&self, c: Colors) -> String {
        format!(r#"{fonts}
:root{{--paper:{paper};--ink:{ink};--dim:{dim};--signal:{signal};--edge:{edge};--rule:var(--ink);color-scheme:{scheme}}}
main.danger{{--rule:var(--signal)}}
html,body{{margin:0;background:var(--paper);color:var(--ink)}}
body{{font:14px/1.6 "nus mono",ui-monospace,Menlo,Consolas,monospace;-webkit-font-smoothing:antialiased}}
main{{display:grid;grid-template-columns:3px minmax(0,1fr);gap:0 22px;padding:44px 48px;max-width:880px}}
.gut{{background:var(--ink)}}.gut.danger{{background:var(--signal)}}.gut.rest{{background:transparent}}
.line{{white-space:pre-wrap;overflow-wrap:anywhere}}.line.d{{color:var(--dim)}}.p{{color:var(--signal)}}.verdict{{font-weight:500}}
.gap{{height:18px}}.head{{font-weight:500;font-size:16px}}.body{{max-width:68ch}}
.cap{{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:var(--dim);margin-bottom:4px}}
.cmd{{display:grid;grid-template-columns:minmax(18ch,max-content) 1fr;gap:18px;cursor:pointer;padding:1px 6px;margin-left:-6px}}
.cmd .what{{color:var(--dim)}}.cmd.unsafe .verb{{color:var(--dim)}}
.cmd:hover{{outline:1px solid var(--ink);outline-offset:-1px}}
.cmd[aria-selected=true]{{background:var(--ink);color:var(--paper)}}.cmd[aria-selected=true] .what,.cmd[aria-selected=true] .verb{{color:var(--paper)}}
.steps{{border-bottom:1px solid var(--edge)}}
.step{{display:grid;grid-template-columns:11ch minmax(0,1fr) 9ch 2ch;gap:0 14px;padding:3px 0 3px 12px;border-top:1px solid var(--edge)}}
.step .name{{font-size:12px;color:var(--dim);line-height:22.4px}}
.step .t,.step .m{{text-align:right}}.step .t{{color:var(--dim)}}.step .d{{color:var(--dim)}}
.step.fail{{box-shadow:inset 3px 0 0 var(--rule)}}.step.fail .name{{color:var(--ink)}}.step.fail .first,.step.fail .m{{font-weight:500}}
.strip{{border-top:1.5px solid var(--ink);border-bottom:1px solid var(--edge);margin:12px 0 2px;padding-top:4px}}
.route{{display:block;width:100%;height:auto;overflow:visible}}
.route text{{font:12px "nus mono",ui-monospace,monospace;fill:var(--dim)}}.route text.at{{fill:var(--ink);font-weight:500}}main.danger .route text.at{{fill:var(--signal)}}
.route .walk{{stroke-dasharray:var(--len);stroke-dashoffset:var(--len);animation:walk var(--t) linear var(--d) forwards}}
.route .pop{{opacity:0;transform-box:fill-box;transform-origin:center;animation:pop var(--t) cubic-bezier(.2,.8,.2,1) var(--d) forwards}}
.route .fade{{opacity:0;animation:fade var(--t) linear var(--d) forwards}}
.route .fly{{opacity:0;animation:fly var(--t) linear var(--d) infinite}}
.route .poll{{opacity:0;animation:poll var(--t) ease-in-out var(--d) infinite}}
.route .spin{{animation:spin var(--t) linear var(--d) infinite}}
.route .pulse{{animation:pulse var(--t) ease-in-out var(--d) infinite}}
@keyframes walk{{to{{stroke-dashoffset:0}}}}
@keyframes pop{{from{{opacity:0;transform:scale(.6)}}to{{opacity:1;transform:none}}}}
@keyframes fade{{to{{opacity:1}}}}
@keyframes fly{{0%{{opacity:1;transform:translateX(0)}}100%{{opacity:1;transform:translateX(var(--dx))}}}}
@keyframes poll{{0%,100%{{opacity:1;transform:translateX(0)}}50%{{opacity:1;transform:translateX(var(--dx))}}}}
@keyframes spin{{to{{transform:rotate(360deg)}}}}
@keyframes pulse{{50%{{opacity:.35}}}}
.route.still .walk,.route.still .pop,.route.still .fade{{animation:none;opacity:1;stroke-dashoffset:0;transform:none}}
.route.still .fly,.route.still .poll{{display:none}}.route.still .spin,.route.still .pulse{{animation:none}}
@media (prefers-reduced-motion:reduce){{.route .walk,.route .pop,.route .fade{{animation:none;opacity:1;stroke-dashoffset:0;transform:none}}.route .fly,.route .poll{{display:none}}.route .spin,.route .pulse{{animation:none}}}}
.prompt{{display:flex;gap:1ch;align-items:baseline}}
.prompt input{{flex:1;min-width:0;font:inherit;color:var(--ink);background:transparent;border:0;outline:0;padding:0;caret-color:var(--ink)}}
@media (max-width:560px){{main{{padding:24px 18px}}}}"#,
            fonts = font_css(), paper = css(c.paper), ink = css(c.ink), dim = css(c.dim), signal = css(c.signal), edge = css([c.ink[0], c.ink[1], c.ink[2], 0.14]), scheme = if c.dark { "dark" } else { "light" })
    }

    /// The first safe command, which ↵ takes.
    pub fn default_act(&self) -> Option<&Act> {
        self.acts.iter().find(|a| !a.unsafe_)
    }
}

/// The verb as the transcript shows it: `open:nus://x` reads `open nus://x`.
fn shown_verb(verb: &str) -> String {
    verb.replacen(':', " ", usize::from(verb.starts_with("open:")))
}

/// Builds the transcript from the page's data (see `Page::script`).
const BUILD_JS: &str = r#"(P,ROUTE)=>{
const d=document;d.open();d.close();
const el=(tag,cls,text)=>{const e=d.createElement(tag);if(cls)e.className=cls;if(text!=null)e.textContent=text;return e};
const root=d.documentElement||d.appendChild(d.createElement('html'));
const head=d.head||root.appendChild(d.createElement('head'));
const body=d.body||root.appendChild(d.createElement('body'));
d.title=P.title;
const meta=el('meta');meta.name='color-scheme';meta.content=P.scheme;head.appendChild(meta);
try{const s=new CSSStyleSheet();s.replaceSync(P.css);d.adoptedStyleSheets=[s]}catch(e){head.appendChild(el('style',null,P.css))}
const main=el('main',P.sev);main.appendChild(el('div','gut '+(P.trace.length?'rest':P.sev)));const col=el('div');main.appendChild(col);body.appendChild(main);
const line=(cls,text)=>col.appendChild(el('div','line'+(cls?' '+cls:''),text));
const first=el('div','line');first.append(el('span','p','»'),' '+P.command);col.appendChild(first);
if(P.route)col.appendChild(ROUTE(d,P.route,P.pace));
P.log.forEach((l,i)=>line(i===0?'verdict':'',l));
if(P.log.length)col.appendChild(el('div','gap'));line('head',P.head);line('body',P.body);
if(P.trace.length){col.appendChild(el('div','gap'));col.appendChild(el('div','cap','Trace'));const t=el('div','steps');col.appendChild(t);
 P.trace.forEach(s=>{const r=el('div','step'+(s.fail?' fail':''));const w=el('div');
  s.what.forEach((l,i)=>w.appendChild(el('div','line'+(i?' d':' first'),l)));
  r.append(el('span','name',s.name),w,el('span','t',s.time),el('span','m',s.mark));t.appendChild(r)})}
P.notes.forEach(n=>{col.appendChild(el('div','gap'));col.appendChild(el('div','cap',n.label));n.lines.forEach((l,i)=>line(i?'d':'',l))});
const run=v=>{try{window.nusInterstitial(JSON.stringify({token:P.token,verb:v}))}catch(e){}};
const rows=[];
if(P.acts.length){col.appendChild(el('div','gap'));col.appendChild(el('div','cap','Next'));
 const next=el('div');next.setAttribute('role','listbox');col.appendChild(next);
 P.acts.forEach(a=>{const r=el('div','cmd'+(a.unsafe?' unsafe':''));r.setAttribute('role','option');r.dataset.verb=a.verb;
  r.append(el('span','verb','» '+a.shown),el('span','what',a.what));r.onclick=()=>run(a.verb);next.appendChild(r);rows.push(r)})}
let sel=Math.max(0,P.acts.findIndex(a=>!a.unsafe));
const mark=()=>rows.forEach((r,i)=>r.setAttribute('aria-selected',String(i===sel)));
rows.forEach((r,i)=>r.onmouseenter=()=>{sel=i;mark()});
col.appendChild(el('div','gap'));const pr=el('div','prompt');pr.appendChild(el('span','p','»'));
const input=el('input');input.autocomplete='off';input.spellcheck=false;input.setAttribute('aria-label','Type a command, or press Enter for the highlighted one');pr.appendChild(input);col.appendChild(pr);
d.addEventListener('keydown',e=>{
 if(rows.length&&(e.key==='ArrowDown'||e.key==='ArrowUp')){sel=(sel+(e.key==='ArrowDown'?1:rows.length-1))%rows.length;mark();e.preventDefault();return}
 if(e.key==='Enter'){const typed=input.value.trim().toLowerCase();
  if(!typed){if(rows[sel])run(rows[sel].dataset.verb);return}
  const hit=P.acts.find(a=>a.shown.toLowerCase()===typed||a.shown.toLowerCase().split(' ')[0]===typed);
  if(hit)run(hit.verb);else{input.value='';input.placeholder='no such command · try one of the above'}}
 if(e.key==='Escape')input.value=''});
mark();input.focus();
}"#;

/// Draws a route as SVG, node by node, walking from nus to the break. The
/// same geometry as `interstitial_ui::draw_route`.
const ROUTE_JS: &str = r#"(d,R,T)=>{
const NS='http://www.w3.org/2000/svg',W=820,H=92,pad=14,y=34,n=R.stations.length,at=R.at;
const xs=R.stations.map((_,i)=>pad+i*(W-2*pad)/(n-1));
const svg=d.createElementNS(NS,'svg');svg.setAttribute('viewBox','0 0 '+W+' '+H);svg.setAttribute('role','img');svg.setAttribute('aria-label',R.label);
svg.setAttribute('class','route'+(T.loop>0?'':' still'));
if(R.settled)T={seg:0,pop:0,loop:T.loop};
const INK='var(--ink)',HOT='var(--signal)',FAINT='var(--edge)',PAPER='var(--paper)';
const css=(e,o)=>{let s='';for(const k in o)s+=k+':'+o[k]+';';e.setAttribute('style',s);return e};
const mk=(parent,tag,a,cls)=>{const e=d.createElementNS(NS,tag);for(const k in a)e.setAttribute(k,a[k]);if(cls)e.setAttribute('class',cls);parent.appendChild(e);return e};
const g=(cls,delay,dur)=>css(mk(svg,'g',{},cls),{'--d':delay+'ms','--t':dur+'ms'});
const fill=c=>({fill:c});const stroke=(c,w)=>({fill:'none',stroke:c,'stroke-width':w});
const rect=(p,x,y,w,h,a)=>mk(p,'rect',Object.assign({x,y,width:w,height:h},a));
const circ=(p,cx,cy,r,a)=>mk(p,'circle',Object.assign({cx,cy,r},a));
const poly=(p,pts,a)=>mk(p,'polygon',Object.assign({points:pts.map(q=>q.join(',')).join(' ')},a));
const line=(p,x1,y1,x2,y2,a,cls)=>mk(p,'line',Object.assign({x1,y1,x2,y2},a),cls);
const fail=at*T.seg,live=fail+T.pop;
// The way: walked in ink, the rest faint.
for(let i=0;i<n-1;i++){const a=xs[i]+8,b=xs[i+1]-8;
 if(i+1<=at){const last=i+1===at;
  if(last&&(R.mark==='gap'||R.mark==='glass'))css(line(svg,a,y,b,y,{stroke:INK,'stroke-width':4,'stroke-dasharray':R.mark==='gap'?'7 6':'2 5'},'fade'),{'--d':i*T.seg+'ms','--t':T.seg+'ms'});
  else if(last&&R.mark==='cut'){const m=(a+b)/2,s=g('pop',i*T.seg,T.pop);css(line(svg,a,y,m-8,y,{stroke:INK,'stroke-width':4},'walk'),{'--len':(m-8-a)+'px','--d':i*T.seg+'ms','--t':T.seg+'ms'});
   poly(s,[[m-6,y+11],[m-2,y+11],[m+2,y-11],[m-2,y-11]],fill(HOT));poly(s,[[m+2,y+11],[m+6,y+11],[m+10,y-11],[m+6,y-11]],fill(HOT))}
  else css(line(svg,a,y,b,y,{stroke:INK,'stroke-width':4},'walk'),{'--len':(b-a)+'px','--d':i*T.seg+'ms','--t':T.seg+'ms'})}
 else line(svg,a,y,b,y,{stroke:FAINT,'stroke-width':1.5,'stroke-dasharray':'4 4'})}
// Stations.
for(let i=0;i<n;i++){if(i===at)continue;const x=xs[i],name=R.stations[i],box=name==='page'||name==='nus',s=name==='page'?16:12;
 if(i<at){const p=g('pop',Math.max(0,i*T.seg-T.seg/2),T.pop);if(box)rect(p,x-s/2,y-s/2,s,s,fill(INK));else circ(p,x,y,6.5,fill(INK))}
 else if(box)rect(svg,x-s/2,y-s/2,s,s,stroke(FAINT,2));else circ(svg,x,y,6,stroke(FAINT,2))}
// The break.
const x=xs[at],m=g('pop',fail,T.pop),px=at>0?xs[at-1]:x;
({unknown:()=>circ(m,x,y,9,Object.assign(stroke(HOT,3.5),{'stroke-dasharray':'4 3'})),
 wall:()=>rect(m,x-4,y-17,8,34,fill(HOT)),
 gap:()=>circ(m,x,y,7,stroke(HOT,3)),
 cut:()=>circ(m,x,y,6,stroke(FAINT,2)),
 detour:()=>{circ(m,x,y,6.5,fill(INK));line(m,x,y+7,x,y+26,{stroke:HOT,'stroke-width':4});rect(m,x-8,y+26,16,10,fill(HOT))},
 loop:()=>{circ(m,x,y,6.5,fill(HOT));mk(m,'path',Object.assign({d:'M'+x+' '+(y-9)+' C '+x+' '+(y-34)+', '+px+' '+(y-34)+', '+px+' '+(y-12)},stroke(HOT,3.5)));poly(m,[[px-6,y-16],[px+6,y-16],[px,y-8]],fill(HOT))},
 glass:()=>{poly(m,[[x-9,y-12],[x+9,y-12],[x,y]],fill(HOT));poly(m,[[x,y],[x-9,y+12],[x+9,y+12]],stroke(HOT,2.5))},
 empty:()=>rect(m,x-8,y-8,16,16,stroke(HOT,3.5)),
 seal:()=>{poly(m,[[x-1.5,y-12],[x-13,y],[x-1.5,y+12]],fill(INK));poly(m,[[x+1.5,y-12],[x+13,y],[x+1.5,y+12]],fill(HOT))},
 hazard:()=>{const id='hz'+at;rect(mk(svg,'clipPath',{id}),x-9,y-9,18,18,{});rect(m,x-9,y-9,18,18,fill(HOT));const sg=mk(m,'g',{'clip-path':'url(#'+id+')'});for(const o of [-18,-10,-2,6,14])line(sg,x+o-9,y+9,x+o+9,y-9,{stroke:INK,'stroke-width':4})},
 ban:()=>{circ(m,x,y,10,stroke(INK,3.5));line(m,x-7,y+7,x+7,y-7,{stroke:HOT,'stroke-width':3.5})},
 file:()=>{circ(m,x,y,6.5,fill(INK));line(m,x,y+7,x,y+20,{stroke:INK,'stroke-width':4});rect(m,x-7,y+22,14,12,fill(HOT))},
 stuck:()=>{rect(m,x-10,y-10,20,20,stroke(INK,2.5));circ(m,x,y,5,stroke(INK,2.5))},
 split:()=>{poly(m,[[x-10,y-10],[x+6,y-10],[x-10,y+6]],fill(INK));poly(m,[[x+10,y-6],[x+10,y+10],[x-6,y+10]],fill(HOT))},
 overflow:()=>{rect(m,x-8,y-8,16,16,stroke(INK,2.5));rect(m,x-5,y-5,10,10,fill(INK));rect(m,x-10,y-15,20,4,fill(HOT))},
 stop:()=>rect(m,x-9,y-9,18,18,fill(HOT)),
 moon:()=>{circ(m,x,y,9,fill(INK));circ(m,x+4,y-3,7.5,fill(PAPER))},
 clock:()=>{circ(m,x,y,9,stroke(HOT,3));line(m,x,y,x,y-6,{stroke:HOT,'stroke-width':2.5});line(m,x,y,x+5,y,{stroke:HOT,'stroke-width':2.5});rect(m,xs[0]-7,y-7,14,14,stroke(HOT,2.5))},
 device:()=>{rect(m,x-8,y-8,16,16,fill(INK));line(m,x,y+9,x,y+22,{stroke:INK,'stroke-width':4});rect(m,x-8,y+22,16,10,fill(HOT))},
 uturn:()=>{circ(m,x,y,6.5,fill(INK));mk(m,'path',Object.assign({d:'M'+(x+9)+' '+y+' H'+(x+20)+' V'+(y+18)+' H'+(x-4)},stroke(HOT,3.5)));poly(m,[[x-2,y+12],[x-2,y+24],[x-10,y+18]],fill(HOT))}
})[R.mark]();
// What is still happening there.
if(R.live==='flight'){const a=px+8,b=x-14,e=rect(svg,a-3,y-3,6,6,fill(INK));e.setAttribute('class','fly');css(e,{'--dx':(b-a)+'px','--d':live+'ms','--t':T.loop+'ms'})}
if(R.live==='poll'){const a=px+8,b=x-12;const e=circ(svg,a,y,4,fill(INK));e.setAttribute('class','poll');css(e,{'--dx':(b-a)+'px','--d':live+'ms','--t':T.loop+'ms'})}
if(R.live==='spin'){const s=mk(svg,'g',{},'spin');css(s,{'transform-origin':x+'px '+y+'px','--d':live+'ms','--t':T.loop+'ms'});rect(s,x-3,y-8.5,6,6,fill(HOT))}
if(R.live==='pulse'){const p=mk(svg,'g',{},'pulse');css(p,{'--d':live+'ms','--t':T.loop+'ms'});p.appendChild(m)}
// The stations, named.
R.stations.forEach((name,i)=>{const t=mk(svg,'text',{x:xs[i],y:H-8,'text-anchor':'middle'},i===at?'at':'');t.textContent=name});
const strip=d.createElement('div');strip.className='strip';strip.appendChild(svg);return strip}"#;

/// `nus://` commands that make the real thing happen to the page you're
/// on, so each page can be seen for real.
pub const DEBUG_URLS: [(&str, &str); 4] = [
    ("nus://crash", "crash this page's process"),
    ("nus://oom", "run this page out of memory"),
    ("nus://hang", "hang this page"),
    ("nus://block-download", "download a disguised program"),
];

/// What a `nus://` address asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Internal {
    Show(Page),
    Crash,
    Oom,
    Hang,
    BlockDownload,
}

pub fn internal(url: &str) -> Option<Internal> {
    let rest = url.strip_prefix("nus://")?.trim_end_matches('/');
    Some(match rest {
        "interstitials" | "interstitial" | "" => Internal::Show(Page::index()),
        "crash" => Internal::Crash,
        "oom" => Internal::Oom,
        "hang" => Internal::Hang,
        "block-download" => Internal::BlockDownload,
        _ => Internal::Show(Page::sample(Kind::from_slug(rest.strip_prefix("interstitial/")?)?)),
    })
}

impl PartialEq for Page {
    fn eq(&self, other: &Page) -> bool {
        self.token == other.token
    }
}

// ── Theme, fonts, escaping ───────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub paper: [f32; 4],
    pub ink: [f32; 4],
    pub dim: [f32; 4],
    pub signal: [f32; 4],
    pub dark: bool,
}

static COLORS: RwLock<Colors> = RwLock::new(Colors {
    paper: [1.0, 1.0, 1.0, 1.0],
    ink: [0.0784, 0.0784, 0.0784, 1.0],
    dim: [0.541, 0.522, 0.478, 1.0],
    signal: [0.784, 0.063, 0.180, 1.0],
    dark: false,
});

/// The window's look, for pages written from Chromium's callbacks.
pub fn set_colors(c: Colors) {
    *COLORS.write().unwrap_or_else(|e| e.into_inner()) = c;
}

fn css(c: [f32; 4]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("rgba({},{},{},{})", b(c[0]), b(c[1]), b(c[2]), c[3])
}

/// Plex Mono, the app's own, inlined: the page can't reach nus's files.
fn font_css() -> &'static str {
    static CSS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CSS.get_or_init(|| {
        let face = |bytes: &[u8], weight: u16| format!("@font-face{{font-family:\"nus mono\";font-weight:{weight};src:url(data:font/ttf;base64,{}) format(\"truetype\")}}", base64(bytes));
        face(nus_render::text::bundled::PLEX_MONO, 400) + &face(nus_render::text::bundled::PLEX_MONO_MEDIUM, 500)
    })
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn host(url: &str) -> String {
    let h = crate::sites::host_of(url);
    if h.is_empty() { url.to_string() } else { h }
}

pub(crate) fn is_local(url: &str) -> bool {
    let h = host(url);
    h.starts_with("localhost") || h.starts_with("127.") || h == "[::1]" || h.starts_with("0.0.0.0")
}

fn clock_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil(secs as i64 / 86400);
    format!("{y}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600 % 24, secs / 60 % 60)
}

/// Days since 1970-01-01 → (year, month, day).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

// ── What failed, a step at a time ────────────────────────────────────

/// The steps of a load, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Name,
    Connect,
    Secure,
    Request,
    Answer,
}

/// A load error, said: the step it failed at and what happened there.
struct Failure {
    stage: Stage,
    /// A trace of this one step alone, under this name: the failure isn't
    /// part of the load (you're offline; nus blocked the host).
    only: Option<&'static str>,
    head: String,
    body: String,
    what: String,
}

/// The port a URL goes to, written or implied by its scheme.
pub fn port_of(url: &str) -> u16 {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let port = match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || authority.starts_with('[') => p.parse().ok(),
        _ => None,
    };
    port.filter(|_| !authority.ends_with(']')).unwrap_or(if url.starts_with("https:") { 443 } else { 80 })
}

/// What this computer resolves a local host to, for the trace's first step.
/// Only for local names: those resolve from the hosts file, never the network.
fn local_addresses(url: &str) -> Option<String> {
    use std::net::ToSocketAddrs;
    if !is_local(url) {
        return None;
    }
    let host = host(url);
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let mut seen: Vec<String> = Vec::new();
    for a in (bare, port_of(url)).to_socket_addrs().ok()? {
        let ip = a.ip().to_string();
        if !seen.contains(&ip) {
            seen.push(ip);
        }
    }
    (!seen.is_empty() && seen != [bare.to_string()]).then(|| format!("{bare} → {}", seen.join(", ")))
}

fn failure(url: &str, code: &str) -> Failure {
    let host = host(url);
    let port = port_of(url);
    let f = |stage, head: String, body: String, what: String| Failure { stage, only: None, head, body, what };
    match code {
        "ERR_NAME_NOT_RESOLVED" | "ERR_NAME_RESOLUTION_FAILED" => f(Stage::Name, format!("DNS lookup failed · {host}"),
            format!("No address was returned for {host}."), "no address returned".into()),
        "ERR_INTERNET_DISCONNECTED" => Failure { only: Some("network"), ..f(Stage::Name, "No network connection".into(),
            "This device has no active network connection.".into(), "no active connection".into()) },
        "ERR_BLOCKED_BY_CLIENT" => Failure { only: Some("blocking"), ..f(Stage::Name, format!("Blocked by content blocker · {host}"),
            format!("{host} is on the ad and tracker block list. No request was sent. Blocking can be turned off per site in the site panel."),
            "on the block list".into()) },
        "ERR_CONNECTION_REFUSED" if is_local(url) => f(Stage::Connect, format!("Connection refused · {host}:{port}"),
            format!("No process is listening on port {port}."), "connection refused".into()),
        "ERR_CONNECTION_REFUSED" => f(Stage::Connect, format!("Connection refused · {host}:{port}"),
            format!("The host rejected the connection on port {port}."), "connection refused".into()),
        "ERR_CONNECTION_TIMED_OUT" => f(Stage::Connect, format!("Connection timed out · {host}:{port}"),
            "No response to the connection attempt. The host may be down, or a firewall may be dropping packets.".into(), "no response".into()),
        "ERR_ADDRESS_UNREACHABLE" => f(Stage::Connect, format!("No route to host · {host}"),
            format!("This network has no route to {host}. Check VPN and routing settings."), "no route".into()),
        "ERR_NETWORK_ACCESS_DENIED" => f(Stage::Connect, format!("Connection blocked on this device · {host}"),
            "Blocked by a firewall or by the system's network permission for nus.".into(), "blocked on this device".into()),
        "ERR_NETWORK_CHANGED" => f(Stage::Connect, "Network changed during load".into(),
            "The connection was interrupted by a network change.".into(), "interrupted by a network change".into()),
        "ERR_SSL_PROTOCOL_ERROR" => f(Stage::Secure, format!("TLS handshake failed · {host}"),
            "The server's TLS configuration is not supported.".into(), "handshake failed".into()),
        "ERR_TIMED_OUT" => f(Stage::Request, format!("Request timed out · {host}"),
            "Connected; no response arrived before the timeout.".into(), "no response before timeout".into()),
        "ERR_CONNECTION_RESET" | "ERR_CONNECTION_CLOSED" => f(Stage::Answer, format!("Connection reset · {host}"),
            "The connection closed before the response was complete.".into(), "connection reset".into()),
        "ERR_EMPTY_RESPONSE" => f(Stage::Answer, format!("Empty response · {host}"),
            "The server closed the connection without sending data.".into(), "no data".into()),
        "ERR_INVALID_RESPONSE" => f(Stage::Answer, format!("Invalid HTTP response · {host}"),
            "The response could not be parsed.".into(), "unparseable".into()),
        "ERR_TOO_MANY_REDIRECTS" => f(Stage::Answer, format!("Redirect loop · {host}"),
            format!("More than 20 redirects. Clearing cookies for {host} usually resolves this."), "more than 20 redirects".into()),
        "ERR_HTTP_RESPONSE_CODE_FAILURE" => f(Stage::Answer, format!("HTTP error, empty body · {host}"),
            "The server returned an error status with no content.".into(), "error status, empty body".into()),
        "ERR_INVALID_AUTH_CREDENTIALS" => f(Stage::Answer, format!("Authentication failed · {host}"),
            "The username or password was rejected.".into(), "credentials rejected".into()),
        _ => f(Stage::Request, format!("Load failed · {host}"), "The request failed before a response was received.".into(), "failed".into()),
    }
}

/// "2nd time" → "2nd time": capitalized for the start of a sentence.
fn capitalized(text: &str) -> String {
    let mut c = text.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The route of a failed load: the station its step names, and the break.
fn load_route(url: &str, code: &str, f: &Failure) -> Route {
    match f.only {
        Some("network") => return Route::new(url, "dns", Break::Cut),
        Some(_) => return Route::new(url, "nus", Break::Ban),
        None => {}
    }
    let station = match f.stage { Stage::Name => "dns", Stage::Connect => "connect", Stage::Secure => "tls", Stage::Request => "request", Stage::Answer => "response" };
    let mark = match code {
        "ERR_NAME_NOT_RESOLVED" | "ERR_NAME_RESOLUTION_FAILED" => Break::Unknown,
        "ERR_CONNECTION_REFUSED" | "ERR_INVALID_AUTH_CREDENTIALS" => Break::Wall,
        "ERR_NETWORK_ACCESS_DENIED" => Break::Ban,
        "ERR_NETWORK_CHANGED" | "ERR_CONNECTION_RESET" | "ERR_CONNECTION_CLOSED" => Break::Cut,
        "ERR_SSL_PROTOCOL_ERROR" => Break::Seal,
        "ERR_EMPTY_RESPONSE" | "ERR_INVALID_RESPONSE" | "ERR_HTTP_RESPONSE_CODE_FAILURE" => Break::Empty,
        "ERR_TOO_MANY_REDIRECTS" => Break::Loop,
        _ => Break::Gap,
    };
    Route::new(url, station, mark)
}

/// The steps of a failed load: those before the failure went through, the
/// failing one says what happened and Chromium's name for it, the rest
/// weren't reached.
fn load_trace(url: &str, code: &str, f: &Failure) -> Vec<Step> {
    let failed = || vec![f.what.clone(), code.to_string()];
    if let Some(name) = f.only {
        return vec![step(name, failed(), String::new(), Mark::Fail)];
    }
    let host = host(url);
    let port = port_of(url);
    let mut stages = vec![(Stage::Name, "dns"), (Stage::Connect, "connect")];
    if url.starts_with("https:") {
        stages.push((Stage::Secure, "tls"));
    }
    stages.extend([(Stage::Request, "request"), (Stage::Answer, "response")]);
    stages.into_iter().map(|(stage, name)| {
        if stage == f.stage {
            step(name, failed(), String::new(), Mark::Fail)
        } else if stage > f.stage {
            step(name, vec!["not reached".into()], String::new(), Mark::Skip)
        } else {
            let what = match stage {
                Stage::Name => local_addresses(url).unwrap_or_else(|| format!("{host} resolved")),
                Stage::Connect => format!("{host}:{port} connected"),
                Stage::Secure => "certificate valid".into(),
                Stage::Request => "sent".into(),
                Stage::Answer => "received".into(),
            };
            step(name, vec![what], String::new(), Mark::Ok)
        }
    }).collect()
}

/// 1st, 2nd, 3rd, 4th…
fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix} time")
}

/// Chromium's code for how a renderer ended, and what it means. `name` is
/// CEF's description when it has one (Windows: `STATUS_BREAKPOINT`); on
/// macOS and Linux a crash's code is the signal.
pub fn exit_words(name: &str, code: i32) -> Option<(String, String)> {
    let named = |n: &str| -> Option<&'static str> {
        Some(match n {
            "STATUS_ACCESS_VIOLATION" | "EXCEPTION_ACCESS_VIOLATION" => "bad memory access",
            "STATUS_BREAKPOINT" | "EXCEPTION_BREAKPOINT" => "failed internal check",
            "STATUS_STACK_BUFFER_OVERRUN" => "stack corruption",
            "STATUS_STACK_OVERFLOW" => "stack overflow",
            "STATUS_HEAP_CORRUPTION" => "heap corruption",
            "STATUS_OUT_OF_MEMORY" => "out of memory",
            "STATUS_INVALID_IMAGE_HASH" => "blocked by another program",
            _ => return None,
        })
    };
    if !name.is_empty() && name.parse::<i64>().is_err() {
        return Some((name.to_string(), named(name).unwrap_or("renderer error").to_string()));
    }
    if cfg!(windows) {
        return (code != 0).then(|| (format!("exit code {code:#x}"), "renderer error".into()));
    }
    let (sig, words) = match code {
        4 => ("SIGILL", "illegal instruction"),
        5 => ("SIGTRAP", "failed internal check"),
        6 => ("SIGABRT", "abort"),
        7 | 10 => ("SIGBUS", "bad memory access"),
        9 => ("SIGKILL", "external kill"),
        11 => ("SIGSEGV", "bad memory access"),
        _ if code != 0 => return Some((format!("exit code {code}"), "renderer error".into())),
        _ => return None,
    };
    Some((format!("{sig} ({code})"), words.into()))
}

// ── Deciding which page ──────────────────────────────────────────────

fn cert_words(code: &str) -> &'static str {
    match code {
        "NET::ERR_CERT_DATE_INVALID" => "certificate expired or not yet valid",
        "NET::ERR_CERT_AUTHORITY_INVALID" => "issuer not trusted",
        "NET::ERR_CERT_COMMON_NAME_INVALID" => "issued for a different host",
        "NET::ERR_CERT_REVOKED" => "certificate revoked",
        _ => "certificate could not be verified",
    }
}

/// Chromium's error number → its name, for the ones that get a page of
/// their own or a clearer line.
pub fn error_name(code: i32) -> String {
    match code {
        -105 => "ERR_NAME_NOT_RESOLVED".into(),
        -106 => "ERR_INTERNET_DISCONNECTED".into(),
        -102 => "ERR_CONNECTION_REFUSED".into(),
        -118 => "ERR_CONNECTION_TIMED_OUT".into(),
        -7 => "ERR_TIMED_OUT".into(),
        -101 => "ERR_CONNECTION_RESET".into(),
        -100 => "ERR_CONNECTION_CLOSED".into(),
        -109 => "ERR_ADDRESS_UNREACHABLE".into(),
        -20 => "ERR_BLOCKED_BY_CLIENT".into(),
        -21 => "ERR_NETWORK_CHANGED".into(),
        -310 => "ERR_TOO_MANY_REDIRECTS".into(),
        -324 => "ERR_EMPTY_RESPONSE".into(),
        -338 => "ERR_INVALID_AUTH_CREDENTIALS".into(),
        -320 => "ERR_INVALID_RESPONSE".into(),
        -137 => "ERR_NAME_RESOLUTION_FAILED".into(),
        -138 => "ERR_NETWORK_ACCESS_DENIED".into(),
        -104 => "ERR_CONNECTION_FAILED".into(),
        -108 => "ERR_ADDRESS_INVALID".into(),
        -27 => "ERR_BLOCKED_BY_RESPONSE".into(),
        -2 => "ERR_FAILED".into(),
        -379 => "ERR_HTTP_RESPONSE_CODE_FAILURE".into(),
        -301 => "ERR_DISALLOWED_URL_SCHEME".into(),
        -302 => "ERR_UNKNOWN_URL_SCHEME".into(),
        -400 => "ERR_CACHE_MISS".into(),
        -107 => "ERR_SSL_PROTOCOL_ERROR".into(),
        -200 => "NET::ERR_CERT_COMMON_NAME_INVALID".into(),
        -201 => "NET::ERR_CERT_DATE_INVALID".into(),
        -202 => "NET::ERR_CERT_AUTHORITY_INVALID".into(),
        -206 => "NET::ERR_CERT_REVOKED".into(),
        c if (-299..=-200).contains(&c) => format!("NET::ERR_CERT ({c})"),
        c => format!("ERR {c}"),
    }
}

/// The page for a main-frame load error. `now` and `built` are Unix
/// seconds: a date error while the clock reads before this build existed
/// (or years after) is the clock's fault.
pub fn for_error(url: &str, code: i32, can_back: bool, now: i64, built: i64) -> Page {
    let name = error_name(code);
    // Without a build date (built outside git), the clock can't be judged.
    if code == -201 && built > 0 {
        let behind = (built - now) / 86400;
        if behind >= 1 {
            return Page::clock(url, behind, can_back);
        }
        let ahead = (now - built) / 86400;
        if ahead > 3 * 365 {
            return Page::clock(url, -(ahead), can_back);
        }
    }
    if (-299..=-200).contains(&code) {
        return Page::cert(url, &name, can_back);
    }
    if code == -400 {
        return Page::resubmit(url, can_back);
    }
    Page::unreachable(url, &name, can_back)
}

/// Errors a captive portal could be behind: worth asking the network.
pub fn portal_suspect(code: i32) -> bool {
    matches!(code, -105 | -118 | -7 | -101 | -100 | -109 | -107 | -200 | -202)
}

/// When this build was made (its commit), Unix seconds.
pub fn built() -> i64 {
    option_env!("NUS_BUILD_EPOCH").and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// The plain-HTTP check this computer's own system uses for sign-in pages,
/// and what it answers when nothing is in the way.
fn portal_check() -> (&'static str, &'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("captive.apple.com", "/hotspot-detect.html", "Success")
    } else if cfg!(windows) {
        ("www.msftconnecttest.com", "/connecttest.txt", "Microsoft Connect Test")
    } else {
        ("nmcheck.gnome.org", "/check_network_status.txt", "NetworkManager is online")
    }
}

/// Ask the system's own plain-HTTP check whether this network intercepts
/// pages. Some(network) when it does. Blocks up to a few seconds; run it off
/// the UI thread.
pub fn probe_portal() -> Option<String> {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let (host_name, path, expected) = portal_check();
    let addr = (host_name, 80).to_socket_addrs().ok()?.next()?;
    let mut s = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok()?;
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: {host_name}\r\nUser-Agent: CaptiveNetworkSupport\r\n\r\n").as_bytes()).ok()?;
    let mut body = Vec::new();
    let _ = s.take(64 * 1024).read_to_end(&mut body);
    portal_in(&String::from_utf8_lossy(&body), expected)
}

/// Whether an answer to the sign-in check is a sign-in page. Only a redirect
/// or a page in place of the expected text is: a proxy's or a filter's
/// refusal (403, 407, 5xx) or nothing at all says the check was blocked, not
/// that the network wants a sign-in.
fn portal_in(text: &str, expected: &str) -> Option<String> {
    let status: u16 = text.lines().next()?.split_whitespace().nth(1)?.parse().ok()?;
    let to = text.lines().find_map(|l| l.strip_prefix("Location:").or_else(|| l.strip_prefix("location:"))).map(|l| host(l.trim()));
    let network = |to: Option<String>| Some(to.filter(|h| !h.is_empty()).unwrap_or_else(|| "this network".into()));
    match status {
        301 | 302 | 303 | 307 | 308 if to.is_some() => network(to),
        200 if !text.contains(expected) => network(None),
        _ => None,
    }
}

/// A camera or microphone macOS itself refuses nus (denied or restricted
/// in Privacy & Security), in words; None when it would let nus ask.
#[cfg(target_os = "macos")]
pub fn os_denied(audio: bool, video: bool) -> Option<&'static str> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: &'static AnyObject;
        static AVMediaTypeVideo: &'static AnyObject;
    }
    // AVAuthorizationStatus: 1 restricted, 2 denied.
    let refused = |t: &AnyObject| -> bool {
        let status: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: t] };
        matches!(status, 1 | 2)
    };
    let a = audio && refused(unsafe { AVMediaTypeAudio });
    let v = video && refused(unsafe { AVMediaTypeVideo });
    match (v, a) {
        (true, true) => Some("camera and microphone"),
        (true, false) => Some("camera"),
        (false, true) => Some("microphone"),
        _ => None,
    }
}
#[cfg(not(target_os = "macos"))]
pub fn os_denied(_audio: bool, _video: bool) -> Option<&'static str> {
    None
}

// ── Lists: hosts you've said are dangerous, and what you let through ──

static DANGEROUS: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
static ALLOWED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// profile/dangerous.txt: one host per line (`||host^` works too). nus
/// has no Safe Browsing service; this list is yours.
pub fn dangerous(url: &str) -> bool {
    let set = DANGEROUS.get_or_init(|| {
        let path = std::env::current_dir().unwrap_or_default().join("profile").join("dangerous.txt");
        std::fs::read_to_string(path).unwrap_or_default().lines().map(|l| l.trim().trim_start_matches("||").split(['^', '/', '$']).next().unwrap_or("").trim().to_lowercase()).filter(|h| !h.is_empty() && !h.starts_with(['#', '!'])).collect()
    });
    let h = host(url).to_lowercase();
    let mut part = h.as_str();
    loop {
        if set.contains(part) {
            return !allowed(&format!("site:{h}"));
        }
        match part.find('.') {
            Some(i) => part = &part[i + 1..],
            None => return false,
        }
    }
}

/// You went ahead: this session only.
pub fn allow(key: String) {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(key);
}

pub fn allowed(key: &str) -> bool {
    ALLOWED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.contains(key))
}

/// A download name that is a program in disguise, or a kind that is
/// only ever used to attack: Some(why).
pub fn dangerous_file(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let mut parts = lower.rsplit('.');
    let last = parts.next().unwrap_or("");
    let inner = parts.next().unwrap_or("");
    const PROGRAM: [&str; 14] = ["exe", "scr", "bat", "cmd", "com", "pif", "msi", "vbs", "vbe", "js", "jse", "wsf", "ps1", "command"];
    const DOCUMENT: [&str; 14] = ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "jpg", "jpeg", "png", "gif", "mp3", "mp4"];
    if PROGRAM.contains(&last) && DOCUMENT.contains(&inner) {
        return Some("executable disguised as a document");
    }
    if matches!(last, "scr" | "pif" | "vbe" | "jse") {
        return Some("file type used almost only in attacks");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_pick_their_page() {
        let built = 1_790_000_000;
        assert_eq!(for_error("https://a.test/", -201, true, built, built).kind, Kind::Cert);
        assert_eq!(for_error("https://a.test/", -201, true, built - 3 * 86400, built).kind, Kind::Clock);
        assert_eq!(for_error("https://a.test/", -202, true, built, built).kind, Kind::Cert);
        assert_eq!(for_error("https://a.test/", -400, true, built, built).kind, Kind::Resubmit);
        // Built outside git: no build date, so never blame the clock.
        assert_eq!(for_error("https://a.test/", -201, true, built, 0).kind, Kind::Cert);
        let p = for_error("http://localhost:3000/", -102, false, built, built);
        assert_eq!(p.kind, Kind::Unreachable);
        assert_eq!(p.head, "Connection refused · localhost:3000");
        assert_eq!(p.body, "No process is listening on port 3000.");
        let fail = p.trace.iter().find(|s| s.mark == Mark::Fail).unwrap();
        assert_eq!((fail.name.as_str(), fail.what[0].as_str()), ("connect", "connection refused"));
        assert_eq!(p.trace[0].mark, Mark::Ok);
        assert!(p.trace.iter().skip_while(|s| s.mark != Mark::Fail).skip(1).all(|s| s.mark == Mark::Skip));
        // Close isn't first here, so ↵ stays with Try again.
        assert_eq!(p.acts.iter().find(|a| a.verb == "close").map(|a| a.key), Some(""));
        assert_eq!(p.default_act().map(|a| a.key), Some("↵"));
    }

    #[test]
    fn an_empty_error_from_the_server_is_not_a_connection_failure() {
        let p = for_error("http://127.0.0.1:8000/empty500", -379, true, 1, 1);
        assert_eq!(p.head, "HTTP error, empty body · 127.0.0.1");
        let fail = p.trace.iter().find(|s| s.mark == Mark::Fail).unwrap();
        assert_eq!(fail.name, "response");
        assert_eq!(fail.what[1], "ERR_HTTP_RESPONSE_CODE_FAILURE");
        assert!(p.trace.iter().take_while(|s| s.mark != Mark::Fail).all(|s| s.mark == Mark::Ok));
    }

    #[test]
    fn trying_again_and_failing_the_same_way_keeps_the_page() {
        let mut first = Page::unreachable("http://localhost:5173/", "ERR_CONNECTION_REFUSED", false);
        first.last_on_port(None);
        let token = first.token.clone();
        let again = Page::unreachable("http://localhost:5173/", "ERR_CONNECTION_REFUSED", false);
        assert!(first.same_failure(&again));
        first.tried_again(&again);
        first.tried_again(&again);
        assert_eq!(first.token, token, "the page's commands keep working");
        assert!(first.acts.iter().any(|a| a.verb == "watch"));
        assert!(first.trace.iter().any(|s| s.what.last().is_some_and(|l| l == "tried 3 times")));
        assert!(!first.same_failure(&Page::unreachable("http://localhost:5173/", "ERR_EMPTY_RESPONSE", false)));
    }

    #[test]
    fn the_route_breaks_where_the_trace_does() {
        let at = |p: &Page| { let r = p.route.as_ref().unwrap(); (r.stations[r.at], r.mark) };
        let refused = for_error("http://localhost:5173/", -102, false, 1, 1);
        assert_eq!(refused.route.as_ref().unwrap().stations, ["nus", "dns", "connect", "request", "response", "page"], "no tls over http");
        assert_eq!(at(&refused), ("connect", Break::Wall));
        assert_eq!(at(&for_error("https://a.test/", -105, true, 1, 1)), ("dns", Break::Unknown));
        assert_eq!(at(&for_error("https://a.test/", -202, true, 1, 1)), ("tls", Break::Seal));
        assert_eq!(at(&for_error("https://a.test/", -310, true, 1, 1)), ("response", Break::Loop));
        assert_eq!(at(&for_error("https://a.test/", -379, true, 1, 1)), ("response", Break::Empty));
        assert_eq!(at(&for_error("https://a.test/", -106, true, 1, 1)), ("dns", Break::Cut));
        assert_eq!(at(&for_error("https://a.test/", -20, true, 1, 1)), ("nus", Break::Ban));
        assert_eq!(at(&Page::sample(Kind::Hung)), ("page", Break::Stuck));
        assert_eq!(Page::sample(Kind::Hung).route.unwrap().live, Live::Spinning);
        assert_eq!(Page::sample(Kind::Slow).route.unwrap().live, Live::InFlight);
        assert_eq!(at(&Page::sample(Kind::Crash)), ("page", Break::Split));
        let mut watched = refused.clone();
        watched.watching(None);
        assert_eq!(watched.route.unwrap().live, Live::Polling, "a watched port is tried until it answers");
        // Every page that can fail has a route; a page's own dialog has none.
        for k in Kind::ALL {
            let has = Page::sample(k).route.is_some();
            assert_eq!(has, !matches!(k, Kind::Dialog | Kind::Index), "{k:?}");
        }
        let h = refused.script();
        assert!(h.contains("\"mark\":\"wall\"") && h.contains("\"stations\":[\"nus\""));
    }

    #[test]
    fn steps_and_times_read_plainly() {
        let p = for_error("https://secure.test/", -201, true, 1, 1);
        assert_eq!(p.trace.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["dns", "connect", "tls", "request", "response"]);
        assert_eq!(p.trace[2].mark, Mark::Fail);
        assert_eq!(port_of("http://localhost:5173/x"), 5173);
        assert_eq!(port_of("https://a.test/"), 443);
        assert_eq!(port_of("http://[::1]/"), 80);
        assert_eq!(port_of("http://[::1]:9000/"), 9000);
        assert_eq!(took(std::time::Duration::from_micros(400)), "0.4 ms");
        assert_eq!(took(std::time::Duration::from_millis(1200)), "1.2 s");
        assert_eq!(took(std::time::Duration::from_secs(12)), "12 s");
        assert_eq!(took(std::time::Duration::from_secs(42 * 60)), "42 min");
        assert_eq!(ordinal(2), "2nd time");
        assert_eq!(ordinal(12), "12th time");
    }

    #[test]
    fn only_a_real_sign_in_page_is_a_portal() {
        let ok = "HTTP/1.0 200 OK\r\n\r\n<HTML><BODY>Success</BODY></HTML>";
        assert_eq!(portal_in(ok, "Success"), None);
        assert_eq!(portal_in("HTTP/1.1 302 Found\r\nLocation: https://wifi.cafe.example/login\r\n\r\n", "Success").as_deref(), Some("wifi.cafe.example"));
        assert_eq!(portal_in("HTTP/1.1 200 OK\r\n\r\n<html><form>Accept the terms</form></html>", "Success").as_deref(), Some("this network"));
        // A proxy or a filter refusing the check is not a sign-in.
        assert_eq!(portal_in("HTTP/1.1 403 Forbidden\r\n\r\nBlocked by policy", "Success"), None);
        assert_eq!(portal_in("HTTP/1.1 407 Proxy Authentication Required\r\n\r\n", "Success"), None);
        assert_eq!(portal_in("", "Success"), None);
    }

    #[test]
    fn a_crash_says_how_it_ended_and_what_went_with_it() {
        if !cfg!(windows) {
            assert_eq!(exit_words("", 11), Some(("SIGSEGV (11)".into(), "bad memory access".into())));
        }
        assert_eq!(exit_words("STATUS_BREAKPOINT", 0).unwrap().1, "failed internal check");
        let p = Page::sample(Kind::Crash);
        assert_eq!(p.head, "Renderer crashed after 42 min");
        assert_eq!(p.body, "Exit: bad memory access. Other tabs are unaffected. 2nd time on this site this session.");
        assert_eq!(p.default_act().unwrap().verb, "retry-all");
        assert!(p.trace.iter().any(|s| s.name == "same process"));
        let mine = Page::crashed("https://a.test/", &Ended { yours: true, ..Ended::default() });
        assert_eq!(mine.head, "Page terminated");
        assert!(!mine.acts.iter().any(|a| a.verb == "details"));
    }

    #[test]
    fn going_ahead_is_never_the_default() {
        for k in Kind::ALL {
            let p = Page::sample(k);
            if let Some(a) = p.default_act() {
                assert!(!a.unsafe_, "{k:?}");
            }
            let first_unsafe = p.acts.iter().position(|a| a.unsafe_);
            if let Some(i) = first_unsafe {
                assert!(p.acts[i..].iter().all(|a| a.unsafe_), "{k:?}: unsafe commands come last");
            }
        }
    }

    #[test]
    fn internal_addresses() {
        assert!(matches!(internal("nus://interstitials"), Some(Internal::Show(p)) if p.kind == Kind::Index));
        assert!(matches!(internal("nus://interstitial/cert"), Some(Internal::Show(p)) if p.kind == Kind::Cert));
        assert_eq!(internal("nus://crash"), Some(Internal::Crash));
        assert_eq!(internal("nus://interstitial/nope"), None);
        assert_eq!(internal("https://nus.dev"), None);
    }

    #[test]
    fn disguised_programs() {
        assert!(dangerous_file("invoice.pdf.exe").is_some());
        assert!(dangerous_file("Invoice.PDF.EXE").is_some());
        assert!(dangerous_file("screensaver.scr").is_some());
        assert!(dangerous_file("setup.exe").is_none());
        assert!(dangerous_file("report.pdf").is_none());
        assert!(dangerous_file("nus.dmg").is_none());
    }

    #[test]
    fn html_carries_the_token_and_escapes() {
        let mut p = Page::unreachable("https://a.test/<script>", "ERR_NAME_NOT_RESOLVED", true);
        p.token = "tok".into();
        let h = p.script();
        assert!(h.contains("\"tok\""));
        assert!(h.contains("\"trace\":[{"), "the trace goes to the page as data");
        // Text goes in as JSON data, set with textContent: never as markup.
        assert!(h.contains("https://a.test/<script>"));
        assert!(!h.contains("innerHTML") && !h.contains("document.write"));
        assert_eq!(base64(b"nus"), "bnVz");
        assert_eq!(base64(b"nu"), "bnU=");
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20720), (2026, 9, 24));
    }
}
