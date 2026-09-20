//! The journal: one line per finished block, per folder. What ran here,
//! when, how long, how it ended — across restarts. `profile/journal/
//! <slug>.jsonl`, one file per cwd; `nus log`, the palette's `log` rows,
//! and a page per folder read it back. Pruned to KEEP days on append.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nus_render::theme::Theme;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub cmd: String,
    pub cwd: String,
    /// Unix seconds when the command started.
    pub start: u64,
    pub ms: u64,
    pub exit: Option<i32>,
    pub tab: String,
    pub shell: String,
}

fn dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("journal")
}

/// A file name from a folder: `C:\Users\seb\nus` → `C--Users-seb-nus`.
pub fn slug(cwd: &str) -> String {
    let s: String = cwd.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "root".into() } else { s }
}

fn path_for(cwd: &str) -> PathBuf {
    dir().join(format!("{}.jsonl", slug(cwd)))
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn to_json(e: &Entry) -> String {
    serde_json::json!({ "cmd": e.cmd, "cwd": e.cwd, "start": e.start, "ms": e.ms, "exit": e.exit, "tab": e.tab, "shell": e.shell }).to_string()
}

fn from_json(line: &str) -> Option<Entry> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(Entry {
        cmd: v.get("cmd")?.as_str()?.to_string(),
        cwd: v.get("cwd").and_then(|c| c.as_str()).unwrap_or("").to_string(),
        start: v.get("start").and_then(|s| s.as_u64()).unwrap_or(0),
        ms: v.get("ms").and_then(|s| s.as_u64()).unwrap_or(0),
        exit: v.get("exit").and_then(|e| e.as_i64()).map(|e| e as i32),
        tab: v.get("tab").and_then(|t| t.as_str()).unwrap_or("").to_string(),
        shell: v.get("shell").and_then(|t| t.as_str()).unwrap_or("").to_string(),
    })
}

/// Append one finished block. Every so often the file is pruned to
/// `keep_days`; a file that is all old lines goes away.
pub fn append(e: &Entry, keep_days: u32) {
    let path = path_for(&e.cwd);
    let _ = crate::storage::append_line(&path, &to_json(e), crate::storage::HISTORY_FILE, 4000);
    static LAST: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, u64>>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut last = LAST.lock().unwrap();
    let now = now();
    if last.get(&path).is_none_or(|t| now.saturating_sub(*t) >= 60) {
        if last.len() >= 128 { last.clear(); }
        last.insert(path.clone(), now);
        prune(&path, keep_days);
    }
}

fn prune(path: &PathBuf, keep_days: u32) {
    let _guard = crate::storage::FILE_ACCESS.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(text) = crate::storage::tail(path, crate::storage::HISTORY_FILE) else { return };
    let cutoff = now().saturating_sub(keep_days as u64 * 86_400);
    let kept: Vec<&str> = text.lines().filter(|l| from_json(l).is_some_and(|e| e.start >= cutoff)).collect();
    if kept.len() == text.lines().count() {
        return;
    }
    if kept.is_empty() {
        let _ = std::fs::remove_file(path);
    } else {
        let _ = crate::store::write_atomic(path, (kept.join("\n") + "\n").as_bytes());
    }
}

/// The folder's entries, newest first, at most `limit`.
pub fn entries(cwd: &str, limit: usize) -> Vec<Entry> {
    let Ok(text) = crate::storage::tail(&path_for(cwd), crate::storage::HISTORY_FILE) else { return Vec::new() };
    let mut v: Vec<Entry> = text.lines().filter_map(from_json).collect();
    v.reverse();
    v.truncate(limit);
    v
}

/// Every command that started since `since` (unix seconds), in every
/// folder, newest first.
pub fn since(since: u64) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return Vec::new() };
    let mut out: Vec<Entry> = Vec::new();
    for file in rd.flatten() {
        let Ok(text) = crate::storage::tail(&file.path(), crate::storage::HISTORY_FILE) else { continue; };
        for entry in text.lines().filter_map(from_json).filter(|e| e.start >= since) {
            let at = out.partition_point(|e| e.start >= entry.start);
            if at < 200 { out.insert(at, entry); out.truncate(200); }
        }
    }
    out
}

/// How many commands ran in this folder since `since` (unix seconds).
pub fn count_since(cwd: &str, since: u64) -> usize {
    let Ok(text) = crate::storage::tail(&path_for(cwd), crate::storage::HISTORY_FILE) else { return 0 };
    text.lines().filter_map(from_json).filter(|e| e.start >= since).count()
}

/// Every folder with a journal: (cwd, entries), most recent folder first.
pub fn folders() -> Vec<(String, usize)> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return Vec::new() };
    let mut out: Vec<(String, usize, u64)> = rd
        .flatten()
        .filter_map(|e| {
            let text = crate::storage::tail(&e.path(), crate::storage::HISTORY_FILE).ok()?;
            let mut last = 0;
            let mut cwd = String::new();
            let mut n = 0;
            for l in text.lines().filter_map(from_json) {
                n += 1;
                last = last.max(l.start);
                if cwd.is_empty() {
                    cwd = l.cwd;
                }
            }
            (n > 0).then_some((cwd, n, last))
        })
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out.into_iter().map(|(c, n, _)| (c, n)).collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn hex(c: nus_render::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

/// `1.2s`, `340ms`, `2m 05s`.
pub fn took(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f32 / 1000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, (ms / 1000) % 60)
    }
}

