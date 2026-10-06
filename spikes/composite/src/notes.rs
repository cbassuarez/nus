//! Notes: Markdown files beside the work, opened in the editor pane. Two
//! homes. A PROJECT's notes live in `<project>/.nus/notes/`, plain text,
//! the project's business (kept out of git by its local exclude file
//! unless you choose otherwise). PERSONAL notes live in `profile/notes/`,
//! sealed by the vault. A note is always a file any editor can read; the
//! search index is built from them and never replaces them.
//!
//! This module names the places and reads what a note points at; what a
//! note is lives in notes_model.rs, where it is kept in notes_store.rs,
//! and who is editing it in notes_session.rs. Nothing here talks to the
//! network.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Folder,
    Profile,
}

impl Place {
    pub fn label(self) -> &'static str {
        match self {
            Place::Folder => "folder",
            Place::Profile => "personal",
        }
    }
}

pub fn profile_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("notes")
}

/// Which home a path is in, if it is a note at all.
pub fn place_of(path: &Path) -> Option<Place> {
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        return None;
    }
    let parent = path.parent()?;
    let root = profile_dir();
    let root = root.canonicalize().unwrap_or(root);
    if parent == root || parent.canonicalize().is_ok_and(|p| p == root) {
        return Some(Place::Profile);
    }
    let mut it = parent.components().rev();
    let notes = it.next()?.as_os_str();
    let dot = it.next()?.as_os_str();
    (notes == "notes" && dot == ".nus").then_some(Place::Folder)
}

/// The folder a folder note belongs to (the one holding `.nus`).
pub fn folder_of(path: &Path) -> Option<PathBuf> {
    (place_of(path) == Some(Place::Folder)).then(|| path.parent()?.parent()?.parent().map(Path::to_path_buf)).flatten()
}

/// A file name for a new note: the title's words, else the minute.
pub fn file_name(title: &str, secs: u64) -> String {
    let mut slug = String::new();
    for ch in title.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.chars().count() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        let (y, mo, d, h, mi) = civil(secs);
        format!("note-{y:04}-{mo:02}-{d:02}-{h:02}{mi:02}.md")
    } else {
        format!("{slug}.md")
    }
}

/// What a note points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    /// A clipped block: its command and exit.
    Block { cmd: String, exit: Option<i32>, line: usize },
    Page { url: String, line: usize },
    /// A path, and a line in it when one was written (`term.rs:812`).
    File { path: String, at: Option<u32>, line: usize },
    /// A written link to another note: `[words](note:<id>)`, or
    /// `note:<home>/<id>` in another home.
    Note { note_id: String, home_id: Option<String>, label: String, line: usize },
}

fn info_value(info: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=");
    let at = info.find(&pat)? + pat.len();
    let rest = &info[at..];
    if let Some(q) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut esc = false;
        for ch in q.chars() {
            match (esc, ch) {
                (true, c) => { out.push(c); esc = false; }
                (false, '\\') => esc = true,
                (false, '"') => return Some(out),
                (false, c) => out.push(c),
            }
        }
        Some(out)
    } else {
        Some(rest.split_whitespace().next().unwrap_or("").to_string())
    }
}

/// Everything a note points at, in the order it appears. Plain text in,
/// no I/O: blocks by their fences, pages by their http(s) addresses, files
/// by a path with a slash and an extension (and an optional `:line`).
pub fn refs(text: &str) -> Vec<Ref> {
    let mut out: Vec<Ref> = Vec::new();
    let mut fence: Option<String> = None;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim_start();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) && t.trim_end().chars().all(|c| c == '`') {
                fence = None;
            }
            continue;
        }
        let ticks = t.chars().take_while(|c| *c == '`').count();
        if ticks >= 3 {
            let info = &t[ticks..];
            if info.starts_with("nus-block") {
                let cmd = info_value(info, "cmd").unwrap_or_default();
                let exit = info_value(info, "exit").and_then(|e| e.parse().ok());
                out.push(Ref::Block { cmd, exit, line: i });
            }
            fence = Some("`".repeat(ticks));
            continue;
        }
        for word in l.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '[' | ']' | '"' | '\'' | '`')) {
            if word.starts_with("note:") {
                continue;
            }
            let w = word.trim_end_matches(['.', ',', ';', '—']);
            if w.starts_with("https://") || w.starts_with("http://") {
                if !out.iter().any(|r| matches!(r, Ref::Page { url, .. } if url == w)) {
                    out.push(Ref::Page { url: w.to_string(), line: i });
                }
            } else if let Some(r) = file_ref(w, i) {
                if !out.contains(&r) {
                    out.push(r);
                }
            }
        }
    }
    for l in crate::notes_model::note_links(text) {
        out.push(Ref::Note { note_id: l.note_id, home_id: l.home_id, label: l.label, line: l.line });
    }
    out.sort_by_key(|r| match r { Ref::Block { line, .. } | Ref::Page { line, .. } | Ref::File { line, .. } | Ref::Note { line, .. } => *line });
    out
}

