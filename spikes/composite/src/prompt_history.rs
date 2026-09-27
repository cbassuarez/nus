//! History stays at the prompt. Searching never writes to the shell;
//! accepting inserts a command, and looking at its output keeps the draft.
use nus_render::text::Style;
use nus_render::{Rect, Scene};
use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

use crate::app::{fade, App, Pane, TermPane};
use crate::prompt_code::PromptLine;

#[derive(Clone, Debug)]
struct Entry {
    command: String,
    block: Option<u64>,
    exit: Option<i32>,
}

#[derive(Clone, Copy)]
enum Hit {
    Choose(usize),
    Output,
    Close,
}

struct Search {
    draft: PromptLine,
    query: String,
    cursor: crate::field::Cursor,
    entries: Vec<Entry>,
    selected: usize,
    viewing: bool,
    old_block: Option<u64>,
    unfolded: Vec<(u64, u64)>,
}

#[derive(Default)]
struct Recall {
    draft: String,
    matches: Vec<String>,
    at: Option<usize>,
    expected: String,
    pending_from: Option<String>,
    start: Option<(u64, usize)>,
}

impl Recall {
    fn begin(draft: &str, entries: &[Entry], start: (u64, usize)) -> Self {
        Self {
            draft: draft.into(),
            matches: entries
                .iter()
                .filter(|e| e.command != draft && e.command.starts_with(draft))
                .map(|e| e.command.clone())
                .collect(),
            expected: draft.into(),
            start: Some(start),
            ..Self::default()
        }
    }

    fn step(&mut self, up: bool) -> Option<String> {
        self.at = match (self.at, up) {
            (None, true) if !self.matches.is_empty() => Some(0),
            (Some(i), true) => Some((i + 1).min(self.matches.len() - 1)),
            (Some(0), false) => None,
            (Some(i), false) => Some(i - 1),
            _ => return None,
        };
        Some(
            self.at
                .map(|i| self.matches[i].clone())
                .unwrap_or_else(|| self.draft.clone()),
        )
    }
}

#[derive(Default)]
pub(crate) struct HistoryState {
    search: Option<Search>,
    recall: Option<Recall>,
    hits: Vec<(Rect, Hit)>,
}

impl HistoryState {
    pub(crate) fn active(&self) -> bool {
        self.search.is_some()
    }
    pub(crate) fn paste(&mut self, text: &str) -> bool {
        let Some(s) = self.search.as_mut() else {
            return false;
        };
        let at = s.cursor.at(&s.query);
        let (a, b) = s.cursor.range(&s.query).unwrap_or((at, at));
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(4000usize.saturating_sub(s.query.chars().count() - (b - a)))
            .collect();
        let start = crate::field::byte_at(&s.query, a);
        let end = crate::field::byte_at(&s.query, b);
        s.query.replace_range(start..end, &text);
        s.cursor.move_to(&s.query, a + text.chars().count(), false);
        s.selected = 0;
        true
    }
}

fn safe_command(text: &str) -> bool {
    !text.trim().is_empty() && crate::prompt_code::safe_text(text)
}

/// Recover a completed single logical command from shell marks. Soft wraps
/// join without adding whitespace; an actual continuation line is declined.
pub(crate) fn completed_command(term: &nus_vt::Term, start: u64) -> Option<String> {
    let marks = &term.marks;
    let a = marks
        .iter()
        .position(|m| m.line == start && m.kind == nus_vt::MarkKind::PromptStart)?;
    let b = marks
        .iter()
        .skip(a + 1)
        .take_while(|m| m.kind != nus_vt::MarkKind::PromptStart)
        .find(|m| m.kind == nus_vt::MarkKind::CommandStart)?;
    let c = marks
        .iter()
        .skip(a + 1)
        .take_while(|m| m.kind != nus_vt::MarkKind::PromptStart)
        .find(|m| m.kind == nus_vt::MarkKind::OutputStart)?;
    if c.line < b.line {
        return None;
    }
    let end_row = if c.col == 0 && c.line > b.line {
        c.line - 1
    } else {
        c.line
    };
    let mut text = String::new();
    for line in b.line..=end_row {
        let row = term.grid().row_abs(line)?;
        if line < end_row && !row.wrapped {
            return None;
        }
        // A wide glyph wrapping early leaves an unmarked padding blank in
        // VT. Decline it rather than save a space that was never typed.
        if line < end_row
            && row
                .cells
                .last()
                .is_some_and(|c| c.ch == ' ' && !c.flags.contains(nus_vt::Flags::WIDE_SPACER))
            && term
                .grid()
                .row_abs(line + 1)
                .and_then(|r| r.cells.first())
                .is_some_and(|c| c.flags.contains(nus_vt::Flags::WIDE))
        {
            return None;
        }
        let from = if line == b.line { b.col } else { 0 };
        let to = if line == c.line { c.col } else { term.cols() };
        for cell in row
            .cells
            .iter()
            .take(to)
            .skip(from)
            .filter(|cell| !cell.flags.contains(nus_vt::Flags::WIDE_SPACER))
        {
            text.push(if cell.ch == '\0' { ' ' } else { cell.ch });
        }
    }
    let text = text.trim_end().to_string();
    safe_command(&text).then_some(text)
}