/// `today 14:02`, `yesterday 09:10`, `3 days ago`. UTC clock, like the block page.
pub fn when(start: u64) -> String {
    let n = now();
    let days = (n / 86_400).saturating_sub(start / 86_400);
    let (h, mi) = ((start / 3600) % 24, (start / 60) % 60);
    match days {
        0 => format!("today {h:02}:{mi:02}"),
        1 => format!("yesterday {h:02}:{mi:02}"),
        d => format!("{d} days ago"),
    }
}

/// The folder's journal as a Broadsheet page: a ledger, newest first.
pub fn page_html(cwd: &str, entries: &[Entry], theme: &Theme, signal: nus_render::Color) -> String {
    let (paper, ink, dim) = (hex(theme.paper), hex(theme.ink), hex(theme.dim));
    let sig = hex(signal);
    let rows: String = entries
        .iter()
        .map(|e| {
            let lamp = match e.exit {
                Some(0) => "ok",
                Some(_) => "bad",
                None => "",
            };
            format!(
                "<tr><td class=\"when\">{}</td><td><span class=\"lamp {}\"></span>{}</td><td class=\"took\">{}</td><td class=\"exit\">{}</td></tr>",
                when(e.start),
                lamp,
                esc(e.cmd.trim()),
                took(e.ms),
                e.exit.map(|x| x.to_string()).unwrap_or_default()
            )
        })
        .collect();
    let (ran, failed) = (entries.len(), entries.iter().filter(|e| e.exit.is_some_and(|x| x != 0)).count());
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>log · {title}</title>
<style>
:root {{ --paper:{paper}; --ink:{ink}; --dim:{dim}; --signal:{sig}; }}
html,body {{ margin:0; background:var(--paper); color:var(--ink); }}
body {{ font-family:"IBM Plex Mono","Cascadia Mono",Consolas,monospace; font-size:13px; line-height:1.5; padding:28px 34px; }}
.band {{ position:fixed; left:0; top:0; right:0; height:6px; background:var(--signal); }}
h1 {{ font-family:"Newsreader","Times New Roman",serif; font-style:italic; font-weight:400; font-size:30px; margin:8px 0 6px; }}
.dateline {{ font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--dim); display:flex; gap:14px; border-top:1.5px solid var(--ink); border-bottom:1px solid var(--ink); padding:8px 0; margin:10px 0 18px; }}
table {{ border-collapse:collapse; width:100%; }}
td {{ padding:5px 10px 5px 0; border-bottom:1px solid color-mix(in srgb, var(--ink) 12%, transparent); vertical-align:top; }}
td.when, td.took, td.exit {{ color:var(--dim); white-space:nowrap; font-size:12px; }}
td.took, td.exit {{ text-align:right; }}
.lamp {{ display:inline-block; width:8px; height:8px; background:var(--dim); margin-right:8px; vertical-align:-1px; }}
.lamp.ok {{ background:#2e7d32; }} .lamp.bad {{ background:var(--signal); }}
.foot {{ margin-top:22px; border-top:1px solid var(--ink); padding-top:8px; font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--dim); }}
</style></head><body>
<div class="band"></div>
<h1>log</h1>
<div class="dateline"><span>{cwd}</span><span>·</span><span>{ran} commands</span><span>·</span><span>{failed} failed</span></div>
<table>{rows}</table>
<div class="foot">the journal from nus · profile/journal · newest first</div>
</body></html>"#,
        title = esc(cwd),
        cwd = esc(cwd),
    )
}

/// Write the page under profile/blocks/ (the block pages' home) and return its path.
pub fn write_page(html: &str) -> Option<PathBuf> {
    let dir = std::env::current_dir().ok()?.join("profile").join("blocks");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("log-{}.html", now()));
    std::fs::write(&path, html).ok()?;
    Some(path)
}

/// Is this a command that took a while — worth a line even so? Everything
/// is; this exists so a future filter has one place to live.
pub fn worth(e: &Entry) -> bool {
    !e.cmd.trim().is_empty()
}

/// A command as one line, the grid's wraps removed.
pub fn oneline(cmd: &str) -> String {
    cmd.chars().filter(|c| !matches!(c, '\n' | '\r')).collect::<String>().trim().to_string()
}

#[allow(dead_code)]
pub fn ago(d: Duration) -> String {
    took(d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_and_json_roundtrip() {
        assert_eq!(slug("C:\\Users\\seb\\nus"), "C--Users-seb-nus");
        assert_eq!(slug("/home/seb/proj.x"), "home-seb-proj.x");
        let e = Entry { cmd: "cargo test".into(), cwd: "/x".into(), start: 1000, ms: 2500, exit: Some(0), tab: "01".into(), shell: "pwsh".into() };
        assert_eq!(from_json(&to_json(&e)), Some(e));
        assert_eq!(took(340), "340ms");
        assert_eq!(took(2500), "2.5s");
        assert_eq!(took(125_000), "2m 05s");
    }

    #[test]
    fn page_shape() {
        let e = Entry { cmd: "ls <x>".into(), cwd: "/x".into(), start: now(), ms: 10, exit: Some(1), tab: "".into(), shell: "".into() };
        let h = page_html("/x", &[e], &Theme::ink(), [1.0, 0.0, 0.0, 1.0]);
        assert!(h.contains("lamp bad"));
        assert!(h.contains("ls &lt;x&gt;"));
        assert!(h.contains("1 failed"));
    }
}
