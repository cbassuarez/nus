//! One open note, however many places show it. A SESSION holds a note's
//! text, its undo, and its saving; every editor view of that note (a
//! split, another window, the hatch) is a VIEW with its own caret and
//! scroll that follows the session. A capture changes the session once,
//! and every view sees it without its caret moving.
//!
//! Saving is the session's: half a second after typing stops, or every
//! two seconds while it does not, a snapshot goes to the writer thread,
//! which commits it through notes_store.rs. An answer for an older
//! snapshot never marks a newer one saved. "Saved" in the footer means
//! the store said so; nothing here says it sooner.
//!
//! The service lives on the UI thread, above every window's App: all
//! windows are one process and one thread, so this is the one place a
//! note is open. Only the writer thread does disk work.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use ropey::Rope;

use crate::notes_model::{self as model, Document, ModelError, Source};
use crate::notes_store::{BaseToken, CommitAck, Home, NoteError, NoteKey, Recovered, Scope, WriteCap};

/// Typing stops this long: save.
pub const IDLE: Duration = Duration::from_millis(500);
/// Typing goes on this long: save anyway.
pub const CHECKPOINT: Duration = Duration::from_secs(2);
/// After a failed save, try again this often on its own.
const RETRY: Duration = Duration::from_secs(5);
const UNDO_DEPTH: usize = 400;
/// Keystrokes this close together are one undo step.
const MERGE: Duration = Duration::from_millis(400);
/// A capture's Undo stays possible this long, even in a note nobody has
/// open (its session is kept for it).
pub const UNDO_WINDOW: Duration = Duration::from_secs(120);

/// A change between two texts, in chars: `start..old_end` became
/// `start..new_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Splice {
    pub start: usize,
    pub old_end: usize,
    pub new_end: usize,
}

impl Splice {
    /// Where a position lands after the change. A position at the edit's
    /// start stays before whatever was inserted there.
    pub fn map(&self, pos: usize) -> usize {
        if pos <= self.start {
            pos
        } else if pos >= self.old_end {
            pos - self.old_end + self.new_end
        } else {
            self.new_end
        }
    }
}

/// The one change that turns `a` into `b`: common prefix and suffix off.
pub fn splice(a: &Rope, b: &Rope) -> Option<Splice> {
    let (la, lb) = (a.len_chars(), b.len_chars());
    let prefix = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    if prefix == la && prefix == lb {
        return None;
    }
    let room = la.min(lb) - prefix;
    let suffix = a.chars_at(la).reversed().zip(b.chars_at(lb).reversed()).take(room).take_while(|(x, y)| x == y).count();
    Some(Splice { start: prefix, old_end: la - suffix, new_end: lb - suffix })
}

/// What a note's footer says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Saved,
    Saving,
    /// Kept in memory, not on disk: the reason, and Retry.
    Failed(String),
    Conflict,
    ReadOnly(String),
    Locked,
    /// A crash left this unsaved; it saves like any edit once looked at.
    Recovered,
}

impl Status {
    pub fn word(&self) -> String {
        match self {
            Status::Saved => "Saved".into(),
            Status::Saving => "Saving…".into(),
            Status::Failed(_) => "Not saved · Retry".into(),
            Status::Conflict => "Conflict · Review".into(),
            Status::ReadOnly(_) => "Read only".into(),
            Status::Locked => "Personal notes locked".into(),
            Status::Recovered => "Recovered draft · Saving…".into(),
        }
    }
}

/// A view's hold on a session, kept on its editor buffer.
#[derive(Clone, Debug)]
pub struct View {
    pub key: NoteKey,
    /// The session generation this view last matched.
    pub seen: u64,
    /// The text as of `seen`: what a local change is measured against.
    synced: Rope,
    /// The next local change starts a new undo step.
    pub(crate) fresh_step: bool,
    pub title: String,
    pub legacy: bool,
    pub scope: Scope,
    pub home_name: String,
}

/// A capture that landed: where, so Undo can take exactly it back.
#[derive(Clone, Debug)]
struct Mark {
    at: Instant,
    capture_id: String,
    source_id: Option<String>,
    start: usize,
    end: usize,
    text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    pub key: NoteKey,
    pub capture_id: String,
    pub title: String,
    pub path: PathBuf,
    /// The same request came twice; nothing was added the second time.
    pub repeated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoCapture {
    Removed,
    /// You wrote in it since; it stays, and you can open it instead.
    Edited,
    Gone,
}

struct Session {
    key: NoteKey,
    home: Home,
    path: PathBuf,
    /// The header as known (body unused); the body is `text`.
    doc: Document,
    text: Rope,
    gen: u64,
    saved_gen: u64,
    base: BaseToken,
    undo: Vec<Rope>,
    redo: Vec<Rope>,
    last_change: Instant,
    dirty_since: Option<Instant>,
    inflight: Option<u64>,
    failed: Option<(String, Instant)>,
    /// The version on disk when it changed under us.
    conflict: Option<Vec<u8>>,
    read_only: Option<String>,
    views: usize,
    marks: Vec<Mark>,
    /// A crash's leftover draft this session took on: retired once saved.
    recovered_op: Option<String>,
}

impl Session {
    fn dirty(&self) -> bool {
        self.gen > self.saved_gen
    }

    fn status(&self) -> Status {
        if let Some(why) = &self.read_only {
            return Status::ReadOnly(why.clone());
        }
        if self.conflict.is_some() {
            return Status::Conflict;
        }
        if let Some((why, _)) = &self.failed {
            if *why == NoteError::Locked.to_string() {
                return Status::Locked;
            }
            return Status::Failed(why.clone());
        }
        if self.recovered_op.is_some() && self.dirty() {
            return Status::Recovered;
        }
        if self.dirty() || self.inflight.is_some() { Status::Saving } else { Status::Saved }
    }

