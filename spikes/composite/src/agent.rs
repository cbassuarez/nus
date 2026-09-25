//! Assistants in the shell — what claude, codex and the rest are doing in
//! each pane, for the Ledger: the lines under a tab in the sidebar (working
//! 4:12 · edit term.rs; waiting · run cargo test? ALLOW DENY ALWAYS; done ·
//! 3 files) and the strip's "1 waiting · 2 working".
//!
//! What a pane's assistant is doing comes from three places, strongest
//! first:
//!
//! 1. Its own hooks. `nus hook claude` (crates/cli) runs from the
//!    assistant's hook config, and forwards the event here with the pane
//!    it came from (`NUS_PANE`, else its process's ancestry). Once a pane
//!    has heard from hooks, guesses no longer move it.
//! 2. The terminal. A notification escape (OSC 9 / 777) or a bell from a
//!    pane running an assistant means it is waiting, and the words say why.
//! 3. The command line. A known assistant running in the shell is working.
//!
//! The sidebar's answers are keystrokes into the pane — what you would
//! press at the assistant's own prompt — so the terminal and the sidebar
//! are never out of step: answer in either, and the other follows.

use std::time::Instant;

use serde_json::Value;

/// A new pane's name for `nus hook`: unique on this machine for a long
/// while — the process, the time, a counter.
pub fn mint_pane() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    format!("p{:x}-{:x}-{n:x}", std::process::id(), t)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Between turns: nothing to show.
    Idle,
    /// A turn is running.
    Working,
    /// It needs you: a permission, or a question.
    Waiting,
    /// The turn finished.
    Done,
}

/// A permission prompt the sidebar can answer: the tool and what it would do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub tool: String,
    pub what: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Always,
    Deny,
}

#[derive(Clone, Debug)]
pub struct Agent {
    /// "claude", "codex", … — the program, as `cutoff::program` names it.
    pub name: String,
    pub phase: Phase,
    /// When the phase began: waiting 0:04, done 12m.
    pub since: Instant,
    /// When the turn began: working 4:12.
    pub turn: Option<Instant>,
    /// The tool at work, in words: "edit vt/src/term.rs".
    pub doing: Option<String>,
    /// Why it is waiting, in its own words.
    pub reason: Option<String>,
    /// The permission prompt on screen, when the sidebar can answer it.
    pub ask: Option<Ask>,
    /// The last tool it was about to use: a permission ask names it.
    pub pending: Option<Ask>,
    pub session: Option<String>,
    /// Files it changed this turn.
    pub touched: Vec<String>,
    /// Hooks report for this pane.
    pub hooked: bool,
}

impl Agent {
    pub fn new(name: &str, now: Instant) -> Agent {
        Agent { name: name.into(), phase: Phase::Working, since: now, turn: Some(now), doing: None, reason: None, ask: None, pending: None, session: None, touched: Vec::new(), hooked: false }
    }

    fn set(&mut self, phase: Phase, now: Instant) {
        if self.phase != phase {
            self.phase = phase;
            self.since = now;
        }
        if phase != Phase::Waiting {
            self.ask = None;
            self.reason = None;
        }
    }

    /// The Ledger's second line: the question, or the tool at work.
    pub fn line(&self) -> Option<String> {
        match self.phase {
            Phase::Waiting => self.ask.as_ref().map(question).or_else(|| self.reason.clone()),
            Phase::Working => self.doing.clone(),
            Phase::Done => self.reason.clone(),
            Phase::Idle => None,
        }
    }
}

/// Keep what comes over the wire short and single-line.
fn clip(s: &str, n: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n - 1).collect::<String>())
    } else {
        one
    }
}

/// A path as the sidebar has room for: its last two parts.
fn tail(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    match parts.len() {
        0 => String::new(),
        1 => parts[0].into(),
        n => format!("{}/{}", parts[n - 2], parts[n - 1]),
    }
}

/// Tools that change files, by the names Claude Code gives them.
fn edits(tool: &str) -> bool {
    matches!(tool, "Edit" | "MultiEdit" | "Write" | "NotebookEdit")
}

