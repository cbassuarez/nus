//! Keeping: one record for everything worth coming back to. A kept item is
//! a library record (library_store.rs) whose roles say what it is for:
//! `reading` puts it on the reading list, `collections` files it, `pin`
//! gives it a place in the sidebar's grid, `keyword` lets the prompt reach
//! it, `quote` points at a passage. The reading list, folders and pins are
//! views of the one store, not stores of their own.
//!
//! ⌘D (Ctrl+D on a page elsewhere) keeps what is in front; the palette's
//! KEEP THIS PAGE does the same anywhere. A slip hangs from the address
//! field for a few seconds to say so and to change the roles (keep_ui.rs).
//!
//! This module holds what is true of a kept item apart from any window:
//! how two addresses are told to be the same page.

/// Query parameters that only say where a link was followed from.
const TRACKING: &[&str] = &["fbclid", "gclid", "dclid", "msclkid", "mc_cid", "mc_eid", "igshid", "yclid", "ref_src", "_hsenc", "_hsmi"];

/// The address two saves of one page share: the host lowercased and
/// without `www.`, no default port, no tracking parameters, no empty
/// query or fragment, no trailing slash on a path. Anything that isn't an
/// http(s) URL is its own canonical form. Only used to compare; the
/// stored source stays exactly as it was given.
pub fn canon(source: &str) -> String {
    let Ok(mut u) = url::Url::parse(source.trim()) else { return source.to_string() };
    if !matches!(u.scheme(), "http" | "https") { return source.to_string(); }
    if let Some(host) = u.host_str().map(|h| h.trim_start_matches("www.").to_string()) {
        if u.set_host(Some(&host)).is_err() { return source.to_string(); }
    }
    let kept: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| !k.starts_with("utm_") && !TRACKING.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        u.set_query(None);
    } else {
        u.query_pairs_mut().clear().extend_pairs(kept);
    }
    if u.fragment() == Some("") { u.set_fragment(None); }
    let path = u.path().to_string();
    if path.len() > 1 && path.ends_with('/') { u.set_path(path.trim_end_matches('/')); }
    let mut out = u.to_string();
    // `https://example.org/` and `https://example.org` are one page.
    if u.path() == "/" && u.query().is_none() && u.fragment().is_none() { out.pop(); }
    out
}

/// A keyword's address with what followed it: `%s` takes the rest, encoded;
/// an address without `%s` is the place itself, and the rest is ignored.
pub fn expand(template: &str, rest: &str) -> String {
    if template.contains("%s") { template.replace("%s", &crate::prompt::encode_query(rest.trim())) } else { template.to_string() }
}

/// A keyword must be one short word: what is typed first at the prompt.
pub fn valid_keyword(word: &str) -> bool {
    !word.is_empty() && word.chars().count() <= 64 && !word.chars().any(|c| c.is_whitespace() || c.is_control())
        && !word.starts_with(['>', '?', '@', '/', '.', '~'])
}

/// What the kept palette narrows by: words, and facets written as words.
/// `to:read` · `done` · `copy` · `in:<collection>` · `said:<word>`; the rest matches
/// the title, the address and the keyword.
#[derive(Debug, Default, PartialEq)]
pub struct Facets {
    pub words: Vec<String>,
    pub reading: bool,
    pub done: bool,
    pub copy: bool,
    pub collections: Vec<String>,
    /// `said:<word>`: the saved copy holds it (keep_index.rs; the caller checks).
    pub said: Vec<String>,
}

pub fn facets(q: &str) -> Facets {
    let mut f = Facets::default();
    for w in q.split_whitespace() {
        let lw = w.to_lowercase();
        match lw.as_str() {
            "to:read" => f.reading = true,
            "done" | "is:done" => f.done = true,
            "copy" | "has:copy" => f.copy = true,
            _ => match (lw.strip_prefix("in:"), lw.strip_prefix("said:")) {
                (Some(c), _) if !c.is_empty() => f.collections.push(c.to_string()),
                (_, Some(w)) if !w.is_empty() => f.said.push(w.to_string()),
                _ => f.words.push(lw),
            },
        }
    }
    f
}

