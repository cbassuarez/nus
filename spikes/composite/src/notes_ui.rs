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
    /// Line numbers in notes' margins, on or off.
    Numbers,
    /// This view of the note: Read view, or Focus, on or off.
    ReadView,
    Focus,
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
    /// it (itself included) that already has notes or is a repository's
    /// top, else the folder itself. Opening a subfolder never makes a
    /// second home, and a repository is its own project even inside a
    /// folder that has notes (a home folder's would otherwise take every
    /// repository under it).
    pub(crate) fn notes_project(folder: &Path) -> PathBuf {
        folder.ancestors()
            .find(|a| a.join(".nus").join("notes").is_dir() || a.join(".git").exists())
            .unwrap_or(folder)
            .to_path_buf()
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
            NoteAct::Open(p) => self.place_note(&p),
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
            NoteAct::Numbers => {
                self.behavior.notes_numbers = !self.behavior.notes_numbers;
                self.save_prefs();
            }
            NoteAct::ReadView => {
                if let Some(e) = self.focused_editor() {
                    e.read_view = !e.read_view;
                    e.reveal();
                }
            }
            NoteAct::Focus => {
                if let Some(e) = self.focused_editor() {
                    e.focus = !e.focus;
                }
            }
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
            Ok(path) if whole_tab => self.open_note(&path, true),
            Ok(path) => self.place_note(&path),
            Err(e) => self.notice_problem("Could Not Make Note", e.to_string()),
        }
    }

    /// The header's note button: which note, first. New ones, the notes
    /// open elsewhere (another tab, another window), the recent ones;
    /// typing searches them all.
    pub(crate) fn header_note(&mut self) {
        self.close_settings();
        let profile = self.project_here().is_none();
        if crate::private::enabled() { self.note_act(NoteAct::New { profile }); return; }
        index::ensure_started();
        self.open_palette(PaletteMode::NoteOpen);
    }

    /// A note into this tab where it fits: a fresh tab (only the home in
    /// it) becomes the note; beside a page or a shell it gets a tab of its
    /// own, so neither is closed; else it opens beside, or in the editor
    /// already there.
    pub(crate) fn place_note(&mut self, path: &Path) {
        let Some(tab) = self.tabs.get(self.active) else { return self.open_note(path, true) };
        if tab.right.is_none() && matches!(tab.left, Pane::Home(_)) {
            let mut e = EditorPane::new(Rect::new(0.0, 0.0, 1.0, 1.0));
            match e.open(path) {
                Ok(_) => {
                    let tab = &mut self.tabs[self.active];
                    tab.left = Pane::Editor(e);
                    tab.focus_right = false;
                    self.layout();
                    self.apply_term_resizes(false);
                    self.dirty = true;
                }
                Err(err) => self.notice_problem("Could Not Open Note", err.to_string()),
            }
            return;
        }
        let whole_tab = tab.right.as_ref().is_some_and(|p| !matches!(p, Pane::Editor(_)));
        self.open_note(path, whole_tab);
    }

    /// The note picker's rows (the header's note button).
    pub(crate) fn note_open_rows(&self, input: &str) -> Vec<crate::app::PaletteRow> {
        use crate::app::PaletteRow;
        let row = |num: &str, text: String, action: Action| PaletteRow { num: num.into(), text, action };
        let q = input.trim();
        let mut rows = Vec::new();
        let mut listed: Vec<PathBuf> = Vec::new();
        // Notes this tab already shows are not offered again.
        let here: Vec<PathBuf> = self.tabs.get(self.active).map(|t| std::iter::once(&t.left).chain(t.right.as_ref()).filter_map(|p| match p {
            Pane::Editor(e) => e.buf().filter(|b| b.note.is_some()).and_then(|b| b.path.clone()),
            _ => None,
        }).collect()).unwrap_or_default();
        let project = self.project_here();
        let id = project.as_ref().and_then(|p| Home::folder(p)).map(|h| h.id);
        // A title is a note's first line: short enough that where it is shows.
        let short = |t: &str| {
            let t = t.trim();
            if t.is_empty() { "Untitled".to_string() } else if t.chars().count() > 44 { format!("{}…", t.chars().take(43).collect::<String>().trim_end()) } else { t.to_string() }
        };
        if q.is_empty() {
            if let Some(p) = &project {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                rows.push(row("+", format!("new note in {name} · local Markdown, out of git"), Action::Note(NoteAct::New { profile: false })));
            }
            rows.push(row("+", "new personal note · sealed on this device".into(), Action::Note(NoteAct::New { profile: true })));
            // Open in another tab or window: a second view here, the same note.
            for (_, path, title, home, views) in session::open() {
                if here.contains(&path) || listed.contains(&path) {
                    continue;
                }
                let many = if views > 1 { format!(" in {views} places") } else { String::new() };
                rows.push(row("↗", format!("{} · open elsewhere{many} · {home}", short(&title)), Action::Note(NoteAct::Open(path.clone()))));
                listed.push(path);
            }
        }
        let hits: Vec<Hit> = if q.is_empty() {
            let mut v = id.as_ref().map(|id| index::recent(Some(id), 8)).unwrap_or_default();
            v.extend(index::recent_personal(8));
            v
        } else {
            match index::search(q, id.as_deref(), 16) {
                Ok(v) => v,
                Err(why) => {
                    rows.push(row("·", why, Action::Noop));
                    Vec::new()
                }
            }
        };
        for h in hits {
            if here.contains(&h.path) || listed.contains(&h.path) {
                continue;
            }
            let mut text = format!("{} · {} · {}", short(&h.title), h.home_name, notes::when(h.modified, notes::now()));
            if !h.snippet.is_empty() {
                text.push_str(&format!(" · {}", h.snippet));
            }
            listed.push(h.path.clone());
            rows.push(row("✎", text, Action::Note(NoteAct::Open(h.path))));
        }
        if !q.is_empty() && listed.is_empty() {
            rows.push(row("·", format!("no notes match “{q}”"), Action::Noop));
        }
        rows
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
            rows.push(("#", format!("line numbers · {} · every note", if self.behavior.notes_numbers { "on, turn off" } else { "off, turn on" }), Note(NoteAct::Numbers)));
            let chord = if cfg!(target_os = "macos") { "⌘⌥" } else { "Ctrl+Alt+" };
            let (reading, focusing) = self.tabs.get(self.active).and_then(|t| match t.focused_ref() { Pane::Editor(e) => Some((e.read_view, e.focus)), _ => None }).unwrap_or_default();
            rows.push(("¶", format!("{} · {chord}R", if reading { "back to writing" } else { "read view · the note as a document" }), Note(NoteAct::ReadView)));
            rows.push(("◎", format!("{} · {chord}F", if focusing { "focus off · every paragraph" } else { "focus · dim all but this paragraph" }), Note(NoteAct::Focus)));
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
            (_, Some(Ok(r))) => self.open_ref(r, note, right),
            _ => {}
        }
        self.dirty = true;
        true
    }

    /// Follow what a note points at: a page beside, a file beside at its
    /// line, another note by its identity, a clip at its place in the note.
    /// `note`: the note it is written in; `right`: the pane it is in.
    pub(crate) fn open_ref(&mut self, r: Ref, note: Option<PathBuf>, right: bool) {
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
        match r {
            Ref::Page { url, .. } => self.open_url(&url, false),
            Ref::File { path, at, .. } => {
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
            Ref::Note { note_id, home_id, .. } => {
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
            Ref::Block { line, .. } => {
                // The clip keeps the block's text: go to it in the note.
                if let Some(p) = note { open_here(self, &p, Some(line)); }
            }
        }
        self.dirty = true;
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
        // Read view and Focus: states of this view, not edits.
        if matches!(code, KeyCode::KeyR | KeyCode::KeyF) {
            if let Some(e) = self.focused_editor() {
                if code == KeyCode::KeyR {
                    e.read_view = !e.read_view;
                    e.reveal();
                } else {
                    e.focus = !e.focus;
                }
            }
            self.dirty = true;
            return true;
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
        let ctrl = if cfg!(target_os = "macos") { self.mods.super_key() } else { self.mods.control_key() };
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let mut hit = None;
        let mut follow = None;
        for (right, p) in [(false, Some(&mut tab.left)), (true, tab.right.as_mut())] {
            let Some(Pane::Editor(e)) = p else { continue };
            if !e.rect.contains(x, y) {
                continue;
            }
            if let Some((_, h)) = e.format_hits.iter().find(|(r, _)| r.contains(x, y)) {
                hit = Some((right, Some(*h)));
                break;
            }
            // A click on `[ ]` ticks it; on a link, in Read view (or with
            // Ctrl/⌘), follows it; anywhere else places the caret.
            let reading = e.read_view;
            let Some((line, col)) = e.cell_at(x, y) else { continue };
            let Some(b) = e.buf_mut().filter(|b| b.note.is_some()) else { continue };
            b.ensure_md_kinds();
            if b.md_kind(line) != crate::notes_format::LineKind::Text {
                if reading {
                    hit = Some((right, None));
                    break;
                }
                continue;
            }
            let lt = b.line_text(line);
            let blk = crate::notes_format::block(&lt);
            if matches!(blk.kind, crate::notes_format::BlockKind::Check { .. }) && (blk.indent + 2..blk.indent + 5).contains(&col) {
                b.toggle_checkbox(line);
                hit = Some((right, None));
                break;
            }
            if reading || ctrl {
                // An embed goes where it points; a link, where it links.
                let embed = crate::note_embed::parse(&lt).map(|t| crate::note_embed::link(&t));
                follow = embed.or_else(|| crate::notes_format::link_at(&lt, col)).map(|t| (t, b.path.clone(), right));
                if follow.is_some() || reading {
                    hit = Some((right, None));
                    break;
                }
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
            Some(crate::notes_ui::RailHit2::Read) => {
                if let Some(e) = self.editor_in(right) {
                    e.read_view = !e.read_view;
                    e.reveal();
                }
            }
            Some(crate::notes_ui::RailHit2::Focus) => {
                if let Some(e) = self.editor_in(right) {
                    e.focus = !e.focus;
                }
            }
            None => {}
        }
        if let Some((target, note, right)) = follow {
            self.follow_note_link(&target, note, right);
        }
        self.dirty = true;
        true
    }

    /// The editor in this tab's left or right pane.
    fn editor_in(&mut self, right: bool) -> Option<&mut EditorPane> {
        let tab = self.tabs.get_mut(self.active)?;
        match if right { tab.right.as_mut() } else { Some(&mut tab.left) } {
            Some(Pane::Editor(e)) => Some(e),
            _ => None,
        }
    }
    /// A link clicked in a note's text goes where the same link in the
    /// rails does (`open_ref`): another note by its identity, a page, a
    /// file at its line. `note`: the note it is in; `right`: its pane.
    pub(crate) fn follow_note_link(&mut self, target: &str, note: Option<PathBuf>, right: bool) {
        let target = target.trim();
        // A clip by reference: the note holding it, at the clip.
        if let Some(id) = target.strip_prefix("source:") {
            match index::clip(id) {
                Some(c) => {
                    let line = crate::notes_model::bindings(&c.body).into_iter().find(|b| b.source_id == c.source_id).map_or(0, |b| b.line + 1);
                    self.open_note_at(&c.note.path, line);
                }
                None => self.notice(icons::PENCIL, "Clip Not Found", "its note may be in another project, or in the trash"),
            }
            return;
        }
        let r = if let Some(id) = target.strip_prefix("note:") {
            let (home_id, note_id) = match id.split_once('/') {
                Some((h, n)) => (Some(h.to_string()), n.to_string()),
                None => (None, id.to_string()),
            };
            Some(Ref::Note { note_id, home_id, label: String::new(), line: 0 })
        } else if target.starts_with("http://") || target.starts_with("https://") {
            Some(Ref::Page { url: target.to_string(), line: 0 })
        } else {
            notes::file_ref(target, 0)
        };
        match r {
            Some(r) => self.open_ref(r, note, right),
            None => self.notice(icons::PENCIL, "Not A Link Nus Opens", target.to_string()),
        }
    }
}

/// A press on the formatting rail: an action, or folding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailHit2 {
    Act(crate::notes_format::Act),
    Fold,
    /// Read view and Focus, on this view of the note.
    Read,
    Focus,
}

/// The menu a note offers as you type: after `[[` (or `![[`), notes by
/// name, to link (or embed); after `/` at a line's start, what to make
/// there. The editor's completion menu shows it; Enter or Tab writes it in
/// place of what was typed. Items carry `NOTE_ITEM` so their `$0` places
/// the caret.
pub(crate) const NOTE_ITEM: &str = "nus-note";

pub(crate) fn note_completion(b: &crate::editor::Buffer) -> Option<crate::editor::Completion> {
    use nus_lsp::lsp_types::CompletionItem;
    let line = b.line_of(b.cursor);
    let col = b.col_of(b.cursor);
    let before: String = b.line_text(line).chars().take(col).collect();
    let ls = b.text.line_to_char(line);
    let item = |label: String, detail: String, insert: String| CompletionItem {
        label,
        detail: Some(detail),
        insert_text: Some(insert),
        sort_text: Some(NOTE_ITEM.into()),
        ..Default::default()
    };
    // Notes by name, after [[ (an embed after ![[).
    if let Some(at) = before.rfind("[[") {
        let q = &before[at + 2..];
        if !q.contains("]]") {
            let embed = before[..at].ends_with('!');
            let from = if embed { at - 1 } else { at };
            let start = ls + before[..from].chars().count();
            let me = b.note.as_ref().map(|v| v.key.clone());
            index::ensure_started();
            let hits = if q.trim().is_empty() { index::recent(None, 10) } else { index::search(q.trim(), None, 10).unwrap_or_default() };
            // After ![[, the clips too (blocks, pages' words, files' lines),
            // embedded by reference: the excerpt where it was captured.
            let clips: Vec<CompletionItem> = if embed {
                index::clips(q, me.as_ref().map(|k| k.home_id.as_str()), 6).into_iter().map(|c| {
                    let what = match c.kind.as_str() { "terminal" => "block", "web" => "page", "file" => "lines", "reading" => "reading", k => k };
                    let held = if c.note.title.trim().is_empty() { "Untitled".to_string() } else { c.note.title.clone() };
                    item(c.label.clone(), format!("{what} · in {held}"), format!("![[source:{}]]", c.source_id))
                }).collect()
            } else {
                Vec::new()
            };
            let mut items: Vec<CompletionItem> = hits.into_iter().filter(|h| Some(&h.key) != me.as_ref()).map(|h| {
                let id = if me.as_ref().is_some_and(|k| k.home_id == h.key.home_id) { h.key.note_id.clone() } else { format!("{}/{}", h.key.home_id, h.key.note_id) };
                let title = if h.title.trim().is_empty() { "Untitled".to_string() } else { h.title.clone() };
                if embed {
                    item(title, format!("embed · {}", h.home_name), format!("![[note:{id}]]"))
                } else {
                    let words = title.replace(['[', ']'], "");
                    item(title, h.home_name.clone(), format!("[{words}](note:{id})"))
                }
            }).collect();
            items.extend(clips);
            return (!items.is_empty()).then_some(crate::editor::Completion { items, sel: 0, at: start, scroll: 0 });
        }
    }
    // What to make, after / at a line's start.
    let trimmed = before.trim_start();
    let q = trimmed.strip_prefix('/')?;
    if q.contains(char::is_whitespace) {
        return None;
    }
    let start = ls + before.chars().count() - trimmed.chars().count();
    let today = crate::me::today();
    let all: [(&str, &str, String); 13] = [
        ("Heading 1", "# ", "# ".into()),
        ("Heading 2", "## ", "## ".into()),
        ("Heading 3", "### ", "### ".into()),
        ("Bullet list", "- ", "- ".into()),
        ("Numbered list", "1. ", "1. ".into()),
        ("Checklist", "- [ ] · a task", "- [ ] ".into()),
        ("Quote", "> ", "> ".into()),
        ("Code block", "``` ```", "```\n$0\n```".into()),
        ("Divider", "---", "---\n".into()),
        ("Link to a note", "[[", "[[".into()),
        ("Embed a note or a clip", "![[ · live", "![[".into()),
        ("Embed a file's lines", "![[path#L1-L10]] · live", "![[$0#L1-L10]]".into()),
        ("Today's date", "", today.clone()),
    ];
    let ql = q.to_lowercase();
    let items: Vec<CompletionItem> = all.into_iter()
        .filter(|(name, _, _)| ql.is_empty() || name.to_lowercase().split_whitespace().any(|w| w.starts_with(&ql)) || name.to_lowercase().starts_with(&ql))
        .map(|(name, hint, insert)| item(name.to_string(), if hint.is_empty() { today.clone() } else { hint.to_string() }, insert))
        .collect();
    (!items.is_empty()).then_some(crate::editor::Completion { items, sel: 0, at: start, scroll: 0 })
}

/// What a note's body is drawn into, and with what (editor.rs hands it over).
pub(crate) struct NoteFrame {
    pub pane: Rect,
    /// The column: where rows start, how wide they may run.
    pub left: f32,
    pub top: f32,
    pub bottom: f32,
    pub width: f32,
    /// A body row's height, and its baseline from the row's top.
    pub ch: f32,
    pub baseline_off: f32,
    pub mono: Style,
    /// The line numbers' style, when they are on.
    pub numbers: Option<Style>,
    pub focused: bool,
    pub caret: bool,
    pub overwrite: bool,
    /// Scroll until the caret's row is on screen.
    pub reveal: bool,
    /// Read view: every line formatted, no caret.
    pub read: bool,
    /// Focus: all but the caret's paragraph dimmed.
    pub focus: bool,
    pub sel: Option<(usize, usize)>,
    pub matches: Vec<(usize, usize)>,
    pub find_cur: Option<(usize, usize)>,
    pub wash: nus_render::Color,
    pub sel_color: nus_render::Color,
    pub match_color: nus_render::Color,
    pub match_cur: nus_render::Color,
}

/// How one character of a note is shown. Off the caret's line (and in
/// Read view) the Markdown is the document: its markers take no room.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shown {
    /// As typed.
    Char(char),
    /// Not at all: `#`, `**`, backticks, a link's address.
    Hidden,
    /// A list's dash, as a bullet.
    Bullet,
    /// A checkbox's `[ ]`, as one box.
    Box,
    /// A quote's `>`, as a bar beside its words.
    Bar,
}

/// One line of a note, measured as it will be drawn: its characters, each
/// one's Markdown style and how it shows, where each starts (one stop more
/// than characters), how far rows after the first stand in, and a
/// heading's size and the room above it.
struct NoteLine {
    chars: Vec<char>,
    styles: Vec<crate::notes_format::Style>,
    shown: Vec<Shown>,
    xs: Vec<f32>,
    hang: f32,
    scale: f32,
    above: f32,
    /// A line that is only an embed: what it points at, and what shows.
    embed: Option<(crate::note_embed::Target, crate::note_embed::Shown)>,
}

/// (colour, bold, tint behind, underline, strike) for a Markdown style.
fn look_of(st: crate::notes_format::Style, ink: nus_render::Color, paper: nus_render::Color, signal: nus_render::Color) -> (nus_render::Color, bool, Option<nus_render::Color>, bool, bool) {
    use crate::notes_format::Style as S;
    let faint = crate::surface::mix(paper, ink, 0.42);
    match st {
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
    }
}

/// A heading's size against the body's, and the room above its first row
/// (in body rows).
fn heading_scale(level: Option<u8>) -> (f32, f32) {
    match level {
        Some(1) => (1.6, 0.5),
        Some(2) => (1.3, 0.35),
        Some(3) => (1.1, 0.2),
        _ => (1.0, 0.0),
    }
}

impl App {
    /// Runs of one style (a tab is a run of its own): `(from, to, style)`.
    fn note_runs(n: usize, chars: &[char], styles: &[crate::notes_format::Style]) -> Vec<(usize, usize, crate::notes_format::Style)> {
        let mut runs = Vec::new();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            if chars[i] != '\t' {
                while j < n && styles[j] == styles[i] && chars[j] != '\t' {
                    j += 1;
                }
            }
            runs.push((i, j, styles[i]));
            i = j;
        }
        runs
    }

    /// A note's line, measured run by run in the faces and size it is drawn
    /// in. `raw`: the caret's line while writing, its Markdown as typed.
    fn measure_note_line(&self, b: &crate::editor::Buffer, line: usize, mono: Style, raw: bool, ch: f32) -> NoteLine {
        use crate::notes_format::{BlockKind, LineKind, Style as S};
        let full = b.line_text(line);
        let chars: Vec<char> = full.chars().collect();
        let n = chars.len();
        let kind = b.md_kind(line);
        let mut styles = vec![S::Plain; n];
        for sp in crate::notes_format::styles(&full, kind) {
            for s in styles.iter_mut().take((sp.start + sp.len).min(n)).skip(sp.start) {
                *s = sp.style;
            }
        }
        // An embed off the caret's line is a box of what it points at.
        if !raw && kind == LineKind::Text {
            if let Some(target) = crate::note_embed::parse(&full) {
                let shown = self.embed_content(b, &target);
                return NoteLine { chars, styles, shown: vec![Shown::Hidden; n], xs: vec![0.0; n + 1], hang: 0.0, scale: 1.0, above: 0.0, embed: Some((target, shown)) };
            }
        }
        let blk = (kind == LineKind::Text).then(|| crate::notes_format::block(&full));
        let (scale, above) = heading_scale(blk.as_ref().and_then(|b| match b.kind { BlockKind::Heading(l) => Some(l), _ => None }));
        let mut shown: Vec<Shown> = chars.iter().map(|&c| Shown::Char(c)).collect();
        if let Some(bk) = blk.as_ref().filter(|_| !raw) {
            for (s, st) in shown.iter_mut().zip(&styles) {
                if matches!(st, S::Marker | S::Url) {
                    *s = Shown::Hidden;
                }
            }
            let at = bk.indent.min(n);
            match bk.kind {
                BlockKind::Bullet(_) if at < n => shown[at] = Shown::Bullet,
                BlockKind::Check { .. } => {
                    for (k, s) in shown.iter_mut().enumerate().take((at + 5).min(n)).skip(at) {
                        *s = if k < at + 2 { Shown::Hidden } else { Shown::Box };
                    }
                }
                BlockKind::Quote if at < n => shown[at] = Shown::Bar,
                _ => {}
            }
        }
        let (ink, paper, signal) = (self.theme.ink, self.theme.paper, self.surface.signal);
        let size = (mono.px * scale).round();
        let bar_w = (size * 0.9).round();
        let mut xs = Vec::with_capacity(n + 1);
        xs.push(0.0);
        let mut x = 0.0;
        for (a, z, st) in Self::note_runs(n, &chars, &styles) {
            let bold = look_of(st, ink, paper, signal).1;
            let style = Style { font: if bold { self.f.notes_bold } else { mono.font }, px: size, ..mono };
            let mut i = a;
            while i < z {
                match shown[i] {
                    Shown::Hidden => {
                        xs.push(x);
                        i += 1;
                    }
                    Shown::Bar => {
                        x += bar_w;
                        xs.push(x);
                        i += 1;
                    }
                    Shown::Char('\t') => {
                        x += self.fonts.measure_as_is(style, "    ");
                        xs.push(x);
                        i += 1;
                    }
                    _ => {
                        let mut j = i;
                        let mut text = String::new();
                        while j < z && !matches!(shown[j], Shown::Hidden | Shown::Bar | Shown::Char('\t')) {
                            text.push(if shown[j] == Shown::Bullet { '•' } else { chars[j] });
                            j += 1;
                        }
                        let stops = self.fonts.stops(style, &text);
                        for k in 1..=(j - i) {
                            xs.push(x + stops.get(k).copied().unwrap_or(0.0));
                        }
                        x += stops.last().copied().unwrap_or(0.0);
                        i = j;
                    }
                }
            }
        }
        // A list item's (or a quote's) further rows stand under its words.
        let hang = match blk.as_ref().map(|b| (b.kind, (b.indent + b.marker_len).min(n))) {
            Some((BlockKind::Para | BlockKind::Heading(_), _)) | None => 0.0,
            Some((_, body)) => xs.get(body).copied().unwrap_or(0.0),
        };
        NoteLine { chars, styles, shown, xs, hang, scale, above: above * ch, embed: None }
    }

    /// What an embed shows now: a file's lines, another note's opening
    /// lines (its open session's text first), a page's address.
    fn embed_content(&self, b: &crate::editor::Buffer, target: &crate::note_embed::Target) -> crate::note_embed::Shown {
        use crate::note_embed as em;
        match target {
            em::Target::File { path, lines } => {
                let base = b.path.as_deref().and_then(notes::folder_of);
                let since = b.path.as_deref().and_then(|p| std::fs::metadata(p).ok()?.modified().ok());
                em::file(path, *lines, base.as_deref(), since)
            }
            em::Target::Note { id, home } => {
                let home_id = home.clone().or_else(|| b.note.as_ref().map(|v| v.key.home_id.clone())).unwrap_or_default();
                let key = NoteKey { home_id, note_id: id.clone() };
                let live = session::document(&key).map(|d| (d.title().to_string(), d.body.clone()));
                match live.or_else(|| index::body_of(&key)) {
                    Some((t, body)) => em::note(&t, Some(&body)),
                    None => em::note("", None),
                }
            }
            em::Target::Page(u) => {
                // Live while a tab shows it; else as history last saw it.
                let canon = crate::keep::canon(u);
                let open = self.tabs.iter().flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref())).find_map(|p| match p {
                    Pane::Web(w) => {
                        let s = w.tab.shared.borrow();
                        (crate::keep::canon(&s.url) == canon).then(|| s.title.clone())
                    }
                    _ => None,
                });
                let seen = || self.recent.iter().find_map(|r| match &r.item {
                    crate::start::Saved::Page { url, title } if crate::keep::canon(url) == canon && !title.is_empty() => Some(title.clone()),
                    _ => None,
                });
                let title = open.clone().filter(|t| !t.trim().is_empty()).or_else(seen);
                em::page(u, title.as_deref(), open.is_some())
            }
            em::Target::Source { id } => {
                let Some(c) = index::clip(id) else { return em::clip("", "", "", None, false) };
                // The note holding it, as it stands if open (its excerpt
                // may have been edited since); else as last saved.
                let doc = session::document(&c.note.key);
                let body = doc.as_ref().map(|d| d.body.clone()).unwrap_or(c.body.clone());
                let excerpt = crate::notes_model::bindings(&body).into_iter().find(|b| b.source_id == c.source_id).and_then(|b| b.text);
                let edited = doc.as_ref().is_some_and(|d| crate::notes_model::evidence(d).iter().any(|(b, e)| b.source_id == c.source_id && *e == crate::notes_model::Evidence::Edited));
                em::clip(&c.label, &c.kind, &c.note.title, excerpt.as_deref(), edited)
            }
        }
    }

    /// An embed's box: its head, then its lines.
    fn embed_height(s: &crate::note_embed::Shown, ch: f32) -> f32 {
        ch * 0.5 + ch * 0.8 * (1 + s.lines.len()) as f32
    }

    /// An embed as a box in the note: a bar in the signal, what it is (and
    /// a word when its source moved on), then its lines, in the editor's
    /// face for code. Returns the head's baseline.
    fn draw_embed(&mut self, scene: &mut Scene, target: &crate::note_embed::Target, s: &crate::note_embed::Shown, r: Rect, f: &NoteFrame, dim: bool) -> f32 {
        use crate::note_embed::Target;
        let (ink, paper, signal) = (self.theme.ink, self.theme.paper, self.surface.signal);
        let lh = f.ch * 0.8;
        let bx = Rect::new(r.x, r.y + f.ch * 0.15, r.w, r.h - f.ch * 0.3);
        scene.rect(bx, crate::surface::mix(paper, ink, if dim { 0.02 } else { 0.045 }));
        scene.rect(Rect::new(bx.x, bx.y, self.px(2.0).max(1.0), bx.h), if dim { fade(signal, 0.4) } else { signal });
        let pad = self.px(12.0);
        let text = if dim { crate::surface::mix(paper, ink, 0.35) } else { ink };
        let head = Style { color: text, ..self.label_strong() };
        let hb = (bx.y + lh * 0.75).round();
        let isz = self.px(12.0);
        let icon = match target { Target::Note { .. } => icons::PENCIL, Target::File { .. } => icons::CODE, Target::Page(_) => icons::GLOBE, Target::Source { .. } => icons::TERMINAL };
        self.fonts.draw_icon(scene, icon, isz, bx.x + pad, hb - isz + self.px(1.0), text);
        let hx = bx.x + pad + isz + self.px(8.0);
        // A file's or a note's name as it is, not as a label.
        let shown = self.fit_as_is(head, &s.head, bx.w * 0.55).into_owned();
        let hw = self.fonts.draw_as_is(scene, head, hx, hb, &shown);
        if let Some(n) = &s.note {
            let ns = Style { color: if dim { fade(signal, 0.5) } else { signal }, ..self.label() };
            let words = self.fit(ns, n, (bx.right() - pad - hx - hw - self.px(14.0)).max(0.0)).into_owned();
            self.fonts.draw(scene, ns, hx + hw + self.px(14.0), hb, &words);
        }
        let body = Style { font: if s.code { self.f.editor } else { f.mono.font }, px: (f.mono.px * 0.82).round(), color: text, tracking: 0.0 };
        for (k, l) in s.lines.iter().enumerate() {
            let by = (bx.y + lh * (k as f32 + 1.0) + lh * 0.75).round();
            let t = self.fit(body, l, bx.w - 2.0 * pad).into_owned();
            self.fonts.draw_as_is(scene, body, bx.x + pad, by, &t);
        }
        hb
    }

    /// A measured line's rows at a width: (start, end) columns.
    fn note_line_rows(l: &NoteLine, width: f32) -> Vec<(usize, usize)> {
        if l.embed.is_some() {
            return vec![(0, l.chars.len())];
        }
        let widths: Vec<f32> = l.xs.windows(2).map(|w| w[1] - w[0]).collect();
        crate::note_layout::wrap(&l.chars, &widths, width, l.hang.min(width * 0.5))
    }

    /// How tall a measured line stands, wrapped at a width.
    fn note_line_height(l: &NoteLine, width: f32, ch: f32) -> f32 {
        if let Some((_, s)) = &l.embed {
            return Self::embed_height(s, ch);
        }
        l.above + Self::note_line_rows(l, width).len() as f32 * ch * l.scale
    }

    /// A note's text in its own face, wrapped to the column, its lines
    /// numbered in the margin; off the caret's line its Markdown is the
    /// document (live preview). The selection, the find's matches, the
    /// caret. Returns the rows as drawn, for a click and the keys.
    pub(crate) fn draw_note_body(&mut self, scene: &mut Scene, b: &mut crate::editor::Buffer, f: &NoteFrame) -> crate::note_layout::Layout {
        use crate::note_layout::{Layout, Row};
        let n_lines = b.text.len_lines();
        let cur_line = b.line_of(b.cursor);
        let cur_col = b.col_of(b.cursor);
        let raw = |line: usize| !f.read && line == cur_line;
        // Focus: the caret's paragraph (the lines between blank ones).
        let para = f.focus.then(|| {
            let blank = |l: usize| b.line_text(l).trim().is_empty();
            let (mut p0, mut p1) = (cur_line, cur_line);
            while p0 > 0 && !blank(p0) && !blank(p0 - 1) {
                p0 -= 1;
            }
            while p1 + 1 < n_lines && !blank(p1) && !blank(p1 + 1) {
                p1 += 1;
            }
            (p0, p1)
        });
        // The caret's row on screen: the first line shown goes back only as
        // far as still leaves room for every row down to the caret's.
        if f.reveal {
            if cur_line < b.scroll {
                b.scroll = cur_line;
            } else {
                let here = self.measure_note_line(b, cur_line, f.mono, raw(cur_line), f.ch);
                let rows = Self::note_line_rows(&here, f.width);
                let k = rows.iter().position(|&(s, e)| cur_col >= s && cur_col < e).unwrap_or(rows.len() - 1);
                let mut used = here.above + (k + 1) as f32 * f.ch * here.scale;
                let mut first = cur_line;
                while first > b.scroll {
                    let above = self.measure_note_line(b, first - 1, f.mono, raw(first - 1), f.ch);
                    let h = Self::note_line_height(&above, f.width, f.ch);
                    if used + h > f.bottom - f.top + 0.5 {
                        break;
                    }
                    used += h;
                    first -= 1;
                }
                b.scroll = b.scroll.max(first);
            }
        }
        let (ink, paper, signal) = (self.theme.ink, self.theme.paper, self.surface.signal);
        let px1 = self.px(1.0).max(1.0);
        let past = self.px(5.0);
        let num_gap = self.px(10.0);
        let bar = self.px(2.0).max(1.0);
        let mut layout = Layout::default();
        let mut y = f.top;
        let mut line = b.scroll;
        'lines: while line < n_lines && y < f.bottom {
            let l = self.measure_note_line(b, line, f.mono, raw(line), f.ch);
            let dim = para.is_some_and(|(p0, p1)| line < p0 || line > p1);
            if let Some((target, shown)) = &l.embed {
                let h = Self::embed_height(shown, f.ch);
                if y + h > f.bottom + 0.5 && !layout.rows.is_empty() {
                    break 'lines;
                }
                let base = self.draw_embed(scene, target, shown, Rect::new(f.left, y, f.width, h), f, dim);
                if let Some(ns) = f.numbers {
                    let num = (line + 1).to_string();
                    let w = self.fonts.measure_as_is(ns, &num);
                    self.fonts.draw_as_is(scene, ns, (f.left - num_gap - w).round(), base, &num);
                }
                let len = l.chars.len();
                layout.rows.push(Row { line, start: 0, end: len, y, h, base, px: f.mono.px, x0: f.left, xs: vec![0.0; len + 1], last: true });
                y += h;
                line += 1;
                continue;
            }
            let spans = Self::note_line_rows(&l, f.width);
            let ls = b.text.line_to_char(line);
            let len = l.chars.len();
            let last_k = spans.len() - 1;
            let runs = Self::note_runs(len, &l.chars, &l.styles);
            let rh = f.ch * l.scale;
            let size = (f.mono.px * l.scale).round();
            for (k, &(s, e)) in spans.iter().enumerate() {
                let above = if k == 0 { l.above } else { 0.0 };
                if y + above + rh > f.bottom + 0.5 {
                    break 'lines;
                }
                let x0 = f.left + if k > 0 { l.hang.min(f.width * 0.5) } else { 0.0 };
                let top = y + above;
                let base = top + f.baseline_off * l.scale;
                let row = Row { line, start: s, end: e, y, h: above + rh, base, px: size, x0, xs: l.xs[s..=e].iter().map(|x| x - l.xs[s]).collect(), last: k == last_k };
                if line == cur_line && f.focused && !f.read {
                    scene.rect(Rect::new(f.pane.x, top, f.pane.w, rh), f.wash);
                }
                // A quote's bar runs beside every row of it, not only the first.
                if k > 0 {
                    if let Some(i) = l.shown.iter().position(|s| *s == Shown::Bar) {
                        let x = f.left + l.xs[i] + self.px(2.0);
                        let h = if k == last_k { rh * 0.85 } else { rh };
                        scene.rect(Rect::new(x, top, bar, h), crate::surface::mix(paper, ink, if dim { 0.15 } else { 0.35 }));
                    }
                }
                if k == 0 {
                    if let Some(ns) = f.numbers {
                        let num = (line + 1).to_string();
                        let ns = if line == cur_line && !f.read { Style { color: fade(ink, 0.7), ..ns } } else { ns };
                        let w = self.fonts.measure_as_is(ns, &num);
                        self.fonts.draw_as_is(scene, ns, (f.left - num_gap - w).round(), base, &num);
                    }
                }
                // Under the words: the selection, then the find's matches.
                let (rs, re) = (ls + s, ls + e);
                let wash = |scene: &mut Scene, a: usize, z: usize, color| {
                    let (s0, s1) = (a.max(rs), z.min(re));
                    let past_end = row.last && z > ls + len;
                    if s1 > s0 || (past_end && a <= re) {
                        let x = row.x(s0 - ls);
                        let w = row.x(s1.max(s0) - ls) - x + if past_end { past } else { 0.0 };
                        scene.rect(Rect::new(x, top, w.max(px1), rh), color);
                    }
                };
                if let Some((a, z)) = f.sel {
                    wash(scene, a, z, f.sel_color);
                }
                for &(a, z) in &f.matches {
                    wash(scene, a, z, if Some((a, z)) == f.find_cur { f.match_cur } else { f.match_color });
                }
                // The words, run by run, each piece at its own stop.
                for &(a, z, st) in &runs {
                    let (a, z) = (a.max(s), z.min(e));
                    if z <= a {
                        continue;
                    }
                    let (mut color, bold, bg, under, strike) = look_of(st, ink, paper, signal);
                    if dim {
                        color = crate::surface::mix(paper, color, 0.3);
                    }
                    let style = Style { font: if bold { self.f.notes_bold } else { f.mono.font }, px: size, color, ..f.mono };
                    let mut i = a;
                    while i < z {
                        match l.shown[i] {
                            Shown::Hidden | Shown::Char('\t') => i += 1,
                            Shown::Bar => {
                                let x = row.x(i) + self.px(2.0);
                                let h = if last_k > 0 { rh * 0.85 } else { rh * 0.7 };
                                scene.rect(Rect::new(x, top + rh * 0.15, bar, h), crate::surface::mix(paper, ink, if dim { 0.15 } else { 0.35 }));
                                i += 1;
                            }
                            Shown::Box => {
                                let mut j = i;
                                while j < z && l.shown[j] == Shown::Box {
                                    j += 1;
                                }
                                let checked = matches!(st, crate::notes_format::Style::Box { checked: true });
                                let side = (size * 0.68).round();
                                let x = (row.x(i) + (row.x(j) - row.x(i) - side) / 2.0).round();
                                let by = (base - side * 0.9).round();
                                let r = Rect::new(x, by, side, side);
                                if checked {
                                    scene.rect(r, color);
                                    self.fonts.draw_icon(scene, nus_render::text::icons::CHECK, side, x, by, paper);
                                } else {
                                    scene.outline(r, self.px(1.5), color);
                                }
                                i = j;
                            }
                            _ => {
                                let mut j = i;
                                let mut text = String::new();
                                while j < z && !matches!(l.shown[j], Shown::Hidden | Shown::Bar | Shown::Box | Shown::Char('\t')) {
                                    text.push(if l.shown[j] == Shown::Bullet { '•' } else { l.chars[j] });
                                    j += 1;
                                }
                                let x = row.x(i);
                                let w = row.x(j) - x;
                                if let Some(bg) = bg {
                                    scene.rect(Rect::new(x, top, w, rh), bg);
                                }
                                // As typed: a note is not a label, whatever its tracking.
                                self.fonts.draw_as_is(scene, style, x, base, &text);
                                if under {
                                    scene.rect(Rect::new(x, base + self.px(2.0), w, px1), color);
                                }
                                if strike {
                                    scene.rect(Rect::new(x, top + rh * 0.55, w, px1), color);
                                }
                                i = j;
                            }
                        }
                    }
                }
                y += above + rh;
                layout.rows.push(row);
            }
            line += 1;
        }
        // The caret, where the rows put it (not in Read view).
        if f.caret && !f.read {
            if let Some(i) = layout.row_of(cur_line, cur_col) {
                let r = &layout.rows[i];
                let (x, base, size) = (r.x(cur_col), r.base, r.px);
                if f.sel.is_some() {
                    self.draw_selection_edge(scene, x, base, size, 1.0, self.last_key);
                } else {
                    let replacement = f.overwrite.then(|| b.replacement_len()).flatten().map(|n| r.x(cur_col + n) - x);
                    self.draw_text_caret(scene, x, base, size, 1.0, self.last_key, replacement);
                }
            }
        }
        layout
    }

    /// The formatting rail in a note's margin (a column of buttons), or as
    /// a row under the strip when the margin is too narrow. Folded, it is
    /// one tab. A button lights when the caret's text already is that.
    /// `reserve`: the row's right end is left to the pane's own controls.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_format_rail(&mut self, scene: &mut Scene, e: &mut EditorPane, area: Rect, as_row: bool, folded: bool, active: &crate::notes_format::Active, reserve: f32) {
        use crate::notes_format::RAIL;
        let t = self.theme.clone();
        let (ink, paper) = (t.ink, t.paper);
        let strong = self.label_strong();
        let (mx, my) = self.mouse;
        let hair = self.px(m::HAIRLINE);
        let btn = self.px(34.0);
        let chord = if cfg!(target_os = "macos") { "⌘⌥" } else { "Ctrl+Alt+" };
        let (reading, focusing) = (e.read_view, e.focus);
        // Each button says what it does, and its keys, when the pointer rests.
        let words = |hit: RailHit2, folded: bool| match hit {
            RailHit2::Act(act) => {
                let (name, key) = act.name();
                format!("{name} · {chord}{key}")
            }
            RailHit2::Fold if folded => "Show the formatting tools".to_string(),
            RailHit2::Fold => "Fold the formatting tools away".to_string(),
            RailHit2::Read if reading => format!("Back to writing · {chord}R"),
            RailHit2::Read => format!("Read view · {chord}R"),
            RailHit2::Focus if focusing => format!("Show every paragraph · {chord}F"),
            RailHit2::Focus => format!("Focus on this paragraph · {chord}F"),
        };
        let draw = |app: &mut App, scene: &mut Scene, r: Rect, face: &str, lit: bool, hit: RailHit2| {
            if lit {
                scene.rect(r, ink);
            } else if r.contains(mx, my) {
                scene.rect(r, crate::surface::mix(paper, ink, 0.08));
            }
            let st = Style { color: if lit { paper } else { ink }, ..strong };
            let tw = app.fonts.measure_as_is(st, face);
            app.fonts.draw_as_is(scene, st, (r.x + (r.w - tw) / 2.0).round(), (r.y + r.h * 0.64).round(), face);
            app.offer_tip(crate::app::hover_key(&format!("note-rail:{hit:?}"), 0), r, words(hit, folded));
        };
        // Read and Focus are states of this view: icons, lit while on.
        let toggle = |app: &mut App, scene: &mut Scene, r: Rect, hit: RailHit2| {
            let (icon, lit) = if hit == RailHit2::Read { (icons::BOOK, reading) } else { (icons::CROSSHAIR, focusing) };
            if lit {
                scene.rect(r, ink);
            }
            let s = app.px(15.0);
            let color = if lit { paper } else { ink };
            app.icon_button(scene, icon, s, (r.x + (r.w - s) / 2.0).round(), (r.y + (r.h - s) / 2.0).round(), color, r, crate::app::hover_key(&format!("note-rail:{hit:?}"), 1), crate::app::IconMotion::Still);
            app.offer_tip(crate::app::hover_key(&format!("note-rail:{hit:?}"), 0), r, words(hit, folded));
        };
        if as_row {
            let area = Rect::new(area.x, area.y, (area.w - reserve).max(btn * 2.0), area.h);
            scene.rect(area, paper);
            scene.hline(area.x, area.bottom() - hair, area.w, hair, ink);
            let pad = self.px(8.0);
            if folded {
                let tw = self.fonts.measure_as_is(strong, "▸ Aa");
                let r = Rect::new(area.x + pad, area.y, tw + self.px(20.0), area.h);
                draw(self, scene, r, "▸ Aa", false, RailHit2::Fold);
                e.format_hits.push((r, RailHit2::Fold));
                return;
            }
            let fold = Rect::new(area.right() - btn - pad, area.y, btn, area.h);
            let read = Rect::new(fold.x - 2.0 * btn - pad, area.y, btn, area.h);
            let focus = Rect::new(fold.x - btn - pad, area.y, btn, area.h);
            toggle(self, scene, read, RailHit2::Read);
            e.format_hits.push((read, RailHit2::Read));
            toggle(self, scene, focus, RailHit2::Focus);
            e.format_hits.push((focus, RailHit2::Focus));
            let mut x = area.x + pad;
            for act in RAIL {
                let face = act.face();
                let w = (self.fonts.measure(strong, face) + self.px(18.0)).max(btn);
                // What does not fit is still in the palette (`note format`).
                if x + w > read.x - pad {
                    break;
                }
                let r = Rect::new(x, area.y, w, area.h);
                draw(self, scene, r, face, act.lit(active), RailHit2::Act(act));
                e.format_hits.push((r, RailHit2::Act(act)));
                x += w;
            }
            draw(self, scene, fold, "◂", false, RailHit2::Fold);
            e.format_hits.push((fold, RailHit2::Fold));
            return;
        }
        // In the margin: a ruled column; folded, one tab.
        if folded {
            let r = Rect::new(area.x, area.y, area.w, btn);
            scene.rect(r, paper);
            scene.outline(r, hair, ink);
            draw(self, scene, r, "▸", false, RailHit2::Fold);
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
            draw(self, scene, r, act.face(), act.lit(active), RailHit2::Act(act));
            e.format_hits.push((r, RailHit2::Act(act)));
            y += btn;
        }
        scene.hline(area.x, y, area.w, hair, crate::surface::mix(paper, ink, 0.3));
        for hit in [RailHit2::Read, RailHit2::Focus] {
            let r = Rect::new(area.x, y, area.w, btn);
            toggle(self, scene, r, hit);
            e.format_hits.push((r, hit));
            y += btn;
        }
        scene.hline(area.x, y, area.w, hair, ink);
        let r = Rect::new(area.x, y, area.w, btn);
        draw(self, scene, r, "◂", false, RailHit2::Fold);
        e.format_hits.push((r, RailHit2::Fold));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_is_its_own_project_even_under_a_folder_with_notes() {
        let root = std::env::temp_dir().join(format!("nus-notes-project-{}", std::process::id()));
        let home = root.join("home");
        let repo = home.join("work").join("repo");
        std::fs::create_dir_all(home.join(".nus").join("notes")).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src").join("deep")).unwrap();
        std::fs::create_dir_all(home.join("loose")).unwrap();
        // Inside the repository: its top, however deep.
        assert_eq!(App::notes_project(&repo.join("src").join("deep")), repo);
        // Outside any repository, under the folder with notes: that folder.
        assert_eq!(App::notes_project(&home.join("loose")), home);
        // A repository with notes in a subfolder: the nearer one wins.
        std::fs::create_dir_all(repo.join("src").join(".nus").join("notes")).unwrap();
        assert_eq!(App::notes_project(&repo.join("src").join("deep")), repo.join("src"));
        let _ = std::fs::remove_dir_all(&root);
    }
}