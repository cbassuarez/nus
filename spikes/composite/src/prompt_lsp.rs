//! The prompt line has a language server too: bash-language-server or
//! PowerShell Editor Services by shell, fed the typed command as a
//! one-line document. QUIET (the default) underlines a diagnostic and
//! rides completions on the ghost prediction (Tab accepts); MENU shows a
//! small list under the caret; OFF is off. Nothing here blocks: text goes
//! out debounced, answers come back through the app's LSP host.

use std::time::Instant;

use nus_lsp::lsp_types::{CompletionItem, Diagnostic, Position, Url};
use nus_render::text::Style;
use nus_render::{Rect, Scene};
use winit::event::ElementState;
use winit::keyboard::{Key as WKey, NamedKey};

use crate::app::{fade, App, Pane, TermPane};
use crate::editor::Pending;
use crate::settings::PromptLsp;
use nus_render::theme::metric as m;

pub struct LineLsp {
    pub key: String,
    pub uri: Url,
    pub opened: bool,
    /// The last text the server saw, and when the line last changed.
    pub sent: String,
    pub changed_at: Instant,
    pub dirty: bool,
    pub diags: Vec<Diagnostic>,
    /// Completions for the word at the end of the line.
    pub items: Vec<CompletionItem>,
    pub sel: usize,
    pub word: String,
    pub menu: bool,
    pub ghost: Option<String>,
}

