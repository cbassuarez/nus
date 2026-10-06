//! Finding notes: an in-memory SQLite FTS5 index of every note in every
//! home "All notes" covers. It is a projection, never a source of truth:
//! it lives in `:memory:` with its temporary storage in memory too, so a
//! personal note's words are never written to disk in the clear, and it
//! is rebuilt from the notes whenever nus starts. If it cannot be built,
//! notes still open and save; search says it is not there.
//!
//! One worker thread reads the notes (skipping files unchanged since it
//! last looked) and writes the index; the UI thread reads it under the
//! same lock, briefly, and never waits for a scan to finish.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::notes_model::{self as model, Document, Kind};
use crate::notes_query::{self as query, Place, Query};
use crate::notes_store::{Home, NoteKey, Scope};

/// A note found: where, what it is called, and why it came up.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub key: NoteKey,
    pub path: PathBuf,
    pub title: String,
    pub home_name: String,
    pub scope: Scope,
    pub modified: u64,
    /// A few words around the match, escaped of nothing: drawn as text.
    pub snippet: String,
    /// "title", "body", "exact path", "same file", …
    pub reason: String,
    /// The line to open at (0-based).
    pub line: usize,
    pub unfiled: bool,
    pub content_sha256: String,
}

/// An open checklist item: its note, its line, the line as written (to
/// tick it only while it still says that) and its words.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub note: Hit,
    pub line: usize,
    pub text: String,
    pub words: String,
}

pub struct Index {
    conn: Connection,
    pub generation: u64,
    homes: HashMap<String, (String, Scope)>,
}

fn scope_word(s: Scope) -> &'static str {
    match s {
        Scope::Folder => "folder",
        Scope::Personal => "personal",
    }
}

/// A page's address for "same page": no fragment, no known tracking
/// parameters, the rest (a query can matter) as it was.
pub fn normalize_url(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url.trim()) else { return url.trim().to_string() };
    u.set_fragment(None);
    let kept: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| {
        let k = k.to_ascii_lowercase();
        !(k.starts_with("utm_") || matches!(k.as_str(), "fbclid" | "gclid" | "mc_cid" | "mc_eid" | "igshid" | "ref_src"))
    }).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if kept.is_empty() {
        u.set_query(None);
    } else {
        u.query_pairs_mut().clear().extend_pairs(kept);
    }
    u.to_string()
}

pub fn lookup_file(home_id: &str, relative: &str) -> String {
    format!("file:{home_id}:{}", relative.replace('\\', "/").trim_start_matches("./"))
}

pub fn lookup_url(url: &str) -> String {
    format!("url:{}", normalize_url(url))
}

pub fn lookup_command(home_id: &str, command: &str) -> String {
    format!("cmd:{home_id}:{}", command.trim())
}