fn entries(p: &TermPane) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for b in p.blocks().into_iter().rev().filter(|b| !b.running) {
        let Some(command) = completed_command(&p.term, b.start) else {
            continue;
        };
        if safe_command(&command) && seen.insert(command.clone()) {
            out.push(Entry {
                command,
                block: Some(b.start),
                exit: b.exit,
            });
        }
    }
    for command in p.history.iter().rev() {
        if safe_command(command) && seen.insert(command.clone()) {
            out.push(Entry {
                command: command.clone(),
                block: None,
                exit: None,
            });
        }
    }
    out.truncate(2000);
    out
}

fn matching(entries: &[Entry], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let text = e.command.to_lowercase();
            words.iter().all(|w| text.contains(w)).then_some(i)
        })
        .collect()
}

fn live(p: &mut TermPane) {
    let off = p.term.grid().display_offset;
    p.term.grid_mut().scroll_display(-(off as isize));
    p.trip = None;
    p.view_key = None;
}

fn close(p: &mut TermPane) {
    if let Some(s) = p.prompt_history.search.take() {
        if s.viewing {
            live(p);
        }
        p.block_sel = s.old_block;
        for fold in s.unfolded {
            if !p.folds.contains(&fold) {
                p.folds.push(fold);
            }
        }
        p.folds.sort();
        p.view_key = None;
    }
    p.prompt_history.hits.clear();
}

fn insert(p: &mut TermPane) {
    let Some(s) = p.prompt_history.search.as_ref() else {
        return;
    };
    let matches = matching(&s.entries, &s.query);
    let chosen = matches
        .get(s.selected)
        .map(|i| s.entries[*i].command.clone());
    let draft = s.draft.clone();
    close(p);
    let (Some(chosen), Some(line)) = (chosen, p.prompt_line()) else {
        return;
    };
    // The shell may redraw or change prompts while the search has focus.
    // Never replace a line other than the draft the user searched from.
    if line.start != draft.start || line.text != draft.text || line.caret != draft.caret {
        return;
    }
    let bytes = line.replace_bytes(&p.term, 0..line.text.len(), &chosen);
    let _ = p.pty.write(&bytes);
    p.line_ok = false;
    p.prompt_quiet = Some(line.stamp());
    p.prompt_edit_pending = Some(crate::prompt_code::PromptEdit::new(
        &line,
        0..line.text.len(),
        &chosen,
    ));
}

fn output(p: &mut TermPane) {
    let Some(s) = p.prompt_history.search.as_mut() else {
        return;
    };
    let matches = matching(&s.entries, &s.query);
    let Some(block) = matches.get(s.selected).and_then(|i| s.entries[*i].block) else {
        return;
    };
    s.viewing = true;
    p.block_sel = Some(block);
    // Reveal the original output in the existing transcript, including a
    // block that the user previously folded. No new output surface.
    let output = p
        .term
        .marks
        .iter()
        .find(|m| m.line >= block && m.kind == nus_vt::MarkKind::OutputStart)
        .map(|m| m.line)
        .unwrap_or(u64::MAX);
    s.unfolded
        .extend(p.folds.iter().filter(|(a, _)| *a == output).copied());
    p.folds.retain(|(a, _)| *a != output);
    p.term.grid_mut().scroll_to_abs(block);
    p.trip = None;
    p.view_key = None;
}

