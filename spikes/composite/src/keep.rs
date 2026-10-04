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

#[cfg(test)]
mod tests {
    use super::canon;

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
}
