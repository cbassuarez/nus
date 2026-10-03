//! Where notes live and the one way they are written. Every save, capture,
//! trash and restore goes through `commit`, under the home's writer lock,
//! and only if the note on disk is still the one the editor started from:
//! a note changed elsewhere is a conflict with both versions kept, never
//! whichever was written last.
//!
//! Two kinds of HOME. A project's is `<project>/.nus/notes/`, plain
//! Markdown any editor can open. The personal one is `profile/notes/`,
//! every file sealed by the vault before it touches the disk (only the
//! empty lock file is not). Each home has an id of its own in
//! `.state/home.json`, so a copied project is a different home even with
//! the same notes in it.
//!
//! ```text
//! <slug>--<note-id>.md                 a project note's head
//! <note-id>.md                         a personal note's head
//! .state/home.json                     the home's identity
//! .state/recovery/<op>.json            a commit in flight: both versions
//! .state/revisions/<note-id>/<rev>.md  checkpoints, pruned by policy
//! .state/trash/<note-id>.json          a trashed note's tombstone
//! .state/.writer.lock                  the lock, never replaced
//! ```
//!
//! A commit writes its intent (old and new bytes) first, then the head,
//! verifies the head, then retires the intent. Whatever point a crash
//! stops it at, `recover` finds either the finished head or the intent,
//! and never guesses between versions by modification time.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};

use crate::notes_model::{self as model, Document, ModelError};

/// A head this large still opens; it is never indexed past MAX_INDEXED.
pub const MAX_NOTE: usize = 32 * 1024 * 1024;
/// Checkpoints: at most one per five minutes of saving, a hundred per
/// note, thirty days, 256 MiB a home. Recovery intents are not history
/// and are never pruned by this.
pub const CHECKPOINT_EVERY: u64 = 5 * 60;
pub const KEEP_REVISIONS: usize = 100;
pub const KEEP_DAYS: u64 = 30;
pub const HISTORY_BUDGET: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    Folder,
    Personal,
}

/// A registered place notes live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    pub id: String,
    pub scope: Scope,
    /// The notes directory itself.
    pub root: PathBuf,
    /// The project it belongs to (Folder only).
    pub project: Option<PathBuf>,
}

/// A note's identity everywhere: which home, which note.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteKey {
    pub home_id: String,
    pub note_id: String,
}

/// What a save expects to replace: the exact bytes' hash (None: nothing
/// there yet) and the revision they carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseToken {
    pub key: NoteKey,
    pub sha256: Option<String>,
    pub revision: u64,
}

#[derive(Debug)]
pub enum NoteError {
    Io(io::Error),
    Model(ModelError),
    /// The note changed since it was read. Both versions exist: yours in
    /// the session, theirs here.
    Conflict { theirs: Vec<u8> },
    /// Personal notes need the vault, and it is not open.
    Locked,
    /// Incognito keeps no notes.
    Private,
    ReadOnly,
    /// A path outside the home, or through a symbolic link.
    Escape,
    /// Two heads with one id in one home.
    Duplicate(Vec<PathBuf>),
    NotFound,
    /// A legacy note cannot do this until it is migrated.
    Legacy,
    Busy,
}

impl std::fmt::Display for NoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoteError::Io(e) => write!(f, "{e}"),
            NoteError::Model(e) => write!(f, "{e}"),
            NoteError::Conflict { .. } => write!(f, "this note changed elsewhere; both versions are kept"),
            NoteError::Locked => write!(f, "unlock personal notes to open or search them"),
            NoteError::Private => write!(f, "incognito keeps no notes"),
            NoteError::ReadOnly => write!(f, "this project is read only"),
            NoteError::Escape => write!(f, "that path is outside the notes folder"),
            NoteError::Duplicate(p) => write!(f, "{} notes share one id", p.len()),
            NoteError::NotFound => write!(f, "the note is gone"),
            NoteError::Legacy => write!(f, "migrate this note first"),
            NoteError::Busy => write!(f, "another writer holds the notes lock"),
        }
    }
}

impl From<io::Error> for NoteError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => NoteError::ReadOnly,
            io::ErrorKind::NotFound => NoteError::NotFound,
            _ => NoteError::Io(e),
        }
    }
}

impl From<ModelError> for NoteError {
    fn from(e: ModelError) -> Self {
        NoteError::Model(e)
    }
}

pub type Result<T> = std::result::Result<T, NoteError>;

/// The right to write notes at all. Incognito never gets one, so no new
/// capture path can forget to ask.
#[derive(Clone)]
pub struct WriteCap(());

impl WriteCap {
    pub fn grant() -> Result<WriteCap> {
        if crate::private::enabled() {
            return Err(NoteError::Private);
        }
        Ok(WriteCap(()))
    }
    #[cfg(test)]
    pub fn test() -> WriteCap {
        WriteCap(())
    }
}

/// A note as read: where, what, and the base a save must match.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub key: NoteKey,
    pub path: PathBuf,
    pub doc: Document,
    pub base: BaseToken,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitAck {
    pub key: NoteKey,
    pub path: PathBuf,
    pub sha256: String,
    pub revision: u64,
    /// The bytes now on disk: the header as stamped, the body as sent.
    pub bytes_len: usize,
}

// --- homes -------------------------------------------------------------

impl Home {
    fn state(&self) -> PathBuf {
        self.root.join(".state")
    }

    fn profile(&self) -> Option<&Path> {
        (self.scope == Scope::Personal).then(|| self.root.parent()).flatten()
    }

    /// A short name for where this home is: the project's folder name, or
    /// "Personal".
    pub fn name(&self) -> String {
        match &self.project {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            None => "Personal".into(),
        }
    }

