//! Finding what was kept (keep.rs): kept items lead the palette, the home
//! prompt's web rows and the address field's list, ahead of history, and
//! wear the ribbon there. A note that cites the best match comes with it.
//! The KEPT palette narrows with words (`to:read in:rust copy`). A keyword
//! on an item — `rs`, or `rs %s` for a search — is typed first wherever an
//! address is: the rest goes into `%s`.

use std::collections::HashSet;

use crate::app::{Action, App, PaletteMode, PaletteRow};
use crate::library::Entry;

/// A palette row that is a kept item draws the ribbon in its first column.
pub const KEPT: &str = "kept";

impl App {
    /// Where a kept item opens, if it opens anywhere from a row.
    fn kept_action(e: &Entry, new_tab: bool) -> Option<Action> {
        if let Some(path) = e.source.strip_prefix("file:").filter(|p| !p.starts_with("//")) {
            return Some(Action::OpenFile(path.to_string()));
        }
        let ok = url::Url::parse(&e.source).ok().is_some_and(|u| matches!(u.scheme(), "http" | "https" | "file"));
        if !ok || e.source.contains("%s") { return None; }
        // A new tab opens in the item's own container; in place, the pane's.
        Some(if new_tab { Action::OpenKept(e.id.clone()) } else { Action::OpenInPane(e.source.clone()) })
    }

    /// A kept item as a row reads: its title, its host, what it is for.
    fn kept_text(e: &Entry) -> String {
        let host = e.source.split("//").nth(1).unwrap_or(&e.source).split('/').next().unwrap_or("").trim_start_matches("www.");
        // A local file has no host: its name says where it is.
        let host = if host.is_empty() { e.source.rsplit('/').next().unwrap_or(&e.source) } else { host };
        let mut text = if e.title.is_empty() || e.title == e.source { host.to_string() } else { format!("{} · {host}", e.title) };
        if !e.keyword.is_empty() { text.push_str(&format!(" · {}", e.keyword)); }
        if e.reading != Some(false) && !e.finished {
            let p = (e.progress * 100.0).round() as u32;
            text.push_str(&if p > 0 { format!(" · to read {p}%") } else { " · to read".to_string() });
        }
        if e.pin.is_some() { text.push_str(" · pinned"); }
        for c in e.collections.iter().take(2) { text.push_str(&format!(" · {c}")); }
        text
    }

