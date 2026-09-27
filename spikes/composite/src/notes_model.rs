//! What a note IS, apart from where it lives (notes_store.rs) and who is
//! editing it (notes_session.rs). A note is Markdown with an optional
//! header: a strict JSON object between `---` lines, whose `nus` member is
//! ours and whose other members are whoever else's, kept as found.
//!
//! ```text
//! ---
//! {"title": "Why the prompt wraps", "tags": [], "nus": {"schema": 1, "id": "…", …}}
//! ---
//! The body, exactly as written.
//! ```
//!
//! A file without that header is a LEGACY note (every note before this
//! format): it reads as it always did, its title from its first heading,
//! and nothing here rewrites it until it is migrated on purpose.
//!
//! A captured source is a record in `nus.sources` bound to the fence or
//! quote right under a `<!-- nus:source <id> -->` line. The record keeps
//! what was captured and its hash, so an edited excerpt shows as edited
//! instead of passing for the original. Nothing here does I/O.

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA: u64 = 1;
/// Text indexed per note; a longer body stays whole on disk and in the
/// editor, and search says it stopped.
pub const MAX_INDEXED: usize = 2 * 1024 * 1024;
/// The header's size and counts, bounded apart from the body.
pub const MAX_META: usize = 64 * 1024;
pub const MAX_TAGS: usize = 256;
pub const MAX_SOURCES: usize = 2000;

/// A 128-bit random id as 32 lowercase hex digits.
pub fn new_id() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(hex(&b))
}

pub fn valid_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of exact bytes: a note's content identity.
pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// SHA-256 of captured text, CRLF read as LF and nothing else changed.
pub fn captured_sha256(text: &str) -> String {
    sha256(text.replace("\r\n", "\n").as_bytes())
}

/// `2026-09-26T18:00:00Z`.
pub fn rfc3339(secs: u64) -> String {
    let (y, mo, d, h, mi) = crate::notes::civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{:02}Z", secs % 60)
}

/// Seconds since the epoch for `rfc3339`'s shape (and a bare date).
pub fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes().get(4) != Some(&b'-') || s.as_bytes().get(7) != Some(&b'-') || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let (h, mi, sec) = if s.len() > 10 { (num(11, 13)?, num(14, 16)?, num(17, 19).unwrap_or(0)) } else { (0, 0, 0) };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec).ok()
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelError {
    NotUtf8,
    /// A header that opens but is not a JSON object we can read.
    BadHeader(String),
    TooLarge(&'static str),
    /// A schema newer than this build: read-only here, export still works.
    Newer(u64),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::NotUtf8 => write!(f, "the note is not UTF-8 text"),
            ModelError::BadHeader(e) => write!(f, "the note's header could not be read: {e}"),
            ModelError::TooLarge(what) => write!(f, "the note's {what} is over its limit"),
            ModelError::Newer(v) => write!(f, "this note was written by a newer nus (format {v}); it opens read only"),
        }
    }
}

/// One note, split into what nus keeps (the header) and what you wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The whole header object, unknown members included. Empty (and not
    /// written) for a legacy note.
    pub meta: Map<String, Value>,
    pub body: String,
    /// Line ending the header was written with, kept on save.
    crlf: bool,
}

impl Document {
    /// A new note's document: an id, a revision, no body yet.
    pub fn new(id: &str, title: &str, filed: bool, now: u64) -> Document {
        let stamp = rfc3339(now);
        let meta = json!({
            "title": title.trim(),
            "tags": [],
            "nus": {
                "schema": SCHEMA,
                "id": id,
                "revision": 1,
                "created_at": stamp,
                "updated_at": stamp,
                "filed": filed,
                "sources": [],
                "assets": [],
                "aliases": [],
            },
        });
        let Value::Object(meta) = meta else { unreachable!() };
        Document { meta, body: String::new(), crlf: false }
    }

    /// Text with no header: how a legacy note, or one this build cannot
    /// read as ours, is shown.
    pub fn plain(text: &str) -> Document {
        Document { meta: Map::new(), body: text.to_string(), crlf: false }
    }