    fn title(&self) -> String {
        let mut d = self.doc.clone();
        if d.is_legacy() || d.meta.get("title").and_then(|t| t.as_str()).is_none_or(|t| t.trim().is_empty()) {
            d.body = self.text.slice(..self.text.len_chars().min(4096)).to_string();
        }
        d.title()
    }

    /// Change the text as one step (or merged into the last one), keeping
    /// marks where their text went.
    fn set_text(&mut self, text: Rope, merge: bool) {
        let Some(sp) = splice(&self.text, &text) else { return };
        let now = Instant::now();
        if !(merge && now.duration_since(self.last_change) < MERGE && !self.undo.is_empty()) {
            self.undo.push(self.text.clone());
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.replace(text, sp, now);
    }

    fn replace(&mut self, text: Rope, sp: Splice, now: Instant) {
        for m in &mut self.marks {
            m.start = sp.map(m.start);
            m.end = if m.end <= sp.start { m.end } else { sp.map(m.end) };
        }
        self.text = text;
        self.bump(now);
    }

    fn bump(&mut self, now: Instant) {
        self.gen += 1;
        self.last_change = now;
        self.dirty_since.get_or_insert(now);
    }

    /// Nothing holds this session open: no view, nothing unsaved, no
    /// capture recent enough to undo.
    fn idle(&self) -> bool {
        self.views == 0 && !self.dirty() && self.inflight.is_none() && self.conflict.is_none() && self.failed.is_none()
            && self.marks.iter().all(|m| m.at.elapsed() > UNDO_WINDOW)
    }

    fn document(&self) -> Document {
        let mut d = self.doc.clone();
        d.body = self.text.to_string();
        d
    }
}

struct Job {
    key: NoteKey,
    gen: u64,
    home: Home,
    path: PathBuf,
    base: BaseToken,
    doc: Document,
    cap: WriteCap,
}

struct Done {
    key: NoteKey,
    gen: u64,
    result: Result<(CommitAck, Document), NoteError>,
}

struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
}

impl Worker {
    fn start() -> Worker {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done, rx) = std::sync::mpsc::channel::<Done>();
        std::thread::Builder::new()
            .name("nus-notes-writer".into())
            .spawn(move || {
                for job in jobs {
                    let result = job.home.commit(&job.cap, &job.path, &job.base, job.doc);
                    if done.send(Done { key: job.key, gen: job.gen, result }).is_err() {
                        break;
                    }
                }
            })
            .expect("the notes writer thread starts");
        Worker { tx, rx }
    }
}

/// A committed save, for the search index to catch up on.
#[derive(Clone, Debug)]
pub struct Committed {
    pub home: Home,
    pub path: PathBuf,
}

struct Service {
    sessions: HashMap<NoteKey, Session>,
    worker: Option<Worker>,
    profile: PathBuf,
    committed: Vec<Committed>,
    /// Homes whose leftover intents were looked at this run.
    recovered_homes: std::collections::HashSet<String>,
    drafts: Vec<Recovered>,
}

impl Service {
    fn new() -> Service {
        Service {
            sessions: HashMap::new(),
            worker: None,
            profile: crate::notes::profile_dir().parent().map(Path::to_path_buf).unwrap_or_default(),
            committed: Vec::new(),
            recovered_homes: Default::default(),
            drafts: Vec::new(),
        }
    }

    fn worker(&mut self) -> &Worker {
        self.worker.get_or_insert_with(Worker::start)
    }
}

thread_local! {
    static SERVICE: RefCell<Service> = RefCell::new(Service::new());
}

fn with<R>(f: impl FnOnce(&mut Service) -> R) -> R {
    SERVICE.with(|s| f(&mut s.borrow_mut()))
}

/// Point the service at another profile (the tests' own).
#[cfg(test)]
pub fn set_profile(profile: &Path) {
    with(|s| s.profile = profile.to_path_buf());
}

pub fn profile() -> PathBuf {
    with(|s| s.profile.clone())
}

pub fn personal_root() -> PathBuf {
    profile().join("notes")
}

// --- homes -------------------------------------------------------------