pub(crate) fn file_ref(w: &str, line: usize) -> Option<Ref> {
    if w.contains("://") || !w.contains('/') {
        return None;
    }
    let (path, at) = match w.rsplit_once(':') {
        Some((p, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => (p, n.parse().ok()),
        _ => (w, None),
    };
    let path = path.split(':').next().unwrap_or(path);
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    let ok = !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && path.chars().all(|c| c.is_alphanumeric() || "/._-~".contains(c));
    ok.then(|| Ref::File { path: path.to_string(), at, line })
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A short age for the index: `14:02`, `tue`, `sep 20`.
pub fn when(secs: u64, now: u64) -> String {
    let (_, mo, d, h, mi) = civil(secs);
    let age = now.saturating_sub(secs);
    if age < 86_400 {
        format!("{h:02}:{mi:02}")
    } else if age < 6 * 86_400 {
        ["thu", "fri", "sat", "sun", "mon", "tue", "wed"][((secs / 86_400) % 7) as usize].to_string()
    } else {
        let m = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"][(mo as usize).saturating_sub(1).min(11)];
        format!("{m} {d}")
    }
}

pub(crate) fn civil(secs: u64) -> (i64, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_the_title_or_the_minute() {
        assert_eq!(file_name("Resize loses the cursor line!", 0), "resize-loses-the-cursor-line.md");
        assert_eq!(file_name("  ", 1_790_431_320), "note-2026-09-26-1402.md");
        assert_eq!(file_name("../../etc/passwd", 0), "etc-passwd.md");
    }

    #[test]
    fn profile_notes_are_sealed_and_folder_notes_are_not() {
        let profile = std::env::current_dir().unwrap().join("profile");
        assert!(crate::protected_state::is_private_path(&profile.join("notes/ideas.md")));
        assert!(crate::protected_state::is_private_path(&profile.join("notes/.state/recovery/a.json")));
        assert!(!crate::protected_state::is_private_path(&profile.join("notesy.md")));
        assert!(!crate::protected_state::is_private_path(Path::new("/w/proj/.nus/notes/a.md")));
    }

    #[test]
    fn places_by_directory() {
        assert_eq!(place_of(Path::new("/w/proj/.nus/notes/a.md")), Some(Place::Folder));
        assert_eq!(place_of(Path::new("/w/proj/.nus/notes/a.txt")), None);
        assert_eq!(place_of(Path::new("/w/proj/notes/a.md")), None);
        assert_eq!(place_of(&profile_dir().join("ideas.md")), Some(Place::Profile));
        assert_eq!(folder_of(Path::new("/w/proj/.nus/notes/a.md")), Some(PathBuf::from("/w/proj")));
    }

    #[test]
    fn refs_include_written_note_links() {
        let id = "1c557afcd3864f9c85f659ea18770ea1";
        let r = refs(&format!("see [the fix](note:{id})\nhttps://x.org\n"));
        assert_eq!(r[0], Ref::Note { note_id: id.into(), home_id: None, label: "the fix".into(), line: 0 });
        assert_eq!(r[1], Ref::Page { url: "https://x.org".into(), line: 1 });
    }

    #[test]
    fn refs_find_blocks_pages_and_files() {
        let note = "# t\nsee crates/vt/src/term.rs:812 and https://ghostty.org/docs/vt/reflow.\n\n```nus-block cmd=\"cargo test -p \\\"vt\\\"\" exit=101 cwd=\"~/nus\"\nhttps://inside.example/ignored\n```\n> — [x](https://ghostty.org/docs/vt/reflow)\nnot/a/file and v1.2 and ./README.md\n";
        let r = refs(note);
        assert_eq!(r[0], Ref::File { path: "crates/vt/src/term.rs".into(), at: Some(812), line: 1 });
        assert_eq!(r[1], Ref::Page { url: "https://ghostty.org/docs/vt/reflow".into(), line: 1 });
        assert_eq!(r[2], Ref::Block { cmd: "cargo test -p \"vt\"".into(), exit: Some(101), line: 3 });
        assert_eq!(r[3], Ref::File { path: "./README.md".into(), at: None, line: 7 });
        assert_eq!(r.len(), 4);
    }
}
