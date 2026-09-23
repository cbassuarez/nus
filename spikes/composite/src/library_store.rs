//! Backwards-compatible storage for profile/library. No GUI or network API.
//! Mutations lock and reread one record, then apply only the requested delta.
//! Existing <id>.article files remain readable and are never migrated/deleted.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_TEXT: usize = 2 * 1024 * 1024;
pub const MAX_BLOCKS: usize = 4096;
const MAX_RECORD: usize = 64 * 1024;
const MAX_ENTRIES: usize = 20_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub quote: String,
    #[serde(default)] pub before: String,
    #[serde(default)] pub after: String,
    pub fraction: f32,
    #[serde(default)] pub snapshot: String,
    #[serde(default)] pub block: Option<u32>,
    #[serde(default)] pub offset: u32,
    #[serde(default)] pub block_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub source: String,
    pub title: String,
    pub saved: u64,
    #[serde(default)] pub words: usize,
    #[serde(default)] pub progress: f32,
    #[serde(default)] pub anchor: String,
    #[serde(default)] pub archived: bool,
    // Additive fields: all old records deserialize without being rewritten.
    #[serde(default)] pub schema: u32,
    #[serde(default)] pub revision: u64,
    #[serde(default)] pub container: Option<String>,
    #[serde(default)] pub finished: bool,
    #[serde(default)] pub deleted: bool,
    #[serde(default)] pub snapshot: Option<String>,
    #[serde(default)] pub position: Option<Position>,
    #[serde(default)] pub capture: Option<String>,
    #[serde(default)] pub note: String,
    // Preserve metadata written by compatible clients instead of losing it.
    #[serde(flatten)] pub extra: BTreeMap<String, serde_json::Value>,
}