impl Index {
    pub fn open() -> rusqlite::Result<Index> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(include_str!("notes_schema.sql"))?;
        Ok(Index { conn, generation: 0, homes: HashMap::new() })
    }

    /// The database is memory only, its temporary storage and journal
    /// too (checked, not assumed).
    pub fn memory_only(&self) -> bool {
        let files: Vec<String> = self.conn.prepare("PRAGMA database_list").and_then(|mut s| s.query_map([], |r| r.get::<_, String>(2))?.collect()).unwrap_or_default();
        let temp: i64 = self.conn.query_row("PRAGMA temp_store", [], |r| r.get(0)).unwrap_or(0);
        let journal: String = self.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap_or_default();
        files.iter().all(String::is_empty) && temp == 2 && journal == "memory"
    }

    pub fn set_home(&mut self, home: &Home) {
        self.homes.insert(home.id.clone(), (home.name(), home.scope));
    }

    /// Index one note (replacing what was there for its file).
    pub fn put(&mut self, home: &Home, path: &Path, doc: &Document, sha: &str, modified: u64) -> rusqlite::Result<()> {
        self.set_home(home);
        let locator = path.to_string_lossy().into_owned();
        let note_id = doc.id().map(str::to_string).unwrap_or_else(|| crate::notes_store::legacy_id(&home.id, path));
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM note_document WHERE home_id = ?1 AND (locator = ?2 OR note_id = ?3)", params![home.id, locator, note_id])?;
        let (body, complete) = if doc.body.len() > model::MAX_INDEXED {
            let mut end = model::MAX_INDEXED;
            while !doc.body.is_char_boundary(end) {
                end -= 1;
            }
            (&doc.body[..end], false)
        } else {
            (doc.body.as_str(), true)
        };
        let tags = doc.tags();
        let sources = doc.sources();
        let source_text = sources.iter().map(|s| s.search_text()).collect::<Vec<_>>().join("\n");
        tx.execute(
            "INSERT INTO note_document(note_id, home_id, scope, locator, content_sha256, document_revision, generation, title, body, tags_text, source_text, modified_ms, deleted, filed, index_complete)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![note_id, home.id, scope_word(home.scope), locator, sha, doc.revision().max(1) as i64, self.generation as i64, doc.title(), body, tags.join(" "), source_text, (modified * 1000) as i64, doc.deleted_at().is_some() as i64, doc.filed() as i64, complete as i64],
        )?;
        for t in &tags {
            tx.execute("INSERT OR IGNORE INTO note_tag VALUES (?1, ?2, ?3, ?4)", params![home.id, note_id, t, t.to_lowercase()])?;
        }
        for l in model::literals(doc) {
            tx.execute("INSERT OR IGNORE INTO note_literal VALUES (?1, ?2, ?3, ?4, ?5)", params![home.id, note_id, l.kind, l.value, l.value])?;
        }
        let mut lookups: Vec<(String, &str)> = Vec::new();
        for s in &sources {
            let kind = s.kind();
            if kind == Kind::Other || !model::valid_id(s.id()) {
                continue;
            }
            let at = s.captured_at().unwrap_or(0) as i64 * 1000;
            let json = serde_json::Value::Object(s.0.clone()).to_string();
            tx.execute(
                "INSERT OR IGNORE INTO note_source(source_id, home_id, note_id, kind, target_note_id, target_home_id, browser_profile_id, browser_container_id, snapshot_sha256, source_json, label, captured_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![s.id(), home.id, note_id, kind.word(), s.get("target_note_id"), s.get("target_home_id"), s.get("browser_profile_id"), s.get("browser_container_id"), s.get("snapshot_sha256").filter(|h| h.len() == 64), json, s.label(), at],
            )?;
            match kind {
                Kind::File => if let Some(r) = s.get("relative_path") { lookups.push((lookup_file(&home.id, r), "same file")) },
                Kind::Web => if let Some(u) = s.get("url") { lookups.push((lookup_url(u), "same page")) },
                Kind::Reading => {
                    if let Some(u) = s.get("source_url") { lookups.push((lookup_url(u), "same saved article")) }
                    if let Some(id) = s.get("library_id") { lookups.push((format!("reading:{id}"), "same saved article")) }
                }
                Kind::Terminal => if let Some(c) = s.get("command") { lookups.push((lookup_command(&home.id, c), "same command in this project")) },
                _ => {}
            }
        }
        // Written links to other notes: the only backlinks there are.
        for l in doc.links() {
            let target_home = l.home_id.clone().unwrap_or_else(|| home.id.clone());
            let sid = &model::sha256(format!("{}:{}:{}:{}", home.id, note_id, target_home, l.line).as_bytes())[..32];
            let json = serde_json::json!({"line": l.line}).to_string();
            tx.execute(
                "INSERT OR IGNORE INTO note_source(source_id, home_id, note_id, kind, target_note_id, target_home_id, source_json, label, captured_ms) VALUES (?1, ?2, ?3, 'note', ?4, ?5, ?6, ?7, 0)",
                params![sid, home.id, note_id, l.note_id, target_home, json, l.label],
            )?;
        }
        for (k, why) in lookups {
            tx.execute("INSERT OR IGNORE INTO note_lookup VALUES (?1, ?2, ?3, ?4)", params![home.id, note_id, k, why])?;
        }
        tx.commit()?;
        self.generation += 1;
        Ok(())
    }

    pub fn remove_path(&mut self, home_id: &str, path: &Path) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM note_document WHERE home_id = ?1 AND locator = ?2", params![home_id, path.to_string_lossy()])?;
        self.generation += 1;
        Ok(())
    }

    /// Forget a home entirely (a locked vault's, a project that went).
    pub fn clear_home(&mut self, home_id: &str) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM note_document WHERE home_id = ?1", params![home_id])?;
        self.homes.remove(home_id);
        self.generation += 1;
        Ok(())
    }

    pub fn paths(&self, home_id: &str) -> Vec<PathBuf> {
        self.conn.prepare("SELECT locator FROM note_document WHERE home_id = ?1")
            .and_then(|mut s| s.query_map(params![home_id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<String>>>())
            .unwrap_or_default().into_iter().map(PathBuf::from).collect()
    }

    fn hit(&self, r: &rusqlite::Row) -> rusqlite::Result<Hit> {
        let home_id: String = r.get("home_id")?;
        let (home_name, scope) = self.homes.get(&home_id).cloned().unwrap_or_else(|| ("?".into(), Scope::Folder));
        Ok(Hit {
            key: NoteKey { home_id, note_id: r.get("note_id")? },
            path: PathBuf::from(r.get::<_, String>("locator")?),
            title: r.get("title")?,
            home_name,
            scope,
            modified: (r.get::<_, i64>("modified_ms")? / 1000) as u64,
            snippet: String::new(),
            reason: String::new(),
            line: 0,
            unfiled: r.get::<_, i64>("filed")? == 0,
            content_sha256: r.get("content_sha256")?,
        })
    }

    fn home_clause(homes: Option<&[String]>) -> String {
        match homes {
            None => String::new(),
            Some(ids) => format!(" AND d.home_id IN ({})", ids.iter().map(|i| format!("'{}'", i.replace('\'', ""))).collect::<Vec<_>>().join(",")),
        }
    }

    /// Newest first, trash left out.
    pub fn recent(&self, homes: Option<&[String]>, limit: usize) -> Vec<Hit> {
        let sql = format!("SELECT d.* FROM note_document d WHERE d.deleted = 0{} ORDER BY d.modified_ms DESC, d.home_id, d.note_id LIMIT {limit}", Self::home_clause(homes));
        self.rows(&sql, &[])
    }

    /// Open checklist items in these homes' notes, newest note first.
    pub fn tasks(&self, homes: &[String], limit: usize) -> Vec<Task> {
        let sql = format!("SELECT d.* FROM note_document d WHERE d.deleted = 0{} ORDER BY d.modified_ms DESC, d.home_id, d.note_id", Self::home_clause(Some(homes)));
        let Ok(mut st) = self.conn.prepare(&sql) else { return Vec::new() };
        let notes: Vec<(Hit, String)> = st.query_map([], |r| Ok((self.hit(r)?, r.get::<_, String>("body")?))).map(|it| it.flatten().collect()).unwrap_or_default();
        let mut out = Vec::new();
        for (hit, body) in notes {
            for (line, text, words) in crate::notes_format::open_tasks(&body) {
                if out.len() >= limit {
                    return out;
                }
                out.push(Task { note: hit.clone(), line, text, words });
            }
        }
        out
    }

    pub fn trashed(&self, limit: usize) -> Vec<Hit> {
        self.rows(&format!("SELECT d.* FROM note_document d WHERE d.deleted = 1 ORDER BY d.modified_ms DESC LIMIT {limit}"), &[])
    }

    fn rows(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Vec<Hit> {
        let Ok(mut st) = self.conn.prepare(sql) else { return Vec::new() };
        st.query_map(args, |r| self.hit(r)).map(|it| it.flatten().collect()).unwrap_or_default()
    }

    /// Notes that link to this one, with the line each link is on.
    pub fn backlinks(&self, key: &NoteKey) -> Vec<Hit> {
        let Ok(mut st) = self.conn.prepare(
            "SELECT d.*, s.source_json FROM note_source s JOIN note_document d ON d.home_id = s.home_id AND d.note_id = s.note_id
             WHERE s.kind = 'note' AND s.target_home_id = ?1 AND s.target_note_id = ?2 AND d.deleted = 0 ORDER BY d.modified_ms DESC",
        ) else { return Vec::new() };
        st.query_map(params![key.home_id, key.note_id], |r| {
            let mut h = self.hit(r)?;
            let json: String = r.get("source_json")?;
            h.line = serde_json::from_str::<serde_json::Value>(&json).ok().and_then(|v| v.get("line")?.as_u64()).unwrap_or(0) as usize;
            h.reason = "links here".into();
            Ok(h)
        }).map(|it| it.flatten().collect()).unwrap_or_default()
    }

    /// Notes whose sources are exactly this file, page or command.
    pub fn about(&self, lookups: &[String]) -> Vec<Hit> {
        let mut out: Vec<Hit> = Vec::new();
        for l in lookups {
            let Ok(mut st) = self.conn.prepare("SELECT d.*, k.reason FROM note_lookup k JOIN note_document d ON d.home_id = k.home_id AND d.note_id = k.note_id WHERE k.lookup = ?1 AND d.deleted = 0 ORDER BY d.modified_ms DESC") else { continue };
            let found: Vec<Hit> = st.query_map(params![l], |r| {
                let mut h = self.hit(r)?;
                h.reason = r.get("reason")?;
                Ok(h)
            }).map(|it| it.flatten().collect()).unwrap_or_default();
            for h in found {
                if !out.iter().any(|o| o.key == h.key) {
                    out.push(h);
                }
            }
        }
        out
    }

    pub fn title_of(&self, path: &Path) -> Option<String> {
        self.conn.query_row("SELECT title FROM note_document WHERE locator = ?1", params![path.to_string_lossy()], |r| r.get(0)).optional().ok().flatten()
    }

    /// The home ids a place names, or why it names none.
    pub fn resolve(&self, place: &Place, here: Option<&str>) -> Result<Option<Vec<String>>, String> {
        match place {
            Place::All => Ok(None),
            Place::Here => Ok(here.map(|h| vec![h.to_string()])),
            Place::Personal => Ok(Some(self.homes.iter().filter(|(_, (_, s))| *s == Scope::Personal).map(|(id, _)| id.clone()).collect())),
            Place::Named(n) => {
                let ids: Vec<String> = self.homes.iter().filter(|(_, (name, _))| name.eq_ignore_ascii_case(n)).map(|(id, _)| id.clone()).collect();
                match ids.len() {
                    0 => Err(format!("in:{n} · no project by that name has notes here")),
                    1 => Ok(Some(ids)),
                    k => Err(format!("in:{n} · {k} projects have that name · open one and search it")),
                }
            }
        }
    }

    /// Search. Filters, exact values and words must all hold; words rank
    /// by where they matched (a title first), then by how recent.
    pub fn search(&self, q: &Query, homes: Option<&[String]>, limit: usize) -> Result<Vec<Hit>, String> {
        if let Some(p) = q.problems.first() {
            return Err(p.clone());
        }
        let mut sql = String::from("SELECT d.*");
        let fts = q.fts();
        if fts.is_some() {
            sql.push_str(", snippet(note_fts, 1, '', '', '…', 10) AS snip FROM note_fts JOIN note_document d ON d.row_id = note_fts.rowid WHERE note_fts MATCH ?1 AND d.deleted = 0");
        } else {
            sql.push_str(", '' AS snip FROM note_document d WHERE d.deleted = 0 AND ?1 IS NULL");
        }
        sql.push_str(&Self::home_clause(homes));
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts.clone())];
        let mut n = 2;
        let mut push = |sql: &mut String, clause: &str, v: Box<dyn rusqlite::ToSql>| {
            sql.push_str(&clause.replace("?N", &format!("?{n}")));
            args.push(v);
            n += 1;
        };
        for l in &q.literals {
            push(&mut sql, " AND EXISTS (SELECT 1 FROM note_literal l WHERE l.home_id = d.home_id AND l.note_id = d.note_id AND l.normalized_value = ?N", Box::new(l.value.clone()));
            push(&mut sql, " AND l.kind = ?N)", Box::new(l.kind.to_string()));
        }
        for t in &q.quoted {
            push(&mut sql, " AND (instr(lower(d.title), lower(?N)) > 0", Box::new(t.clone()));
            push(&mut sql, " OR instr(lower(d.body), lower(?N)) > 0)", Box::new(t.clone()));
        }
        for t in &q.tags {
            push(&mut sql, " AND EXISTS (SELECT 1 FROM note_tag t WHERE t.home_id = d.home_id AND t.note_id = d.note_id AND t.normalized_tag = ?N)", Box::new(t.clone()));
        }
        for s in &q.sources {
            push(&mut sql, " AND EXISTS (SELECT 1 FROM note_source s WHERE s.home_id = d.home_id AND s.note_id = d.note_id AND s.kind = ?N)", Box::new(s.clone()));
        }
        if let Some(b) = q.before {
            push(&mut sql, " AND d.modified_ms < ?N", Box::new((b * 1000) as i64));
        }
        if let Some(a) = q.after {
            push(&mut sql, " AND d.modified_ms > ?N", Box::new((a * 1000) as i64));
        }
        if q.unfiled {
            sql.push_str(" AND d.filed = 0");
        }
        if fts.is_some() {
            sql.push_str(" ORDER BY bm25(note_fts, 8.0, 1.0, 4.0, 2.0), d.modified_ms DESC, d.home_id, d.note_id");
        } else {
            sql.push_str(" ORDER BY d.modified_ms DESC, d.home_id, d.note_id");
        }
        sql.push_str(&format!(" LIMIT {limit}"));
        let mut st = self.conn.prepare(&sql).map_err(|e| format!("search could not run: {e}"))?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let rows = st.query_map(refs.as_slice(), |r| {
            let mut h = self.hit(r)?;
            h.snippet = r.get::<_, String>("snip")?.replace(['\n', '\r'], " ");
            let body: String = r.get("body")?;
            let tags: String = r.get("tags_text")?;
            let source: String = r.get("source_text")?;
            let (reason, needle) = reason(q, &h.title, &tags, &source);
            h.reason = reason;
            if let Some(needle) = needle {
                let lower = needle.to_lowercase();
                h.line = body.lines().position(|l| l.to_lowercase().contains(&lower)).unwrap_or(0);
                if h.snippet.is_empty() {
                    h.snippet = body.lines().nth(h.line).unwrap_or("").trim().chars().take(80).collect();
                }
            }
            Ok(h)
        }).map_err(|e| format!("search could not run: {e}"))?;
        let mut out: Vec<Hit> = rows.flatten().collect();
        // A note whose title is exactly what you typed comes first.
        let whole = q.words.join(" ").to_lowercase();
        if !whole.is_empty() {
            out.sort_by_key(|h| h.title.to_lowercase() != whole);
        }
        Ok(out)
    }
}