    /// Read a note's exact bytes. A file without our header is a legacy
    /// note: its whole text is the body.
    pub fn parse(bytes: &[u8]) -> Result<Document, ModelError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ModelError::NotUtf8)?;
        let crlf = text.starts_with("---\r\n");
        let open = if crlf { "---\r\n" } else { "---\n" };
        let Some(rest) = text.strip_prefix(open).filter(|r| r.trim_start().starts_with('{')) else {
            return Ok(Document { meta: Map::new(), body: text.to_string(), crlf: false });
        };
        // JSON strings cannot hold a raw newline, so the first line that is
        // exactly `---` after the object closes it.
        let mut at = 0;
        let (header, body) = loop {
            let Some(i) = rest[at..].find("\n---") else { return Err(ModelError::BadHeader("no closing ---".into())) };
            let end = at + i;
            let after = &rest[end + 4..];
            let body = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).or_else(|| after.is_empty().then_some(""));
            if let Some(body) = body {
                break (&rest[..end], body);
            }
            at = end + 1;
        };
        let header = header.trim_end_matches('\r');
        if header.len() > MAX_META {
            return Err(ModelError::TooLarge("header"));
        }
        let meta: Map<String, Value> = serde_json::from_str(header).map_err(|e| ModelError::BadHeader(e.to_string()))?;
        let doc = Document { meta, body: body.to_string(), crlf };
        if let Some(v) = doc.schema() {
            if v > SCHEMA {
                return Err(ModelError::Newer(v));
            }
        }
        doc.check_bounds()?;
        Ok(doc)
    }

    /// The note as bytes: the header (when there is one) then the body,
    /// untouched.
    pub fn to_bytes(&self) -> Vec<u8> {
        if self.meta.is_empty() {
            return self.body.clone().into_bytes();
        }
        let nl = if self.crlf { "\r\n" } else { "\n" };
        let mut header = serde_json::to_string_pretty(&self.meta).unwrap_or_else(|_| "{}".into());
        if self.crlf {
            header = header.replace('\n', "\r\n");
        }
        format!("---{nl}{header}{nl}---{nl}{}", self.body).into_bytes()
    }

    pub fn check_bounds(&self) -> Result<(), ModelError> {
        if self.tags().len() > MAX_TAGS {
            return Err(ModelError::TooLarge("tag list"));
        }
        if self.sources().len() > MAX_SOURCES {
            return Err(ModelError::TooLarge("source list"));
        }
        if !self.meta.is_empty() && serde_json::to_string(&self.meta).map_or(0, |s| s.len()) > MAX_META {
            return Err(ModelError::TooLarge("header"));
        }
        Ok(())
    }

    pub fn is_legacy(&self) -> bool {
        self.nus().is_none()
    }

    fn nus(&self) -> Option<&Map<String, Value>> {
        self.meta.get("nus")?.as_object()
    }

    fn nus_mut(&mut self) -> Option<&mut Map<String, Value>> {
        self.meta.get_mut("nus")?.as_object_mut()
    }

    pub fn schema(&self) -> Option<u64> {
        self.nus()?.get("schema")?.as_u64()
    }

    pub fn id(&self) -> Option<&str> {
        self.nus()?.get("id")?.as_str().filter(|s| valid_id(s))
    }

    pub fn revision(&self) -> u64 {
        self.nus().and_then(|n| n.get("revision")?.as_u64()).unwrap_or(0)
    }

    /// The title you gave it, else its first line (a heading's words), else
    /// "Untitled". A title is not a file name: renaming moves nothing.
    pub fn title(&self) -> String {
        let set = self.meta.get("title").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if !set.is_empty() {
            return set.to_string();
        }
        provisional_title(&self.body)
    }

    pub fn set_title(&mut self, title: &str) {
        self.meta.insert("title".into(), Value::String(title.trim().to_string()));
    }

    pub fn tags(&self) -> Vec<String> {
        self.meta.get("tags").and_then(Value::as_array).map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }

    pub fn set_tags(&mut self, tags: &[String]) {
        let mut seen = std::collections::HashSet::new();
        let tags: Vec<Value> = tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty() && seen.insert(t.to_lowercase())).map(|t| Value::String(t.to_string())).collect();
        self.meta.insert("tags".into(), Value::Array(tags));
    }

    /// An authored note is filed; a quick capture with no named note is
    /// not, until Keep as Note. It never decides where the note lives.
    pub fn filed(&self) -> bool {
        self.nus().and_then(|n| n.get("filed")?.as_bool()).unwrap_or(true)
    }

    pub fn set_filed(&mut self, filed: bool) {
        if let Some(n) = self.nus_mut() {
            n.insert("filed".into(), Value::Bool(filed));
        }
    }

    pub fn created(&self) -> Option<u64> {
        self.nus().and_then(|n| parse_time(n.get("created_at")?.as_str()?))
    }

    pub fn updated(&self) -> Option<u64> {
        self.nus().and_then(|n| parse_time(n.get("updated_at")?.as_str()?))
    }

    pub fn deleted_at(&self) -> Option<u64> {
        self.nus().and_then(|n| parse_time(n.get("deleted_at")?.as_str()?))
    }

    pub fn set_deleted(&mut self, at: Option<u64>) {
        if let Some(n) = self.nus_mut() {
            match at {
                Some(t) => { n.insert("deleted_at".into(), Value::String(rfc3339(t))); }
                None => { n.remove("deleted_at"); }
            }
        }
    }

    /// The step a commit takes: the next revision, when, and the hash of
    /// the bytes it replaces. Lineage, never a rule for who wins.
    pub fn stamp_commit(&mut self, previous_sha256: Option<&str>, now: u64) {
        let next = self.revision() + 1;
        if let Some(n) = self.nus_mut() {
            n.insert("revision".into(), json!(next));
            n.insert("updated_at".into(), Value::String(rfc3339(now)));
            match previous_sha256 {
                Some(h) => { n.insert("previous_sha256".into(), Value::String(h.to_string())); }
                None => { n.remove("previous_sha256"); }
            }
        }
    }

    /// Take a commit's stamp (revision, time, lineage) from the document
    /// that was saved, keeping every other header change made since.
    pub fn adopt_stamp(&mut self, saved: &Document) {
        let (Some(from), Some(to)) = (saved.nus().cloned(), self.nus_mut()) else { return };
        for k in ["revision", "updated_at", "previous_sha256"] {
            match from.get(k) {
                Some(v) => { to.insert(k.into(), v.clone()); }
                None => { to.remove(k); }
            }
        }
    }

    pub fn sources(&self) -> Vec<Source> {
        self.nus().and_then(|n| n.get("sources")?.as_array()).map(|a| a.iter().filter_map(|v| v.as_object().map(|o| Source(o.clone()))).collect()).unwrap_or_default()
    }

    pub fn push_source(&mut self, source: Source) {
        if let Some(n) = self.nus_mut() {
            let list = n.entry("sources").or_insert_with(|| Value::Array(Vec::new()));
            if let Some(a) = list.as_array_mut() {
                a.push(Value::Object(source.0));
            }
        }
    }

    pub fn remove_source(&mut self, id: &str) {
        if let Some(a) = self.nus_mut().and_then(|n| n.get_mut("sources")?.as_array_mut()) {
            a.retain(|v| v.get("id").and_then(Value::as_str) != Some(id));
        }
    }

    /// Links to other notes written in the body: `[words](note:<id>)` in
    /// this home, `[words](note:<home>/<id>)` in another. A note's name
    /// merely mentioned in prose or code is not a link.
    pub fn links(&self) -> Vec<Link> {
        note_links(&self.body)
    }
}