    /// The personal home in `profile`, its identity made on first use.
    /// Locked when the vault is.
    pub fn personal(profile: &Path, cap: Option<&WriteCap>) -> Result<Home> {
        nus_vault::available(profile).map_err(|_| NoteError::Locked)?;
        let root = profile.join("notes");
        let mut home = Home { id: String::new(), scope: Scope::Personal, root, project: None };
        home.id = home.identity(cap)?;
        Ok(home)
    }

    /// A project's home when it has one already; None means no notes were
    /// ever made there (nothing is created by looking).
    pub fn folder(project: &Path) -> Option<Home> {
        let project = canonical(project);
        let root = project.join(".nus").join("notes");
        let mut home = Home { id: String::new(), scope: Scope::Folder, root, project: Some(project) };
        home.id = home.identity(None).ok()?;
        Some(home)
    }

    /// A project's home, made (and kept out of git) when it is not there.
    pub fn ensure_folder(project: &Path, cap: &WriteCap) -> Result<Home> {
        let project = canonical(project);
        if !project.is_dir() {
            return Err(NoteError::NotFound);
        }
        let root = project.join(".nus").join("notes");
        let mut home = Home { id: String::new(), scope: Scope::Folder, root, project: Some(project.clone()) };
        home.id = home.identity(Some(cap))?;
        if let Err(e) = exclude_from_git(&project) {
            tracing::warn!("notes: could not keep notes out of git: {e}");
        }
        Ok(home)
    }

    /// Read (or with `cap`, make) `.state/home.json`.
    fn identity(&self, cap: Option<&WriteCap>) -> Result<String> {
        let path = self.state().join("home.json");
        match self.read_file(&path) {
            Ok(bytes) => {
                let v: Value = serde_json::from_slice(&bytes).map_err(|e| NoteError::Model(ModelError::BadHeader(e.to_string())))?;
                let id = v.get("home_id").and_then(Value::as_str).filter(|s| model::valid_id(s)).ok_or(NoteError::Model(ModelError::BadHeader("home id".into())))?;
                Ok(id.to_string())
            }
            Err(NoteError::NotFound) if cap.is_some() => {
                let id = model::new_id()?;
                let at = model::rfc3339(crate::notes::now());
                let body = serde_json::to_vec_pretty(&json!({"schema": 1, "home_id": id, "created_at": at}))
                    .map_err(|e| NoteError::Io(io::Error::other(e)))?;
                let _lock = self.lock()?;
                // Someone may have made it while we waited.
                if let Ok(bytes) = self.read_file(&path) {
                    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    if let Some(id) = v.get("home_id").and_then(Value::as_str).filter(|s| model::valid_id(s)) {
                        return Ok(id.to_string());
                    }
                }
                self.write_file(&path, &body, true)?;
                Ok(id)
            }
            Err(e) => Err(e),
        }
    }

    /// Give this home a new identity: it was copied from another that is
    /// still where it was. Its notes keep their ids; the pair differs.
    pub fn fork_identity(&mut self, _cap: &WriteCap) -> Result<()> {
        let id = model::new_id()?;
        let at = model::rfc3339(crate::notes::now());
        let body = serde_json::to_vec_pretty(&json!({"schema": 1, "home_id": id, "created_at": at, "forked_from": self.id}))
            .map_err(|e| NoteError::Io(io::Error::other(e)))?;
        let _lock = self.lock()?;
        self.write_file(&self.state().join("home.json"), &body, false)?;
        self.id = id;
        Ok(())
    }

