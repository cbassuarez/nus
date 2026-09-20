//! Syntax, by tree-sitter. Bash and PowerShell grammars ride in the
//! binary for the command line (the shell's own language) and the ask
//! panel's blocks; any other grammar loads at runtime from
//! profile/grammars/<name>/ — the shared library `tree-sitter build`
//! makes (`<name>.dll` / `.so` / `.dylib`, exporting `tree_sitter_<name>`)
//! beside its `highlights.scm` — so the core stays light and a bundle is
//! a folder. Highlight captures map onto the command line's classes.

use crate::predict::Tok;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// The capture names we colour, in the order the configuration indexes them.
const NAMES: &[&str] = &[
    "keyword", "function", "function.builtin", "function.call", "function.method", "string", "string.special", "number", "constant", "constant.builtin", "operator", "comment", "variable", "variable.builtin", "property", "type", "punctuation", "punctuation.bracket", "punctuation.delimiter", "embedded",
];

fn class_of(name: &str) -> Tok {
    match name {
        "keyword" | "function" | "function.builtin" | "function.call" | "function.method" => Tok::Command,
        "string" | "string.special" => Tok::Str,
        "number" | "constant" | "constant.builtin" => Tok::Num,
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => Tok::Op,
        "comment" => Tok::Plain,
        "variable" | "variable.builtin" | "property" | "type" => Tok::Path,
        _ => Tok::Plain,
    }
}

struct Loaded {
    config: HighlightConfiguration,
    // Keeps a runtime grammar's library alive for as long as its language.
    _lib: Option<libloading::Library>,
}

thread_local! {
    static LANGS: RefCell<HashMap<String, Option<Rc<Loaded>>>> = RefCell::new(HashMap::new());
    static HL: RefCell<Highlighter> = RefCell::new(Highlighter::new());
}

fn grammars_dir() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("grammars")
}

/// A grammar folder: the library `tree-sitter build` produced and its
/// queries. The symbol is `tree_sitter_<name>` with dashes as underscores.
fn load_runtime(name: &str) -> Option<Loaded> {
    let dir = grammars_dir().join(name);
    let ext = if cfg!(windows) { "dll" } else if cfg!(target_os = "macos") { "dylib" } else { "so" };
    let lib_path = ["parser", name].iter().map(|b| dir.join(format!("{b}.{ext}"))).find(|p| p.exists())?;
    let highlights = std::fs::read_to_string(dir.join("highlights.scm")).ok()?;
    let injections = std::fs::read_to_string(dir.join("injections.scm")).unwrap_or_default();
    let locals = std::fs::read_to_string(dir.join("locals.scm")).unwrap_or_default();
    let symbol = format!("tree_sitter_{}", name.replace('-', "_"));
    // SAFETY: a tree-sitter grammar library exports a plain C function that
    // returns a static TSLanguage; that's the contract `tree-sitter build`
    // fulfils, and we keep the library loaded for as long as the language.
    let lib = unsafe { libloading::Library::new(&lib_path).ok()? };
    let language: Language = unsafe {
        let f: libloading::Symbol<unsafe extern "C" fn() -> *const tree_sitter::ffi::TSLanguage> = lib.get(symbol.as_bytes()).ok()?;
        Language::from_raw(f())
    };
    let mut config = HighlightConfiguration::new(language, name, &highlights, &injections, &locals).ok()?;
    config.configure(NAMES);
    tracing::info!("grammar {name} from {}", lib_path.display());
    Some(Loaded { config, _lib: Some(lib) })
}

fn load(name: &str) -> Option<Loaded> {
    let built_in = match name {
        "bash" | "sh" | "zsh" | "shell" => Some((Language::new(tree_sitter_bash::LANGUAGE), "bash", tree_sitter_bash::HIGHLIGHT_QUERY, "", "")),
        "powershell" | "pwsh" | "ps1" => Some((Language::new(tree_sitter_powershell::LANGUAGE), "powershell", tree_sitter_powershell::HIGHLIGHTS_QUERY, "", "")),
        "rust" => Some((Language::new(tree_sitter_rust::LANGUAGE), "rust", tree_sitter_rust::HIGHLIGHTS_QUERY, tree_sitter_rust::INJECTIONS_QUERY, "")),
        "python" => Some((Language::new(tree_sitter_python::LANGUAGE), "python", tree_sitter_python::HIGHLIGHTS_QUERY, "", "")),
        "javascript" | "js" => Some((Language::new(tree_sitter_javascript::LANGUAGE), "javascript", tree_sitter_javascript::HIGHLIGHT_QUERY, tree_sitter_javascript::INJECTIONS_QUERY, tree_sitter_javascript::LOCALS_QUERY)),
        "typescript" | "ts" => Some((Language::new(tree_sitter_typescript::LANGUAGE_TYPESCRIPT), "typescript", tree_sitter_typescript::HIGHLIGHTS_QUERY, "", tree_sitter_typescript::LOCALS_QUERY)),
        "json" => Some((Language::new(tree_sitter_json::LANGUAGE), "json", tree_sitter_json::HIGHLIGHTS_QUERY, "", "")),
        "go" => Some((Language::new(tree_sitter_go::LANGUAGE), "go", tree_sitter_go::HIGHLIGHTS_QUERY, "", "")),
        "toml" => Some((Language::new(tree_sitter_toml_ng::LANGUAGE), "toml", tree_sitter_toml_ng::HIGHLIGHTS_QUERY, "", "")),
        _ => None,
    };
    if let Some((language, n, hl, inj, loc)) = built_in {
        let mut config = HighlightConfiguration::new(language, n, hl, inj, loc).ok()?;
        config.configure(NAMES);
        return Some(Loaded { config, _lib: None });
    }
    load_runtime(name)
}

