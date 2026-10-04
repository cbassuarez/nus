//! The sidebar as views of what is kept (keep.rs). The pinned grid stays
//! the order of the sidebar's spawners — places and shells are in it too —
//! but a pinned page or file is a kept item whose `pin` role is its place
//! in that grid: pinning keeps it, unpinning leaves it kept. A plain folder
//! is a collection: its rows are the kept items that sit in it, so filing
//! from the library, the slip or the tab menu is one act, and a collection
//! made anywhere shows as a folder. Folders' old item lists are filed into
//! collections once; folders.json still records them, for older builds.

use std::hash::{Hash, Hasher};

use crate::app::App;
use crate::folders::{Folder, Item, Kind};
use crate::library::Entry;
use crate::pins::{Pin, Target};

/// What a pin keeps, as a library source and container, when it keeps one.
pub fn pin_source(target: &Target) -> Option<(String, Option<String>)> {
    match target {
        Target::Page { url, container } => Some((url.clone(), crate::library::container(container))),
        Target::File { path } => Some((format!("file:{path}"), None)),
        _ => None,
    }
}

/// The pin target for a kept item, when it can be pinned.
fn target_of(e: &Entry) -> Option<Target> {
    if let Some(path) = e.source.strip_prefix("file:").filter(|p| !p.starts_with("//")) {
        return Some(Target::File { path: path.to_string() });
    }
    url::Url::parse(&e.source).ok().filter(|u| matches!(u.scheme(), "http" | "https" | "file"))?;
    Some(Target::Page { url: e.source.clone(), container: e.container.clone().unwrap_or_else(|| crate::containers::PERSONAL.to_string()) })
}

fn host(source: &str) -> String {
    source.split("//").nth(1).unwrap_or(source).split('/').next().unwrap_or("").trim_start_matches("www.").to_string()
}

impl App {
    /// Each frame, cheaply: when the library or the pins changed, bring
    /// the pin roles and the plain folders in line with them.
    pub(crate) fn sync_kept_side(&mut self) {
        if crate::private::enabled() || !self.library.is_loaded() { return; }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.pins.items.hash(&mut h);
        self.folders.iter().filter(|f| f.kind == Kind::Plain).for_each(|f| (&f.name, f.items.len()).hash(&mut h));
        let seen = (self.library.generation(), h.finish());
        if self.kept_side_seen == Some(seen) { return; }
        self.kept_side_seen = Some(seen);
        if !self.folders_filed {
            self.folders_filed = true;
            self.file_folder_items();
        }
        self.sync_pin_roles();
        self.derive_folders();
    }

    /// A pinned page or file is kept, its role its place in the grid; a
    /// kept item no pin names is not pinned.
    fn sync_pin_roles(&mut self) {
        let store = self.library.store().clone();
        let mut named: Vec<String> = Vec::new();
        for (i, pin) in self.pins.items.clone().iter().enumerate() {
            let Some((source, container)) = pin_source(&pin.target) else { continue };
            let found = self.library.kept(&source, &container).cloned();
            let e = match found {
                Some(e) => e,
                None => match store.keep(&source, &pin.title, container, crate::journal::now()) {
                    Ok((e, _)) => e,
                    Err(e) => { tracing::info!("pin keep failed: {e}"); continue; }
                },
            };
            named.push(e.id.clone());
            if e.pin != Some(i as u32) {
                match store.update(&e.id, |e| e.pin = Some(i as u32)) {
                    Ok(e) => self.library.remember(e),
                    Err(e) => tracing::info!("pin role failed: {e}"),
                }
            } else if !self.library.entries.contains_key(&e.id) {
                self.library.remember(e);
            }
        }
        let stale: Vec<String> = self.library.entries.values().filter(|e| !e.deleted && e.pin.is_some() && !named.contains(&e.id)).map(|e| e.id.clone()).collect();
        for id in stale {
            match store.update(&id, |e| e.pin = None) {
                Ok(e) => self.library.remember(e),
                Err(e) => tracing::info!("unpin role failed: {e}"),
            }
        }
    }

    /// Once: what plain folders held before they were collections is filed
    /// into them, so nothing a folder showed goes missing.
    fn file_folder_items(&mut self) {
        let held: Vec<(String, Item)> = self.folders.iter().filter(|f| f.kind == Kind::Plain).flat_map(|f| f.items.iter().map(|it| (f.name.clone(), it.clone()))).collect();
        for (name, it) in held {
            let filed = self.library.kept(&it.url, &None).is_some_and(|e| e.collections.contains(&name));
            if !filed { self.keep_into_folder(&it.url, &it.title, None, &name); }
        }
    }

