//! Ask: a small panel beside a shell for the moment you feel dumb. One
//! line in ("how do I …"), a few blocks out — each a command to INSERT
//! at the prompt, RUN, or COPY — with the shell, cwd and the last
//! command's output sent along so the answer fits. It isn't a chat: the
//! last few turns stay for scrolling, nothing more. Ctrl+Shift+? (or
//! Ctrl+Shift+A where nothing else has it), or the palette.
//!
//! Backends are whatever is on the machine, in this order: `claude -p`,
//! `codex exec`, the Copilot CLI (`gh copilot -p`, only if it's already
//! installed), `ollama run`, and the Anthropic API through curl when
//! ANTHROPIC_API_KEY is set. NUS_ASK_CMD overrides with any command that
//! reads the prompt on stdin and prints markdown.

use crate::app::{IconMotion, Caps, App, Pane, TermPane};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style};
use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

pub const PANEL_W: f32 = 340.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Prose(String),
    Code { lang: String, text: String },
}

#[derive(Clone, Debug)]
pub struct Turn {
    pub q: String,
    pub blocks: Vec<Block>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AskHit {
    Field,
    Close,
    Insert(usize, usize),
    Run(usize, usize),
    Copy(usize, usize),
    /// A context chip: toggles what goes along.
    Ctx(crate::askctx::Ctx),
    /// A skill chip: its prompt, sent with its context.
    Skill(usize),
    /// REMEMBER on a turn: the answer's first line goes to memory.
    Remember(usize),
}

pub struct Ask {
    pub input: String,
    pub focus: bool,
    pub turns: Vec<Turn>,
    pub pending: Option<(Receiver<Result<String, String>>, Instant, String)>,
    pub scroll: f32,
    pub hits: Vec<(Rect, AskHit)>,
    pub rect: Rect,
    /// Copied block, for the moment's CHIP feedback.
    pub copied: Option<(usize, usize, Instant)>,
    /// What goes along with a question.
    pub ctx: Vec<crate::askctx::Ctx>,
    /// A question waiting on the page's text: the eval id, title, url,
    /// the question, the gathered rest, when it was asked.
    pub gathering: Option<(i32, String, String, String, crate::askctx::Gathered, Instant, Option<String>)>,
    /// Turns that went to memory.
    pub remembered: Vec<usize>,
}

impl Ask {
    pub fn new() -> Ask {
        Ask::with_ctx(vec![crate::askctx::Ctx::Shell, crate::askctx::Ctx::Block, crate::askctx::Ctx::Page])
    }

    /// A panel with the settings' default chips.
    pub fn from_keys(keys: &[String]) -> Ask {
        let ctx: Vec<crate::askctx::Ctx> = crate::askctx::Ctx::ALL.iter().copied().filter(|c| keys.iter().any(|k| k == c.key())).collect();
        Ask::with_ctx(ctx)
    }