pub fn id(source: &str) -> String {
    // The ORIGINAL library identifier, retained for existing personal entries.
    let mut a = 0xcbf29ce484222325u64;
    let mut b = 0x84222325cbf29ce4u64;
    for c in source.bytes() {
        a = (a ^ c as u64).wrapping_mul(0x100000001b3);
        b = (b ^ c as u64).wrapping_mul(0x100000001b3).rotate_left(7);
    }
    format!("{a:016x}{b:016x}")
}
pub fn valid_id(s: &str) -> bool { s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()) }
fn valid_hash(s: &str) -> bool { s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) }
pub fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn invalid(s: impl Into<String>) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, s.into()) }
fn conflict() -> io::Error { io::Error::other("The saved item changed; reopen it before retrying.") }
pub fn validate_position(p: &Position) -> io::Result<()> {
    if !p.fraction.is_finite() || !(0.0..=1.0).contains(&p.fraction) || p.quote.len() > 4096
        || p.before.len() > 1024 || p.after.len() > 1024 || p.snapshot.len() > 128
        || p.block_hash.len() > 128 { return Err(invalid("Invalid reading position")); }
    Ok(())
}
fn check_entry(e: &Entry) -> io::Result<()> {
    if !valid_id(&e.id) || e.schema > 1 || !e.progress.is_finite() || !(0.0..=1.0).contains(&e.progress)
        || e.source.len() > 16384 || e.title.len() > 8192 || e.anchor.len() > 8192 || e.note.len() > 8192
        || e.container.as_ref().is_some_and(|c| c.len() > 1024)
        || e.snapshot.as_ref().is_some_and(|s| !valid_hash(s)) { return Err(invalid("Invalid or newer library record; original left untouched")); }
    if let Some(p) = &e.position { validate_position(p)?; }
    Ok(())
}
fn no_symlinks(path: &Path) -> io::Result<()> {
    for part in path.ancestors() {
        if part.as_os_str().is_empty() { continue; }
        match fs::symlink_metadata(part) {
            Ok(m) if m.file_type().is_symlink() => return Err(invalid("Library paths must not traverse symbolic links")),
            Ok(_) => {},
            Err(e) if e.kind() == io::ErrorKind::NotFound => {},
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    no_symlinks(path)?;
    let file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit as u64 { return Err(invalid("Library file exceeds its size limit")); }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err(invalid("Library file grew beyond its size limit")); }
    Ok(bytes)
}
fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)] { File::open(path)?.sync_all()?; }
    #[cfg(not(unix))] { let _ = path; }
    Ok(())
}
fn atomic(path: &Path, bytes: &[u8], immutable: bool) -> io::Result<()> {
    no_symlinks(path)?;
    let parent = path.parent().ok_or_else(|| invalid("Missing library parent"))?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    if immutable {
        match tmp.persist_noclobber(path) {
            Ok(_) => {},
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {
                if bounded(path, MAX_TEXT)? != bytes { return Err(invalid("Existing snapshot object failed integrity check")); }
            },
            Err(e) => return Err(e.error),
        }
    } else { tmp.persist(path).map_err(|e| e.error)?; }
    sync_dir(parent)
}
fn token() -> io::Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(b.iter().map(|n| format!("{n:02x}")).collect())
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    #[cfg(test)] fail_record_write: bool,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root, #[cfg(test)] fail_record_write: false }
    }
    fn path(&self, key: &str) -> io::Result<PathBuf> {
        if !valid_id(key) { return Err(invalid("Invalid library identifier")); }
        Ok(self.root.join(format!("{key}.json")))
    }
    fn lock(&self) -> io::Result<File> {
        no_symlinks(&self.root)?;
        fs::create_dir_all(&self.root)?;
        let path = self.root.join(".writer.lock");
        no_symlinks(&path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let file = options.open(path)?;
        // Rust 1.89+: OS lock is released on close, even after a crash. Never
        // remove/recreate the lockfile (which would allow two different locks).
        file.try_lock().map_err(|e| io::Error::other(format!("Library writer is busy or unavailable: {e}")))?;
        Ok(file)
    }
    pub fn read(&self, key: &str) -> io::Result<Entry> {
        let bytes = bounded(&self.path(key)?, MAX_RECORD)?;
        let e: Entry = serde_json::from_slice(&bytes).map_err(invalid_json)?;
        check_entry(&e)?;
        if e.id != key { return Err(invalid("Library identifier does not match its filename")); }
        Ok(e)
    }
    fn write(&self, e: &mut Entry) -> io::Result<()> {
        check_entry(e)?;
        e.schema = 1;
        e.revision = e.revision.checked_add(1).ok_or_else(|| invalid("Library revision overflow"))?;
        let bytes = serde_json::to_vec_pretty(e).map_err(invalid_json)?;
        if bytes.len() > MAX_RECORD { return Err(invalid("Library metadata exceeds 64 KiB")); }
        #[cfg(test)] if self.fail_record_write {
            return Err(io::Error::other("injected failure before record replacement"));
        }
        atomic(&self.path(&e.id)?, &bytes, false)
    }
    pub fn list(&self) -> io::Result<(BTreeMap<String, Entry>, Vec<String>)> {
        no_symlinks(&self.root)?;
        let files = match fs::read_dir(&self.root) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((BTreeMap::new(), vec![])),
            Err(e) => return Err(e),
        };
        let mut out = BTreeMap::new();
        let mut errors = Vec::new();
        let mut count = 0;
        for file in files {
            let file = file?;
            let path = file.path();
            if path.extension().is_none_or(|e| e != "json") { continue; }
            count += 1;
            if count > MAX_ENTRIES { return Err(invalid("Library exceeds 20,000 records; refusing a partial listing")); }
            let key = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            match self.read(key) {
                Ok(e) => { out.insert(e.id.clone(), e); },
                Err(e) => errors.push(format!("{}: {e}", file.file_name().to_string_lossy())),
            }
        }
        Ok((out, errors))
    }
    /// `container=None` is the ORIGINAL personal/file namespace. URLs retain
    /// queries and fragments; new non-personal containers are separate records.
    pub fn save_link(&self, source: &str, title: &str, container: Option<String>, now: u64) -> io::Result<(Entry, bool)> {
        if source.is_empty() || source.len() > 16384 || title.len() > 8192 { return Err(invalid("Invalid saved source or title")); }
        let key = match &container { None => id(source), Some(c) => id(&format!("container\0{c}\0{source}")) };
        let _lock = self.lock()?;
        let (entries,errors)=self.list()?;
        if !errors.is_empty(){return Err(invalid("Unreadable library records; save was not applied"));}
        if let Some(mut existing)=entries.into_values().find(|e|e.source==source && e.container==container) {
            if existing.deleted {existing.deleted=false;existing.capture=None;self.write(&mut existing)?;}
            return Ok((existing,false));
        }
        let key=match self.read(&key) {
            Ok(e) if e.extra.get("original_source").and_then(|v|v.as_str())==Some(source)=>id(&token()?),
            Ok(_)=>return Err(invalid("Reading identifier collision; original left untouched")),
            Err(e) if e.kind()==io::ErrorKind::NotFound=>key,
            Err(e)=>return Err(e),
        };
        match self.read(&key) {
            Ok(mut e) => {
                if e.source != source || e.container != container { return Err(invalid("Reading identifier collision; original left untouched")); }
                // An explicit Save can restore a removed item even after
                // relaunch. Rotate/cancel its old ticket; never resurrect a
                // capture that was in flight before removal.
                if e.deleted { e.deleted = false; e.capture = None; self.write(&mut e)?; }
                return Ok((e, false)); // no title/archival/progress reset
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {},
            Err(e) => return Err(e),
        }
        let mut e = Entry { id: key, source: source.into(), title: title.into(), saved: now, words: 0,
            progress: 0.0, anchor: String::new(), archived: false, schema: 1, revision: 0,
            container, finished: false, deleted: false, snapshot: None, position: None,
            capture: None, note: String::new(), extra: BTreeMap::new() };
        self.write(&mut e)?;
        Ok((e, true))
    }
    /// Install a versioned set of link defaults once. Unlike an explicit Save,
    /// this never restores tombstones or rewrites an existing record. All reads,
    /// record writes and the completion marker share the normal writer lock.
    /// A crash before the marker is safe: a retry preserves any completed records.
    pub fn seed_links_once(&self, version: u32, links: &[(&str, &str)], now: u64) -> io::Result<Vec<Entry>> {
        if version == 0 || links.is_empty() || links.len() > 32 {
            return Err(invalid("Invalid reading-default version or count"));
        }
        // Construct and validate everything before touching the filesystem.
        let mut candidates = BTreeMap::new();
        for &(source, title) in links {
            if source.is_empty() { return Err(invalid("Empty reading-default source")); }
            let e = Entry { id: id(source), source: source.into(), title: title.into(), saved: now,
                words: 0, progress: 0.0, anchor: String::new(), archived: false,
                schema: 1, revision: 0, container: None, finished: false, deleted: false,
                snapshot: None, position: None, capture: None, note: String::new(), extra: BTreeMap::new() };
            check_entry(&e)?;
            if candidates.insert(e.id.clone(), e).is_some() {
                return Err(invalid("Duplicate reading-default identifier"));
            }
        }
        let _lock = self.lock()?;
        // Not JSON: list() must never mistake this for an Entry. The typed
        // version also means a caller cannot supply a path or lockfile name.
        let marker = self.root.join(format!(".defaults-v{version}"));
        match bounded(&marker, 16) {
            Ok(bytes) if bytes == b"complete\n" => return Ok(Vec::new()),
            Ok(_) => return Err(invalid("Unrecognized reading-default marker; original left untouched")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {},
            Err(e) => return Err(e),
        }
        let mut missing = Vec::new();
        for candidate in candidates.into_values() {
            match self.read(&candidate.id) {
                Ok(existing) => {
                    if existing.source != candidate.source || existing.container.is_some() {
                        return Err(invalid("Reading-default identifier collision; original left untouched"));
                    }
                    // Existing means existing, even when deleted or archived.
                },
                Err(e) if e.kind() == io::ErrorKind::NotFound => missing.push(candidate),
                Err(e) => return Err(e),
            }
        }
        for e in &mut missing { self.write(e)?; }
        atomic(&marker, b"complete\n", true)?;
        Ok(missing)
    }

    pub fn start_capture(&self, key: &str) -> io::Result<Entry> {
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if e.deleted { return Err(conflict()); }
        e.capture = Some(token()?);
        self.write(&mut e)?;
        Ok(e)
    }
    pub fn capture_failed(&self, key: &str, ticket: &str, reason: &str) -> io::Result<Entry> {
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if e.deleted || e.capture.as_deref() != Some(ticket) { return Err(conflict()); }
        e.capture = None;
        e.note = reason.chars().take(1000).collect();
        self.write(&mut e)?;
        Ok(e)
    }
    pub fn commit(&self, key: &str, ticket: &str, bytes: &[u8], title: &str, words: usize, note: &str) -> io::Result<Entry> {
        validate_article(bytes)?;
        let hash = digest(bytes);
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if e.deleted || e.capture.as_deref() != Some(ticket) { return Err(conflict()); }
        // Immutable content first; record reference is the commit point. A
        // failed record write cannot replace an older readable snapshot.
        atomic(&self.root.join("objects").join(format!("{hash}.article")), bytes, true)?;
        e.snapshot = Some(hash);
        e.capture = None;
        if !title.is_empty() { e.title = title.into(); }
        e.words = words;
        e.note = note.chars().take(1000).collect();
        self.write(&mut e)?;
        Ok(e)
    }
    pub fn article(&self, e: &Entry) -> io::Result<(Vec<u8>, String)> {
        check_entry(e)?;
        let path = match &e.snapshot {
            Some(h) => self.root.join("objects").join(format!("{h}.article")),
            None if e.extra.get("legacy_article_disabled").and_then(|v| v.as_bool()) == Some(true) => {
                return Err(io::Error::new(io::ErrorKind::NotFound, "The edited source has no saved article yet"));
            }
            None => self.root.join(format!("{}.article", e.id)),
        };
        let bytes = bounded(&path, MAX_TEXT)?;
        validate_article(&bytes)?;
        let hash = digest(&bytes);
        if e.snapshot.as_ref().is_some_and(|h| h != &hash) { return Err(invalid("Saved article failed integrity check")); }
        Ok((bytes, hash))
    }
    pub fn progress(&self, key: &str, p: &Position) -> io::Result<Entry> {
        validate_position(p)?;
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if e.deleted { return Err(conflict()); }
        let current = match &e.snapshot { Some(h) => h.clone(), None => self.article(&e)?.1 };
        if current != p.snapshot { return Err(conflict()); }
        e.progress = p.fraction;
        e.anchor = p.quote.clone();
        e.position = Some(p.clone());
        self.write(&mut e)?;
        Ok(e)
    }
    /// Save an explicit form in one record commit. Edits keep their identity;
    /// another window's changes cause a conflict instead of being overwritten.
    pub fn save_item(&self, target: Option<(&str,u64)>, source: &str, title: &str, notes: &str, now: u64) -> io::Result<Entry> {
        if source.is_empty() || source.len()>16384 || title.trim().is_empty() || title.len()>8192 || notes.len()>24000 {return Err(invalid("Title, source or notes exceed their limit"));}
        let _lock=self.lock()?;
        let (all,errors)=self.list()?;
        if !errors.is_empty(){return Err(invalid("Repair unreadable library records before editing sources"));}
        if all.values().any(|e|!e.deleted && e.source==source && e.container.is_none() && target.is_none_or(|(id,_)|id!=e.id)) {
            return Err(invalid("This source is already in your reading list. Edit its existing item."));
        }
        let mut e=if let Some((id,revision))=target {
            let e=self.read(id)?;if e.deleted||e.revision!=revision{return Err(conflict());}e
        }else {
            let key=id(source);
            let key=if all.contains_key(&key){id(&token()?)}else{key};
            Entry{id:key,source:source.into(),title:title.into(),saved:now,words:0,progress:0.0,anchor:String::new(),archived:false,schema:1,revision:0,container:None,finished:false,deleted:false,snapshot:None,position:None,capture:None,note:String::new(),extra:BTreeMap::new()}
        };
        if e.source!=source {
            e.extra.entry("original_source".into()).or_insert_with(||serde_json::Value::String(e.source.clone()));
            e.extra.insert("legacy_article_disabled".into(), serde_json::Value::Bool(true));
            e.snapshot=None;e.position=None;e.progress=0.0;e.finished=false;e.words=0;e.anchor.clear();e.note.clear();
        }
        e.source=source.into();e.title=title.trim().into();e.capture=None;
        let changed=e.extra.get("user_notes").and_then(|v|v.as_str()).unwrap_or("")!=notes;
        e.extra.insert("user_notes".into(),serde_json::Value::String(notes.into()));
        if source.starts_with("note:") {
            let bytes=serde_json::to_vec(&serde_json::json!({"title":e.title,"byline":"","when":"","blocks":[{"Para":notes}]})).map_err(invalid_json)?;
            validate_article(&bytes)?;let hash=digest(&bytes);
            atomic(&self.root.join("objects").join(format!("{hash}.article")),&bytes,true)?;
            e.snapshot=Some(hash);e.words=notes.split_whitespace().count();
            if changed{e.position=None;e.progress=0.0;e.anchor.clear();}
        }
        self.write(&mut e)?;Ok(e)
    }

    pub fn state(&self, key: &str, finished: Option<bool>, archived: Option<bool>) -> io::Result<Entry> {
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if e.deleted { return Err(conflict()); }
        if let Some(value) = finished { e.finished = value; }
        if let Some(value) = archived { e.archived = value; }
        self.write(&mut e)?;
        Ok(e)
    }
    pub fn remove(&self, key: &str) -> io::Result<Entry> {
        self.remove_checked(key, None)
    }
    /// Confirmation binds to the revision actually shown, not a cached row or
    /// whichever reader happens to have focus when the user accepts it.
    pub fn remove_at_revision(&self, key: &str, revision: u64) -> io::Result<Entry> {
        self.remove_checked(key, Some(revision))
    }
    fn remove_checked(&self, key: &str, revision: Option<u64>) -> io::Result<Entry> {
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if e.deleted || revision.is_some_and(|r| r != e.revision) { return Err(conflict()); }
        e.deleted = true;
        e.capture = None;
        self.write(&mut e)?;
        Ok(e)
    }
    pub fn undo_remove(&self, key: &str, revision: u64) -> io::Result<Entry> {
        let _lock = self.lock()?;
        let mut e = self.read(key)?;
        if !e.deleted || e.revision != revision { return Err(conflict()); }
        e.deleted = false;
        e.capture = None;
        self.write(&mut e)?;
        Ok(e)
    }
}
/// A deliberately link-only item may open its original on explicit activation.
/// A missing declared snapshot, legacy reading position, corruption, permission
/// error or symlink refusal must NOT silently become network navigation.
/// article() is always tried first, including its legacy <id>.article lookup.
pub fn link_only_fallback(e: &Entry, error: io::ErrorKind) -> bool {
    error == io::ErrorKind::NotFound && !e.deleted && e.snapshot.is_none()
        && e.words == 0 && e.position.is_none() && e.anchor.is_empty() && e.progress == 0.0
}

fn invalid_json(e: serde_json::Error) -> io::Error { invalid(e.to_string()) }
pub fn validate_article(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_TEXT { return Err(invalid("Article exceeds the 2 MiB snapshot limit")); }
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(invalid_json)?;
    for key in ["title", "byline", "when"] {
        if v.get(key).and_then(|v| v.as_str()).is_none_or(|s| s.len() > 8192) { return Err(invalid("Invalid article metadata")); }
    }
    let blocks = v.get("blocks").and_then(|v| v.as_array()).ok_or_else(|| invalid("Missing article blocks"))?;
    if blocks.len() > MAX_BLOCKS { return Err(invalid("Too many article blocks")); }
    for b in blocks {
        let obj = b.as_object().filter(|o| o.len() == 1).ok_or_else(|| invalid("Invalid article block"))?;
        let (kind, val) = obj.iter().next().unwrap();
        let ok = match kind.as_str() {
            "Heading" => val.as_array().is_some_and(|a| a.len() == 2 && a[0].as_u64().is_some_and(|n| (1..=6).contains(&n)) && a[1].is_string()),
            "Image" | "Link" => val.as_array().is_some_and(|a| a.len() == 2 && a.iter().all(|v| v.is_string())),
            "Para" | "Pre" | "Item" | "Quote" | "Caption" => val.is_string(),
            _ => false,
        };
        if !ok { return Err(invalid("Unknown or malformed article block; no content was discarded")); }
    }
    Ok(())
}
/// Only an unambiguous occurrence with matching context restores a passage.
pub fn locate_quote(text: &str, p: &Position) -> Option<usize> {
    if p.quote.is_empty() { return None; }
    let hits: Vec<_> = text.match_indices(&p.quote).filter(|(at, _)| {
        text[..*at].ends_with(&p.before) && text[*at + p.quote.len()..].starts_with(&p.after)
    }).map(|(at, _)| at).collect();
    (hits.len() == 1).then(|| hits[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, Store) {
        // macOS temporary-directory aliases must not turn a test of record
        // safety into a refusal caused by /var -> /private/var.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("library");
        (temp, Store::new(root))
    }
    fn article(body: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"title":"Article","byline":"Author","when":"",
            "blocks":[{"Heading":[2,"Heading"]},{"Para":body},{"Pre":"\tlet x = 1;\n\n"}]})).unwrap()
    }
    fn saved(s: &Store, source: &str) -> Entry {
        s.save_link(source, "Original title", None, 123).unwrap().0
    }
    fn snapshot(s: &Store, e: &Entry, body: &str) -> Entry {
        let pending = s.start_capture(&e.id).unwrap();
        s.commit(&e.id, pending.capture.as_deref().unwrap(), &article(body), "Article", 20, "").unwrap()
    }
    fn position(e: &Entry, fraction: f32) -> Position {
        Position { fraction, snapshot: e.snapshot.clone().unwrap(), quote: "saved text".into(), ..Default::default() }
    }
    fn legacy(s: &Store) -> (Entry, Vec<u8>) {
        fs::create_dir_all(&s.root).unwrap();
        let source = "https://example.test/article?q=1#section";
        let key = id(source);
        let bytes = serde_json::to_vec_pretty(&json!({"id":key,"source":source,"title":"Legacy",
            "saved":100,"words":20,"progress":0.4,"anchor":"saved text","archived":true})).unwrap();
        fs::write(s.path(&key).unwrap(), &bytes).unwrap();
        fs::write(s.root.join(format!("{key}.article")), article("saved text")).unwrap();
        (s.read(&key).unwrap(), bytes)
    }
    #[test]
    fn empty_library_is_read_only_and_empty() {
        let (_t, s) = store();
        assert!(s.list().unwrap().0.is_empty());
        assert!(!s.root.exists());
    }
    #[test]
    fn original_identifier_algorithm_is_retained() {
        assert_eq!(id(""), "cbf29ce48422232584222325cbf29ce4");
        assert_eq!(id("hello"), "a430d84680aabd0b90168a2829807bab");
        assert!(valid_id(&id("Unicode · 日本語")));
    }
    #[test]
    fn legacy_read_does_not_rewrite_or_migrate() {
        let (_t, s) = store(); let (e, bytes) = legacy(&s);
        assert!(e.archived); assert!(!e.finished); assert_eq!(e.schema, 0);
        let (content, hash) = s.article(&e).unwrap();
        assert_eq!(content, article("saved text")); assert_eq!(hash, digest(&content));
        assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), bytes);
        assert!(!s.root.join("objects").exists());
    }
    #[test]
    fn duplicate_legacy_save_preserves_bytes_and_state() {
        let (_t, s) = store(); let (e, bytes) = legacy(&s);
        let (same, created) = s.save_link(&e.source, "Changed page title", None, 999).unwrap();
        assert!(!created); assert_eq!(same, e);
        assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), bytes);
    }
    #[test]
    fn new_snapshot_round_trips_after_service_restart() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "saved text");
        let reopened = Store::new(s.root.clone());
        let e2 = reopened.read(&e.id).unwrap();
        assert_eq!(reopened.article(&e2).unwrap().0, article("saved text"));
    }
    #[test]
    fn duplicate_save_never_resets_read_archive_or_snapshot() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "saved text");
        s.progress(&e.id, &position(&e, 0.7)).unwrap(); s.state(&e.id, Some(true), Some(true)).unwrap();
        let before = fs::read(s.path(&e.id).unwrap()).unwrap();
        let (_, created) = s.save_link(&e.source, "Different", None, 999).unwrap();
        assert!(!created); assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), before);
    }
    #[test]
    fn container_and_exact_source_identity_remain_distinct() {
        let (_t, s) = store(); let source = "https://example.test/app?q=1#one";
        let personal = saved(&s, source);
        let work = s.save_link(source, "Work", Some("Work".into()), 123).unwrap().0;
        assert_ne!(personal.id, work.id); assert_eq!(personal.id, id(source));
        assert_ne!(personal.id, saved(&s, "https://example.test/app?q=2#one").id);
        assert_ne!(personal.id, saved(&s, "https://example.test/app?q=1#two").id);
    }
    #[test]
    fn identity_collision_is_refused_without_replacement() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a");
        let mut other = e.clone(); other.source = "https://example.test/b".into();
        fs::write(s.path(&e.id).unwrap(), serde_json::to_vec(&other).unwrap()).unwrap();
        let before = fs::read(s.path(&e.id).unwrap()).unwrap();
        assert!(s.save_link(&e.source, "A", None, 1).is_err());
        assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), before);
    }
    #[test]
    fn unknown_record_fields_survive_field_specific_mutation() {
        let (_t, s) = store(); let mut e = saved(&s, "https://example.test/a");
        e.extra.insert("client_extension".into(), json!({"value":42})); s.write(&mut e).unwrap();
        let e = s.state(&e.id, None, Some(true)).unwrap();
        assert_eq!(e.extra["client_extension"], json!({"value":42}));
    }
    #[test]
    fn newer_schema_is_not_overwritten() {
        let (_t, s) = store(); let mut e = saved(&s, "https://example.test/a"); e.schema = 99;
        let bytes = serde_json::to_vec(&e).unwrap(); fs::write(s.path(&e.id).unwrap(), &bytes).unwrap();
        assert!(s.state(&e.id, Some(true), None).is_err());
        assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), bytes);
    }
    #[test]
    fn corrupt_record_does_not_erase_other_items() {
        let (_t, s) = store(); let a = saved(&s, "https://example.test/a"); let b = saved(&s, "https://example.test/b");
        fs::write(s.path(&a.id).unwrap(), b"{broken").unwrap();
        let (all, errors) = s.list().unwrap(); assert_eq!(all.len(), 1); assert!(all.contains_key(&b.id)); assert_eq!(errors.len(), 1);
        assert!(s.save_link(&a.source, "Replacement", None, 1).is_err());
        assert_eq!(fs::read(s.path(&a.id).unwrap()).unwrap(), b"{broken");
    }
    #[test]
    fn filename_identity_mismatch_is_rejected() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a");
        let wrong = id("another"); fs::copy(s.path(&e.id).unwrap(), s.path(&wrong).unwrap()).unwrap();
        assert!(s.read(&wrong).is_err());
    }
    #[test]
    fn path_traversal_is_rejected_before_file_access() {
        let (_t, s) = store(); for key in ["", "..", "../settings", "/tmp/x", "ab/../cd"] { assert!(s.read(key).is_err()); }
    }
    #[test]
    fn corrupt_snapshot_hash_does_not_fall_back_to_original_network() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "saved text");
        fs::write(s.root.join("objects").join(format!("{}.article", e.snapshot.as_ref().unwrap())), article("changed")).unwrap();
        assert!(s.article(&e).is_err()); assert_eq!(s.read(&e.id).unwrap(), e);
    }
    #[test]
    fn missing_snapshot_retains_the_link_record() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a");
        assert!(s.article(&e).is_err()); assert_eq!(s.read(&e.id).unwrap(), e);
    }
    #[test]
    fn failed_refresh_retains_previous_saved_copy() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "first");
        let pending = s.start_capture(&e.id).unwrap();
        let after = s.capture_failed(&e.id, pending.capture.as_deref().unwrap(), "Missing article").unwrap();
        assert_eq!(after.snapshot, e.snapshot); assert_eq!(s.article(&after).unwrap().0, article("first"));
    }
    #[test]
    fn new_capture_invalidates_an_older_ticket() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a");
        let first = s.start_capture(&e.id).unwrap(); let next = s.start_capture(&e.id).unwrap();
        assert_ne!(first.capture, next.capture);
        assert!(s.commit(&e.id, first.capture.as_deref().unwrap(), &article("old"), "Old", 1, "").is_err());
        s.commit(&e.id, next.capture.as_deref().unwrap(), &article("new"), "New", 1, "").unwrap();
    }
    #[test]
    fn deletion_cancels_late_capture_and_cannot_resurrect() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a"); let p = s.start_capture(&e.id).unwrap();
        s.remove(&e.id).unwrap();
        assert!(s.commit(&e.id, p.capture.as_deref().unwrap(), &article("late"), "Late", 1, "").is_err());
        assert!(s.read(&e.id).unwrap().deleted);
    }
    #[test]
    fn undo_restores_item_but_not_old_capture_ticket() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a"); let p = s.start_capture(&e.id).unwrap();
        let removed = s.remove(&e.id).unwrap(); let undone = s.undo_remove(&e.id, removed.revision).unwrap();
        assert!(!undone.deleted); assert!(undone.capture.is_none());
        assert!(s.capture_failed(&e.id, p.capture.as_deref().unwrap(), "late").is_err());
    }
    #[test]
    fn stale_undo_cannot_clobber_a_later_removal() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a"); let first = s.remove(&e.id).unwrap();
        s.undo_remove(&e.id, first.revision).unwrap(); s.remove(&e.id).unwrap();
        assert!(s.undo_remove(&e.id, first.revision).is_err()); assert!(s.read(&e.id).unwrap().deleted);
    }
    #[test]
    fn explicit_save_restores_removed_item_after_restart() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "saved text"); s.remove(&e.id).unwrap();
        let restarted = Store::new(s.root.clone());
        let (restored, _) = restarted.save_link(&e.source, "New title", None, 999).unwrap();
        assert!(!restored.deleted); assert_eq!(restored.snapshot, e.snapshot); assert_eq!(restored.saved, e.saved);
    }
    #[test]
    fn completion_and_archive_are_independent() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a");
        let e = s.state(&e.id, None, Some(true)).unwrap(); assert!(!e.finished);
        let e = s.state(&e.id, Some(true), None).unwrap(); assert!(e.archived);
        let e = s.state(&e.id, None, Some(false)).unwrap(); assert!(e.finished);
    }
    #[test]
    fn separate_writers_merge_only_requested_fields() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "saved text");
        let second = Store::new(s.root.clone());
        s.state(&e.id, None, Some(true)).unwrap(); second.progress(&e.id, &position(&e, 0.5)).unwrap();
        let final_e = s.read(&e.id).unwrap(); assert!(final_e.archived); assert_eq!(final_e.progress, 0.5);
    }
    #[test]
    fn old_reader_progress_cannot_overwrite_refreshed_snapshot() {
        let (_t, s) = store(); let old = snapshot(&s, &saved(&s, "https://example.test/a"), "old");
        let new = snapshot(&s, &old, "new");
        assert!(s.progress(&old.id, &position(&old, 0.8)).is_err()); assert_eq!(s.read(&old.id).unwrap(), new);
    }
    #[test]
    fn nonfinite_or_oversized_positions_are_rejected() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "saved text");
        for f in [f32::NAN, f32::INFINITY, -0.1, 1.1] { assert!(s.progress(&e.id, &position(&e, f)).is_err()); }
        let mut p = position(&e, 0.4); p.quote = "x".repeat(4097); assert!(s.progress(&e.id, &p).is_err());
    }
    #[test]
    fn record_failure_keeps_original_reference_and_bytes() {
        let (_t, s) = store(); let e = snapshot(&s, &saved(&s, "https://example.test/a"), "first"); let p = s.start_capture(&e.id).unwrap();
        let before = fs::read(s.path(&e.id).unwrap()).unwrap();
        let mut failed = s.clone(); failed.fail_record_write = true;
        assert!(failed.commit(&e.id, p.capture.as_deref().unwrap(), &article("replacement"), "New", 20, "").is_err());
        assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), before);
        assert_eq!(s.article(&s.read(&e.id).unwrap()).unwrap().0, article("first"));
    }
    #[test]
    fn unavailable_object_directory_keeps_previous_reference() {
        let (_t, s) = store(); let (e, _) = legacy(&s); let p = s.start_capture(&e.id).unwrap();
        fs::write(s.root.join("objects"), b"not a directory").unwrap();
        assert!(s.commit(&e.id, p.capture.as_deref().unwrap(), &article("new"), "New", 20, "").is_err());
        assert_eq!(s.article(&s.read(&e.id).unwrap()).unwrap().0, article("saved text"));
    }
    #[test]
    fn revision_overflow_is_refused_without_touching_disk() {
        let (_t, s) = store(); let mut e = saved(&s, "https://example.test/a"); e.revision = u64::MAX;
        let bytes = serde_json::to_vec(&e).unwrap(); fs::write(s.path(&e.id).unwrap(), &bytes).unwrap();
        assert!(s.state(&e.id, Some(true), None).is_err()); assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), bytes);
    }
    #[test]
    fn writer_lock_fails_closed_without_waiting_or_losing_state() {
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a"); let guard = s.lock().unwrap();
        assert!(s.state(&e.id, Some(true), None).is_err()); assert_eq!(s.read(&e.id).unwrap(), e);
        drop(guard); assert!(s.state(&e.id, Some(true), None).unwrap().finished);
    }
    #[test]
    fn malformed_or_unknown_article_blocks_are_not_silently_discarded() {
        for block in [json!({"Future":"data"}), json!({"Heading":[9,"heading"]}), json!({"Para":99}), json!({"Image":["alt"]})] {
            let bytes = serde_json::to_vec(&json!({"title":"T","byline":"","when":"","blocks":[block]})).unwrap();
            assert!(validate_article(&bytes).is_err());
        }
        assert!(validate_article(&vec![b'x'; MAX_TEXT+1]).is_err());
    }
    #[test]
    fn limits_do_not_truncate_article_structure() {
        let bytes = serde_json::to_vec(&json!({"title":"T","byline":"","when":"","blocks":vec![json!({"Para":"x"});MAX_BLOCKS+1]})).unwrap();
        assert!(validate_article(&bytes).is_err());
        let bytes = serde_json::to_vec(&json!({"title":"x".repeat(8193),"byline":"","when":"","blocks":[]})).unwrap();
        assert!(validate_article(&bytes).is_err());
    }
    #[test]
    fn quote_restore_requires_a_unique_context_match() {
        let p = Position {quote:"same".into(),..Default::default()};
        assert_eq!(locate_quote("same and same", &p), None);
        let p = Position {before:"and ".into(),..p};
        assert_eq!(locate_quote("same and same", &p), Some(9));
        let p = Position {quote:"日本語".into(),before:"a ".into(),after:" b".into(),..Default::default()};
        assert_eq!(locate_quote("a 日本語 b", &p), Some(2));
    }
    #[cfg(unix)]
    #[test]
    fn symlink_record_and_object_are_refused() {
        use std::os::unix::fs::symlink;
        let (_t, s) = store(); let e = saved(&s, "https://example.test/a");
        let target = s.root.join("private-record"); fs::rename(s.path(&e.id).unwrap(), &target).unwrap();
        symlink(&target, s.path(&e.id).unwrap()).unwrap(); assert!(s.read(&e.id).is_err());
        let (_t2, s2) = store(); let e2 = saved(&s2, "https://example.test/b");
        let target = s2.root.join("private-article"); fs::write(&target, article("text")).unwrap();
        symlink(target, s2.root.join(format!("{}.article",e2.id))).unwrap(); assert!(s2.article(&e2).is_err());
    }
}


