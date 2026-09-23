//! Pure selection logic used by the native/CEF context menu, not a test copy.
pub type Item = (i32, String, bool);

pub fn enabled(items: &[Item], id: i32) -> bool {
    items.iter().any(|(key, text, on)| *key == id && *on && !text.is_empty())
}

pub fn step(items: &[Item], selected: Option<i32>, backwards: bool) -> Option<i32> {
    let ids: Vec<i32> = items.iter().filter(|(_, text, on)| *on && !text.is_empty()).map(|(id, _, _)| *id).collect();
    if ids.is_empty() { return None; }
    let index = match selected.and_then(|id| ids.iter().position(|key| *key == id)) {
        Some(i) if backwards => if i == 0 { ids.len() - 1 } else { i - 1 },
        Some(i) => (i + 1) % ids.len(),
        None if backwards => ids.len() - 1,
        None => 0,
    };
    Some(ids[index])
}

pub fn edge(items: &[Item], last: bool) -> Option<i32> {
    let mut ids = items.iter().filter(|(_, text, on)| *on && !text.is_empty()).map(|(id, _, _)| *id);
    if last { ids.next_back() } else { ids.next() }
}

pub fn prefix(items: &[Item], selected: Option<i32>, query: &str) -> Option<i32> {
    if query.is_empty() || items.is_empty() { return selected; }
    let start = selected.and_then(|id| items.iter().position(|(key, _, _)| *key == id)).map_or(0, |i| i + 1);
    let query = query.to_lowercase();
    for offset in 0..items.len() {
        let (id, text, on) = &items[(start + offset) % items.len()];
        if *on && !text.is_empty() && text.to_lowercase().starts_with(&query) { return Some(*id); }
    }
    selected.filter(|id| enabled(items, *id))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rows() -> Vec<Item> {
        vec![(1, "Disabled".into(), false), (-1, "".into(), false),
            (2, "Open".into(), true), (3, "Archive".into(), true),
            (4, "".into(), true), (5, "Remove".into(), false)]
    }
    #[test]
    fn keyboard_wraps_and_skips_disabled_rows_and_separators() {
        let r = rows();
        assert_eq!(step(&r, None, false), Some(2));
        assert_eq!(step(&r, Some(2), false), Some(3));
        assert_eq!(step(&r, Some(3), false), Some(2));
        assert_eq!(step(&r, Some(2), true), Some(3));
        assert_eq!(step(&r, Some(1), false), Some(2));
    }
    #[test]
    fn empty_and_all_disabled_menus_have_no_selection() {
        assert_eq!(step(&[], None, true), None);
        assert_eq!(step(&[(1, "no".into(), false)], Some(1), false), None);
    }
    #[test]
    fn home_end_and_dispatch_use_the_same_eligibility() {
        let r = rows();
        assert_eq!(edge(&r, false), Some(2));
        assert_eq!(edge(&r, true), Some(3));
        assert!(!enabled(&r, 4));
        assert!(!enabled(&r, 1));
        assert!(!enabled(&r, 999));
        assert!(enabled(&r, 3));
    }
    #[test]
    fn typeahead_is_case_insensitive_and_does_not_select_disabled_actions() {
        let r = rows();
        assert_eq!(prefix(&r, Some(2), "aR"), Some(3));
        assert_eq!(prefix(&r, None, "Remove"), None);
        assert_eq!(prefix(&r, Some(3), "Open"), Some(2));
    }
}