    pub fn with_ctx(ctx: Vec<crate::askctx::Ctx>) -> Ask {
        Ask { input: String::new(), focus: true, turns: Vec::new(), pending: None, scroll: 0.0, hits: Vec::new(), rect: Rect::new(0.0, 0.0, 0.0, 0.0), copied: None, ctx, gathering: None, remembered: Vec::new() }
    }
}

/// A backend: its name and how to run a prompt through it.
#[derive(Clone, Debug, PartialEq)]
pub struct Backend {
    pub name: String,
    pub how: String,
}

fn on_path(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else { return false };
    let names: Vec<String> = if cfg!(windows) { vec![format!("{bin}.exe"), format!("{bin}.cmd"), format!("{bin}.bat"), bin.to_string()] } else { vec![bin.to_string()] };
    std::env::split_paths(&path).any(|d| names.iter().any(|n| d.join(n).is_file()))
}

fn copilot_installed() -> bool {
    if on_path("copilot") {
        return true;
    }
    let base = std::env::var("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_default();
    base.join("GitHub CLI").join("copilot").exists()
}

/// The backends this machine has, best first.
pub fn backends() -> Vec<Backend> {
    let mut v = Vec::new();
    if let Ok(cmd) = std::env::var("NUS_ASK_CMD") {
        v.push(Backend { name: "custom".into(), how: cmd });
    }
    if on_path("claude") {
        v.push(Backend { name: "claude".into(), how: "claude -p --output-format text".into() });
    }
    if on_path("codex") {
        v.push(Backend { name: "codex".into(), how: "codex exec -".into() });
    }
    if on_path("gh") && copilot_installed() {
        v.push(Backend { name: "copilot".into(), how: "gh copilot -p".into() });
    }
    if on_path("ollama") {
        v.push(Backend { name: "ollama".into(), how: "ollama run llama3.2".into() });
    }
    if std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.is_empty()) && on_path("curl") {
        v.push(Backend { name: "anthropic api".into(), how: "curl · claude-sonnet-5".into() });
    }
    v
}

fn no_window(c: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
}

/// Run one prompt through a backend, blocking; called on a worker thread.
fn run(backend: &Backend, prompt: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut c = match backend.name.as_str() {
        "custom" => {
            let mut c = if cfg!(windows) { Command::new("cmd") } else { Command::new("sh") };
            c.args(if cfg!(windows) { vec!["/C", &backend.how] } else { vec!["-c", &backend.how] });
            c
        }
        "claude" => {
            let mut c = Command::new("claude");
            c.args(["-p", "--output-format", "text"]);
            c
        }
        "codex" => {
            let mut c = Command::new("codex");
            c.args(["exec", "-"]);
            c
        }
        "copilot" => {
            let mut c = Command::new("gh");
            c.args(["copilot", "-p", prompt]);
            c
        }
        "ollama" => {
            let mut c = Command::new("ollama");
            c.args(["run", "llama3.2"]);
            c
        }
        "anthropic api" => {
            let key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
            let body = serde_json::json!({
                "model": "claude-sonnet-5",
                "max_tokens": 700,
                "messages": [{ "role": "user", "content": prompt }]
            })
            .to_string();
            let mut c = Command::new("curl");
            c.args(["-s", "https://api.anthropic.com/v1/messages", "-H", "content-type: application/json", "-H", "anthropic-version: 2023-06-01", "-H", &format!("x-api-key: {key}"), "--data-binary", "@-"]);
            no_window(&mut c);
            c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
            let mut child = c.spawn().map_err(|e| e.to_string())?;
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(body.as_bytes());
            }
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| crate::surface::first_line(text.trim()))?;
            if let Some(e) = v.get("error") {
                return Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("api error").to_string());
            }
            let answer: String = v.get("content").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            return Ok(answer);
        }
        _ => return Err("no backend".into()),
    };
    no_window(&mut c);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("{}: {e}", backend.name))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(prompt.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() && text.is_empty() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(crate::surface::first_line(e.trim()));
    }
    Ok(text)
}

/// Markdown into blocks: fenced code, and the prose between.
pub fn parse(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut prose = String::new();
    let mut code: Option<(String, String)> = None;
    for line in md.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("```") {
            match code.take() {
                Some((lang, text)) => out.push(Block::Code { lang, text: text.trim_end().to_string() }),
                None => {
                    if !prose.trim().is_empty() {
                        out.push(Block::Prose(prose.trim().to_string()));
                    }
                    prose.clear();
                    code = Some((rest.trim().to_string(), String::new()));
                }
            }
            continue;
        }
        match code.as_mut() {
            Some((_, text)) => {
                text.push_str(line);
                text.push('\n');
            }
            None => {
                let l = t.trim_start_matches(['#', '*', '-', ' ']).trim();
                if l.is_empty() {
                    if !prose.ends_with('\n') && !prose.is_empty() {
                        prose.push('\n');
                    }
                } else {
                    if !prose.is_empty() && !prose.ends_with('\n') {
                        prose.push(' ');
                    }
                    prose.push_str(l);
                }
            }
        }
    }
    if let Some((lang, text)) = code.take() {
        out.push(Block::Code { lang, text: text.trim_end().to_string() });
    }
    if !prose.trim().is_empty() {
        out.push(Block::Prose(prose.trim().to_string()));
    }
    // Inline code alone on a line reads as a command too.
    out.into_iter()
        .map(|b| match b {
            Block::Prose(p) if p.starts_with('`') && p.ends_with('`') && p.len() > 2 && !p[1..p.len() - 1].contains('`') => Block::Code { lang: String::new(), text: p[1..p.len() - 1].to_string() },
            b => b,
        })
        .collect()
}

impl App {
    /// The shell the panel belongs to: the focused terminal.
    pub(crate) fn ask_term(&mut self) -> Option<&mut TermPane> {
        let tab = self.tabs.get_mut(self.active)?;
        match tab.focused() {
            Pane::Term(t) => Some(t),
            _ => None,
        }
    }