    /// The kept items a query reaches, best first.
    fn kept_matches(&self, q: &str) -> Vec<&Entry> {
        let f = crate::keep::facets(q);
        let said = (!f.said.is_empty()).then(|| self.kept_words.search(&f.said));
        let mut hits: Vec<(i64, &Entry)> = self.library.entries.values()
            .filter(|e| said.as_ref().is_none_or(|ids| ids.contains(&e.id)))
            .filter_map(|e| crate::keep::score(e, &f).map(|s| (s, e))).collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.saved.cmp(&a.1.saved)).then(a.1.id.cmp(&b.1.id)));
        hits.into_iter().map(|(_, e)| e).collect()
    }

    /// The canonical addresses of everything kept: history leaves them out
    /// where kept rows already list them.
    pub(crate) fn kept_addresses(&self) -> HashSet<String> {
        self.library.entries.values().filter(|e| !e.deleted).map(|e| crate::keep::canon(&e.source)).collect()
    }

    /// Kept rows for a query, ahead of history: a keyword typed first opens
    /// its address with the rest; then the best matches, and the notes that
    /// cite the best of them. Nothing for an empty query unless `idle` asks
    /// for the newest few.
    pub(crate) fn kept_rows(&self, q: &str, new_tab: bool, limit: usize, idle: bool) -> Vec<PaletteRow> {
        if crate::private::enabled() { return Vec::new(); }
        let q = q.trim();
        let mut rows = Vec::new();
        if let Some((url, text)) = self.keyword_target(q) {
            rows.push(PaletteRow { num: KEPT.into(), text, action: if new_tab { Action::NewBrowser(url) } else { Action::OpenInPane(url) } });
        }
        if q.is_empty() && !idle { return rows; }
        let first = q.split_whitespace().next().unwrap_or("").to_lowercase();
        for e in self.kept_matches(q) {
            if rows.len() >= limit { break; }
            // The keyword row above already is this item.
            if !first.is_empty() && e.keyword.to_lowercase() == first { continue; }
            let Some(action) = Self::kept_action(e, new_tab) else { continue };
            rows.push(PaletteRow { num: KEPT.into(), text: Self::kept_text(e), action });
        }
        if let Some(best) = (!q.is_empty()).then(|| self.kept_matches(q).into_iter().find(|e| Self::kept_action(e, new_tab).is_some())).flatten() {
            for h in crate::notes_index::about(&[crate::notes_index::lookup_url(&best.source)]).into_iter().take(2) {
                rows.push(PaletteRow { num: "✎".into(), text: format!("{} · note · cites {}", h.title, if best.title.is_empty() { &best.source } else { &best.title }), action: Action::Note(crate::notes_ui::NoteAct::Open(h.path)) });
            }
        }
        rows
    }

    /// A keyword typed first: the kept item's address with the rest in it,
    /// and how the row reads.
    pub(crate) fn keyword_target(&self, q: &str) -> Option<(String, String)> {
        if crate::private::enabled() { return None; }
        let q = q.trim();
        let (word, rest) = q.split_once(char::is_whitespace).map(|(w, r)| (w, r.trim())).unwrap_or((q, ""));
        if word.is_empty() { return None; }
        let e = self.library.entries.values().find(|e| !e.deleted && !e.keyword.is_empty() && e.keyword.eq_ignore_ascii_case(word))?;
        // A search keyword with nothing after it waits for the words.
        if e.source.contains("%s") && rest.is_empty() { return None; }
        let url = crate::keep::expand(&e.source, rest);
        let title = if e.title.is_empty() { e.source.clone() } else { e.title.clone() };
        let text = if rest.is_empty() || !e.source.contains("%s") { format!("{} → {title}", e.keyword) } else { format!("{} → {title} · {rest}", e.keyword) };
        Some((url, text))
    }

    /// The KEPT palette: everything kept, narrowed by what is typed.
    pub(crate) fn kept_mode_rows(&self, input: &str) -> Vec<PaletteRow> {
        let mut rows: Vec<PaletteRow> = self.kept_matches(input).into_iter()
            .filter_map(|e| Self::kept_action(e, true).map(|action| PaletteRow { num: KEPT.into(), text: Self::kept_text(e), action }))
            .take(200).collect();
        if rows.is_empty() {
            let text = if self.library.entries.values().all(|e| e.deleted) { "Nothing kept yet · Ctrl+D on a page keeps it" } else { "Nothing kept matches · try to:read, in:<folder>, done, copy, said:<word>" };
            rows.push(PaletteRow { num: "·".into(), text: text.into(), action: Action::Library });
        }
        rows
    }

    /// The keyword field, for the kept item a slip was showing.
    pub(crate) fn ask_keyword(&mut self, id: String) {
        self.keep_keyword_for = Some(id);
        self.keep_slip = None;
        self.open_palette(PaletteMode::KeepKeyword);
    }

    pub(crate) fn keyword_rows(&self, input: &str) -> Vec<PaletteRow> {
        let word = input.trim();
        let Some(e) = self.keep_keyword_for.as_ref().and_then(|id| self.library.entries.get(id)) else { return Vec::new() };
        let title = if e.title.is_empty() { e.source.clone() } else { e.title.clone() };
        if word.is_empty() {
            let text = if e.keyword.is_empty() { format!("a word that opens {title} · nothing set") } else { format!("remove the keyword “{}” from {title}", e.keyword) };
            return vec![PaletteRow { num: "+".into(), text, action: Action::KeepKeyword(String::new()) }];
        }
        if !crate::keep::valid_keyword(word) {
            return vec![PaletteRow { num: "·".into(), text: "one short word, no spaces; don't start with > ? @ / . ~".into(), action: Action::Noop }];
        }
        let search = if e.source.contains("%s") { format!(" · {word} <words> searches") } else { String::new() };
        vec![PaletteRow { num: "+".into(), text: format!("Keyword · {word} opens {title}{search}"), action: Action::KeepKeyword(word.to_string()) }]
    }

    /// Set (or, empty, clear) the asked-for item's keyword. A keyword is one
    /// item's: another item holding it gives it up, and says so.
    pub(crate) fn set_keyword(&mut self, word: String) {
        let Some(id) = self.keep_keyword_for.take() else { return };
        let word = word.trim().to_string();
        if !word.is_empty() && !crate::keep::valid_keyword(&word) { return; }
        let store = self.library.store().clone();
        let holders: Vec<String> = self.library.entries.values().filter(|e| !e.deleted && e.id != id && !word.is_empty() && e.keyword.eq_ignore_ascii_case(&word)).map(|e| e.id.clone()).collect();
        for other in holders {
            match store.update(&other, |e| e.keyword.clear()) {
                Ok(e) => { let title = e.title.clone(); self.library.remember(e); self.notice(nus_render::text::icons::BOOK, "Keyword Moved", format!("“{word}” no longer opens {title}")); }
                Err(e) => { tracing::info!("keyword release failed: {e}"); self.notice(nus_render::text::icons::BOOK, "Keyword Not Set", "another item holds it · try again"); return; }
            }
        }
        match store.update(&id, |e| e.keyword = word.clone()) {
            Ok(e) => {
                self.library.remember(e);
                if word.is_empty() { self.notice(nus_render::text::icons::BOOK, "Keyword Removed", ""); } else { self.notice(nus_render::text::icons::BOOK, "Keyword Set", format!("type {word} where an address goes")); }
            }
            Err(e) => { tracing::info!("keyword failed: {e}"); self.notice(nus_render::text::icons::BOOK, "Keyword Not Set", "the item changed elsewhere · try again"); }
        }
        self.dirty = true;
    }
}
