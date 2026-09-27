//! Prompt language servers contribute to the same Code menu as history
//! and Git. Async answers are bound to the exact command and caret.
use crate::app::{App, Pane, TermPane};
use crate::editor::Pending;
use crate::prompt_code::{CodeItem, CodeMenu, PromptLine, PromptStamp};
use crate::settings::PromptLsp;
use nus_lsp::lsp_types::{
    CompletionItem, CompletionTextEdit, Diagnostic, InsertTextFormat, Position, Url,
};
use nus_render::{text::Style, Rect, Scene};
use std::time::Instant;

pub struct LineLsp {
    pub key: String,
    pub uri: Url,
    pub opened: bool,
    pub sent: String,
    pub changed_at: Instant,
    pub dirty: bool,
    pub diags: Vec<Diagnostic>,
    pub items: Vec<CompletionItem>,
    pub snapshot: Option<PromptStamp>,
    // Kept for the existing screenshot diagnostic probe.
    pub menu: bool,
    pub ghost: Option<String>,
}

pub fn word_at_end(text: &str) -> String {
    text.chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\'))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
pub fn lsp_wants(text: &str) -> bool {
    if crate::git_complete::is_git(text) || text.ends_with(char::is_whitespace) {
        return false;
    }
    let word = word_at_end(text);
    let before = text[..text.len() - word.len()].chars().last();
    if before == Some('$')
        || word.contains('/')
        || word.contains('\\')
        || word.starts_with('.')
        || word.starts_with('~')
    {
        return true;
    }
    matches!(
        crate::predict::tokens(text).last(),
        Some((_, _, crate::predict::Tok::Command))
    )
}

fn byte_at(text: &str, pos: Position) -> Option<usize> {
    if pos.line != 0 {
        return None;
    }
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == pos.character {
            return Some(byte);
        }
        units += ch.len_utf16() as u32;
        if units > pos.character {
            return None;
        }
    }
    (units == pos.character).then_some(text.len())
}

pub fn code_item(line: &PromptLine, item: &CompletionItem) -> Option<CodeItem> {
    if item.insert_text_format == Some(InsertTextFormat::SNIPPET)
        || item
            .additional_text_edits
            .as_ref()
            .is_some_and(|e| !e.is_empty())
    {
        return None;
    }
    let (range, insert) = match &item.text_edit {
        Some(CompletionTextEdit::Edit(e)) => (
            byte_at(&line.text, e.range.start)?..byte_at(&line.text, e.range.end)?,
            e.new_text.clone(),
        ),
        Some(CompletionTextEdit::InsertAndReplace(e)) => (
            byte_at(&line.text, e.replace.start)?..byte_at(&line.text, e.replace.end)?,
            e.new_text.clone(),
        ),
        None => {
            let word = word_at_end(line.prefix());
            let start = line.caret - word.len();
            let mut end = line.caret;
            for (i, ch) in line.text[line.caret..].char_indices() {
                if !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '\\')) {
                    break;
                }
                end = line.caret + i + ch.len_utf8();
            }
            (
                start..end,
                item.insert_text
                    .clone()
                    .unwrap_or_else(|| item.label.clone()),
            )
        }
    };
    if range.start > line.caret
        || range.end < line.caret
        || range.start > range.end
        || insert.is_empty()
        || !crate::prompt_code::safe_text(&insert)
    {
        return None;
    }
    let word_range = line.token_range();
    if range.start < word_range.start || range.end > word_range.end {
        return None;
    }
    if !insert.starts_with(&line.text[range.start..line.caret])
        || insert == line.text[range.clone()]
    {
        return None;
    }
    Some(CodeItem {
        label: item.label.chars().filter(|c| !c.is_control()).collect(),
        detail: item
            .detail
            .as_deref()
            .unwrap_or("Code")
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect(),
        range,
        insert,
    })
}