/// How well a kept item answers a query, best first; None when it doesn't.
/// A keyword typed whole wins, then an address that starts with the query,
/// then items whose title and address hold every word.
pub fn score(e: &crate::library::Entry, f: &Facets) -> Option<i64> {
    if e.deleted { return None; }
    let on_list = e.reading != Some(false);
    if f.reading && !(on_list && !e.finished && !e.archived) { return None; }
    if f.done && !(on_list && e.finished) { return None; }
    if f.copy && e.snapshot.is_none() { return None; }
    if !f.collections.iter().all(|c| e.collections.iter().any(|have| have.to_lowercase() == *c)) { return None; }
    let title = e.title.to_lowercase();
    let address = e.source.to_lowercase();
    let host = address.split("//").nth(1).unwrap_or(&address).trim_start_matches("www.");
    let keyword = e.keyword.to_lowercase();
    let mut score = 0;
    if let Some(first) = f.words.first() {
        if !keyword.is_empty() && *first == keyword { score += 10_000; }
        if host.starts_with(first.as_str()) { score += 1_000; }
    }
    let all = f.words.iter().all(|w| title.contains(w.as_str()) || address.contains(w.as_str()) || keyword == *w);
    if !all && score < 10_000 { return None; }
    // Ties go to the newer keep, in the caller's sort.
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::{canon, expand, facets, score, valid_keyword, Facets};
    use crate::library::Entry;

    #[test]
    fn one_page_however_it_was_reached() {
        let a = canon("https://www.Example.org/docs/?utm_source=x&page=2#");
        assert_eq!(a, "https://example.org/docs?page=2");
        assert_eq!(canon("https://example.org:443/docs?page=2&fbclid=abc"), a);
        assert_eq!(canon("https://example.org/"), canon("https://EXAMPLE.org"));
    }

    #[test]
    fn what_tells_pages_apart_stays() {
        assert_ne!(canon("https://example.org/a?q=1"), canon("https://example.org/a?q=2"));
        assert_ne!(canon("https://example.org/a#part"), canon("https://example.org/a"));
        assert_ne!(canon("http://example.org/a"), canon("https://example.org/a"));
        assert_ne!(canon("https://example.org:8443/a"), canon("https://example.org/a"));
    }

    #[test]
    fn other_sources_are_their_own_form() {
        for s in ["file:/home/me/notes.md", "note:0123", "not a url", "about:blank"] {
            assert_eq!(canon(s), s);
        }
    }

    #[test]
    fn keywords_take_the_rest() {
        assert_eq!(expand("https://docs.rs/releases/search?query=%s", " serde json "), "https://docs.rs/releases/search?query=serde+json");
        assert_eq!(expand("https://github.com", "ignored"), "https://github.com");
        assert!(valid_keyword("rs") && valid_keyword("gh"));
        assert!(!valid_keyword("two words") && !valid_keyword("") && !valid_keyword(">x") && !valid_keyword("?q"));
    }

    #[test]
    fn facets_are_words() {
        let f = facets("to:read in:Rust ownership said:borrow");
        assert_eq!(f, Facets { words: vec!["ownership".into()], reading: true, collections: vec!["rust".into()], said: vec!["borrow".into()], ..Default::default() });
    }

    #[test]
    fn a_keyword_wins_then_the_host_then_the_words() {
        let e = |source: &str, title: &str, keyword: &str| Entry { id: "0".repeat(32), source: source.into(), title: title.into(), keyword: keyword.into(), reading: Some(false), ..Default::default() };
        let rs = e("https://docs.rs/releases/search?query=%s", "docs.rs search", "rs");
        let rust = e("https://doc.rust-lang.org/book/", "The Rust Book", "");
        let other = e("https://example.org/rust", "Rust notes", "");
        let q = facets("rs serde");
        assert!(score(&rs, &q).unwrap() >= 10_000);
        assert!(score(&rust, &q).is_none());
        let q = facets("doc");
        assert!(score(&rust, &q).unwrap() >= 1_000);
        assert!(score(&other, &facets("rust")).is_some());
        // Facets narrow: kept-only items are not on the reading list.
        assert!(score(&rust, &facets("to:read")).is_none());
        let reading = Entry { reading: None, ..rust.clone() };
        assert!(score(&reading, &facets("to:read")).is_some());
    }
}
