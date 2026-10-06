//! The sidebar's NOTES page, in the footer where the menu drawer's button
//! was: the open tasks of this project's notes and the personal ones (tick
//! one here, or go to it), then those notes themselves, newest first, and
//! their tags (a tag narrows the page to its notes). The
//! picker (the header's note button) is for one note; this is for looking
//! over them.

use std::path::PathBuf;

use nus_render::text::{icons, Style};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

use crate::app::{fade, hover_key, App, Pane, SideHit};
use crate::files::SidePage;
use crate::notes_index::{self as index, Hit, Task};
use crate::notes_store::Home;

/// What the page shows, kept for a click on it.
#[derive(Default)]
pub struct NotesSide {
    pub scroll: f32,
    pub rect: Option<Rect>,
    pub rows: Vec<Row>,
    /// Sections folded to their heads: tasks, this project, personal, tags.
    pub folded: [bool; 4],
    /// A tag picked: the page shows its notes (and their tasks) only.
    pub tag: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Row {
    Head(usize),
    Task(Task),
    Note(PathBuf),
    /// A tag, and how many notes carry it.
    Tag(String, usize),
}

/// The head of the notes under a picked tag; a click clears the tag.
const TAGGED: usize = 4;

/// Tasks shown at most; the rest are in their notes.
const TASKS: usize = 60;

impl App {
    pub(crate) fn toggle_notes_side(&mut self) {
        // The compact sidebar has no room for a list: the picker instead.
        if self.sidebar_icons() {
            self.header_note();
            return;
        }
        self.side_page = if self.side_page == SidePage::Notes { SidePage::Tabs } else { SidePage::Notes };
        if self.side_page == SidePage::Notes {
            if !self.sidebar_visible() {
                self.run(crate::app::Action::ToggleSidebar);
            }
            index::ensure_started();
        }
        self.play_event("toggle");
        self.dirty = true;
    }

    /// The page, in the sidebar's list area between the header and the footer.
    pub(crate) fn draw_notes_side(&mut self, scene: &mut Scene, sb: Rect, top: f32, bottom: f32) {
        index::ensure_started();
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        let row_h = self.px(m::ROW_H);
        let pad = self.px(m::ROW_PAD_X);
        let (mx, my) = self.mouse;
        // The head: NOTES, and + for a new one.
        let head_h = self.header_h();
        let base = top + self.px(22.0);
        let isz = self.px(13.0);
        self.fonts.draw_icon(scene, icons::PENCIL, isz, sb.x + pad, base - isz + self.px(2.0), ink);
        self.fonts.draw(scene, Style { color: ink, ..label }, sb.x + pad + isz + self.px(8.0), base, "NOTES");
        let project = self.notes_folder().map(|f| App::notes_project(&f));
        let plus = Rect::new(sb.right() - pad - self.px(24.0), top, self.px(24.0) + pad, head_h);
        let psz = self.px(12.0);
        self.icon_button(scene, icons::PLUS, psz, plus.x + self.px(6.0), base - psz + self.px(1.0), ink, plus, hover_key("notes-new", 0), crate::app::IconMotion::Pop);
        self.offer_tip(hover_key("notes-new-tip", 0), plus, if project.is_some() { "New note in this project".into() } else { "New personal note".into() });
        self.side_hits.push((plus, SideHit::NotesNew));
        scene.hline(sb.x, top + head_h - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), t.tint);

