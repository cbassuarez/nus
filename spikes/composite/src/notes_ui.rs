//! Notes in the window (notes.rs names the places; notes_model.rs,
//! notes_store.rs and notes_session.rs are what a note is, where it is
//! kept and who is editing it). A note is the editor pane on a Markdown
//! file under `.nus/notes/` or `profile/notes/`: beside a shell it is a
//! peer like a page, and given the width of a whole tab it grows two
//! rails, the notes of this project and your personal ones on the left
//! and what the note points at on the right.
//!
//! ADD TO NOTE is one command wherever there is something to keep (a
//! block, a shell or page selection, a page, a file's lines): it freezes
//! what is there, asks where it goes (the palette, with what you type as
//! why it matters), adds it through the note's session and says where it
//! went, with Open and Undo, while you stay in the work.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nus_render::text::{icons, Style};
use nus_render::{Rect, Scene};

use crate::app::{fade, Action, App, Caps, PaletteMode, PaletteRow, Pane};
use crate::editor::EditorPane;
use crate::notes::{self, Place, Ref};
use crate::notes_capture::{self as capture, Origin};
use crate::notes_index::{self as index, Hit};
use crate::notes_model::Evidence;
use crate::notes_session::{self as session, Status};
use crate::notes_store::{Home, NoteKey, Scope};
use nus_render::theme::metric as m;

/// What the palette and menus ask of notes.
#[derive(Clone, PartialEq, Debug)]
pub enum NoteAct {
    /// A new note beside the shell: in this project, or personal.
    New { profile: bool },
    /// The notes, as a whole tab.
    Tab,
    /// A block (this tab's selected or last one, or the one named), frozen
    /// for Add to Note.
    CaptureBlock(Option<u64>),
    /// Text selected in a shell.
    CaptureShellSelection(String),
    /// The page in this tab: a link, or the words selected on it.
    CapturePage,
    /// The lines selected in the focused editor.
    CaptureEditorSelection,
    /// Where the pending capture goes, and why it matters (may be empty).
    CaptureInto(Dest, String),
    /// The project's notes up in the hatch.
    Hatch,
    Open(PathBuf),
    /// Open a note at a line (a search hit, a backlink).
    OpenAt(PathBuf, usize),
    /// The focused note's title, or its tags: ask, then set.
    Title,
    SetTitle(String),
    Tags,
    SetTags(String),
    /// A quick capture's note leaves the inbox.
    KeepAsNote,
    Trash,
    Restore(PathBuf),
    /// A note changed elsewhere: keep yours (theirs kept as a checkpoint),
    /// take theirs (yours kept), or look at theirs first.
    KeepMine,
    TakeTheirs,
    ShowTheirs,
    /// Let go of unsaved changes, on purpose.
    Discard,
    Export { clean: bool },
    /// A legacy note becomes a canonical one (its original is kept).
    Migrate,
    /// A captured command, typed into this tab's shell, not run.
    InsertCommand(String, String),
    /// A link to this note, written at the caret of the focused note.
    InsertLink(PathBuf),
    /// Migrate every legacy note of a project (None: personal), copy
    /// first, and count what happened.
    MigrateAll(Option<PathBuf>),
    /// A source's file, beside the note, at a line.
    OpenSourceAt(PathBuf, usize),
    /// A page source's original address, beside the note.
    OpenOriginal(String),
    /// A reading source's saved copy: the exact one it pinned, or nothing.
    OpenReading(String, Option<String>),
    /// Formatting on the focused note (the rail, the keys, the palette).
    Format(crate::notes_format::Act),
}

/// Where a capture goes.
#[derive(Clone, PartialEq, Debug)]
pub enum Dest {
    Note(PathBuf),
    NewInProject(PathBuf),
    NewPersonal,
}

/// A click on a rail.
#[derive(Clone, Debug)]
pub enum RailHit {
    Open(PathBuf),
    New { profile: bool },
    Ref(usize),
    Back(usize),
}

/// The whole-tab rails' state, kept on the editor pane while its buffer is
/// a note and the pane is wide enough to show them.
#[derive(Default)]
pub struct Rails {
    path: Option<PathBuf>,
    folder: Option<PathBuf>,
    folder_notes: Vec<Hit>,
    profile_notes: Vec<Hit>,
    refs: Vec<Ref>,
    backlinks: Vec<Hit>,
    /// Each source marker's line and what its excerpt says of itself.
    evidence: Vec<(usize, Evidence)>,
    revision: Option<u64>,
    listed: Option<Instant>,
    /// The index's generation when the lists were read.
    seen: u64,
    pub hits: Vec<(Rect, RailHit)>,
}

/// Below this width (logical px) a note is a plain pane: no rails.
const RAILS_FROM: f32 = 1100.0;
const INDEX_W: f32 = 272.0;
const REFS_W: f32 = 300.0;
const RELIST: Duration = Duration::from_secs(3);

fn fit(fonts: &nus_render::text::FontSystem, style: Style, text: &str, max_w: f32) -> String {
    if fonts.measure(style, text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect();
        if fonts.measure(style, &format!("{s}…")) <= max_w { lo = mid } else { hi = mid - 1 }
    }
    format!("{}…", chars[..lo].iter().collect::<String>())
}

impl App {
    /// The folder notes belong to now: the focused shell's, else FILES'.
    pub(crate) fn notes_folder(&self) -> Option<PathBuf> {
        self.focused_cwd().map(PathBuf::from).filter(|p| p.is_dir()).or_else(|| self.files_root.clone())
    }

    /// The project a folder belongs to for notes: the nearest folder above
    /// it (itself included) that already has notes, else its repository's
    /// top, else the folder itself. Opening a subfolder never makes a
    /// second home.
    pub(crate) fn notes_project(folder: &Path) -> PathBuf {
        if let Some(p) = folder.ancestors().find(|a| a.join(".nus").join("notes").is_dir()) {
            return p.to_path_buf();
        }
        folder.ancestors().find(|a| a.join(".git").exists()).unwrap_or(folder).to_path_buf()
    }

    fn project_here(&self) -> Option<PathBuf> {
        self.notes_folder().map(|f| App::notes_project(&f))
    }

    pub(crate) fn note_act(&mut self, act: NoteAct) {
        if crate::private::enabled() {
            self.notice(icons::EYE_SLASH, "Not In Incognito", "notes are files; a private window keeps none");
            return;
        }
        match act {
            NoteAct::New { profile } => self.new_note(profile, false),
            NoteAct::Tab => self.open_notes_tab(),
            NoteAct::CaptureBlock(start) => self.capture_block(start),
            NoteAct::CaptureShellSelection(text) => {
                let cwd = self.focused_cwd().unwrap_or_default();
                self.begin_capture(Origin::ShellSelection { text, cwd });
            }
            NoteAct::CapturePage => self.capture_page(String::new()),
            NoteAct::CaptureEditorSelection => self.capture_editor_selection(),
            NoteAct::CaptureInto(dest, why) => self.finish_capture(dest, &why),
            NoteAct::Hatch => self.note_in_hatch(),
            NoteAct::Open(p) => self.open_note(&p, false),
            NoteAct::OpenAt(p, line) => self.open_note_at(&p, line),
            NoteAct::Title => self.ask_about_note(PaletteMode::NoteTitle),
            NoteAct::SetTitle(t) => self.with_note(|key| session::set_title(key, &t).map(|_| "Title Set")),
            NoteAct::Tags => self.ask_about_note(PaletteMode::NoteTags),
            NoteAct::SetTags(t) => {
                let tags: Vec<String> = t.split([',', ' ']).map(|x| x.trim().trim_start_matches('#').to_string()).filter(|x| !x.is_empty()).collect();
                self.with_note(|key| session::set_tags(key, &tags).map(|_| "Tags Set"))
            }
            NoteAct::KeepAsNote => self.with_note(|key| session::set_filed(key, true).map(|_| "Kept As Note")),
            NoteAct::Trash => self.trash_note(),
            NoteAct::Restore(p) => self.restore_note(&p),
            NoteAct::KeepMine => self.with_note(|key| session::keep_mine(key).map(|_| "Kept Your Version")),
            NoteAct::TakeTheirs => self.with_note(|key| session::take_theirs(key).map(|_| "Took Their Version")),
            NoteAct::ShowTheirs => self.show_theirs(),
            NoteAct::Discard => self.with_note(|key| session::discard(key).map(|_| "Changes Let Go")),
            NoteAct::Export { clean } => self.export_note(clean),
            NoteAct::Migrate => self.migrate_note(),
            NoteAct::InsertCommand(cmd, cwd) => self.insert_command(&cmd, &cwd),
            NoteAct::InsertLink(p) => self.insert_note_link(&p),
            NoteAct::MigrateAll(project) => self.migrate_all(project),
            NoteAct::OpenSourceAt(path, line) => {
                self.open_file(&path, true);
                if let Some(e) = self.focused_editor() {
                    if let Some(b) = e.buf_mut() {
                        if b.ready() { b.cursor = b.at(line, 0); b.anchor = None; } else { b.pending_position = Some(nus_lsp::lsp_types::Position::new(line as u32, 0)); }
                    }
                    e.reveal();
                }
            }
            NoteAct::OpenOriginal(url) => self.open_url(&url, false),
            NoteAct::OpenReading(id, pinned) => self.open_pinned_reading(&id, pinned.as_deref()),
            NoteAct::Format(act) => self.note_format(act),
        }
        self.dirty = true;
    }

    /// The focused (else this tab's) note: its view.
    fn note_view(&self) -> Option<session::View> {
        let tab = self.tabs.get(self.active)?;
        let panes = [(tab.focus_right, if tab.focus_right { tab.right.as_ref() } else { Some(&tab.left) }), (!tab.focus_right, if tab.focus_right { Some(&tab.left) } else { tab.right.as_ref() })];
        panes.into_iter().find_map(|(_, p)| match p {
            Some(Pane::Editor(e)) => e.buf().and_then(|b| b.note.clone()),
            _ => None,
        })
    }

    fn with_note(&mut self, f: impl FnOnce(&NoteKey) -> Result<&'static str, crate::notes_store::NoteError>) {
        let Some(v) = self.note_view() else {
            self.notice(icons::PENCIL, "No Note Here", "open a note first");
            return;
        };
        match f(&v.key) {
            Ok(words) => self.notice(icons::PENCIL, words, v.title),
            Err(e) => self.notice_problem("Note Unchanged", e.to_string()),
        }
    }

    fn ask_about_note(&mut self, mode: PaletteMode) {
        if self.note_view().is_none() {
            self.notice(icons::PENCIL, "No Note Here", "open a note first");
            return;
        }
        self.open_palette(mode);
    }