/// The first line with words in it, as a title: a heading's words, or the
/// start of a sentence.
pub fn provisional_title(body: &str) -> String {
    let mut fence = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence || t.is_empty() || t == "---" || t.starts_with("<!--") {
            continue;
        }
        // The legacy header's own lines are not a title.
        if t.starts_with("space:") || t.starts_with("made:") {
            continue;
        }
        let words = t.trim_start_matches('#').trim_start_matches(['-', '*', '>']).trim();
        let words = words.strip_prefix("[ ]").or_else(|| words.strip_prefix("[x]")).unwrap_or(words).trim();
        if words.is_empty() {
            continue;
        }
        let mut s: String = words.chars().take(80).collect();
        if words.chars().count() > 80 {
            s.push('…');
        }
        return s;
    }
    "Untitled".into()
}

/// A captured source: the record as stored, unknown members kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Source(pub Map<String, Value>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Terminal,
    Web,
    File,
    Reading,
    Note,
    /// A kind this build does not know: kept, shown, never acted on.
    Other,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Terminal => "terminal",
            Kind::Web => "web",
            Kind::File => "file",
            Kind::Reading => "reading",
            Kind::Note => "note",
            Kind::Other => "other",
        }
    }
}

impl Source {
    pub fn new(kind: Kind, id: &str, label: &str, captured_at: u64) -> Source {
        let mut m = Map::new();
        m.insert("id".into(), Value::String(id.into()));
        m.insert("kind".into(), Value::String(kind.word().into()));
        m.insert("label".into(), Value::String(label.into()));
        m.insert("captured_at".into(), Value::String(rfc3339(captured_at)));
        Source(m)
    }

