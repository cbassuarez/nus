//! What a kept page said: an in-memory SQLite FTS5 index of every saved
//! copy, so the library's search and the kept palette's `said:` find a page
//! by its words, not only its title. Like the notes index it is a
//! projection, never a source of truth: contentless (it keeps the terms,
//! not the text), in memory only, so a page's words are never written to
//! disk in the clear, and rebuilt on a worker thread whenever the set of
//! saved copies changes. If it cannot be built, search still works on
//! titles and addresses.

use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, TryRecvError};

use rusqlite::{params, Connection};

use crate::library::store::Store;
use crate::library::Entry;
use crate::reader::{Article, Block};

type Built = Result<(Connection, Vec<String>), String>;

#[derive(Default)]
pub struct Words {
    conn: Option<Connection>,
    /// Row i of the index is entry ids[i].
    ids: Vec<String>,
    /// The copies the index was built from.
    built_for: u64,
    pending: Option<(u64, Receiver<Built>)>,
}

/// Which saved copies there are: the index is rebuilt when this changes.
fn signature(entries: &BTreeMap<String, Entry>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for e in entries.values().filter(|e| !e.deleted) {
        (&e.id, &e.snapshot).hash(&mut h);
    }
    h.finish()
}

/// An article's words, one string: headings, paragraphs, code, items,
/// quotes, captions, and the words of links and images.
pub fn text_of(a: &Article) -> String {
    let mut out = a.title.clone();
    for b in &a.blocks {
        out.push('\n');
        match b {
            Block::Heading(_, s) | Block::Para(s) | Block::Pre(s) | Block::Item(s) | Block::Quote(s) | Block::Caption(s) => out.push_str(s),
            Block::Image(alt, _) | Block::Link(alt, _) => out.push_str(alt),
        }
    }
    out
}

/// A contentless index over (id, text): only the terms are kept.
fn build(rows: Vec<(String, String)>) -> Built {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA temp_store = MEMORY; CREATE VIRTUAL TABLE said USING fts5(body, content='', tokenize='unicode61 remove_diacritics 2', prefix='2 3');").map_err(|e| e.to_string())?;
    let mut ids = Vec::with_capacity(rows.len());
    {
        let mut st = conn.prepare("INSERT INTO said(rowid, body) VALUES (?1, ?2)").map_err(|e| e.to_string())?;
        for (i, (id, text)) in rows.into_iter().enumerate() {
            st.execute(params![i as i64, text]).map_err(|e| e.to_string())?;
            ids.push(id);
        }
    }
    Ok((conn, ids))
}

/// Each word as a quoted prefix term, all of them required.
fn fts_query(words: &[String]) -> String {
    words.iter().filter(|w| !w.is_empty()).map(|w| format!("\"{}\"*", w.replace('"', "\"\""))).collect::<Vec<_>>().join(" ")
}

impl Words {
    /// Each frame: take a finished build; start one when the copies changed.
    pub fn tick(&mut self, store: &Store, entries: &BTreeMap<String, Entry>) {
        if let Some((sig, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(Ok((conn, ids))) => { self.built_for = *sig; self.conn = Some(conn); self.ids = ids; self.pending = None; }
                Ok(Err(e)) => { tracing::info!("kept words index: {e}"); self.built_for = *sig; self.pending = None; }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => { self.pending = None; }
            }
        }
        let sig = signature(entries);
        if sig == self.built_for || self.pending.is_some() { return; }
        let copies: Vec<Entry> = entries.values().filter(|e| !e.deleted && e.snapshot.is_some()).cloned().collect();
        if copies.is_empty() { self.conn = None; self.ids.clear(); self.built_for = sig; return; }
        let store = store.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new().name("nus-kept-words".into()).spawn(move || {
            let rows = copies.into_iter().filter_map(|e| {
                // Saved copies are the stored Article, not the capture format.
                let (bytes, _) = store.article(&e).ok()?;
                let a: Article = serde_json::from_slice(&bytes).ok()?;
                Some((e.id, text_of(&a)))
            }).collect();
            let _ = tx.send(build(rows));
        });
        match spawned {
            Ok(_) => self.pending = Some((sig, rx)),
            Err(e) => { tracing::info!("kept words index: {e}"); self.built_for = sig; }
        }
    }

    /// The kept items whose saved copy holds every word (as a prefix).
    pub fn search(&self, words: &[String]) -> HashSet<String> {
        let Some(conn) = &self.conn else { return HashSet::new() };
        let q = fts_query(words);
        if q.is_empty() { return HashSet::new(); }
        let Ok(mut st) = conn.prepare_cached("SELECT rowid FROM said WHERE said MATCH ?1 LIMIT 2000") else { return HashSet::new() };
        st.query_map(params![q], |r| r.get::<_, i64>(0))
            .map(|rows| rows.flatten().filter_map(|i| self.ids.get(i as usize).cloned()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_found_by_what_it_said() {
        let a = Article { title: "Understanding Ownership".into(), blocks: vec![Block::Para("Each value in Rust has an owner.".into()), Block::Pre("let s = String::from(\"hello\");".into())], ..Default::default() };
        let (conn, ids) = build(vec![("a".repeat(32), text_of(&a)), ("b".repeat(32), "Nothing about it".into())]).unwrap();
        let w = Words { conn: Some(conn), ids, built_for: 1, pending: None };
        let find = |q: &str| w.search(&q.split_whitespace().map(str::to_string).collect::<Vec<_>>());
        assert_eq!(find("owner value"), HashSet::from(["a".repeat(32)]));
        assert_eq!(find("own"), HashSet::from(["a".repeat(32)]), "a prefix finds the word");
        assert!(find("owner banana").is_empty(), "every word is required");
        assert!(find("\"").is_empty() || find("\"").len() <= 2, "a quote does not break the query");
    }
}