impl App {
    pub(crate) fn prompt_lsp_tick(&mut self) {
        let mode = self.behavior.prompt_lsp;
        let active = self.active;
        let (line, profile, missing) = {
            let Some(Pane::Term(t)) = self.tabs.get_mut(active).map(|t| t.focused()) else {
                return;
            };
            let line = t.prompt_line();
            if t.code_menu
                .as_ref()
                .is_some_and(|m| line.as_ref().is_none_or(|l| !l.matches(&m.stamp)))
            {
                t.code_menu = None;
                self.dirty = true;
            }
            if t.prompt_edit_pending
                .as_ref()
                .is_some_and(|e| line.as_ref().is_none_or(|l| !e.waiting(l)))
            {
                t.prompt_edit_pending = None;
            }
            if mode == PromptLsp::Off || !self.behavior.shell_integration {
                if let Some(l) = t.plsp.take() {
                    if let Some(s) = self.lsp.map.get(&l.key) {
                        s.client.did_close(l.uri);
                    }
                }
                t.plsp_tried = false;
                return;
            }
            (line, t.profile, t.plsp.is_none() && !t.plsp_tried)
        };
        if missing {
            self.prompt_lsp_start(active, profile);
        }
        let Some(Pane::Term(t)) = self.tabs.get_mut(active).map(|t| t.focused()) else {
            return;
        };
        let Some(l) = t.plsp.as_mut() else { return };
        let Some(line) = line else {
            if !l.diags.is_empty() || !l.items.is_empty() || l.ghost.is_some() || l.menu {
                l.diags.clear();
                l.items.clear();
                l.ghost = None;
                l.menu = false;
                l.snapshot = None;
                self.dirty = true;
            }
            return;
        };
        let stamp = line.stamp();
        if l.snapshot.as_ref() != Some(&stamp) {
            l.snapshot = Some(stamp.clone());
            l.changed_at = crate::clock::now();
            l.dirty = true;
            l.diags.clear();
            l.items.clear();
            l.ghost = None;
            l.menu = false;
            self.dirty = true;
        }
        if !l.dirty || crate::clock::since(l.changed_at).as_millis() < 120 {
            return;
        }
        let Some(server) = self.lsp.map.get(&l.key) else {
            return;
        };
        if !server.client.is_ready() {
            return;
        }
        let language = if l.uri.as_str().ends_with(".ps1") {
            "powershell"
        } else {
            "shellscript"
        };
        if !l.opened {
            server.client.did_open(l.uri.clone(), language, &line.text);
            l.opened = true;
        } else if l.sent != line.text {
            server.client.did_change(l.uri.clone(), &line.text);
        }
        l.sent = line.text.clone();
        l.dirty = false;
        if lsp_wants(line.prefix()) && !word_at_end(line.prefix()).is_empty() {
            let pos = Position::new(0, line.prefix().encode_utf16().count() as u32);
            let id = server.client.completion(l.uri.clone(), pos, None);
            self.lsp.pending.insert(
                (l.key.clone(), id),
                Pending::PromptCompletion {
                    uri: l.uri.clone(),
                    stamp,
                },
            );
        }
    }

    fn prompt_lsp_start(&mut self, ti: usize, profile: usize) {
        let kind = self
            .profiles
            .get(profile)
            .map(|p| crate::shell::kind_of(&p.program))
            .unwrap_or(crate::shell::Kind::Other);
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
        let Some(server) = nus_lsp::registry::SERVERS
            .iter()
            .find(|s| s.command == command)
        else {
            return;
        };
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let key = self.lsp_key_for_server(server, &home, true);
        let Some(Pane::Term(t)) = self.tabs.get_mut(ti).map(|t| t.focused()) else {
            return;
        };
        t.plsp_tried = true;
        let Some(key) = key else { return };
        let path =
            std::env::temp_dir().join(format!("nus-prompt-{}.{}", t.pty.pid().unwrap_or(0), ext));
        let Ok(uri) = Url::from_file_path(&path) else {
            return;
        };
        t.plsp = Some(LineLsp {
            key,
            uri,
            opened: false,
            sent: String::new(),
            changed_at: crate::clock::now(),
            dirty: false,
            diags: Vec::new(),
            items: Vec::new(),
            snapshot: None,
            menu: false,
            ghost: None,
        });
    }