/// Why a note matched, in a word or two, and the text to look for in its
/// body to find the line.
fn reason(q: &Query, title: &str, tags: &str, source: &str) -> (String, Option<String>) {
    if let Some(l) = q.literals.first() {
        let what = match l.kind { "flag" => "exact flag", "path" => "exact path", "url" => "exact URL", _ => "exact text" };
        return (what.into(), Some(l.value.clone()));
    }
    if let Some(t) = q.quoted.first() {
        return ("exact text".into(), Some(t.clone()));
    }
    let Some(w) = q.words.first() else { return (String::new(), None) };
    let w = w.to_lowercase();
    let why = if title.to_lowercase().contains(&w) {
        "title"
    } else if tags.to_lowercase().contains(&w) {
        "tag"
    } else if source.to_lowercase().contains(&w) {
        "source"
    } else {
        "body"
    };
    (why.into(), Some(w))
}

// --- the app's one index ---------------------------------------------------

enum Msg {
    Scan(Vec<Home>),
    Refresh(Home, PathBuf),
}

struct Global {
    index: Option<Index>,
    /// What could not be read, per location name.
    missing: Vec<String>,
    scanning: bool,
    error: Option<String>,
    last_scan: Option<Instant>,
    /// Files as last read: (modified, size), to skip unchanged ones.
    seen: HashMap<PathBuf, (u64, u64)>,
}

