//! The UI-visible failure boundary for sensitive nus-owned persistence.
use std::{path::Path, sync::Mutex};
static ERROR: Mutex<Option<String>> = Mutex::new(None);
static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn record<T>(result: std::io::Result<T>) -> std::io::Result<T> {
    if let Err(e) = &result {
        if e.kind() != std::io::ErrorKind::NotFound {
            let mut message = ERROR.lock().unwrap();
            if message.is_none() {
                PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            *message=Some("Sensitive state could not be saved or unlocked. Check the OS keychain. Existing encrypted files are preserved; plaintext saving is disabled.".into());
        }
    }
    result
}
pub fn status() -> String {
    ERROR.lock().unwrap().clone().unwrap_or_else(||"Sensitive nus state uses authenticated encryption and an OS-keychain key. External agent files and explicit exports are outside this vault.".into())
}
pub fn take_notice() -> Option<String> {
    PENDING
        .swap(false, std::sync::atomic::Ordering::Relaxed)
        .then(status)
}
pub fn ready(profile: &Path) -> std::io::Result<()> {
    record(nus_vault::available(profile))
}
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    record(nus_vault::read(path))
}
pub fn read_text(path: &Path) -> std::io::Result<String> {
    record(nus_vault::read_text(path))
}
pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    record(nus_vault::write(path, bytes))
}
pub fn update(path: &Path, transform: impl FnOnce(&[u8]) -> std::io::Result<Vec<u8>>) -> std::io::Result<()> {
    record(nus_vault::update(path, transform))
}
pub fn write_json(path: &Path, value: &impl serde::Serialize) -> std::io::Result<()> {
    write(path, &serde_json::to_vec(value)?)
}

/// Run once in the primary process before loading sessions or starting writers.
/// Existing encrypted records are left byte-for-byte intact. Symlinks are never
/// traversed, and migration never creates a plaintext backup.
pub fn migrate(profile: &Path) {
    if crate::private::enabled() {
        return;
    }
    if ready(profile).is_err() || profile.join(".vault-format").is_file() {
        return;
    }
    let mut paths = vec![];
    for name in [
        "session.json",
        "recent.json",
        "memory.md",
        "sync/key",
        "sync/forge.token",
    ] {
        let path = profile.join(name);
        if path.symlink_metadata().is_ok_and(|m| m.is_file()) {
            paths.push(path);
        }
    }
    let mut dirs = vec![
        profile.join("journal"),
        profile.join("replay"),
        profile.join("hold"),
    ];
    while let Some(dir) = dirs.pop() {
        if !dir.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|e| matches!(e.to_str(), Some("json" | "jsonl" | "cast" | "png")))
            {
                paths.push(entry.path());
            }
        }
    }
    let mut complete = true;
    for path in paths {
        if record(nus_vault::read_at(profile, &path)).is_err() {
            complete = false;
        }
    }
    if complete {
        let _ = record(nus_vault::finish_migration(profile));
    }
}

/// Only the current profile's protected records receive special editor I/O.
/// A path in the form `canonicalize` gives, even when it does not exist yet:
/// the nearest existing folder canonicalized, the rest as written. On
/// Windows a canonical path is verbatim (`\\?\C:\…`), so a file about to be
/// written (a new note) would otherwise never match its canonical profile.
fn canonical(path: &Path) -> std::path::PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => canonical(parent).join(name),
        _ => path.to_path_buf(),
    }
}

pub fn is_private_path(path: &Path) -> bool {
    let root = canonical(&std::env::current_dir().unwrap_or_default().join("profile"));
    let path = canonical(path);
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    matches!(
        rel.to_str(),
        Some("session.json" | "recent.json" | "memory.md" | "sync/key" | "sync/forge.token" | "passwords.json")
    ) || ["journal", "replay", "hold"]
        .iter()
        .any(|dir| rel.starts_with(dir))
        // Personal notes are sealed like memory.md, and so is everything
        // kept beside them (history, recovery, the index of imports); a
        // project's notes are its own files and stay plain.
        || rel.starts_with("notes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_not_written_yet_compares_like_its_folder() {
        let dir = std::env::temp_dir().join(format!("nus-canonical-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = canonical(&dir);
        // A note about to be written, two folders that do not exist yet down.
        let new = canonical(&dir.join("notes").join(".state").join("a.md"));
        assert!(new.starts_with(&root), "{new:?} is under {root:?}");
        assert_eq!(new.strip_prefix(&root).unwrap(), Path::new("notes").join(".state").join("a.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}