    /// Plain folders' rows from the collections; a collection with no folder
    /// gets one, a folder whose collection emptied goes.
    fn derive_folders(&mut self) {
        let collections = self.library_collections();
        let mut changed = false;
        for name in &collections {
            if !self.folders.iter().any(|f| f.kind == Kind::Plain && &f.name == name) {
                let id = self.next_folder_id;
                self.next_folder_id += 1;
                // Before the live folders, with the plain ones.
                let at = self.folders.iter().rposition(|f| f.kind == Kind::Plain).map(|i| i + 1).unwrap_or(0);
                self.folders.insert(at, Folder { id, name: name.clone(), kind: Kind::Plain, items: Vec::new(), open: false, note: String::new() });
                changed = true;
            }
        }
        for f in self.folders.iter_mut().filter(|f| f.kind == Kind::Plain) {
            let mut kept: Vec<&Entry> = self.library.entries.values().filter(|e| !e.deleted && e.collections.contains(&f.name)).collect();
            kept.sort_by(|a, b| b.saved.cmp(&a.saved).then(a.id.cmp(&b.id)));
            let items: Vec<Item> = kept.into_iter().map(|e| Item { title: if e.title.is_empty() { e.source.clone() } else { e.title.clone() }, url: e.source.clone(), detail: host(&e.source) }).collect();
            if f.items != items { f.items = items; changed = true; }
        }
        let before = self.folders.len();
        self.folders.retain(|f| f.kind != Kind::Plain || !f.items.is_empty());
        changed |= self.folders.len() != before;
        if changed {
            self.save_folders();
            self.dirty = true;
        }
    }

    /// A folder row's item opened: the kept item, in its own container.
    pub(crate) fn open_kept_source(&mut self, source: &str) -> bool {
        let Some(e) = self.library.entries.values().find(|e| !e.deleted && crate::keep::canon(&e.source) == crate::keep::canon(source)).cloned() else { return false };
        self.open_kept_entry(&e);
        true
    }

    /// Take an item out of a plain folder: out of that collection.
    pub(crate) fn uncollect(&mut self, source: &str, name: &str) {
        let canon = crate::keep::canon(source);
        let ids: Vec<String> = self.library.entries.values().filter(|e| !e.deleted && crate::keep::canon(&e.source) == canon && e.collections.iter().any(|c| c == name)).map(|e| e.id.clone()).collect();
        let store = self.library.store().clone();
        for id in ids {
            match store.update(&id, |e| e.collections.retain(|c| c != name)) {
                Ok(e) => self.library.remember(e),
                Err(e) => tracing::info!("uncollect failed: {e}"),
            }
        }
    }

    /// Pin a kept item, or unpin it; it stays kept either way.
    pub(crate) fn toggle_kept_pin(&mut self, id: &str) {
        let Some(e) = self.library.entries.get(id).cloned() else { return };
        let Some(target) = target_of(&e) else {
            self.notice(nus_render::text::icons::PIN, "Can't Pin This", "only pages and files pin");
            return;
        };
        if let Some(i) = self.pins.items.iter().position(|p| p.target == target) {
            self.pin_action(crate::pins::Act::Remove(i));
            self.notice(nus_render::text::icons::PIN, "Unpinned", "still kept");
        } else {
            let title = if e.title.is_empty() { host(&e.source) } else { e.title.clone() };
            self.pins.items.push(Pin { id: format!("pin-{}", crate::remote::new_token()), title: title.clone(), target });
            self.save_prefs();
            self.layout();
            self.notice(nus_render::text::icons::PIN, "Pinned", title);
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_and_items_name_the_same_place() {
        let page = Target::Page { url: "https://docs.rs/wgpu".into(), container: crate::containers::PERSONAL.into() };
        assert_eq!(pin_source(&page), Some(("https://docs.rs/wgpu".to_string(), None)));
        let work = Target::Page { url: "https://mail.test".into(), container: "work".into() };
        assert_eq!(pin_source(&work), Some(("https://mail.test".to_string(), Some("work".to_string()))));
        assert_eq!(pin_source(&Target::Ports), None);
        let e = Entry { source: "https://mail.test".into(), container: Some("work".into()), ..Default::default() };
        assert_eq!(target_of(&e), Some(work));
        let f = Entry { source: "file:/home/me/todo.md".into(), ..Default::default() };
        assert_eq!(target_of(&f), Some(Target::File { path: "/home/me/todo.md".into() }));
        assert_eq!(pin_source(&target_of(&f).unwrap()).unwrap().0, f.source);
    }
}