    pub fn with(mut self, key: &str, value: Value) -> Source {
        self.0.insert(key.into(), value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.as_str()
    }

    pub fn id(&self) -> &str {
        self.get("id").unwrap_or("")
    }

    pub fn kind(&self) -> Kind {
        match self.get("kind") {
            Some("terminal") => Kind::Terminal,
            Some("web") => Kind::Web,
            Some("file") => Kind::File,
            Some("reading") => Kind::Reading,
            Some("note") => Kind::Note,
            _ => Kind::Other,
        }
    }

    pub fn label(&self) -> &str {
        self.get("label").unwrap_or("")
    }

    pub fn captured_at(&self) -> Option<u64> {
        parse_time(self.get("captured_at")?)
    }

    /// The captured text itself: a terminal/file excerpt or a page quote.
    pub fn captured(&self) -> Option<&str> {
        self.get("excerpt").or_else(|| self.get("quote"))
    }

    /// The record's own evidence holds: its text hashes to its hash.
    pub fn verified(&self) -> Option<bool> {
        let hash = self.get("captured_sha256")?;
        Some(captured_sha256(self.captured()?) == hash)
    }

    /// Words search reads for this source: its label, command, path, URL,
    /// title and quote.
    pub fn search_text(&self) -> String {
        ["label", "command", "relative_path", "url", "source_url", "title", "excerpt", "quote"]
            .iter()
            .filter_map(|k| self.get(k))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The fence or quote bound to a source marker, as the note shows it now.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub source_id: String,
    /// The marker's line (0-based).
    pub line: usize,
    /// The visible text under it; None when nothing bindable follows.
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// The visible excerpt is still exactly what was captured.
    Original,
    /// Someone edited the visible text; the original is in the record.
    Edited,
    /// The record's own hash does not match its own text.
    VerificationFailed,
    /// No record for this marker in this note.
    Unresolved,
    /// Two markers claim the same record.
    Ambiguous,
    /// The marker binds nothing (a paragraph, a blank line, the end).
    Unbound,
}

pub fn marker(id: &str) -> String {
    format!("<!-- nus:source {id} -->")
}

fn marker_id(line: &str) -> Option<&str> {
    let id = line.strip_prefix("<!-- nus:source ")?.strip_suffix(" -->")?;
    valid_id(id).then_some(id)
}

/// A fence opening this line: its character and length.
fn fence_open(line: &str) -> Option<(char, usize)> {
    let c = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = line.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

fn fence_closes(line: &str, (c, n): (char, usize)) -> bool {
    let t = line.trim_end();
    t.chars().count() >= n && t.chars().all(|x| x == c)
}

/// Every source marker outside a fence, with what it binds: the fence or
/// quote on the very next line, never one further down.
pub fn bindings(body: &str) -> Vec<Binding> {
    let norm = body.replace("\r\n", "\n");
    let lines: Vec<&str> = norm.split('\n').collect();
    let fenced = |at: usize| -> Option<(String, usize)> {
        let open = fence_open(lines[at])?;
        let end = (at + 1..lines.len()).find(|&j| fence_closes(lines[j], open))?;
        Some((lines[at + 1..end].join("\n"), end + 1))
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(id) = marker_id(lines[i]) {
            let mut next = i + 1;
            let mut text = None;
            if next < lines.len() {
                if let Some((t, after)) = fenced(next) {
                    text = Some(t);
                    next = after;
                } else if lines[next].starts_with('>') {
                    let mut q = Vec::new();
                    while next < lines.len() && lines[next].starts_with('>') {
                        let l = &lines[next][1..];
                        q.push(l.strip_prefix(' ').unwrap_or(l));
                        next += 1;
                    }
                    text = Some(q.join("\n"));
                }
            }
            out.push(Binding { source_id: id.to_string(), line: i, text });
            i = next;
        } else if let Some((_, after)) = fenced(i) {
            i = after;
        } else {
            i += 1;
        }
    }
    out
}

/// What each marker in `doc` says about its evidence.
pub fn evidence(doc: &Document) -> Vec<(Binding, Evidence)> {
    let all = bindings(&doc.body);
    let sources = doc.sources();
    all.iter()
        .map(|b| {
            let twice = all.iter().filter(|o| o.source_id == b.source_id).count() > 1;
            let state = match (sources.iter().find(|s| s.id() == b.source_id), &b.text) {
                _ if twice => Evidence::Ambiguous,
                (None, _) => Evidence::Unresolved,
                (Some(_), None) => Evidence::Unbound,
                (Some(s), Some(text)) => match (s.verified(), s.get("captured_sha256")) {
                    (Some(false), _) => Evidence::VerificationFailed,
                    (_, Some(h)) if captured_sha256(text) == h => Evidence::Original,
                    (_, Some(_)) => Evidence::Edited,
                    _ => match s.captured() {
                        Some(c) if c.replace("\r\n", "\n") == *text => Evidence::Original,
                        _ => Evidence::Edited,
                    },
                },
            };
            (b.clone(), state)
        })
        .collect()
}

/// Markdown without nus: no header, no markers, each bound source
/// followed by a plain caption line. The visible quotes are unchanged.
pub fn clean_export(doc: &Document) -> String {
    let sources = doc.sources();
    let bound = bindings(&doc.body);
    let norm = doc.body.replace("\r\n", "\n");
    let mut out = String::new();
    let lines: Vec<&str> = norm.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        match bound.iter().find(|b| b.line == i) {
            Some(b) => {
                // Skip the marker, copy the block it binds, then caption it.
                let len = b.text.as_ref().map(|t| t.split('\n').count() + if fence_open(lines.get(i + 1).copied().unwrap_or("")).is_some() { 2 } else { 0 }).unwrap_or(0);
                for l in &lines[i + 1..(i + 1 + len).min(lines.len())] {
                    out.push_str(l);
                    out.push('\n');
                }
                if let Some(s) = sources.iter().find(|s| s.id() == b.source_id) {
                    out.push_str(&caption(s));
                    out.push('\n');
                }
                i += 1 + len;
            }
            None => {
                out.push_str(lines[i]);
                if i + 1 < lines.len() {
                    out.push('\n');
                }
                i += 1;
            }
        }
    }
    out
}

/// One readable line saying where a source came from.
pub fn caption(s: &Source) -> String {
    let when = s.get("captured_at").map(|t| t.get(..10).unwrap_or(t)).unwrap_or("");
    let origin = match s.kind() {
        Kind::Terminal => s.get("command").map(|c| format!("`{c}`")),
        Kind::Web => s.get("url").map(str::to_string),
        Kind::Reading => s.get("source_url").map(str::to_string),
        Kind::File => s.get("relative_path").map(str::to_string),
        _ => None,
    };
    let label = s.label();
    match origin {
        Some(o) if !label.is_empty() => format!("— {label}, {o}, captured {when}"),
        Some(o) => format!("— {o}, captured {when}"),
        None => format!("— {label}, captured {when}"),
    }
}

/// A written link to a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// None: the same home as the note it is written in.
    pub home_id: Option<String>,
    pub note_id: String,
    pub label: String,
    pub line: usize,
}

/// `[label](note:<id>)` and `[label](note:<home>/<id>)`, outside code.
pub fn note_links(body: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, line) in body.lines().enumerate() {
        if let Some(f) = fence {
            if fence_closes(line.trim_start(), f) {
                fence = None;
            }
            continue;
        }
        if let Some(f) = fence_open(line.trim_start()) {
            fence = Some(f);
            continue;
        }
        let mut rest = strip_code_spans(line);
        while let Some(at) = rest.find("](note:") {
            let label_start = rest[..at].rfind('[').map(|s| s + 1);
            let tail = &rest[at + 7..];
            let Some(close) = tail.find(')') else { break };
            let target = &tail[..close];
            let (home, id) = match target.rsplit_once('/') {
                Some((h, id)) => (Some(h.to_string()), id),
                None => (None, target),
            };
            if valid_id(id) && home.as_deref().is_none_or(|h| !h.is_empty() && !h.contains(char::is_whitespace)) {
                let label = label_start.map(|s| rest[s..at].to_string()).unwrap_or_default();
                out.push(Link { home_id: home, note_id: id.to_string(), label, line: i });
            }
            rest = tail[close..].to_string();
        }
    }
    out
}

