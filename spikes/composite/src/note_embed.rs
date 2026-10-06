//! Embeds in a note: a line that is only `![[target]]` shows the target's
//! content where it stands, live. A target is another note (`note:<id>`,
//! or `note:<home>/<id>`), a file (a path, relative to the note's project,
//! with an optional `#L10-L20`), or a page (an http(s) address). The note
//! keeps only the line; what shows is read again whenever it changes, and
//! says so when the source moved on since the note was last written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Note { id: String, home: Option<String> },
    /// A file and, when given, its lines (1-based, inclusive).
    File { path: String, lines: Option<(usize, usize)> },
    Page(String),
    /// A clip (a block, a page's words, a file's lines) captured into a
    /// note, by its source id: the excerpt as that note holds it now.
    Source { id: String },
}

/// The target of a line that is only an embed.
pub fn parse(line: &str) -> Option<Target> {
    let t = line.trim();
    let inner = t.strip_prefix("![[")?.strip_suffix("]]")?.trim();
    if inner.is_empty() || inner.contains("]]") {
        return None;
    }
    if let Some(id) = inner.strip_prefix("note:") {
        return Some(match id.split_once('/') {
            Some((h, n)) => Target::Note { id: n.to_string(), home: Some(h.to_string()) },
            None => Target::Note { id: id.to_string(), home: None },
        });
    }
    if let Some(id) = inner.strip_prefix("source:").filter(|id| id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())) {
        return Some(Target::Source { id: id.to_string() });
    }
    if inner.starts_with("http://") || inner.starts_with("https://") {
        return Some(Target::Page(inner.to_string()));
    }
    let (path, lines) = match inner.rsplit_once("#L") {
        Some((p, range)) => {
            let (a, z) = range.split_once("-L").or_else(|| range.split_once('-')).unwrap_or((range, range));
            match (a.parse::<usize>(), z.parse::<usize>()) {
                (Ok(a), Ok(z)) if a >= 1 && z >= a => (p, Some((a, z))),
                _ => (inner, None),
            }
        }
        None => (inner, None),
    };
    Some(Target::File { path: path.to_string(), lines })
}

/// What an embed shows: a head (what it is), its lines, and a word when the
/// source moved on or cannot be read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub head: String,
    pub lines: Vec<String>,
    pub note: Option<String>,
    /// Code (a file) is set in the editor's face; a note's words in its own.
    pub code: bool,
}

/// Lines an embed shows at most; the rest is a click away.
pub const MAX_LINES: usize = 12;

thread_local! {
    /// Files read for embeds, by path: when they were read and what they said.
    static FILES: std::cell::RefCell<HashMap<PathBuf, (Option<SystemTime>, Vec<String>)>> = Default::default();
}

fn read_lines(path: &Path) -> Option<(Option<SystemTime>, Vec<String>)> {
    let modified = std::fs::metadata(path).ok()?.modified().ok();
    let cached = FILES.with(|f| f.borrow().get(path).filter(|(m, _)| *m == modified).cloned());
    if let Some(c) = cached {
        return Some(c);
    }
    let meta = std::fs::metadata(path).ok()?;
    // A huge file is not read for a glance.
    if meta.len() > 4 * 1024 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<String> = text.lines().map(|l| l.replace('\t', "    ")).collect();
    FILES.with(|f| f.borrow_mut().insert(path.to_path_buf(), (modified, lines.clone())));
    Some((modified, lines))
}

/// A file target's path on disk: as written when absolute, else from the
/// note's project folder (the folder `.nus/notes` sits in).
pub fn file_path(path: &str, base: Option<&Path>) -> PathBuf {
    let p = Path::new(path.trim_start_matches("./"));
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.map(|b| b.join(p)).unwrap_or_else(|| p.to_path_buf())
    }
}

