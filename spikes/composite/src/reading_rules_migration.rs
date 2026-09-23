//! Narrow, lossless removal of the old *bundled* reading shelf. This module has
//! no Lua runtime or App dependency, so its real implementation is unit-tested.
use std::{fs, io::{self, Write}, path::Path};

pub const LEGACY_TABLE: &str = "folders = {\n  [\"Starter references\"] = {\n    { title = \"Rust standard library\", url = \"https://doc.rust-lang.org/std/\", detail = \"Rust project · API reference\" },\n    { title = \"MDN Web Docs\", url = \"https://developer.mozilla.org/\", detail = \"MDN · web platform reference\" },\n  },\n}";

/// Long Lua strings/comments may use any number of '=' signs.
fn long_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'[') { return None; }
    let mut at = start + 1;
    while bytes.get(at) == Some(&b'=') { at += 1; }
    if bytes.get(at) != Some(&b'[') { return None; }
    let equals = at - start - 1;
    at += 1;
    while at < bytes.len() {
        if bytes[at] == b']' && bytes.get(at + 1..at + 1 + equals)
            .is_some_and(|s| s.iter().all(|b| *b == b'='))
            && bytes.get(at + 1 + equals) == Some(&b']') {
            return Some(at + equals + 2);
        }
        at += 1;
    }
    Some(bytes.len()) // Unclosed strings are opaque, never migration targets.
}

/// A conservative lexical guard, not a Lua rewriter. It only approves the exact
/// stock table at top level, never a matching example inside a comment/string or
/// a nested table/function. Ambiguity means leave the file alone.
fn top_level_code_at(source: &str, target: usize) -> bool {
    let b = source.as_bytes();
    let (mut at, mut brackets, mut blocks) = (0usize, 0i32, 0i32);
    while at < target {
        if b.get(at..at + 2) == Some(b"--") {
            at = if let Some(end) = long_end(b, at + 2) { end }
                 else { b[at..].iter().position(|c| *c == b'\n').map_or(b.len(), |n| at + n + 1) };
        } else if matches!(b[at], b'\'' | b'"' | b'`') {
            let quote = b[at]; at += 1;
            while at < b.len() {
                if b[at] == b'\\' { at = (at + 2).min(b.len()); }
                else if b[at] == quote { at += 1; break; }
                else { at += 1; }
            }
        } else if let Some(end) = long_end(b, at) {
            at = end;
        } else if b[at].is_ascii_alphabetic() || b[at] == b'_' {
            let start = at;
            while at < b.len() && (b[at].is_ascii_alphanumeric() || b[at] == b'_') { at += 1; }
            match &source[start..at] {
                "function" | "then" | "do" | "repeat" => blocks += 1,
                "end" | "until" | "elseif" => blocks -= 1,
                _ => {},
            }
            if blocks < 0 { return false; }
        } else {
            match b[at] {
                b'{' | b'(' | b'[' => brackets += 1,
                b'}' | b')' | b']' => brackets -= 1,
                _ => {},
            }
            if brackets < 0 { return false; }
            at += 1;
        }
    }
    at == target && brackets == 0 && blocks == 0
}

pub fn migrate(source: &str) -> Option<String> {
    let crlf = LEGACY_TABLE.replace('\n', "\r\n");
    let mut candidates = Vec::new();
    for table in [LEGACY_TABLE, crlf.as_str()] {
        for (at, _) in source.match_indices(table) {
            let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
            if !source[line_start..at].trim().is_empty() { continue; }
            let end = at + table.len();
            let next_line = source[end..].split('\n').next().unwrap_or("");
            if !next_line.trim().is_empty() { continue; }
            if top_level_code_at(source, at) { candidates.push((at, end)); }
        }
    }
    if candidates.len() != 1 { return None; }
    let (start, end) = candidates[0];
    let mut result = source.to_owned();
    result.replace_range(start..end, "folders = {}");
    Some(result)
}

