//! Aggregate retention for disposable Chromium caches, only after CEF stops.
//! Cookies, storage, databases, service workers and user files are never candidates.
use std::path::{Path, PathBuf};

pub const BUDGET: u64 = 256 * 1024 * 1024;
const NAMES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "GPUPersistentCache",
    "DawnCache",
    "ShaderCache",
    "GrShaderCache",
    "GraphiteDawnCache",
];

fn directory(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn size_age(path: &Path) -> (u64, std::time::SystemTime) {
    let mut bytes = 0;
    let mut newest = std::time::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(path) else {
        return (bytes, newest);
    };
    for e in entries.flatten() {
        let Ok(m) = e.path().symlink_metadata() else {
            continue;
        };
        if m.file_type().is_symlink() {
            continue;
        }
        if m.is_dir() {
            let (n, at) = size_age(&e.path());
            bytes += n;
            newest = newest.max(at);
        } else if m.is_file() {
            bytes += m.len();
            newest = newest.max(m.modified().unwrap_or(std::time::UNIX_EPOCH));
        }
    }
    (bytes, newest)
}

pub fn maintain(root: &Path, budget: u64) {
    if !directory(root) {
        return;
    }
    let mut profiles = vec![root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(root) {
        profiles.extend(
            entries
                .flatten()
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("container-")
                        && directory(&e.path())
                })
                .map(|e| e.path()),
        );
    }
    let mut caches: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    for profile in profiles {
        for base in [profile.clone(), profile.join("Default")] {
            if !directory(&base) {
                continue;
            }
            for name in NAMES {
                let path = base.join(name);
                if directory(&path) {
                    let (bytes, age) = size_age(&path);
                    caches.push((path, bytes, age));
                }
            }
        }
    }
    caches.sort_by_key(|(_, _, age)| *age);
    let mut total: u64 = caches.iter().map(|(_, bytes, _)| bytes).sum();
    for (path, bytes, _) in caches {
        if total <= budget {
            break;
        }
        // Remove whole stopped cache stores, never individual live database files.
        if std::fs::remove_dir_all(path).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_growth_is_bounded_across_containers_and_preserves_site_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let protected = [
            "Default/Cookies",
            "Default/IndexedDB/data",
            "Default/Local Storage/data",
            "Default/Service Worker/CacheStorage/data",
            "container-work/Default/Cookies",
            "downloads/keep",
        ];
        for name in protected {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"keep").unwrap();
        }
        for _ in 0..20 {
            for name in [
                "Default/Cache/Cache_Data/data",
                "Default/Code Cache/js/data",
                "container-work/Default/Cache/data",
            ] {
                let path = root.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, vec![0; 1024]).unwrap();
            }
            maintain(root, 1500);
            for name in protected {
                assert_eq!(std::fs::read(root.join(name)).unwrap(), b"keep");
            }
            assert!(size_age(root).0 <= 1500 + 4 * protected.len() as u64);
        }
    }
    #[cfg(unix)]
    #[test]
    fn does_not_follow_linked_profiles_or_caches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("profile");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(root.join("Default")).unwrap();
        std::fs::create_dir_all(outside.join("Cache")).unwrap();
        std::fs::write(outside.join("Cache/keep"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("container-linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("Cache"), root.join("Default/Cache")).unwrap();
        maintain(&root, 0);
        assert_eq!(std::fs::read(outside.join("Cache/keep")).unwrap(), b"keep");
    }
}
