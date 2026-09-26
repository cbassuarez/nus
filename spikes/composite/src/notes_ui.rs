//! Notes in the window (notes.rs is the files). A note is the editor pane
//! on a markdown file under `.nus/notes/` or `profile/notes/`: beside a
//! shell it is a peer like a page, and given the width of a whole tab it
//! grows two rails, the notes of this folder and your profile on the left
//! and what the note points at on the right. CLIP puts a finished block
//! into the note beside it (or the folder's inbox when none is open).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nus_render::text::{icons, Style};
use nus_render::{Rect, Scene};

use crate::app::{App, Caps, Pane};
use crate::editor::EditorPane;
use crate::notes::{self, Entry, Place, Ref};
use nus_render::theme::metric as m;

/// What the palette asks of notes.
#[derive(Clone, PartialEq, Debug)]
pub enum NoteAct {
    /// A new note beside the shell: in the folder, or the profile.
    New { profile: bool },
    /// The notes, as a whole tab.
    Tab,
    /// The focused (or last) block, clipped into the note.
    ClipBlock,
    /// The page beside, as a link line in the note.
    AddPage,
    /// The folder's inbox up in the hatch: the scratch sheet is the hatch.
    Hatch,
    Open(PathBuf),
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
    folder_notes: Vec<Entry>,
    profile_notes: Vec<Entry>,
    refs: Vec<Ref>,
    backlinks: Vec<(PathBuf, usize)>,
    revision: Option<u64>,
    listed: Option<Instant>,
    newest: u64,
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
    fn notes_folder(&self) -> Option<PathBuf> {
        self.focused_cwd().map(PathBuf::from).filter(|p| p.is_dir()).or_else(|| self.files_root.clone())
    }

    pub(crate) fn note_act(&mut self, act: NoteAct) {
        if crate::private::enabled() {
            self.notice(icons::EYE_SLASH, "Not In Incognito", "notes are files; a private window keeps none");
            return;
        }
        match act {
            NoteAct::New { profile } => self.new_note(profile, false),
            NoteAct::Tab => self.open_notes_tab(),
            NoteAct::ClipBlock => self.clip_block(None),
            NoteAct::AddPage => self.add_page_to_note(),
            NoteAct::Hatch => self.note_in_hatch(),
            NoteAct::Open(p) => self.open_note(&p, false),
        }
    }

    /// Make a note and open it: beside the shell, or in the pane asking.
    pub(crate) fn new_note(&mut self, profile: bool, whole_tab: bool) {
        let dir = if profile {
            notes::profile_dir()
        } else {
            let Some(folder) = self.notes_folder() else {
                self.notice(icons::PENCIL, "No Folder Here", "open a shell in a folder, or make a profile note");
                return;
            };
            if let Err(e) = notes::exclude_from_git(&folder) {
                tracing::warn!("notes: could not update .git/info/exclude: {e}");
            }
            notes::folder_dir(&folder)
        };
        match notes::create(&dir, "", "") {
            Ok(path) => self.open_note(&path, whole_tab),
            Err(e) => self.notice_problem("Could Not Make Note", e.to_string()),
        }
    }

    /// A note in the editor: a new tab of its own (the rails show when it
    /// is wide), else beside the shell.
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

    /// The notes as a whole tab: the newest note of this folder, else of
    /// the profile, else the folder's inbox.
    pub(crate) fn open_notes_tab(&mut self) {
        let folder = self.notes_folder();
        let newest = folder.as_ref().and_then(|f| notes::list(&notes::folder_dir(f)).into_iter().next())
            .or_else(|| notes::list(&notes::profile_dir()).into_iter().next())
            .map(|e| e.path);
        let path = match (newest, folder) {
            (Some(p), _) => p,
            (None, Some(f)) => {
                let _ = notes::exclude_from_git(&f);
                match notes::inbox(&f) {
                    Ok(p) => p,
                    Err(e) => return self.notice_problem("Could Not Make Note", e.to_string()),
                }
            }
            (None, None) => return self.new_note(true, true),
        };
        self.open_note(&path, true);
    }

