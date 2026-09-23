use crate::{
    app::{App, Pane},
    file_viewer::{Config, Kind},
};
impl App {
    pub(crate) fn viewer_config(&self) -> Config {
        Config {
            prefs: self.behavior.viewers.clone(),
            paper: self.paper(),
            ink: self.theme.ink,
            accent: self.surface.signal,
        }
    }
    pub(crate) fn sync_viewer_preferences(&mut self, force: bool) {
        let config = self.viewer_config();
        if !force && self.viewer_config_seen.as_ref() == Some(&config) {
            return;
        }
        self.viewer_config_seen = Some(config.clone());
        for tab in &self.tabs {
            for pane in std::iter::once(&tab.left).chain(tab.right.as_ref()) {
                if let Pane::Web(w) = pane {
                    let s = w.tab.shared.borrow();
                    let (changed, theme_only) = if let Ok(mut c) = s.viewer.write() {
                        let theme_only = c.prefs == config.prefs;
                        if *c != config {
                            *c = config.clone();
                            (true, theme_only)
                        } else {
                            (false, false)
                        }
                    } else {
                        (false, false)
                    };
                    let local = url::Url::parse(&s.url)
                        .ok()
                        .and_then(|u| u.to_file_path().ok())
                        .is_some_and(|p| Kind::of(&p).is_some());
                    drop(s);
                    if local && changed && theme_only && !force {
                        if config.prefs.theme == crate::file_viewer::Theme::Follow {
                            let css = |v: [f32; 4]| {
                                format!(
                                    "rgb({},{},{})",
                                    (v[0] * 255.0) as u8,
                                    (v[1] * 255.0) as u8,
                                    (v[2] * 255.0) as u8
                                )
                            };
                            let colors = serde_json::to_string(&[
                                css(config.paper),
                                css(config.ink),
                                css(config.accent),
                            ])
                            .unwrap();
                            w.tab.eval(&format!("(()=>{{const c={colors},s=document.documentElement.style;['--paper','--ink','--accent'].forEach((k,i)=>s.setProperty(k,c[i]))}})()"));
                        }
                    } else if local && (changed || force) {
                        w.tab.reload();
                    }
                }
            }
        }
    }
    /// File-tree viewing is distinct from the explicit Edit command.
    pub(crate) fn open_document(&mut self, path: &std::path::Path, split: bool) -> bool {
        if crate::private::enabled() {
            return false;
        }
        if crate::protected_state::is_private_path(path){self.open_file(path,split);return true;}
        if !Kind::of(path).is_some_and(|k| self.behavior.viewers.allows(k)) {
            return false;
        }
        let Ok(path) = path.canonicalize() else {
            return false;
        };
        let Ok(url) = url::Url::from_file_path(path) else {
            return false;
        };
        let existing = self.tabs.iter().enumerate().find_map(|(i, t)| {
            [(false, Some(&t.left)), (true, t.right.as_ref())]
                .into_iter()
                .find_map(|(right, p)| {
                    matches!(p,Some(Pane::Web(w)) if w.tab.shared.borrow().url == url.as_str())
                        .then_some((i, right))
                })
        });
        if let Some((i, right)) = existing {
            self.tabs[i].focus_right = right;
            self.activate(i);
            return true;
        }
        if split
            && self
                .tabs
                .get(self.active)
                .is_some_and(|t| t.right.is_none())
        {
            if let Some(w) = self.new_web_pane(url.as_str()) {
                let t = &mut self.tabs[self.active];
                t.right = Some(Pane::Web(w));
                t.focus_right = true;
                self.layout();
                self.dirty = true;
                return true;
            }
        } else {
            self.open_url(url.as_str(), true);
            return true;
        }
        false
    }
}