    /// The lock every writer of this home takes. The file is never removed.
    pub fn lock(&self) -> Result<File> {
        let state = self.state();
        self.contained(&state)?;
        fs::create_dir_all(&state)?;
        let path = state.join(".writer.lock");
        let mut o = OpenOptions::new();
        o.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let file = o.open(path)?;
        // A child process forked elsewhere in nus holds a copy of the
        // descriptor until its exec: wait that out, not a real writer.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => return Err(NoteError::Busy),
            }
        }
    }

    /// `path` is inside this home and reaches it through no symbolic link.
    fn contained(&self, path: &Path) -> Result<()> {
        let base = match &self.project {
            Some(p) => p.clone(),
            None => self.root.parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        let rel = path.strip_prefix(&base).map_err(|_| NoteError::Escape)?;
        if !path.starts_with(&self.root) {
            return Err(NoteError::Escape);
        }
        let mut at = base;
        for c in rel.components() {
            let Component::Normal(part) = c else { return Err(NoteError::Escape) };
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(m) if m.file_type().is_symlink() => return Err(NoteError::Escape),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Read a file of this home: through the vault when personal.
    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.contained(path)?;
        match self.profile() {
            Some(profile) => nus_vault::read_at(profile, path).map_err(|e| match e.kind() {
                io::ErrorKind::NotFound => NoteError::NotFound,
                _ if nus_vault::available(profile).is_err() => NoteError::Locked,
                _ => NoteError::Io(e),
            }),
            None => {
                let mut f = File::open(path)?;
                if f.metadata()?.len() > MAX_NOTE as u64 {
                    return Err(NoteError::Model(ModelError::TooLarge("file")));
                }
                let mut v = Vec::new();
                f.read_to_end(&mut v)?;
                Ok(v)
            }
        }
    }

    /// Write a file of this home durably: sealed before any temporary file
    /// exists when personal; with `fresh`, never over an existing file.
    fn write_file(&self, path: &Path, bytes: &[u8], fresh: bool) -> Result<()> {
        self.contained(path)?;
        #[cfg(test)]
        faults::hit(faults::Step::Write(path.to_path_buf()))?;
        let parent = path.parent().ok_or(NoteError::Escape)?;
        fs::create_dir_all(parent)?;
        if fresh && fs::symlink_metadata(path).is_ok() {
            return Err(NoteError::Io(io::Error::new(io::ErrorKind::AlreadyExists, "a note already has that name")));
        }
        match self.profile() {
            Some(profile) => nus_vault::write_at(profile, path, bytes).map_err(|e| {
                if nus_vault::available(profile).is_err() { NoteError::Locked } else { NoteError::from(e) }
            })?,
            None => {
                let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
                tmp.write_all(bytes)?;
                tmp.as_file().sync_all()?;
                if fresh {
                    tmp.persist_noclobber(path).map_err(|e| e.error)?;
                } else {
                    tmp.persist(path).map_err(|e| e.error)?;
                }
            }
        }
        sync_dir(parent);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        self.contained(path)?;
        match fs::remove_file(path) {
            Ok(()) => {
                if let Some(p) = path.parent() {
                    sync_dir(p);
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Where a new note's head goes: a readable slug beside the id in a
    /// project, the id alone when personal (a name would say too much).
    pub fn head_path(&self, id: &str, title: &str) -> PathBuf {
        match self.scope {
            Scope::Personal => self.root.join(format!("{id}.md")),
            Scope::Folder => {
                let stem = crate::notes::file_name(title, 0);
                let stem = stem.trim_end_matches(".md");
                let slug = if stem.starts_with("note-1970") || stem.is_empty() { "note" } else { stem };
                self.root.join(format!("{slug}--{id}.md"))
            }
        }
    }
}

fn canonical(p: &Path) -> PathBuf {
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    PathBuf::from(c.to_string_lossy().trim_start_matches(r"\\?\"))
}

fn sync_dir(_path: &Path) {
    #[cfg(unix)]
    if let Ok(f) = File::open(_path) {
        let _ = f.sync_all();
    }
}

/// Keep a project's notes out of its repository, the way git keeps local
/// ignores: one line for `.nus/notes/` in the repository's own exclude
/// file (wherever `git rev-parse` says it is, worktrees included), never
/// `.gitignore`, never the rest of `.nus/`. Sharing notes with a project
/// is an explicit choice this does not make.
pub fn exclude_from_git(project: &Path) -> io::Result<()> {
    let out = nus_compat::command("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "--show-toplevel", "--git-path", "info/exclude"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let (top, exclude) = match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            let mut lines = text.lines();
            let (Some(top), Some(ex)) = (lines.next(), lines.next()) else { return Ok(()) };
            let ex = PathBuf::from(ex);
            let ex = if ex.is_absolute() { ex } else { project.join(ex) };
            (canonical(Path::new(top)), ex)
        }
        // No git, or not a repository: a plain `.git` directory above is
        // still honoured; anything else is left alone.
        _ => match project.ancestors().find(|a| a.join(".git").is_dir()) {
            Some(root) => (canonical(root), root.join(".git").join("info").join("exclude")),
            None => return Ok(()),
        },
    };
    let rel = canonical(project);
    let rel = rel.strip_prefix(&top).unwrap_or(Path::new(""));
    let mut line = String::from("/");
    for c in rel.components() {
        line.push_str(&c.as_os_str().to_string_lossy());
        line.push('/');
    }
    let broad = format!("{line}.nus/");
    line.push_str(".nus/notes/");
    let old = fs::read_to_string(&exclude).unwrap_or_default();
    if old.lines().any(|l| l.trim() == line || l.trim() == broad) {
        return Ok(());
    }
    if let Some(dir) = exclude.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut text = old;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("# nus notes (Settings · Notes)\n");
    text.push_str(&line);
    text.push('\n');
    fs::write(exclude, text)
}

// --- notes -------------------------------------------------------------

/// A legacy note (no header) has no id of its own until it is migrated;
/// it answers to one derived from its home and file name.
pub fn legacy_id(home_id: &str, path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    model::sha256(format!("legacy:{home_id}:{name}").as_bytes())[..32].to_string()
}

/// One head in a home, as listed.
#[derive(Clone, Debug)]
pub struct Entry {
    pub key: NoteKey,
    pub path: PathBuf,
    pub doc: Option<Document>,
    pub modified: u64,
    pub legacy: bool,
    /// Why it could not be read, when it could not.
    pub error: Option<String>,
}

impl Home {
    /// Every head in this home: newest first, trashed ones included (the
    /// caller hides them), unreadable ones reported, never skipped. Two
    /// heads with one id are both listed; `duplicates` names them.
    pub fn list(&self) -> Result<Vec<Entry>> {
        self.contained(&self.root)?;
        let rd = match fs::read_dir(&self.root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let path = e.path();
            if !e.file_type().is_ok_and(|t| t.is_file()) || path.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let modified = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            match self.read_file(&path) {
                Ok(bytes) => {
                    match Document::parse(&bytes) {
                        Ok(doc) => {
                            let legacy = doc.is_legacy();
                            let note_id = doc.id().map(str::to_string).unwrap_or_else(|| legacy_id(&self.id, &path));
                            out.push(Entry { key: NoteKey { home_id: self.id.clone(), note_id }, path, doc: Some(doc), modified, legacy, error: None });
                        }
                        Err(err) => out.push(Entry { key: NoteKey { home_id: self.id.clone(), note_id: legacy_id(&self.id, &path) }, path, doc: None, modified, legacy: true, error: Some(err.to_string()) }),
                    }
                }
                Err(NoteError::Locked) => return Err(NoteError::Locked),
                Err(err) => out.push(Entry { key: NoteKey { home_id: self.id.clone(), note_id: legacy_id(&self.id, &path) }, path, doc: None, modified, legacy: true, error: Some(err.to_string()) }),
            }
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
        Ok(out)
    }

    /// Heads that share an id with another head of this home.
    pub fn duplicates(entries: &[Entry]) -> Vec<Vec<PathBuf>> {
        let mut by: std::collections::BTreeMap<&str, Vec<PathBuf>> = Default::default();
        for e in entries.iter().filter(|e| !e.legacy) {
            by.entry(e.key.note_id.as_str()).or_default().push(e.path.clone());
        }
        by.into_values().filter(|v| v.len() > 1).collect()
    }

    /// Open a head by its path.
    pub fn open(&self, path: &Path) -> Result<Snapshot> {
        let bytes = self.read_file(path)?;
        let doc = Document::parse(&bytes)?;
        let note_id = doc.id().map(str::to_string).unwrap_or_else(|| legacy_id(&self.id, path));
        let key = NoteKey { home_id: self.id.clone(), note_id };
        let base = BaseToken { key: key.clone(), sha256: Some(model::sha256(&bytes)), revision: doc.revision() };
        Ok(Snapshot { key, path: path.to_path_buf(), doc, base })
    }

    /// Find a note's head by its key.
    pub fn locate(&self, key: &NoteKey) -> Result<PathBuf> {
        if key.home_id != self.id {
            return Err(NoteError::NotFound);
        }
        if self.scope == Scope::Personal {
            let p = self.root.join(format!("{}.md", key.note_id));
            if p.is_file() {
                return Ok(p);
            }
        }
        let found: Vec<PathBuf> = self.list()?.into_iter().filter(|e| e.key == *key).map(|e| e.path).collect();
        match found.len() {
            0 => Err(NoteError::NotFound),
            1 => Ok(found.into_iter().next().unwrap()),
            _ => Err(NoteError::Duplicate(found)),
        }
    }

    /// A new note, never over another: a fresh random id, published only
    /// where nothing is.
    pub fn create(&self, cap: &WriteCap, doc: Document) -> Result<Snapshot> {
        let _ = cap;
        let id = doc.id().ok_or(NoteError::Legacy)?.to_string();
        let path = self.head_path(&id, &doc.title());
        let _lock = self.lock()?;
        let bytes = doc.to_bytes();
        self.write_file(&path, &bytes, true)?;
        let key = NoteKey { home_id: self.id.clone(), note_id: id };
        let base = BaseToken { key: key.clone(), sha256: Some(model::sha256(&bytes)), revision: doc.revision() };
        Ok(Snapshot { key, path, doc, base })
    }

    /// Save a note: only over the exact bytes `expected` names. The header
    /// is stamped (next revision, time, lineage); the body goes as sent.
    pub fn commit(&self, cap: &WriteCap, path: &Path, expected: &BaseToken, mut doc: Document) -> Result<(CommitAck, Document)> {
        let _ = cap;
        if expected.key.home_id != self.id {
            return Err(NoteError::Escape);
        }
        if let Some(id) = doc.id() {
            if id != expected.key.note_id {
                return Err(NoteError::Model(ModelError::BadHeader("the note's id changed".into())));
            }
        }
        doc.check_bounds()?;
        let _lock = self.lock()?;
        let current = match self.read_file(path) {
            Ok(b) => Some(b),
            Err(NoteError::NotFound) => None,
            Err(e) => return Err(e),
        };
        let current_sha = current.as_deref().map(model::sha256);
        if current_sha != expected.sha256 {
            let theirs = current.unwrap_or_default();
            return Err(NoteError::Conflict { theirs });
        }
        if !doc.is_legacy() {
            doc.stamp_commit(current_sha.as_deref(), crate::notes::now());
        }
        let bytes = doc.to_bytes();
        let sha = model::sha256(&bytes);
        if current_sha.as_deref() == Some(sha.as_str()) {
            let ack = CommitAck { key: expected.key.clone(), path: path.to_path_buf(), sha256: sha, revision: doc.revision(), bytes_len: bytes.len() };
            return Ok((ack, doc));
        }
        // 1. The intent: both versions whole, before the head is touched.
        let op = model::new_id()?;
        let intent = self.state().join("recovery").join(format!("{op}.json"));
        let record = json!({
            "schema": 1,
            "op": op,
            "home_id": self.id,
            "note_id": expected.key.note_id,
            "head": path.file_name().map(|n| n.to_string_lossy().into_owned()),
            "previous_sha256": current_sha,
            "proposed_sha256": sha,
            "previous": current.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()),
            "proposed": String::from_utf8_lossy(&bytes),
            "at": model::rfc3339(crate::notes::now()),
        });
        let record = serde_json::to_vec(&record).map_err(|e| NoteError::Io(io::Error::other(e)))?;
        self.write_file(&intent, &record, true)?;
        #[cfg(test)]
        faults::hit(faults::Step::AfterIntent)?;
        // 2. A checkpoint of what is being replaced, when one is due.
        if let Some(old) = &current {
            if let Err(e) = self.checkpoint(&expected.key.note_id, old) {
                tracing::warn!("notes: checkpoint skipped: {e}");
            }
        }
        // 3. The head.
        self.write_file(path, &bytes, false)?;
        #[cfg(test)]
        faults::hit(faults::Step::AfterHead)?;
        // 4. Verified, then the intent retires.
        let back = self.read_file(path)?;
        if model::sha256(&back) != sha {
            return Err(NoteError::Io(io::Error::other("the saved note did not read back the same")));
        }
        self.remove_file(&intent)?;
        let ack = CommitAck { key: expected.key.clone(), path: path.to_path_buf(), sha256: sha, revision: doc.revision(), bytes_len: bytes.len() };
        Ok((ack, doc))
    }

    /// Keep `old` as a checkpoint when the newest is five minutes old, then
    /// prune by policy. Never touches recovery intents or trash.
    fn checkpoint(&self, note_id: &str, old: &[u8]) -> Result<()> {
        let dir = self.state().join("revisions").join(note_id);
        let now = crate::notes::now();
        let mut revs = self.revisions(note_id);
        if revs.first().is_some_and(|(t, _)| now.saturating_sub(*t) < CHECKPOINT_EVERY) {
            return Ok(());
        }
        let path = dir.join(format!("{now:012}-{}.md", &model::new_id()?[..8]));
        self.write_file(&path, old, true)?;
        revs.insert(0, (now, path));
        // Pruning: count and age per note.
        for (i, (t, p)) in revs.iter().enumerate() {
            if i >= KEEP_REVISIONS || now.saturating_sub(*t) > KEEP_DAYS * 86_400 {
                let _ = self.remove_file(p);
            }
        }
        self.prune_budget()?;
        Ok(())
    }

    /// Keep these bytes as a checkpoint now, whatever the five-minute rule
    /// says: the version a conflict's choice is about to replace.
    pub fn keep_version(&self, _cap: &WriteCap, note_id: &str, bytes: &[u8]) -> Result<()> {
        if !model::valid_id(note_id) {
            return Err(NoteError::Escape);
        }
        let _lock = self.lock()?;
        let path = self.state().join("revisions").join(note_id).join(format!("{:012}-{}.md", crate::notes::now(), &model::new_id()?[..8]));
        self.write_file(&path, bytes, true)
    }

    /// A note's checkpoints, newest first, as (time, path).
    pub fn revisions(&self, note_id: &str) -> Vec<(u64, PathBuf)> {
        let dir = self.state().join("revisions").join(note_id);
        let mut v: Vec<(u64, PathBuf)> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let t = p.file_name()?.to_str()?.split('-').next()?.parse().ok()?;
                Some((t, p))
            })
            .collect();
        v.sort_by(|a, b| b.cmp(a));
        v
    }

    /// Hold history under the home's budget: oldest checkpoints first.
    fn prune_budget(&self) -> Result<()> {
        let root = self.state().join("revisions");
        let mut all: Vec<(u64, u64, PathBuf)> = Vec::new();
        for d in fs::read_dir(&root).into_iter().flatten().flatten() {
            for f in fs::read_dir(d.path()).into_iter().flatten().flatten() {
                let p = f.path();
                let size = f.metadata().map_or(0, |m| m.len());
                let t = p.file_name().and_then(|n| n.to_str()?.split('-').next()?.parse().ok()).unwrap_or(0);
                all.push((t, size, p));
            }
        }
        let mut total: u64 = all.iter().map(|a| a.1).sum();
        all.sort();
        for (_, size, p) in all {
            if total <= HISTORY_BUDGET {
                break;
            }
            self.remove_file(&p)?;
            total -= size;
        }
        Ok(())
    }

    /// History's size against its budget, for Settings.
    pub fn history_bytes(&self) -> u64 {
        let root = self.state().join("revisions");
        fs::read_dir(&root).into_iter().flatten().flatten()
            .flat_map(|d| fs::read_dir(d.path()).into_iter().flatten().flatten())
            .map(|f| f.metadata().map_or(0, |m| m.len()))
            .sum()
    }

    /// Put a note in the trash: a new revision saying so, and a tombstone.
    /// Nothing is deleted.
    pub fn trash(&self, cap: &WriteCap, path: &Path, expected: &BaseToken) -> Result<CommitAck> {
        let snap = self.open(path)?;
        if snap.doc.is_legacy() {
            return Err(NoteError::Legacy);
        }
        let mut doc = snap.doc;
        let now = crate::notes::now();
        doc.set_deleted(Some(now));
        let (ack, _) = self.commit(cap, path, expected, doc)?;
        let stone = json!({"schema": 1, "note_id": ack.key.note_id, "head": path.file_name().map(|n| n.to_string_lossy().into_owned()), "deleted_at": model::rfc3339(now), "revision": ack.revision});
        let stone = serde_json::to_vec(&stone).map_err(|e| NoteError::Io(io::Error::other(e)))?;
        let _lock = self.lock()?;
        self.write_file(&self.state().join("trash").join(format!("{}.json", ack.key.note_id)), &stone, false)?;
        Ok(ack)
    }

    /// Bring a trashed note back, as a new revision.
    pub fn restore(&self, cap: &WriteCap, path: &Path, expected: &BaseToken) -> Result<CommitAck> {
        let snap = self.open(path)?;
        let mut doc = snap.doc;
        doc.set_deleted(None);
        let (ack, _) = self.commit(cap, path, expected, doc)?;
        let _lock = self.lock()?;
        self.remove_file(&self.state().join("trash").join(format!("{}.json", ack.key.note_id)))?;
        Ok(ack)
    }
}

// --- migration ---------------------------------------------------------

impl Home {
    /// Make a legacy note canonical, on purpose and copy first: the new
    /// note is written, then the original's exact bytes move to
    /// `.state/imports/` (resealed there when personal) and the ledger
    /// says what became what. Running it again for the same original
    /// finds the note it already made.
    pub fn adopt_legacy(&self, cap: &WriteCap, path: &Path, doc: Document) -> Result<Snapshot> {
        let original = self.read_file(path)?;
        let original_sha = model::sha256(&original);
        let ledger_path = self.state().join("imports.json");
        let mut ledger: Value = match self.read_file(&ledger_path) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|_| json!({"schema": 1, "imports": []})),
            Err(NoteError::NotFound) => json!({"schema": 1, "imports": []}),
            Err(e) => return Err(e),
        };
        let done = ledger["imports"].as_array().and_then(|a| a.iter().find(|e| e["original_sha256"] == original_sha.as_str() && e["status"] == "imported")).and_then(|e| e["target"].as_str().map(|t| self.root.join(t)));
        if let Some(target) = done.filter(|t| t.is_file()) {
            return self.open(&target);
        }
        let snap = self.create(cap, doc)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let kept = self.state().join("imports").join(format!("{}-{name}", &original_sha[..12]));
        let _lock = self.lock()?;
        if !kept.exists() {
            self.write_file(&kept, &original, true)?;
        }
        if model::sha256(&self.read_file(&kept)?) != original_sha {
            return Err(NoteError::Io(io::Error::other("the kept original did not read back the same")));
        }
        let entry = json!({
            "legacy_kind": "markdown",
            "locator": name,
            "original_sha256": original_sha,
            "kept": kept.file_name().map(|n| n.to_string_lossy().into_owned()),
            "note_id": snap.key.note_id,
            "target": snap.path.file_name().map(|n| n.to_string_lossy().into_owned()),
            "imported_sha256": snap.base.sha256,
            "status": "imported",
            "at": model::rfc3339(crate::notes::now()),
        });
        if let Some(a) = ledger["imports"].as_array_mut() {
            a.push(entry);
        }
        let bytes = serde_json::to_vec_pretty(&ledger).map_err(|e| NoteError::Io(io::Error::other(e)))?;
        self.write_file(&ledger_path, &bytes, false)?;
        // Last: the original leaves the notes list (its bytes are kept).
        self.remove_file(path)?;
        Ok(snap)
    }
}

// --- recovery ----------------------------------------------------------

/// What an unfinished commit left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovered {
    /// The head is the new version: the commit only missed its last step.
    Finished { key: NoteKey },
    /// The head is still the old version: the new one is a draft to offer.
    Draft { op: String, key: NoteKey, head: PathBuf, proposed: String },
    /// The head is neither: every version is kept for you to choose.
    Diverged { op: String, key: NoteKey, head: PathBuf, previous: Option<String>, proposed: String, current: Option<String> },
}

impl Home {
    /// Settle every intent a crash left, idempotently: finished ones are
    /// retired, the rest are returned and kept until chosen.
    pub fn recover(&self, _cap: &WriteCap) -> Result<Vec<Recovered>> {
        let dir = self.state().join("recovery");
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(&dir) else { return Ok(out) };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        for intent in files {
            let bytes = match self.read_file(&intent) {
                Ok(b) => b,
                Err(NoteError::Locked) => return Err(NoteError::Locked),
                Err(e) => {
                    tracing::warn!("notes: recovery record unreadable, kept: {e}");
                    continue;
                }
            };
            let Ok(v) = serde_json::from_slice::<Value>(&bytes) else { continue };
            let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
            let (Some(op), Some(note_id), Some(head), Some(proposed_sha), Some(proposed)) = (s("op"), s("note_id"), s("head"), s("proposed_sha256"), s("proposed")) else { continue };
            if head.contains(['/', '\\']) || head == ".." {
                continue;
            }
            let key = NoteKey { home_id: self.id.clone(), note_id };
            let path = self.root.join(&head);
            let current = match self.read_file(&path) {
                Ok(b) => Some(b),
                Err(NoteError::NotFound) => None,
                Err(e) => return Err(e),
            };
            let current_sha = current.as_deref().map(model::sha256);
            if current_sha.as_deref() == Some(proposed_sha.as_str()) {
                self.remove_file(&intent)?;
                out.push(Recovered::Finished { key });
            } else if current_sha == s("previous_sha256") {
                out.push(Recovered::Draft { op, key, head: path, proposed });
            } else {
                out.push(Recovered::Diverged { op, key, head: path, previous: s("previous"), proposed, current: current.map(|b| String::from_utf8_lossy(&b).into_owned()) });
            }
        }
        Ok(out)
    }

    /// Done with a recovery record: its draft was taken or let go.
    pub fn retire(&self, _cap: &WriteCap, op: &str) -> Result<()> {
        if !model::valid_id(op) {
            return Err(NoteError::Escape);
        }
        let _lock = self.lock()?;
        self.remove_file(&self.state().join("recovery").join(format!("{op}.json")))
    }
}

/// Crash injection for the tests: fail at a named step of a commit.
#[cfg(test)]
pub mod faults {
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Clone, Debug, PartialEq)]
    pub enum Step {
        AfterIntent,
        AfterHead,
        Write(PathBuf),
    }

    thread_local! {
        static AT: RefCell<Option<Step>> = const { RefCell::new(None) };
        static WRITE_FAILS: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// Stop the next commit at `step` (a crash, as far as disk knows).
    pub fn at(step: Option<Step>) {
        AT.with(|a| *a.borrow_mut() = step);
    }

    /// Fail every write whose path contains `part` (disk full, say).
    pub fn writes_fail(part: Option<&str>) {
        WRITE_FAILS.with(|w| *w.borrow_mut() = part.map(str::to_string));
    }

    pub(super) fn hit(step: Step) -> super::Result<()> {
        if let Step::Write(p) = &step {
            let fail = WRITE_FAILS.with(|w| w.borrow().clone());
            if fail.is_some_and(|f| p.to_string_lossy().contains(&f)) {
                return Err(super::NoteError::Io(std::io::Error::other("injected: no space left on device")));
            }
            return Ok(());
        }
        let now = AT.with(|a| a.borrow().clone());
        if now.as_ref() == Some(&step) {
            AT.with(|a| *a.borrow_mut() = None);
            return Err(super::NoteError::Io(std::io::Error::other(format!("injected crash at {step:?}"))));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Home) {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::ensure_folder(dir.path(), &WriteCap::test()).unwrap();
        (dir, home)
    }

    fn note(home: &Home, title: &str, body: &str) -> Snapshot {
        let mut doc = Document::new(&model::new_id().unwrap(), title, true, 0);
        doc.body = body.into();
        home.create(&WriteCap::test(), doc).unwrap()
    }

    #[test]
    fn a_home_has_one_identity_and_looking_makes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Home::folder(dir.path()).is_none());
        assert!(!dir.path().join(".nus").exists());
        let a = Home::ensure_folder(dir.path(), &WriteCap::test()).unwrap();
        let b = Home::folder(dir.path()).unwrap();
        assert_eq!(a.id, b.id);
        assert!(model::valid_id(&a.id));
        assert_eq!(a.name(), dir.path().file_name().unwrap().to_string_lossy());
    }

    #[test]
    fn create_never_clobbers_and_same_titles_differ() {
        let (_d, home) = project();
        let a = note(&home, "Same", "one");
        let b = note(&home, "Same", "two");
        assert_ne!(a.path, b.path);
        assert!(a.path.file_name().unwrap().to_string_lossy().starts_with("same--"));
        let mut again = a.doc.clone();
        again.body = "clobber".into();
        assert!(home.create(&WriteCap::test(), again).is_err());
        assert_eq!(home.open(&a.path).unwrap().doc.body, "one");
    }

    #[test]
    fn a_commit_needs_the_bytes_it_started_from() {
        let (_d, home) = project();
        let s = note(&home, "t", "first");
        let mut doc = s.doc.clone();
        doc.body = "second".into();
        let (ack, saved) = home.commit(&WriteCap::test(), &s.path, &s.base, doc).unwrap();
        assert_eq!(saved.revision(), 2);
        assert_eq!(ack.revision, 2);
        // Same base again: the note moved on, so this is a conflict and
        // neither version is lost.
        let mut stale = s.doc.clone();
        stale.body = "third".into();
        match home.commit(&WriteCap::test(), &s.path, &s.base, stale) {
            Err(NoteError::Conflict { theirs }) => {
                assert_eq!(model::sha256(&theirs), ack.sha256);
                assert!(String::from_utf8(theirs).unwrap().ends_with("second"));
            }
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert_eq!(home.open(&s.path).unwrap().doc.body, "second");
    }

    #[test]
    fn an_external_edit_of_the_same_length_is_still_seen() {
        let (_d, home) = project();
        let s = note(&home, "t", "abc");
        let text = fs::read_to_string(&s.path).unwrap().replace("abc", "abd");
        let mtime = fs::metadata(&s.path).unwrap().modified().unwrap();
        fs::write(&s.path, text).unwrap();
        File::options().write(true).open(&s.path).unwrap().set_modified(mtime).unwrap();
        let mut doc = s.doc.clone();
        doc.body = "mine".into();
        assert!(matches!(home.commit(&WriteCap::test(), &s.path, &s.base, doc), Err(NoteError::Conflict { .. })));
    }

    #[test]
    fn unknown_fields_survive_a_commit() {
        let (_d, home) = project();
        let mut doc = Document::new(&model::new_id().unwrap(), "t", true, 0);
        doc.meta.insert("custom_owner".into(), json!({"keep": [1, 2]}));
        let s = home.create(&WriteCap::test(), doc).unwrap();
        let mut d = s.doc.clone();
        d.body = "x".into();
        home.commit(&WriteCap::test(), &s.path, &s.base, d).unwrap();
        assert_eq!(home.open(&s.path).unwrap().doc.meta["custom_owner"], json!({"keep": [1, 2]}));
    }

    #[test]
    fn a_crash_at_any_step_leaves_a_head_or_an_intent() {
        for step in [faults::Step::AfterIntent, faults::Step::AfterHead] {
            let (_d, home) = project();
            let s = note(&home, "t", "old");
            let mut doc = s.doc.clone();
            doc.body = "new".into();
            faults::at(Some(step.clone()));
            assert!(home.commit(&WriteCap::test(), &s.path, &s.base, doc).is_err());
            let rec = home.recover(&WriteCap::test()).unwrap();
            assert_eq!(rec.len(), 1, "{step:?}");
            match (&step, &rec[0]) {
                (faults::Step::AfterIntent, Recovered::Draft { proposed, op, .. }) => {
                    assert!(proposed.ends_with("new"));
                    assert_eq!(home.open(&s.path).unwrap().doc.body, "old");
                    // Kept until chosen; taking it retires it.
                    assert_eq!(home.recover(&WriteCap::test()).unwrap().len(), 1);
                    home.retire(&WriteCap::test(), op).unwrap();
                }
                (faults::Step::AfterHead, Recovered::Finished { .. }) => {
                    assert_eq!(home.open(&s.path).unwrap().doc.body, "new");
                }
                other => panic!("unexpected {other:?}"),
            }
            assert!(home.recover(&WriteCap::test()).unwrap().is_empty(), "idempotent: {step:?}");
        }
    }

    #[test]
    fn an_intent_whose_head_moved_on_keeps_every_version() {
        let (_d, home) = project();
        let s = note(&home, "t", "old");
        let mut doc = s.doc.clone();
        doc.body = "new".into();
        faults::at(Some(faults::Step::AfterIntent));
        let _ = home.commit(&WriteCap::test(), &s.path, &s.base, doc);
        fs::write(&s.path, "someone else").unwrap();
        match &home.recover(&WriteCap::test()).unwrap()[0] {
            Recovered::Diverged { previous, proposed, current, .. } => {
                assert!(previous.as_ref().unwrap().ends_with("old"));
                assert!(proposed.ends_with("new"));
                assert_eq!(current.as_deref(), Some("someone else"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_failed_write_keeps_the_old_head() {
        let (_d, home) = project();
        let s = note(&home, "t", "old");
        let mut doc = s.doc.clone();
        doc.body = "new".into();
        faults::writes_fail(Some(".md"));
        let r = home.commit(&WriteCap::test(), &s.path, &s.base, doc);
        faults::writes_fail(None);
        assert!(r.is_err());
        assert_eq!(home.open(&s.path).unwrap().doc.body, "old");
    }

    #[test]
    fn trash_is_a_revision_and_restore_undoes_it() {
        let (_d, home) = project();
        let s = note(&home, "t", "keep me");
        let ack = home.trash(&WriteCap::test(), &s.path, &s.base).unwrap();
        let t = home.open(&s.path).unwrap();
        assert!(t.doc.deleted_at().is_some());
        assert_eq!(t.doc.body, "keep me");
        assert!(home.root.join(".state/trash").join(format!("{}.json", ack.key.note_id)).is_file());
        home.restore(&WriteCap::test(), &s.path, &t.base).unwrap();
        let r = home.open(&s.path).unwrap();
        assert!(r.doc.deleted_at().is_none());
        assert_eq!(r.doc.revision(), 3);
    }

    #[test]
    fn checkpoints_keep_the_replaced_version() {
        let (_d, home) = project();
        let s = note(&home, "t", "v1");
        let mut doc = s.doc.clone();
        doc.body = "v2".into();
        let (ack, _) = home.commit(&WriteCap::test(), &s.path, &s.base, doc.clone()).unwrap();
        let revs = home.revisions(&s.key.note_id);
        assert_eq!(revs.len(), 1);
        assert!(fs::read_to_string(&revs[0].1).unwrap().ends_with("v1"));
        // A second save inside five minutes adds none.
        let base = BaseToken { key: s.key.clone(), sha256: Some(ack.sha256), revision: ack.revision };
        doc.body = "v3".into();
        home.commit(&WriteCap::test(), &s.path, &base, doc).unwrap();
        assert_eq!(home.revisions(&s.key.note_id).len(), 1);
    }

    #[test]
    fn listing_reads_legacy_notes_and_names_duplicates() {
        let (_d, home) = project();
        let a = note(&home, "a", "x");
        fs::write(home.root.join("old.md"), "---\nmade: 2026-01-01T00:00Z\n---\n# Old one\n").unwrap();
        fs::copy(&a.path, home.root.join("copy.md")).unwrap();
        let list = home.list().unwrap();
        assert_eq!(list.len(), 3);
        let old = list.iter().find(|e| e.legacy).unwrap();
        assert_eq!(old.doc.as_ref().unwrap().title(), "Old one");
        assert_eq!(old.key.note_id, legacy_id(&home.id, &home.root.join("old.md")));
        let dup = Home::duplicates(&list);
        assert_eq!(dup.len(), 1);
        assert_eq!(dup[0].len(), 2);
        assert!(matches!(home.locate(&a.key), Err(NoteError::Duplicate(_))));
    }

    #[test]
    fn a_copied_project_can_take_its_own_identity() {
        let (d, home) = project();
        let s = note(&home, "t", "x");
        let copy = tempfile::tempdir().unwrap();
        let dst = copy.path().join(".nus/notes/.state");
        fs::create_dir_all(&dst).unwrap();
        fs::copy(home.root.join(".state/home.json"), dst.join("home.json")).unwrap();
        fs::copy(&s.path, copy.path().join(".nus/notes").join(s.path.file_name().unwrap())).unwrap();
        let mut other = Home::folder(copy.path()).unwrap();
        assert_eq!(other.id, home.id);
        other.fork_identity(&WriteCap::test()).unwrap();
        assert_ne!(other.id, home.id);
        assert_eq!(Home::folder(copy.path()).unwrap().id, other.id);
        let listed = other.list().unwrap();
        assert_eq!(listed[0].key.note_id, s.key.note_id);
        assert_ne!(listed[0].key, s.key);
        drop(d);
    }

    #[test]
    #[cfg(unix)]
    fn paths_through_links_are_refused() {
        let (_d, home) = project();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), home.root.join("linked")).unwrap();
        assert!(matches!(home.read_file(&home.root.join("linked/x.md")), Err(NoteError::Escape)));
        assert!(matches!(home.read_file(&home.root.join("../../x.md")), Err(NoteError::Escape)));
    }

    #[test]
    fn git_exclude_names_only_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        fs::create_dir_all(repo.join(".git/info")).unwrap();
        fs::create_dir_all(repo.join("sub")).unwrap();
        exclude_from_git(&repo.join("sub")).unwrap();
        exclude_from_git(&repo.join("sub")).unwrap();
        let text = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert_eq!(text.matches("/sub/.nus/notes/").count(), 1, "{text}");
        // An older, broader line already covers it.
        fs::write(repo.join(".git/info/exclude"), "/.nus/\n").unwrap();
        exclude_from_git(repo).unwrap();
        assert_eq!(fs::read_to_string(repo.join(".git/info/exclude")).unwrap(), "/.nus/\n");
    }

    #[test]
    fn personal_notes_are_sealed_on_disk_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("profile");
        fs::create_dir_all(&profile).unwrap();
        nus_vault::install_test_key(&profile).unwrap();
        let home = Home::personal(&profile, Some(&WriteCap::test())).unwrap();
        let mut doc = Document::new(&model::new_id().unwrap(), "PLAINTEXT-TITLE", true, 0);
        doc.body = "PLAINTEXT-BODY".into();
        let s = home.create(&WriteCap::test(), doc).unwrap();
        assert_eq!(s.path.file_name().unwrap().to_string_lossy(), format!("{}.md", s.key.note_id));
        let mut d = s.doc.clone();
        d.body = "PLAINTEXT-SECOND".into();
        faults::at(Some(faults::Step::AfterIntent));
        let _ = home.commit(&WriteCap::test(), &s.path, &s.base, d);
        let mut stack = vec![profile.join("notes")];
        let mut files = 0;
        while let Some(p) = stack.pop() {
            for e in fs::read_dir(&p).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                    continue;
                }
                files += 1;
                let bytes = fs::read(e.path()).unwrap();
                let text = String::from_utf8_lossy(&bytes);
                assert!(!text.contains("PLAINTEXT"), "{} holds plaintext", e.path().display());
            }
        }
        assert!(files >= 4, "head, home, lock, intent: {files}");
        assert_eq!(home.open(&s.path).unwrap().doc.body, "PLAINTEXT-BODY");
        assert!(matches!(home.recover(&WriteCap::test()).unwrap()[0], Recovered::Draft { .. }));
        nus_vault::remove_test_key(&profile).unwrap();
    }
}