fn strip_code_spans(line: &str) -> String {
    let mut out = String::new();
    let mut code = false;
    for c in line.chars() {
        if c == '`' {
            code = !code;
            out.push(' ');
        } else if code {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// A code-like value search must match exactly: punctuation, case and a
/// URL's query and fragment all count.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Literal {
    pub kind: &'static str,
    pub value: String,
}

pub fn nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect()
}

/// Classify one whitespace-free token as a flag, a URL or a path.
pub fn literal_of(token: &str) -> Option<Literal> {
    let t = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '<' | '>' | ',' | ';'));
    let t = t.strip_suffix('.').filter(|s| !s.ends_with('.')).unwrap_or(t);
    if t.len() < 2 || t.len() > 2048 {
        return None;
    }
    let kind = if t.starts_with("https://") || t.starts_with("http://") || t.starts_with("file://") {
        "url"
    } else if t.starts_with('-') && t.chars().nth(1).is_some_and(|c| c.is_alphanumeric() || c == '-') && t.trim_start_matches('-').chars().next().is_some_and(char::is_alphanumeric) {
        "flag"
    } else if (t.contains('/') || t.contains('\\')) && t.chars().any(char::is_alphanumeric) && !t.contains("://") {
        "path"
    } else {
        return None;
    };
    Some(Literal { kind, value: nfc(t) })
}