/// Projects whose notes are part of "All notes": kept sealed in the
/// profile, because where your projects are is yours.
fn registry_path() -> PathBuf {
    profile().join("notes-projects.json")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registered {
    pub home_id: String,
    pub project: PathBuf,
}

pub fn registered() -> Vec<Registered> {
    let Ok(bytes) = nus_vault::read_at(&profile(), &registry_path()) else { return Vec::new() };
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    v.get("projects").and_then(|p| p.as_array()).into_iter().flatten().filter_map(|p| {
        Some(Registered { home_id: p.get("home_id")?.as_str()?.to_string(), project: PathBuf::from(p.get("root")?.as_str()?) })
    }).collect()
}

fn save_registry(list: &[Registered]) {
    let v = serde_json::json!({"schema": 1, "projects": list.iter().map(|r| serde_json::json!({"home_id": r.home_id, "root": r.project.to_string_lossy()})).collect::<Vec<_>>()});
    if let Err(e) = nus_vault::write_at(&profile(), &registry_path(), v.to_string().as_bytes()) {
        tracing::warn!("notes: the project list could not be saved: {e}");
    }
}

/// A project's home, made when needed and registered: a project copied
/// from one still in place takes a new identity; a moved one keeps its
/// own and the registry follows it.
pub fn project_home(project: &Path, cap: &WriteCap) -> Result<Home, NoteError> {
    let mut home = Home::ensure_folder(project, cap)?;
    let root = home.project.clone().unwrap_or_default();
    let mut list = registered();
    if let Some(other) = list.iter().find(|r| r.home_id == home.id && r.project != root) {
        let still_there = Home::folder(&other.project).is_some_and(|h| h.id == home.id);
        if still_there {
            home.fork_identity(cap)?;
        } else {
            list.retain(|r| r.home_id != home.id);
        }
    }
    if !list.iter().any(|r| r.home_id == home.id && r.project == root) {
        list.retain(|r| r.project != root);
        list.push(Registered { home_id: home.id.clone(), project: root });
        save_registry(&list);
    }
    Ok(home)
}

pub fn personal_home(cap: Option<&WriteCap>) -> Result<Home, NoteError> {
    Home::personal(&profile(), cap)
}

/// Every home "All notes" covers that can be opened now: Personal (when
/// unlocked) and each registered project that is still there.
pub fn homes() -> (Vec<Home>, Vec<(String, NoteError)>) {
    let mut ok = Vec::new();
    let mut missing = Vec::new();
    match personal_home(None) {
        Ok(h) => ok.push(h),
        Err(NoteError::NotFound) => {}
        Err(e) => missing.push(("Personal".to_string(), e)),
    }
    for r in registered() {
        match Home::folder(&r.project) {
            Some(h) if h.id == r.home_id => ok.push(h),
            _ => missing.push((r.project.display().to_string(), NoteError::NotFound)),
        }
    }
    (ok, missing)
}

/// The home a note's file is in.
pub fn home_of(path: &Path, cap: &WriteCap) -> Result<Home, NoteError> {
    let parent = path.parent().ok_or(NoteError::Escape)?;
    let personal = personal_root();
    let personal = personal.canonicalize().unwrap_or(personal);
    if parent == personal || parent.canonicalize().is_ok_and(|p| p == personal) {
        return personal_home(Some(cap));
    }
    let notes = parent.file_name().ok_or(NoteError::Escape)?;
    let dot = parent.parent().and_then(Path::file_name).ok_or(NoteError::Escape)?;
    if notes != "notes" || dot != ".nus" {
        return Err(NoteError::Escape);
    }
    let project = parent.parent().and_then(Path::parent).ok_or(NoteError::Escape)?;
    project_home(project, cap)
}

// --- sessions ----------------------------------------------------------

/// Open a note for a new view: the session if it is open already (from
/// anywhere), else from disk. Returns the view and the text to show.
pub fn attach(path: &Path) -> Result<(View, Rope), NoteError> {
    let cap = WriteCap::grant()?;
    let home = home_of(path, &cap)?;
    let key = open_session(&home, path, &cap)?;
    with(|s| {
        let session = s.sessions.get_mut(&key).ok_or(NoteError::NotFound)?;
        session.views += 1;
        let view = View {
            key: key.clone(),
            seen: session.gen,
            synced: session.text.clone(),
            fresh_step: true,
            title: session.title(),
            legacy: session.doc.is_legacy(),
            scope: home.scope,
            home_name: home.name(),
        };
        Ok((view, session.text.clone()))
    })
}

/// Make sure a session for `path` is open (no view needed: a capture into
/// a note nobody is looking at still goes through here).
fn open_session(home: &Home, path: &Path, cap: &WriteCap) -> Result<NoteKey, NoteError> {
    let path = canonical(path);
    if let Some(key) = with(|s| s.sessions.values().find(|x| x.path == path).map(|x| x.key.clone())) {
        return Ok(key);
    }
    // Look once per home and run for what a crash left behind.
    let first = with(|s| s.recovered_homes.insert(home.id.clone()));
    if first {
        match home.recover(cap) {
            Ok(found) => with(|s| s.drafts.extend(found.into_iter().filter(|r| !matches!(r, Recovered::Finished { .. })))),
            Err(e) => tracing::warn!("notes: recovery check failed: {e}"),
        }
    }
    let (doc, base, key, read_only) = match home.open(&path) {
        Ok(snap) => (snap.doc, snap.base, snap.key, None),
        Err(NoteError::Model(ModelError::Newer(v))) => {
            // Shown as it is, never saved over.
            let bytes = home.read_file(&path)?;
            let key = NoteKey { home_id: home.id.clone(), note_id: crate::notes_store::legacy_id(&home.id, &path) };
            let base = BaseToken { key: key.clone(), sha256: Some(model::sha256(&bytes)), revision: 0 };
            let doc = Document::plain(&String::from_utf8_lossy(&bytes));
            (doc, base, key, Some(ModelError::Newer(v).to_string()))
        }
        Err(e) => return Err(e),
    };
    if let Some(key) = with(|s| s.sessions.contains_key(&key).then(|| key.clone())) {
        // The same note under another path (a duplicate id): keep the one open.
        return Ok(key);
    }
    let text = Rope::from_str(&doc.body);
    let now = Instant::now();
    let mut session = Session {
        key: key.clone(),
        home: home.clone(),
        path,
        doc,
        text,
        gen: 0,
        saved_gen: 0,
        base,
        undo: Vec::new(),
        redo: Vec::new(),
        last_change: now,
        dirty_since: None,
        inflight: None,
        failed: None,
        conflict: None,
        read_only,
        views: 0,
        marks: Vec::new(),
        recovered_op: None,
    };
    // A draft a crash left for this note becomes its unsaved text.
    let draft = with(|s| {
        let at = s.drafts.iter().position(|r| matches!(r, Recovered::Draft { key: k, .. } | Recovered::Diverged { key: k, .. } if *k == key))?;
        Some(s.drafts.remove(at))
    });
    match draft {
        Some(Recovered::Draft { op, proposed, .. }) => {
            if let Ok(d) = Document::parse(proposed.as_bytes()) {
                session.doc.meta = d.meta.clone();
                session.set_text(Rope::from_str(&d.body), false);
                session.recovered_op = Some(op);
            }
        }
        Some(Recovered::Diverged { op, proposed, current, .. }) => {
            if let Ok(d) = Document::parse(proposed.as_bytes()) {
                session.set_text(Rope::from_str(&d.body), false);
                session.conflict = Some(current.unwrap_or_default().into_bytes());
                session.recovered_op = Some(op);
            }
        }
        _ => {}
    }
    with(|s| s.sessions.insert(key.clone(), session));
    Ok(key)
}

fn canonical(p: &Path) -> PathBuf {
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\"))
}

/// A view goes away. The session stays while it has unsaved text: it
/// keeps saving on its own, and closing never discards it.
pub fn detach(view: &View) {
    with(|s| {
        if let Some(x) = s.sessions.get_mut(&view.key) {
            x.views = x.views.saturating_sub(1);
            if x.idle() {
                s.sessions.remove(&view.key);
            }
        }
    });
}

/// A view's text changed: the session takes it. When the session moved on
/// since this view last looked (a capture landed), the view's change is
/// applied on top of it and the merged text comes back for the view.
pub fn push(view: &mut View, text: &Rope, merge: bool) -> Option<(Rope, Splice)> {
    let merge = merge && !std::mem::take(&mut view.fresh_step);
    with(|s| {
        let x = s.sessions.get_mut(&view.key)?;
        if x.read_only.is_some() {
            // Put the view back: nothing is saved over a note we cannot write.
            let sp = splice(text, &x.text)?;
            view.synced = x.text.clone();
            view.seen = x.gen;
            return Some((x.text.clone(), sp));
        }
        let back = if view.seen == x.gen {
            x.set_text(text.clone(), merge);
            None
        } else {
            // Rebase: the view's own change, moved past the session's.
            let local = splice(&view.synced, text)?;
            let remote = splice(&view.synced, &x.text);
            let (a, b) = match remote {
                Some(r) => (r.map(local.start), if local.old_end <= r.start { local.old_end } else { r.map(local.old_end) }),
                None => (local.start, local.old_end),
            };
            let mut merged = x.text.clone();
            let b = b.max(a).min(merged.len_chars());
            merged.remove(a..b);
            merged.insert(a, &text.slice(local.start..local.new_end).to_string());
            x.set_text(merged.clone(), false);
            let sp = splice(text, &merged)?;
            Some((merged, sp))
        };
        view.seen = x.gen;
        view.synced = x.text.clone();
        view.title = x.title();
        back
    })
}

/// What changed in the session since this view looked: the new text and
/// the change, to move the caret and scroll by.
pub fn pull(view: &mut View) -> Option<(Rope, Splice)> {
    with(|s| {
        let x = s.sessions.get(&view.key)?;
        view.title = x.title();
        if x.gen == view.seen {
            return None;
        }
        view.seen = x.gen;
        let sp = splice(&view.synced, &x.text);
        view.synced = x.text.clone();
        view.fresh_step = true;
        sp.map(|sp| (x.text.clone(), sp))
    })
}

/// Undo in any view undoes the note's last step, for every view.
pub fn undo(view: &View) -> bool {
    with(|s| {
        let Some(x) = s.sessions.get_mut(&view.key) else { return false };
        let Some(prev) = x.undo.pop() else { return false };
        if x.read_only.is_some() {
            return false;
        }
        let now = Instant::now();
        x.redo.push(x.text.clone());
        let sp = splice(&x.text, &prev).unwrap_or(Splice { start: 0, old_end: 0, new_end: 0 });
        x.replace(prev, sp, now);
        true
    })
}

pub fn redo(view: &View) -> bool {
    with(|s| {
        let Some(x) = s.sessions.get_mut(&view.key) else { return false };
        let Some(next) = x.redo.pop() else { return false };
        let now = Instant::now();
        x.undo.push(x.text.clone());
        let sp = splice(&x.text, &next).unwrap_or(Splice { start: 0, old_end: 0, new_end: 0 });
        x.replace(next, sp, now);
        true
    })
}

pub fn status(key: &NoteKey) -> Option<Status> {
    with(|s| s.sessions.get(key).map(Session::status))
}

pub fn dirty(key: &NoteKey) -> bool {
    with(|s| s.sessions.get(key).is_some_and(Session::dirty))
}

/// The note's file and its home's name, for the footer and Details.
pub fn where_is(key: &NoteKey) -> Option<(PathBuf, String, Scope)> {
    with(|s| s.sessions.get(key).map(|x| (x.path.clone(), x.home.name(), x.home.scope)))
}

/// The session's header and body as they stand (unsaved changes too).
pub fn document(key: &NoteKey) -> Option<Document> {
    with(|s| s.sessions.get(key).map(Session::document))
}

/// Sessions with text not yet saved, for Close and Quit to ask about.
pub fn unsaved() -> Vec<(NoteKey, String, Status)> {
    with(|s| s.sessions.values().filter(|x| x.dirty() || x.conflict.is_some()).map(|x| (x.key.clone(), x.title(), x.status())).collect())
}

/// Does this session have views besides `view`?
pub fn others(view: &View) -> bool {
    with(|s| s.sessions.get(&view.key).is_some_and(|x| x.views > 1))
}

/// Set a note's title (the header's, not the file's name).
pub fn set_title(key: &NoteKey, title: &str) -> Result<(), NoteError> {
    with(|s| {
        let x = s.sessions.get_mut(key).ok_or(NoteError::NotFound)?;
        if x.doc.is_legacy() {
            return Err(NoteError::Legacy);
        }
        x.doc.set_title(title);
        x.bump(Instant::now());
        Ok(())
    })
}

pub fn set_tags(key: &NoteKey, tags: &[String]) -> Result<(), NoteError> {
    with(|s| {
        let x = s.sessions.get_mut(key).ok_or(NoteError::NotFound)?;
        if x.doc.is_legacy() {
            return Err(NoteError::Legacy);
        }
        x.doc.set_tags(tags);
        x.bump(Instant::now());
        Ok(())
    })
}

/// Keep as Note: a quick capture leaves the inbox; it moves nowhere.
pub fn set_filed(key: &NoteKey, filed: bool) -> Result<(), NoteError> {
    with(|s| {
        let x = s.sessions.get_mut(key).ok_or(NoteError::NotFound)?;
        x.doc.set_filed(filed);
        x.bump(Instant::now());
        Ok(())
    })
}

/// Send what is due to the writer and take its answers. Cheap; call every
/// loop. Returns true when a footer should change.
pub fn tend() -> bool {
    let now = Instant::now();
    let cap = WriteCap::grant().ok();
    with(|s| {
        let mut changed = drain(s, None, Duration::ZERO);
        // Sessions kept only for a capture's Undo go once that has passed.
        s.sessions.retain(|_, x| !x.idle());
        let Some(cap) = cap else { return changed };
        let due: Vec<NoteKey> = s.sessions.values().filter(|x| due(x, now)).map(|x| x.key.clone()).collect();
        for key in due {
            let Some(x) = s.sessions.get_mut(&key) else { continue };
            let job = Job { key: key.clone(), gen: x.gen, home: x.home.clone(), path: x.path.clone(), base: x.base.clone(), doc: x.document(), cap: cap.clone() };
            x.inflight = Some(x.gen);
            changed = true;
            if s.worker().tx.send(job).is_err() {
                s.worker = None;
                if let Some(x) = s.sessions.get_mut(&key) {
                    x.inflight = None;
                    x.failed = Some(("the notes writer stopped".into(), now));
                }
            }
        }
        changed
    })
}

fn due(x: &Session, now: Instant) -> bool {
    x.dirty()
        && x.inflight.is_none()
        && x.conflict.is_none()
        && x.read_only.is_none()
        && x.failed.as_ref().is_none_or(|(_, at)| now.duration_since(*at) >= RETRY)
        && (now.duration_since(x.last_change) >= IDLE || x.dirty_since.is_some_and(|d| now.duration_since(d) >= CHECKPOINT))
}

/// Take the writer's answers; wait up to `wait` for one about `key`.
fn drain(s: &mut Service, key: Option<&NoteKey>, wait: Duration) -> bool {
    let mut changed = false;
    let deadline = Instant::now() + wait;
    loop {
        let waiting = key.is_some_and(|k| s.sessions.get(k).is_some_and(|x| x.inflight.is_some()));
        let Some(w) = &s.worker else { return changed };
        let done = if waiting {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return changed;
            }
            match w.rx.recv_timeout(left) {
                Ok(d) => d,
                Err(_) => return changed,
            }
        } else {
            match w.rx.try_recv() {
                Ok(d) => d,
                Err(_) => return changed,
            }
        };
        changed = true;
        settle(s, done.key, done.gen, done.result);
    }
}

