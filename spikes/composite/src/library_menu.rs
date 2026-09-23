//! Item-owned reading commands shared by row menus and the reader's More button.
//! The menu keeps an Entry id, never a row index or an implicit focused reader.
use super::{App, Entry, Hit, Pane, reading_action_available};
use winit::{event::ElementState, keyboard::{Key, NamedKey}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Add,
    Edit,
    OpenSaved,
    Original,
    CopySource,
    Finished(bool),
    Archived(bool),
    Refresh,
    Remove,
    ConfirmRemove(u64),
    Cancel,
    Find,
    CopySelection,
    Smaller,
    Larger,
}

impl App {
    fn menu_reading_entry(&mut self, id: &str) -> Option<Entry> {
        match self.library.store.read(id) {
            Ok(e) if !e.deleted => Some(e),
            Ok(_) => { self.library_message("This reading item was removed. No other item was changed."); None },
            Err(e) => { self.library_message(format!("Could not read this item: {e}")); None },
        }
    }

    pub(crate) fn library_context_anchor(&self, id: &str) -> (f32, f32) {
        self.library_home().map(|h| {
            h.library_ui.hits.iter().find_map(|(r, hit)| {
                (matches!(hit, Hit::Row(key) if key == id)
                    || (h.reading.as_ref().is_some_and(|r| r.id == id) && *hit == Hit::More))
                    .then_some((r.x, r.bottom()))
            }).unwrap_or((h.rect.x + self.px(16.0), h.rect.y + self.px(48.0)))
        }).unwrap_or(self.mouse)
    }

    pub(crate) fn library_context_menu(&mut self, id: &str, at: (f32, f32)) {
        if crate::private::enabled() { return; }
        self.library.flush(true);
        let Some(e) = self.menu_reading_entry(id) else { return; };
        let available = self.library.store.article(&e).is_ok();
        let original = reading_action_available(&e.source, available, &Hit::Original);
        let reading_here = self.library_home().is_some_and(|h| h.reading.as_ref()
            .is_some_and(|r| r.id == id && r.available));
        let row_index = self.library_home().filter(|h| h.reading.is_none()).and_then(|h|
            self.library_rows_for(&h.input, h.library_ui.filter).iter().position(|e| e.id == id));
        if let Some(h) = self.library_home_mut() {
            if h.reading.is_none() {
                if let Some(index) = row_index { h.sel = index + 1; }
                h.library_ui.selected = Some(id.to_owned());
                h.library_ui.focus = Some(Hit::Row(id.to_owned()));
            }
        }
        let mut rows = Vec::new();
        if available { rows.push((Some(MenuAction::OpenSaved), "Open saved copy".into(), true)); }
        if original {
            rows.push((Some(MenuAction::Original),
                if available { "Open original" } else { "Open original link" }.into(), true));
        }
        rows.push((Some(MenuAction::CopySource),
            if e.source.starts_with("http:") || e.source.starts_with("https:") { "Copy link" } else { "Copy source" }.into(), true));
        rows.push((Some(MenuAction::Edit), "Edit title, link and notes…".into(), true));
        rows.push((Some(MenuAction::Add), "Add item…".into(), true));
        rows.push((None, String::new(), false));
        rows.push((Some(MenuAction::Finished(!e.finished)),
            if e.finished { "Mark unfinished" } else { "Mark finished" }.into(), true));
        rows.push((Some(MenuAction::Archived(!e.archived)),
            if e.archived { "Unarchive" } else { "Archive" }.into(), true));
        if reading_action_available(&e.source, available, &Hit::Refresh) {
            rows.push((Some(MenuAction::Refresh), "Refresh saved copy".into(), true));
        }
        if reading_here {
            rows.push((None, String::new(), false));
            rows.push((Some(MenuAction::Find), "Find in article".into(), true));
            let selected = self.library_home().and_then(|h|h.reading.as_ref())
                .is_some_and(|r| !r.reader.reading_selected_text().is_empty());
            rows.push((Some(MenuAction::CopySelection), "Copy selection".into(), selected));
            rows.push((Some(MenuAction::Smaller), "Smaller text".into(), true));
            rows.push((Some(MenuAction::Larger), "Larger text".into(), true));
        }
        rows.push((None, String::new(), false));
        rows.push((Some(MenuAction::Remove), "Remove…".into(), true));
        self.open_reading_menu(id.to_owned(), at, rows);
    }

    pub(crate) fn library_context_at(&mut self, x: f32, y: f32) -> bool {
        if !self.library_focus_at(x, y) { return false; }
        if self.library_home().is_some_and(|h| h.library_ui.confirm) { return true; }
        let id = self.library_home().and_then(|h| {
            h.library_ui.hits.iter().rev().find_map(|(r, hit)| match hit {
                Hit::Row(id) if r.contains(x, y) => Some(id.clone()),
                _ => None,
            }).or_else(|| h.reading.as_ref().map(|r| r.id.clone()))
        });
        if self.library_home().is_some_and(|h|h.library_ui.draft.is_some()){return true;}
        if let Some(id) = id { self.library_context_menu(&id, (x, y)); }
        else {self.open_reading_menu(String::new(),(x,y),vec![(Some(MenuAction::Add),"Add link or note…".into(),true)]);}
        true // Never forward the blank part of a native library to a browser.
    }