impl App {
    pub(crate) fn prompt_history_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if !self.behavior.shell_integration || ev.state != ElementState::Pressed {
            return false;
        }
        let (ctrl, alt, sup, shift) = (
            self.mods.control_key(),
            self.mods.alt_key(),
            self.mods.super_key(),
            self.mods.shift_key(),
        );
        let control = |c: &str| {
            let code = match c {
                "r" => KeyCode::KeyR,
                "o" => KeyCode::KeyO,
                "u" => KeyCode::KeyU,
                "v" => KeyCode::KeyV,
                _ => KeyCode::KeyC,
            };
            ctrl && !alt
                && !sup
                && !shift
                && (ev.physical_key == PhysicalKey::Code(code)
                    || matches!(&ev.logical_key, Key::Character(k) if k.eq_ignore_ascii_case(c)))
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let Pane::Term(p) = tab.focused() else {
            return false;
        };
        if p.term.modes().contains(nus_vt::Modes::ALT_SCREEN)
            || !p.term.at_prompt()
            || p.confirm_paste.is_some()
            || p.confirm_close.is_some()
        {
            close(p);
            p.prompt_history.recall = None;
            return false;
        }
        if p.prompt_history.active() {
            if matches!(
                ev.logical_key,
                Key::Named(
                    NamedKey::Shift
                        | NamedKey::Control
                        | NamedKey::Alt
                        | NamedKey::Super
                        | NamedKey::Meta
                        | NamedKey::CapsLock
                )
            ) {
                return true;
            }
            if control("o") {
                output(p);
                self.dirty = true;
                return true;
            }
            if matches!(ev.logical_key, Key::Named(NamedKey::Escape)) && !ctrl && !alt && !sup {
                close(p);
                self.dirty = true;
                return true;
            }
            if control("c") {
                close(p);
                self.dirty = true;
                return true;
            }
            let viewing = p.prompt_history.search.as_ref().is_some_and(|s| s.viewing);
            if viewing {
                live(p);
                if let Some(s) = p.prompt_history.search.as_mut() {
                    s.viewing = false;
                }
            }
            if shift && matches!(ev.logical_key, Key::Named(NamedKey::Tab)) {
                close(p);
                self.dirty = true;
                return false;
            }
            if !ctrl
                && !shift
                && !alt
                && !sup
                && matches!(ev.logical_key, Key::Named(NamedKey::Enter | NamedKey::Tab))
            {
                insert(p);
                self.dirty = true;
                return true;
            }
            let s = p.prompt_history.search.as_mut().unwrap();
            let count = matching(&s.entries, &s.query).len();
            let plain = !ctrl && !alt && !sup && !shift;
            if !plain
                && matches!(
                    ev.logical_key,
                    Key::Named(
                        NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::Enter | NamedKey::Tab
                    )
                )
            {
                close(p);
                self.dirty = true;
                return false;
            }
            if control("r") || plain && matches!(ev.logical_key, Key::Named(NamedKey::ArrowDown)) {
                if count > 0 {
                    s.selected = (s.selected + 1) % count;
                }
            } else if plain && matches!(ev.logical_key, Key::Named(NamedKey::ArrowUp)) {
                if count > 0 {
                    s.selected = (s.selected + count - 1) % count;
                }
            } else if control("u") {
                s.query.clear();
                s.cursor = crate::field::Cursor::default();
                s.selected = 0;
            } else {
                let took = crate::field::edit_at(&mut s.query, &mut s.cursor, ev, self.mods, 4000);
                if took.changed() {
                    s.selected = 0;
                }
                if !took.taken() && (ctrl || alt || sup) {
                    close(p);
                    self.dirty = true;
                    return false;
                }
            }
            self.dirty = true;
            return true;
        }
        if control("r") {
            let Some(draft) = p.prompt_line() else {
                return false;
            };
            if p.prompt_edit_pending
                .as_ref()
                .is_some_and(|edit| edit.waiting(&draft))
            {
                return true;
            }
            p.prompt_history.search = Some(Search {
                query: draft.text.clone(),
                cursor: crate::field::Cursor::default(),
                draft,
                entries: entries(p),
                selected: 0,
                viewing: false,
                old_block: p.block_sel,
                unfolded: Vec::new(),
            });
            p.prompt_history.recall = None;
            p.code_menu = None;
            self.dirty = true;
            return true;
        }
        // An open Code menu owns Up/Down. Native multiline editing and
        // modified arrows continue to the shell unchanged.
        let up = matches!(ev.logical_key, Key::Named(NamedKey::ArrowUp));
        let down = matches!(ev.logical_key, Key::Named(NamedKey::ArrowDown));
        if ctrl || alt || sup || shift || !(up || down) || p.code_menu.is_some() || p.sel.is_some()
        {
            if !matches!(
                ev.logical_key,
                Key::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super)
            ) {
                p.prompt_history.recall = None;
            }
            return false;
        }
        let Some(line) = p
            .prompt_line()
            .filter(|l| l.at_end() && !l.text.contains('\n'))
        else {
            return false;
        };
        if p.prompt_edit_pending
            .as_ref()
            .is_some_and(|edit| edit.waiting(&line))
        {
            return true;
        }
        if let Some(recall) = p.prompt_history.recall.as_mut() {
            // Echo can be delivered as erase, cursor movement, then text.
            // A repeat must wait for the complete command, not mistake a
            // transient blank line for a new prefix.
            if recall.start == Some(line.start)
                && recall.pending_from.is_some()
                && recall.expected != line.text
            {
                return true;
            }
            if recall.start != Some(line.start) || recall.expected != line.text {
                p.prompt_history.recall = None;
            } else {
                recall.pending_from = None;
            }
        }
        if p.prompt_history.recall.is_none() {
            if down {
                return false;
            }
            p.prompt_history.recall = Some(Recall::begin(&line.text, &entries(p), line.start));
        }
        let recall = p.prompt_history.recall.as_mut().unwrap();
        let Some(chosen) = recall.step(up) else {
            return false;
        };
        if chosen != line.text {
            let bytes = line.replace_bytes(&p.term, 0..line.text.len(), &chosen);
            let _ = p.pty.write(&bytes);
            recall.pending_from = Some(line.text.clone());
            recall.expected = chosen.clone();
            p.line_ok = false;
            p.prompt_quiet = Some(line.stamp());
            p.prompt_edit_pending = Some(crate::prompt_code::PromptEdit::new(
                &line,
                0..line.text.len(),
                &chosen,
            ));
        }
        self.dirty = true;
        true
    }

    pub(crate) fn prompt_history_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let Pane::Term(p) = tab.focused() else {
            return false;
        };
        let hit = p
            .prompt_history
            .hits
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, h)| *h);
        let Some(hit) = hit else { return false };
        match hit {
            Hit::Choose(i) => {
                if let Some(s) = p.prompt_history.search.as_mut() {
                    s.selected = i;
                }
                insert(p);
            }
            Hit::Output => output(p),
            Hit::Close => close(p),
        }
        self.dirty = true;
        true
    }

    pub(crate) fn draw_prompt_history(
        &mut self,
        scene: &mut Scene,
        p: &mut TermPane,
        paper: nus_render::Color,
    ) {
        p.prompt_history.hits.clear();
        if !self.behavior.shell_integration || !p.term.at_prompt() {
            close(p);
            return;
        }
        if self.prompt_composing {
            return;
        }
        let Some(s) = p.prompt_history.search.as_ref() else {
            return;
        };
        let matches = matching(&s.entries, &s.query);
        let (_, ch) = p.grid.cell_size();
        let pad = self.px(8.0);
        let x = p.origin.0;
        let w = (p.rect.right() - x - self.px(18.0)).max(1.0);
        let st = Style {
            font: p.grid.font,
            px: p.grid.px,
            color: self.theme.ink,
            tracking: self.behavior.typography.terminal_spacing * self.scale,
        };
        let dim = Style {
            color: self.theme.dim,
            ..st
        };
        let selected = s.selected.min(matches.len().saturating_sub(1));
        let row_h = ch + self.px(4.0);
        if s.viewing {
            let y = p.rect.bottom() - row_h - pad;
            scene.rect(Rect::new(x, y, w, row_h + pad), paper);
            scene.hline(x, y, w, self.px(1.0), fade(self.surface.signal, 0.4));
            let text = self.fit(dim, "Original output · Esc returns to your draft", w - pad);
            self.fonts
                .draw(scene, dim, x, y + p.grid.metrics.baseline + pad, &text);
            p.prompt_history
                .hits
                .push((Rect::new(x, y, w, row_h + pad), Hit::Close));
            return;
        }
        let available = (p.rect.h - self.px(32.0)).max(row_h);
        let shown = matches
            .len()
            .min(5)
            .min(((available / row_h) as usize).saturating_sub(2));
        let first = selected.saturating_sub(shown.saturating_sub(1));
        let h = ((shown.max(1) + 2) as f32 * row_h + pad).min(available);
        let below = p.origin.1 + (p.term.cursor().row + 1) as f32 * ch + pad;
        let y = if below + h <= p.rect.bottom() {
            below
        } else {
            (p.origin.1 + p.term.cursor().row as f32 * ch - h - pad).max(p.origin.1)
        };
        let old_clip = scene.clip();
        scene.layer(Some(Rect::new(x, y, w, h)));
        scene.rect(Rect::new(x, y, w, h), paper);
        let ew = self.fonts.measure(dim, "esc");
        let label_w = self
            .fonts
            .draw(scene, dim, x, y + p.grid.metrics.baseline, "history  ");
        let query_x = x + label_w;
        let query_w = (w - label_w - ew - pad * 2.0).max(1.0);
        let caret_byte = crate::field::byte_at(&s.query, s.cursor.at(&s.query));
        let caret_w = self.fonts.measure(st, &s.query[..caret_byte]);
        let offset = (caret_w - query_w + pad).max(0.0);
        scene.layer(Some(Rect::new(query_x, y, query_w, ch)));
        if let Some((a, b)) = s.cursor.range(&s.query) {
            let a = self
                .fonts
                .measure(st, &s.query[..crate::field::byte_at(&s.query, a)]);
            let b = self
                .fonts
                .measure(st, &s.query[..crate::field::byte_at(&s.query, b)]);
            scene.rect(
                Rect::new(query_x + a - offset, y, b - a, ch),
                self.theme.selection,
            );
        }
        self.fonts.draw(
            scene,
            st,
            query_x - offset,
            y + p.grid.metrics.baseline,
            &s.query,
        );
        self.draw_line_caret_on(
            scene, query_x + caret_w - offset, y + p.grid.metrics.baseline,
            st.px, 1.0, self.last_key, paper,
        );
        scene.layer(Some(Rect::new(x, y, w, h)));
        self.fonts
            .draw(scene, dim, x + w - ew, y + p.grid.metrics.baseline, "esc");
        p.prompt_history
            .hits
            .push((Rect::new(x + w - ew - pad, y, ew + pad, row_h), Hit::Close));
        scene.hline(x, y + ch, w, self.px(1.0), fade(self.surface.signal, 0.3));
        if matches.is_empty() {
            self.fonts.draw(
                scene,
                dim,
                x,
                y + row_h + p.grid.metrics.baseline,
                "No matching commands",
            );
        }
        for (slot, &idx) in matches.iter().enumerate().skip(first).take(shown) {
            let entry = &s.entries[idx];
            let yy = y + (slot - first + 1) as f32 * row_h;
            let detail = match (entry.block, entry.exit) {
                (Some(_), Some(0)) => "this session · completed".into(),
                (Some(_), Some(n)) => format!("this session · exit {n}"),
                (Some(_), None) => "this session".into(),
                _ => "history".into(),
            };
            let dw = self.fonts.measure(dim, &detail);
            let detail_width = if w > self.px(500.0) {
                dw + self.px(24.0)
            } else {
                0.0
            };
            let text = self.fit(st, &entry.command, w - detail_width);
            let tw = self.fonts.draw(
                scene,
                if slot == selected { st } else { dim },
                x,
                yy + p.grid.metrics.baseline,
                &text,
            );
            if slot == selected {
                scene.hline(x, yy + ch, tw, self.px(1.0), self.surface.signal);
            }
            if detail_width > 0.0 {
                self.fonts.draw(
                    scene,
                    dim,
                    x + w - dw,
                    yy + p.grid.metrics.baseline,
                    &detail,
                );
            }
            p.prompt_history
                .hits
                .push((Rect::new(x, yy, w, row_h), Hit::Choose(slot)));
        }
        let yy = y + (shown.max(1) + 1) as f32 * row_h;
        let has_output = matches
            .get(selected)
            .is_some_and(|i| s.entries[*i].block.is_some());
        let output_words = "⌃O output";
        let output_width = if has_output {
            self.fonts.measure(st, output_words) + pad * 2.0
        } else {
            0.0
        };
        let hint = self.fit(
            dim,
            "Enter inserts · ↑↓ chooses · Esc keeps draft",
            w - output_width,
        );
        self.fonts
            .draw(scene, dim, x, yy + p.grid.metrics.baseline, &hint);
        if has_output {
            let words = output_words;
            let tw = self.fonts.measure(st, words);
            let xx = x + w - tw;
            self.fonts
                .draw(scene, st, xx, yy + p.grid.metrics.baseline, words);
            p.prompt_history
                .hits
                .push((Rect::new(xx, yy, tw, row_h), Hit::Output));
        }
        scene.layer(old_clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn items() -> Vec<Entry> {
        [
            "git status --short",
            "cargo test",
            "git status",
            "git log --oneline",
        ]
        .iter()
        .map(|s| Entry {
            command: s.to_string(),
            block: None,
            exit: None,
        })
        .collect()
    }
    #[test]
    fn recall_preserves_prefix_and_restores_exact_draft() {
        let mut r = Recall::begin("git st", &items(), (1, 2));
        assert_eq!(r.step(true).as_deref(), Some("git status --short"));
        assert_eq!(r.step(true).as_deref(), Some("git status"));
        assert_eq!(r.step(true).as_deref(), Some("git status"));
        assert_eq!(r.step(false).as_deref(), Some("git status --short"));
        assert_eq!(r.step(false).as_deref(), Some("git st"));
        assert_eq!(r.step(false), None);
    }
    #[test]
    fn search_matches_all_words_in_any_order() {
        assert_eq!(matching(&items(), "SHORT GIT"), vec![0]);
        assert_eq!(matching(&items(), "test git"), Vec::<usize>::new());
        assert_eq!(matching(&items(), "  ").len(), 4);
    }
    #[test]
    fn completed_history_joins_soft_wraps_without_corrupting_words() {
        let mut term = nus_vt::Term::new(12, 12, 100);
        term.advance(
            b"\x1b]133;A\x07$ \x1b]133;B\x07printf '%s' abcdefghijklmnop\r\n\x1b]133;C\x07",
        );
        assert_eq!(
            completed_command(&term, 0).as_deref(),
            Some("printf '%s' abcdefghijklmnop")
        );
        let mut multiline = nus_vt::Term::new(80, 12, 100);
        multiline.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07echo 'a\r\nb'\r\n\x1b]133;C\x07");
        assert_eq!(completed_command(&multiline, 0), None);
    }
    #[test]
    fn recalled_commands_cannot_submit_or_inject_terminal_controls() {
        assert!(safe_command("printf 'café 界'"));
        assert!(!safe_command("echo ok\nrm file"));
        assert!(!safe_command("echo\r"));
        assert!(!safe_command("\x1b[200~"));
        assert!(!safe_command("\t"));
    }

    #[test]
    fn pasting_in_history_edits_the_query_selection_and_preserves_draft() {
        let mut term = nus_vt::Term::new(80, 24, 100);
        term.advance(b"$ \x1b]133;B\x07git status");
        let draft = PromptLine::read(&term).unwrap();
        let mut state = HistoryState::default();
        state.search = Some(Search {
            draft: draft.clone(),
            query: "git old tail".into(),
            cursor: crate::field::Cursor {
                caret: Some(7),
                anchor: Some(4),
            },
            entries: items(),
            selected: 1,
            viewing: false,
            old_block: None,
            unfolded: Vec::new(),
        });
        assert!(state.paste("café\n"));
        let search = state.search.as_ref().unwrap();
        assert_eq!(search.query, "git café tail");
        assert_eq!(search.cursor.at(&search.query), 8);
        assert_eq!(search.selected, 0);
        assert!(search.draft.matches(&draft.stamp()));
        assert_eq!(PromptLine::read(&term).unwrap().text, "git status");
    }
}
