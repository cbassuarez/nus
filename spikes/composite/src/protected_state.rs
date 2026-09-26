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
pub fn is_private_path(path: &Path) -> bool {
    let root = std::env::current_dir().unwrap_or_default().join("profile");
    let root = root.canonicalize().unwrap_or(root);
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    matches!(
        rel.to_str(),
        Some("session.json" | "recent.json" | "memory.md" | "sync/key" | "sync/forge.token" | "passwords.json")
    ) || ["journal", "replay", "hold"]
        .iter()
        .any(|dir| rel.starts_with(dir))
        // Profile notes are sealed like memory.md (notes.rs); folder notes
        // are the project's own files and stay plain.
        || rel.parent() == Some(Path::new("notes")) && rel.extension().is_some_and(|e| e == "md")
}
