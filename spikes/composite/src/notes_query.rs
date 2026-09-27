//! What you type to find a note, read into parts: plain words (matched as
//! words, never as search syntax), "quoted text" (matched as written,
//! punctuation and all), code-like values (a flag, a path, a URL: matched
//! exactly), and filters:
//!
//! ```text
//! in:nus  in:personal  in:all   tag:release   source:terminal|web|file|reading
//! before:2026-09-01  after:2026-09-01  is:unfiled
//! ```
//!
//! A filter this does not know is said back, not quietly dropped or
//! searched for as a word.

use crate::notes_model::{self as model, Literal};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The project you are in (the default when there is one).
    Here,
    All,
    Personal,
    /// A project by its folder's name.
    Named(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub words: Vec<String>,
    /// The last word is still being typed: match it as a prefix.
    pub prefix_last: bool,
    pub quoted: Vec<String>,
    pub literals: Vec<Literal>,
    pub place: Option<Place>,
    pub tags: Vec<String>,
    pub sources: Vec<String>,
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub unfiled: bool,
    /// What could not be read, in words to show.
    pub problems: Vec<String>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.quoted.is_empty() && self.literals.is_empty() && self.tags.is_empty() && self.sources.is_empty() && self.before.is_none() && self.after.is_none() && !self.unfiled
    }

    /// The FTS5 expression for the words: each one a quoted phrase (so
    /// `OR`, `NEAR`, `*` and quotes are only ever words), all required.
    pub fn fts(&self) -> Option<String> {
        if self.words.is_empty() {
            return None;
        }
        let n = self.words.len();
        let parts: Vec<String> = self.words.iter().enumerate().map(|(i, w)| {
            let p = format!("\"{}\"", w.replace('"', "\"\""));
            if i + 1 == n && self.prefix_last { format!("{p}*") } else { p }
        }).collect();
        Some(parts.join(" AND "))
    }
}

const SOURCES: [&str; 4] = ["terminal", "web", "file", "reading"];

pub fn parse(input: &str) -> Query {
    let mut q = Query { prefix_last: !input.ends_with(char::is_whitespace), ..Default::default() };
    let mut rest = input.trim();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('"') {
            let (inside, tail) = match after.find('"') {
                Some(i) => (&after[..i], &after[i + 1..]),
                None => (after, ""),
            };
            let inside = inside.trim();
            if !inside.is_empty() {
                q.quoted.push(model::nfc(inside));
            }
            rest = tail.trim_start();
            if rest.is_empty() {
                q.prefix_last = false;
            }
            continue;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..end];
        rest = rest[end..].trim_start();
        if let Some((key, value)) = token.split_once(':').filter(|(k, v)| k.len() > 1 && k.chars().all(|c| c.is_ascii_alphabetic()) && !v.starts_with("//")) {
            let value = value.trim();
            match key.to_ascii_lowercase().as_str() {
                "in" => q.place = Some(match value.to_lowercase().as_str() {
                    "all" | "*" => Place::All,
                    "personal" | "me" => Place::Personal,
                    "here" | "this" | "project" => Place::Here,
                    _ => Place::Named(value.to_string()),
                }),
                "tag" if !value.is_empty() => q.tags.push(value.trim_start_matches('#').to_lowercase()),
                "source" if SOURCES.contains(&value.to_lowercase().as_str()) => q.sources.push(value.to_lowercase()),
                "source" => q.problems.push(format!("source:{value} · use terminal, web, file or reading")),
                "before" | "after" => match model::parse_time(value).filter(|_| value.len() == 10) {
                    Some(t) if key == "before" => q.before = Some(t),
                    Some(t) => q.after = Some(t + 86_400 - 1),
                    None => q.problems.push(format!("{key}:{value} · a date is YYYY-MM-DD")),
                },
                "is" if value.eq_ignore_ascii_case("unfiled") => q.unfiled = true,
                _ => q.problems.push(format!("{token} · not a filter here · in: tag: source: before: after: is:unfiled")),
            }
            if rest.is_empty() {
                q.prefix_last = false;
            }
            continue;
        }
        if let Some(l) = model::literal_of(token) {
            q.literals.push(l);
            if rest.is_empty() {
                q.prefix_last = false;
            }
            continue;
        }
        // A word: its letters and digits; punctuation around it is not
        // part of what FTS matches anyway.
        let w: String = token.chars().filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'' | '.')).collect();
        let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        if !w.is_empty() {
            q.words.push(model::nfc(&w));
        } else if rest.is_empty() {
            q.prefix_last = false;
        }
    }
    if q.words.is_empty() {
        q.prefix_last = false;
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_words_quotes_and_literals() {
        let q = parse("in:nus tag:#Release source:terminal after:2026-09-01 \"a-b c\" reflow --no-ff src/a-b.rs is:unfiled wrap");
        assert_eq!(q.place, Some(Place::Named("nus".into())));
        assert_eq!(q.tags, vec!["release"]);
        assert_eq!(q.sources, vec!["terminal"]);
        assert_eq!(q.after, Some(1_788_220_800 + 86_399));
        assert_eq!(q.quoted, vec!["a-b c"]);
        assert_eq!(q.words, vec!["reflow", "wrap"]);
        assert!(q.unfiled);
        assert!(q.literals.contains(&Literal { kind: "flag", value: "--no-ff".into() }));
        assert!(q.literals.contains(&Literal { kind: "path", value: "src/a-b.rs".into() }));
        assert!(q.prefix_last);
        assert!(q.problems.is_empty());
    }

    #[test]
    fn words_cannot_be_search_syntax() {
        let q = parse("alpha OR beta\" NEAR*");
        assert_eq!(q.fts().unwrap(), "\"alpha\" AND \"OR\" AND \"beta\" AND \"NEAR\"*");
        let q = parse("alpha ");
        assert_eq!(q.fts().unwrap(), "\"alpha\"");
    }

    #[test]
    fn urls_are_literals_not_filters() {
        let q = parse("https://example.com/?x=1#head");
        assert_eq!(q.literals, vec![Literal { kind: "url", value: "https://example.com/?x=1#head".into() }]);
        assert!(q.problems.is_empty());
    }

    #[test]
    fn unknown_filters_and_bad_dates_are_said_back() {
        let q = parse("color:red before:tuesday");
        assert_eq!(q.problems.len(), 2);
        assert!(q.words.is_empty());
    }
}