/// Backup publication and replacement both use sibling temp files. The backup
/// is never clobbered. A concurrent edit detected before replacement is refused.
/// External editors which ignore locks are not a transactional filesystem API.
pub fn migrate_file(path: &Path) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() { return Ok(false); }
    let original = fs::read_to_string(path)?;
    let Some(updated) = migrate(&original) else { return Ok(false); };
    let parent = path.parent().ok_or_else(|| io::Error::other("Rules path has no parent"))?;
    let backup = path.with_extension("luau.before-reading-defaults");
    let mut keep = tempfile::NamedTempFile::new_in(parent)?;
    keep.write_all(original.as_bytes())?;
    keep.as_file().sync_all()?;
    match keep.persist_noclobber(&backup) {
        Ok(_) => {},
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {
            if fs::symlink_metadata(&backup)?.file_type().is_symlink()
                || fs::read(&backup)? != original.as_bytes() {
                return Err(io::Error::other("Existing rules backup differs; migration was not applied"));
            }
        },
        Err(e) => return Err(e.error),
    }
    let mut replacement = tempfile::NamedTempFile::new_in(parent)?;
    replacement.as_file().set_permissions(metadata.permissions())?;
    replacement.write_all(updated.as_bytes())?;
    replacement.as_file().sync_all()?;
    if fs::read(path)? != original.as_bytes() || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("Rules changed during migration; original left untouched"));
    }
    replacement.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)] fs::File::open(parent)?.sync_all()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_stock_table_is_removed_but_other_rules_are_byte_preserved() {
        let before = "-- my rules\nfunction example()\n return 'keep'\nend\n";
        let after = "\nfunction other() return 7 end\n";
        let source = format!("{before}{LEGACY_TABLE}{after}");
        let changed = migrate(&source).unwrap();
        assert_eq!(changed, format!("{before}folders = {{}}{after}"));
        assert!(migrate(&changed).is_none());
    }
    #[test]
    fn customized_titles_destinations_and_extra_items_are_not_changed() {
        for changed in [LEGACY_TABLE.replace("MDN Web Docs", "My MDN"),
            LEGACY_TABLE.replace("developer.mozilla.org/", "example.test/"),
            LEGACY_TABLE.replace("  },\n}", "    { title = 'Mine', url = 'https://example.test/' },\n  },\n}")] {
            assert!(migrate(&changed).is_none());
        }
    }
    #[test]
    fn strings_comments_functions_and_duplicate_assignments_are_not_rewritten() {
        for source in [format!("--[[\n{LEGACY_TABLE}\n]]"),
            format!("--[=[\n{LEGACY_TABLE}\n]=]"),
            format!("local example = [==[\n{LEGACY_TABLE}\n]==]"),
            format!("local example = `\n{LEGACY_TABLE}\n`"),
            format!("function example()\n{LEGACY_TABLE}\nend"),
            format!("if enabled then\n{LEGACY_TABLE}\nend"),
            format!("{LEGACY_TABLE}\n{LEGACY_TABLE}"),
            format!("local {LEGACY_TABLE}")] {
            assert!(migrate(&source).is_none(), "rewrote {source}");
        }
    }
    #[test]
    fn prior_control_flow_and_long_strings_do_not_confuse_the_guard() {
        let prefix = "function x() if y then return [=[end function]=] elseif z then return 1 else return 2 end end\n";
        assert!(migrate(&format!("{prefix}{LEGACY_TABLE}\n")).is_some());
    }
    #[test]
    fn crlf_is_preserved() {
        let source = format!("-- before\n{LEGACY_TABLE}\n-- after\n").replace('\n', "\r\n");
        assert_eq!(migrate(&source).unwrap(), "-- before\r\nfolders = {}\r\n-- after\r\n");
    }
    #[test]
    fn migration_keeps_a_backup_and_is_idempotent_on_disk() {
        let t = tempfile::tempdir().unwrap(); let path = t.path().join("rules.luau");
        fs::write(&path, LEGACY_TABLE).unwrap();
        assert!(migrate_file(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "folders = {}");
        assert_eq!(fs::read_to_string(path.with_extension("luau.before-reading-defaults")).unwrap(), LEGACY_TABLE);
        assert!(!migrate_file(&path).unwrap());
    }
    #[test]
    fn a_different_existing_backup_prevents_replacement() {
        let t = tempfile::tempdir().unwrap(); let path = t.path().join("rules.luau");
        fs::write(&path, LEGACY_TABLE).unwrap();
        fs::write(path.with_extension("luau.before-reading-defaults"), "keep me").unwrap();
        assert!(migrate_file(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), LEGACY_TABLE);
    }
}
