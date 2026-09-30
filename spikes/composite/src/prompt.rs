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
            Self::Saved => "Saved commands",
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
            Self::Automatic => "Search, URL or shell",
            Self::Shell => "Shell",
            Self::Web => "Web search",
            Self::Assistant => "Default assistant",
        }
    }
}
/// Where a search goes. The engine's own query URL, or a template of the
/// user's with `%s` where the words go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    #[default]
    Google,
    DuckDuckGo,
    Bing,
    Brave,
    Kagi,
    Startpage,
    /// `Config::search_url`, a template with `%s`.
    Custom,
}
impl SearchEngine {
    pub const ALL: [Self; 7] = [
        Self::Google,
        Self::DuckDuckGo,
        Self::Bing,
        Self::Brave,
        Self::Kagi,
        Self::Startpage,
        Self::Custom,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Bing => "Bing",
            Self::Brave => "Brave",
            Self::Kagi => "Kagi",
            Self::Startpage => "Startpage",
            Self::Custom => "Custom",
        }
    }
    /// The query template: `%s` is the words, percent-encoded.
    pub fn template(self) -> &'static str {
        match self {
            Self::Google => "https://www.google.com/search?q=%s",
            Self::DuckDuckGo => "https://duckduckgo.com/?q=%s",
            Self::Bing => "https://www.bing.com/search?q=%s",
            Self::Brave => "https://search.brave.com/search?q=%s",
            Self::Kagi => "https://kagi.com/search?q=%s",
            Self::Startpage => "https://www.startpage.com/do/search?q=%s",
            Self::Custom => "",
        }
    }
}
/// The words as a query string: spaces to `+`, the rest percent-encoded
/// the way a form submits them.
pub fn encode_query(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for b in q.trim().bytes() {
        match b {
            b' ' => out.push('+'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'*' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub saved_names: std::collections::BTreeMap<String, String>,
    pub saved_preview: bool,
    pub saved_library: bool,
    pub saved_run: bool,
    /// SEARCH ENGINE: where `?` and the search row go.
    pub engine: SearchEngine,
    /// The custom engine's template, `%s` for the words.
    pub search_url: String,
}
impl Default for Config {
    fn default() -> Self {
        Self::preset(Preset::Mixed)
    }
}
impl Config {
    /// Only upgrade the exact former default source arrangement; retain custom
    /// source choices, saved commands, routes and search-engine preferences.
    pub fn migrate_default_sources(&mut self) {
        use Source::*;
        let old_home = [Saved, Projects, Sessions, Assistants, Activity];
        let old_order = [Saved, Projects, Sessions, Assistants, Activity, Shell, Web, Layouts, Actions, Settings];
        let old: Vec<_> = old_order.into_iter().map(|source| SourceConfig {source,home:old_home.contains(&source),search:true,count:3}).collect();
        if self.route == Route::Automatic && self.sources == old { self.sources = Self::preset(Preset::Mixed).sources; }
    }
    pub fn preset(p: Preset) -> Self {
        use Source::*;
        let home: &[Source] = match p {
            Preset::Shell => &[Saved, Projects, Shell, Sessions],
            Preset::Web => &[Saved, Web, Sessions],
            Preset::Assistants => &[Saved, Assistants, Activity, Projects],
            Preset::Mixed => &[Web, Saved, Projects, Sessions, Shell, Activity],
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
            saved_names: Default::default(),
            saved_preview: true,
            saved_library: true,
            saved_run: false,
            engine: SearchEngine::default(),
            search_url: String::new(),
        }
    }
    /// The URL that searches for `q`: the engine's, or Google's when the
    /// custom template has no `%s` to put the words in.
    pub fn search_url(&self, q: &str) -> String {
        let template = match self.engine {
            SearchEngine::Custom if self.search_url.contains("%s") => self.search_url.as_str(),
            SearchEngine::Custom => SearchEngine::Google.template(),
            e => e.template(),
        };
        template.replacen("%s", &encode_query(q), 1)
    }
    /// The engine's front door: the origin of its query URL.
    pub fn search_home(&self) -> String {
        let url = self.search_url("");
        let end = url.find("://").map(|i| i + 3).unwrap_or(0);
        let path = url[end..].find('/').map(|i| end + i).unwrap_or(url.len());
        format!("{}/", &url[..path])
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
/// A line only a shell could mean: a path to run, a pipe or a chain, a
/// flag, a variable. Plain words are never taken for a command, so a
/// word that happens to name a program (`y`, `yes`, `open maps`) is a
/// harmless search, not something run.
pub fn shell_syntax(q: &str) -> bool {
    let q = q.trim();
    q.starts_with(['.', '/', '~', '$'])
        || [" | ", "&&", "||", " > ", " >> ", " < ", ";", "`", "$(", "=", " -"].iter().any(|t| q.contains(t))
}

pub fn looks_like_url(q: &str) -> bool {
    pinnable(q)
}
/// An address worth pinning: a scheme, localhost, or a dotted host, with
/// whatever path, query or fragment follows it (`github.com/me/repo`).
/// Existing files and directories are resolved before URL routing at the
/// prompt; explicit relative paths never look like a host.
pub fn pinnable(q: &str) -> bool {
    if q.is_empty() || q.contains(char::is_whitespace) || q.contains('\\') {
        return false;
    }
    if q.contains("://") || q.starts_with("localhost") {
        return true;
    }
    let host = q.split(['/', '?', '#']).next().unwrap_or("");
    host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
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
pub(crate) type Rows = std::rc::Rc<[PaletteRow]>;

// Compare the rendered title piece by piece without building a temporary String.
// Exact comparisons avoid hash collisions and scattered revision invalidation.
pub(crate) struct TitleMatch<'a>(pub &'a str);
impl std::fmt::Write for TitleMatch<'_> {
    fn write_str(&mut self, part: &str) -> std::fmt::Result {
        self.0 = self.0.strip_prefix(part).ok_or(std::fmt::Error)?;
        Ok(())
    }
}
struct TabStamp { id: u64, title: String, right: bool, cwd: [Option<String>; 2] }
impl TabStamp {
    fn cwd(tab: &crate::app::Tab) -> [Option<&str>; 2] {
        fn cwd(p: Option<&Pane>) -> Option<&str> {
            match p { Some(Pane::Term(t)) => t.cwd.as_deref(), _ => None }
        }
        [cwd(Some(&tab.left)), cwd(tab.right.as_ref())]
    }
    fn new(tab: &crate::app::Tab) -> Self {
        Self { id: tab.id, title: tab.title(), right: tab.focus_right, cwd: Self::cwd(tab).map(|v|v.map(str::to_owned)) }
    }
    fn matches(&self, tab: &crate::app::Tab) -> bool {
        self.id == tab.id && self.right == tab.focus_right && tab.title_matches(&self.title)
            && self.cwd.each_ref().map(|v|v.as_deref()) == Self::cwd(tab)
    }
}
pub(crate) struct PromptCache {
    at: std::time::Instant,
    input: String,
    config: Config,
    active: usize,
    backend: String,
    assistants: crate::assistants::Config,
    workspace: Option<std::path::PathBuf>,
    tabs: Vec<TabStamp>,
    rows: Rows,
}
impl PromptCache {
    fn matches(&self, app: &App, input: &str) -> bool {
        self.input == input && self.config == app.behavior.prompt && self.active == app.active
            && self.backend == app.behavior.ask_backend && self.assistants == app.behavior.assistants
            && self.workspace == app.workspace && self.tabs.len() == app.tabs.len()
            && self.tabs.iter().zip(&app.tabs).all(|(a,b)| a.matches(b))
            && crate::clock::since(self.at) < std::time::Duration::from_millis(500)
    }
}

impl App {
    pub(crate) fn palette_rows(&self, mode: PaletteMode, input: &str) -> Vec<PaletteRow> {
        match mode {
            PaletteMode::Go => self.prompt_rows(input),
            // One prompt, not two: a new tab's palette leads with what a
            // new tab is (shells, a page, open ports) and then has all of
            // the prompt (saved commands, projects, sessions, assistants),
            // with nothing listed twice.
            PaletteMode::New => {
                let mut rows = self.palette_rows_raw(PaletteMode::New, input);
                for r in self.prompt_rows(input) {
                    if !rows.iter().any(|x| x.action == r.action) {
                        rows.push(r);
                    }
                }
                rows.truncate(16);
                rows
            }
            _ => self.palette_rows_raw(mode, input),
        }
    }
    pub(crate) fn prompt_action(&self, input: &str) -> Option<PaletteRow> {
        let q = input.trim();
        if q.is_empty() {
            return None;
        }
        if matches!(q.to_lowercase().as_str(), "saved commands" | "saved shortcuts" | "commands") {
            return Some(row("Saved commands · open your collection".into(), Action::SettingsAt(crate::settings::SEC_SAVED, None)));
        }
        // The word for a shell opens one; it is never typed into it.
        if is_new_shell(q) {
            return Some(row("New shell · your default shell".into(), Action::NewTerminal(self.behavior.default_profile)));
        }
        if q.eq_ignore_ascii_case("home") {
            return Some(row("Home · prompt and background".into(), Action::Home));
        }
        if q.eq_ignore_ascii_case("settings") {
            return Some(row("Open settings".into(), Action::SettingsAt(0, None)));
        }
        if let Some(url) = q.strip_prefix("pin ").map(str::trim).filter(|s| pinnable(s)) {
            return Some(row(format!("Pin to sidebar · {url}"), Action::PinUrl(url.into())));
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
        // `search https://example.com/?q=%s`: a search engine of your own.
        if let Some(template) = q
            .strip_prefix("search ")
            .map(str::trim)
            .filter(|s| s.contains("%s") && looks_like_url(s))
        {
            return Some(row(
                format!("Set search engine · {template}"),
                Action::SetSearch(template.into()),
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
            let url = self.behavior.prompt.search_url(query.trim());
            return Some(row(
                format!("Search the web · {}", query.trim()),
                Action::NewBrowser(url),
            ));
        }
        if q.starts_with('@') {
            return None;
        }
        // An address is explicit intent, regardless of the preferred route for
        // ordinary words. Shell and assistant prefixes still take precedence.
        if looks_like_url(q) { return Some(row(format!("Open page · {q}"), Action::NewBrowser(self.url_or_search(q).0))); }
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
        if route == Route::Web || (route == Route::Automatic && !shell_syntax(q)) {
            let (url, _) = self.url_or_search(q);
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
    /// The search row: what the line says, asked of the engine. Beside the
    /// route's own row whenever the line is words rather than an address,
    /// so a search is one arrow away whichever way Enter goes.
    fn prompt_search(&self, q: &str) -> Option<PaletteRow> {
        let q = q.trim();
        if q.is_empty() || q.starts_with(['>', '?', '@']) || looks_like_url(q) || std::path::Path::new(q).exists() {
            return None;
        }
        Some(row(
            format!("Search {} · {q}", self.behavior.prompt.engine.name()),
            Action::NewBrowser(self.behavior.prompt.search_url(q)),
        ))
    }
    pub(crate) fn prompt_rows(&self, input: &str) -> Vec<PaletteRow> {
        self.prompt_rows_shared(input).to_vec()
    }
    pub(crate) fn prompt_rows_shared(&self, input: &str) -> Rows {
        if crate::private::enabled() { return self.private_rows(input).into(); }
        // Reading commands bypass route/source settings.
        if input.trim().get(..8).is_some_and(|s| s.eq_ignore_ascii_case("reading:")) {
            return self.palette_rows_raw(PaletteMode::Go, input).into();
        }
        if let Some(cached) = self.prompt_cache.borrow().iter().find(|c| c.matches(self, input)) {
            return cached.rows.clone();
        }
        let rows: Rows = self.build_prompt_rows(input).into();
        let entry = PromptCache {
            at: crate::clock::now(), input: input.into(), config: self.behavior.prompt.clone(),
            active: self.active, backend: self.behavior.ask_backend.clone(),
            assistants: self.behavior.assistants.clone(), workspace: self.workspace.clone(),
            tabs: self.tabs.iter().map(TabStamp::new).collect(), rows: rows.clone(),
        };
        // Two entries let split Home panes keep independent queries without
        // evicting each other every frame. External sources still refresh at 500ms.
        let mut cache = self.prompt_cache.borrow_mut();
        cache.retain(|c| c.input != input);
        if cache.len() == 2 { cache.pop_front(); }
        cache.push_back(entry);
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
            || q.eq_ignore_ascii_case("settings")
            || is_new_shell(q);
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
        let saved_exact = (!empty && config.ordered().iter().any(|s| s.source == Source::Saved && s.search))
            .then(|| config.saved.iter().enumerate().find(|(_, value)| config.saved_names.get(*value).unwrap_or(value).eq_ignore_ascii_case(q)))
            .flatten()
            .map(|(i, value)| PaletteRow { num: "saved".into(), text: config.saved_names.get(value).unwrap_or(value).clone(), action: Action::SavedUse(i, config.saved_run) });
        if !empty {
            if let Some(saved) = saved_exact.clone().filter(|_| !config.saved_run) {
                out.push(saved);
            }
            // nus's own verb, named exactly: harmless, so first.
            let exact = raw.iter().find(|r| {
                r.text
                    .split(" · ")
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(q))
            });
            if let Some(r) = exact {
                out.push(r.clone());
            }
            // Then the route: for plain words, a search. Nothing typed as
            // words runs as a command on Enter.
            if let Some(r) = self.prompt_action(q) {
                if !out.iter().any(|i| i.action == r.action) {
                    out.push(r);
                }
            }
            if let Some(r) = self.prompt_search(q) {
                if !out.iter().any(|i| i.action == r.action) {
                    out.push(r);
                }
            }
            // The shell stays one arrow away.
            if self.behavior.prompt.route == Route::Automatic && !looks_like_url(q) && !std::path::Path::new(q).exists() {
                let shell = row(format!("Run in a new terminal · {q}"), Action::PromptShell(q.into()));
                if !out.iter().any(|i| i.action == shell.action) {
                    out.push(shell);
                }
            }
            // A saved command by name that would run waits below the search;
            // one that only goes onto the line leads, above.
            if let Some(saved) = saved_exact.filter(|_| config.saved_run) {
                out.push(saved);
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
                    if config.saved_library && (empty || "saved commands".contains(&q.to_lowercase())) {
                        items.push(PaletteRow { num: "saved-library".into(), text: "Saved commands".into(), action: Action::SettingsAt(crate::settings::SEC_SAVED, None) });
                    }
                    for (i, saved) in config.saved.iter().enumerate() {
                        let name = config.saved_names.get(saved).map(String::as_str).unwrap_or(saved);
                        if empty || format!("{name} {saved}").to_lowercase().contains(&q.to_lowercase()) {
                            items.push(PaletteRow { num: "saved".into(), text: name.into(), action: Action::SavedUse(i, config.saved_run) });
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
                Source::Web => {
                    if empty {
                        items.push(row("Search the web · type a question or keywords".into(), Action::PromptInput("? ".into())));
                        items.push(row("Visit a website · type an address".into(), Action::PromptInput("https://".into())));
                    }
                    items.extend(self.history_rows(q, true, 8));
                },
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
            // The palette's own search row says the same as the prompt's.
            let search = self.behavior.prompt.search_url(q);
            items.extend(
                raw.iter()
                    .filter(|r| category(&r.action) == source.source)
                    .filter(|r| !matches!(&r.action, Action::OpenInPane(u) | Action::NewBrowser(u) if *u == search))
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
                    t.type_origin = Some(crate::finish_work::Origin::NusAction);
                }
                let tab = self.make_tab(Pane::Term(t), None);
                self.tabs.push(tab);
                self.activate(self.tabs.len() - 1);
                self.layout();
            }
            Err(e) => self.notice_problem("Could Not Open Terminal", e.to_string()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pins_take_paths() {
        for ok in ["github.com/me/repo", "news.ycombinator.com/item?id=1", "localhost:3000/admin", "https://x.dev/a#b", "example.com"] {
            assert!(pinnable(ok), "{ok}");
        }
        for no in ["", "hello", "two words.com", ".hidden/x", "C:\\x.y"] {
            assert!(!pinnable(no), "{no}");
        }
    }

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
    fn searches_go_to_the_engine() {
        let mut c = Config::default();
        assert_eq!(c.engine, SearchEngine::Google);
        assert_eq!(c.search_url("rust wgpu"), "https://www.google.com/search?q=rust+wgpu");
        assert_eq!(c.search_home(), "https://www.google.com/");
        c.engine = SearchEngine::DuckDuckGo;
        assert_eq!(c.search_url("a&b #1"), "https://duckduckgo.com/?q=a%26b+%231");
        assert_eq!(c.search_home(), "https://duckduckgo.com/");
        // A custom template needs a %s; without one Google answers.
        c.engine = SearchEngine::Custom;
        c.search_url = "https://example.org/find?words=%s&lang=en".into();
        assert_eq!(c.search_url("é"), "https://example.org/find?words=%C3%A9&lang=en");
        c.search_url = "https://example.org/".into();
        assert_eq!(c.search_url("x"), "https://www.google.com/search?q=x");
    }
    #[test]
    fn urls_do_not_eat_relative_commands() {
        assert!(looks_like_url("nus.dev"));
        assert!(looks_like_url("localhost:3000"));
        assert!(looks_like_url("example.com/path?q=1"));
        assert!(!looks_like_url("./script.sh"));
        assert!(!looks_like_url("src/main.rs"));
        assert!(!looks_like_url("../example.com/path"));
        assert!(!looks_like_url("cargo test"));
    }
    #[test]
    fn words_search_and_only_shell_syntax_runs() {
        for words in ["y", "yes", "youtube", "open maps", "rust borrow checker", "what is 2+2"] {
            assert!(!shell_syntax(words), "{words}");
        }
        for cmd in ["./build.sh", "~/bin/x", "/usr/bin/env", "ls -la", "cat a | grep b", "make && make install", "FOO=1 cargo run", "$EDITOR notes"] {
            assert!(shell_syntax(cmd), "{cmd}");
        }
    }
}

/// `shell`, `new shell`, `terminal`, `new terminal`: a new shell, by name.
pub(crate) fn is_new_shell(q: &str) -> bool {
    matches!(q.trim().to_lowercase().as_str(), "shell" | "new shell" | "terminal" | "new terminal")
}

/// Browser modifiers are shared by the home prompt and every palette mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination { Current, Tab, Window }
pub(crate) fn destination(mods: winit::keyboard::ModifiersState, mac: bool) -> Destination {
    if mods.shift_key() { Destination::Window }
    else if if mac { mods.super_key() } else { mods.alt_key() || mods.control_key() } { Destination::Tab }
    else { Destination::Current }
}
impl App {
    /// True means an explicitly requested separate destination: keep the home
    /// pane that initiated it. Ordinary actions use the common action runner.
    pub(crate) fn dispatch_prompt_action(&mut self, action: Action) -> bool {
        if let Action::NewBrowser(url) | Action::OpenInPane(url) = &action {
            if !url.is_empty() {
                match destination(self.mods, cfg!(target_os="macos")) {
                    Destination::Window => { self.new_window_urls.push(url.clone()); self.new_window_request=true; return true; }
                    Destination::Tab => { self.open_url(url,true); return true; }
                    Destination::Current => {}
                }
            }
        }
        self.run(action); false
    }
    pub(crate) fn insert_prompt_input(&mut self, input: String) {
        if let Some(Pane::Home(h)) = self.tabs.get_mut(self.active).map(|t|t.focused()) {
            if !h.library { h.input=input; h.cur=Default::default(); h.sel=0; h.row_start=0; self.dirty=true; return; }
        }
        self.open_palette(PaletteMode::Go);
        self.palette=Some((PaletteMode::Go,input));
    }
}
#[cfg(test)] mod destination_tests {
    use super::*;
    use winit::keyboard::ModifiersState as M;
    #[test] fn browser_modifiers_match_on_all_platforms() {
        assert_eq!(destination(M::empty(),true),Destination::Current);
        assert_eq!(destination(M::SUPER,true),Destination::Tab);
        assert_eq!(destination(M::CONTROL,false),Destination::Tab);
        assert_eq!(destination(M::ALT,false),Destination::Tab);
        assert_eq!(destination(M::CONTROL,true),Destination::Current);
        assert_eq!(destination(M::SHIFT|M::SUPER,true),Destination::Window);
        assert_eq!(destination(M::SHIFT|M::CONTROL,false),Destination::Window);
    }
    #[test] fn mixed_prioritizes_web_without_changing_custom_choices() {
        let c=Config::preset(Preset::Mixed);
        assert_eq!(c.sources[0].source,Source::Web);
        assert!(c.sources.iter().any(|s|s.source==Source::Assistants && s.search && !s.home));
        let mut custom=Config::preset(Preset::Assistants); let original=custom.clone();
        custom.migrate_default_sources(); assert_eq!(custom,original);
    }
    #[test] fn legacy_default_upgrade_preserves_saved_commands_and_search_settings() {
        use Source::*;
        let mut c=Config::default();
        c.sources=[Saved,Projects,Sessions,Assistants,Activity,Shell,Web,Layouts,Actions,Settings].into_iter()
            .map(|source|SourceConfig{source,home:[Saved,Projects,Sessions,Assistants,Activity].contains(&source),search:true,count:3}).collect();
        c.saved=vec!["> cargo test".into()];c.engine=SearchEngine::Kagi;c.home_limit=5;
        let mut customized=c.clone();customized.sources[0].count=4;
        let unchanged=customized.clone();customized.migrate_default_sources();assert_eq!(customized,unchanged);
        c.migrate_default_sources();
        assert_eq!(c.sources,Config::default().sources);
        assert_eq!(c.saved,["> cargo test"]);assert_eq!(c.engine,SearchEngine::Kagi);assert_eq!(c.home_limit,5);
    }
}