/// What a tool call does, in a few words: "run cargo test", "edit a/b.rs".
fn describe(ev: &Value) -> Option<Ask> {
    let tool = ev.get("tool").and_then(Value::as_str)?.to_string();
    let s = |k: &str| ev.get(k).and_then(Value::as_str).filter(|v| !v.is_empty());
    let what = match tool.as_str() {
        "Bash" => s("command").map(|c| c.lines().next().unwrap_or(c).to_string()).unwrap_or_default(),
        t if edits(t) || t == "Read" => s("file").map(tail).unwrap_or_default(),
        "Grep" | "Glob" => s("pattern").unwrap_or_default().to_string(),
        "WebFetch" | "WebSearch" => s("url").or(s("query")).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    Some(Ask { tool, what: clip(&what, 120) })
}

/// The verb for a tool, lower case, as the Ledger says it.
fn verb(tool: &str) -> String {
    match tool {
        "Bash" => "run".into(),
        t if edits(t) => "edit".into(),
        "Read" => "read".into(),
        "Grep" | "Glob" => "search".into(),
        "WebFetch" => "fetch".into(),
        "WebSearch" => "search the web for".into(),
        "Task" => "start a subagent".into(),
        t => t.to_lowercase(),
    }
}

fn doing(ask: &Ask) -> String {
    if ask.what.is_empty() { verb(&ask.tool) } else { format!("{} {}", verb(&ask.tool), ask.what) }
}

/// A permission, as a question: "run cargo test --workspace?"
pub fn question(ask: &Ask) -> String {
    format!("{}?", doing(ask))
}

/// One event from `nus hook`, onto a pane's assistant. `name` is the
/// agent that sent it ("claude", "codex").
pub fn apply(slot: &mut Option<Agent>, name: &str, ev: &Value, now: Instant) {
    let event = ev.get("event").and_then(Value::as_str).unwrap_or("");
    if event == "SessionEnd" {
        *slot = None;
        return;
    }
    let a = slot.get_or_insert_with(|| {
        let mut a = Agent::new(name, now);
        a.set(Phase::Idle, now);
        a
    });
    a.hooked = true;
    a.name = name.into();
    if let Some(id) = ev.get("session").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        a.session = Some(clip(id, 80));
    }
    let text = |k: &str| ev.get(k).and_then(Value::as_str).map(|m| clip(m, 200)).filter(|m| !m.is_empty());
    match event {
        "SessionStart" => a.set(Phase::Idle, now),
        "UserPromptSubmit" => {
            a.set(Phase::Working, now);
            a.turn = Some(now);
            a.touched.clear();
            a.doing = None;
            a.pending = None;
        }
        "PreToolUse" => {
            if a.phase != Phase::Working {
                a.set(Phase::Working, now);
            }
            a.turn.get_or_insert(now);
            if let Some(ask) = describe(ev) {
                a.doing = Some(doing(&ask));
                a.pending = Some(ask);
            }
        }
        "PostToolUse" => {
            // A tool ran: whatever was asked has been answered.
            if a.phase != Phase::Working {
                a.set(Phase::Working, now);
            }
            let tool = ev.get("tool").and_then(Value::as_str).unwrap_or("");
            if edits(tool) {
                if let Some(f) = ev.get("file").and_then(Value::as_str).filter(|f| !f.is_empty()) {
                    if !a.touched.iter().any(|t| t == f) && a.touched.len() < 200 {
                        a.touched.push(f.to_string());
                    }
                }
            }
            a.pending = None;
        }
        "Notification" => {
            let message = text("message");
            let permission = message.as_deref().is_some_and(|m| m.to_lowercase().contains("permission"));
            // "Waiting for your input" after a finished turn is not news.
            if !permission && matches!(a.phase, Phase::Done | Phase::Idle) {
                return;
            }
            a.set(Phase::Waiting, now);
            a.reason = message;
            a.ask = if permission { a.pending.clone() } else { None };
        }
        "Stop" => {
            a.set(Phase::Done, now);
            a.doing = None;
            a.pending = None;
        }
        // Codex's notify program: one event, when a turn completes.
        "agent-turn-complete" => {
            a.set(Phase::Done, now);
            a.doing = None;
            a.reason = text("message");
        }
        _ => {}
    }
}