    /// The folder's inbox as the hatch's tab. No sheet of its own: the
    /// hatch already carries any tab over everything, so a note is HOISTed
    /// like a shell would be, and LAND brings it down. A tab already
    /// showing the inbox is the one that goes up, never a second buffer.
    pub(crate) fn note_in_hatch(&mut self) {
        let path = match self.notes_folder() {
            Some(f) => {
                let _ = notes::exclude_from_git(&f);
                match notes::inbox(&f) {
                    Ok(p) => p,
                    Err(e) => return self.notice_problem("Could Not Make Note", e.to_string()),
                }
            }
            None => match notes::list(&notes::profile_dir()).into_iter().next() {
                Some(e) => e.path,
                None => match notes::create(&notes::profile_dir(), "", "") {
                    Ok(p) => p,
                    Err(e) => return self.notice_problem("Could Not Make Note", e.to_string()),
                },
            },
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

    /// The note open in this tab, as (right pane?, buffer index), when its
    /// buffer is ready for text.
    fn note_in_tab(&self) -> Option<(bool, usize)> {
        let tab = self.tabs.get(self.active)?;
        let panes = [(false, Some(&tab.left)), (true, tab.right.as_ref())];
        let order = if tab.focus_right { [panes[1], panes[0]] } else { panes };
        order.into_iter().find_map(|(right, p)| match p {
            Some(Pane::Editor(e)) => e.buf().filter(|b| b.ready() && b.path.as_deref().is_some_and(|p| notes::place_of(p).is_some())).map(|_| (right, e.active)),
            _ => None,
        })
    }

    /// Put `text` at the end of the note beside, saving it, or of the
    /// folder's inbox (then open it beside). Says where it went.
    fn into_note(&mut self, text: &str, folder: Option<PathBuf>, masked: usize, what: &str) {
        let masked = if masked > 0 { format!(" · {masked} secret{} masked", if masked == 1 { "" } else { "s" }) } else { String::new() };
        if let Some((right, bi)) = self.note_in_tab() {
            let tab = &mut self.tabs[self.active];
            let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
            let Some(Pane::Editor(e)) = pane else { return };
            let Some(b) = e.buffers.get_mut(bi) else { return };
            let end = b.len_chars();
            b.cursor = end;
            b.anchor = None;
            let sep = if end > 0 && b.text.char(end - 1) != '\n' { "\n" } else { "" };
            b.insert(&format!("{sep}{text}"), false);
            let name = b.name();
            let saved = b.path.clone().map(|p| notes::save(&p, &b.text.to_string()));
            if matches!(saved, Some(Ok(()))) {
                b.dirty = false;
            }
            e.reveal();
            match saved {
                Some(Err(err)) => self.notice_problem("Clipped, Not Saved", err.to_string()),
                _ => self.notice(icons::PENCIL, format!("{} Clipped", what.caps()), format!("into {name}{masked}")),
            }
            self.dirty = true;
            return;
        }
        let Some(folder) = folder.or_else(|| self.notes_folder()) else {
            self.notice(icons::PENCIL, "No Folder Here", "open a note beside, or a shell in a folder");
            return;
        };
        let _ = notes::exclude_from_git(&folder);
        let written = notes::inbox(&folder).and_then(|p| notes::append(&p, text).map(|_| p));
        match written {
            Ok(p) => {
                self.open_file(&p, true);
                self.notice(icons::PENCIL, format!("{} Clipped", what.caps()), format!("into the folder's inbox{masked}"));
            }
            Err(e) => self.notice_problem("Could Not Clip", e.to_string()),
        }
    }

    /// Clip a block: the one asked for, else the selected one, else the
    /// last finished one in this tab's shell.
    pub(crate) fn clip_block(&mut self, start: Option<u64>) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let panes = if tab.focus_right { [tab.right.as_ref(), Some(&tab.left)] } else { [Some(&tab.left), tab.right.as_ref()] };
        let Some(t) = panes.into_iter().flatten().find_map(|p| match p { Pane::Term(t) => Some(t), _ => None }) else {
            self.notice(icons::PENCIL, "No Shell Here", "clip works on a shell's blocks");
            return;
        };
        let blocks = t.blocks();
        let pick = start.or(t.block_sel).and_then(|s| blocks.iter().find(|b| b.start == s))
            .or_else(|| blocks.iter().rev().find(|b| !b.running && !b.cmd.trim().is_empty()));
        let Some(b) = pick.cloned() else {
            self.notice(icons::PENCIL, "No Block Yet", "run a command, then clip it");
            return;
        };
        let cmd = crate::cutoff::oneline(&t.block_cmd_text(b.start));
        let cmd = if cmd.trim().is_empty() { b.cmd.clone() } else { cmd };
        let output = t.block_output_text(b.start);
        let cwd = t.term.cwd.clone().or_else(|| t.cwd.clone()).unwrap_or_default();
        let (text, masked) = notes::clip_block(&cmd, &output, &cwd, b.exit, notes::now());
        let folder = Some(PathBuf::from(&cwd)).filter(|p| p.is_dir());
        self.into_note(&text, folder, masked, "block");
    }

    /// The page beside, as a link line in the note.
    pub(crate) fn add_page_to_note(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let page = std::iter::once(&tab.left).chain(tab.right.as_ref()).find_map(|p| match p {
            Pane::Web(w) => { let s = w.tab.shared.borrow(); Some((s.title.clone(), s.url.clone())) }
            _ => None,
        });
        match page {
            Some((title, url)) if url.starts_with("http") => {
                let line = notes::link_line(&title, &url);
                self.into_note(&line, None, 0, "page");
            }
            _ => self.notice(icons::PENCIL, "No Page Here", "open a page beside the note first"),
        }
    }

    /// Palette rows for `note …` / `notes` / `clip`.
    pub(crate) fn note_rows(&self, q: &str) -> Vec<(&'static str, String, crate::app::Action)> {
        use crate::app::Action::Note;
        let mut rows = Vec::new();
        if crate::private::enabled() || !(q.starts_with("note") || q.starts_with("clip")) {
            return rows;
        }
        let folder = self.notes_folder();
        if let Some(f) = &folder {
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            rows.push(("✎", format!("new note here · {name}/.nus/notes · plain, out of git"), Note(NoteAct::New { profile: false })));
        }
        rows.push(("✎", "new profile note · sealed, travels with sync".into(), Note(NoteAct::New { profile: true })));
        rows.push(("✎", "notes · the whole tab, with the index".into(), Note(NoteAct::Tab)));
        rows.push(("✎", "note in the hatch · this folder's inbox, over everything".into(), Note(NoteAct::Hatch)));
        rows.push(("✎", "clip this block into the note".into(), Note(NoteAct::ClipBlock)));
        rows.push(("✎", "add the page beside to the note".into(), Note(NoteAct::AddPage)));
        if !q.starts_with("note") {
            return rows;
        }
        let rest = q.trim_start_matches("notes").trim_start_matches("note").trim().to_lowercase();
        let mut all: Vec<Entry> = folder.map(|f| notes::list(&notes::folder_dir(&f))).unwrap_or_default();
        all.extend(notes::list(&notes::profile_dir()));
        for e in all.into_iter().filter(|e| rest.is_empty() || e.name.to_lowercase().contains(&rest)).take(8) {
            let place = notes::place_of(&e.path).map(Place::label).unwrap_or("");
            rows.push(("✎", format!("{} · {place} · {}", e.name, notes::when(e.modified, notes::now())), Note(NoteAct::Open(e.path))));
        }
        rows
    }

    /// The strip's word for a note: where it lives.
    pub(crate) fn note_place_word(e: &EditorPane) -> Option<&'static str> {
        e.buf().and_then(|b| b.path.as_deref()).and_then(notes::place_of).map(Place::label)
    }