        // What there is: tasks, this project's notes, the personal ones.
        let here = project.as_ref().and_then(|p| Home::folder(p)).map(|h| h.id);
        let tags = index::tags(here.as_deref());
        // A tag no note carries any more is let go.
        if self.notes_side.tag.as_ref().is_some_and(|t| !tags.iter().any(|(n, _)| n == t)) {
            self.notes_side.tag = None;
        }
        let tagged: Option<Vec<Hit>> = self.notes_side.tag.as_ref().map(|t| index::tagged(here.as_deref(), t, 80));
        let mut tasks = index::tasks(here.as_deref(), TASKS);
        if let Some(list) = &tagged {
            tasks.retain(|t| list.iter().any(|h| h.path == t.note.path));
        }
        let mine: Vec<Hit> = here.as_ref().map(|id| index::recent(Some(id), 40)).unwrap_or_default();
        let personal = index::recent_personal(40);
        let project_name = project.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
        let open_note = self.tabs.get(self.active).and_then(|t| match t.focused_ref() {
            Pane::Editor(e) => e.buf().filter(|b| b.note.is_some()).and_then(|b| b.path.clone()),
            _ => None,
        });
        let mut rows: Vec<Row> = Vec::new();
        let mut heads: Vec<(usize, String)> = Vec::new();
        heads.push((0, format!("OPEN TASKS · {}", tasks.len())));
        rows.push(Row::Head(0));
        if !self.notes_side.folded[0] {
            rows.extend(tasks.iter().cloned().map(Row::Task));
        }
        if let (Some(tag), Some(list)) = (&self.notes_side.tag, &tagged) {
            heads.push((TAGGED, format!("#{tag} · {}", list.len())));
            rows.push(Row::Head(TAGGED));
            rows.extend(list.iter().map(|h| Row::Note(h.path.clone())));
        } else {
            if here.is_some() {
                heads.push((1, format!("{project_name} · {}", mine.len())));
                rows.push(Row::Head(1));
                if !self.notes_side.folded[1] {
                    rows.extend(mine.iter().map(|h| Row::Note(h.path.clone())));
                }
            }
            heads.push((2, format!("PERSONAL · {}", personal.len())));
            rows.push(Row::Head(2));
            if !self.notes_side.folded[2] {
                rows.extend(personal.iter().map(|h| Row::Note(h.path.clone())));
            }
        }
        if !tags.is_empty() {
            heads.push((3, format!("TAGS · {}", tags.len())));
            rows.push(Row::Head(3));
            if !self.notes_side.folded[3] {
                rows.extend(tags.iter().map(|(t, n)| Row::Tag(t.clone(), *n)));
            }
        }
        let picked = self.notes_side.tag.clone();
        let hits: Vec<&Hit> = mine.iter().chain(personal.iter()).chain(tagged.iter().flatten()).collect();

        let list = Rect::new(sb.x, top + head_h, sb.w, (bottom - top - head_h).max(0.0));
        self.notes_side.rect = Some(list);
        let total = rows.len() as f32 * row_h;
        self.notes_side.scroll = self.notes_side.scroll.clamp(0.0, (total - list.h).max(0.0));
        scene.layer(Some(list));
        let mut y = list.y - self.notes_side.scroll;
        let now = crate::notes::now();
        for (k, r) in rows.iter().enumerate() {
            if y + row_h < list.y || y > list.bottom() {
                y += row_h;
                continue;
            }
            let rr = Rect::new(sb.x, y, sb.w, row_h);
            let hot = rr.contains(mx, my) && list.contains(mx, my);
            let b = y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
            // The row first: a click is resolved from the last hit back, so a
            // task's box (pushed after) wins over its row.
            self.side_hits.push((rr, SideHit::NotesRow(k)));
            match r {
                Row::Head(s) => {
                    let csz = self.px(10.0);
                    // The picked tag's head clears it; the others fold.
                    let (icon, color) = if *s == TAGGED {
                        (icons::CLOSE, self.surface.signal)
                    } else {
                        (if self.notes_side.folded[*s] { icons::CARET_RIGHT } else { icons::CARET_DOWN }, t.dim)
                    };
                    self.fonts.draw_icon(scene, icon, csz, sb.x + pad, b - csz + self.px(1.0), color);
                    let words = heads.iter().find(|(i, _)| i == s).map(|(_, w)| w.clone()).unwrap_or_default();
                    let st = if *s == TAGGED { Style { color: self.surface.signal, ..label } } else { dim };
                    // A tag is shown as written, not as a label.
                    let shown = self.fit_as_is(st, words, sb.w - 2.0 * pad - csz - self.px(8.0)).into_owned();
                    self.fonts.draw_as_is(scene, st, sb.x + pad + csz + self.px(8.0), b, &shown);
                    if *s == TAGGED {
                        self.offer_tip(hover_key("notes-tagged", 0), rr, "Every note again".into());
                    }
                }
                Row::Tag(tag, count) => {
                    let on = picked.as_deref() == Some(tag.as_str());
                    if hot {
                        scene.rect(rr, fade(t.tint, 0.5));
                    }
                    if on {
                        scene.rect(Rect::new(sb.x, y, self.px(2.0), row_h), self.surface.signal);
                    }
                    let n = count.to_string();
                    let nw = self.fonts.measure(dim, &n);
                    let tx = sb.x + pad + self.px(4.0);
                    let st = Style { color: if on { self.surface.signal } else { ink }, ..ui };
                    let shown = self.fit_as_is(st, format!("#{tag}"), sb.right() - tx - pad - nw - self.px(10.0)).into_owned();
                    self.fonts.draw_as_is(scene, st, tx, b, &shown);
                    self.fonts.draw(scene, dim, sb.right() - pad - nw, b, &n);
                    self.offer_tip(hover_key("notes-tag", k), rr, if on { format!("#{tag} · every note again") } else { format!("Only the notes tagged #{tag}") });
                }
                Row::Task(task) => {
                    if hot {
                        scene.rect(rr, fade(t.tint, 0.5));
                    }
                    let side = self.px(11.0);
                    let bx = Rect::new(sb.x + pad + self.px(2.0), y + (row_h - side) / 2.0, side, side);
                    let box_hot = Rect::new(sb.x, y, pad + side + self.px(10.0), row_h);
                    scene.outline(bx, self.px(1.5), if box_hot.contains(mx, my) { self.surface.signal } else { ink });
                    let tx = bx.right() + self.px(10.0);
                    let st = Style { color: ink, ..ui };
                    let shown = self.fit(st, &task.words, sb.right() - tx - pad);
                    self.fonts.draw_as_is(scene, st, tx, b, &shown);
                    let title = if task.note.title.trim().is_empty() { "Untitled".to_string() } else { task.note.title.clone() };
                    self.offer_tip(hover_key("notes-task", k), rr, format!("{} · in {title}, line {} · the box ticks it", task.words, task.line + 1));
                    self.side_hits.push((box_hot, SideHit::NotesTick(k)));
                }
                Row::Note(path) => {
                    let Some(h) = hits.iter().find(|h| &h.path == path) else { y += row_h; continue };
                    if hot {
                        scene.rect(rr, fade(t.tint, 0.5));
                    }
                    if open_note.as_deref() == Some(path.as_path()) {
                        scene.rect(Rect::new(sb.x, y, self.px(2.0), row_h), self.surface.signal);
                    }
                    let when = crate::notes::when(h.modified, now);
                    let ww = self.fonts.measure(dim, &when);
                    let tx = sb.x + pad + self.px(4.0);
                    let title = if h.title.trim().is_empty() { "Untitled" } else { h.title.as_str() };
                    let st = Style { color: if h.unfiled { t.dim } else { ink }, ..ui };
                    let shown = self.fit(st, title, sb.right() - tx - pad - ww - self.px(10.0));
                    self.fonts.draw_as_is(scene, st, tx, b, &shown);
                    self.fonts.draw(scene, dim, sb.right() - pad - ww, b, &when);
                }
            }
            y += row_h;
        }
        scene.layer(None);
        self.notes_side.rows = rows;
    }