#[cfg(test)]
mod defaults_and_context_tests {
    use super::*;

    const SITE: &str = "https://cbassuarez.com/nus.dev/";
    fn fixture() -> (tempfile::TempDir, Store) {
        let t = tempfile::tempdir().unwrap();
        let s = Store::new(t.path().canonicalize().unwrap().join("library"));
        (t, s)
    }
    #[test]
    fn defaults_are_ordinary_links_and_idempotent() {
        let (_t, s) = fixture();
        let added = s.seed_links_once(1, &[(SITE, "nus.dev")], 10).unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].id, id(SITE));
        assert_eq!(added[0].source, SITE);
        assert_eq!(added[0].title, "nus.dev");
        assert!(added[0].snapshot.is_none() && added[0].capture.is_none());
        let bytes = fs::read(s.path(&added[0].id).unwrap()).unwrap();
        assert!(s.seed_links_once(1, &[(SITE, "Changed")], 20).unwrap().is_empty());
        assert_eq!(fs::read(s.path(&added[0].id).unwrap()).unwrap(), bytes);
        let (entries, errors) = s.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(errors.is_empty(), "marker was parsed as a reading record");
    }
    #[test]
    fn defaults_preserve_existing_records_and_tombstones_byte_for_byte() {
        for removed in [false, true] {
            let (_t, s) = fixture();
            let e = s.save_link(SITE, "My title", None, 3).unwrap().0;
            s.state(&e.id, Some(true), Some(true)).unwrap();
            if removed { s.remove(&e.id).unwrap(); }
            let before = fs::read(s.path(&e.id).unwrap()).unwrap();
            assert!(s.seed_links_once(1, &[(SITE, "nus.dev")], 100).unwrap().is_empty());
            assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), before);
        }
    }
    #[test]
    fn deletion_and_manual_record_removal_survive_relaunch() {
        let (_t, s) = fixture();
        s.seed_links_once(1, &[(SITE, "nus.dev")], 1).unwrap();
        s.remove(&id(SITE)).unwrap();
        let restarted = Store::new(s.root.clone());
        restarted.seed_links_once(1, &[(SITE, "nus.dev")], 2).unwrap();
        assert!(restarted.read(&id(SITE)).unwrap().deleted);
        fs::remove_file(s.path(&id(SITE)).unwrap()).unwrap();
        restarted.seed_links_once(1, &[(SITE, "nus.dev")], 3).unwrap();
        assert!(restarted.list().unwrap().0.is_empty());
    }
    #[test]
    fn failed_seed_does_not_claim_completion_and_can_retry() {
        let (_t, s) = fixture();
        let mut failing = s.clone();
        failing.fail_record_write = true;
        assert!(failing.seed_links_once(1, &[(SITE, "nus.dev")], 1).is_err());
        assert!(!s.root.join(".defaults-v1").exists());
        assert_eq!(s.seed_links_once(1, &[(SITE, "nus.dev")], 2).unwrap().len(), 1);
    }
    #[test]
    fn interrupted_seed_with_a_written_record_finishes_without_rewriting() {
        let (_t, s) = fixture();
        let e = s.save_link(SITE, "Already committed", None, 1).unwrap().0;
        let bytes = fs::read(s.path(&e.id).unwrap()).unwrap();
        s.seed_links_once(1, &[(SITE, "nus.dev")], 2).unwrap();
        assert_eq!(fs::read(s.path(&e.id).unwrap()).unwrap(), bytes);
        assert_eq!(fs::read(s.root.join(".defaults-v1")).unwrap(), b"complete\n");
    }
    #[test]
    fn busy_seed_uses_the_existing_writer_lock_and_retries_safely() {
        let (_t, s) = fixture();
        let other = Store::new(s.root.clone());
        let guard = s.lock().unwrap();
        assert!(other.seed_links_once(1, &[(SITE, "nus.dev")], 1).is_err());
        assert!(!s.root.join(".defaults-v1").exists());
        drop(guard);
        other.seed_links_once(1, &[(SITE, "nus.dev")], 2).unwrap();
        assert!(s.seed_links_once(1, &[(SITE, "nus.dev")], 3).unwrap().is_empty());
    }
    #[test]
    fn invalid_seed_is_rejected_before_filesystem_access() {
        let (_t, s) = fixture();
        assert!(s.seed_links_once(0, &[(SITE, "nus.dev")], 1).is_err());
        assert!(s.seed_links_once(1, &[("", "empty")], 1).is_err());
        assert!(s.seed_links_once(1, &[(SITE, "x"), (SITE, "y")], 1).is_err());
        assert!(!s.root.exists());
    }
    #[test]
    fn corrupt_defaults_marker_is_not_replaced() {
        let (_t, s) = fixture();
        fs::create_dir_all(&s.root).unwrap();
        fs::write(s.root.join(".defaults-v1"), b"future format").unwrap();
        assert!(s.seed_links_once(1, &[(SITE, "nus.dev")], 1).is_err());
        assert_eq!(fs::read(s.root.join(".defaults-v1")).unwrap(), b"future format");
        assert!(s.list().unwrap().0.is_empty());
    }
    #[test]
    fn corrupted_existing_default_is_never_replaced() {
        let (_t, s) = fixture();
        fs::create_dir_all(&s.root).unwrap();
        fs::write(s.path(&id(SITE)).unwrap(), b"{broken").unwrap();
        assert!(s.seed_links_once(1, &[(SITE, "nus.dev")], 1).is_err());
        assert_eq!(fs::read(s.path(&id(SITE)).unwrap()).unwrap(), b"{broken");
        assert!(!s.root.join(".defaults-v1").exists());
    }
    #[test]
    fn context_mutations_and_guarded_removal_touch_only_the_explicit_id() {
        let (_t, s) = fixture();
        let a = s.save_link("https://example.test/a", "A", None, 1).unwrap().0;
        let b = s.save_link("https://example.test/b", "B", None, 2).unwrap().0;
        let before_a = fs::read(s.path(&a.id).unwrap()).unwrap();
        let updated = s.state(&b.id, Some(true), Some(true)).unwrap();
        assert!(s.remove_at_revision(&b.id, b.revision).is_err());
        assert_eq!(s.read(&b.id).unwrap(), updated);
        let deleted = s.remove_at_revision(&b.id, updated.revision).unwrap();
        assert!(deleted.deleted);
        assert_eq!(fs::read(s.path(&a.id).unwrap()).unwrap(), before_a);
        let undone = s.undo_remove(&b.id, deleted.revision).unwrap();
        assert!(!undone.deleted && undone.finished && undone.archived);
    }
    #[test]
    fn guarded_remove_cancels_capture_and_stale_confirm_cannot_remove_restored_item() {
        let (_t, s) = fixture();
        let e = s.save_link(SITE, "nus", None, 1).unwrap().0;
        let pending = s.start_capture(&e.id).unwrap();
        assert!(s.remove_at_revision(&e.id, e.revision).is_err());
        let removed = s.remove_at_revision(&e.id, pending.revision).unwrap();
        assert!(removed.capture.is_none());
        let restored = s.undo_remove(&e.id, removed.revision).unwrap();
        assert!(s.remove_at_revision(&e.id, pending.revision).is_err());
        assert_eq!(s.read(&e.id).unwrap(), restored);
    }
    #[test]
    fn only_missing_link_only_content_can_fall_back_to_source() {
        let (_t, s) = fixture();
        let e = s.save_link(SITE, "nus", None, 1).unwrap().0;
        assert!(link_only_fallback(&e, io::ErrorKind::NotFound));
        for kind in [io::ErrorKind::InvalidData, io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            assert!(!link_only_fallback(&e, kind));
        }
        let mut old = e.clone(); old.words = 10;
        assert!(!link_only_fallback(&old, io::ErrorKind::NotFound));
        let mut old = e.clone(); old.snapshot = Some("0".repeat(64));
        assert!(!link_only_fallback(&old, io::ErrorKind::NotFound));
        let mut old = e.clone(); old.position = Some(Position::default());
        assert!(!link_only_fallback(&old, io::ErrorKind::NotFound));
        let mut old = e; old.deleted = true;
        assert!(!link_only_fallback(&old, io::ErrorKind::NotFound));
    }
    #[cfg(unix)]
    #[test]
    fn defaults_marker_symlink_is_refused() {
        let (_t, s) = fixture();
        fs::create_dir_all(&s.root).unwrap();
        let target = s.root.join("outside");
        fs::write(&target, b"complete\n").unwrap();
        std::os::unix::fs::symlink(&target, s.root.join(".defaults-v1")).unwrap();
        assert!(s.seed_links_once(1, &[(SITE, "nus.dev")], 1).is_err());
    }
}

