//! What the assistant sees. Chips above the field — shell · block · page ·
//! tabs · editor · memory — each an icon that is lit when it goes along.
//! Shell + the focused block + the split's page are on by default; ALL
//! TABS and EDITOR are a tap. Skills are rules (`skills = { … }`): a saved
//! prompt with its own context, a chip and a palette row. Memory is
//! `profile/memory.md`, appended by REMEMBER on a turn, read into every
//! prompt when on.

use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};

use crate::app::{fade, hover_key, App, IconMotion, Pane};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctx {
    Shell,
    Block,
    Page,
    Tabs,
    Editor,
    Memory,
}

impl Ctx {
    pub const ALL: [Ctx; 6] = [Ctx::Shell, Ctx::Block, Ctx::Page, Ctx::Tabs, Ctx::Editor, Ctx::Memory];
    pub fn icon(self) -> (&'static str, &'static str) {
        use nus_render::text::icons as i;
        match self {
            Ctx::Shell => i::TERMINAL,
            Ctx::Block => i::HASH,
            Ctx::Page => i::GLOBE,
            Ctx::Tabs => i::SQUARES,
            Ctx::Editor => i::CODE,
            Ctx::Memory => i::BOOK,
        }
    }
    pub fn words(self) -> &'static str {
        match self {
            Ctx::Shell => "this shell · cwd and the last command",
            Ctx::Block => "the focused block · command and output",
            Ctx::Page => "the page beside · its text",
            Ctx::Tabs => "all tabs · titles and urls",
            Ctx::Editor => "the editor · the open file",
            Ctx::Memory => "memory · profile/memory.md",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Ctx::Shell => "shell",
            Ctx::Block => "block",
            Ctx::Page => "page",
            Ctx::Tabs => "tabs",
            Ctx::Editor => "editor",
            Ctx::Memory => "memory",
        }
    }
}

/// A skill from rules.luau: a prompt with its own context and a chip.
#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub prompt: String,
    pub context: Vec<Ctx>,
}

/// The gathered context, ready to write into a prompt.
#[derive(Default, Debug)]
pub struct Gathered {
    pub shell: Option<(String, String, String)>, // profile, cwd, os
    pub block: Option<(String, String, Option<i32>)>,
    pub page: Option<(String, String, String)>, // title, url, text
    pub tabs: Vec<(String, String)>,
    pub editor: Option<(String, String)>, // path, text
    pub memory: Option<String>,
}

impl Gathered {
    /// Did anything nus did not author go into this prompt? Page text and
    /// tab titles come off the web, where a page can write whatever it
    /// likes — including a line addressed to the assistant. An answer built
    /// on that is an answer a stranger had a hand in.
    ///
    /// Shell output is not counted, though a hostile repository or a curl
    /// can reach it too: almost every question carries a block, so marking
    /// those would mark everything and mean nothing.
    pub fn from_the_web(&self) -> bool {
        self.page.is_some() || !self.tabs.is_empty()
    }

    /// The prompt's context section: what's there, in a fixed order.
    pub fn render(&self) -> String {
        self.render_filtered(true)
    }
    pub fn render_original(&self)->String{self.render_filtered(false)}
    fn render_filtered(&self,redact:bool)->String {
        let mut s = String::new();
        if let Some((profile, cwd, os)) = &self.shell {
            s.push_str(&format!("The shell is {profile} on {os}; the working directory is {cwd}.\n"));
        }
        if let Some((cmd, out, exit)) = &self.block {
            let out=if redact{crate::secrets::scrub(out).text}else{out.clone()};
            let tail: Vec<&str> = out.lines().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
            s.push_str(&format!("\nThe command in focus was:\n{}\n", cmd.trim()));
            if let Some(e) = exit {
                s.push_str(&format!("It exited with {e}.\n"));
            }
            s.push_str(&format!("Its output ended with:\n{}\n", tail.join("\n")));
        }
        if let Some((title, url, text)) = &self.page {
            let text=if redact{crate::secrets::scrub(text).text}else{text.clone()};
            let text: String = text.chars().take(6000).collect();
            s.push_str(&format!("\nThe page beside the shell is \"{title}\" ({url}). Its text:\n{text}\n"));
        }
        if !self.tabs.is_empty() {
            s.push_str("\nThe open tabs:\n");
            for (t, u) in &self.tabs {
                s.push_str(&format!("- {t} — {u}\n"));
            }
        }
        if let Some((path, text)) = &self.editor {
            let text=if redact{crate::secrets::scrub(text).text}else{text.clone()};
            let text: String = text.chars().take(8000).collect();
            s.push_str(&format!("\nThe file open in the editor is {path}:\n{text}\n"));
        }
        if let Some(m) = &self.memory {
            if !m.trim().is_empty() {
                s.push_str(&format!("\nThings to remember about this user:\n{m}\n"));
            }
        }
        s
    }
}

fn memory_path() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("memory.md")
}

pub fn read_memory() -> String {
    crate::protected_state::read_text(&memory_path()).unwrap_or_default()
}

pub fn remember(line: &str) {
    let mut m = read_memory();
    if !m.is_empty() && !m.ends_with('\n') {
        m.push('\n');
    }
    m.push_str(&format!("- {}\n", line.trim()));
    let _ = std::fs::create_dir_all(memory_path().parent().unwrap());
    let _ = crate::protected_state::write(&memory_path(), m.as_bytes());
}

