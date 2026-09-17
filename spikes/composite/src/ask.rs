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

use crate::app::{App, Pane, TermPane};
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
}

impl Ask {
    pub fn new() -> Ask {
        Ask { input: String::new(), focus: true, turns: Vec::new(), pending: None, scroll: 0.0, hits: Vec::new(), rect: Rect::new(0.0, 0.0, 0.0, 0.0), copied: None }
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
    fn ask_term(&mut self) -> Option<&mut TermPane> {
        let tab = self.tabs.get_mut(self.active)?;
        match tab.focused() {
            Pane::Term(t) => Some(t),
            _ => None,
        }
    }

    /// Ctrl+Shift+?: open the panel (and focus its field), or close it.
    pub(crate) fn toggle_ask(&mut self) {
        let Some(t) = self.ask_term() else { return };
        if t.ask.is_some() {
            t.ask = None;
        } else {
            t.ask = Some(Ask::new());
        }
        self.play_event("toggle");
        self.layout();
        self.dirty = true;
    }

    /// Send the field: the prompt gets the shell, OS, cwd and the last
    /// command's tail; a worker runs the backend.
    pub(crate) fn ask_send(&mut self) {
        let backend = backends().into_iter().next();
        let profile = self.tabs.get(self.active).and_then(|t| match &t.left {
            Pane::Term(p) => self.profiles.get(p.profile).map(|p| p.name.clone()),
            _ => None,
        }).unwrap_or_else(|| "shell".into());
        let Some(t) = self.ask_term() else { return };
        let Some(ask) = t.ask.as_mut() else { return };
        let q = ask.input.trim().to_string();
        if q.is_empty() || ask.pending.is_some() {
            return;
        }
        let Some(backend) = backend else {
            ask.turns.push(Turn { q: q.clone(), blocks: Vec::new(), error: Some("no assistant on this machine · claude, codex, copilot, ollama, or ANTHROPIC_API_KEY".into()) });
            ask.input.clear();
            self.dirty = true;
            return;
        };
        // Context: the last command and the tail of its output.
        let last = t.term.marks.iter().rev().find(|m| m.kind == nus_vt::MarkKind::CommandStart).map(|m| (t.term.command_text(m), t.term.output_text(m)));
        let cwd = t.cwd.clone().unwrap_or_default();
        let os = if cfg!(windows) { "Windows 11" } else if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
        let mut prompt = format!(
            "You are a terminal assistant inside a {profile} shell on {os}. The user's working directory is {cwd}.\n\
             Answer the question below for this exact shell. Reply with at most three fenced code blocks, each one complete command (or short script) ready to paste, \
             each preceded by one short line saying what it does. No preamble, no closing remarks, no headings.\n"
        );
        if let Some((cmd, out)) = last {
            if !cmd.trim().is_empty() {
                let tail: Vec<&str> = out.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect();
                prompt.push_str(&format!("\nThe last command was:\n{}\nIts output ended with:\n{}\n", cmd.trim(), tail.join("\n")));
            }
        }
        prompt.push_str(&format!("\nQuestion: {q}\n"));
        let (tx, rx) = channel();
        let b = backend.clone();
        let p = prompt.clone();
        std::thread::spawn(move || {
            let _ = tx.send(run(&b, &p));
        });
        ask.pending = Some((rx, Instant::now(), backend.name.clone()));
        ask.input.clear();
        ask.turns.push(Turn { q, blocks: Vec::new(), error: None });
        self.play_event("control.press");
        self.dirty = true;
    }

    /// Once a loop: answers that have arrived.
    pub(crate) fn tend_ask(&mut self) {
        let mut changed = false;
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
        }
        if let Some((text, run)) = typed {
            // One line goes to the prompt as is; more lines go as one paste.
            let text = text.replace("\r\n", "\n");
            let bytes = if run { format!("{text}\r") } else { text.clone() };
            let _ = t.pty.write(bytes.replace('\n', "\r").as_bytes());
            self.play_event("control.release");
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
        let who = backends().into_iter().next().map(|b| b.name.to_uppercase()).unwrap_or_else(|| "NO ASSISTANT".into());
        self.fonts.draw(scene, Style { color: t.dim, ..label }, x, base, &who);
        let isz = self.px(12.0);
        let cx = pr.right() - pad - isz;
        self.fonts.draw_icon(scene, nus_render::text::icons::CLOSE, isz, cx, pr.y + ((head_h - isz) / 2.0).round(), if Rect::new(cx - 6.0, pr.y, isz + 12.0, head_h).contains(mx, my) { ink } else { t.dim });
        ask.hits.push((Rect::new(cx - self.px(8.0), pr.y, isz + self.px(16.0), head_h), AskHit::Close));
        scene.hline(pr.x, pr.y + head_h, pr.w, self.px(m::STRUCTURE), ink);
        // The field.
        let fy = pr.y + head_h + self.px(8.0);
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
                        let card = Rect::new(pr.x + pad, y, inner_w, card_h);
                        scene.rect(card, crate::surface::mix(paper, ink, 0.06));
                        scene.outline(card, self.px(m::HAIRLINE), fade(t.dim, 0.6));
                        scene.layer(Some(card));
                        let mut ly = y + self.px(6.0) + self.px(12.0);
                        for line in &shown {
                            self.fonts.draw(scene, mono, card.x + self.px(8.0), ly, line);
                            ly += code_line_h;
                        }
                        scene.layer(Some(view));
                        y += card_h;
                        // Chips: INSERT · RUN · COPY, and the language, dim.
                        let cy = y + self.px(4.0);
                        let ch = self.px(18.0);
                        let mut cx = card.x;
                        let copied = ask.copied.is_some_and(|(a, b, at)| a == ti && b == bi && at.elapsed().as_secs_f32() < 1.2);
                        for (word, hit) in [("INSERT", AskHit::Insert(ti, bi)), ("RUN", AskHit::Run(ti, bi)), (if copied { "COPIED" } else { "COPY" }, AskHit::Copy(ti, bi))] {
                            let ww = self.fonts.measure(label, word);
                            let chip = Rect::new(cx, cy, ww + self.px(14.0), ch);
                            let hot = chip.contains(mx, my);
                            if hot {
                                scene.rect(chip, ink);
                            } else {
                                scene.outline(chip, self.px(m::HAIRLINE), t.dim);
                            }
                            self.fonts.draw(scene, Style { color: if hot { t.paper } else { ink }, ..label }, chip.x + self.px(7.0), cy + self.px(13.0), word);
                            ask.hits.push((chip, hit));
                            cx += chip.w + self.px(6.0);
                        }
                        if !lang.is_empty() {
                            let lw = self.fonts.measure(dim, &lang.to_uppercase());
                            self.fonts.draw(scene, dim, card.right() - lw, cy + self.px(13.0), &lang.to_uppercase());
                        }
                        y += ch + self.px(4.0) + self.px(8.0);
                    }
                }
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

/// Code lines cut to `cols` characters; continuation lines carry a hanging
/// indent of two spaces so a wrapped command still reads as one.
fn wrap_code(text: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= cols {
            out.push(line.to_string());
            continue;
        }
        let mut start = 0;
        let mut first = true;
        while start < chars.len() {
            let room = if first { cols } else { cols.saturating_sub(2).max(1) };
            let end = (start + room).min(chars.len());
            let piece: String = chars[start..end].iter().collect();
            out.push(if first { piece } else { format!("  {piece}") });
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
        assert_eq!(w, vec!["abcdef", "  ghij"]);
        assert_eq!(wrap_code("ok", 6), vec!["ok"]);
    }
}