/// Every literal in a note: its sources' commands, paths and URLs, and the
/// flags, paths and URLs in its code spans and fences.
pub fn literals(doc: &Document) -> Vec<Literal> {
    let mut out = std::collections::BTreeSet::new();
    fn take(out: &mut std::collections::BTreeSet<Literal>, text: &str) {
        for w in text.split_whitespace() {
            if let Some(l) = literal_of(w) {
                out.insert(l);
            }
        }
    }
    for s in doc.sources() {
        for k in ["command", "relative_path", "url", "source_url"] {
            if let Some(v) = s.get(k) {
                take(&mut out, v);
                if k != "command" {
                    if let Some(l) = literal_of(v).or_else(|| (k == "relative_path").then(|| Literal { kind: "path", value: nfc(v) })) {
                        out.insert(l);
                    }
                }
            }
        }
    }
    let mut fence: Option<(char, usize)> = None;
    for line in doc.body.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if fence_closes(t, f) { fence = None } else { take(&mut out, line) }
            continue;
        }
        if let Some(f) = fence_open(t) {
            fence = Some(f);
            take(&mut out, t.trim_start_matches(f.0));
            continue;
        }
        let mut code = false;
        let mut span = String::new();
        for c in line.chars() {
            if c == '`' {
                if code { take(&mut out, &span); span.clear(); }
                code = !code;
            } else if code {
                span.push(c);
            }
        }
        // Bare URLs in prose count too.
        for w in line.split_whitespace().filter(|w| w.contains("://")) {
            take(&mut out, w);
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../tests/notes/example-note.md");

    #[test]
    fn the_example_note_reads_and_round_trips() {
        let doc = Document::parse(EXAMPLE.as_bytes()).unwrap();
        assert_eq!(doc.id(), Some("2c557afcd3864f9c85f659ea18770ea1"));
        assert_eq!(doc.revision(), 1);
        assert_eq!(doc.title(), "Why the prompt wraps");
        assert_eq!(doc.tags(), vec!["terminal", "reflow"]);
        assert!(doc.filed());
        assert!(doc.body.starts_with("\nThe command is one logical line"));
        assert!(doc.body.contains("```nus-block") && doc.body.contains("- [ ]"));
        // Unknown members survive a save; the body's bytes are untouched.
        let again = Document::parse(&doc.to_bytes()).unwrap();
        assert_eq!(again, doc);
        assert_eq!(again.meta["custom_owner"], "example of an unknown field that must survive saves");
        assert_eq!(doc.created(), parse_time("2026-09-26T18:00:00Z"));
    }

    #[test]
    fn the_example_sources_are_typed() {
        let v: Value = serde_json::from_str(include_str!("../tests/notes/example-sources.json")).unwrap();
        let sources: Vec<Source> = v["sources"].as_array().unwrap().iter().map(|s| Source(s.as_object().unwrap().clone())).collect();
        let kinds: Vec<Kind> = sources.iter().map(Source::kind).collect();
        assert_eq!(kinds, vec![Kind::Web, Kind::File, Kind::Reading, Kind::Note]);
        assert!(sources.iter().all(|s| valid_id(s.id())));
        assert!(sources[0].search_text().contains("https://www.sqlite.org/fts5.html"));
        assert_eq!(sources[2].get("snapshot_sha256").map(str::len), Some(64));
        assert_eq!(sources[3].get("target_home_id"), Some("personal-home"));
    }

    #[test]
    fn a_marker_binds_its_fence_and_the_hash_tells_edits_apart() {
        let doc = Document::parse(EXAMPLE.as_bytes()).unwrap();
        let s = &doc.sources()[0];
        assert_eq!(s.verified(), Some(true));
        let b = bindings(&doc.body);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].text.as_deref(), s.captured());
        assert_eq!(evidence(&doc)[0].1, Evidence::Original);

        let mut edited = doc.clone();
        edited.body = edited.body.replace("12 passed; 0 failed", "Edited: investigate");
        assert_eq!(evidence(&edited)[0].1, Evidence::Edited);
        // The record still holds the original.
        assert_eq!(edited.sources()[0].captured(), Some("test result: ok. 12 passed; 0 failed"));

        let mut twice = doc.clone();
        twice.body = format!("{}\n{}", doc.body, doc.body);
        assert!(evidence(&twice).iter().all(|(_, e)| *e == Evidence::Ambiguous));
    }

    #[test]
    fn binding_follows_the_documented_subset() {
        let id = "d66a5679d5e14ed690867050053bc16e";
        let m = marker(id);
        assert!(bindings(&format!("```text\n{m}\n```\n")).is_empty(), "a marker inside a fence is text");
        let orphan = bindings(&format!("{m}\nUnrelated paragraph\n\n```\ntext\n```\n"));
        assert_eq!(orphan, vec![Binding { source_id: id.into(), line: 0, text: None }]);
        let quote = bindings(&format!("{m}\n> first\n>second\n"));
        assert_eq!(quote[0].text.as_deref(), Some("first\nsecond"));
        let long = bindings(&format!("{m}\n````md\n```\ninner\n```\n````\n"));
        assert_eq!(long[0].text.as_deref(), Some("```\ninner\n```"));
        assert!(bindings("<!-- nus:source NOTHEX -->\n```\nx\n```").is_empty());
        let mut doc = Document::new(&"0".repeat(32), "t", true, 0);
        doc.body = format!("{}\n```\ntext\n```\n", marker(&"1".repeat(32)));
        assert_eq!(evidence(&doc)[0].1, Evidence::Unresolved);
    }

    #[test]
    fn a_damaged_record_is_not_called_edited() {
        let mut doc = Document::new(&"0".repeat(32), "t", true, 0);
        let id = "a".repeat(32);
        doc.push_source(Source::new(Kind::Terminal, &id, "x", 0).with("excerpt", json!("abc")).with("captured_sha256", json!("f".repeat(64))));
        doc.body = format!("{}\n```\nabc\n```\n", marker(&id));
        assert_eq!(evidence(&doc)[0].1, Evidence::VerificationFailed);
    }

    #[test]
    fn clean_export_drops_markers_and_keeps_the_quote() {
        let doc = Document::parse(EXAMPLE.as_bytes()).unwrap();
        let out = clean_export(&doc);
        assert!(!out.contains("nus:source"));
        assert!(!out.contains("\"nus\""));
        assert!(out.contains("test result: ok. 12 passed; 0 failed\n```\n— Reflow regression test, `cargo test -p nus-vt reflow`, captured 2026-09-26\n"));
        assert!(out.contains("- [ ] Verify the original command survives a narrower window."));
    }

    #[test]
    fn legacy_notes_read_without_a_header() {
        let old = "---\nspace: nus\nmade: 2026-09-20T10:00Z\n---\n# Resize bug\n\nbody\n";
        let doc = Document::parse(old.as_bytes()).unwrap();
        assert!(doc.is_legacy());
        assert_eq!(doc.body, old);
        assert_eq!(doc.title(), "Resize bug");
        assert_eq!(doc.to_bytes(), old.as_bytes());
        assert_eq!(Document::parse(b"").unwrap().title(), "Untitled");
    }

    #[test]
    fn newer_and_broken_headers_say_so() {
        let newer = "---\n{\"nus\": {\"schema\": 9, \"id\": \"00000000000000000000000000000000\"}}\n---\nx";
        assert_eq!(Document::parse(newer.as_bytes()), Err(ModelError::Newer(9)));
        assert!(matches!(Document::parse(b"---\n{not json\n---\nx"), Err(ModelError::BadHeader(_))));
        assert_eq!(Document::parse(&[0xff, 0xfe]), Err(ModelError::NotUtf8));
    }

    #[test]
    fn crlf_headers_and_bodies_survive() {
        let mut doc = Document::new(&"0".repeat(32), "t", true, 0);
        doc.body = "line\r\nnext\r\n".into();
        let bytes = doc.to_bytes();
        let again = Document::parse(&bytes).unwrap();
        assert_eq!(again.body, "line\r\nnext\r\n");
        let crlf = String::from_utf8(bytes).unwrap().replace("---\n", "---\r\n").replacen("\n}", "\r\n}", 1);
        let doc = Document::parse(crlf.as_bytes()).unwrap();
        assert_eq!(Document::parse(&doc.to_bytes()).unwrap(), doc);
    }

    #[test]
    fn titles_come_from_the_header_or_the_first_line() {
        let mut doc = Document::new(&"0".repeat(32), "", true, 0);
        doc.body = "\n```\ncode first\n```\n## The *real* start\n".into();
        assert_eq!(doc.title(), "The *real* start");
        doc.set_title("Given");
        assert_eq!(doc.title(), "Given");
    }

    #[test]
    fn commits_step_the_revision_and_record_lineage() {
        let mut doc = Document::new(&"0".repeat(32), "t", true, 0);
        doc.stamp_commit(Some(&"a".repeat(64)), 86_400);
        assert_eq!(doc.revision(), 2);
        assert_eq!(doc.updated(), Some(86_400));
        assert_eq!(doc.nus().unwrap()["previous_sha256"], "a".repeat(64));
    }

    #[test]
    fn links_are_written_links_not_mentions() {
        let a = "1c557afcd3864f9c85f659ea18770ea1";
        let body = format!("see [results](note:{a}) and [other](note:home-x/{a})\n`[code](note:{a})`\n```\n[x](note:{a})\n```\nplain {a}\n");
        let l = note_links(&body);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0], Link { home_id: None, note_id: a.into(), label: "results".into(), line: 0 });
        assert_eq!(l[1].home_id.as_deref(), Some("home-x"));
    }

    #[test]
    fn literals_keep_their_punctuation() {
        let mut doc = Document::new(&"0".repeat(32), "t", true, 0);
        doc.push_source(Source::new(Kind::Terminal, &"a".repeat(32), "x", 0).with("command", json!("git merge --no-ff -n src/a-b.rs")));
        doc.body = "Use `--ff-only` with https://example.com/?x=1#head.\n```sh\nls C:\\Users\\me\n```\n".into();
        let l = literals(&doc);
        let has = |k: &str, v: &str| l.contains(&Literal { kind: match k { "flag" => "flag", "path" => "path", _ => "url" }, value: v.into() });
        assert!(has("flag", "--no-ff") && has("flag", "-n") && has("flag", "--ff-only"));
        assert!(has("path", "src/a-b.rs") && has("path", "C:\\Users\\me"));
        assert!(has("url", "https://example.com/?x=1#head"));
        assert!(!has("flag", "--n"));
        assert_eq!(literal_of("é").map(|l| l.kind), None);
        assert_eq!(nfc("e\u{301}"), "é");
    }

    #[test]
    fn times_parse_back() {
        for t in [0, 1_790_431_320, 1_790_431_359] {
            assert_eq!(parse_time(&rfc3339(t)), Some(t));
        }
        assert_eq!(parse_time("2026-09-01"), Some(1_788_220_800));
        assert_eq!(parse_time("2026-13-01"), None);
    }
}