    /// Make a note and open it beside the shell: in this project, or
    /// personal. It exists (saved, empty) before a key is pressed.
    pub(crate) fn new_note(&mut self, profile: bool, whole_tab: bool) {
        let home = if profile { self.personal_home() } else { self.project_home_here() };
        let Some(home) = home else { return };
        match session::create(&home, "", true) {
            Ok(path) => self.open_note(&path, whole_tab),
            Err(e) => self.notice_problem("Could Not Make Note", e.to_string()),
        }
    }

    pub(crate) fn header_note(&mut self) {
        self.close_settings();
        let profile = self.project_here().is_none();
        if crate::private::enabled() { self.note_act(NoteAct::New { profile }); return; }
        // Keep a page or shell already beside the work. Editors can open
        // another buffer; other occupied splits get a separate note tab.
        let whole_tab = self.tabs.get(self.active).is_some_and(|t| t.right.as_ref().is_some_and(|p| !matches!(p, Pane::Editor(_))));
        self.new_note(profile, whole_tab);
    }

    fn personal_home(&mut self) -> Option<Home> {
        let cap = match crate::notes_store::WriteCap::grant() {
            Ok(c) => c,
            Err(e) => {
                self.notice_problem("Could Not Make Note", e.to_string());
                return None;
            }
        };
        match session::personal_home(Some(&cap)) {
            Ok(h) => Some(h),
            Err(e) => {
                self.notice_problem("Personal Notes Locked", e.to_string());
                None
            }
        }
    }

    fn project_home_here(&mut self) -> Option<Home> {
        let Some(project) = self.project_here() else {
            self.notice(icons::PENCIL, "No Folder Here", "open a shell in a folder, or make a personal note");
            return None;
        };
        let cap = crate::notes_store::WriteCap::grant().ok()?;
        match session::project_home(&project, &cap) {
            Ok(h) => Some(h),
            Err(e) => {
                self.notice_problem("Could Not Make Note", e.to_string());
                None
            }
        }
    }

    /// A note in the editor: a new tab of its own (the rails show when it
    /// is wide), else beside the shell. A note already open in this tab is
    /// focused, not opened twice.
    pub(crate) fn open_note(&mut self, path: &Path, whole_tab: bool) {
        if !whole_tab {
            self.open_file(path, true);
            return;
        }
        let mut e = EditorPane::new(Rect::new(0.0, 0.0, 1.0, 1.0));
        match e.open(path) {
            Ok(_) => {
                let tab = self.make_tab(Pane::Editor(e), None);
                self.tabs.push(tab);
                let n = self.tabs.len() - 1;
                self.activate(n);
                self.apply_term_resizes(false);
                self.dirty = true;
            }
            Err(err) => self.notice_problem("Could Not Open Note", err.to_string()),
        }
    }

    pub(crate) fn open_note_at(&mut self, path: &Path, line: usize) {
        self.open_note(path, false);
        if let Some(e) = self.focused_editor() {
            if let Some(b) = e.buf_mut().filter(|b| b.note.is_some()) {
                let line = line.min(b.text.len_lines().saturating_sub(1));
                b.cursor = b.at(line, 0);
                b.anchor = None;
            }
            e.reveal();
        }
    }

    /// The notes as a whole tab: the newest note of this project, else a
    /// personal one, else a new one.
    pub(crate) fn open_notes_tab(&mut self) {
        index::ensure_started();
        let project = self.project_here().and_then(|p| Home::folder(&p));
        let newest = project.as_ref().and_then(|h| index::recent(Some(&h.id), 1).into_iter().next())
            .or_else(|| index::recent(None, 1).into_iter().next())
            .map(|h| h.path);
        match newest {
            Some(p) => self.open_note(&p, true),
            None => self.new_note(project.is_none(), true),
        }
    }

    /// The project's notes up in the hatch. No sheet of its own: the hatch
    /// already carries any tab over everything, so the note captures last
    /// went to (else the newest, else a new unfiled one) is HOISTed like a
    /// shell would be, and LAND brings it down. A tab already showing it
    /// is the one that goes up, never a second view.
    pub(crate) fn note_in_hatch(&mut self) {
        index::ensure_started();
        let project = self.project_here();
        let known = capture::last_destination(project.as_deref())
            .or_else(|| {
                let h = project.as_ref().and_then(|p| Home::folder(p));
                h.and_then(|h| index::recent(Some(&h.id), 1).into_iter().next()).map(|h| h.path)
            });
        let path = match known {
            Some(p) => p,
            None => {
                let home = if project.is_some() { self.project_home_here() } else { self.personal_home() };
                let Some(home) = home else { return };
                match session::create(&home, "Inbox", false) {
                    Ok(p) => p,
                    Err(e) => return self.notice_problem("Could Not Make Note", e.to_string()),
                }
            }
        };
        // Buffers hold canonical paths without Windows' verbatim prefix.
        let path = path.canonicalize().map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\"))).unwrap_or(path);
        let showing = |t: &crate::app::Tab| t.right.is_none() && matches!(&t.left, Pane::Editor(e) if e.buf().and_then(|b| b.path.as_deref()) == Some(path.as_path()));
        match self.tabs.iter().position(showing) {
            Some(i) if self.tabs[i].hatch => {
                if !self.hatch.as_ref().is_some_and(|h| h.visible && !h.hiding) {
                    self.toggle_hatch();
                }
                return;
            }
            Some(i) => self.activate(i),
            None => {
                self.open_note(&path, true);
                if !self.tabs.get(self.active).is_some_and(showing) {
                    return;
                }
            }
        }
        self.hoist();
    }

    // --- capture ------------------------------------------------------

    /// Freeze a capture and ask where it goes.
    pub(crate) fn begin_capture(&mut self, origin: Origin) {
        if crate::private::enabled() {
            self.notice(icons::EYE_SLASH, "Not In Incognito", "notes are files; a private window keeps none");
            return;
        }
        match capture::draft(origin, notes::now()) {
            Ok(d) => {
                self.pending_capture = Some(d);
                index::ensure_started();
                self.open_palette(PaletteMode::NoteCapture);
            }
            Err(e) => self.notice_problem("Could Not Capture", e.to_string()),
        }
    }