    /// Keep the rails' lists current; cheap to call every draw.
    fn tend_rails(&self, e: &mut EditorPane) {
        let Some((path, revision)) = e.buf().and_then(|b| Some((b.path.clone()?, b.revision))) else { e.notes = None; return };
        let Some(place) = notes::place_of(&path) else { e.notes = None; return };
        let rails = e.notes.get_or_insert_with(Rails::default);
        let moved = rails.path.as_deref() != Some(path.as_path());
        if moved || rails.listed.is_none_or(|t| t.elapsed() > RELIST) {
            rails.folder = match place {
                Place::Folder => notes::folder_of(&path),
                Place::Profile => self.notes_folder(),
            };
            rails.folder_notes = rails.folder.as_ref().map(|f| notes::list(&notes::folder_dir(f))).unwrap_or_default();
            rails.profile_notes = notes::list(&notes::profile_dir());
            rails.listed = Some(Instant::now());
            let newest = rails.folder_notes.iter().chain(&rails.profile_notes).map(|e| e.modified).max().unwrap_or(0);
            if moved || newest != rails.newest {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let all: Vec<Entry> = rails.folder_notes.iter().chain(&rails.profile_notes).cloned().collect();
                rails.backlinks = notes::backlinks(&all, &stem, &path);
                rails.newest = newest;
            }
        }
        if moved || rails.revision != Some(revision) {
            if let Some(b) = e.buffers.get(e.active) {
                if b.ready() && b.text.len_bytes() <= 1024 * 1024 {
                    let rails = e.notes.as_mut().unwrap();
                    rails.refs = notes::refs(&b.text.to_string());
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
        let sections: [(String, &Vec<Entry>); 2] = [
            (rails.folder.as_ref().and_then(|f| f.file_name()).map(|n| format!("This Folder · {}", n.to_string_lossy())).unwrap_or_else(|| "This Folder".into()), &rails.folder_notes),
            ("Profile · Sealed".into(), &rails.profile_notes),
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
                let when = notes::when(entry.modified, now);
                if on {
                    scene.rect(row, ink);
                } else if row.contains(mx, my) {
                    scene.rect(row, crate::surface::mix(paper, ink, 0.05));
                }
                let (st, wst) = if on { (Style { color: paper, ..ui }, Style { color: paper, ..label }) } else { (ui, dim_label) };
                let ww = fonts.measure(wst, &when);
                fonts.draw(scene, wst, row.right() - pad - ww, y + row_h * 0.62, &when);
                fonts.draw(scene, st, ix.x + pad, y + row_h * 0.62, &fit(fonts, st, &entry.name, ix.w - 3.0 * pad - ww));
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
                Ref::Block { cmd, exit, .. } => {
                    let failed = exit.is_some_and(|c| c != 0);
                    scene.rect(lamp, if failed { signal } else { ink });
                    ("Block", if failed { format!("exit {}", exit.unwrap_or(0)) } else { "in note".into() }, cmd.clone())
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
                fonts.draw(scene, ui_dim, rx.x + pad, y + row_h * 0.62, &fit(fonts, ui_dim, "no other note names this one", inner));
            }
            for (i, (p, line)) in rails.backlinks.iter().enumerate() {
                if y + row_h > r.bottom() { break; }
                let row = Rect::new(rx.x + structure, y, rx.w - structure, row_h);
                if row.contains(mx, my) { scene.rect(row, crate::surface::mix(paper, ink, 0.05)); }
                let at = format!("line {line}");
                let aw = fonts.measure(dim_label, &at);
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
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
            (_, Some(Err((p, line)))) => open_here(self, &p, Some(line.saturating_sub(1))),
            (_, Some(Ok(Ref::Page { url, .. }))) => self.open_url(&url, false),
            (_, Some(Ok(Ref::File { path, at, .. }))) => {
                let base = note.as_deref().and_then(notes::folder_of).or_else(|| self.notes_folder()).unwrap_or_default();
                let p = Path::new(path.trim_start_matches("./"));
                let full = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
                if full.is_file() {
                    open_here(self, &full, at.map(|n| n.saturating_sub(1) as usize));
                } else {
                    self.notice(icons::PENCIL, "Not Found", full.display().to_string());
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
        let dir = match (profile, folder.or_else(|| self.notes_folder())) {
            (true, _) => notes::profile_dir(),
            (false, Some(f)) => { let _ = notes::exclude_from_git(&f); notes::folder_dir(&f) }
            (false, None) => return self.notice(icons::PENCIL, "No Folder Here", "make a profile note instead"),
        };
        match notes::create(&dir, "", "") {
            Ok(path) => {
                let tab = &mut self.tabs[self.active];
                let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
                if let Some(Pane::Editor(e)) = pane { let _ = e.open(&path); }
            }
            Err(e) => self.notice_problem("Could Not Make Note", e.to_string()),
        }
    }
}