fn settle(s: &mut Service, key: NoteKey, gen: u64, result: Result<(CommitAck, Document), NoteError>) {
    let Some(x) = s.sessions.get_mut(&key) else { return };
    x.inflight = None;
    match result {
        Ok((ack, saved)) => {
            x.base = BaseToken { key: key.clone(), sha256: Some(ack.sha256.clone()), revision: ack.revision };
            x.doc.adopt_stamp(&saved);
            x.saved_gen = x.saved_gen.max(gen);
            x.failed = None;
            if !x.dirty() {
                x.dirty_since = None;
            }
            if let Some(op) = x.recovered_op.take() {
                if let Ok(cap) = WriteCap::grant() {
                    let _ = x.home.retire(&cap, &op);
                }
            }
            s.committed.push(Committed { home: x.home.clone(), path: ack.path });
            if x.idle() {
                s.sessions.remove(&key);
            }
        }
        Err(NoteError::Conflict { theirs, .. }) => x.conflict = Some(theirs),
        Err(e) => x.failed = Some((e.to_string(), Instant::now())),
    }
}

/// Save now and say how it went (Cmd+S, closing the last view, quitting).
/// Waits for a save already under way rather than racing it.
pub fn flush(key: &NoteKey) -> Status {
    let cap = match WriteCap::grant() {
        Ok(c) => c,
        Err(e) => return Status::Failed(e.to_string()),
    };
    with(|s| {
        drain(s, Some(key), Duration::from_secs(10));
        let Some(x) = s.sessions.get_mut(key) else { return Status::Saved };
        if x.inflight.is_some() {
            return Status::Saving;
        }
        if x.dirty() && x.conflict.is_none() && x.read_only.is_none() {
            let gen = x.gen;
            let result = x.home.commit(&cap, &x.path, &x.base, x.document());
            settle(s, key.clone(), gen, result);
        }
        s.sessions.get(key).map_or(Status::Saved, Session::status)
    })
}

