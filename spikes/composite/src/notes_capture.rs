//! Add to Note: one way in for every source. A capture is frozen the
//! moment you ask for it (the block's text, the page's selection, the
//! file's lines) into a DRAFT that says what it is, how much was left out
//! and how many secrets were masked. Choosing where it goes adds it
//! through the note's session (notes_session.rs) with a request id, so
//! the same capture twice lands once. No clicks run anything, and nothing
//! here reaches the network.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::notes_model::{self as model, Kind, Source};

/// A whole block keeps its last lines, up to this many and this size.
pub const BLOCK_LINES: usize = 200;
pub const BLOCK_BYTES: usize = 64 * 1024;
/// A selection is kept as chosen, up to this size.
pub const SELECTION_BYTES: usize = 2 * 1024 * 1024;

/// Where a capture comes from, as known when you asked.
#[derive(Clone, Debug)]
pub enum Origin {
    /// A shell block: its command, output and how it ended. `cwd` is the
    /// shell's folder when you captured, which is all a block knows.
    Block { cmd: String, output: String, cwd: String, exit: Option<i32>, running: bool, shell: String },
    /// Text selected in a shell.
    ShellSelection { text: String, cwd: String },
    /// A page, and the words selected on it (maybe none: a link then).
    Page { url: String, title: String, quote: String, container: String },
    /// A reading-list item: its annotation (maybe none) and the saved copy
    /// it belongs to, pinned by hash.
    Reading { library_id: String, source_url: String, title: String, snapshot: Option<String>, annotation: String, container: String },
    /// Lines of a file, relative to its project when it has one.
    File { path: PathBuf, project: Option<PathBuf>, start_line: usize, end_line: usize, text: String, file_sha256: Option<String> },
}

/// A frozen capture, ready for a destination.
#[derive(Clone, Debug)]
pub struct Draft {
    pub request_id: String,
    pub kind: Kind,
    /// What it is, in a few words: the command, the page's title, the file.
    pub label: String,
    /// The Markdown that goes in (marker, block or quote, caption).
    pub markdown: String,
    pub source: Source,
    pub masked: usize,
    pub omitted_lines: usize,
    pub incomplete: bool,
    /// The project this came from, for the default destination.
    pub project: Option<PathBuf>,
}

impl Draft {
    /// A few words about the capture for the picker and the receipt.
    pub fn summary(&self) -> String {
        let mut parts = vec![self.kind.word().to_string()];
        if self.incomplete {
            parts.push("still running · what shows so far".into());
        }
        if self.omitted_lines > 0 {
            parts.push(format!("{} earlier lines left out", self.omitted_lines));
        }
        if self.masked > 0 {
            parts.push(format!("{} secret{} masked", self.masked, if self.masked == 1 { "" } else { "s" }));
        }
        parts.join(" · ")
    }

    /// The Markdown with "why this matters" before it, when given.
    pub fn with_why(&self, why: &str) -> String {
        let why = why.trim();
        if why.is_empty() {
            self.markdown.clone()
        } else {
            format!("{why}\n\n{}", self.markdown)
        }
    }
}

/// A fence longer than any run of backticks in `body`.
pub fn fence_for(body: &str) -> String {
    let longest = body.lines().map(|l| l.trim_start().chars().take_while(|c| *c == '`').count()).max().unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

fn attr(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ")
}

/// A URL without its user and password; everything else kept (a query
/// can matter). The scrubber still looks at what is left.
pub fn clean_url(url: &str) -> (String, usize) {
    let stripped = match url::Url::parse(url) {
        Ok(mut u) if !u.username().is_empty() || u.password().is_some() => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            (u.to_string(), 1)
        }
        _ => (url.to_string(), 0),
    };
    let s = crate::secrets::scrub(&stripped.0);
    (s.text, stripped.1 + s.findings)
}