/// The trailing word being completed: letters, digits, `_`, `-`, `.`, `/`.
pub fn word_at_end(text: &str) -> String {
    text.chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

impl App {
    /// Once a loop: keep the focused shell's line in step with its server.
    pub(crate) fn prompt_lsp_tick(&mut self) {
        let mode = self.behavior.prompt_lsp;
        let active = self.active;
        // Which shell and what it says.
        let (typed, profile, plsp_missing) = {
            let Some(tab) = self.tabs.get_mut(active) else { return };
            let Pane::Term(t) = tab.focused() else { return };
            if mode == PromptLsp::Off {
                if let Some(l) = t.plsp.take() {
                    if let Some(s) = self.lsp.map.get(&l.key) {
                        s.client.did_close(l.uri);
                    }
                }
                t.plsp_tried = false;
                return;
            }
            let typed = t.typed().map(|(_, s)| s);
            (typed, t.profile, t.plsp.is_none() && !t.plsp_tried)
        };
        if plsp_missing {
            self.prompt_lsp_start(active, profile);
        }
        let Some(tab) = self.tabs.get_mut(active) else { return };
        let Pane::Term(t) = tab.focused() else { return };
        let Some(l) = t.plsp.as_mut() else { return };
        let Some(text) = typed else {
            // Not at a prompt: nothing to show.
            if !l.diags.is_empty() || !l.items.is_empty() || l.ghost.is_some() || l.menu {
                l.diags.clear();
                l.items.clear();
                l.ghost = None;
                l.menu = false;
                self.dirty = true;
            }
            return;
        };
        if text != l.sent && !l.dirty {
            l.dirty = true;
            l.changed_at = Instant::now();
            // Stale answers go the moment the line moves.
            l.ghost = None;
            l.items.clear();
            l.menu = false;
            self.dirty = true;
        }
        if l.dirty && l.changed_at.elapsed().as_millis() >= 120 && text != l.sent {
            let Some(s) = self.lsp.map.get(&l.key) else { return };
            if !s.client.is_ready() {
                return;
            }
            let language = if l.uri.as_str().ends_with(".ps1") { "powershell" } else { "shellscript" };
            if !l.opened {
                s.client.did_open(l.uri.clone(), language, &text);
                l.opened = true;
            } else {
                s.client.did_change(l.uri.clone(), &text);
            }
            l.sent = text.clone();
            l.dirty = false;
            l.word = word_at_end(&text);
            if !l.word.is_empty() {
                let pos = Position::new(0, text.encode_utf16().count() as u32);
                let id = s.client.completion(l.uri.clone(), pos, None);
                self.lsp.pending.insert((l.key.clone(), id), Pending::PromptCompletion { uri: l.uri.clone() });
            }
        } else if l.dirty && text == l.sent {
            l.dirty = false;
        }
    }

    /// Start (or find) the server for a shell's prompt line.
    fn prompt_lsp_start(&mut self, ti: usize, profile: usize) {
        let kind = self.profiles.get(profile).map(|p| crate::shell::kind_of(&p.program)).unwrap_or(crate::shell::Kind::Other);
        let (command, ext) = match kind {
            crate::shell::Kind::PowerShell => ("powershell-editor-services", "ps1"),
            crate::shell::Kind::Bash | crate::shell::Kind::Zsh => ("bash-language-server", "sh"),
            _ => {
                if let Some(Pane::Term(t)) = self.tabs.get_mut(ti).map(|t| t.focused()) {
                    t.plsp_tried = true;
                }
                return;
            }
        };
        let Some(server) = nus_lsp::registry::SERVERS.iter().find(|s| s.command == command) else { return };
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(std::path::PathBuf::from).unwrap_or_default();
        let key = self.lsp_key_for_server(server, &home, true);
        let Some(Pane::Term(t)) = self.tabs.get_mut(ti).map(|t| t.focused()) else { return };
        t.plsp_tried = true;
        let Some(key) = key else { return };
        let id = t.pty.pid().unwrap_or(0);
        let path = std::env::temp_dir().join(format!("nus-prompt-{id}.{ext}"));
        let Ok(uri) = Url::from_file_path(&path) else { return };
        t.plsp = Some(LineLsp {
            key,
            uri,
            opened: false,
            sent: String::new(),
            changed_at: Instant::now(),
            dirty: false,
            diags: Vec::new(),
            items: Vec::new(),
            sel: 0,
            word: String::new(),
            menu: false,
            ghost: None,
        });
    }

    /// The server answered a prompt completion: filter to the word, and
    /// ghost or list it per the setting.
    pub(crate) fn prompt_lsp_items(&mut self, uri: &Url, items: Vec<CompletionItem>) {
        let mode = self.behavior.prompt_lsp;
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(t) = p else { continue };
                let Some(l) = t.plsp.as_mut() else { continue };
                if &l.uri != uri {
                    continue;
                }
                let w = l.word.to_lowercase();
                let mut items: Vec<CompletionItem> = items
                    .into_iter()
                    .filter(|it| {
                        let label = it.filter_text.as_deref().unwrap_or(&it.label).to_lowercase();
                        label.starts_with(&w) && label != w
                    })
                    .collect();
                items.sort_by(|a, b| a.sort_text.as_deref().unwrap_or(&a.label).cmp(b.sort_text.as_deref().unwrap_or(&b.label)));
                items.truncate(40);
                l.sel = 0;
                match mode {
                    PromptLsp::Quiet => {
                        l.ghost = items.first().map(|it| it.label[l.word.len().min(it.label.len())..].to_string()).filter(|s| !s.is_empty());
                        l.items = items;
                        l.menu = false;
                    }
                    PromptLsp::Menu => {
                        l.menu = !items.is_empty();
                        l.ghost = None;
                        l.items = items;
                    }
                    PromptLsp::Off => {}
                }
                self.dirty = true;
                return;
            }
        }
    }

    /// Diagnostics for a prompt line.
    pub(crate) fn prompt_lsp_diags(&mut self, uri: &Url, diags: Vec<Diagnostic>) -> bool {
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(t) = p else { continue };
                let Some(l) = t.plsp.as_mut() else { continue };
                if &l.uri == uri {
                    l.diags = diags;
                    self.dirty = true;
                    return true;
                }
            }
        }
        false
    }

    /// Keys while a completion menu is up, or Tab on a ghost. Returns true
    /// when consumed.
    pub(crate) fn prompt_lsp_key(&mut self, ev: &winit::event::KeyEvent) -> bool {
        if ev.state != ElementState::Pressed || self.behavior.prompt_lsp == PromptLsp::Off {
            return false;
        }
        if self.mods.control_key() || self.mods.alt_key() || self.mods.super_key() {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let Pane::Term(t) = tab.focused() else { return false };
        let Some(l) = t.plsp.as_mut() else { return false };
        let key = &ev.logical_key;
        if l.menu {
            match key {
                WKey::Named(NamedKey::ArrowDown) => {
                    l.sel = (l.sel + 1).min(l.items.len().saturating_sub(1));
                    self.dirty = true;
                    return true;
                }
                WKey::Named(NamedKey::ArrowUp) => {
                    l.sel = l.sel.saturating_sub(1);
                    self.dirty = true;
                    return true;
                }
                WKey::Named(NamedKey::Escape) => {
                    l.menu = false;
                    self.dirty = true;
                    return true;
                }
                WKey::Named(NamedKey::Tab) => {
                    if let Some(it) = l.items.get(l.sel) {
                        let rest = it.label[l.word.len().min(it.label.len())..].to_string();
                        let _ = t.pty.write(rest.as_bytes());
                    }
                    l.menu = false;
                    l.items.clear();
                    self.dirty = true;
                    return true;
                }
                _ => {
                    // Any other key: the shell gets it; the menu closes and
                    // comes back with the next answer.
                    l.menu = false;
                    self.dirty = true;
                    return false;
                }
            }
        }
        if matches!(key, WKey::Named(NamedKey::Tab)) {
            if let Some(g) = l.ghost.take() {
                let _ = t.pty.write(g.as_bytes());
                l.items.clear();
                self.dirty = true;
                return true;
            }
        }
        false
    }

    /// Drawn over the prompt row after the coloured tokens: the ghost, a
    /// diagnostic's underline (and its message when the pointer rests on
    /// it), the menu.
    pub(crate) fn draw_prompt_lsp(&mut self, scene: &mut Scene, p: &TermPane, col0: usize, typed: &str, history_ghost: bool) {
        let Some(l) = p.plsp.as_ref() else { return };
        let (cw, ch) = p.grid.cell_size();
        let cur = p.term.cursor();
        let y = p.origin.1 + cur.row as f32 * ch;
        let base = y + p.grid.metrics.baseline;
        let font = p.grid.font;
        let px = p.grid.px;
        let t = self.theme.clone();
        let ansi = |i: usize| crate::theme_edit::from_rgb(t.ansi[i]);
        let mono = Style { font, px, color: t.ink, tracking: 0.0 };
        let (mx, my) = self.mouse;
        // The ghost, unless history already had one.
        if !history_ghost {
            if let Some(g) = &l.ghost {
                let x = p.origin.0 + cur.col as f32 * cw;
                let cols_left = p.term.cols().saturating_sub(cur.col);
                let g: String = g.chars().take(cols_left).collect();
                self.fonts.draw(scene, Style { color: fade(t.ink, 0.38), ..mono }, x, base, &g);
            }
        }
        // Diagnostics on the line.
        let n = typed.chars().count();
        for d in &l.diags {
            if d.range.start.line != 0 {
                continue;
            }
            let a = nus_lsp::offset_of(typed, d.range.start).min(n);
            let z = nus_lsp::offset_of(typed, d.range.end).clamp(a + 1, n.max(a + 1));
            let sev = crate::editor::severity_ansi(d);
            let uy = y + ch - self.px(2.0);
            let mut ux = p.origin.0 + (col0 + a) as f32 * cw;
            let end = p.origin.0 + (col0 + z) as f32 * cw;
            let span = Rect::new(ux, y, end - ux, ch);
            while ux < end {
                scene.rect(Rect::new(ux, uy, self.px(2.0), self.px(1.5)), ansi(sev));
                ux += self.px(4.0);
            }
            if span.contains(mx, my) {
                let msg = d.message.lines().next().unwrap_or("").to_string();
                let w = self.fonts.measure(mono, &msg) + self.px(16.0);
                let h = ch + self.px(8.0);
                let bx = span.x.min(p.rect.right() - w - self.px(8.0)).max(p.rect.x);
                let by = if y - h - self.px(4.0) > p.rect.y { y - h - self.px(4.0) } else { y + ch + self.px(4.0) };
                let r = Rect::new(bx, by, w, h);
                scene.rect(r, t.paper);
                scene.outline(r, self.px(m::HAIRLINE), ansi(sev));
                self.fonts.draw(scene, mono, bx + self.px(8.0), by + self.px(4.0) + p.grid.metrics.baseline, &msg);
            }
        }
        // The menu.
        if l.menu && !l.items.is_empty() {
            let shown = l.items.len().min(8);
            let row_h = ch + self.px(4.0);
            let mono_dim = Style { color: t.dim, ..mono };
            let wmax = l
                .items
                .iter()
                .take(shown)
                .map(|i| self.fonts.measure(mono, &i.label) + i.detail.as_ref().map(|d| self.fonts.measure(mono_dim, d) + self.px(16.0)).unwrap_or(0.0))
                .fold(0.0f32, f32::max)
                .min(p.rect.w * 0.6);
            let bw = wmax + self.px(20.0);
            let bh = shown as f32 * row_h + self.px(6.0);
            let word_cols = l.word.chars().count();
            let bx = (p.origin.0 + (cur.col.saturating_sub(word_cols)) as f32 * cw).min(p.rect.right() - bw - self.px(8.0)).max(p.rect.x);
            let below = y + ch + self.px(2.0);
            let by = if below + bh > p.rect.bottom() { y - bh - self.px(2.0) } else { below };
            let r = Rect::new(bx, by, bw, bh);
            scene.rect(r, t.paper);
            scene.outline(r, self.px(m::HAIRLINE), t.ink);
            let mut yy = by + self.px(3.0);
            for (k, it) in l.items.iter().enumerate().take(shown) {
                if k == l.sel {
                    scene.rect(Rect::new(bx, yy, bw, row_h), fade(self.surface.signal, 0.18));
                }
                let b = yy + self.px(2.0) + p.grid.metrics.baseline;
                let lw = self.fonts.draw(scene, mono, bx + self.px(10.0), b, &it.label);
                if let Some(d) = &it.detail {
                    let dw = self.fonts.measure(mono_dim, d);
                    if lw + dw + self.px(30.0) < bw {
                        self.fonts.draw(scene, mono_dim, bx + bw - self.px(10.0) - dw, b, d);
                    }
                }
                yy += row_h;
            }
            let hint = "TAB ACCEPTS · ESC";
            let hw = self.fonts.measure(self.label(), hint);
            let dim = Style { color: t.dim, ..self.label() };
            self.fonts.draw(scene, dim, bx + bw - hw - self.px(10.0), by + bh + self.px(14.0), hint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_word() {
        assert_eq!(word_at_end("git sta"), "sta");
        assert_eq!(word_at_end("ls ./src/ma"), "./src/ma");
        assert_eq!(word_at_end("echo hi "), "");
        assert_eq!(word_at_end("ech"), "ech");
    }
}