    /// A click on the page: a head folds its section, a task goes to its
    /// line, a note opens.
    pub(crate) fn notes_side_click(&mut self, k: usize) {
        let Some(r) = self.notes_side.rows.get(k).cloned() else { return };
        match r {
            Row::Head(TAGGED) => self.notes_side.tag = None,
            Row::Head(s) => self.notes_side.folded[s] = !self.notes_side.folded[s],
            // A tag narrows the page to its notes; again, every note.
            Row::Tag(tag, _) => {
                self.notes_side.tag = if self.notes_side.tag.as_deref() == Some(tag.as_str()) { None } else { Some(tag) };
                self.notes_side.scroll = 0.0;
            }
            Row::Task(task) => self.open_note_at(&task.note.path, task.line),
            Row::Note(path) => self.place_note(&path),
        }
        self.dirty = true;
    }

    /// A task's box: ticked through its note's session, open or not, and
    /// only while the line still says what the list showed.
    pub(crate) fn notes_side_tick(&mut self, k: usize) {
        let Some(Row::Task(task)) = self.notes_side.rows.get(k).cloned() else { return };
        match crate::notes_session::toggle_task(&task.note.path, task.line, &task.text) {
            Ok(true) => self.play_event("toggle"),
            Ok(false) => self.notice(icons::PENCIL, "Task Changed", "the note moved on; open it to tick this one"),
            Err(e) => self.notice_problem("Task Not Ticked", e.to_string()),
        }
        self.dirty = true;
    }

    /// The wheel over the page.
    pub(crate) fn notes_side_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if self.side_page != SidePage::Notes || !self.sidebar_visible() || !self.notes_side.rect.is_some_and(|r| r.contains(x, y)) {
            return false;
        }
        self.notes_side.scroll -= dy;
        self.dirty = true;
        true
    }

    /// New from the page's head: in this project, else personal.
    pub(crate) fn notes_side_new(&mut self) {
        let profile = self.project_here_for_side().is_none();
        self.new_note(profile, false);
    }

    fn project_here_for_side(&self) -> Option<PathBuf> {
        self.notes_folder().map(|f| App::notes_project(&f))
    }
}