impl App {
    /// Gather everything the lit chips ask for, except the page (async).
    pub(crate) fn gather_context(&mut self, on: &[Ctx]) -> Gathered {
        let mut g = Gathered::default();
        let os = if cfg!(windows) { "Windows 11" } else if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
        let active = self.active;
        let Some(tab) = self.tabs.get(active) else { return g };
        // Shell + block: the tab's shell (the left pane, or the focused one).
        let term = match (&tab.left, tab.right.as_ref()) {
            (Pane::Term(t), _) => Some(t),
            (_, Some(Pane::Term(t))) => Some(t),
            _ => None,
        };
        if let Some(t) = term {
            if on.contains(&Ctx::Shell) {
                let profile = self.profiles.get(t.profile).map(|p| p.name.clone()).unwrap_or_else(|| "shell".into());
                g.shell = Some((profile, t.cwd.clone().unwrap_or_default(), os.into()));
            }
            if on.contains(&Ctx::Block) {
                let blocks = t.blocks();
                let b = t.block_sel.and_then(|s| blocks.iter().find(|b| b.start == s)).or(blocks.last());
                if let Some(b) = b {
                    g.block = Some((b.cmd.clone(), t.block_output_text(b.start), b.exit));
                }
            }
        }
        if on.contains(&Ctx::Tabs) {
            g.tabs = self
                .tabs
                .iter()
                .filter(|t| t.peek.is_none())
                .map(|t| {
                    let url = match &t.left {
                        Pane::Web(w) => w.tab.shared.borrow().url.clone(),
                        Pane::Term(t) => t.cwd.clone().unwrap_or_default(),
                        Pane::Editor(e) => e.buf().and_then(|b| b.path.as_ref()).map(|p| p.display().to_string()).unwrap_or_default(),
                        _ => String::new(),
                    };
                    (t.title(), url)
                })
                .collect();
        }
        if on.contains(&Ctx::Editor) {
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Editor(e) = p {
                    if let Some(b) = e.buf() {
                        g.editor = Some((b.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "untitled".into()), b.text.to_string()));
                    }
                }
            }
        }
        if on.contains(&Ctx::Memory) {
            g.memory = Some(read_memory());
        }
        g
    }

    /// Ask the split's page for its text; the id to wait on, if there is a page.
    pub(crate) fn request_page_text(&mut self) -> Option<(i32, String, String)> {
        let tab = self.tabs.get(self.active)?;
        for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
            if let Pane::Web(w) = p {
                let (title, url) = {
                    let s = w.tab.shared.borrow();
                    (s.title.clone(), s.url.clone())
                };
                let id = w.tab.eval_reply(crate::reader::EXTRACT_JS);
                return Some((id, title, url));
            }
        }
        None
    }

    /// The page's text, once its reply is in: the reader's blocks joined.
    pub(crate) fn take_page_text(&mut self, id: i32) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
            if let Pane::Web(w) = p {
                if let Some(v) = w.tab.take_reply(id) {
                    let json = v.pointer("/result/result/value").and_then(|x| x.as_str()).unwrap_or("");
                    let text = crate::reader::Article::parse(json)
                        .map(|a| {
                            a.blocks
                                .iter()
                                .filter_map(|b| match b {
                                    crate::reader::Block::Heading(_, s) | crate::reader::Block::Para(s) | crate::reader::Block::Pre(s) | crate::reader::Block::Item(s) | crate::reader::Block::Quote(s) => Some(s.clone()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    return Some(text);
                }
            }
        }
        None
    }

    /// The chips row above the field: the context icons, then the skills.
    /// Returns the row's height and the hits it made.
    pub(crate) fn draw_ask_chips(&mut self, scene: &mut Scene, ctx: &[Ctx], x0: f32, y: f32, w: f32) -> (f32, Vec<(Rect, crate::ask::AskHit)>) {
        let mut hits = Vec::new();
        let t = self.theme.clone();
        let ink = t.ink;
        let (mx, my) = self.mouse;
        let isz = self.px(14.0);
        let gap = self.px(10.0);
        let h = self.px(24.0);
        let mut x = x0;
        for c in Ctx::ALL {
            let on = ctx.contains(&c);
            let hit = Rect::new(x - self.px(4.0), y, isz + self.px(8.0), h);
            let color = if on { self.surface.signal } else { t.dim };
            self.icon_button(scene, c.icon(), isz, x, y + (h - isz) / 2.0, color, hit, hover_key("askctx", c as usize), IconMotion::Pop);
            if hit.contains(mx, my) {
                self.tip_words(hit, c.words());
            }
            hits.push((hit, crate::ask::AskHit::Ctx(c)));
            x += isz + gap;
        }
        // Skills, as text chips after a rule.
        let skills = self.skills.clone();
        if !skills.is_empty() {
            x += self.px(4.0);
            scene.vline(x, y + self.px(5.0), h - self.px(10.0), self.px(1.0), fade(ink, 0.4));
            x += self.px(10.0);
            let label = self.label();
            for (i, sk) in skills.iter().enumerate() {
                let word = sk.name.to_uppercase();
                let ww = self.fonts.measure(label, &word);
                if x + ww > x0 + w {
                    break;
                }
                let hit = Rect::new(x - self.px(4.0), y, ww + self.px(8.0), h);
                let hot = hit.contains(mx, my);
                self.fonts.draw(scene, Style { color: if hot { ink } else { t.dim }, ..label }, x, y + h / 2.0 + self.px(5.0), &word);
                if hot {
                    let words = sk.prompt.chars().take(60).collect::<String>();
                    self.tip_words(hit, &words);
                }
                hits.push((hit, crate::ask::AskHit::Skill(i)));
                x += ww + gap;
            }
        }
        (h, hits)
    }
}

#[allow(dead_code)]
fn _unused(_: Instant) {}