#[cfg(test)] mod form_tests {
    use super::*;
    #[test]
    fn changing_a_legacy_source_does_not_show_the_old_article_as_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("library");
        let store = Store::new(root.clone());
        let a = store.save_link("https://example.test/old", "Old", None, 1).unwrap().0;
        let article = br#"{"title":"Old","byline":"","when":"","blocks":[{"Para":"Original article"}]}"#;
        let legacy = root.join(format!("{}.article", a.id));
        std::fs::write(&legacy, article).unwrap();
        assert!(store.article(&a).is_ok());
        let a = store.state(&a.id, Some(true), None).unwrap();
        let b = store.save_item(Some((&a.id,a.revision)), "https://example.test/new", "New", "", 2).unwrap();
        assert!(!b.finished);
        assert_eq!(store.article(&b).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read(legacy).unwrap(), article);
    }
    #[test] fn edit_is_atomic_revision_guarded_and_preserves_identity(){
        let dir=tempfile::tempdir().unwrap();let store=Store::new(dir.path().canonicalize().unwrap().join("library"));
        let a=store.save_item(None,"https://example.test/a","First","remember",1).unwrap();
        let b=store.save_item(Some((&a.id,a.revision)),"https://example.test/b","Edited","new note",2).unwrap();
        assert_eq!(a.id,b.id);assert_eq!(b.extra["user_notes"],"new note");
        assert!(store.save_item(Some((&a.id,a.revision)),"https://example.test/c","stale","lost",3).is_err());
        assert_eq!(store.read(&a.id).unwrap(),b);
        assert_eq!(store.save_link("https://example.test/b","ignored",None,4).unwrap().0.id,b.id);
        assert_ne!(store.save_link("https://example.test/a","new original",None,4).unwrap().0.id,b.id);
        assert!(store.save_item(None,"https://example.test/b","duplicate","",5).is_err());
    }
    #[test] fn note_edits_save_readable_content_and_cancel_old_captures(){
        let dir=tempfile::tempdir().unwrap();let store=Store::new(dir.path().canonicalize().unwrap().join("library"));
        let a=store.save_item(None,"note:first","My note","one two three",1).unwrap();
        assert_eq!(a.words,3);assert!(store.article(&a).is_ok());
        let capturing=store.start_capture(&a.id).unwrap();
        let b=store.save_item(Some((&a.id,capturing.revision)),"note:first","Changed","four five",2).unwrap();
        assert_eq!(b.words,2);assert_ne!(a.snapshot,b.snapshot);assert!(b.capture.is_none());
        assert!(store.commit(&a.id,capturing.capture.as_deref().unwrap(),&store.article(&a).unwrap().0,"Old",3,"").is_err());
    }
}
