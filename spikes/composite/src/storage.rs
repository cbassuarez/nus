//! Budgets for generated history and disposable files. Never walks user
//! downloads, projects, cookies, IndexedDB, or other site-owned data.
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub static FILE_ACCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub const MIB: u64 = 1024 * 1024;
pub const REPLAY_SEGMENT: u64 = 4 * MIB;
pub const REPLAY_WINDOW: u64 = 32 * MIB;
pub const HISTORY_FILE: u64 = MIB;
pub const HISTORY_LINES: usize = 2000;
pub const CLOSED_TABS: usize = 50;

/// Read a bounded tail, discarding the partial first line. A large historic
/// file must not first be loaded into RAM just to truncate its entries.
pub fn tail(path: &Path, limit: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let skipped = len.saturating_sub(limit);
    file.seek(SeekFrom::Start(skipped))?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    if skipped > 0 {
        if let Some(end) = bytes.iter().position(|b| *b == b'\n') { bytes.drain(..=end); }
        else { bytes.clear(); }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn append_line(path: &Path, line: &str, limit: u64, lines: usize) -> std::io::Result<()> {
    let _guard = FILE_ACCESS.lock().unwrap_or_else(|e| e.into_inner());
    // One pathological command cannot become an oversized history record.
    if line.len() as u64 + 1 > limit / 4 { return Ok(()); }
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    let size = file.metadata()?.len();
    drop(file);
    // Amortize compaction; the byte ceiling is enforced on every append.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    if size >= limit || WRITES.fetch_add(1, Ordering::Relaxed) % 128 == 0 {
        let text = tail(path, limit)?;
        let mut kept: Vec<_> = text.lines().rev().take(lines).collect();
        kept.reverse();
        crate::store::write_atomic(path, (kept.join("\n") + "\n").as_bytes())?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct Entry { pub path: PathBuf, pub bytes: u64, pub modified: u64 }
pub fn files(root: &Path, recursive: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return out };
    for entry in entries.flatten() {
        let Ok(meta) = entry.path().symlink_metadata() else { continue };
        if meta.file_type().is_symlink() { continue; }
        if meta.is_dir() && recursive { out.extend(files(&entry.path(), false)); }
        else if meta.is_file() {
            out.push(Entry { path: entry.path(), bytes: meta.len(), modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()) });
        }
    }
    out
}

pub fn prune_files(mut entries: Vec<Entry>, budget: u64, cutoff: u64) {
    let _guard = FILE_ACCESS.lock().unwrap_or_else(|e| e.into_inner());
    // Refresh under the same lock used by appenders; a queued sweep must not
    // delete a history file just written since it collected its candidates.
    entries.retain_mut(|e| {
        let Ok(m) = e.path.symlink_metadata() else {return false;};
        if !m.is_file() {return false;}
        e.bytes=m.len();e.modified=m.modified().ok().and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0,|d|d.as_secs());true
    });
    entries.sort_by_key(|e| e.modified);
    let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
    for e in entries {
        if (e.modified < cutoff || total > budget) && std::fs::remove_file(&e.path).is_ok() { total = total.saturating_sub(e.bytes); }
    }
}

/// Run off the input/render thread, once a minute across all windows.
pub fn tick(active_replays: Vec<PathBuf>, journal_days: u32, replay_days: u32) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = crate::journal::now();
    if now.saturating_sub(LAST.load(Ordering::Relaxed)) < 60 || BUSY.swap(true, Ordering::AcqRel) { return; }
    LAST.store(now, Ordering::Relaxed);
    let root = PathBuf::from("profile");
    std::thread::spawn(move || {
        maintain(&root, &active_replays, journal_days, replay_days, now);
        BUSY.store(false, Ordering::Release);
    });
}

pub fn maintain(root: &Path, active: &[PathBuf], journal_days: u32, replay_days: u32, now: u64) {
    for (dir, extension, budget, days) in [("blocks", "html", 16 * MIB, 7), ("journal", "jsonl", 8 * MIB, journal_days), ("history", "txt", 4 * MIB, 90)] {
        let entries = files(&root.join(dir), false).into_iter().filter(|e| e.path.extension().is_some_and(|x| x == extension)).collect();
        prune_files(entries, budget, now.saturating_sub(days as u64 * 86400));
    }
    let replay = root.join("replay");
    let mut sessions = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&replay) {
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) || e.file_name().to_string_lossy().parse::<u64>().is_err() { continue; }
            let path = e.path();
            if active.iter().any(|a| a == &path || a.canonicalize().ok() == path.canonicalize().ok()) { continue; }
            let entries = files(&path, true);
            let size = entries.iter().map(|e| e.bytes).sum::<u64>();
            let modified = entries.iter().map(|e| e.modified).max().unwrap_or(0);
            sessions.push(Entry { path, bytes: size, modified });
        }
    }
    sessions.sort_by_key(|e| e.modified);
    let mut total: u64 = sessions.iter().map(|e| e.bytes).sum();
    let cutoff = now.saturating_sub(replay_days.max(1) as u64 * 86400);
    for e in sessions {
        if (e.modified < cutoff || total > 128 * MIB) && std::fs::remove_dir_all(&e.path).is_ok() { total = total.saturating_sub(e.bytes); }
    }
    // Chromium file logging is disabled. Bound legacy logs from older builds.
    for path in [root.join("debug.log"), root.join("chrome_debug.log"), root.join("pses.log"), root.parent().unwrap_or(root).join("debug.log")] {
        if path.symlink_metadata().is_ok_and(|m| m.is_file() && m.len() > MIB) {
            if let Ok(text) = tail(&path, MIB / 2) { let _ = crate::store::write_atomic(&path, text.as_bytes()); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_history_is_bounded_before_loading_and_after_append() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("history.txt");
        std::fs::write(&path, "old\n".repeat(1000)).unwrap();
        append_line(&path, "new", 128, 5).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.len() <= 128 && text.lines().count() <= 5 && text.ends_with("new\n"));
        std::fs::write(&path, "x".repeat(10000)).unwrap();
        assert!(tail(&path, 128).unwrap().is_empty());
    }
    #[test]
    fn cleanup_preserves_user_data_and_active_replays() {
        let dir = tempfile::tempdir().unwrap(); let root = dir.path();
        for name in ["downloads/keep.pdf", "Default/Cookies", "blocks/custom.txt", "replay/1/tab-1.cast", "replay/2/tab-2.cast"] {
            let path = root.join(name); std::fs::create_dir_all(path.parent().unwrap()).unwrap(); std::fs::write(path, "keep").unwrap();
        }
        maintain(root, &[root.join("replay/2")], 7, 7, u64::MAX);
        for name in ["downloads/keep.pdf", "Default/Cookies", "blocks/custom.txt", "replay/2/tab-2.cast"] { assert!(root.join(name).exists(), "{name}"); }
        assert!(!root.join("replay/1").exists());
    }
}