/// The last `lines` lines of `text`, no more than `bytes` bytes, whole
/// lines only; and how many lines were left out.
pub fn tail(text: &str, lines: usize, bytes: usize) -> (String, usize) {
    let all: Vec<&str> = text.trim_end_matches('\n').lines().collect();
    let mut from = all.len().saturating_sub(lines);
    let mut size: usize = all[from..].iter().map(|l| l.len() + 1).sum();
    while size > bytes && from < all.len() {
        size -= all[from].len() + 1;
        from += 1;
    }
    (all[from..].join("\n"), from)
}

fn home_short(p: &str) -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    match p.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => p.to_string(),
    }
}

fn quote(text: &str) -> String {
    text.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
}

/// Freeze a capture.
pub fn draft(origin: Origin, now: u64) -> std::io::Result<Draft> {
    let id = model::new_id()?;
    let request_id = model::new_id()?;
    let stamp = model::rfc3339(now);
    let date = stamp.get(..10).unwrap_or("").to_string();
    Ok(match origin {
        Origin::Block { cmd, output, cwd, exit, running, shell } => {
            let (kept, omitted) = tail(&output, BLOCK_LINES, BLOCK_BYTES);
            let body = crate::secrets::scrub(&kept);
            let cmd_clean = crate::secrets::scrub(cmd.trim());
            let excerpt = body.text.trim_end().to_string();
            let fence = fence_for(&excerpt);
            let mut info = format!("nus-block cmd=\"{}\"", attr(&cmd_clean.text));
            if let Some(code) = exit.filter(|_| !running) {
                info.push_str(&format!(" exit={code}"));
            }
            if running {
                info.push_str(" running");
            }
            if !cwd.is_empty() {
                info.push_str(&format!(" cwd=\"{}\"", attr(&home_short(&cwd))));
            }
            info.push_str(&format!(" at=\"{stamp}\""));
            let mut md = format!("{}\n{fence}{info}\n{excerpt}\n{fence}\n", model::marker(&id));
            if omitted > 0 {
                md.push_str(&format!("\n({omitted} earlier lines left out)\n"));
            }
            let label = crate::cutoff::oneline(&cmd_clean.text);
            let mut source = Source::new(Kind::Terminal, &id, &label, now)
                .with("command", json!(cmd_clean.text))
                .with("shell", json!(shell))
                // The shell's folder at capture: a block does not record
                // where it ran, so this says when it was looked at.
                .with("cwd_at_capture", json!(cwd))
                .with("excerpt", json!(excerpt))
                .with("captured_sha256", json!(model::captured_sha256(&excerpt)))
                .with("redaction", json!({"applied": true, "masked_count": body.findings + cmd_clean.findings}))
                .with("complete", json!(!running))
                .with("omitted_lines", json!(omitted));
            if let Some(code) = exit.filter(|_| !running) {
                source = source.with("exit", json!(code));
            }
            Draft { request_id, kind: Kind::Terminal, label, markdown: md, source, masked: body.findings + cmd_clean.findings, omitted_lines: omitted, incomplete: running, project: Some(PathBuf::from(cwd)).filter(|p| p.is_dir()) }
        }
        Origin::ShellSelection { text, cwd } => {
            let text: String = text.chars().take(SELECTION_BYTES).collect();
            let s = crate::secrets::scrub(&text);
            let excerpt = s.text.trim_end_matches('\n').to_string();
            let fence = fence_for(&excerpt);
            let md = format!("{}\n{fence}text\n{excerpt}\n{fence}\n", model::marker(&id));
            let label = crate::cutoff::oneline(excerpt.lines().next().unwrap_or("shell selection"));
            let source = Source::new(Kind::Terminal, &id, &label, now)
                .with("cwd_at_capture", json!(cwd))
                .with("excerpt", json!(excerpt))
                .with("captured_sha256", json!(model::captured_sha256(&excerpt)))
                .with("redaction", json!({"applied": true, "masked_count": s.findings}))
                .with("selection", json!(true));
            Draft { request_id, kind: Kind::Terminal, label, markdown: md, source, masked: s.findings, omitted_lines: 0, incomplete: false, project: Some(PathBuf::from(cwd)).filter(|p| p.is_dir()) }
        }
        Origin::Page { url, title, quote: q, container } => {
            let (url, url_masked) = clean_url(&url);
            let title = title.trim().to_string();
            let name = if title.is_empty() { url.clone() } else { title.replace(['[', ']'], "") };
            let text: String = q.chars().take(SELECTION_BYTES).collect();
            let s = crate::secrets::scrub(text.trim());
            let quoted = s.text.trim().to_string();
            let md = if quoted.is_empty() {
                format!("- [{name}]({url}) · {date}\n")
            } else {
                format!("{}\n{}\n\n— [{name}]({url}) · {date}\n", model::marker(&id), quote(&quoted))
            };
            let mut source = Source::new(Kind::Web, &id, &name, now)
                .with("url", json!(url))
                .with("title", json!(title))
                .with("browser_profile_id", json!("default"))
                .with("browser_container_id", json!(container));
            if !quoted.is_empty() {
                source = source.with("quote", json!(quoted)).with("captured_sha256", json!(model::captured_sha256(&quoted)));
            }
            source = source.with("redaction", json!({"applied": true, "masked_count": s.findings + url_masked}));
            Draft { request_id, kind: Kind::Web, label: name, markdown: md, source, masked: s.findings + url_masked, omitted_lines: 0, incomplete: false, project: None }
        }
        Origin::Reading { library_id, source_url, title, snapshot, annotation, container } => {
            let (url, url_masked) = if source_url.starts_with("http") { clean_url(&source_url) } else { (source_url.clone(), 0) };
            let name = if title.trim().is_empty() { url.clone() } else { title.trim().replace(['[', ']'], "") };
            let s = crate::secrets::scrub(annotation.trim());
            let quoted = s.text.trim().to_string();
            let link = if url.starts_with("http") { format!("[{name}]({url})") } else { name.clone() };
            let md = if quoted.is_empty() {
                format!("- {link} · saved article · {date}\n")
            } else {
                format!("{}\n{}\n\n— {link} · from the reading list · {date}\n", model::marker(&id), quote(&quoted))
            };
            let mut source = Source::new(Kind::Reading, &id, &name, now)
                .with("library_id", json!(library_id))
                .with("source_url", json!(url))
                .with("browser_profile_id", json!("default"))
                .with("browser_container_id", json!(container))
                .with("redaction", json!({"applied": true, "masked_count": s.findings + url_masked}));
            if let Some(h) = snapshot.filter(|h| h.len() == 64) {
                source = source.with("snapshot_sha256", json!(h));
            }
            if !quoted.is_empty() {
                source = source.with("quote", json!(quoted)).with("captured_sha256", json!(model::captured_sha256(&quoted)));
            }
            Draft { request_id, kind: Kind::Reading, label: name, markdown: md, source, masked: s.findings + url_masked, omitted_lines: 0, incomplete: false, project: None }
        }
        Origin::File { path, project, start_line, end_line, text, file_sha256 } => {
            let rel = project.as_ref().and_then(|p| path.strip_prefix(p).ok()).map(|r| r.to_string_lossy().replace('\\', "/"));
            let shown = rel.clone().unwrap_or_else(|| home_short(&path.to_string_lossy()));
            let s = crate::secrets::scrub(&text);
            let excerpt = s.text.trim_end_matches('\n').to_string();
            let lang = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let fence = fence_for(&excerpt);
            let lines = if start_line == end_line { format!("{}", start_line + 1) } else { format!("{}-{}", start_line + 1, end_line + 1) };
            let md = format!("{}\n{fence}{lang}\n{excerpt}\n{fence}\n\n— `{shown}:{lines}` · {date}\n", model::marker(&id));
            let mut source = Source::new(Kind::File, &id, &format!("{shown}:{lines}"), now)
                .with("range", json!({"start_line": start_line + 1, "end_line": end_line + 1}))
                .with("coordinate_unit", json!("line_1_based"))
                .with("excerpt", json!(excerpt))
                .with("captured_sha256", json!(model::captured_sha256(&excerpt)))
                .with("redaction", json!({"applied": true, "masked_count": s.findings}));
            if let Some(r) = rel {
                source = source.with("relative_path", json!(r));
            }
            // This device's own path, a hint only: another device's note
            // resolves through the project, never this.
            source = source.with("device_path_hint", json!(path.to_string_lossy()));
            if let Some(h) = file_sha256 {
                source = source.with("content_sha256", json!(h));
            }
            Draft { request_id, kind: Kind::File, label: shown, markdown: md, source, masked: s.findings, omitted_lines: 0, incomplete: false, project }
        }
    })
}