static GLOBAL: OnceLock<Mutex<Global>> = OnceLock::new();
static WORKER: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
/// Scans at most this often on their own (notes nus saved itself are
/// indexed as they save).
const RESCAN: Duration = Duration::from_secs(60);

fn global() -> &'static Mutex<Global> {
    GLOBAL.get_or_init(|| {
        let (index, error) = match Index::open() {
            Ok(i) if i.memory_only() => (Some(i), None),
            Ok(_) => (None, Some("search needs an in-memory database and could not get one".into())),
            Err(e) => (None, Some(format!("search is unavailable: {e}"))),
        };
        Mutex::new(Global { index, missing: Vec::new(), scanning: false, error, last_scan: None, seen: HashMap::new() })
    })
}

fn worker() -> Sender<Msg> {
    WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        std::thread::Builder::new().name("nus-notes-index".into()).spawn(move || {
            for msg in rx {
                match msg {
                    Msg::Scan(homes) => scan(&homes),
                    Msg::Refresh(home, path) => read_one(&home, &path, true),
                }
            }
        }).expect("the notes index thread starts");
        Mutex::new(tx)
    }).lock().map(|t| t.clone()).unwrap_or_else(|e| e.into_inner().clone())
}

fn scan(homes: &[Home]) {
    for home in homes {
        let files: Vec<PathBuf> = std::fs::read_dir(&home.root).into_iter().flatten().flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        for p in &files {
            read_one(home, p, false);
        }
        // Files gone since: out of the index (a missing project is not a
        // deleted note; only this home's own listing says so).
        if let Ok(mut g) = global().lock() {
            let gone: Vec<PathBuf> = g.index.as_ref().map(|i| i.paths(&home.id)).unwrap_or_default().into_iter().filter(|p| !files.contains(p)).collect();
            for p in gone {
                if let Some(i) = g.index.as_mut() {
                    let _ = i.remove_path(&home.id, &p);
                }
                g.seen.remove(&p);
            }
        }
    }
    if let Ok(mut g) = global().lock() {
        g.scanning = false;
    }
}