    /// Ctrl+Shift+?: open the panel (and focus its field), or close it.
    pub(crate) fn toggle_ask(&mut self) {
        let keys = self.behavior.ask_ctx.clone();
        let Some(t) = self.ask_term() else { return };
        if t.ask.is_some() {
            t.ask = None;
        } else {
            t.ask = Some(Ask::from_keys(&keys));
        }
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    /// A question from remote control: the panel opens with it and sends.
    pub(crate) fn ask_from_remote(&mut self, q: &str) {
        let keys = self.behavior.ask_ctx.clone();
        let Some(t) = self.ask_term() else { return };
        if t.ask.is_none() {
            t.ask = Some(Ask::from_keys(&keys));
        }
        if let Some(ask) = t.ask.as_mut() {
            ask.input = q.to_string();
        }
        self.layout();
        self.ask_send();
    }

    /// Send the field: the prompt gets the shell, OS, cwd and the last
    /// command's tail; a worker runs the backend.
    pub(crate) fn ask_send(&mut self) {
        self.ask_send_with(None);
    }

    /// Send the field, or a skill's prompt with the field as its subject.
    /// The context the chips ask for goes along; the page's text arrives
    /// a tick later, so the prompt is built in `tend_ask`.
    pub(crate) fn ask_send_with(&mut self, skill: Option<usize>) {
        let sk = skill.and_then(|i| self.skills.get(i).cloned());
        let (q, ctx, skill_prompt) = {
            let Some(t) = self.ask_term() else { return };
            let Some(ask) = t.ask.as_mut() else { return };
            if ask.pending.is_some() || ask.gathering.is_some() {
                return;
            }
            let q = ask.input.trim().to_string();
            let ctx = match &sk {
                Some(sk) if !sk.context.is_empty() => sk.context.clone(),
                _ => ask.ctx.clone(),
            };
            if q.is_empty() && sk.is_none() {
                return;
            }
            (q, ctx, sk.map(|s| s.prompt))
        };
        if backends().is_empty() {
            if let Some(ask) = self.ask_term().and_then(|t| t.ask.as_mut()) {
                ask.turns.push(Turn { q: q.clone(), blocks: Vec::new(), error: Some("no assistant on this machine · claude, codex, copilot, ollama, or ANTHROPIC_API_KEY".into()) });
                ask.input.clear();
            }
            self.dirty = true;
            return;
        }
        let gathered = self.gather_context(&ctx);
        let page = if ctx.contains(&crate::askctx::Ctx::Page) { self.request_page_text() } else { None };
        let Some(ask) = self.ask_term().and_then(|t| t.ask.as_mut()) else { return };
        ask.input.clear();
        let shown = match &skill_prompt {
            Some(p) if q.is_empty() => p.chars().take(80).collect(),
            _ => q.clone(),
        };
        ask.turns.push(Turn { q: shown, blocks: Vec::new(), error: None });
        match page {
            Some((id, title, url)) => ask.gathering = Some((id, title, url, q, gathered, Instant::now(), skill_prompt)),
            None => ask.gathering = Some((-1, String::new(), String::new(), q, gathered, Instant::now(), skill_prompt)),
        }
        self.play_event("control.press");
        self.dirty = true;
    }

    /// The gathered context is complete (or the page timed out): build
    /// the prompt and fire the backend.
    fn ask_fire(&mut self, q: String, mut g: crate::askctx::Gathered, page: Option<(String, String, String)>, skill_prompt: Option<String>) {
        let Some(backend) = backends().into_iter().next() else { return };
        if let Some(p) = page {
            g.page = Some(p);
        }
        let mut prompt = String::from(
            "You are the assistant inside nus, a terminal that is also a browser. Answer for exactly this situation. \
             Reply with at most three fenced code blocks when a command is the answer, each one complete and ready to paste, each preceded by one short line saying what it does; \
             answer in short plain prose otherwise. No preamble, no closing remarks, no headings.\n\n",
        );
        prompt.push_str(&g.render());
        match (skill_prompt, q.is_empty()) {
            (Some(p), true) => prompt.push_str(&format!("\nTask: {p}\n")),
            (Some(p), false) => prompt.push_str(&format!("\nTask: {p}\nAbout: {q}\n")),
            (None, _) => prompt.push_str(&format!("\nQuestion: {q}\n")),
        }
        let (tx, rx) = channel();
        let b = backend.clone();
        let p = prompt.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run(&b, &p));
        });
        if let Some(ask) = self.ask_term().and_then(|t| t.ask.as_mut()) {
            ask.pending = Some((rx, Instant::now(), backend.name.clone()));
        }
        self.dirty = true;
    }

    /// Once a loop: answers that have arrived.
    pub(crate) fn tend_ask(&mut self) {
        let mut changed = false;
        // A question waiting on the page's text.
        let waiting = self.ask_term().and_then(|t| t.ask.as_ref()).and_then(|a| a.gathering.as_ref().map(|g| (g.0, g.5)));
        if let Some((id, since)) = waiting {
            let text = if id >= 0 { self.take_page_text(id) } else { Some(String::new()) };
            let timed_out = since.elapsed().as_millis() > 1500;
            if text.is_some() || timed_out {
                if let Some((_, title, url, q, g, _, sk)) = self.ask_term().and_then(|t| t.ask.as_mut()).and_then(|a| a.gathering.take()) {
                    let page = match text {
                        Some(t) if id >= 0 && !t.is_empty() => Some((title, url, t)),
                        _ => None,
                    };
                    self.ask_fire(q, g, page, sk);
                }
            }
        }
        for tab in self.tabs.iter_mut() {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(t) = p else { continue };
                let Some(ask) = t.ask.as_mut() else { continue };
                let Some((rx, at, _)) = ask.pending.as_ref() else { continue };
                match rx.try_recv() {
                    Ok(r) => {
                        if let Some(turn) = ask.turns.last_mut() {
                            match r {
                                Ok(md) => turn.blocks = parse(&md),
                                Err(e) => turn.error = Some(e),
                            }
                        }
                        ask.pending = None;
                        ask.scroll = f32::MAX; // to the bottom
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if let Some(turn) = ask.turns.last_mut() {
                            turn.error = Some("the assistant went away".into());
                        }
                        ask.pending = None;
                        changed = true;
                    }
                    Err(_) => {
                        if at.elapsed().as_secs() > 90 {
                            if let Some(turn) = ask.turns.last_mut() {
                                turn.error = Some("no answer in 90s".into());
                            }
                            ask.pending = None;
                        }
                        changed = true; // the lamp breathes
                    }
                }
            }
        }
        if changed {
            self.dirty = true;
        }
    }

    /// Keys while the field has focus. Returns true when taken.
    pub(crate) fn ask_key(&mut self, key: &winit::event::KeyEvent) -> bool {
        use winit::event::ElementState;
        use winit::keyboard::{Key as WKey, NamedKey};
        if key.state != ElementState::Pressed {
            return false;
        }
        let ctrl = self.mods.control_key();
        let Some(t) = self.ask_term() else { return false };
        let Some(ask) = t.ask.as_mut() else { return false };
        if !ask.focus {
            return false;
        }
        match &key.logical_key {
            WKey::Named(NamedKey::Escape) => {
                t.ask = None;
                self.layout();
            }
            WKey::Named(NamedKey::Enter) => self.ask_send(),
            WKey::Named(NamedKey::Backspace) => {
                if ctrl {
                    let trimmed = ask.input.trim_end().to_string();
                    let cut = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                    ask.input.truncate(cut);
                } else {
                    ask.input.pop();
                }
            }
            WKey::Named(NamedKey::Space) => ask.input.push(' '),
            WKey::Character(c) if !ctrl => ask.input.push_str(c),
            WKey::Character(c) if ctrl && c.eq_ignore_ascii_case("v") => {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(text) = cb.get_text() {
                        ask.input.push_str(text.lines().next().unwrap_or(""));
                    }
                }
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// A click in a panel: a block's chip, the field, the close. Returns
    /// true when taken.
    pub(crate) fn ask_click(&mut self, x: f32, y: f32) -> bool {
        let Some(t) = self.ask_term() else { return false };
        let Some(ask) = t.ask.as_mut() else { return false };
        if !ask.rect.contains(x, y) {
            if ask.focus {
                ask.focus = false;
                self.dirty = true;
            }
            return false;
        }
        let hit = ask.hits.iter().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h);
        let mut typed: Option<(String, bool)> = None;
        let mut copy: Option<String> = None;
        let mut sound: Option<&'static str> = None;
        match hit {
            Some(AskHit::Field) | None => ask.focus = true,
            Some(AskHit::Close) => {
                t.ask = None;
                self.layout();
                self.dirty = true;
                return true;
            }
            Some(AskHit::Insert(ti, bi)) | Some(AskHit::Run(ti, bi)) => {
                if let Some(Block::Code { text, .. }) = ask.turns.get(ti).and_then(|t| t.blocks.get(bi)) {
                    typed = Some((text.clone(), matches!(hit, Some(AskHit::Run(..)))));
                }
                ask.focus = false;
            }
            Some(AskHit::Copy(ti, bi)) => {
                if let Some(Block::Code { text, .. }) = ask.turns.get(ti).and_then(|t| t.blocks.get(bi)) {
                    copy = Some(text.clone());
                    ask.copied = Some((ti, bi, Instant::now()));
                }
            }
            Some(AskHit::Ctx(c)) => {
                if let Some(i) = ask.ctx.iter().position(|x| *x == c) {
                    ask.ctx.remove(i);
                } else {
                    ask.ctx.push(c);
                }
                sound = Some("toggle");
            }
            Some(AskHit::Skill(i)) => {
                self.ask_send_with(Some(i));
                return true;
            }
            Some(AskHit::Remember(ti)) => {
                // The first prose line of the answer, or the question.
                let line = ask.turns.get(ti).and_then(|t| {
                    t.blocks.iter().find_map(|b| match b {
                        Block::Prose(s) => Some(s.lines().next().unwrap_or("").to_string()),
                        _ => None,
                    })
                }).filter(|s| !s.is_empty()).or_else(|| ask.turns.get(ti).map(|t| t.q.clone()));
                if let Some(l) = line {
                    crate::askctx::remember(&l);
                    if !ask.remembered.contains(&ti) {
                        ask.remembered.push(ti);
                    }
                }
                sound = Some("copied");
            }
        }
        if let Some((text, run)) = typed {
            // One line goes to the prompt as is; more lines go as one paste.
            let text = text.replace("\r\n", "\n");
            let bytes = if run { format!("{text}\r") } else { text.clone() };
            let _ = t.pty.write(bytes.replace('\n', "\r").as_bytes());
            sound = Some("control.release");
        }
        if let Some(s) = sound {
            self.play_event(s);
        }
        if let Some(text) = copy {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(text);
            }
            self.play_event("copied");
        }
        self.dirty = true;
        true
    }

    /// Wheel over a panel scrolls it. Returns true when taken.
    pub(crate) fn ask_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(t) = self.ask_term() else { return false };
        let Some(ask) = t.ask.as_mut() else { return false };
        if !ask.rect.contains(x, y) {
            return false;
        }
        ask.scroll = (ask.scroll - dy).max(0.0);
        self.dirty = true;
        true
    }

    /// Draw the panel at the pane's right, inside `r` (the pane rect below
    /// its header). The layout gave the shell the rest.
    pub(crate) fn draw_ask(&mut self, scene: &mut Scene, p: &mut TermPane, r: Rect, focused: bool) {
        let Some(ask) = p.ask.as_mut() else { return };
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let mono = Style { font: self.f.term, px: self.px(12.5), color: ink, tracking: 0.0 };
        let (mx, my) = self.mouse;
        let w = self.px(PANEL_W).min(r.w * 0.6);
        let pr = Rect::new(r.right() - w, r.y, w, r.h);
        ask.rect = pr;
        ask.hits.clear();
        scene.layer(Some(pr));
        scene.rect(pr, paper);
        scene.vline(pr.x, pr.y, pr.h, self.px(m::STRUCTURE), ink);
        let pad = self.px(12.0);
        // Head: ASK, the backend, ×.
        let head_h = self.px(30.0);
        let base = pr.y + self.px(20.0);
        let wm = Style { font: self.f.wordmark, px: self.px(18.0), color: ink, tracking: 0.0 };
        let mut x = pr.x + pad;
        x += self.fonts.draw(scene, wm, x, base + self.px(1.0), "ask") + self.px(10.0);
        let who = backends().into_iter().next().map(|b| b.name.caps()).unwrap_or_else(|| "NO ASSISTANT".into());
        self.fonts.draw(scene, Style { color: t.dim, ..label }, x, base, &who);
        let isz = self.px(12.0);
        let cx = pr.right() - pad - isz;
        self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, isz, cx, pr.y + ((head_h - isz) / 2.0).round(), if Rect::new(cx - 6.0, pr.y, isz + 12.0, head_h).contains(mx, my) { ink } else { t.dim });
        ask.hits.push((Rect::new(cx - self.px(8.0), pr.y, isz + self.px(16.0), head_h), AskHit::Close));
        scene.hline(pr.x, pr.y + head_h, pr.w, self.px(m::STRUCTURE), ink);
        // The chips: what goes along, and the skills.
        let ctx_now = ask.ctx.clone();
        let (chips_h, chip_hits) = self.draw_ask_chips(scene, &ctx_now, pr.x + pad, pr.y + head_h + self.px(6.0), pr.w - 2.0 * pad);
        ask.hits.extend(chip_hits);
        // The field.
        let fy = pr.y + head_h + self.px(8.0) + chips_h;
        let field = Rect::new(pr.x + pad, fy, pr.w - 2.0 * pad, self.px(28.0));
        let lit = ask.focus && focused;
        scene.outline(field, self.px(if lit { m::STRUCTURE } else { m::HAIRLINE }), if lit { ink } else { t.dim });
        let fb = fy + self.px(19.0);
        if ask.input.is_empty() {
            self.fonts.draw(scene, Style { color: t.dim, ..ui }, field.x + self.px(8.0), fb, "how do I …");
        } else {
            scene.layer(Some(field));
            let iw = self.fonts.measure(ui, &ask.input);
            let shift = (iw - (field.w - self.px(16.0))).max(0.0);
            self.fonts.draw(scene, ui, field.x + self.px(8.0) - shift, fb, &ask.input);
            scene.layer(Some(pr));
        }
        if lit {
            let iw = self.fonts.measure(ui, &ask.input);
            let cxr = (field.x + self.px(8.0) + iw.min(field.w - self.px(16.0))).round();
            let on = (self.started.elapsed().as_secs_f32() * 2.0) as u32 % 2 == 0;
            if on {
                scene.rect(Rect::new(cxr, fy + self.px(6.0), self.px(1.5), field.h - self.px(12.0)), ink);
            }
            self.dirty = true;
        }
        ask.hits.push((field, AskHit::Field));
        // The turns, oldest first, scrolled so the newest is in view.
        let top = field.bottom() + self.px(10.0);
        let bottom = pr.bottom() - self.px(8.0);
        let view = Rect::new(pr.x, top, pr.w, (bottom - top).max(0.0));
        let inner_w = pr.w - 2.0 * pad;
        // Measure first: total height, then place with the scroll.
        let dim = Style { color: t.dim, ..label };
        let mut lines_cache: Vec<(usize, usize, Vec<String>)> = Vec::new();
        // Code wraps by column: the mono advance says how many fit a card.
        let cell = self.fonts.measure(mono, "M").max(1.0);
        let cols_fit = (((inner_w - self.px(16.0)) / cell).floor() as usize).max(8);
        let mut total = 0.0;
        let line_h = self.px(17.0);
        let code_line_h = self.px(16.0);
        for (ti, turn) in ask.turns.iter().enumerate() {
            let ql = crate::reader::wrap(&self.fonts, strong, &turn.q, inner_w);
            total += ql.len() as f32 * line_h + self.px(6.0);
            lines_cache.push((ti, usize::MAX, ql));
            for (bi, b) in turn.blocks.iter().enumerate() {
                match b {
                    Block::Prose(p) => {
                        let l = crate::reader::wrap(&self.fonts, dim, p, inner_w);
                        total += l.len() as f32 * line_h + self.px(4.0);
                        lines_cache.push((ti, bi, l));
                    }
                    Block::Code { text, .. } => {
                        let n = wrap_code(text, cols_fit) .len().max(1) as f32;
                        total += n * code_line_h + self.px(12.0) + self.px(22.0) + self.px(8.0);
                    }
                }
            }
            if let Some(e) = &turn.error {
                let l = crate::reader::wrap(&self.fonts, dim, e, inner_w);
                total += l.len() as f32 * line_h + self.px(4.0);
                lines_cache.push((ti, usize::MAX - 1, l));
            }
            if turn.blocks.is_empty() && turn.error.is_none() {
                total += self.px(20.0);
            }
            total += self.px(10.0);
        }
        let max_scroll = (total - view.h).max(0.0);
        if ask.scroll > max_scroll {
            ask.scroll = max_scroll;
        }
        scene.layer(Some(view));
        let mut y = top - ask.scroll;
        let mut cache_i = 0;
        let mut next_lines = |ti: usize, bi: usize| -> Vec<String> {
            while cache_i < lines_cache.len() {
                let (a, b, ref l) = lines_cache[cache_i];
                cache_i += 1;
                if a == ti && b == bi {
                    return l.clone();
                }
            }
            Vec::new()
        };
        let pending = ask.pending.is_some();
        let n_turns = ask.turns.len();
        for (ti, turn) in ask.turns.iter().enumerate() {
            // The question.
            for l in next_lines(ti, usize::MAX) {
                self.fonts.draw(scene, Style { color: ink, ..strong }, pr.x + pad, y + self.px(13.0), &l);
                y += line_h;
            }
            y += self.px(6.0);
            if turn.blocks.is_empty() && turn.error.is_none() {
                if pending && ti + 1 == n_turns {
                    // Waiting: a breathing dot.
                    let k = 0.35 + 0.65 * (self.started.elapsed().as_secs_f32() * 3.0).sin().abs();
                    let d = self.px(7.0);
                    scene.push(nus_render::Instance::rounded(Rect::new(pr.x + pad, y + self.px(4.0), d, d), d / 2.0, crate::app::fade(self.surface.signal, k)));
                    self.fonts.draw(scene, dim, pr.x + pad + d + self.px(8.0), y + self.px(11.0), "THINKING");
                }
                y += self.px(20.0);
            }
            for (bi, b) in turn.blocks.iter().enumerate() {
                match b {
                    Block::Prose(_) => {
                        for l in next_lines(ti, bi) {
                            self.fonts.draw(scene, dim, pr.x + pad, y + self.px(13.0), &l);
                            y += line_h;
                        }
                        y += self.px(4.0);
                    }
                    Block::Code { text, lang } => {
                        let shown = wrap_code(text, cols_fit);
                        let n = shown.len().max(1) as f32;
                        let card_h = n * code_line_h + self.px(12.0);
                        // The grammar's colours, per original line, when there is one.
                        let lang_key = match lang.to_ascii_lowercase().as_str() {
                            "" => "bash",
                            "sh" | "shell" | "zsh" | "console" => "bash",
                            "pwsh" | "ps1" | "ps" => "powershell",
                            other => other,
                        }
                        .to_string();
                        let coloured: Vec<Vec<(usize, usize, crate::predict::Tok)>> = text.lines().map(|l| crate::syntax::spans(&lang_key, l).unwrap_or_default()).collect();
                        let card = Rect::new(pr.x + pad, y, inner_w, card_h);
                        scene.rect(card, crate::surface::mix(paper, ink, 0.06));
                        scene.outline(card, self.px(m::HAIRLINE), fade(t.dim, 0.6));
                        scene.layer(Some(card));
                        let mut ly = y + self.px(6.0) + self.px(12.0);
                        let cell = self.fonts.measure(mono, "M").max(1.0);
                        for (piece, li, off) in &shown {
                            self.fonts.draw(scene, mono, card.x + self.px(8.0), ly, piece);
                            // Coloured runs over the plain line: monospace, so x is a column.
                            let indent = if *off > 0 { 2 } else { 0 };
                            let plen = piece.chars().count().saturating_sub(indent);
                            for &(a, l, class) in coloured.get(*li).map(|v| v.as_slice()).unwrap_or(&[]) {
                                let (s0, s1) = (a.max(*off), (a + l).min(off + plen));
                                if s1 <= s0 {
                                    continue;
                                }
                                let color = match class {
                                    crate::predict::Tok::Command => crate::theme_edit::from_rgb(t.ansi[4]),
                                    crate::predict::Tok::Flag => crate::theme_edit::from_rgb(t.ansi[6]),
                                    crate::predict::Tok::Str => crate::theme_edit::from_rgb(t.ansi[2]),
                                    crate::predict::Tok::Num => crate::theme_edit::from_rgb(t.ansi[5]),
                                    crate::predict::Tok::Op => crate::theme_edit::from_rgb(t.ansi[3]),
                                    _ => continue,
                                };
                                let run: String = piece.chars().skip(indent + (s0 - off)).take(s1 - s0).collect();
                                let x = card.x + self.px(8.0) + (indent + (s0 - off)) as f32 * cell;
                                scene.rect(Rect::new(x, ly - self.px(11.0), (s1 - s0) as f32 * cell, code_line_h), crate::surface::mix(paper, ink, 0.06));
                                self.fonts.draw(scene, Style { color, ..mono }, x, ly, &run);
                            }
                            ly += code_line_h;
                        }
                        scene.layer(Some(view));
                        y += card_h;
                        // Chips: insert · run · copy as icons (the tooltip says which), and the language, dim.
                        let cy = y + self.px(4.0);
                        let ch = self.px(18.0);
                        let mut cx = card.x;
                        let copied = ask.copied.is_some_and(|(a, b, at)| a == ti && b == bi && at.elapsed().as_secs_f32() < 1.2);
                        let isz = self.px(13.0);
                        for (k, icon, words, hit) in [
                            (0usize, nus_render::text::icons::ENTER, "insert at the prompt", AskHit::Insert(ti, bi)),
                            (1, nus_render::text::icons::TERMINAL_BOLD, "run it", AskHit::Run(ti, bi)),
                            (2, if copied { nus_render::text::icons::CHECK } else { nus_render::text::icons::COPY }, "copy", AskHit::Copy(ti, bi)),
                        ] {
                            let chip = Rect::new(cx, cy, isz + self.px(12.0), ch);
                            let hot = chip.contains(mx, my);
                            self.icon_button(scene, icon, isz, chip.x + self.px(6.0), cy + (ch - isz) / 2.0, if copied && k == 2 { self.surface.signal } else { ink }, chip, crate::app::hover_key("askchip", ti * 100 + bi * 10 + k), IconMotion::Pop);
                            if hot {
                                self.tip_words(chip, words);
                            }
                            ask.hits.push((chip, hit));
                            cx += chip.w + self.px(4.0);
                        }
                        if !lang.is_empty() {
                            let lw = self.fonts.measure(dim, &lang.caps());
                            self.fonts.draw(scene, dim, card.right() - lw, cy + self.px(13.0), &lang.caps());
                        }
                        y += ch + self.px(4.0) + self.px(8.0);
                    }
                }
            }
            // REMEMBER: the answer's first line into memory, as a book icon at the turn's end.
            if !turn.blocks.is_empty() && turn.error.is_none() {
                let isz = self.px(13.0);
                let chip = Rect::new(pr.right() - pad - isz - self.px(8.0), y - self.px(2.0), isz + self.px(8.0), self.px(18.0));
                let done = ask.remembered.contains(&ti);
                self.icon_button(scene, nus_render::text::icons::BOOK, isz, chip.x + self.px(4.0), chip.y + self.px(2.0), if done { self.surface.signal } else { t.dim }, chip, crate::app::hover_key("askmem", ti), IconMotion::Pop);
                if chip.contains(mx, my) {
                    self.tip_words(chip, if done { "remembered" } else { "remember this" });
                }
                ask.hits.push((chip, AskHit::Remember(ti)));
            }
            if turn.error.is_some() {
                for l in next_lines(ti, usize::MAX - 1) {
                    self.fonts.draw(scene, Style { color: self.surface.signal, ..label }, pr.x + pad, y + self.px(13.0), &l);
                    y += line_h;
                }
                y += self.px(4.0);
            }
            y += self.px(10.0);
        }
        if ask.turns.is_empty() {
            let hint = crate::reader::wrap(&self.fonts, dim, "One line in, a few commands out. The shell, the folder and the last command's output go along, so ask about what you see.", inner_w);
            let mut hy = top + self.px(4.0);
            for l in hint {
                self.fonts.draw(scene, dim, pr.x + pad, hy + self.px(13.0), &l);
                hy += line_h;
            }
        }
        scene.layer(None);
    }
}

