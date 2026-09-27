//! Bringing older notes in. A note written before ids and sources (a
//! LEGACY note) keeps working as it is: it opens, saves and is searched
//! without being rewritten. Migrating one is explicit (the palette's
//! "migrate this note"): its header's known lines become typed metadata,
//! lines it does not know are kept verbatim, its body is untouched, and
//! the original's exact bytes are kept beside the ledger that says what
//! became what (notes_store.rs `adopt_legacy`).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::notes_model::{self as model, Document};
use crate::notes_store::{NoteError, WriteCap};

/// A legacy note as a canonical document: the old `---` header (`space:`,
/// `made:`) read into metadata, anything else in it kept as written.
pub fn canonical(legacy: &str, id: &str, fallback_time: u64) -> Document {
    let mut body = legacy;
    let mut made = None;
    let mut space = None;
    let mut unknown: Vec<String> = Vec::new();
    let head = legacy.strip_prefix("---\n").or_else(|| legacy.strip_prefix("---\r\n"));
    if let Some(rest) = head {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
                for line in rest[..end].lines() {
                    match line.split_once(':') {
                        Some(("made", v)) => made = model::parse_time(v.trim()),
                        Some(("space", v)) => space = Some(v.trim().to_string()),
                        _ if line.trim().is_empty() => {}
                        _ => unknown.push(line.to_string()),
                    }
                }
                body = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);
            }
        }
    }
    let mut doc = Document::new(id, "", true, made.unwrap_or(fallback_time));
    doc.body = body.to_string();
    if let Some(Value::Object(n)) = doc.meta.get_mut("nus") {
        n.insert("imported_from".into(), json!({"kind": "markdown", "space": space}));
        if !unknown.is_empty() {
            n.insert("legacy_frontmatter".into(), json!(unknown.join("\n")));
        }
    }
    doc
}

/// Migrate the legacy note at `path`; the new note's path.
pub fn migrate(path: &Path) -> Result<PathBuf, NoteError> {
    let cap = WriteCap::grant()?;
    let home = crate::notes_session::home_of(path, &cap)?;
    let snap = home.open(path)?;
    if !snap.doc.is_legacy() {
        return Ok(path.to_path_buf());
    }
    let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(crate::notes::now(), |d| d.as_secs());
    let doc = canonical(&snap.doc.body, &model::new_id()?, modified);
    let made = home.adopt_legacy(&cap, path, doc)?;
    crate::notes_index::refresh(vec![crate::notes_session::Committed { home: home.clone(), path: made.path.clone() }]);
    Ok(made.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes_store::Home;

    #[test]
    fn the_old_header_becomes_metadata_and_the_body_stays() {
        let old = "---\nspace: nus\nmade: 2026-09-20T10:00Z\nowner: someone\n---\n# Resize bug\r\n\nbody\n";
        let d = canonical(old, &"0".repeat(32), 5);
        assert_eq!(d.body, "# Resize bug\r\n\nbody\n");
        assert_eq!(d.title(), "Resize bug");
        assert_eq!(d.created(), model::parse_time("2026-09-20T10:00Z"));
        let nus = d.meta["nus"].as_object().unwrap();
        assert_eq!(nus["legacy_frontmatter"], "owner: someone");
        assert_eq!(nus["imported_from"]["space"], "nus");
        let plain = canonical("no header\n", &"0".repeat(32), 5);
        assert_eq!(plain.body, "no header\n");
        assert_eq!(plain.created(), Some(5));
    }

    #[test]
    fn migrating_is_copy_first_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home::ensure_folder(dir.path(), &WriteCap::test()).unwrap();
        let old = home.root.join("old.md");
        let bytes = "---\nmade: 2026-09-20T10:00Z\n---\n# Old\n\ntext\n";
        std::fs::write(&old, bytes).unwrap();
        let doc = canonical(bytes, &model::new_id().unwrap(), 0);
        let made = home.adopt_legacy(&WriteCap::test(), &old, doc).unwrap();
        assert!(!old.exists(), "the legacy file leaves the list");
        let kept: Vec<_> = std::fs::read_dir(home.root.join(".state/imports")).unwrap().flatten().collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read_to_string(kept[0].path()).unwrap(), bytes, "its exact bytes are kept");
        assert_eq!(home.open(&made.path).unwrap().doc.body, "# Old\n\ntext\n");
        // The same original again (restored by hand): the same note.
        std::fs::write(&old, bytes).unwrap();
        let again = home.adopt_legacy(&WriteCap::test(), &old, canonical(bytes, &model::new_id().unwrap(), 0)).unwrap();
        assert_eq!(again.key, made.key);
        assert_eq!(home.list().unwrap().iter().filter(|e| !e.legacy).count(), 1);
    }
}