fn read_one(home: &Home, path: &Path, force: bool) {
    let meta = std::fs::metadata(path).ok();
    let stamp = meta.as_ref().map(|m| (m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()), m.len()));
    if !force {
        if let (Some(s), Ok(g)) = (stamp, global().lock()) {
            if g.seen.get(path) == Some(&s) {
                return;
            }
        }
    }
    let Some(stamp) = stamp else {
        if let Ok(mut g) = global().lock() {
            if let Some(i) = g.index.as_mut() {
                let _ = i.remove_path(&home.id, path);
            }
        }
        return;
    };
    let Ok(bytes) = home.read_file(path) else { return };
    let sha = model::sha256(&bytes);
    let doc = match Document::parse(&bytes) {
        Ok(d) => d,
        // A note this build cannot read is still findable by its words.
        Err(_) => Document::plain(&String::from_utf8_lossy(&bytes)),
    };
    if let Ok(mut g) = global().lock() {
        if let Some(i) = g.index.as_mut() {
            if let Err(e) = i.put(home, path, &doc, &sha, stamp.0) {
                tracing::warn!("notes index: {e}");
            }
        }
        g.seen.insert(path.to_path_buf(), stamp);
    }
}

/// Start (or refresh, once a minute) the index of every home.
pub fn ensure_started() {
    let due = global().lock().map(|g| !g.scanning && g.index.is_some() && g.last_scan.is_none_or(|t| t.elapsed() > RESCAN)).unwrap_or(false);
    if !due || crate::private::enabled() {
        return;
    }
    let (homes, missing) = crate::notes_session::homes();
    let personal_locked = missing.iter().any(|(_, e)| matches!(e, crate::notes_store::NoteError::Locked));
    if let Ok(mut g) = global().lock() {
        // Locked personal notes lend nothing to search: no titles, no
        // counts, no snippets.
        if personal_locked {
            if let Some(i) = g.index.as_mut() {
                let ids: Vec<String> = i.homes.iter().filter(|(_, (_, s))| *s == Scope::Personal).map(|(id, _)| id.clone()).collect();
                for id in ids {
                    let _ = i.clear_home(&id);
                }
            }
        }
        g.scanning = true;
        g.last_scan = Some(Instant::now());
        g.missing = missing.into_iter().map(|(name, e)| format!("{name} · {e}")).collect();
        if let Some(i) = g.index.as_mut() {
            for h in &homes {
                i.set_home(h);
            }
        }
    }
    let _ = worker().send(Msg::Scan(homes));
}