/// Save everything unsaved; what could not be saved is returned.
pub fn flush_all() -> Vec<(NoteKey, String, Status)> {
    let keys: Vec<NoteKey> = with(|s| s.sessions.keys().cloned().collect());
    for k in &keys {
        flush(k);
    }
    unsaved()
}

/// Saves that landed since last asked (the index reads these).
pub fn take_committed() -> Vec<Committed> {
    with(|s| std::mem::take(&mut s.committed))
}

// --- conflicts ---------------------------------------------------------

/// Keep your version: theirs is kept as a checkpoint first, then yours is
/// saved over it.
pub fn keep_mine(key: &NoteKey) -> Result<(), NoteError> {
    let cap = WriteCap::grant()?;
    with(|s| {
        let x = s.sessions.get_mut(key).ok_or(NoteError::NotFound)?;
        let theirs = x.conflict.clone().ok_or(NoteError::NotFound)?;
        x.home.keep_version(&cap, &key.note_id, &theirs)?;
        let rev = Document::parse(&theirs).map(|d| d.revision()).unwrap_or(0);
        x.base = BaseToken { key: key.clone(), sha256: (!theirs.is_empty()).then(|| model::sha256(&theirs)), revision: rev };
        if let Ok(d) = Document::parse(&theirs) {
            x.doc.adopt_stamp(&d);
        }
        x.conflict = None;
        x.bump(Instant::now());
        Ok(())
    })
}