    /// A block: the one asked for, else the selected one, else the last
    /// finished one in this tab's shell. A running block is captured as
    /// far as it has got, and says so.
    pub(crate) fn capture_block(&mut self, start: Option<u64>) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let panes = if tab.focus_right { [tab.right.as_ref(), Some(&tab.left)] } else { [Some(&tab.left), tab.right.as_ref()] };
        let Some(t) = panes.into_iter().flatten().find_map(|p| match p { Pane::Term(t) => Some(t), _ => None }) else {
            self.notice(icons::PENCIL, "No Shell Here", "add to note works on a shell's blocks");
            return;
        };
        let blocks = t.blocks();
        let pick = start.or(t.block_sel).and_then(|s| blocks.iter().find(|b| b.start == s))
            .or_else(|| blocks.iter().rev().find(|b| !b.running && !b.cmd.trim().is_empty()));
        let Some(b) = pick.cloned() else {
            self.notice(icons::PENCIL, "No Block Yet", "run a command, then add it to a note");
            return;
        };
        let cmd = crate::cutoff::oneline(&t.block_cmd_text(b.start));
        let cmd = if cmd.trim().is_empty() { b.cmd.clone() } else { cmd };
        let output = t.block_output_text(b.start);
        let cwd = t.term.cwd.clone().or_else(|| t.cwd.clone()).unwrap_or_default();
        let shell = t.program.clone();
        self.begin_capture(Origin::Block { cmd, output, cwd, exit: b.exit, running: b.running, shell });
    }

    /// The page in this tab, with the words selected on it when a menu
    /// handed them over.
    pub(crate) fn capture_page(&mut self, selection: String) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let panes = if tab.focus_right { [tab.right.as_ref(), Some(&tab.left)] } else { [Some(&tab.left), tab.right.as_ref()] };
        let page = panes.into_iter().flatten().find_map(|p| match p {
            Pane::Web(w) => { let s = w.tab.shared.borrow(); Some((s.title.clone(), s.url.clone(), w.container.clone())) }
            _ => None,
        });
        match page {
            Some((title, url, container)) if url.starts_with("http") => self.begin_capture(Origin::Page { url, title, quote: selection, container }),
            _ => self.notice(icons::PENCIL, "No Page Here", "open a page first"),
        }
    }

    /// A page's menu asked: its own address, title and selection, as the
    /// menu saw them (a page that moved on since is not read again).
    pub(crate) fn capture_page_selection(&mut self, url: String, title: String, quote: String, container: String) {
        self.begin_capture(Origin::Page { url, title, quote, container });
    }

    /// The lines selected in the focused editor (a code file; a note's own
    /// text is already a note).
    pub(crate) fn capture_editor_selection(&mut self) {
        let Some(e) = self.focused_editor() else {
            self.notice(icons::PENCIL, "No Editor Here", "select lines in a file first");
            return;
        };
        let Some(b) = e.buf() else { return };
        let Some((a, z)) = b.selection() else {
            self.notice(icons::PENCIL, "Nothing Selected", "select lines in a file first");
            return;
        };
        let Some(path) = b.path.clone() else { return };
        let text = b.selected_text();
        let (start_line, end_line) = (b.line_of(a), b.line_of(z.saturating_sub(1).max(a)));
        let file_sha256 = (!b.dirty).then(|| crate::notes_model::sha256(b.text.to_string().as_bytes()));
        let project = path.parent().map(App::notes_project);
        self.begin_capture(Origin::File { path, project, start_line, end_line, text, file_sha256 });
    }

    /// Palette rows for NoteCapture: what is being added, then where.
    pub(crate) fn note_capture_rows(&self, input: &str) -> Vec<PaletteRow> {
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        let Some(d) = &self.pending_capture else { return vec![row("·", "nothing to add · capture again".into(), Action::Noop)] };
        let why = input.trim().to_string();
        // Destinations first (Enter takes the first); what is being added,
        // and that typing says why it matters, last.
        let mut rows = Vec::new();
        let project = d.project.clone().map(|p| App::notes_project(&p)).or_else(|| self.project_here());
        let into = |dest: Dest| Action::Note(NoteAct::CaptureInto(dest, why.clone()));
        let mut seen: Vec<PathBuf> = Vec::new();
        if let Some(last) = capture::last_destination(project.as_deref()) {
            let title = index::title_of(&last).unwrap_or_else(|| last.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            rows.push(row("→", format!("add to {title} · last time"), into(Dest::Note(last.clone()))));
            seen.push(last);
        }
        // An open note beside is offered only when it lives in this project.
        if let Some(v) = self.note_view() {
            if let Some((path, _, scope)) = session::where_is(&v.key) {
                let here = match (scope, &project) {
                    (Scope::Folder, Some(p)) => notes::folder_of(&path).is_some_and(|f| f == *p),
                    (Scope::Personal, None) => true,
                    _ => false,
                };
                if here && !seen.contains(&path) {
                    rows.push(row("→", format!("add to {} · open beside", v.title), into(Dest::Note(path.clone()))));
                    seen.push(path);
                }
            }
        }
        match &project {
            Some(p) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                rows.push(row("+", format!("new note in {name} · unfiled until kept"), into(Dest::NewInProject(p.clone()))));
                rows.push(row("+", "new personal note · unfiled until kept".into(), into(Dest::NewPersonal)));
            }
            None => rows.push(row("+", "new personal note · unfiled until kept".into(), into(Dest::NewPersonal))),
        }
        let home = project.as_ref().and_then(|p| Home::folder(p));
        let mut recent = home.as_ref().map(|h| index::recent(Some(&h.id), 6)).unwrap_or_default();
        recent.extend(index::recent_personal(4));
        for h in recent {
            if seen.contains(&h.path) {
                continue;
            }
            seen.push(h.path.clone());
            rows.push(row("·", format!("add to {} · {} · {}", h.title, h.home_name, notes::when(h.modified, notes::now())), into(Dest::Note(h.path))));
        }
        rows.push(row("✎", format!("adding {} · {}{}", d.label, d.summary(), if why.is_empty() { " · type why it matters, or don't" } else { " · with why it matters" }), Action::Noop));
        rows
    }

    /// The pending capture goes where you chose.
    fn finish_capture(&mut self, dest: Dest, why: &str) {
        let Some(d) = self.pending_capture.clone() else { return };
        let project = d.project.clone().map(|p| App::notes_project(&p)).or_else(|| self.project_here());
        let path = match dest {
            Dest::Note(p) => p,
            Dest::NewInProject(p) => {
                let cap = crate::notes_store::WriteCap::grant().ok();
                let home = cap.as_ref().map(|c| session::project_home(&p, c));
                match home.map(|h| h.and_then(|h| session::create(&h, &capture::title_for(&d), false))) {
                    Some(Ok(p)) => p,
                    Some(Err(e)) => return self.notice_problem("Could Not Make Note", e.to_string()),
                    None => return,
                }
            }
            Dest::NewPersonal => {
                let Some(home) = self.personal_home() else { return };
                match session::create(&home, &capture::title_for(&d), false) {
                    Ok(p) => p,
                    Err(e) => return self.notice_problem("Could Not Make Note", e.to_string()),
                }
            }
        };
        let c = session::Capture { request_id: d.request_id.clone(), markdown: d.with_why(why), source: Some(d.source.clone()) };
        match session::capture(&path, c) {
            Ok(r) => {
                self.pending_capture = None;
                capture::remember_destination(project.as_deref(), &r.path);
                if r.repeated {
                    self.notice(icons::PENCIL, "Already Added", r.title);
                    return;
                }
                self.toast_two(icons::PENCIL, format!("Added To {}", r.title.caps()), d.summary(), crate::toast::Act::OpenNote(r.path.clone()), crate::toast::Act::UndoCapture(r.key, r.capture_id), false);
            }
            Err(e) => self.notice_problem("Could Not Add To Note", e.to_string()),
        }
    }

    /// The receipt's Undo: only while the capture is as it landed.
    pub(crate) fn undo_capture(&mut self, key: &NoteKey, id: &str) {
        match session::undo_capture(key, id) {
            session::UndoCapture::Removed => self.notice(icons::PENCIL, "Capture Taken Back", ""),
            session::UndoCapture::Edited => {
                let path = session::where_is(key).map(|w| w.0);
                match path {
                    Some(p) => self.toast(icons::PENCIL, "Capture Kept", "you wrote in it since", Some(crate::toast::Act::OpenNote(p))),
                    None => self.notice(icons::PENCIL, "Capture Kept", "you wrote in it since"),
                }
            }
            session::UndoCapture::Gone => self.notice(icons::PENCIL, "Nothing To Undo", "the note was closed and saved"),
        }
    }

    // --- saving, closing ------------------------------------------------

    /// Cmd+S in a note: save now, say so only when it did not work.
    pub(crate) fn save_note_now(&mut self) -> bool {
        let Some(v) = self.focused_editor().and_then(|e| e.buf()).and_then(|b| b.note.clone()) else { return false };
        match session::flush(&v.key) {
            Status::Saved | Status::Saving => self.play_event("toggle"),
            st => self.note_unsaved(&v.key, &v.title, st),
        }
        self.dirty = true;
        true
    }

    pub(crate) fn note_unsaved(&mut self, key: &NoteKey, title: &str, st: Status) {
        match st {
            Status::Conflict => self.toast_problem("Note Changed Elsewhere", format!("{title} · both versions are kept · palette: note conflict"), None),
            Status::ReadOnly(why) => self.toast_problem("Note Read Only", format!("{title} · {why}"), None),
            Status::Locked => self.toast_problem("Personal Notes Locked", title.to_string(), None),
            _ => self.toast_two(icons::WARNING, "Note Not Saved", format!("{title} · your changes are still here"), crate::toast::Act::RetryNote(key.clone()), crate::toast::Act::SaveNoteCopy(key.clone()), true),
        }
    }

    /// Closing a view of a note: the last view of one with unsaved text
    /// saves it first, and stays open when that did not work.
    pub(crate) fn note_may_close(&mut self, v: &session::View) -> bool {
        if session::others(v) {
            return true;
        }
        if !session::dirty(&v.key) && session::status(&v.key).is_none_or(|s| s == Status::Saved) {
            return true;
        }
        match session::flush(&v.key) {
            Status::Saved => true,
            st => {
                self.note_unsaved(&v.key, &v.title, st);
                false
            }
        }
    }

    /// The notes in these tabs whose last views they hold, saved before
    /// the tabs go. Returns the tabs that may go.
    pub(crate) fn notes_let_tabs_close(&mut self, targets: &[usize]) -> Vec<usize> {
        let mut here: std::collections::HashMap<NoteKey, (usize, String, Vec<usize>)> = Default::default();
        for &i in targets {
            let Some(tab) = self.tabs.get(i) else { continue };
            for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                let Pane::Editor(e) = p else { continue };
                for v in e.buffers.iter().filter_map(|b| b.note.as_ref()) {
                    let entry = here.entry(v.key.clone()).or_insert((0, v.title.clone(), Vec::new()));
                    entry.0 += 1;
                    entry.2.push(i);
                }
            }
        }
        let mut held: Vec<usize> = Vec::new();
        for (key, (count, title, tabs)) in here {
            if session::views(&key) > count || session::status(&key).is_none_or(|s| s == Status::Saved) {
                continue;
            }
            match session::flush(&key) {
                Status::Saved => {}
                st => {
                    self.note_unsaved(&key, &title, st);
                    held.extend(tabs);
                }
            }
        }
        targets.iter().copied().filter(|t| !held.contains(t)).collect()
    }

    /// Before this window goes: every note it holds the last view of,
    /// saved. False (and a word why) when one could not be.
    pub(crate) fn notes_let_window_close(&mut self) -> bool {
        let all: Vec<usize> = (0..self.tabs.len()).collect();
        self.notes_let_tabs_close(&all).len() == all.len()
    }

    pub(crate) fn retry_note(&mut self, key: &NoteKey) {
        let title = session::document(key).map(|d| d.title()).unwrap_or_default();
        match session::flush(key) {
            Status::Saved => self.notice(icons::CHECK, "Note Saved", title),
            st => self.note_unsaved(key, &title, st),
        }
    }

    /// Save Copy: a personal copy (a project that cannot be written is
    /// likely why we are here), else one beside it; then the original
    /// goes back to what is on disk, its text now safe in the copy.
    pub(crate) fn save_note_copy(&mut self, key: &NoteKey) {
        let cap = crate::notes_store::WriteCap::grant().ok();
        let home = cap.as_ref().and_then(|c| session::personal_home(Some(c)).ok())
            .or_else(|| session::where_is(key).and_then(|(p, _, _)| notes::folder_of(&p)).and_then(|f| Home::folder(&f)));
        let Some(home) = home else {
            self.notice_problem("Could Not Save Copy", "no notes folder can be written");
            return;
        };
        match session::copy_to(key, &home) {
            Ok(path) => {
                let _ = session::discard(key);
                self.toast(icons::PENCIL, "Saved As A Copy", home.name(), Some(crate::toast::Act::OpenNote(path)));
            }
            Err(e) => self.notice_problem("Could Not Save Copy", e.to_string()),
        }
    }

    fn show_theirs(&mut self) {
        let Some(v) = self.note_view() else { return };
        let Some(text) = session::theirs(&v.key) else {
            self.notice(icons::PENCIL, "No Conflict", v.title);
            return;
        };
        // Their version, as a scratch file to read beside yours; choosing
        // is still Keep Mine or Take Theirs.
        let dir = std::env::temp_dir().join("nus-note-conflicts");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{}-theirs.md", &v.key.note_id[..8]));
        if v.scope == Scope::Personal {
            self.notice_problem("Not Shown As A File", "a personal note's other version stays sealed · keep mine, or take theirs");
            return;
        }
        match std::fs::write(&path, text) {
            Ok(()) => self.open_file(&path, true),
            Err(e) => self.notice_problem("Could Not Show", e.to_string()),
        }
    }

    fn trash_note(&mut self) {
        let Some(v) = self.note_view() else { return };
        if session::flush(&v.key) != Status::Saved {
            return self.notice_problem("Not Trashed", "save the note first");
        }
        let Some((path, _, _)) = session::where_is(&v.key) else { return };
        let cap = match crate::notes_store::WriteCap::grant() { Ok(c) => c, Err(_) => return };
        let result = session::home_of(&path, &cap).and_then(|home| {
            let snap = home.open(&path)?;
            home.trash(&cap, &path, &snap.base)?;
            Ok(home)
        });
        match result {
            Ok(home) => {
                // Close its views here; the file and its history stay.
                for tab in self.tabs.iter_mut() {
                    for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                        if let Pane::Editor(e) = p {
                            e.buffers.retain(|b| b.note.as_ref().is_none_or(|n| n.key != v.key));
                            e.active = e.active.min(e.buffers.len().saturating_sub(1));
                        }
                    }
                }
                index::refresh(vec![session::Committed { home, path: path.clone() }]);
                self.toast(icons::PENCIL, "Moved To Trash", v.title, None);
            }
            Err(e) => self.notice_problem("Not Trashed", e.to_string()),
        }
    }

    fn restore_note(&mut self, path: &Path) {
        let cap = match crate::notes_store::WriteCap::grant() { Ok(c) => c, Err(_) => return };
        let result = session::home_of(path, &cap).and_then(|home| {
            let snap = home.open(path)?;
            home.restore(&cap, path, &snap.base)?;
            Ok(home)
        });
        match result {
            Ok(home) => {
                index::refresh(vec![session::Committed { home, path: path.to_path_buf() }]);
                self.open_note(path, false);
            }
            Err(e) => self.notice_problem("Not Restored", e.to_string()),
        }
    }

    /// Export: the note as Markdown, somewhere you choose. Lossless keeps
    /// the header and markers; clean leaves nus out and captions sources.
    fn export_note(&mut self, clean: bool) {
        let Some(v) = self.note_view() else { return };
        let Some(doc) = session::document(&v.key) else { return };
        let bytes = if clean { crate::notes_model::clean_export(&doc).into_bytes() } else { doc.to_bytes() };
        let name = format!("{}.md", notes::file_name(&v.title, notes::now()).trim_end_matches(".md"));
        let dir = crate::browser::downloads_dir();
        match crate::pick::save_as(&self.window, if clean { "Export note (clean Markdown)" } else { "Export note" }, dir, name) {
            Ok(p) => self.note_export = Some((bytes, p)),
            Err(e) => self.notice_problem("Could Not Export", e),
        }
    }

    /// A legacy note becomes a canonical one: the original bytes are kept
    /// as its first checkpoint, the old header's lines stay in the body.
    fn migrate_note(&mut self) {
        let Some(v) = self.note_view() else { return };
        if !v.legacy {
            return self.notice(icons::PENCIL, "Already Current", v.title);
        }
        if session::flush(&v.key) != Status::Saved {
            return self.notice_problem("Not Migrated", "save the note first");
        }
        let Some((path, _, _)) = session::where_is(&v.key) else { return };
        match crate::notes_import::migrate(&path) {
            Ok(new_path) => {
                for tab in self.tabs.iter_mut() {
                    for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                        if let Pane::Editor(e) = p {
                            e.buffers.retain(|b| b.note.as_ref().is_none_or(|n| n.key != v.key));
                            e.active = e.active.min(e.buffers.len().saturating_sub(1));
                        }
                    }
                }
                self.open_note(&new_path, false);
                self.toast(icons::PENCIL, "Note Migrated", "the original is kept in its history", None);
            }
            Err(e) => self.notice_problem("Not Migrated", e.to_string()),
        }
    }

    /// A captured command, typed into this tab's shell and left for you
    /// to run: never Enter. Multiline text asks first, as a paste does.
    fn insert_command(&mut self, cmd: &str, cwd: &str) {
        let here = self.focused_cwd().unwrap_or_default();
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let right = matches!(tab.right, Some(Pane::Term(_)));
        let pane = if right && tab.focus_right { tab.right.as_mut() } else { Some(&mut tab.left) };
        let pane = match pane {
            Some(Pane::Term(_)) => pane,
            _ => tab.right.as_mut().filter(|p| matches!(p, Pane::Term(_))),
        };
        let Some(Pane::Term(t)) = pane else {
            self.notice(icons::PENCIL, "No Shell Here", "open a shell beside the note");
            return;
        };
        if cmd.lines().count() > 1 || cmd.chars().any(|c| c.is_control() && c != '\t') {
            t.confirm_paste = Some(cmd.to_string());
        } else {
            t.write_paste(cmd);
        }
        if !cwd.is_empty() && cwd != here {
            self.notice(icons::PENCIL, "Inserted, Not Run", format!("captured in {cwd} · this shell is in {here}"));
        } else {
            self.notice(icons::PENCIL, "Inserted, Not Run", cmd.to_string());
        }
    }

    /// The quiet facts about a note: when, which revision, where, and how
    /// much history its home keeps.
    fn note_details(&self, v: &session::View) -> Option<String> {
        let d = session::document(&v.key)?;
        let (path, home_name, scope) = session::where_is(&v.key)?;
        let mut parts = vec!["details".to_string()];
        if let Some(t) = d.created() {
            parts.push(format!("made {}", notes::when(t, notes::now())));
        }
        if let Some(t) = d.updated() {
            parts.push(format!("saved {}", notes::when(t, notes::now())));
        }
        if d.revision() > 0 {
            parts.push(format!("revision {}", d.revision()));
        }
        parts.push(match scope {
            Scope::Personal => "personal · sealed on this device".into(),
            Scope::Folder => format!("{home_name} · local Markdown · {}", path.display()),
        });
        if let Some(home) = crate::notes_store::WriteCap::grant().ok().and_then(|c| session::home_of(&path, &c).ok()) {
            let mb = home.history_bytes() as f64 / (1024.0 * 1024.0);
            parts.push(format!("history {mb:.1} of {} MB", crate::notes_store::HISTORY_BUDGET / (1024 * 1024)));
        }
        Some(parts.join(" · "))
    }

    fn migrate_all(&mut self, project: Option<PathBuf>) {
        let home = match &project {
            Some(p) => Home::folder(p),
            None => session::personal_home(None).ok(),
        };
        let Some(home) = home else { return };
        let list = match home.list() {
            Ok(l) => l,
            Err(e) => return self.notice_problem("Not Migrated", e.to_string()),
        };
        let legacy: Vec<PathBuf> = list.into_iter().filter(|e| e.legacy && e.error.is_none()).map(|e| e.path).collect();
        let open: Vec<PathBuf> = self.tabs.iter().flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref())).filter_map(|p| match p { Pane::Editor(e) => Some(e), _ => None })
            .flat_map(|e| e.buffers.iter().filter(|b| b.note.is_some()).filter_map(|b| b.path.clone())).collect();
        let (mut done, mut skipped, mut failed) = (0, 0, 0);
        for p in &legacy {
            // A note open with unsaved text is left for its own migrate.
            if open.contains(p) {
                skipped += 1;
                continue;
            }
            match crate::notes_import::migrate(p) {
                Ok(_) => done += 1,
                Err(e) => {
                    tracing::warn!("notes: {} not migrated: {e}", p.display());
                    failed += 1;
                }
            }
        }
        debug_assert_eq!(done + skipped + failed, legacy.len());
        self.toast(icons::PENCIL, "Notes Migrated", format!("{done} of {} · {skipped} open, left · {failed} failed, originals kept", legacy.len()), None);
    }

    /// A reading source opens the saved copy it cites and no other: a
    /// refreshed or removed item says so, and the note's quote stands.
    fn open_pinned_reading(&mut self, id: &str, pinned: Option<&str>) {
        match (self.reading_snapshot(id), pinned) {
            (Some(now), Some(h)) if now.as_deref() == Some(h) => self.read_saved(id),
            (Some(_), None) => self.read_saved(id),
            (Some(_), Some(_)) => self.notice(icons::BOOK, "Saved Copy Changed", "the item was refreshed since · the note keeps the quote it cited"),
            (None, _) => self.notice(icons::BOOK, "Saved Version Unavailable", "the item left the reading list · the note keeps its quote"),
        }
    }

    /// What "notes here" asks the index: the focused file in its project,
    /// the page in this tab, the last command in this tab's shell.
    pub(crate) fn lookups_here(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Some(tab) = self.tabs.get(self.active) else { return out };
        for p in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
            match p {
                Pane::Editor(e) => {
                    if let Some(path) = e.buf().filter(|b| b.note.is_none()).and_then(|b| b.path.clone()) {
                        if let Some(l) = file_lookup(&path) {
                            out.push(l);
                        }
                    }
                }
                Pane::Web(w) => {
                    let url = w.tab.shared.borrow().url.clone();
                    if url.starts_with("http") {
                        out.push(index::lookup_url(&url));
                    }
                }
                Pane::Term(t) => {
                    let cwd = t.term.cwd.clone().or_else(|| t.cwd.clone()).unwrap_or_default();
                    let home = Some(PathBuf::from(&cwd)).filter(|p| p.is_dir()).and_then(|p| Home::folder(&App::notes_project(&p)));
                    if let (Some(home), Some(b)) = (home, t.blocks().iter().rev().find(|b| !b.running && !b.cmd.trim().is_empty())) {
                        let cmd = crate::cutoff::oneline(&t.block_cmd_text(b.start));
                        out.push(index::lookup_command(&home.id, if cmd.is_empty() { &b.cmd } else { &cmd }));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Write `[title](note:<id>)` at the caret of the focused note: a link
    /// that survives renames, and the only kind of backlink there is.
    fn insert_note_link(&mut self, target: &Path) {
        let Some(v) = self.note_view() else { return };
        let Some(cap) = crate::notes_store::WriteCap::grant().ok() else { return };
        let Ok(home) = session::home_of(target, &cap) else { return };
        let Ok(snap) = home.open(target) else {
            return self.notice_problem("Could Not Link", "that note could not be read");
        };
        if snap.doc.is_legacy() {
            return self.notice(icons::PENCIL, "Migrate It First", "a note needs an id before it can be linked to");
        }
        let title = snap.doc.title().replace(['[', ']'], "");
        let link = if snap.key.home_id == v.key.home_id { format!("[{title}](note:{})", snap.key.note_id) } else { format!("[{title}](note:{}/{})", snap.key.home_id, snap.key.note_id) };
        if let Some(e) = self.focused_editor() {
            if let Some(b) = e.buf_mut().filter(|b| b.note.is_some()) {
                b.insert(&link, false);
            }
            e.reveal();
        }
    }

    /// Once a loop: the export dialog's answer, and the index kept up.
    pub(crate) fn tend_notes(&mut self) {
        index::ensure_started();
        if let Some((bytes, mut pick)) = self.note_export.take() {
            match pick.poll() {
                None => self.note_export = Some((bytes, pick)),
                Some(Ok(Some(path))) => match std::fs::write(&path, &bytes) {
                    Ok(()) => self.toast(icons::PENCIL, "Note Exported", path.display().to_string(), Some(crate::toast::Act::RevealPath(path))),
                    Err(e) => self.notice_problem("Could Not Export", e.to_string()),
                },
                Some(Ok(None)) => {}
                Some(Err(e)) => self.notice_problem("Could Not Export", e),
            }
        }
        for c in session::take_committed() {
            index::refresh(vec![c]);
        }
    }

    pub(crate) fn note_title_rows(&self, input: &str) -> Vec<PaletteRow> {
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        let now = self.note_view().map(|v| v.title).unwrap_or_default();
        let q = input.trim();
        if q.is_empty() {
            vec![row("·", format!("title this note · now “{now}” · empty = its first line"), Action::Note(NoteAct::SetTitle(String::new())))]
        } else {
            vec![row("→", format!("call this note “{q}” · its file keeps its name"), Action::Note(NoteAct::SetTitle(q.to_string())))]
        }
    }

    pub(crate) fn note_tags_rows(&self, input: &str) -> Vec<PaletteRow> {
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        let now = self.note_view().and_then(|v| session::document(&v.key)).map(|d| d.tags().join(", ")).unwrap_or_default();
        let q = input.trim();
        vec![row("→", if q.is_empty() { format!("tags · now “{now}” · empty = none") } else { format!("tag this note “{q}” · commas or spaces between") }, Action::Note(NoteAct::SetTags(q.to_string())))]
    }

    /// Palette rows for notes: `note …`, `notes …`, `add to note`, `clip`,
    /// and a search of every note when the query starts `notes `.
    pub(crate) fn note_rows(&self, q: &str) -> Vec<(&'static str, String, Action)> {
        use crate::app::Action::Note;
        let mut rows = Vec::new();
        if crate::private::enabled() {
            return rows;
        }
        let words = ["note", "notes", "clip", "add to note", "capture"];
        if !words.iter().any(|w| q.starts_with(w) || (q.len() >= 3 && w.starts_with(q))) {
            return rows;
        }
        index::ensure_started();
        let project = self.project_here();
        let view = self.note_view();
        if let Some(p) = &project {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            rows.push(("✎", format!("new note in {name} · local Markdown, out of git"), Note(NoteAct::New { profile: false })));
        }
        rows.push(("✎", "new personal note · sealed on this device".into(), Note(NoteAct::New { profile: true })));
        rows.push(("✎", "notes · the whole tab, with the index".into(), Note(NoteAct::Tab)));
        rows.push(("✎", "notes in the hatch · over everything".into(), Note(NoteAct::Hatch)));
        rows.push(("✎", "add this block to a note… · clip".into(), Note(NoteAct::CaptureBlock(None))));
        rows.push(("✎", "add the page to a note…".into(), Note(NoteAct::CapturePage)));
        rows.push(("✎", "add the selected lines to a note…".into(), Note(NoteAct::CaptureEditorSelection)));
        if let Some(v) = &view {
            if let Some(d) = self.note_details(v) {
                rows.push(("·", d, Action::Noop));
            }
            rows.push(("✎", format!("title · {}", v.title), Note(NoteAct::Title)));
            if q.starts_with("note format") || q.starts_with("notes format") {
                let chord = if cfg!(target_os = "macos") { "⌘⌥" } else { "Ctrl+Alt+" };
                for act in crate::notes_format::RAIL {
                    let (name, key) = act.name();
                    rows.push(("✎", format!("format · {name} · {chord}{key}"), Note(NoteAct::Format(act))));
                }
                return rows;
            }
            rows.push(("✎", "format… · bold, lists, headings · type: note format".into(), Action::Noop));
            rows.push(("✎", "tags · this note".into(), Note(NoteAct::Tags)));
            let doc = session::document(&v.key);
            if doc.as_ref().is_some_and(|d| !d.filed()) {
                rows.push(("✎", "keep as note · out of the inbox".into(), Note(NoteAct::KeepAsNote)));
            }
            rows.push(("✎", "export this note · Markdown with its sources".into(), Note(NoteAct::Export { clean: false })));
            rows.push(("✎", "export this note · clean Markdown, sources as captions".into(), Note(NoteAct::Export { clean: true })));
            if v.legacy {
                rows.push(("✎", "migrate this note · an id and sources; the original is kept".into(), Note(NoteAct::Migrate)));
            } else {
                rows.push(("✎", "move this note to the trash · kept, restorable".into(), Note(NoteAct::Trash)));
            }
            match session::status(&v.key) {
                Some(Status::Conflict) => {
                    rows.push(("!", "note conflict · keep mine (theirs kept as history)".into(), Note(NoteAct::KeepMine)));
                    rows.push(("!", "note conflict · take theirs (mine kept as history)".into(), Note(NoteAct::TakeTheirs)));
                    rows.push(("!", "note conflict · show theirs beside".into(), Note(NoteAct::ShowTheirs)));
                    rows.push(("!", "note · let go of my unsaved changes".into(), Note(NoteAct::Discard)));
                }
                Some(Status::Failed(_)) => rows.push(("!", "note · let go of my unsaved changes".into(), Note(NoteAct::Discard))),
                _ => {}
            }
            // Its sources: back to each, the way each can be reached.
            if let Some(d) = doc {
                let base = session::where_is(&v.key).and_then(|(p, _, _)| notes::folder_of(&p));
                for row in source_rows(&d, base.as_deref()) {
                    rows.push(row);
                }
            }
        }
        for (key, path) in session::drafts() {
            let _ = key;
            rows.push(("!", format!("recovered draft · {} · unsaved when nus stopped", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), Note(NoteAct::Open(path))));
        }
        // Search: `notes <words>`, `note <words>`.
        let rest = q.strip_prefix("notes").or_else(|| q.strip_prefix("note")).unwrap_or("").trim();
        // Notes here: whose sources are exactly this file, page or command.
        if rest.is_empty() || rest == "here" {
            for h in index::about(&self.lookups_here()) {
                rows.push(("✎", format!("notes here · {} · {} · {}", h.title, h.home_name, h.reason), Note(NoteAct::Open(h.path))));
            }
        }
        if rest.starts_with("migrate") {
            let homes: Vec<(Option<PathBuf>, Option<Home>)> = vec![
                (project.clone(), project.as_ref().and_then(|p| Home::folder(p))),
                (None, session::personal_home(None).ok()),
            ];
            for (at, home) in homes {
                let Some(home) = home else { continue };
                // A dry run: what is there, before anything changes.
                match home.list() {
                    Ok(list) => {
                        let legacy = list.iter().filter(|e| e.legacy && e.error.is_none()).count();
                        let unreadable = list.iter().filter(|e| e.error.is_some()).count();
                        let dup = Home::duplicates(&list).len();
                        let example = list.iter().find(|e| e.legacy).and_then(|e| e.doc.as_ref()).map(|d| format!(" · “{}”…", d.title())).unwrap_or_default();
                        let mut words = format!("migrate {legacy} legacy note{} in {}{example} · originals kept", if legacy == 1 { "" } else { "s" }, home.name());
                        if unreadable > 0 { words.push_str(&format!(" · {unreadable} unreadable, left as they are")); }
                        if dup > 0 { words.push_str(&format!(" · {dup} ids used twice, left for you")); }
                        rows.push(("⇪", words, if legacy > 0 { Note(NoteAct::MigrateAll(at)) } else { Action::Noop }));
                    }
                    Err(e) => rows.push(("·", format!("{} · {e}", home.name()), Action::Noop)),
                }
            }
            return rows;
        }
        if rest == "trash" {
            for h in index::trashed(12) {
                rows.push(("↺", format!("restore · {} · {} · trashed {}", h.title, h.home_name, notes::when(h.modified, notes::now())), Note(NoteAct::Restore(h.path))));
            }
            return rows;
        }
        if let Some(words) = rest.strip_prefix("link").map(str::trim) {
            if view.is_some() {
                let here = project.as_ref().and_then(|p| Home::folder(p)).map(|h| h.id);
                let found = if words.is_empty() { index::recent(None, 8) } else { index::search(words, None, 8).unwrap_or_default() };
                for h in found.into_iter().filter(|h| view.as_ref().is_none_or(|v| v.key != h.key)) {
                    rows.push(("↗", format!("link to {} · {} · written where the caret is", h.title, h.home_name), Note(NoteAct::InsertLink(h.path))));
                }
                let _ = here;
                return rows;
            }
        }
        if view.is_some() {
            rows.push(("↗", "link to another note… · type: note link <words>".into(), Action::Noop));
        }
        let here = project.as_ref().and_then(|p| Home::folder(p)).map(|h| h.id);
        let hits: Vec<Hit> = if rest.is_empty() {
            let mut v = here.as_ref().map(|id| index::recent(Some(id), 6)).unwrap_or_default();
            v.extend(index::recent_personal(4));
            v
        } else {
            match index::search(rest, here.as_deref(), 12) {
                Ok(v) => v,
                Err(why) => {
                    rows.push(("·", why, Action::Noop));
                    Vec::new()
                }
            }
        };
        if !rest.is_empty() && hits.is_empty() {
            rows.push(("·", format!("no notes match “{rest}”"), Action::Noop));
        }
        for h in hits {
            let mut text = format!("{} · {} · {}", h.title, h.home_name, notes::when(h.modified, notes::now()));
            if !h.snippet.is_empty() {
                text.push_str(&format!(" · {}", h.snippet));
            }
            if !h.reason.is_empty() {
                text.push_str(&format!(" · {}", h.reason));
            }
            rows.push(("✎", text, Note(NoteAct::OpenAt(h.path, h.line))));
        }
        if let Some(st) = index::status_line() {
            rows.push(("·", st, Action::Noop));
        }
        rows
    }

    /// The strip's word for a note: where it lives, and whether it is saved.
    pub(crate) fn note_place_word(e: &EditorPane) -> Option<String> {
        let b = e.buf()?;
        if let Some(v) = &b.note {
            let st = session::status(&v.key).map(|s| s.word()).unwrap_or_default();
            let place = match v.scope {
                Scope::Personal => "personal".to_string(),
                Scope::Folder => format!("{} · local markdown", v.home_name),
            };
            return Some(if st.is_empty() { place } else { format!("{place} · {}", st.to_lowercase()) });
        }
        if let Some(p) = b.path.as_deref().and_then(notes::place_of) {
            return Some(p.label().to_string());
        }
        // A file with notes about it says so, quietly: `notes · 2`.
        let path = b.path.clone()?;
        let n = recall_count(&path);
        (n > 0).then(|| format!("notes · {n} · palette: notes here"))
    }

    /// Keep the rails' lists current; cheap to call every draw: the lists
    /// come from the search index, read again when it moves on.
    fn tend_rails(&self, e: &mut EditorPane) {
        let Some((path, revision)) = e.buf().and_then(|b| Some((b.path.clone()?, b.revision))) else { e.notes = None; return };
        let Some(place) = notes::place_of(&path) else { e.notes = None; return };
        let key = e.buf().and_then(|b| b.note.as_ref().map(|v| v.key.clone()));
        let rails = e.notes.get_or_insert_with(Rails::default);
        let moved = rails.path.as_deref() != Some(path.as_path());
        let generation = index::generation();
        if moved || rails.seen != generation || rails.listed.is_none_or(|t| t.elapsed() > RELIST) {
            rails.folder = match place {
                Place::Folder => notes::folder_of(&path),
                Place::Profile => self.project_here(),
            };
            let home = rails.folder.as_ref().and_then(|f| Home::folder(f));
            rails.folder_notes = home.map(|h| index::recent(Some(&h.id), 200)).unwrap_or_default();
            rails.profile_notes = index::recent_personal(200);
            rails.backlinks = key.as_ref().map(index::backlinks).unwrap_or_default();
            rails.listed = Some(Instant::now());
            rails.seen = generation;
        }
        if moved || rails.revision != Some(revision) {
            if let Some(b) = e.buffers.get(e.active) {
                if b.ready() && b.text.len_bytes() <= 1024 * 1024 {
                    let text = b.text.to_string();
                    let evidence = key.as_ref().and_then(session::document).map(|d| crate::notes_model::evidence(&d).into_iter().map(|(b, e)| (b.line, e)).collect()).unwrap_or_default();
                    let rails = e.notes.as_mut().unwrap();
                    rails.refs = notes::refs(&text);
                    rails.evidence = evidence;
                    rails.revision = Some(revision);
                }
            }
        }
        if let Some(rails) = e.notes.as_mut() { rails.path = Some(path); }
    }

    /// Draw the rails when the note is wide enough; returns the rect the
    /// text keeps.
    pub(crate) fn draw_note_rails(&mut self, scene: &mut Scene, e: &mut EditorPane, r: Rect) -> Rect {
        if r.w < self.px(RAILS_FROM) {
            e.notes = None;
            return r;
        }
        self.tend_rails(e);
        let Some(rails) = e.notes.as_mut() else { return r };
        rails.hits.clear();
        let t = self.theme.clone();
        let (ink, paper, dim) = (t.ink, t.paper, t.dim);
        let label = self.label();
        let strong = self.label_strong();
        let dim_label = Style { color: dim, ..label };
        let ui = self.ui();
        let ui_dim = Style { color: dim, ..ui };
        let signal = self.surface.signal;
        let (mx, my) = self.mouse;
        let (hair, structure) = (self.px(m::HAIRLINE), self.px(m::STRUCTURE));
        let (pad, row_h, head_h) = (self.px(18.0), self.px(30.0), self.header_h());
        let now = notes::now();
        let px = |v: f32| (v * self.scale).round();
        let mut word = Style { font: self.f.wordmark, px: px(34.0), color: ink, tracking: 0.0 };
        word.px = word.px.min(head_h * 1.4);
        let fonts = &mut self.fonts;
        let current = rails.path.clone();

        // The index.
        let ix = Rect::new(r.x, r.y, px(INDEX_W), r.h);
        scene.rect(ix, paper);
        scene.vline(ix.right() - structure, r.y, r.h, structure, ink);
        fonts.draw(scene, word, ix.x + pad, r.y + px(46.0), "notes");
        scene.hline(ix.x, r.y + px(62.0), ix.w - structure, structure, ink);
        let foot = Rect::new(ix.x, r.bottom() - px(m::FOOT_H), ix.w - structure, px(m::FOOT_H));
        let mut y = r.y + px(62.0) + px(8.0);
        let sections: [(String, &Vec<Hit>); 2] = [
            (rails.folder.as_ref().and_then(|f| f.file_name()).map(|n| format!("This Project · {}", n.to_string_lossy())).unwrap_or_else(|| "This Project".into()), &rails.folder_notes),
            ("Personal".into(), &rails.profile_notes),
        ];
        for (head, list) in sections {
            if y + row_h > foot.y { break; }
            fonts.draw(scene, strong, ix.x + pad, y + row_h * 0.62, &fit(fonts, strong, &head, ix.w - 2.0 * pad));
            y += row_h;
            if list.is_empty() {
                fonts.draw(scene, ui_dim, ix.x + pad, y + row_h * 0.62, "none yet");
                y += row_h;
            }
            for entry in list.iter() {
                if y + row_h > foot.y { break; }
                let row = Rect::new(ix.x, y, ix.w - structure, row_h);
                let on = current.as_deref() == Some(entry.path.as_path());
                let when = if entry.unfiled { format!("inbox · {}", notes::when(entry.modified, now)) } else { notes::when(entry.modified, now) };
                if on {
                    scene.rect(row, ink);
                } else if row.contains(mx, my) {
                    scene.rect(row, crate::surface::mix(paper, ink, 0.05));
                }
                let (st, wst) = if on { (Style { color: paper, ..ui }, Style { color: paper, ..label }) } else { (ui, dim_label) };
                let ww = fonts.measure(wst, &when);
                fonts.draw(scene, wst, row.right() - pad - ww, y + row_h * 0.62, &when);
                fonts.draw(scene, st, ix.x + pad, y + row_h * 0.62, &fit(fonts, st, &entry.title, ix.w - 3.0 * pad - ww));
                scene.hline(ix.x, row.bottom() - hair, row.w, hair, if on { ink } else { crate::surface::mix(paper, ink, 0.12) });
                rails.hits.push((row, RailHit::Open(entry.path.clone())));
                y += row_h;
            }
            y += px(10.0);
        }
        scene.hline(foot.x, foot.y, foot.w, structure, ink);
        let half = Rect::new(foot.x, foot.y, foot.w * 0.55, foot.h);
        let other = Rect::new(half.right(), foot.y, foot.w - half.w, foot.h);
        for (cell, words, profile) in [(half, "+ New Note", false), (other, "Profile", true)] {
            if cell.contains(mx, my) { scene.rect(cell, crate::surface::mix(paper, ink, 0.05)); }
            let st = if profile { dim_label } else { strong };
            fonts.draw(scene, st, cell.x + pad, cell.y + cell.h * 0.62, words);
            if rails.folder.is_some() || profile {
                rails.hits.push((cell, RailHit::New { profile }));
            }
        }
        scene.vline(other.x, other.y, other.h, hair, ink);

        // What it points at.
        let rx = Rect::new(r.right() - px(REFS_W), r.y, px(REFS_W), r.h);
        scene.rect(rx, paper);
        scene.vline(rx.x, r.y, r.h, structure, ink);
        let inner = rx.w - 2.0 * pad - structure;
        let mut y = r.y;
        fonts.draw(scene, strong, rx.x + pad, y + head_h * 0.62, "Points At");
        y += head_h;
        scene.hline(rx.x, y - hair, rx.w, hair, ink);
        let tall = px(50.0);
        if rails.refs.is_empty() {
            fonts.draw(scene, ui_dim, rx.x + pad, y + row_h * 0.7, &fit(fonts, ui_dim, "clip a block, or paste a link", inner));
            y += row_h + px(6.0);
        }
        for (i, rf) in rails.refs.iter().enumerate() {
            if y + tall > r.bottom() - px(120.0) { break; }
            let row = Rect::new(rx.x + structure, y, rx.w - structure, tall);
            if row.contains(mx, my) { scene.rect(row, crate::surface::mix(paper, ink, 0.05)); }
            let lamp = Rect::new(rx.x + pad, y + px(12.0), px(8.0), px(8.0));
            let (kind, act, words) = match rf {
                Ref::Block { cmd, exit, line } => {
                    let failed = exit.is_some_and(|c| c != 0);
                    scene.rect(lamp, if failed { signal } else { ink });
                    // What the captured excerpt says of itself, when it is
                    // bound to a source record (the marker is the line above).
                    let evidence = rails.evidence.iter().find(|(l, _)| l + 1 == *line).map(|(_, e)| *e);
                    let act = match evidence {
                        Some(Evidence::Edited) => "edited excerpt".to_string(),
                        Some(Evidence::VerificationFailed) => "original unverified".to_string(),
                        Some(Evidence::Ambiguous) => "bound twice".to_string(),
                        _ if failed => format!("exit {}", exit.unwrap_or(0)),
                        Some(Evidence::Original) => "captured copy".to_string(),
                        _ => "in note".to_string(),
                    };
                    ("Block", act, cmd.clone())
                }
                Ref::Note { label, note_id, .. } => {
                    scene.rect(lamp, signal);
                    let name = if label.is_empty() { format!("note {}", &note_id[..8]) } else { label.clone() };
                    ("Note", "open beside".into(), name)
                }
                Ref::Page { url, .. } => {
                    scene.rect(lamp, ink);
                    scene.rect(Rect::new(lamp.x + hair, lamp.y + hair, lamp.w - 2.0 * hair, lamp.h - 2.0 * hair), paper);
                    ("Page", "open beside".into(), url.trim_start_matches("https://").trim_start_matches("http://").to_string())
                }
                Ref::File { path, at, .. } => {
                    scene.rect(lamp, ink);
                    ("File", "open".into(), match at { Some(n) => format!("{path}:{n}"), None => path.clone() })
                }
            };
            fonts.draw(scene, strong, lamp.right() + px(8.0), y + px(20.0), kind);
            let aw = fonts.measure(dim_label, &act);
            fonts.draw(scene, dim_label, rx.right() - pad - aw, y + px(20.0), &act);
            fonts.draw(scene, ui, rx.x + pad, y + px(40.0), &fit(fonts, ui, &words, inner));
            scene.hline(row.x, row.bottom() - hair, row.w, hair, crate::surface::mix(paper, ink, 0.12));
            rails.hits.push((row, RailHit::Ref(i)));
            y += tall;
        }
        y += px(14.0);
        if y + head_h < r.bottom() {
            scene.hline(rx.x, y, rx.w, hair, ink);
            fonts.draw(scene, strong, rx.x + pad, y + head_h * 0.62, "Points Here");
            y += head_h;
            scene.hline(rx.x, y - hair, rx.w, hair, ink);
            if rails.backlinks.is_empty() {
                fonts.draw(scene, ui_dim, rx.x + pad, y + row_h * 0.62, &fit(fonts, ui_dim, "no note links here yet", inner));
            }
            for (i, h) in rails.backlinks.iter().enumerate() {
                if y + row_h > r.bottom() { break; }
                let row = Rect::new(rx.x + structure, y, rx.w - structure, row_h);
                if row.contains(mx, my) { scene.rect(row, crate::surface::mix(paper, ink, 0.05)); }
                let at = format!("line {}", h.line + 1);
                let aw = fonts.measure(dim_label, &at);
                let name = h.title.clone();
                fonts.draw(scene, ui, rx.x + pad, y + row_h * 0.62, &fit(fonts, ui, &name, inner - aw - pad));
                fonts.draw(scene, dim_label, rx.right() - pad - aw, y + row_h * 0.62, &at);
                scene.hline(row.x, row.bottom() - hair, row.w, hair, crate::surface::mix(paper, ink, 0.12));
                rails.hits.push((row, RailHit::Back(i)));
                y += row_h;
            }
        }
        Rect::new(ix.right(), r.y, rx.x - ix.right(), r.h)
    }

    /// A press on a rail, in any note pane of this tab. True when taken.
    pub(crate) fn note_rails_mouse(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let mut found = None;
        for (right, p) in [(false, Some(&mut tab.left)), (true, tab.right.as_mut())] {
            let Some(Pane::Editor(e)) = p else { continue };
            let Some(rails) = &e.notes else { continue };
            if let Some((_, hit)) = rails.hits.iter().find(|(r, _)| r.contains(x, y)) {
                let target = match hit {
                    RailHit::Ref(i) => rails.refs.get(*i).cloned().map(Ok),
                    RailHit::Back(i) => rails.backlinks.get(*i).cloned().map(Err),
                    _ => None,
                };
                found = Some((right, hit.clone(), target, rails.path.clone()));
                break;
            }
        }
        let Some((right, hit, target, note)) = found else { return false };
        self.tabs[self.active].focus_right = right;
        let open_here = |app: &mut App, path: &Path, line: Option<usize>| {
            let tab = &mut app.tabs[app.active];
            let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
            let Some(Pane::Editor(e)) = pane else { return };
            match e.open(path) {
                Ok(i) => {
                    if let (Some(line), Some(b)) = (line, e.buffers.get_mut(i)) {
                        if b.ready() {
                            b.cursor = b.at(line, 0);
                            b.anchor = None;
                        } else {
                            b.pending_position = Some(nus_lsp::lsp_types::Position::new(line as u32, 0));
                        }
                    }
                    e.reveal();
                }
                Err(err) => tracing::warn!("notes: {err}"),
            }
        };
        match (hit, target) {
            (RailHit::Open(p), _) => open_here(self, &p, None),
            (RailHit::New { profile }, _) => self.new_note_in_place(profile, right),
            (_, Some(Err(h))) => open_here(self, &h.path, Some(h.line)),
            (_, Some(Ok(Ref::Page { url, .. }))) => self.open_url(&url, false),
            (_, Some(Ok(Ref::File { path, at, .. }))) => {
                let base = note.as_deref().and_then(notes::folder_of).or_else(|| self.notes_folder()).unwrap_or_default();
                let p = Path::new(path.trim_start_matches("./"));
                let full = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
                if full.is_file() {
                    // Beside the note, never in its place: the note stays
                    // where you were in it.
                    self.open_file(&full, true);
                    if let (Some(n), Some(e)) = (at, self.focused_editor()) {
                        if let Some(b) = e.buf_mut() {
                            let line = n.saturating_sub(1) as usize;
                            if b.ready() { b.cursor = b.at(line, 0); b.anchor = None; } else { b.pending_position = Some(nus_lsp::lsp_types::Position::new(line as u32, 0)); }
                        }
                        e.reveal();
                    }
                } else {
                    self.notice(icons::PENCIL, "Not Found", full.display().to_string());
                }
            }
            (_, Some(Ok(Ref::Note { note_id, home_id, .. }))) => {
                // A link to a note: open it beside, found by its identity
                // (a renamed file or title still resolves).
                let home_of_note = note.as_deref().and_then(|p| crate::notes_store::WriteCap::grant().ok().and_then(|c| session::home_of(p, &c).ok()));
                let target_home = home_id.or_else(|| home_of_note.map(|h| h.id)).unwrap_or_default();
                let key = NoteKey { home_id: target_home, note_id };
                // The index first; its own home's files when the index has
                // not caught up.
                let found = index::path_of(&key).or_else(|| {
                    let home = note.as_deref().and_then(|p| crate::notes_store::WriteCap::grant().ok().and_then(|c| session::home_of(p, &c).ok()))?;
                    home.locate(&key).ok()
                });
                match found {
                    Some(p) => self.open_note(&p, false),
                    None => self.notice(icons::PENCIL, "Note Not Found", "it may be in a project not open here, or in the trash"),
                }
            }
            (_, Some(Ok(Ref::Block { line, .. }))) => {
                // The clip keeps the block's text: go to it in the note.
                if let Some(p) = note { open_here(self, &p, Some(line)); }
            }
            _ => {}
        }
        self.dirty = true;
        true
    }

    /// A new note from the rail's foot, opened in the pane that asked.
    fn new_note_in_place(&mut self, profile: bool, right: bool) {
        let folder = self.tabs.get(self.active).and_then(|t| {
            let p = if right { t.right.as_ref() } else { Some(&t.left) };
            match p { Some(Pane::Editor(e)) => e.notes.as_ref().and_then(|r| r.folder.clone()), _ => None }
        });
        let home = match (profile, folder.or_else(|| self.project_here())) {
            (true, _) => self.personal_home(),
            (false, Some(f)) => crate::notes_store::WriteCap::grant().ok().and_then(|c| session::project_home(&f, &c).ok()),
            (false, None) => return self.notice(icons::PENCIL, "No Folder Here", "make a personal note instead"),
        };
        let Some(home) = home else { return };
        match session::create(&home, "", true) {
            Ok(path) => {
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                if let Some(Pane::Editor(e)) = pane { let _ = e.open(&path); }
            }
            Err(e) => self.notice_problem("Could Not Make Note", e.to_string()),
        }
    }
}

/// The index's key for a file: its path within its project's home.
fn file_lookup(path: &Path) -> Option<String> {
    let project = App::notes_project(path.parent()?);
    let home = Home::folder(&project)?;
    let rel = path.strip_prefix(&project).ok()?;
    Some(index::lookup_file(&home.id, &rel.to_string_lossy()))
}

thread_local! {
    /// How many notes cite a file, as of an index generation: drawn every
    /// frame, asked once per change.
    static RECALL: std::cell::RefCell<std::collections::HashMap<PathBuf, (u64, usize)>> = Default::default();
}

fn recall_count(path: &Path) -> usize {
    let generation = index::generation();
    if let Some(n) = RECALL.with(|r| r.borrow().get(path).filter(|(g, _)| *g == generation).map(|(_, n)| *n)) {
        return n;
    }
    let n = file_lookup(path).map(|l| index::about(&[l]).len()).unwrap_or(0);
    RECALL.with(|r| {
        let mut r = r.borrow_mut();
        if r.len() > 256 {
            r.clear();
        }
        r.insert(path.to_path_buf(), (generation, n));
    });
    n
}

/// Palette rows that return to a note's sources: a file at the captured
/// lines (or where they moved to, or each place when they are in several,
/// or nothing when they are gone), a page's original, a saved article's
/// pinned copy, a command to insert.
fn source_rows(d: &crate::notes_model::Document, project: Option<&Path>) -> Vec<(&'static str, String, Action)> {
    use crate::notes_anchor::{locate, Anchor};
    use crate::notes_model::Kind;
    let mut rows = Vec::new();
    let note = |a: NoteAct| Action::Note(a);
    for s in d.sources() {
        let label = crate::app::fit_cmd(s.label(), 48);
        match s.kind() {
            Kind::File => {
                let path = s.get("relative_path").and_then(|r| project.map(|p| p.join(r))).filter(|p| p.is_file())
                    .or_else(|| s.get("device_path_hint").map(PathBuf::from).filter(|p| p.is_file()));
                let Some(path) = path else {
                    rows.push(("·", format!("source · {label} · the file is not here · the note keeps its excerpt"), Action::Noop));
                    continue;
                };
                let hint = s.0.get("range").and_then(|r| r.get("start_line")?.as_u64()).unwrap_or(1).saturating_sub(1) as usize;
                let text = std::fs::metadata(&path).ok().filter(|m| m.len() <= 8 * 1024 * 1024).and_then(|_| std::fs::read_to_string(&path).ok()).unwrap_or_default();
                match locate(&text, s.captured().unwrap_or(""), hint) {
                    Anchor::Found(l) => rows.push(("↗", format!("open source · {label}"), note(NoteAct::OpenSourceAt(path, l)))),
                    Anchor::Moved(l) => rows.push(("↗", format!("open source · {label} · moved to line {}", l + 1), note(NoteAct::OpenSourceAt(path, l)))),
                    Anchor::Ambiguous(ls) => {
                        let n = ls.len();
                        for (i, l) in ls.into_iter().enumerate() {
                            rows.push(("↗", format!("open source · {label} · line {} · {} of {n} places", l + 1, i + 1), note(NoteAct::OpenSourceAt(path.clone(), l))));
                        }
                    }
                    Anchor::Missing => rows.push(("↗", format!("open source · {label} · the excerpt is not in it any more · at its old line"), note(NoteAct::OpenSourceAt(path, hint)))),
                }
            }
            Kind::Web => if let Some(u) = s.get("url") {
                rows.push(("↗", format!("open original · {label} · the note keeps its quote"), note(NoteAct::OpenOriginal(u.to_string()))));
            },
            Kind::Reading => if let Some(id) = s.get("library_id") {
                rows.push(("↗", format!("open saved copy · {label}"), note(NoteAct::OpenReading(id.to_string(), s.get("snapshot_sha256").map(str::to_string)))));
            },
            Kind::Terminal => if let Some(cmd) = s.get("command") {
                rows.push(("↵", format!("insert command · {} · not run", crate::app::fit_cmd(cmd, 50)), note(NoteAct::InsertCommand(cmd.to_string(), s.get("cwd_at_capture").unwrap_or("").to_string()))));
            },
            _ => {}
        }
    }
    rows
}

impl App {
    /// Formatting on the focused note. The rail folds and unfolds itself.
    pub(crate) fn note_format(&mut self, act: crate::notes_format::Act) {
        use crate::notes_format::{Act, LineTool};
        let Some(e) = self.focused_editor() else { return };
        let Some(b) = e.buf_mut().filter(|b| b.note.is_some()) else { return };
        b.ensure_md_kinds();
        match act {
            Act::Bold => b.format_inline("**"),
            Act::Italic => b.format_inline("_"),
            Act::Code => b.format_inline("`"),
            Act::Mark => b.format_inline("=="),
            Act::Link => b.format_link(),
            Act::Heading(h) => b.format_lines(LineTool::Heading(h)),
            Act::Bullet => b.format_lines(LineTool::Bullet),
            Act::Number => b.format_lines(LineTool::Number),
            Act::Check => b.format_lines(LineTool::Check),
            Act::Quote => b.format_lines(LineTool::Quote),
        }
        e.reveal();
        self.dirty = true;
    }

    /// The formatting keys, in a focused note only: ⌘⌥ (Ctrl+Alt) with a
    /// letter or digit, and ⌘↵ (Ctrl+Enter) to tick a checklist item. The
    /// app's own ⌘B, ⌘E, ⌘H and ⌘K stay the app's, and ⌥⌘H stays macOS's
    /// Hide Others: highlight is ⌘⌥M.
    pub(crate) fn note_format_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use crate::notes_format::Act;
        use winit::keyboard::{KeyCode, PhysicalKey};
        if ev.state != winit::event::ElementState::Pressed {
            return false;
        }
        let cmd = if cfg!(target_os = "macos") { self.mods.super_key() } else { self.mods.control_key() };
        let (alt, shift) = (self.mods.alt_key(), self.mods.shift_key());
        let in_note = self.tabs.get(self.active).is_some_and(|t| matches!(t.focused_ref(), Pane::Editor(e) if e.buf().is_some_and(|b| b.note.is_some() && b.ready())));
        if !in_note || !cmd || shift {
            return false;
        }
        let PhysicalKey::Code(code) = ev.physical_key else { return false };
        if !alt && code == KeyCode::Enter {
            let Some(e) = self.focused_editor() else { return false };
            let Some(b) = e.buf_mut() else { return false };
            let line = b.line_of(b.cursor);
            let ticked = b.toggle_checkbox(line);
            self.dirty = true;
            return ticked;
        }
        if !alt {
            return false;
        }
        // AltGr is Ctrl+Alt off macOS: a key that typed a character there
        // (`{` on a German 7) is typing, not formatting.
        if !cfg!(target_os = "macos") && ev.text.as_ref().is_some_and(|t| !t.chars().all(|c| c.is_ascii_alphanumeric())) {
            return false;
        }
        let act = match code {
            KeyCode::KeyB => Act::Bold,
            KeyCode::KeyI => Act::Italic,
            KeyCode::KeyE => Act::Code,
            KeyCode::KeyK => Act::Link,
            KeyCode::KeyM => Act::Mark,
            KeyCode::Digit1 => Act::Heading(1),
            KeyCode::Digit2 => Act::Heading(2),
            KeyCode::Digit3 => Act::Heading(3),
            KeyCode::Digit7 => Act::Bullet,
            KeyCode::Digit8 => Act::Number,
            KeyCode::Digit9 => Act::Check,
            KeyCode::Quote => Act::Quote,
            _ => return false,
        };
        self.note_format(act);
        true
    }

    /// A press on a note's formatting rail, or on a checklist box. True
    /// when taken.
    pub(crate) fn note_format_mouse(&mut self, x: f32, y: f32) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let mut hit = None;
        for (right, p) in [(false, Some(&mut tab.left)), (true, tab.right.as_mut())] {
            let Some(Pane::Editor(e)) = p else { continue };
            if !e.rect.contains(x, y) {
                continue;
            }
            if let Some((_, h)) = e.format_hits.iter().find(|(r, _)| r.contains(x, y)) {
                hit = Some((right, Some(*h)));
                break;
            }
            // A click on `[ ]` ticks it; anywhere else places the caret.
            let Some((line, col)) = e.cell_at(x, y) else { continue };
            let Some(b) = e.buf_mut().filter(|b| b.note.is_some()) else { continue };
            b.ensure_md_kinds();
            if b.md_kind(line) != crate::notes_format::LineKind::Text {
                continue;
            }
            let lt = b.line_text(line);
            let blk = crate::notes_format::block(&lt);
            if matches!(blk.kind, crate::notes_format::BlockKind::Check { .. }) && (blk.indent + 2..blk.indent + 5).contains(&col) {
                b.toggle_checkbox(line);
                hit = Some((right, None));
                break;
            }
        }
        let Some((right, act)) = hit else { return false };
        self.tabs[self.active].focus_right = right;
        match act {
            Some(crate::notes_ui::RailHit2::Act(a)) => self.note_format(a),
            Some(crate::notes_ui::RailHit2::Fold) => {
                self.behavior.notes_rail_folded = !self.behavior.notes_rail_folded;
                self.save_prefs();
            }
            None => {}
        }
        self.dirty = true;
        true
    }
}

/// A press on the formatting rail: an action, or folding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailHit2 {
    Act(crate::notes_format::Act),
    Fold,
}