/// Notes that were just saved: read them again.
pub fn refresh(committed: Vec<crate::notes_session::Committed>) {
    for c in committed {
        let _ = worker().send(Msg::Refresh(c.home, c.path));
    }
}

/// Moves on whenever anything in the index changes.
pub fn generation() -> u64 {
    global().lock().ok().and_then(|g| g.index.as_ref().map(|i| i.generation)).unwrap_or(0)
}

fn read<R>(f: impl FnOnce(&Index) -> R) -> Option<R> {
    // A scan writing a large note holds the lock for a moment: the UI
    // does not wait for it.
    let g = global().try_lock().ok()?;
    g.index.as_ref().map(f)
}

pub fn recent(home_id: Option<&str>, limit: usize) -> Vec<Hit> {
    let ids = home_id.map(|h| vec![h.to_string()]);
    read(|i| i.recent(ids.as_deref(), limit)).unwrap_or_default()
}

pub fn recent_personal(limit: usize) -> Vec<Hit> {
    read(|i| {
        let ids: Vec<String> = i.homes.iter().filter(|(_, (_, s))| *s == Scope::Personal).map(|(id, _)| id.clone()).collect();
        if ids.is_empty() { Vec::new() } else { i.recent(Some(&ids), limit) }
    }).unwrap_or_default()
}

pub fn trashed(limit: usize) -> Vec<Hit> {
    read(|i| i.trashed(limit)).unwrap_or_default()
}

/// Open tasks in this project's notes (when there is one) and the
/// personal ones.
pub fn tasks(here: Option<&str>, limit: usize) -> Vec<Task> {
    read(|i| {
        let mut ids: Vec<String> = i.homes.iter().filter(|(_, (_, s))| *s == Scope::Personal).map(|(id, _)| id.clone()).collect();
        ids.extend(here.map(str::to_string));
        if ids.is_empty() { Vec::new() } else { i.tasks(&ids, limit) }
    }).unwrap_or_default()
}

pub fn backlinks(key: &NoteKey) -> Vec<Hit> {
    read(|i| i.backlinks(key)).unwrap_or_default()
}

pub fn about(lookups: &[String]) -> Vec<Hit> {
    read(|i| i.about(lookups)).unwrap_or_default()
}

/// Where a note is, by its identity (its file may have been renamed).
pub fn path_of(key: &NoteKey) -> Option<PathBuf> {
    read(|i| i.conn.query_row("SELECT locator FROM note_document WHERE home_id = ?1 AND note_id = ?2", params![key.home_id, key.note_id], |r| r.get::<_, String>(0)).optional().ok().flatten()).flatten().map(PathBuf::from)
}