/// Take their version: yours is kept as a checkpoint, then the note shows
/// theirs (one undo step brings yours back).
pub fn take_theirs(key: &NoteKey) -> Result<(), NoteError> {
    let cap = WriteCap::grant()?;
    with(|s| {
        let x = s.sessions.get_mut(key).ok_or(NoteError::NotFound)?;
        let theirs = x.conflict.clone().ok_or(NoteError::NotFound)?;
        let mine = x.document().to_bytes();
        x.home.keep_version(&cap, &key.note_id, &mine)?;
        let d = Document::parse(&theirs)?;
        x.base = BaseToken { key: key.clone(), sha256: Some(model::sha256(&theirs)), revision: d.revision() };
        x.doc = d.clone();
        x.set_text(Rope::from_str(&d.body), false);
        x.saved_gen = x.gen;
        x.dirty_since = None;
        x.conflict = None;
        x.failed = None;
        Ok(())
    })
}

/// Their version's text, to look at before choosing.
pub fn theirs(key: &NoteKey) -> Option<String> {
    with(|s| s.sessions.get(key)?.conflict.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()))
}

/// Let go of unsaved changes, on purpose: the note goes back to what is
/// on disk. The unsaved text is kept as a checkpoint when the disk takes
/// it, and stays one undo step away while the note is open.
pub fn discard(key: &NoteKey) -> Result<(), NoteError> {
    let cap = WriteCap::grant()?;
    with(|s| {
        let x = s.sessions.get_mut(key).ok_or(NoteError::NotFound)?;
        let mine = x.document().to_bytes();
        if let Err(e) = x.home.keep_version(&cap, &key.note_id, &mine) {
            tracing::warn!("notes: discarded text could not be kept as a checkpoint: {e}");
        }
        let snap = x.home.open(&x.path)?;
        x.base = snap.base;
        x.doc = snap.doc.clone();
        x.set_text(Rope::from_str(&snap.doc.body), false);
        x.saved_gen = x.gen;
        x.dirty_since = None;
        x.conflict = None;
        x.failed = None;
        x.marks.clear();
        if x.views == 0 {
            s.sessions.remove(key);
        }
        Ok(())
    })
}

/// A new note in `home` holding what this one holds now (unsaved text
/// included): Save Copy, when the note itself cannot be saved.
pub fn copy_to(key: &NoteKey, home: &Home) -> Result<PathBuf, NoteError> {
    let cap = WriteCap::grant()?;
    let (mut doc, title) = with(|s| s.sessions.get(key).map(|x| (x.document(), x.title()))).ok_or(NoteError::NotFound)?;
    let mut fresh = Document::new(&model::new_id()?, &format!("{title} (copy)"), true, crate::notes::now());
    if !doc.is_legacy() {
        for src in doc.sources() {
            fresh.push_source(src);
        }
        fresh.set_tags(&doc.tags());
    }
    fresh.body = std::mem::take(&mut doc.body);
    let snap = home.create(&cap, fresh)?;
    with(|s| s.committed.push(Committed { home: home.clone(), path: snap.path.clone() }));
    Ok(snap.path)
}

/// How many views a session has open.
pub fn views(key: &NoteKey) -> usize {
    with(|s| s.sessions.get(key).map_or(0, |x| x.views))
}

// --- captures ----------------------------------------------------------

/// A capture to add: Markdown to append, the source it quotes (when there
/// is one), and a request id so the same capture twice lands once.
#[derive(Clone, Debug)]
pub struct Capture {
    pub request_id: String,
    pub markdown: String,
    pub source: Option<Source>,
}

/// Add a capture to the note at `path`, open or not. The session is the
/// only writer: an open, unsaved note gets it in its buffer; nothing is
/// appended to a file behind anyone's back.
pub fn capture(path: &Path, c: Capture) -> Result<Receipt, NoteError> {
    let cap = WriteCap::grant()?;
    let home = home_of(path, &cap)?;
    let key = open_session(&home, path, &cap)?;
    with(|s| {
        let x = s.sessions.get_mut(&key).ok_or(NoteError::NotFound)?;
        if let Some(why) = &x.read_only {
            return Err(NoteError::Io(std::io::Error::other(why.clone())));
        }
        let receipt = |x: &Session, repeated| Receipt { key: key.clone(), capture_id: c.request_id.clone(), title: x.title(), path: x.path.clone(), repeated };
        if x.marks.iter().any(|m| m.capture_id == c.request_id) {
            return Ok(receipt(x, true));
        }
        let end = x.text.len_chars();
        let sep = match (end, end.checked_sub(1).map(|i| x.text.char(i)), end.checked_sub(2).map(|i| x.text.char(i))) {
            (0, _, _) => "",
            (_, Some('\n'), Some('\n')) => "",
            (_, Some('\n'), _) => "\n",
            _ => "\n\n",
        };
        let mut md = c.markdown.trim_start_matches('\n').to_string();
        // A legacy note has no source records: no markers pointing at none.
        if x.doc.is_legacy() {
            md = md.lines().filter(|l| !(l.starts_with("<!-- nus:source ") && l.ends_with(" -->"))).collect::<Vec<_>>().join("\n");
        }
        if !md.ends_with('\n') {
            md.push('\n');
        }
        let inserted = format!("{sep}{md}");
        let mut text = x.text.clone();
        text.insert(end, &inserted);
        x.set_text(text, false);
        let source_id = c.source.as_ref().map(|s| s.id().to_string());
        if let Some(src) = c.source {
            if !x.doc.is_legacy() {
                x.doc.push_source(src);
            }
        }
        x.marks.push(Mark { at: Instant::now(), capture_id: c.request_id.clone(), source_id, start: end, end: end + inserted.chars().count(), text: inserted });
        Ok(receipt(x, false))
    })
}