    pub(crate) fn prompt_lsp_items(
        &mut self,
        uri: &Url,
        stamp: &PromptStamp,
        items: Vec<CompletionItem>,
    ) {
        if self.behavior.prompt_lsp == PromptLsp::Off {
            return;
        }
        for tab in &mut self.tabs {
            for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(t) = pane else { continue };
                let Some(line) = t.prompt_line() else {
                    continue;
                };
                let Some(l) = t.plsp.as_mut() else { continue };
                if &l.uri != uri {
                    continue;
                }
                if !line.matches(stamp) || l.snapshot.as_ref() != Some(stamp) || l.dirty {
                    return;
                }
                let mut items: Vec<_> = items
                    .into_iter()
                    .filter(|it| code_item(&line, it).is_some())
                    .collect();
                items.sort_by(|a, b| {
                    a.sort_text
                        .as_deref()
                        .unwrap_or(&a.label)
                        .cmp(b.sort_text.as_deref().unwrap_or(&b.label))
                });
                items.truncate(40);
                l.items = items;
                l.ghost = l
                    .items
                    .first()
                    .and_then(|it| code_item(&line, it))
                    .and_then(|it| {
                        it.insert
                            .strip_prefix(&line.text[it.range])
                            .map(str::to_string)
                    });
                l.menu = false;
                if self.behavior.prompt_lsp == PromptLsp::Menu
                    && t.code_menu.is_none()
                    && !t.prompt_history.active()
                    && t.prompt_quiet.as_ref().is_none_or(|s| !line.matches(s))
                {
                    let items = t.code_items(&line);
                    if !items.is_empty() {
                        t.code_menu = Some(CodeMenu::new(line.stamp(), items));
                    }
                }
                self.dirty = true;
                return;
            }
        }
    }

    pub(crate) fn prompt_lsp_diags(&mut self, uri: &Url, diags: Vec<Diagnostic>) -> bool {
        for tab in &mut self.tabs {
            for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Term(t) = pane else { continue };
                let current = t.prompt_line();
                let Some(l) = t.plsp.as_mut() else { continue };
                if &l.uri == uri {
                    if !l.dirty
                        && current.as_ref().is_some_and(|line| {
                            l.snapshot.as_ref().is_some_and(|s| line.matches(s))
                        })
                    {
                        l.diags = diags;
                        self.dirty = true;
                    }
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn draw_prompt_lsp(&mut self, scene: &mut Scene, p: &TermPane, line: &PromptLine) {
        let Some(l) = p
            .plsp
            .as_ref()
            .filter(|l| !l.dirty && l.snapshot.as_ref().is_some_and(|s| line.matches(s)))
        else {
            return;
        };
        let (cw, ch) = p.grid.cell_size();
        let theme = self.theme.clone();
        for diag in &l.diags {
            let (Some(start), Some(end)) = (
                byte_at(&line.text, diag.range.start),
                byte_at(&line.text, diag.range.end),
            ) else {
                continue;
            };
            let color = crate::theme_edit::from_rgb(theme.ansi[crate::editor::severity_ansi(diag)]);
            for cell in line
                .cells
                .iter()
                .filter(|c| c.byte >= start && c.byte < end.max(start + 1))
            {
                let x = p.origin.0 + cell.col as f32 * cw;
                let y = p.origin.1 + cell.row as f32 * ch;
                let rect = Rect::new(x, y, cell.width as f32 * cw, ch);
                let mut ux = x;
                while ux < rect.right() {
                    scene.rect(
                        Rect::new(ux, y + ch - self.px(2.0), self.px(2.0), self.px(1.5)),
                        color,
                    );
                    ux += self.px(4.0);
                }
                if rect.contains(self.mouse.0, self.mouse.1) && p.code_menu.is_none() {
                    let style = Style {
                        font: p.grid.font,
                        px: p.grid.px,
                        color: theme.ink,
                        tracking: 0.0,
                    };
                    let msg = diag.message.lines().next().unwrap_or("");
                    let width = (self.fonts.measure(style, msg) + self.px(16.0)).min(p.rect.w);
                    let height = ch + self.px(8.0);
                    let bx = x.min(p.rect.right() - width).max(p.rect.x);
                    let by = if y - height > p.rect.y {
                        y - height
                    } else {
                        y + ch
                    };
                    let r = Rect::new(bx, by, width, height);
                    scene.rect(r, theme.paper);
                    scene.outline(r, self.px(1.0), color);
                    let old = scene.clip();
                    scene.layer(Some(r));
                    self.fonts.draw(
                        scene,
                        style,
                        bx + self.px(8.0),
                        by + self.px(4.0) + p.grid.metrics.baseline,
                        msg,
                    );
                    scene.layer(old);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_positions_and_insertion_text_are_respected() {
        assert_eq!(byte_at("a界😀z", Position::new(0, 4)), Some(8));
        assert_eq!(byte_at("a界😀z", Position::new(0, 3)), None);
        let mut term = nus_vt::Term::new(80, 5, 20);
        term.advance("\x1b]133;B\x07ec".as_bytes());
        let line = PromptLine::read(&term).unwrap();
        let item = CompletionItem {
            label: "echo (shell)".into(),
            insert_text: Some("echo".into()),
            ..CompletionItem::default()
        };
        assert_eq!(code_item(&line, &item).unwrap().insert, "echo");
        assert!(code_item(
            &line,
            &CompletionItem {
                insert_text: Some("echo\nrm x".into()),
                ..item.clone()
            }
        )
        .is_none());
        assert!(code_item(
            &line,
            &CompletionItem {
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..item
            }
        )
        .is_none());
    }
    #[test]
    fn replacement_preserves_following_arguments() {
        let mut term = nus_vt::Term::new(80, 5, 20);
        term.advance(b"\x1b]133;B\x07echo ./sr --all\x1b[6D");
        let line = PromptLine::read(&term).unwrap();
        let item = CompletionItem {
            label: "./src".into(),
            ..CompletionItem::default()
        };
        let result = code_item(&line, &item).unwrap();
        assert_eq!(result.range, 5..9);
        assert_eq!(&line.text[result.range.end..], " --all");
    }
    #[test]
    fn trailing_word() {
        assert_eq!(word_at_end("git sta"), "sta");
        assert_eq!(word_at_end("ls ./src/ma"), "./src/ma");
        assert!(!lsp_wants("git push"));
        assert!(!lsp_wants("echo ordinary"));
        assert!(lsp_wants("echo $va"));
    }
}
