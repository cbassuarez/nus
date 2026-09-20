//! The profile's files, written and read so that no version of nus can
//! take another's settings away.
//!
//! Writing is atomic: the bytes go to a sibling temp file, then a rename
//! puts them in place, so a crash mid-write leaves the old file whole
//! rather than a truncated one. Reading a typed file is forgiving: when
//! the whole file does not parse — a variant this build does not know, a
//! field a newer nus renamed, a hand edit gone wrong — it is taken a key
//! at a time, keeping every key that parses on its own and naming the
//! ones that did not, instead of falling back to the defaults for all of
//! them. The unreadable original is kept beside the file, once, so
//! nothing is lost when the next save writes the salvaged version.

use serde::de::DeserializeOwned;
use std::path::Path;

/// Write `bytes` to `path` through a temp file and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// `write_atomic`, for a value as pretty JSON.
pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)
}

/// What a read of a typed file found: the value, and the keys that had
/// to be left out (with the path to them, like `behavior.lead`).
pub struct Read<T> {
    pub value: T,
    pub dropped: Vec<String>,
}

/// Read `path` as a `T`. Missing or empty: the default, nothing dropped.
/// Whole: the value. Otherwise salvaged, key by key, and the original
/// kept as `<name>.unread` beside it (the first time only).
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Read<T> {
    let Ok(text) = std::fs::read_to_string(path) else { return Read { value: T::default(), dropped: Vec::new() } };
    if text.trim().is_empty() {
        return Read { value: T::default(), dropped: Vec::new() };
    }
    match serde_json::from_str::<T>(&text) {
        Ok(value) => Read { value, dropped: Vec::new() },
        Err(whole) => {
            let (value, mut dropped) = salvage::<T>(&text);
            if dropped.is_empty() {
                dropped.push(format!("({whole})"));
            }
            let keep = path.with_extension("unread");
            if !keep.exists() {
                let _ = std::fs::copy(path, &keep);
            }
            tracing::warn!("{}: kept what parsed, left out {}", path.display(), dropped.join(", "));
            Read { value, dropped }
        }
    }
}

/// The file a key at a time: an object's keys are tried one by one on
/// top of what already parsed; a key whose value is itself an object gets
/// the same treatment one level down, so one bad field inside `behavior`
/// costs that field, not `behavior`.
fn salvage<T: DeserializeOwned + Default>(text: &str) -> (T, Vec<String>) {
    let mut dropped = Vec::new();
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else {
        return (T::default(), vec!["(not json)".into()]);
    };
    let Some(obj) = root.as_object() else {
        return (T::default(), vec!["(not an object)".into()]);
    };
    let parses = |v: &serde_json::Value| serde_json::from_value::<T>(v.clone()).is_ok();
    let mut kept = serde_json::Value::Object(Default::default());
    for (key, value) in obj {
        let mut candidate = kept.clone();
        candidate[key] = value.clone();
        if parses(&candidate) {
            kept = candidate;
            continue;
        }
        if let Some(inner) = value.as_object() {
            let mut sub = serde_json::Value::Object(Default::default());
            for (ik, iv) in inner {
                let mut c2 = kept.clone();
                let mut s2 = sub.clone();
                s2[ik] = iv.clone();
                c2[key] = s2.clone();
                if parses(&c2) {
                    sub = s2;
                } else {
                    dropped.push(format!("{key}.{ik}"));
                }
            }
            let mut c3 = kept.clone();
            c3[key] = sub;
            if parses(&c3) {
                kept = c3;
            } else {
                dropped.push(key.clone());
            }
        } else {
            dropped.push(key.clone());
        }
    }
    (serde_json::from_value(kept).unwrap_or_default(), dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    #[serde(default)]
    struct Inner {
        lead: String,
        count: u32,
        mode: Mode,
    }
    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    enum Mode {
        #[default]
        A,
        B,
    }
    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    struct Prefs {
        behavior: Option<Inner>,
        pinned: Option<bool>,
        name: Option<String>,
    }

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nus-store-{}-{}", std::process::id(), crate::clock::now().elapsed().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_whole_file_reads_whole() {
        let d = dir();
        let p = d.join("settings.json");
        write_json(&p, &Prefs { behavior: Some(Inner { lead: "x".into(), count: 2, mode: Mode::B }), pinned: Some(true), name: None }).unwrap();
        let r = read_json::<Prefs>(&p);
        assert!(r.dropped.is_empty());
        assert_eq!(r.value.pinned, Some(true));
        assert_eq!(r.value.behavior.unwrap().count, 2);
        assert!(!d.join(".settings.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn one_unknown_variant_costs_one_key() {
        let d = dir();
        let p = d.join("settings.json");
        // `mode: "Z"` is from a nus this one does not know; `pinned` is a string.
        std::fs::write(&p, r#"{"behavior":{"lead":"keep","count":7,"mode":"Z"},"pinned":"yes","name":"win"}"#).unwrap();
        let r = read_json::<Prefs>(&p);
        assert_eq!(r.dropped, vec!["behavior.mode".to_string(), "pinned".to_string()]);
        let b = r.value.behavior.unwrap();
        assert_eq!((b.lead.as_str(), b.count, b.mode), ("keep", 7, Mode::A));
        assert_eq!(r.value.pinned, None);
        assert_eq!(r.value.name.as_deref(), Some("win"));
        assert!(d.join("settings.unread").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn not_json_is_the_default() {
        let d = dir();
        let p = d.join("settings.json");
        std::fs::write(&p, "{not json").unwrap();
        let r = read_json::<Prefs>(&p);
        assert_eq!(r.value, Prefs::default());
        assert_eq!(r.dropped, vec!["(not json)".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