/// A file's lines for an embed. `since`: when the note was last written.
pub fn file(path: &str, lines: Option<(usize, usize)>, base: Option<&Path>, since: Option<SystemTime>) -> Shown {
    let full = file_path(path, base);
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    let head = match lines {
        Some((a, z)) if a == z => format!("{name} · line {a}"),
        Some((a, z)) => format!("{name} · lines {a}–{z}"),
        None => name,
    };
    let Some((modified, all)) = read_lines(&full) else {
        return Shown { head, lines: Vec::new(), note: Some("not found here".into()), code: true };
    };
    let (a, z) = lines.unwrap_or((1, all.len().max(1)));
    let mut note = None;
    if a > all.len() {
        note = Some(format!("the file has {} lines now", all.len()));
    } else if z > all.len() {
        note = Some(format!("ends at line {} now", all.len()));
    } else if let (Some(m), Some(s)) = (modified, since) {
        if m > s {
            note = Some("changed since this note was written".into());
        }
    }
    let shown: Vec<String> = all.iter().skip(a.saturating_sub(1)).take((z + 1).saturating_sub(a).min(MAX_LINES)).cloned().collect();
    Shown { head, lines: shown, note, code: true }
}

/// Another note's opening lines (its body as it stands).
pub fn note(title: &str, body: Option<&str>) -> Shown {
    let title = if title.trim().is_empty() { "Untitled" } else { title };
    match body {
        // The head names it already: a first heading that only says the
        // title again is left out.
        Some(b) => {
            // Its words and clips' text, not their machinery: no source
            // markers, no fence lines.
            let mut lines: Vec<&str> = b.lines().filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("<!--") && !t.starts_with("```") && !t.starts_with("~~~")
            }).collect();
            if lines.first().is_some_and(|l| l.trim_start().starts_with('#') && l.trim_start_matches('#').trim() == title.trim()) {
                lines.remove(0);
            }
            Shown { head: title.to_string(), lines: lines.into_iter().take(MAX_LINES).map(str::to_string).collect(), note: None, code: false }
        }
        None => Shown { head: "a note".into(), lines: Vec::new(), note: Some("not found here · it may be in another project or the trash".into()), code: false },
    }
}

/// A page: its title as last seen (live while a tab shows it), its address.
pub fn page(url: &str, title: Option<&str>, open: bool) -> Shown {
    let host = url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url).trim_start_matches("www.");
    let head = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("{t} · {host}"),
        None => host.to_string(),
    };
    Shown { head, lines: vec![url.to_string()], note: open.then(|| "open in a tab".to_string()), code: false }
}

/// A clip by reference: what it was (its label), the excerpt as the note
/// holding it has it now, and where that is. edited: the excerpt no
/// longer matches what was captured.
pub fn clip(label: &str, kind: &str, held_by: &str, excerpt: Option<&str>, edited: bool) -> Shown {
    let label = if label.trim().is_empty() { "a clip" } else { label.trim() };
    let held_by = if held_by.trim().is_empty() { "Untitled" } else { held_by.trim() };
    match excerpt {
        Some(text) => Shown {
            head: format!("{label} · in {held_by}"),
            lines: text.lines().take(MAX_LINES).map(|l| l.replace('\t', "    ")).collect(),
            note: edited.then(|| "edited excerpt".to_string()),
            code: matches!(kind, "terminal" | "file"),
        },
        None => Shown { head: label.to_string(), lines: Vec::new(), note: Some("not found here · its note may be in another project or the trash".into()), code: false },
    }
}