/// A title for a new note made by a capture: what it captured.
pub fn title_for(d: &Draft) -> String {
    let t: String = d.label.chars().take(60).collect();
    if t.trim().is_empty() { "Captured".into() } else { t }
}

// --- where captures go --------------------------------------------------

/// The last note each project's captures went to, kept sealed in the
/// profile (it names your projects).
fn memory_path() -> PathBuf {
    crate::notes_session::profile().join("notes-destinations.json")
}

pub fn last_destination(project: Option<&Path>) -> Option<PathBuf> {
    let key = project.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = nus_vault::read_at(&crate::notes_session::profile(), &memory_path()).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    let p = PathBuf::from(v.get(&key)?.as_str()?);
    p.is_file().then_some(p)
}

pub fn remember_destination(project: Option<&Path>, note: &Path) {
    let key = project.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let profile = crate::notes_session::profile();
    let mut v: Value = nus_vault::read_at(&profile, &memory_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    if let Some(m) = v.as_object_mut() {
        m.insert(key, json!(note.to_string_lossy()));
        // A bounded memory: the most recent sixty-four projects.
        while m.len() > 64 {
            let Some(k) = m.keys().next().cloned() else { break };
            m.remove(&k);
        }
    }
    if let Err(e) = nus_vault::write_at(&profile, &memory_path(), v.to_string().as_bytes()) {
        tracing::warn!("notes: the capture destination could not be remembered: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_keeps_its_tail_masks_secrets_and_verifies() {
        let out: String = (0..300).map(|i| format!("line {i}\n")).collect::<String>() + "token=ghp_abcdefghijklmnopqrstuvwxyz0123\n";
        let d = draft(Origin::Block { cmd: "cargo test".into(), output: out, cwd: "/w".into(), exit: Some(101), running: false, shell: "zsh".into() }, 0).unwrap();
        assert!(d.masked >= 1);
        assert_eq!(d.omitted_lines, 101);
        assert!(!d.markdown.contains("ghp_"));
        assert!(d.markdown.contains("```nus-block cmd=\"cargo test\" exit=101 cwd=\"/w\" at=\"1970-01-01T00:00:00Z\""));
        assert!(d.markdown.contains("(101 earlier lines left out)"));
        let mut doc = model::Document::new(&"0".repeat(32), "t", true, 0);
        doc.body = d.markdown.clone();
        doc.push_source(d.source.clone());
        assert_eq!(model::evidence(&doc)[0].1, model::Evidence::Original);
        assert_eq!(d.source.get("command"), Some("cargo test"));
        assert!(d.summary().contains("101 earlier lines left out"));
    }

    #[test]
    fn a_running_block_says_so() {
        let d = draft(Origin::Block { cmd: "sleep 9".into(), output: "a\n".into(), cwd: String::new(), exit: None, running: true, shell: "sh".into() }, 0).unwrap();
        assert!(d.incomplete);
        assert!(d.markdown.contains(" running"));
        assert_eq!(d.source.0["complete"], json!(false));
    }

    #[test]
    fn a_block_holding_fences_gets_a_longer_one() {
        let d = draft(Origin::Block { cmd: "cat x.md".into(), output: "```rust\nfn main() {}\n```".into(), cwd: String::new(), exit: Some(0), running: false, shell: "sh".into() }, 0).unwrap();
        assert!(d.markdown.contains("\n````nus-block"));
        let mut doc = model::Document::new(&"0".repeat(32), "t", true, 0);
        doc.body = d.markdown.clone();
        doc.push_source(d.source.clone());
        assert_eq!(model::evidence(&doc)[0].1, model::Evidence::Original);
    }

    #[test]
    fn a_page_quote_binds_and_a_link_is_just_a_link() {
        let d = draft(Origin::Page { url: "https://user:pw@example.com/a?q=1#f".into(), title: "Ex [ample]".into(), quote: "one\n\ntwo".into(), container: "PERSONAL".into() }, 0).unwrap();
        assert_eq!(d.source.get("url"), Some("https://example.com/a?q=1#f"));
        assert!(d.masked >= 1);
        assert!(d.markdown.contains("> one\n>\n> two\n\n— [Ex ample](https://example.com/a?q=1#f)"));
        let mut doc = model::Document::new(&"0".repeat(32), "t", true, 0);
        doc.body = d.markdown.clone();
        doc.push_source(d.source.clone());
        assert_eq!(model::evidence(&doc)[0].1, model::Evidence::Original);
        let l = draft(Origin::Page { url: "https://example.com".into(), title: String::new(), quote: String::new(), container: "PERSONAL".into() }, 0).unwrap();
        assert!(l.markdown.starts_with("- [https://example.com](https://example.com)"));
        assert!(!l.markdown.contains("nus:source"));
    }

    #[test]
    fn a_reading_note_pins_its_saved_copy() {
        let d = draft(Origin::Reading { library_id: "47a582414b6beea70e5213419b71bfc1".into(), source_url: "https://example.com/article".into(), title: "An article".into(), snapshot: Some("b".repeat(64)), annotation: "the part that mattered".into(), container: "PERSONAL".into() }, 0).unwrap();
        assert_eq!(d.source.get("snapshot_sha256"), Some("b".repeat(64).as_str()));
        assert!(d.markdown.contains("> the part that mattered\n\n— [An article](https://example.com/article) · from the reading list"));
        let mut doc = model::Document::new(&"0".repeat(32), "t", true, 0);
        doc.body = d.markdown.clone();
        doc.push_source(d.source.clone());
        assert_eq!(model::evidence(&doc)[0].1, model::Evidence::Original);
    }

    #[test]
    fn a_file_capture_is_relative_to_its_project() {
        let d = draft(Origin::File { path: "/w/p/src/a.rs".into(), project: Some("/w/p".into()), start_line: 9, end_line: 11, text: "fn a() {}\n".into(), file_sha256: None }, 0).unwrap();
        assert_eq!(d.source.get("relative_path"), Some("src/a.rs"));
        assert!(d.markdown.contains("```rs\nfn a() {}\n```\n\n— `src/a.rs:10-12`"));
        assert_eq!(d.label, "src/a.rs");
    }

    #[test]
    fn why_goes_first_and_is_optional() {
        let d = draft(Origin::ShellSelection { text: "x".into(), cwd: String::new() }, 0).unwrap();
        assert_eq!(d.with_why("  "), d.markdown);
        assert!(d.with_why("because").starts_with("because\n\n<!-- nus:source "));
    }

    #[test]
    fn tails_are_whole_lines_under_both_limits() {
        assert_eq!(tail("a\nb\nc\n", 2, 100), ("b\nc".into(), 1));
        assert_eq!(tail("aaaa\nbbbb\ncc", 10, 6), ("cc".into(), 2));
    }
}
