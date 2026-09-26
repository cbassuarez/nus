//! Notes: markdown files beside the work, opened in the editor pane. Two
//! homes. FOLDER notes live in `<folder>/.nus/notes/`, plain text, the
//! project's business (kept out of git by `.git/info/exclude` unless you
//! commit them). PROFILE notes live in `profile/notes/`, sealed by the
//! vault like memory.md and carried by sync. A note is always a file any
//! editor can read; nothing here is an index or a database.
//!
//! A CLIP is a finished block quoted into a note with its own text, so it
//! outlives the journal and replay it came from; a page comes in as a link.
//! Clips pass the same secret scrubber Ask uses before they are written.
//! Nothing in this module talks to the network.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lines of block output a clip keeps, from the end (Ask's block chip
/// keeps the same number).
pub const CLIP_LINES: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Folder,
    Profile,
}

impl Place {
    pub fn label(self) -> &'static str {
        match self {
            Place::Folder => "folder",
            Place::Profile => "profile · sealed",
        }
    }
}

pub fn folder_dir(folder: &Path) -> PathBuf {
    folder.join(".nus").join("notes")
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

/// Keep `.nus/` out of the repository the folder is in, the way git keeps
/// local ignores: a line in `.git/info/exclude`, never `.gitignore`. A
/// folder outside git, or a worktree whose `.git` is a file, is left alone.
pub fn exclude_from_git(folder: &Path) -> std::io::Result<()> {
    let Some(root) = folder.ancestors().find(|a| a.join(".git").is_dir()) else { return Ok(()) };
    let rel = folder.strip_prefix(root).unwrap_or(Path::new(""));
    let mut line = String::from("/");
    for c in rel.components() {
        line.push_str(&c.as_os_str().to_string_lossy());
        line.push('/');
    }
    line.push_str(".nus/");
    let info = root.join(".git").join("info");
    let exclude = info.join("exclude");
    let old = std::fs::read_to_string(&exclude).unwrap_or_default();
    if old.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    std::fs::create_dir_all(&info)?;
    let mut text = old;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("# nus notes (Settings · Notes)\n");
    text.push_str(&line);
    text.push('\n');
    std::fs::write(exclude, text)
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

/// A fresh note's first lines.
pub fn template(title: &str, space: &str, secs: u64) -> String {
    let title = if title.trim().is_empty() { "untitled" } else { title.trim() };
    let mut s = String::from("---\n");
    if !space.is_empty() {
        s.push_str(&format!("space: {space}\n"));
    }
    s.push_str(&format!("made: {}\n---\n# {title}\n\n", stamp(secs)));
    s
}

/// Make a new note in `dir`, never over an existing one. Profile notes go
/// through the vault; folder notes are plain files.
pub fn create(dir: &Path, title: &str, space: &str) -> std::io::Result<PathBuf> {
    let secs = now();
    std::fs::create_dir_all(dir)?;
    let name = file_name(title, secs);
    let stem = name.trim_end_matches(".md").to_string();
    let mut path = dir.join(&name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.md"));
        n += 1;
    }
    let body = template(title, space, secs);
    if place_of(&path) == Some(Place::Profile) {
        crate::protected_state::write(&path, body.as_bytes())?;
    } else {
        std::fs::write(&path, body)?;
    }
    Ok(path)
}

/// Write a note whole: through the vault for a profile note, plain for a
/// folder note.
pub fn save(path: &Path, text: &str) -> std::io::Result<()> {
    if place_of(path) == Some(Place::Profile) {
        crate::protected_state::write(path, text.as_bytes())
    } else {
        std::fs::write(path, text)
    }
}

/// Add to the end of a note on disk (one that no editor has open).
pub fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if place_of(path) == Some(Place::Profile) {
        crate::protected_state::update(path, |old| {
            let mut v = old.to_vec();
            v.extend_from_slice(text.as_bytes());
            Ok(v)
        })
    } else {
        use std::io::Write;
        std::fs::OpenOptions::new().append(true).create(true).open(path)?.write_all(text.as_bytes())
    }
}

/// The folder's inbox: where a clip goes when no note is open beside it.
pub fn inbox(folder: &Path) -> std::io::Result<PathBuf> {
    let dir = folder_dir(folder);
    let path = dir.join("inbox.md");
    if !path.is_file() {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&path, template("inbox", "", now()))?;
    }
    Ok(path)
}

/// A link to a page, as a line in a note.
pub fn link_line(title: &str, url: &str) -> String {
    let name = if title.trim().is_empty() { url } else { title.trim() };
    format!("\n- [{}]({url})\n", name.replace(['[', ']'], ""))
}