/// Undo one capture: only while its text is exactly as it landed.
pub fn undo_capture(key: &NoteKey, request_id: &str) -> UndoCapture {
    with(|s| {
        let Some(x) = s.sessions.get_mut(key) else { return UndoCapture::Gone };
        let Some(at) = x.marks.iter().position(|m| m.capture_id == request_id) else { return UndoCapture::Gone };
        let m = x.marks[at].clone();
        let intact = m.end <= x.text.len_chars() && x.text.slice(m.start..m.end).to_string() == m.text;
        if !intact {
            return UndoCapture::Edited;
        }
        let mut text = x.text.clone();
        text.remove(m.start..m.end);
        x.set_text(text, false);
        if let Some(id) = &m.source_id {
            x.doc.remove_source(id);
        }
        x.marks.remove(at);
        UndoCapture::Removed
    })
}

/// A new note, saved at once (it exists before anything is typed in it).
pub fn create(home: &Home, title: &str, filed: bool) -> Result<PathBuf, NoteError> {
    let cap = WriteCap::grant()?;
    let doc = Document::new(&model::new_id()?, title, filed, crate::notes::now());
    let snap = home.create(&cap, doc)?;
    with(|s| s.committed.push(Committed { home: home.clone(), path: snap.path.clone() }));
    Ok(snap.path)
}