/// A note's title and body as last indexed, by its identity.
pub fn body_of(key: &NoteKey) -> Option<(String, String)> {
    read(|i| i.conn.query_row("SELECT title, body FROM note_document WHERE home_id = ?1 AND note_id = ?2 AND deleted = 0", params![key.home_id, key.note_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).optional().ok().flatten()).flatten()
}

pub fn title_of(path: &Path) -> Option<String> {
    read(|i| i.title_of(path)).flatten()
}

/// Search from the palette: the project you are in unless `in:` says
/// otherwise.
pub fn search(text: &str, here: Option<&str>, limit: usize) -> Result<Vec<Hit>, String> {
    let q = query::parse(text);
    if q.is_empty() && q.problems.is_empty() {
        return Ok(Vec::new());
    }
    let g = global().try_lock().map_err(|_| "searching available notes…".to_string())?;
    let Some(i) = g.index.as_ref() else { return Err(g.error.clone().unwrap_or_else(|| "search is rebuilding · your notes are safe".into())) };
    let place = q.place.clone().unwrap_or(if here.is_some() { Place::Here } else { Place::All });
    let homes = i.resolve(&place, here)?;
    i.search(&q, homes.as_deref(), limit)
}

/// A line about the index when there is something to say: loading, not
/// there, or locations that could not be read.
pub fn status_line() -> Option<String> {
    let g = global().try_lock().ok()?;
    if let Some(e) = &g.error {
        return Some(format!("{e} · notes still open and save"));
    }
    if g.scanning {
        return Some("searching available notes…".into());
    }
    match g.missing.len() {
        0 => None,
        1 => Some(format!("unavailable: {}", g.missing[0])),
        n => Some(format!("{n} note locations unavailable")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes_model::Source;
    use serde_json::json;

    fn home(id: &str, scope: Scope, name: &str) -> Home {
        Home { id: id.repeat(32 / id.len()), scope, root: PathBuf::from(format!("/n/{name}")), project: (scope == Scope::Folder).then(|| PathBuf::from(format!("/w/{name}"))) }
    }

    fn doc(id: &str, title: &str, body: &str) -> Document {
        let mut d = Document::new(&id.repeat(32 / id.len()), title, true, 0);
        d.body = body.into();
        d
    }

    fn ids(v: &[Hit]) -> Vec<String> {
        v.iter().map(|h| h.key.note_id[..1].to_string()).collect()
    }

    fn find(i: &Index, q: &str) -> Vec<Hit> {
        i.search(&query::parse(q), None, 20).unwrap()
    }

    fn fixture() -> (Index, Home, Home) {
        let mut i = Index::open().unwrap();
        assert!(i.memory_only());
        let p = home("a", Scope::Personal, "personal");
        let f = home("b", Scope::Folder, "nus");
        let mut d1 = doc("1", "Café launch", "quartz body-only discovery");
        d1.set_tags(&["research".into()]);
        d1.push_source(Source::new(Kind::Terminal, &"c".repeat(32), "terminal reflow", 0).with("command", json!("git merge --no-ff src/a-b.rs")));
        i.put(&p, Path::new("/n/personal/1.md"), &d1, &"0".repeat(64), 2).unwrap();
        let mut d2 = doc("2", "Reflow notes", "alpha OR beta is written literally\nsee [the launch](note:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/11111111111111111111111111111111)\n");
        d2.push_source(Source::new(Kind::Web, &"d".repeat(32), "captured reference", 0).with("url", json!("https://example.com/?x=1&utm_source=z#head")));
        i.put(&f, Path::new("/n/nus/2.md"), &d2, &"0".repeat(64), 1).unwrap();
        (i, p, f)
    }

    #[test]
    fn finds_by_body_title_tag_and_source() {
        let (i, ..) = fixture();
        assert_eq!(ids(&find(&i, "quartz")), vec!["1"]);
        assert_eq!(ids(&find(&i, "cafe")), vec!["1"], "diacritics fold");
        assert_eq!(ids(&find(&i, "research")), vec!["1"]);
        assert_eq!(ids(&find(&i, "reference")), vec!["2"]);
        assert_eq!(find(&i, "quartz")[0].reason, "body");
        assert_eq!(find(&i, "launch")[0].reason, "title");
        assert_eq!(ids(&find(&i, "disc")), vec!["1"], "the last word is a prefix while typing");
    }

    #[test]
    fn words_are_words_not_syntax() {
        let (i, ..) = fixture();
        assert_eq!(ids(&find(&i, "alpha OR beta")), vec!["2"]);
        assert!(find(&i, "alpha\" OR quartz").is_empty());
    }

    #[test]
    fn exact_values_keep_their_punctuation() {
        let (i, ..) = fixture();
        assert_eq!(ids(&find(&i, "--no-ff")), vec!["1"]);
        assert!(find(&i, "--ff-only").is_empty());
        assert!(find(&i, "--n").is_empty());
        assert_eq!(ids(&find(&i, "src/a-b.rs")), vec!["1"]);
        assert!(find(&i, "src/a/b.rs").is_empty());
        assert_eq!(find(&i, "--no-ff")[0].reason, "exact flag");
        assert_eq!(ids(&find(&i, "\"OR beta is\"")), vec!["2"]);
    }

    #[test]
    fn filters_and_places() {
        let (i, p, f) = fixture();
        let personal = i.resolve(&Place::Personal, None).unwrap();
        assert_eq!(i.search(&query::parse("quartz"), personal.as_deref(), 9).unwrap().len(), 1);
        let nus = i.resolve(&Place::Named("NUS".into()), None).unwrap();
        assert!(i.search(&query::parse("quartz"), nus.as_deref(), 9).unwrap().is_empty());
        assert!(i.resolve(&Place::Named("elsewhere".into()), None).is_err());
        assert_eq!(ids(&find(&i, "tag:research")), vec!["1"]);
        assert_eq!(ids(&find(&i, "source:web")), vec!["2"]);
        assert!(i.search(&query::parse("color:red"), None, 9).is_err());
        let _ = (p, f);
    }

    #[test]
    fn edits_trash_and_clearing_a_home_leave_nothing_stale() {
        let (mut i, p, _) = fixture();
        let mut d = doc("1", "Café launch", "replacement body");
        i.put(&p, Path::new("/n/personal/1.md"), &d, &"1".repeat(64), 3).unwrap();
        assert!(find(&i, "quartz").is_empty());
        assert_eq!(ids(&find(&i, "replacement")), vec!["1"]);
        d.set_deleted(Some(5));
        i.put(&p, Path::new("/n/personal/1.md"), &d, &"2".repeat(64), 4).unwrap();
        assert!(find(&i, "replacement").is_empty());
        assert_eq!(i.trashed(9).len(), 1);
        i.clear_home(&p.id).unwrap();
        assert!(i.trashed(9).is_empty());
        let n: i64 = i.conn.query_row("SELECT count(*) FROM note_source WHERE home_id = ?1", params![p.id], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        i.conn.execute("INSERT INTO note_fts(note_fts) VALUES('integrity-check')", []).unwrap();
    }

    #[test]
    fn backlinks_are_written_links_across_homes() {
        let (i, p, _) = fixture();
        let key = NoteKey { home_id: p.id.clone(), note_id: "1".repeat(32) };
        let b = i.backlinks(&key);
        assert_eq!(ids(&b), vec!["2"]);
        assert_eq!(b[0].line, 1);
        let other = NoteKey { home_id: "e".repeat(32), note_id: "1".repeat(32) };
        assert!(i.backlinks(&other).is_empty(), "a copied home's note is another note");
    }

    #[test]
    fn notes_here_by_exact_source() {
        let (i, _, f) = fixture();
        let found = i.about(&[lookup_url("https://example.com/?x=1#other")]);
        assert_eq!(ids(&found), vec!["2"]);
        assert_eq!(found[0].reason, "same page");
        assert!(i.about(&[lookup_url("https://example.com/?x=2")]).is_empty(), "a meaningful query is kept");
        assert!(i.about(&[lookup_command(&f.id, "git merge --no-ff src/a-b.rs")]).is_empty(), "the command's note is personal, not this project's");
        let p = i.homes.iter().find(|(_, (_, s))| *s == Scope::Personal).map(|(id, _)| id.clone()).unwrap();
        assert_eq!(ids(&i.about(&[lookup_command(&p, "git merge --no-ff src/a-b.rs")])), vec!["1"]);
    }

    #[test]
    fn same_note_id_in_two_homes_is_two_notes() {
        let (mut i, _, f) = fixture();
        let g = home("f", Scope::Folder, "copy");
        let d = doc("2", "Copied", "same copied document");
        i.put(&g, Path::new("/n/copy/2.md"), &d, &"3".repeat(64), 1).unwrap();
        let hits = find(&i, "copied");
        assert_eq!(hits.len(), 1);
        assert_eq!(ids(&find(&i, "literally")), vec!["2"], "the original is still there");
        assert_ne!(hits[0].key, i.recent(Some(&[f.id.clone()]), 1)[0].key);
    }

    #[test]
    fn tracking_parameters_and_fragments_do_not_make_another_page() {
        assert_eq!(normalize_url("https://x.org/a?b=1&utm_source=q#f"), "https://x.org/a?b=1");
        assert_eq!(normalize_url("https://x.org/a?fbclid=1"), "https://x.org/a");
    }
}