    pub(crate) fn library_context_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        let context = matches!(ev.logical_key, Key::Named(NamedKey::ContextMenu))
            || (matches!(ev.logical_key, Key::Named(NamedKey::F10)) && self.mods.shift_key());
        if !context || ev.state != ElementState::Pressed || self.library_home().is_none()
            || self.mods.control_key() || self.mods.super_key() || self.mods.alt_key()
            || self.palette.is_some() || self.start.is_some() || self.board.open
            || self.me_card.open || self.timeline.is_some() || self.dl_menu || self.splash.is_some()
            || self.look_menu || self.win_menu || self.kinds_menu || self.tab_menu.is_some() {
            return false;
        }
        self.page_menu_keys.insert(ev.physical_key);
        if self.library_home().is_some_and(|h| h.library_ui.confirm) { return true; }
        let id = self.library_home().and_then(|h| h.reading.as_ref().map(|r| r.id.clone())
            .or_else(|| match &h.library_ui.focus { Some(Hit::Row(id)) => Some(id.clone()), _ => h.library_ui.selected.clone() }));
        if let Some(id) = id {
            let at = self.library_context_anchor(&id);
            self.library_context_menu(&id, at);
        } else {self.open_reading_menu(String::new(),self.mouse,vec![(Some(MenuAction::Add),"Add link or note…".into(),true)]);}
        true
    }

    pub(crate) fn library_menu_action(&mut self, id: &str, action: MenuAction, at: (f32, f32)) {
        if crate::private::enabled() || action == MenuAction::Cancel { return; }
        if action==MenuAction::Add {self.library_edit(None);return;}
        if action==MenuAction::Edit {self.library_edit(Some(id));return;}
        // Flush BEFORE showing confirmation, not after it has captured a revision.
        if !matches!(action, MenuAction::ConfirmRemove(_)) { self.library.flush(true); }
        let Some(e) = self.menu_reading_entry(id) else { return; };
        match action {
            MenuAction::OpenSaved => self.read_saved_mode(id, false),
            MenuAction::Original => self.open_reading_source(&e),
            MenuAction::CopySource => self.library_copy(e.source),
            MenuAction::Finished(value) | MenuAction::Archived(value) => {
                let (finished, archived) = if matches!(action, MenuAction::Finished(_)) {
                    (Some(value), None)
                } else { (None, Some(value)) };
                match self.library.store.state(id, finished, archived) {
                    Ok(e) => { self.library.remember(e); self.library_message("Reading state saved."); },
                    Err(e) => self.library_message(format!("State was not saved: {e}")),
                }
            },
            MenuAction::Refresh => self.save_reading_mode(true, Some(id.to_owned())),
            MenuAction::Remove => {
                let title = format!("Remove “{}” from the library?", e.title);
                self.open_reading_menu(id.to_owned(), at, vec![
                    (None, title, false),
                    (None, String::new(), false),
                    (Some(MenuAction::Cancel), "Keep item".into(), true),
                    (Some(MenuAction::ConfirmRemove(e.revision)), "Remove".into(), true),
                ]);
            },
            MenuAction::ConfirmRemove(revision) => self.library_remove_revision(id, revision),
            MenuAction::Find | MenuAction::CopySelection | MenuAction::Smaller | MenuAction::Larger => {
                if !self.library_home().is_some_and(|h| h.reading.as_ref().is_some_and(|r| r.id == id)) {
                    self.library_message("The reader changed. Reopen its options before retrying.");
                    return;
                }
                let hit = match action { MenuAction::Find => Hit::Find, MenuAction::CopySelection => Hit::Copy,
                    MenuAction::Smaller => Hit::Smaller, _ => Hit::Larger };
                self.library_action(hit);
            },
            MenuAction::Cancel|MenuAction::Add|MenuAction::Edit => {},
        }
        self.dirty = true;
    }

    pub(crate) fn library_remove_revision(&mut self, id: &str, revision: u64) {
        if crate::private::enabled() { return; }
        match self.library.store.remove_at_revision(id, revision) {
            Ok(e) => {
                self.library.dirty.remove(id);
                self.library.undo = Some((e.id.clone(), e.revision));
                self.library.remember(e);
                // Close only readers for this id, including split panes. Never
                // make a context-menu operation navigate a different article.
                for tab in &mut self.tabs {
                    for pane in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                        if let Pane::Home(h) = pane {
                            if h.library && h.reading.as_ref().is_some_and(|r| r.id == id) {
                                h.reading = None;
                                h.library_ui.confirm = false;
                                h.library_ui.remove_target = None;
                                h.library_ui.focus = Some(Hit::Search);
                            }
                        }
                    }
                }
                self.library_message("Removed from your library. Undo removal is available.");
            },
            Err(e) => self.library_message(format!("Removal was not applied: {e}")),
        }
    }
}