impl App {
    /// One line of a note, styled from its Markdown: headings and bold in
    /// the bold face, code and marks on a tint, links in the signal and
    /// underlined, ticked items struck through, markers faint. Every
    /// character keeps its cell, so the caret and the mouse stay exact.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_note_line(&mut self, scene: &mut Scene, b: &crate::editor::Buffer, line: usize, at: (f32, f32, f32), scroll_col: usize, columns: usize, cell: (f32, f32), mono: Style) {
        use crate::notes_format::Style as S;
        let (ox, ly, base) = at;
        let (cw, ch) = cell;
        let full = b.line_text(line);
        let chars: Vec<char> = full.chars().collect();
        let n = chars.len();
        if scroll_col >= n {
            return;
        }
        let mut per = vec![S::Plain; n];
        for sp in crate::notes_format::styles(&full, b.md_kind(line)) {
            for c in sp.start..(sp.start + sp.len).min(n) {
                per[c] = sp.style;
            }
        }
        let t = self.theme.clone();
        let (ink, paper, signal) = (t.ink, t.paper, self.surface.signal);
        let faint = crate::surface::mix(paper, ink, 0.42);
        let px1 = self.px(1.0).max(1.0);
        let end = n.min(scroll_col + columns);
        let mut i = scroll_col;
        while i < end {
            let st = per[i];
            let mut j = i + 1;
            while j < end && per[j] == st {
                j += 1;
            }
            // (colour, bold, tint behind, underline, strike)
            let (color, bold, bg, under, strike) = match st {
                S::Plain => (ink, false, None, false, false),
                S::Marker | S::Url | S::Fence => (faint, false, None, false, false),
                S::Heading(_) | S::Bold => (ink, true, None, false, false),
                S::Italic => (signal, false, None, false, false),
                S::Code => (ink, false, Some(fade(ink, 0.07)), false, false),
                S::FenceBody => (ink, false, Some(fade(ink, 0.05)), false, false),
                S::Link => (signal, false, None, true, false),
                S::Mark => (ink, false, Some(fade(signal, 0.28)), false, false),
                S::Quote => (crate::surface::mix(paper, ink, 0.72), false, None, false, false),
                S::Bullet => (signal, false, None, false, false),
                S::Box { .. } => (signal, true, None, false, false),
                S::Done => (crate::surface::mix(paper, ink, 0.5), false, None, false, true),
            };
            let x = ox + (i - scroll_col) as f32 * cw;
            let w = (j - i) as f32 * cw;
            if let Some(bg) = bg {
                scene.rect(Rect::new(x, ly, w, ch), bg);
            }
            let run: String = chars[i..j].iter().collect::<String>().replace('\t', "    ");
            let style = Style { font: if bold { self.f.notes_bold } else { mono.font }, color, ..mono };
            self.fonts.draw(scene, style, x, base, &run);
            if under {
                scene.rect(Rect::new(x, base + self.px(2.0), w, px1), color);
            }
            if strike {
                scene.rect(Rect::new(x, ly + ch * 0.55, w, px1), color);
            }
            i = j;
        }
    }

    /// The formatting rail in a note's margin (a column of buttons), or as
    /// a row under the strip when the margin is too narrow. Folded, it is
    /// one tab. A button lights when the caret's text already is that.
    pub(crate) fn draw_format_rail(&mut self, scene: &mut Scene, e: &mut EditorPane, area: Rect, as_row: bool, folded: bool, active: &crate::notes_format::Active) {
        use crate::notes_format::RAIL;
        let t = self.theme.clone();
        let (ink, paper) = (t.ink, t.paper);
        let strong = self.label_strong();
        let (mx, my) = self.mouse;
        let hair = self.px(m::HAIRLINE);
        let btn = self.px(34.0);
        let draw = |app: &mut App, scene: &mut Scene, r: Rect, face: &str, lit: bool| {
            if lit {
                scene.rect(r, ink);
            } else if r.contains(mx, my) {
                scene.rect(r, crate::surface::mix(paper, ink, 0.08));
            }
            let st = Style { color: if lit { paper } else { ink }, ..strong };
            let tw = app.fonts.measure(st, face);
            app.fonts.draw(scene, st, (r.x + (r.w - tw) / 2.0).round(), (r.y + r.h * 0.64).round(), face);
        };
        if as_row {
            scene.rect(area, paper);
            scene.hline(area.x, area.bottom() - hair, area.w, hair, ink);
            let pad = self.px(8.0);
            if folded {
                let tw = self.fonts.measure(strong, "▸ Aa");
                let r = Rect::new(area.x + pad, area.y, tw + self.px(20.0), area.h);
                draw(self, scene, r, "▸ Aa", false);
                e.format_hits.push((r, RailHit2::Fold));
                return;
            }
            let fold = Rect::new(area.right() - btn - pad, area.y, btn, area.h);
            let mut x = area.x + pad;
            for act in RAIL {
                let face = act.face();
                let w = (self.fonts.measure(strong, face) + self.px(18.0)).max(btn);
                // What does not fit is still in the palette (`note format`).
                if x + w > fold.x - pad {
                    break;
                }
                let r = Rect::new(x, area.y, w, area.h);
                draw(self, scene, r, face, act.lit(active));
                e.format_hits.push((r, RailHit2::Act(act)));
                x += w;
            }
            draw(self, scene, fold, "◂", false);
            e.format_hits.push((fold, RailHit2::Fold));
            return;
        }
        // In the margin: a ruled column; folded, one tab.
        if folded {
            let r = Rect::new(area.x, area.y, area.w, btn);
            scene.rect(r, paper);
            scene.outline(r, hair, ink);
            draw(self, scene, r, "▸", false);
            e.format_hits.push((r, RailHit2::Fold));
            return;
        }
        scene.rect(area, paper);
        scene.outline(area, hair, ink);
        let mut y = area.y;
        for (i, act) in RAIL.into_iter().enumerate() {
            // A rule between the inline tools and the line tools.
            if i == 5 || i == 8 {
                scene.hline(area.x, y, area.w, hair, crate::surface::mix(paper, ink, 0.3));
            }
            let r = Rect::new(area.x, y, area.w, btn);
            draw(self, scene, r, act.face(), act.lit(active));
            e.format_hits.push((r, RailHit2::Act(act)));
            y += btn;
        }
        scene.hline(area.x, y, area.w, hair, ink);
        let r = Rect::new(area.x, y, area.w, btn);
        draw(self, scene, r, "◂", false);
        e.format_hits.push((r, RailHit2::Fold));
    }
}