/// A notification escape or a bell from a pane whose assistant nus knows
/// only by its command line: it is waiting, and the words say why.
pub fn notified(slot: &mut Option<Agent>, body: Option<&str>, now: Instant) {
    let Some(a) = slot.as_mut() else { return };
    if a.hooked {
        return; // hooks say it better, and say it too
    }
    a.set(Phase::Waiting, now);
    a.reason = body.map(|b| clip(b, 200)).filter(|b| !b.is_empty());
}

/// What the shell says is running, when its marks change: a known
/// assistant running is working; the shell back at its prompt ends it.
/// Without shell integration the marks never move, and neither does this.
pub fn observe(slot: &mut Option<Agent>, program: &str, at_prompt: bool, now: Instant) {
    let assistant = crate::cutoff::is_assistant(program);
    match slot {
        None if assistant => *slot = Some(Agent::new(program, now)),
        Some(_) if at_prompt => *slot = None,
        Some(a) if !a.hooked && !assistant => *slot = None,
        Some(a) if assistant && !a.hooked && a.name != program => *slot = Some(Agent::new(program, now)),
        _ => {}
    }
}

/// Keys typed into the pane: an answer at the assistant's own prompt.
pub fn typed(slot: &mut Option<Agent>, bytes: &[u8], now: Instant) {
    let Some(a) = slot.as_mut() else { return };
    if a.phase != Phase::Waiting {
        return;
    }
    // Esc is no; anything else goes on (the next hook corrects it).
    let next = if bytes == b"\x1b" { Phase::Idle } else { Phase::Working };
    a.set(next, now);
}

/// The keys that answer an assistant's permission prompt, where nus knows
/// them. Claude Code: 1 yes, 2 yes and don't ask again, Esc no.
pub fn keys(agent: &str, answer: Answer) -> Option<&'static [u8]> {
    match (agent, answer) {
        ("claude", Answer::Allow) => Some(b"1"),
        ("claude", Answer::Always) => Some(b"2"),
        ("claude", Answer::Deny) => Some(b"\x1b"),
        _ => None,
    }
}

/// After the sidebar answered: what the assistant does next.
pub fn answered(slot: &mut Option<Agent>, answer: Answer, now: Instant) {
    let Some(a) = slot.as_mut() else { return };
    a.set(if answer == Answer::Deny { Phase::Idle } else { Phase::Working }, now);
    a.pending = None;
}