/// Where a click on an embed goes, as a link would (`follow_note_link`).
pub fn link(t: &Target) -> String {
    match t {
        Target::Note { id, home: Some(h) } => format!("note:{h}/{id}"),
        Target::Note { id, home: None } => format!("note:{id}"),
        Target::File { path, lines: Some((a, _)) } => format!("{path}:{a}"),
        Target::File { path, lines: None } => path.clone(),
        Target::Page(u) => u.clone(),
        Target::Source { id } => format!("source:{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_that_is_only_an_embed_names_its_target() {
        assert_eq!(parse("![[note:abc]]"), Some(Target::Note { id: "abc".into(), home: None }));
        assert_eq!(parse("  ![[note:h1/abc]] "), Some(Target::Note { id: "abc".into(), home: Some("h1".into()) }));
        assert_eq!(parse("![[src/a.rs#L10-L20]]"), Some(Target::File { path: "src/a.rs".into(), lines: Some((10, 20)) }));
        assert_eq!(parse("![[src/a.rs#L7]]"), Some(Target::File { path: "src/a.rs".into(), lines: Some((7, 7)) }));
        assert_eq!(parse("![[README.md]]"), Some(Target::File { path: "README.md".into(), lines: None }));
        assert_eq!(parse("![[https://x.dev/a]]"), Some(Target::Page("https://x.dev/a".into())));
        assert_eq!(parse(&format!("![[source:{}]]", "ab".repeat(16))), Some(Target::Source { id: "ab".repeat(16) }));
        assert_eq!(parse("![[source:short]]"), Some(Target::File { path: "source:short".into(), lines: None }));
        assert_eq!(parse("see ![[note:abc]]"), None);
        assert_eq!(parse("![[]]"), None);
        assert_eq!(parse("![[src/a.rs#L9-L3]]"), Some(Target::File { path: "src/a.rs#L9-L3".into(), lines: None }));
    }

    #[test]
    fn a_file_embed_shows_its_lines_and_says_when_they_moved() {
        let dir = std::env::temp_dir().join(format!("nus-embed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let s = file("f.txt", Some((2, 3)), Some(&dir), None);
        assert_eq!(s.head, "f.txt · lines 2–3");
        assert_eq!(s.lines, vec!["two", "three"]);
        assert_eq!(s.note, None);
        assert_eq!(file("f.txt", Some((3, 9)), Some(&dir), None).note.as_deref(), Some("ends at line 4 now"));
        assert_eq!(file("f.txt", Some((8, 9)), Some(&dir), None).note.as_deref(), Some("the file has 4 lines now"));
        let long_ago = SystemTime::UNIX_EPOCH;
        assert_eq!(file("f.txt", Some((1, 1)), Some(&dir), Some(long_ago)).note.as_deref(), Some("changed since this note was written"));
        assert_eq!(file("gone.txt", None, Some(&dir), None).note.as_deref(), Some("not found here"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_note_embed_does_not_repeat_its_title() {
        let s = note("Design doc", Some("# Design doc\n\nThe plan.\n## Why\n"));
        assert_eq!(s.lines, vec!["The plan.", "## Why"]);
        assert_eq!(note("Other", Some("# Design doc\nx")).lines, vec!["# Design doc", "x"]);
        let clipped = note("Run", Some("<!-- nus:source abc -->\n```nus-block cmd=\"ls\"\nfile.txt\n```\n"));
        assert_eq!(clipped.lines, vec!["file.txt"]);
    }

    #[test]
    fn a_page_shows_its_title_and_a_clip_its_excerpt() {
        assert_eq!(page("https://www.x.dev/a", Some("The A"), true).head, "The A · x.dev");
        assert_eq!(page("https://x.dev/a", None, false).head, "x.dev");
        assert_eq!(page("https://x.dev/a", None, true).note.as_deref(), Some("open in a tab"));
        let c = clip("cargo test", "terminal", "Release", Some("ok\n2 passed"), true);
        assert_eq!((c.head.as_str(), c.lines.len(), c.code, c.note.as_deref()), ("cargo test · in Release", 2, true, Some("edited excerpt")));
        assert!(clip("x", "web", "N", None, false).note.is_some());
    }

    #[test]
    fn a_click_goes_where_a_link_would() {
        assert_eq!(link(&Target::File { path: "src/a.rs".into(), lines: Some((10, 20)) }), "src/a.rs:10");
        assert_eq!(link(&Target::Note { id: "k".into(), home: Some("h".into()) }), "note:h/k");
    }
}