fn language(name: &str) -> Option<Rc<Loaded>> {
    let key = name.to_ascii_lowercase();
    LANGS.with(|l| {
        let mut map = l.borrow_mut();
        if let Some(v) = map.get(&key) {
            return v.clone();
        }
        let v = load(&key).map(Rc::new);
        map.insert(key, v.clone());
        v
    })
}

/// Is there a grammar for this language (built in, or in profile/grammars)?
pub fn has(name: &str) -> bool {
    language(name).is_some()
}

/// The grammars available: the built-in two plus every folder in
/// profile/grammars that loads.
pub fn available() -> Vec<String> {
    let mut v: Vec<String> = ["bash", "powershell", "rust", "python", "javascript", "typescript", "json", "go", "toml"].iter().map(|s| s.to_string()).collect();
    if let Ok(rd) = std::fs::read_dir(grammars_dir()) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                if let Some(n) = e.file_name().to_str() {
                    v.push(n.to_string());
                }
            }
        }
    }
    v
}

/// Spans of `text` as (char start, char len, class), from the grammar's
/// highlight captures. None when there's no grammar for `lang`.
pub fn spans(lang: &str, text: &str) -> Option<Vec<(usize, usize, Tok)>> {
    spans_cancellable(lang, text, None)
}

pub fn spans_cancellable(lang: &str, text: &str, cancel: Option<&std::sync::atomic::AtomicUsize>) -> Option<Vec<(usize, usize, Tok)>> {
    let loaded = language(lang)?;
    let bytes = text.as_bytes();
    // Byte offsets → char offsets, once.
    let mut char_at = vec![0usize; bytes.len() + 1];
    let mut n = 0;
    for (i, _) in text.char_indices() {
        char_at[i] = n;
        n += 1;
    }
    char_at[bytes.len()] = n;
    for i in 1..=bytes.len() {
        if char_at[i] == 0 && i < bytes.len() && !text.is_char_boundary(i) {
            char_at[i] = char_at[i - 1];
        }
    }
    let mut out = Vec::new();
    HL.with(|h| {
        let mut h = h.borrow_mut();
        let Ok(events) = h.highlight(&loaded.config, bytes, None, cancel, |_| None) else { return };
        let mut stack: Vec<usize> = Vec::new();
        for ev in events.flatten() {
            match ev {
                HighlightEvent::HighlightStart(hl) => stack.push(hl.0),
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } => {
                    if let Some(&i) = stack.last() {
                        let class = NAMES.get(i).map(|n| class_of(n)).unwrap_or(Tok::Plain);
                        if class != Tok::Plain && end > start {
                            let (a, b) = (char_at[start.min(bytes.len())], char_at[end.min(bytes.len())]);
                            if b > a {
                                out.push((a, b - a, class));
                            }
                        }
                    }
                }
            }
        }
    });
    Some(out)
}

/// The command line's spans: the grammar's, with the regex tokens filling
/// what the grammar leaves plain (flags, paths).
pub fn command_line(lang: &str, text: &str) -> Vec<(usize, usize, Tok)> {
    let regex = crate::predict::tokens(text);
    let Some(mut tree) = spans(lang, text) else { return regex };
    if tree.is_empty() {
        return regex;
    }
    tree.sort_by_key(|s| s.0);
    let extra: Vec<(usize, usize, Tok)> = regex
        .into_iter()
        .filter(|&(a, len, class)| matches!(class, Tok::Flag | Tok::Path) && !tree.iter().any(|&(s, l, _)| a < s + l && s < a + len))
        .collect();
    tree.extend(extra);
    tree.sort_by_key(|s| s.0);
    tree
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_marks_the_command_and_strings() {
        let s = spans("bash", "git commit -m \"hello\" && ls -la").unwrap();
        assert!(s.iter().any(|&(a, l, c)| a == 0 && l == 3 && c == Tok::Command));
        assert!(s.iter().any(|&(_, _, c)| c == Tok::Str));
    }

    #[test]
    fn powershell_marks_cmdlets() {
        let s = spans("powershell", "Get-ChildItem -Force | Sort-Object Length").unwrap();
        assert!(s.iter().any(|&(a, _, c)| a == 0 && c == Tok::Command), "{s:?}");
    }

    #[test]
    fn a_pipeline_with_braces_and_unicode_is_fine() {
        for i in 1..=80 {
            let line: String = "Get-ChildItem -Force ~/Downloads | Where-Object { $_.Length -gt 1024 } | Sort-Object Length -Descending — ünïcode".chars().take(i).collect();
            let _ = spans("powershell", &line);
            let _ = command_line("powershell", &line);
            let _ = command_line("bash", &line);
        }
    }

    #[test]
    fn unknown_language_is_none() {
        assert!(spans("klingon", "x").is_none());
    }
}