/// Unsaved drafts a crash left in homes looked at so far.
pub fn drafts() -> Vec<(NoteKey, PathBuf)> {
    with(|s| s.drafts.iter().filter_map(|r| match r {
        Recovered::Draft { key, head, .. } | Recovered::Diverged { key, head, .. } => Some((key.clone(), head.clone())),
        _ => None,
    }).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) struct Fixture {
        pub dir: tempfile::TempDir,
        pub project: PathBuf,
    }

    pub(crate) fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).unwrap();
        nus_vault::install_test_key(&profile).unwrap();
        set_profile(&profile);
        with(|s| {
            s.sessions.clear();
            s.drafts.clear();
            s.recovered_homes.clear();
        });
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        Fixture { dir, project }
    }

    fn new_note(f: &Fixture, body: &str) -> PathBuf {
        let home = project_home(&f.project, &WriteCap::test()).unwrap();
        let path = create(&home, "t", true).unwrap();
        let (mut v, _) = attach(&path).unwrap();
        let t = Rope::from_str(body);
        push(&mut v, &t, false);
        assert_eq!(flush(&v.key), Status::Saved);
        detach(&v);
        path
    }

    fn type_into(v: &mut View, text: &mut Rope, at: usize, s: &str) {
        text.insert(at, s);
        if let Some((merged, _)) = push(v, text, true) {
            *text = merged;
        }
    }

    #[test]
    fn splices_find_the_one_change() {
        let a = Rope::from_str("hello world");
        assert_eq!(splice(&a, &Rope::from_str("hello brave world")), Some(Splice { start: 6, old_end: 6, new_end: 12 }));
        assert_eq!(splice(&a, &a), None);
        assert_eq!(splice(&Rope::from_str("aaa"), &Rope::from_str("aaaa")), Some(Splice { start: 3, old_end: 3, new_end: 4 }));
        let sp = splice(&a, &Rope::from_str("hello")).unwrap();
        assert_eq!(sp.map(3), 3);
        assert_eq!(sp.map(9), 5);
    }

    #[test]
    fn two_views_share_text_and_keep_their_own_carets() {
        let f = fixture();
        let path = new_note(&f, "one\n");
        let (mut a, ta) = attach(&path).unwrap();
        let (mut b, tb) = attach(&path).unwrap();
        assert_eq!(a.key, b.key);
        let mut ta = ta;
        type_into(&mut a, &mut ta, 0, "zero ");
        let (tb2, sp) = pull(&mut b).unwrap();
        assert_eq!(tb2.to_string(), "zero one\n");
        // B's caret at the end of "one" moves with the text; one at the
        // very start stays at the start.
        assert_eq!(sp.map(3), 8);
        assert_eq!(sp.map(0), 0);
        assert!(pull(&mut b).is_none());
        drop(tb);
        detach(&a);
        detach(&b);
    }

    #[test]
    fn a_capture_lands_once_and_moves_no_caret() {
        let f = fixture();
        let path = new_note(&f, "body\n");
        let (mut a, _) = attach(&path).unwrap();
        let c = Capture { request_id: "r1".into(), markdown: "> quoted\n".into(), source: None };
        let r = capture(&path, c.clone()).unwrap();
        assert!(!r.repeated);
        assert!(capture(&path, c).unwrap().repeated, "the same request twice lands once");
        let (text, sp) = pull(&mut a).unwrap();
        assert_eq!(text.to_string(), "body\n\n> quoted\n");
        // A caret at the old end stays where it was.
        assert_eq!(sp.map(5), 5);
        detach(&a);
    }

    #[test]
    fn a_view_typing_over_a_capture_it_has_not_seen_keeps_both() {
        let f = fixture();
        let path = new_note(&f, "abc\n");
        let (mut a, t) = attach(&path).unwrap();
        let mut t = t;
        capture(&path, Capture { request_id: "r".into(), markdown: "CAP".into(), source: None }).unwrap();
        // The view has not pulled yet and types at the start.
        type_into(&mut a, &mut t, 0, "X");
        assert_eq!(t.to_string(), "Xabc\n\nCAP\n");
        assert_eq!(document(&a.key).unwrap().body, "Xabc\n\nCAP\n");
        detach(&a);
    }

    #[test]
    fn undo_is_the_notes_and_undo_capture_respects_edits() {
        let f = fixture();
        let path = new_note(&f, "x\n");
        let (mut a, _) = attach(&path).unwrap();
        let r = capture(&path, Capture { request_id: "r".into(), markdown: "cap".into(), source: None }).unwrap();
        let mut t = pull(&mut a).unwrap().0;
        type_into(&mut a, &mut t, 0, "typed ");
        // The capture's range moved with the typing; still intact.
        assert_eq!(undo_capture(&r.key, "r"), UndoCapture::Removed);
        assert_eq!(document(&a.key).unwrap().body, "typed x\n");
        let r2 = capture(&path, Capture { request_id: "r2".into(), markdown: "two".into(), source: None }).unwrap();
        t = pull(&mut a).unwrap().0;
        let end = t.len_chars();
        type_into(&mut a, &mut t, end - 1, "!");
        assert_eq!(undo_capture(&r2.key, "r2"), UndoCapture::Edited);
        // Plain undo steps back through the note, for every view.
        assert!(undo(&a));
        assert_eq!(pull(&mut a).unwrap().0.to_string(), "typed x\n\ntwo\n");
        detach(&a);
    }

    #[test]
    fn saving_waits_for_the_writer_and_a_stale_answer_cannot_clean_newer_text() {
        let f = fixture();
        let path = new_note(&f, "v1\n");
        let (mut a, t) = attach(&path).unwrap();
        let mut t = t;
        type_into(&mut a, &mut t, 0, "A");
        with(|s| s.sessions.get_mut(&a.key).unwrap().last_change -= IDLE);
        assert!(tend());
        assert_eq!(status(&a.key), Some(Status::Saving));
        // More typing while that save is out.
        type_into(&mut a, &mut t, 0, "B");
        std::thread::sleep(Duration::from_millis(200));
        tend();
        assert!(dirty(&a.key), "the older save must not mark the newer text saved");
        assert_eq!(flush(&a.key), Status::Saved);
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("BAv1\n"));
        detach(&a);
    }

    #[test]
    fn an_outside_edit_is_a_conflict_with_both_kept() {
        let f = fixture();
        let path = new_note(&f, "mine\n");
        let (mut a, t) = attach(&path).unwrap();
        let mut t = t;
        let outside = std::fs::read_to_string(&path).unwrap().replace("mine", "theirs");
        std::fs::write(&path, &outside).unwrap();
        type_into(&mut a, &mut t, 0, "my ");
        assert_eq!(flush(&a.key), Status::Conflict);
        assert!(theirs(&a.key).unwrap().contains("theirs"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), outside, "theirs is untouched");
        keep_mine(&a.key).unwrap();
        assert_eq!(flush(&a.key), Status::Saved);
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("my mine\n"));
        // Theirs was kept as a checkpoint.
        let home = Home::folder(&f.project).unwrap();
        let revs = home.revisions(&a.key.note_id);
        assert!(revs.iter().any(|(_, p)| std::fs::read_to_string(p).unwrap().contains("theirs")));
        detach(&a);
    }

    #[test]
    fn closing_the_last_view_keeps_unsaved_text_saving() {
        let f = fixture();
        let path = new_note(&f, "x\n");
        let (mut a, t) = attach(&path).unwrap();
        let mut t = t;
        type_into(&mut a, &mut t, 0, "late ");
        detach(&a);
        assert_eq!(unsaved().len(), 1, "a dirty session outlives its views");
        assert!(flush_all().is_empty());
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("late x\n"));
        assert!(unsaved().is_empty());
    }

    #[test]
    fn a_crash_draft_comes_back_as_unsaved_text() {
        let f = fixture();
        let path = new_note(&f, "saved\n");
        let home = Home::folder(&f.project).unwrap();
        let snap = home.open(&path).unwrap();
        let mut d = snap.doc.clone();
        d.body = "unsaved\n".into();
        crate::notes_store::faults::at(Some(crate::notes_store::faults::Step::AfterIntent));
        let _ = home.commit(&WriteCap::test(), &path, &snap.base, d);
        with(|s| s.recovered_homes.clear());
        let (a, t) = attach(&path).unwrap();
        assert_eq!(t.to_string(), "unsaved\n");
        assert_eq!(status(&a.key), Some(Status::Recovered));
        assert_eq!(flush(&a.key), Status::Saved);
        assert!(home.recover(&WriteCap::test()).unwrap().is_empty(), "the draft's record retires once saved");
        detach(&a);
    }

    #[test]
    fn a_copied_project_gets_its_own_home() {
        let f = fixture();
        let a = project_home(&f.project, &WriteCap::test()).unwrap();
        let copy = f.dir.path().join("copy");
        std::fs::create_dir_all(copy.join(".nus/notes/.state")).unwrap();
        std::fs::copy(a.root.join(".state/home.json"), copy.join(".nus/notes/.state/home.json")).unwrap();
        let b = project_home(&copy, &WriteCap::test()).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(registered().len(), 2);
        // Moving a project keeps its identity.
        let moved = f.dir.path().join("moved");
        std::fs::rename(&copy, &moved).unwrap();
        let c = project_home(&moved, &WriteCap::test()).unwrap();
        assert_eq!(c.id, b.id);
        assert_eq!(registered().iter().filter(|r| r.home_id == b.id).count(), 1);
    }

    #[test]
    fn titles_follow_the_first_line_until_set() {
        let f = fixture();
        let path = new_note(&f, "");
        let (mut a, t) = attach(&path).unwrap();
        let mut t = t;
        type_into(&mut a, &mut t, 0, "# Resize notes\n");
        assert_eq!(a.title, "t");
        set_title(&a.key, "").unwrap();
        pull(&mut a);
        assert_eq!(a.title, "Resize notes");
        detach(&a);
    }
}
