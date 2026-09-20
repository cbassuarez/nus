//! One configurable set of sources for Home and the command palette.
use crate::app::{Action, App, PaletteMode, PaletteRow, Pane};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Saved,
    Projects,
    Sessions,
    Shell,
    Web,
    Assistants,
    Activity,
    Layouts,
    Actions,
    Settings,
}
impl Source {
    pub const ALL: [Self; 10] = [
        Self::Saved,
        Self::Projects,
        Self::Sessions,
        Self::Shell,
        Self::Web,
        Self::Assistants,
        Self::Activity,
        Self::Layouts,
        Self::Actions,
        Self::Settings,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Saved => "Saved shortcuts",
            Self::Projects => "Projects",
            Self::Sessions => "Open sessions",
            Self::Shell => "Shell & command history",
            Self::Web => "Web & browser history",
            Self::Assistants => "Assistants",
            Self::Activity => "Work needing attention",
            Self::Layouts => "Saved layouts",
            Self::Actions => "App actions",
            Self::Settings => "Settings",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Shell,
    Web,
    Assistants,
    #[default]
    Mixed,
    Minimal,
}
impl Preset {
    pub const ALL: [Self; 5] = [
        Self::Shell,
        Self::Web,
        Self::Assistants,
        Self::Mixed,
        Self::Minimal,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Shell => "Shell",
            Self::Web => "Web",
            Self::Assistants => "Assistants",
            Self::Mixed => "Mixed",
            Self::Minimal => "Minimal",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    #[default]
    Automatic,
    Shell,
    Web,
    Assistant,
}
impl Route {
    pub const ALL: [Self; 4] = [Self::Automatic, Self::Shell, Self::Web, Self::Assistant];
    pub fn name(self) -> &'static str {
        match self {
            Self::Automatic => "URL or shell",
            Self::Shell => "Shell",
            Self::Web => "Web search",
            Self::Assistant => "Default assistant",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceConfig {
    pub source: Source,
    pub home: bool,
    pub search: bool,
    #[serde(default = "three")]
    pub count: u8,
}
fn three() -> u8 {
    3
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub sources: Vec<SourceConfig>,
    pub route: Route,
    pub home_limit: u8,
    pub search_limit: u8,
    pub wide: bool,
    pub compact: bool,
    pub top: bool,
    pub hints: bool,
    pub saved: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self::preset(Preset::Mixed)
    }
}
impl Config {
    pub fn preset(p: Preset) -> Self {
        use Source::*;
        let home: &[Source] = match p {
            Preset::Shell => &[Saved, Projects, Shell, Sessions],
            Preset::Web => &[Saved, Web, Sessions],
            Preset::Assistants => &[Saved, Assistants, Activity, Projects],
            Preset::Mixed => &[Saved, Projects, Sessions, Assistants, Activity],
            Preset::Minimal => &[],
        };
        let search: &[Source] = match p {
            Preset::Shell => &[Saved, Projects, Shell, Sessions, Layouts, Actions, Settings],
            Preset::Web => &[Saved, Web, Sessions, Actions, Settings],
            Preset::Assistants => &[Saved, Assistants, Projects, Activity, Sessions, Settings],
            Preset::Mixed | Preset::Minimal => &Source::ALL,
        };
        let mut order = home.to_vec();
        for s in Source::ALL {
            if !order.contains(&s) {
                order.push(s);
            }
        }
        Self {
            sources: order
                .into_iter()
                .map(|source| SourceConfig {
                    source,
                    home: home.contains(&source),
                    search: search.contains(&source),
                    count: 3,
                })
                .collect(),
            route: match p {
                Preset::Web => Route::Web,
                Preset::Assistants => Route::Assistant,
                Preset::Shell => Route::Shell,
                _ => Route::Automatic,
            },
            home_limit: 7,
            search_limit: 9,
            wide: false,
            compact: false,
            top: false,
            hints: true,
            saved: Vec::new(),
        }
    }
    pub fn ordered(&self) -> Vec<SourceConfig> {
        let mut out = Vec::new();
        for s in &self.sources {
            if !out.iter().any(|v: &SourceConfig| v.source == s.source) {
                let mut s = s.clone();
                s.count = s.count.clamp(1, 8);
                out.push(s);
            }
        }
        for source in Source::ALL {
            if !out.iter().any(|v| v.source == source) {
                out.push(SourceConfig {
                    source,
                    home: false,
                    search: false,
                    count: 3,
                });
            }
        }
        out
    }
    pub fn matches(&self, p: Preset) -> bool {
        let d = Self::preset(p);
        self.route == d.route
            && self
                .ordered()
                .iter()
                .map(|s| (s.source, s.home, s.search))
                .eq(d.ordered().iter().map(|s| (s.source, s.home, s.search)))
    }
}
pub fn looks_like_url(q: &str) -> bool {
    q.contains("://")
        || q.starts_with("localhost")
        || (q.contains('.')
            && !q.contains(' ')
            && !q.starts_with('.')
            && !q.contains('/')
            && !q.contains('\\'))
}
fn row(text: String, action: Action) -> PaletteRow {
    let num = match &action {
        Action::AssistantDraft(..) | Action::AssistantStart(..) => "*",
        Action::PromptShell(_) | Action::NewTerminal(_) => ">",
        _ => "→",
    };
    PaletteRow {
        num: num.into(),
        text,
        action,
    }
}
fn category(a: &Action) -> Source {
    match a {
        Action::SwitchTab(_) | Action::Reopen | Action::AttachHeld(_) => Source::Sessions,
        Action::RunInShell(_) | Action::NewTerminal(_) | Action::JournalPage => Source::Shell,
        Action::NewBrowser(_) | Action::OpenInPane(_) | Action::OpenItem(..) => Source::Web,
        Action::OpenFolder(_) | Action::Workspace(_) => Source::Projects,
        Action::Ask | Action::Skill(..) | Action::AssistantDraft(..) => Source::Assistants,
        Action::OpenLayout(_) | Action::SaveLayout(_) => Source::Layouts,
        Action::SettingsAt(..) | Action::SettingsRow(..) => Source::Settings,
        _ => Source::Actions,
    }
}
impl App {
    pub(crate) fn palette_rows(&self, mode: PaletteMode, input: &str) -> Vec<PaletteRow> {
        if mode == PaletteMode::Go {
            self.prompt_rows(input)
        } else {
            self.palette_rows_raw(mode, input)
        }
    }
    pub(crate) fn prompt_action(&self, input: &str) -> Option<PaletteRow> {
        let q = input.trim();
        if q.is_empty() {
            return None;
        }
        if q.eq_ignore_ascii_case("home") {
            return Some(row("Home · prompt and background".into(), Action::Home));
        }
        if q.eq_ignore_ascii_case("settings") {
            return Some(row("Open settings".into(), Action::SettingsAt(0, None)));
        }
        if let Some(url) = q
            .strip_prefix("home ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(row(
                format!("Set startup website · {url}"),
                Action::SetHome(url.into()),
            ));
        }
        if std::path::Path::new(q).is_dir() {
            return Some(row(
                format!("Open project · {q}"),
                Action::OpenFolder(q.into()),
            ));
        }
        if std::path::Path::new(q).is_file() {
            return Some(row(format!("Open file · {q}"), Action::OpenFile(q.into())));
        }
        for (i, name) in crate::assistants::NAMES.iter().enumerate() {
            if let Some(rest) = q
                .strip_prefix(&format!("@{}", name.to_lowercase()))
                .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
            {
                return Some(row(
                    format!("Review prompt for {name} · {}", rest.trim()),
                    Action::AssistantDraft(i as u8, rest.trim().into()),
                ));
            }
        }
        if let Some(cmd) = q.strip_prefix('>') {
            return Some(row(
                format!("Run in a new terminal · {}", cmd.trim()),
                Action::PromptShell(cmd.trim().into()),
            ));
        }
        if let Some(query) = q.strip_prefix('?') {
            let (url, _) = Self::url_or_search(query.trim());
            return Some(row(
                format!("Search the web · {}", query.trim()),
                Action::NewBrowser(url),
            ));
        }
        if q.starts_with('@') {
            return None;
        }
        let route = self.behavior.prompt.route;
        if route == Route::Assistant {
            let id = self.default_assistant();
            return Some(row(
                format!(
                    "Review prompt for {} · {q}",
                    crate::assistants::NAMES[id as usize]
                ),
                Action::AssistantDraft(id, q.into()),
            ));
        }
        if route == Route::Web || (route == Route::Automatic && looks_like_url(q)) {
            let (url, _) = Self::url_or_search(q);
            return Some(row(
                format!(
                    "{} · {q}",
                    if looks_like_url(q) {
                        "Open page"
                    } else {
                        "Search the web"
                    }
                ),
                Action::NewBrowser(url),
            ));
        }
        Some(row(
            format!("Run in a new terminal · {q}"),
            Action::PromptShell(q.into()),
        ))
    }
    pub(crate) fn prompt_rows(&self, input: &str) -> Vec<PaletteRow> {
        // Home can animate at display refresh rate. Do not rescan project folders,
        // saved layouts and command history on each painted frame. Tab identity
        // and order belong in the key because session actions contain indices.
        let key = format!(
            "{input}\n{:?}\n{}\n{:?}\n{:?}\n{}\n{:?}",
            self.behavior.prompt,
            self.active,
            self.behavior.ask_backend,
            self.behavior.assistants,
            self.assistant_folder(),
            self.tabs
                .iter()
                .map(|t| (t.id, t.title()))
                .collect::<Vec<_>>()
        );
        if let Some((at, previous, rows)) = self.prompt_cache.borrow().as_ref() {
            if previous == &key && at.elapsed() < std::time::Duration::from_millis(500) {
                return rows.clone();
            }
        }
        let rows = self.build_prompt_rows(input);
        *self.prompt_cache.borrow_mut() = Some((std::time::Instant::now(), key, rows.clone()));
        rows
    }
    fn build_prompt_rows(&self, input: &str) -> Vec<PaletteRow> {
        let q = input.trim();
        let empty = q.is_empty();
        let config = &self.behavior.prompt;
        let limit = if empty {
            config.home_limit.min(12)
        } else {
            config.search_limit.clamp(1, 12)
        } as usize;
        let mut out = Vec::new();
        // Explicit routes are always usable, even if that suggestion source is hidden.
        let explicit = q.starts_with(['>', '?', '@'])
            || q.eq_ignore_ascii_case("home")
            || q.eq_ignore_ascii_case("settings");
        if explicit {
            if let Some(r) = self.prompt_action(q) {
                out.push(r);
            }
            return out;
        }
        let raw = if empty {
            Vec::new()
        } else {
            self.palette_rows_raw(PaletteMode::Go, q)
        };
        if !empty {
            let exact = raw.iter().find(|r| {
                r.text
                    .split(" · ")
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(q))
            });
            if let Some(r) = exact {
                out.push(r.clone());
            }
            if let Some(r) = self.prompt_action(q) {
                if !out.iter().any(|i| i.action == r.action) {
                    out.push(r);
                }
            }
        }
        for source in config
            .ordered()
            .into_iter()
            .filter(|s| if empty { s.home } else { s.search })
        {
            let mut items = Vec::new();
            match source.source {
                Source::Saved => {
                    for saved in &config.saved {
                        if empty || saved.to_lowercase().contains(&q.to_lowercase()) {
                            if let Some(mut r) = self.prompt_action(saved) {
                                r.text = format!("Saved · {}", r.text);
                                items.push(r);
                            }
                        }
                    }
                }
                Source::Projects => items = self.workspace_rows(q),
                Source::Sessions => {
                    for (i, t) in self.tabs.iter().enumerate() {
                        if i != self.active
                            && !matches!(t.left, Pane::Home(_))
                            && (empty || t.title().to_lowercase().contains(&q.to_lowercase()))
                        {
                            items
                                .push(row(format!("Resume · {}", t.title()), Action::SwitchTab(i)));
                        }
                    }
                }
                Source::Assistants => {
                    for (i, name) in crate::assistants::NAMES.iter().enumerate() {
                        items.push(row(
                            if empty {
                                format!("Start with {name} · review a prompt")
                            } else {
                                format!("Ask {name} · {q}")
                            },
                            Action::AssistantDraft(i as u8, q.into()),
                        ));
                    }
                }
                Source::Shell => {
                    if empty {
                        items.push(row(
                            "New terminal · your default shell".into(),
                            Action::NewTerminal(self.behavior.default_profile),
                        ));
                    }
                    {
                        let cwd = self.assistant_folder();
                        for e in crate::journal::entries(&cwd, 30) {
                            let cmd = e.cmd.trim();
                            if !cmd.is_empty()
                                && (empty || cmd.to_lowercase().contains(&q.to_lowercase()))
                            {
                                items.push(row(
                                    format!("Run again · {cmd}"),
                                    Action::PromptShell(cmd.into()),
                                ));
                            }
                        }
                    }
                }
                Source::Web => items = self.history_rows(q, true, 8),
                Source::Activity => {
                    items = self
                        .news
                        .rows
                        .iter()
                        .filter(|r| empty || r.text.to_lowercase().contains(&q.to_lowercase()))
                        .cloned()
                        .collect();
                    if empty && items.is_empty() {
                        items.push(row("Open work in Hatch".into(), Action::Hatch));
                    }
                }
                Source::Layouts => {
                    for (name, path) in crate::layout_file::saved() {
                        if empty || name.to_lowercase().contains(&q.to_lowercase()) {
                            items.push(row(
                                format!("Open layout · {name}"),
                                Action::OpenLayout(path.display().to_string()),
                            ));
                        }
                    }
                }
                Source::Settings => {
                    if empty {
                        items.push(row(
                            "Customize prompt and palette".into(),
                            Action::SettingsAt(crate::settings::SEC_PROMPT, None),
                        ));
                    }
                }
                Source::Actions => {
                    if empty {
                        items.push(row("Home · prompt and background".into(), Action::Home));
                        items.push(row("Open work in Hatch".into(), Action::Hatch));
                    }
                }
            }
            items.extend(
                raw.iter()
                    .filter(|r| category(&r.action) == source.source)
                    .cloned(),
            );
            let mut n = 0;
            for item in items {
                if !out.iter().any(|r: &PaletteRow| r.action == item.action) {
                    out.push(item);
                    n += 1;
                    if n >= source.count {
                        break;
                    }
                }
            }
        }
        if !empty {
            if let Some(r) = self.prompt_action(q) {
                // An exact action match wins over a shell fallback (e.g. "downloads").
                if !out.iter().any(|i| i.action == r.action) {
                    out.push(r);
                }
            }
        }
        out.truncate(limit);
        out
    }
    pub(crate) fn open_prompt_shell(&mut self, command: &str) {
        match self.new_term_pane(false, self.behavior.default_profile) {
            Ok(mut t) => {
                if !command.is_empty() {
                    t.type_at_prompt = Some(format!("{command}\r"));
                }
                let tab = self.make_tab(Pane::Term(t), None);
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
                self.layout();
            }
            Err(e) => self.notice(&format!("Could not open terminal: {e}")),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_separate_home_from_search() {
        let c = Config::preset(Preset::Minimal);
        assert!(c.sources.iter().all(|s| !s.home));
        assert!(c.sources.iter().all(|s| s.search));
        let c = Config::preset(Preset::Shell);
        assert!(
            !c.sources
                .iter()
                .find(|s| s.source == Source::Web)
                .unwrap()
                .search
        );
    }
    #[test]
    fn malformed_order_is_bounded_and_unique() {
        let mut c = Config::default();
        c.sources.push(c.sources[0].clone());
        c.sources[0].count = 255;
        assert_eq!(c.ordered().len(), 10);
        assert_eq!(c.ordered()[0].count, 8);
    }
    #[test]
    fn urls_do_not_eat_relative_commands() {
        assert!(looks_like_url("nus.dev"));
        assert!(looks_like_url("localhost:3000"));
        assert!(!looks_like_url("./script.sh"));
        assert!(!looks_like_url("cargo test"));
    }
}