/// A block, as a clip: a fenced `nus-block` whose info line says what ran,
/// where, how it ended and when; the last CLIP_LINES of its output inside,
/// scrubbed. Returns the text and how many secrets were masked.
pub fn clip_block(cmd: &str, output: &str, cwd: &str, exit: Option<i32>, secs: u64) -> (String, usize) {
    let lines: Vec<&str> = output.trim_end_matches('\n').lines().collect();
    let from = lines.len().saturating_sub(CLIP_LINES);
    let mut body = String::new();
    if from > 0 {
        body.push_str(&format!("… {from} earlier lines\n"));
    }
    body.push_str(&lines[from..].join("\n"));
    let scrubbed = crate::secrets::scrub(&body);
    let cmd_clean = crate::secrets::scrub(cmd.trim());
    let fence = fence_for(&scrubbed.text);
    let mut info = format!("nus-block cmd=\"{}\"", attr(&cmd_clean.text));
    if let Some(code) = exit {
        info.push_str(&format!(" exit={code}"));
    }
    if !cwd.is_empty() {
        info.push_str(&format!(" cwd=\"{}\"", attr(&home_short(cwd))));
    }
    info.push_str(&format!(" at=\"{}\"", stamp(secs)));
    let text = format!("\n{fence}{info}\n{}\n{fence}\n", scrubbed.text.trim_end());
    (text, scrubbed.findings + cmd_clean.findings)
}

/// A fence long enough that nothing inside closes it.
fn fence_for(body: &str) -> String {
    let mut longest = 0;
    for l in body.lines() {
        let n = l.trim_start().chars().take_while(|c| *c == '`').count();
        longest = longest.max(n);
    }
    "`".repeat(longest.max(2) + 1)
}

fn attr(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ")
}

fn home_short(p: &str) -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    match p.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => p.to_string(),
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
    out
}

fn file_ref(w: &str, line: usize) -> Option<Ref> {
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

/// One row of the index.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub modified: u64,
}

/// The notes in a directory, newest first.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Entry> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
        .map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            let path = e.path();
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            Entry { path, name, modified }
        })
        .collect();
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    v
}

/// Notes that mention `name` (its stem), with the first line that does.
pub fn backlinks(entries: &[Entry], name: &str, not: &Path) -> Vec<(PathBuf, usize)> {
    if name.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| e.path != not) {
        let text = if place_of(&e.path) == Some(Place::Profile) {
            crate::protected_state::read_text(&e.path).unwrap_or_default()
        } else {
            std::fs::read_to_string(&e.path).unwrap_or_default()
        };
        if let Some(i) = text.lines().position(|l| l.contains(name)) {
            out.push((e.path.clone(), i + 1));
        }
    }
    out
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `2026-09-26T14:02Z`: minutes are enough for a note, and UTC says so.
pub fn stamp(secs: u64) -> String {
    let (y, mo, d, h, mi) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}Z")
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

fn civil(secs: u64) -> (i64, u32, u32, u32, u32) {
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
    fn stamps_are_utc_minutes() {
        assert_eq!(stamp(0), "1970-01-01T00:00Z");
        // 2026-09-26 14:02 UTC
        assert_eq!(stamp(1_790_431_320), "2026-09-26T14:02Z");
    }

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
        assert!(!crate::protected_state::is_private_path(&profile.join("notes/ideas.txt")));
        assert!(!crate::protected_state::is_private_path(&profile.join("notes/sub/ideas.md")));
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
    fn a_block_clip_keeps_its_tail_and_masks_secrets() {
        let out: String = (0..100).map(|i| format!("line {i}\n")).collect::<String>() + "token=ghp_abcdefghijklmnopqrstuvwxyz0123\n";
        let (text, found) = clip_block("cargo test", &out, "/w", Some(101), 0);
        assert!(found >= 1);
        assert!(text.contains("[!SECRET!]"));
        assert!(!text.contains("ghp_"));
        assert!(text.contains("… 41 earlier lines"));
        assert!(!text.contains("line 40\n"));
        assert!(text.contains("line 99"));
        assert!(text.contains("```nus-block cmd=\"cargo test\" exit=101 cwd=\"/w\" at=\"1970-01-01T00:00Z\""));
    }

    #[test]
    fn a_fence_outgrows_the_backticks_inside() {
        let (text, _) = clip_block("cat README.md", "```rust\nfn main() {}\n```", "", Some(0), 0);
        assert!(text.contains("\n````nus-block"));
        assert!(text.trim_end().ends_with("````"));
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

    #[test]
    fn git_exclude_is_written_once() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        exclude_from_git(&repo.join("sub")).unwrap();
        exclude_from_git(&repo.join("sub")).unwrap();
        exclude_from_git(repo).unwrap();
        let text = std::fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert_eq!(text.matches("/sub/.nus/").count(), 1);
        assert_eq!(text.lines().filter(|l| *l == "/.nus/").count(), 1);
    }

    #[test]
    fn create_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let notes = folder_dir(dir.path());
        let a = create(&notes, "same", "").unwrap();
        let b = create(&notes, "same", "").unwrap();
        assert_ne!(a, b);
        assert!(b.ends_with("same-2.md"));
        assert!(std::fs::read_to_string(&a).unwrap().contains("# same"));
    }
}