/// "4:12", "0:04", "12m", "3h" — as the Ledger counts.
pub fn clock(secs: u64) -> String {
    if secs < 3600 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

pub fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

// ── The app's side: finding the pane, taking the event, answering ─────

use crate::app::{App, Pane};

/// A term pane of a window: (tab index, right half?).
type At = (usize, bool);

impl App {
    fn term_panes(&self) -> impl Iterator<Item = (At, &crate::app::TermPane)> {
        self.tabs.iter().enumerate().flat_map(|(i, tab)| {
            [(false, Some(&tab.left)), (true, tab.right.as_ref())].into_iter().filter_map(move |(right, p)| match p {
                Some(Pane::Term(t)) => Some(((i, right), t)),
                _ => None,
            })
        })
    }

    pub(crate) fn term_at_mut(&mut self, (i, right): At) -> Option<&mut crate::app::TermPane> {
        let tab = self.tabs.get_mut(i)?;
        match if right { tab.right.as_mut() } else { Some(&mut tab.left) } {
            Some(Pane::Term(t)) => Some(t),
            _ => None,
        }
    }

    /// The pane named NUS_PANE=`uid`, in this window.
    pub(crate) fn pane_by_uid(&self, uid: &str) -> Option<At> {
        self.term_panes().find(|(_, t)| t.pane_uid == uid).map(|(at, _)| at)
    }

    /// The pane whose shell `pid` descends from, in this window: for a
    /// shell whose NUS_PANE nus no longer knows (a held shell reattached
    /// from the atlas), or a hook run without it.
    fn pane_by_pid(&self, tree: &std::collections::HashMap<u32, (u32, String)>, pid: u32) -> Option<At> {
        let roots: Vec<(u32, At)> = self.term_panes().filter_map(|(at, t)| t.pty.pid().map(|p| (p, at))).collect();
        let pids: Vec<u32> = roots.iter().map(|r| r.0).collect();
        let root = nus_pty::ports::ancestor_in(tree, pid, &pids)?;
        roots.iter().find(|r| r.0 == root).map(|r| r.1)
    }

    /// One event from `nus hook`, onto the pane it came from.
    fn agent_event(&mut self, at: At, args: &Value) {
        let name = args.get("agent").and_then(Value::as_str).map(crate::cutoff::program).filter(|n| !n.is_empty() && n.len() <= 24).unwrap_or_else(|| "assistant".into());
        let looked_at = at.0 == self.active && self.window.has_focus() && !self.hatch_state.main_hidden;
        let now = crate::clock::now();
        let Some(t) = self.term_at_mut(at) else { return };
        // The shell's own name for itself wins: learn it.
        if let Some(uid) = args.get("pane").and_then(Value::as_str).filter(|u| !u.is_empty() && u.len() <= 64) {
            t.pane_uid = uid.to_string();
        }
        let before = t.agent.as_ref().map(|a| a.phase);
        apply(&mut t.agent, &name, args, now);
        let after = t.agent.as_ref().map(|a| a.phase);
        let news = before != after && matches!(after, Some(Phase::Waiting | Phase::Done));
        if news && !looked_at {
            t.waiting = true;
        }
        if news && !looked_at {
            self.play_event("bell");
        }
        self.dirty = true;
    }

    /// The sidebar answered a permission: the keys the assistant's prompt
    /// takes, into its pane. False when there is nothing to answer.
    pub(crate) fn answer_agent(&mut self, tab: usize, answer: Answer) -> bool {
        let now = crate::clock::now();
        for right in [false, true] {
            let Some(t) = self.term_at_mut((tab, right)) else { continue };
            let Some(a) = t.agent.as_ref().filter(|a| a.phase == Phase::Waiting && a.ask.is_some()) else { continue };
            let Some(bytes) = keys(&a.name, answer) else { continue };
            let _ = t.pty.write(bytes);
            answered(&mut t.agent, answer, now);
            t.waiting = false;
            t.notice = None;
            self.play_event("toggle");
            self.dirty = true;
            return true;
        }
        false
    }
}

/// A hook event, to the window that has its pane: by NUS_PANE first, then
/// by the hook's process ancestry (one reading of the process table for
/// all windows). Neither: the event is dropped — no pane, nothing to say.
pub fn route(apps: &mut [App], args: &Value) -> Result<Value, String> {
    if let Some(uid) = args.get("pane").and_then(Value::as_str).filter(|u| !u.is_empty()) {
        for a in apps.iter_mut() {
            if let Some(at) = a.pane_by_uid(uid) {
                a.agent_event(at, args);
                return Ok(Value::Null);
            }
        }
    }
    if let Some(pid) = args.get("pid").and_then(Value::as_u64) {
        let tree = nus_pty::ports::process_tree();
        for a in apps.iter_mut() {
            if let Some(at) = a.pane_by_pid(&tree, pid as u32) {
                a.agent_event(at, args);
                return Ok(Value::Null);
            }
        }
    }
    Err("not from a nus pane".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn a_claude_turn_through_its_hooks() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut slot = None;
        apply(&mut slot, "claude", &json!({"event": "SessionStart", "session": "abc"}), at(0));
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Idle);
        apply(&mut slot, "claude", &json!({"event": "UserPromptSubmit"}), at(1));
        apply(&mut slot, "claude", &json!({"event": "PreToolUse", "tool": "Edit", "file": "/x/crates/vt/src/term.rs"}), at(2));
        assert_eq!(slot.as_ref().unwrap().line().as_deref(), Some("edit src/term.rs"));
        apply(&mut slot, "claude", &json!({"event": "PostToolUse", "tool": "Edit", "file": "/x/crates/vt/src/term.rs"}), at(3));
        apply(&mut slot, "claude", &json!({"event": "PreToolUse", "tool": "Bash", "command": "cargo test --workspace\necho"}), at(4));
        apply(&mut slot, "claude", &json!({"event": "Notification", "message": "Claude needs your permission to use Bash"}), at(5));
        let a = slot.as_ref().unwrap();
        assert_eq!(a.phase, Phase::Waiting);
        assert_eq!(a.line().as_deref(), Some("run cargo test --workspace?"));
        assert_eq!(a.session.as_deref(), Some("abc"));
        answered(&mut slot, Answer::Allow, at(6));
        apply(&mut slot, "claude", &json!({"event": "PostToolUse", "tool": "Bash"}), at(9));
        apply(&mut slot, "claude", &json!({"event": "Stop"}), at(10));
        let a = slot.as_ref().unwrap();
        assert_eq!(a.phase, Phase::Done);
        assert_eq!(a.touched, vec!["/x/crates/vt/src/term.rs".to_string()]);
        // The idle reminder after a finished turn changes nothing.
        apply(&mut slot, "claude", &json!({"event": "Notification", "message": "Claude is waiting for your input"}), at(70));
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Done);
        apply(&mut slot, "claude", &json!({"event": "SessionEnd"}), at(80));
        assert!(slot.is_none());
    }

    #[test]
    fn guesses_from_the_shell_and_the_terminal() {
        let now = Instant::now();
        let mut slot = None;
        observe(&mut slot, "claude", false, now);
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Working);
        notified(&mut slot, Some("Claude needs your permission"), now);
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Waiting);
        // An answer typed at the terminal's own prompt moves it on.
        typed(&mut slot, b"1", now);
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Working);
        observe(&mut slot, "", true, now);
        assert!(slot.is_none());
        observe(&mut slot, "nvim", false, now);
        assert!(slot.is_none());
    }

    #[test]
    fn hooks_outrank_guesses() {
        let now = Instant::now();
        let mut slot = None;
        apply(&mut slot, "claude", &json!({"event": "UserPromptSubmit"}), now);
        notified(&mut slot, Some("beep"), now);
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Working);
        // Esc at the prompt is a no.
        apply(&mut slot, "claude", &json!({"event": "Notification", "message": "needs your permission"}), now);
        typed(&mut slot, b"\x1b", now);
        assert_eq!(slot.as_ref().unwrap().phase, Phase::Idle);
        // A hooked assistant outlives a wrapper name; the prompt ends it.
        observe(&mut slot, "node", false, now);
        assert!(slot.is_some());
        observe(&mut slot, "", true, now);
        assert!(slot.is_none());
    }

    #[test]
    fn codex_says_when_a_turn_is_done() {
        let now = Instant::now();
        let mut slot = None;
        observe(&mut slot, "codex", false, now);
        apply(&mut slot, "codex", &json!({"event": "agent-turn-complete", "message": "Added OSC 9 parsing."}), now);
        let a = slot.as_ref().unwrap();
        assert_eq!(a.phase, Phase::Done);
        assert_eq!(a.line().as_deref(), Some("Added OSC 9 parsing."));
        assert!(keys("codex", Answer::Allow).is_none());
        assert_eq!(keys("claude", Answer::Deny), Some(&b"\x1b"[..]));
    }

    #[test]
    fn counting() {
        assert_eq!(clock(252), "4:12");
        assert_eq!(clock(4), "0:04");
        assert_eq!(ago(720), "12m");
        assert_eq!(clip("a\nb   c", 10), "a b c");
        assert_eq!(clip("abcdefghijk", 5), "abcd…");
    }
}