fn fade(c: nus_render::Color, k: f32) -> nus_render::Color {
    crate::app::fade(c, k)
}

/// Code lines cut to `cols` characters as (piece, line index, char offset
/// into that line); continuation pieces carry a hanging indent of two
/// spaces so a wrapped command still reads as one.
fn wrap_code(text: &str, cols: usize) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    for (li, line) in text.lines().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= cols {
            out.push((line.to_string(), li, 0));
            continue;
        }
        let mut start = 0;
        let mut first = true;
        while start < chars.len() {
            let room = if first { cols } else { cols.saturating_sub(2).max(1) };
            let end = (start + room).min(chars.len());
            let piece: String = chars[start..end].iter().collect();
            out.push((if first { piece } else { format!("  {piece}") }, li, start));
            first = false;
            start = end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_become_blocks() {
        let md = "List them:
```powershell
Get-ChildItem
```
Count:
```
(Get-ChildItem).Count
```
";
        let b = parse(md);
        assert_eq!(b.len(), 4);
        assert_eq!(b[0], Block::Prose("List them:".into()));
        assert_eq!(b[1], Block::Code { lang: "powershell".into(), text: "Get-ChildItem".into() });
        assert_eq!(b[3], Block::Code { lang: String::new(), text: "(Get-ChildItem).Count".into() });
    }

    #[test]
    fn a_lone_inline_code_line_is_a_command() {
        let b = parse("`ls -la`");
        assert_eq!(b, vec![Block::Code { lang: String::new(), text: "ls -la".into() }]);
    }

    #[test]
    fn code_wraps_with_a_hanging_indent() {
        let w = wrap_code("abcdefghij", 6);
        assert_eq!(w, vec![("abcdef".to_string(), 0, 0), ("  ghij".to_string(), 0, 6)]);
        assert_eq!(wrap_code("ok", 6), vec![("ok".to_string(), 0, 0)]);
    }
}
