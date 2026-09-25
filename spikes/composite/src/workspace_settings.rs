//! Deep controls for assistants, typography and the shared prompt, in nus's native settings.
use super::*;
use crate::{
    assistants::{self, Field},
    fonts::{Family, Weight},
    prompt::{Preset, Route, SearchEngine, Source},
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    AssistantTab(usize),
    Tools(u8),
    /// Connect (true) or disconnect an assistant's hooks: the Ledger's feed.
    Hooks(u8, bool),
    Check,
    Default(u8),
    Draft(u8),
    Setup(u8, u8),
    Edit(Field),
    Memory,
    Template(u8),
    Session(usize),
    Work,
    PromptPreset(Preset),
    Source(Source, u8),
    Move(Source, i8),
    Count(Source, i8),
    Limit(bool, i8),
    Route(Route),
    Engine(SearchEngine),
    EditSearchUrl,
    Layout(u8),
    Pin,
    Unpin(usize),
    SavedLibrary,
    SavedOption(u8),
    SavedEdit(usize, bool),
    SavedUse(usize, bool),
    SavedMove(usize, i8),
    SavedCopy(usize),
    SavedRun(bool),
    Home,
    Pairing(u8),
    EditorFont(Family),
    EditorWeight(Weight),
    TypeSize(u8, i8),
    TypeLine(u8, i8),
    Tracking(i8),
    ResetType,
}
fn hit(h: Hit) -> super::Hit {
    super::Hit::Workspace(h)
}
fn strip(items: Vec<(&str, Hit, bool)>) -> Control {
    Control::Strip(
        items
            .into_iter()
            .map(|(n, h, on)| (n.into(), hit(h), on))
            .collect(),
    )
}
fn info(s: impl Into<String>) -> (String, Control) {
    (String::new(), Control::Info(s.into()))
}
fn buttons(items: Vec<(&str, Hit)>) -> Control {
    strip(items.into_iter().map(|(n, h)| (n, h, false)).collect())
}
pub fn label(h: Hit) -> String {
    match h {
        Hit::Tools(i) => format!(
            "Review {} connection to nus tools",
            assistants::NAMES[i as usize]
        ),
        Hit::Hooks(i, on) => format!("{} {} status hooks", if on { "Connect" } else { "Disconnect" }, assistants::NAMES[i as usize]),
        Hit::AssistantTab(i) => format!(
            "Assistants · {}",
            ["Connections", "Work", "Context", "Advanced"][i]
        ),
        Hit::Check => "Check assistant connections".into(),
        Hit::Default(i) => format!("Use {} by default", assistants::NAMES[i as usize]),
        Hit::Draft(i) => format!("Start {} session", assistants::NAMES[i as usize]),
        Hit::Setup(i, k) => format!(
            "{} {}",
            assistants::NAMES[i as usize],
            match k {
                0 => "installation guide",
                1 => "sign in or start service",
                _ => "diagnose in terminal",
            }
        ),
        Hit::Edit(field) => format!("Edit {field:?}"),
        Hit::Memory => "Edit assistant memory".into(),
        Hit::Template(i) => format!(
            "Draft {}",
            ["a task", "a plan", "a code review"][i as usize]
        ),
        Hit::Session(i) => format!("Resume session {}", i + 1),
        Hit::Work => "Open work in Hatch".into(),
        Hit::PromptPreset(p) => format!("Prompt preset · {}", p.name()),
        Hit::Source(s, k) => format!(
            "{} · {}",
            s.name(),
            if k == 0 {
                "show on home"
            } else {
                "include in search"
            }
        ),
        Hit::Move(s, k) => format!("Move {} {}", s.name(), if k < 0 { "up" } else { "down" }),
        Hit::Count(s, k) => format!(
            "{} {} suggestions",
            s.name(),
            if k < 0 { "fewer" } else { "more" }
        ),
        Hit::Limit(home, k) => format!(
            "{} {} results",
            if home { "Home" } else { "Search" },
            if k < 0 { "fewer" } else { "more" }
        ),
        Hit::Route(r) => format!("Enter routes to {}", r.name()),
        Hit::Engine(e) => format!("Search with {}", e.name()),
        Hit::EditSearchUrl => "Edit the custom search engine".into(),
        Hit::Layout(k) => [
            "Wide prompt",
            "Compact suggestions",
            "Prompt near top",
            "Show route keys at the foot of Home",
        ][k as usize]
            .into(),
        Hit::Pin => "Add a saved prompt shortcut".into(),
        Hit::Unpin(i) => format!("Remove saved shortcut {}", i + 1),
        Hit::SavedLibrary => "Manage saved commands".into(),
        Hit::SavedOption(k) => ["Show saved command previews", "Show collection entry", "Run saved shell commands on activation"][k as usize].into(),
        Hit::SavedEdit(i, name) => format!("{} saved command {}", if name {"Rename"} else {"Edit"}, i+1),
        Hit::SavedUse(i, run) => format!("{} saved command {}", if run {"Run"} else {"Open or insert"}, i+1),
        Hit::SavedMove(i, d) => format!("Move saved command {} {}", i+1, if d<0 {"up"} else {"down"}),
        Hit::SavedCopy(i) => format!("Copy saved command {}",i+1),
        Hit::SavedRun(run) => if run {"Run saved shell commands on activation"}else{"Insert saved shell commands for review"}.into(),
        Hit::Home => "Open Home prompt".into(),
        Hit::Pairing(i) => format!(
            "Font pairing · {}",
            ["Classic", "Clear", "Areal", "Expressive"][i as usize]
        ),
        Hit::EditorFont(f) => format!("Editor font · {}", f.name()),
        Hit::EditorWeight(w) => format!("Editor weight · {}", w.name()),
        Hit::TypeSize(r, k) => format!(
            "{} text {}",
            ["Interface", "Terminal", "Editor"][r as usize],
            if k < 0 { "smaller" } else { "larger" }
        ),
        Hit::TypeLine(r, k) => format!(
            "{} lines {}",
            if r == 1 { "Terminal" } else { "Editor" },
            if k < 0 { "tighter" } else { "looser" }
        ),
        Hit::Tracking(k) => format!(
            "Terminal columns {}",
            if k < 0 { "tighter" } else { "wider" }
        ),
        Hit::ResetType => "Reset typography".into(),
    }
}
impl App {
    pub(super) fn apply_workspace_setting(&mut self, h: Hit) {
        match h {
            Hit::Tools(i) => self.review_assistant_tools(i),
            Hit::Hooks(i, on) => self.review_assistant_hooks(i, on),
            Hit::AssistantTab(i) => {
                self.assistants.tab = i;
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left)
                {
                    s.scroll = 0.0;
                }
            }
            Hit::Check => self.assistants.refresh(self.behavior.assistants.clone()),
            Hit::Default(i) => self.behavior.ask_backend = assistants::BINS[i as usize].into(),
            Hit::Draft(i) => self.draft_assistant(i, ""),
            Hit::Setup(i, k) => self.assistant_setup(i, k),
            Hit::Edit(f) => self.edit_preference(f),
            Hit::Memory => self.assistant_memory(),
            Hit::Template(k) => {
                let id = self.default_assistant();
                self.draft_assistant(
                    id,
                    [
                        "",
                        "Make a plan for ",
                        "Review the current changes. Explain issues before making edits.",
                    ][k as usize],
                );
            }
            Hit::Session(i) => self.activate(i),
            Hit::Work => self.run(crate::app::Action::Hatch),
            Hit::PromptPreset(p) => {
                // Shortcuts and the engine are yours, not the preset's.
                let saved = std::mem::take(&mut self.behavior.prompt.saved);
                let names = std::mem::take(&mut self.behavior.prompt.saved_names);
                let options = (self.behavior.prompt.saved_preview,self.behavior.prompt.saved_library,self.behavior.prompt.saved_run);
                let (engine, search_url) = (self.behavior.prompt.engine, std::mem::take(&mut self.behavior.prompt.search_url));
                self.behavior.prompt = crate::prompt::Config::preset(p);
                self.behavior.prompt.saved = saved;
                self.behavior.prompt.saved_names = names;
                (self.behavior.prompt.saved_preview,self.behavior.prompt.saved_library,self.behavior.prompt.saved_run)=options;
                self.behavior.prompt.engine = engine;
                self.behavior.prompt.search_url = search_url;
            }
            Hit::Source(source, k) => {
                self.behavior.prompt.sources = self.behavior.prompt.ordered();
                if let Some(s) = self
                    .behavior
                    .prompt
                    .sources
                    .iter_mut()
                    .find(|s| s.source == source)
                {
                    if k == 0 {
                        s.home = !s.home;
                    } else {
                        s.search = !s.search;
                    }
                }
            }
            Hit::Move(source, delta) => {
                let v = self.behavior.prompt.ordered();
                self.behavior.prompt.sources = v;
                let v = &mut self.behavior.prompt.sources;
                if let Some(i) = v.iter().position(|s| s.source == source) {
                    let j = (i as isize + delta as isize).clamp(0, v.len() as isize - 1) as usize;
                    v.swap(i, j);
                }
            }
            Hit::Count(source, delta) => {
                self.behavior.prompt.sources = self.behavior.prompt.ordered();
                if let Some(s) = self
                    .behavior
                    .prompt
                    .sources
                    .iter_mut()
                    .find(|s| s.source == source)
                {
                    s.count = (s.count as i16 + delta as i16).clamp(1, 8) as u8;
                }
            }
            Hit::Limit(home, delta) => {
                let n = if home {
                    &mut self.behavior.prompt.home_limit
                } else {
                    &mut self.behavior.prompt.search_limit
                };
                *n = (*n as i16 + delta as i16).clamp(if home { 0 } else { 1 }, 12) as u8;
            }
            Hit::Route(r) => self.behavior.prompt.route = r,
            Hit::Engine(e) => {
                self.behavior.prompt.engine = e;
                // Custom with nothing to fill: ask for the template.
                if e == SearchEngine::Custom && !self.behavior.prompt.search_url.contains("%s") {
                    self.apply_workspace_setting(Hit::EditSearchUrl);
                }
            }
            Hit::EditSearchUrl => {
                self.open_palette(crate::app::PaletteMode::Go);
                if let Some((_, input)) = self.palette.as_mut() {
                    let cur = &self.behavior.prompt.search_url;
                    *input = format!("search {}", if cur.contains("%s") { cur.as_str() } else { "https://example.com/search?q=%s" });
                }
            }
            Hit::Layout(k) => {
                let c = &mut self.behavior.prompt;
                let v = match k {
                    0 => &mut c.wide,
                    1 => &mut c.compact,
                    2 => &mut c.top,
                    _ => &mut c.hints,
                };
                *v = !*v;
            }
            Hit::Pin => self.open_palette(crate::app::PaletteMode::PromptPin),
            Hit::SavedLibrary => self.open_settings_at(super::SEC_SAVED, None),
            Hit::SavedOption(k) => {
                let c=&mut self.behavior.prompt;
                let value=match k {0=>&mut c.saved_preview,1=>&mut c.saved_library,_=>&mut c.saved_run};
                *value=!*value;
            }
            Hit::SavedEdit(i,name) => {
                let c=&self.behavior.prompt;
                if let Some(value)=c.saved.get(i) {
                    let input=if name {c.saved_names.get(value).cloned().unwrap_or_default()}else{value.clone()};
                    let mode=if name {crate::app::PaletteMode::SavedName(i)}else{crate::app::PaletteMode::SavedEdit(i)};
                    self.open_palette(mode); self.palette=Some((mode,input));
                }
            }
            Hit::SavedUse(i,run) => self.saved_use(i,run),
            Hit::SavedCopy(i) => {if let Some(value)=self.behavior.prompt.saved.get(i){if let Ok(mut cb)=arboard::Clipboard::new(){let _=cb.set_text(value.clone());}}},
            Hit::SavedRun(run) => self.behavior.prompt.saved_run=run,
            Hit::SavedMove(i,delta) => {let v=&mut self.behavior.prompt.saved;let j=i as isize+delta as isize;if i<v.len() && j>=0 && (j as usize)<v.len(){v.swap(i,j as usize);}},
            Hit::Unpin(i) => {
                if i < self.behavior.prompt.saved.len() {
                    let value=self.behavior.prompt.saved.remove(i);
                    self.behavior.prompt.saved_names.remove(&value);
                }
            }
            Hit::Home => self.open_home(),
            Hit::Pairing(i) => self.font_pairing(i),
            Hit::EditorFont(f) => {
                self.behavior.typography.editor_family = f;
                self.behavior.typography.system[2].clear();
                self.apply_fonts();
            }
            Hit::EditorWeight(w) => {
                self.behavior.typography.editor_weight = w;
                self.apply_fonts();
            }
            Hit::TypeSize(role, delta) => {
                let c = &mut self.behavior.typography;
                match role {
                    0 => c.ui_scale += (delta as f32) * 0.05,
                    1 => c.terminal_size += delta as f32,
                    _ => c.editor_size += delta as f32,
                };
                self.apply_fonts();
            }
            Hit::TypeLine(role, delta) => {
                let c = &mut self.behavior.typography;
                if role == 1 {
                    c.terminal_line += delta as f32 * 0.05;
                } else {
                    c.editor_line += delta as f32 * 0.05;
                }
                self.apply_fonts();
            }
            Hit::Tracking(delta) => {
                self.behavior.typography.terminal_spacing += delta as f32 * 0.25;
                self.apply_fonts();
            }
            Hit::ResetType => self.font_pairing(0),
        }
    }
    pub(super) fn prompt_settings(&self) -> Vec<(String, Control)> {
        let c = &self.behavior.prompt;
        let mut rows=vec![info("The Home prompt and the command palette show suggestions from these sources. Pick a starting point, then adjust the mix. The preview shows your real suggestions; point at anything about typing to see what appears while you type."),
   ("START FROM A PRESET".into(),strip(Preset::ALL.into_iter().map(|p|(p.name(),Hit::PromptPreset(p),c.matches(p))).collect())),
   ("WHAT'S SUGGESTED".into(),Control::Section),
   ("".into(),Control::Sources(c.ordered().into_iter().map(|s|(s.source,s.home,s.search,s.count)).collect())),
   info("Before typing: what Home lists on an empty line. While typing: what's searched as you type. How many: the most from each source. Empty sources take no space."),
   ("MOST BEFORE TYPING".into(),Control::Stepper(c.home_limit.to_string(),hit(Hit::Limit(true,-1)),hit(Hit::Limit(true,1)))),
   ("MOST WHILE TYPING".into(),Control::Stepper(c.search_limit.to_string(),hit(Hit::Limit(false,-1)),hit(Hit::Limit(false,1)))),
   ("PRESSING ENTER".into(),Control::Section),
   ("WITH NO SUGGESTION PICKED, ENTER".into(),strip(Route::ALL.into_iter().map(|r|(match r { Route::Automatic=>"Opens a URL, else runs it", Route::Shell=>"Runs it in a shell", Route::Web=>"Searches the web", Route::Assistant=>"Asks your assistant" },Hit::Route(r),c.route==r)).collect())),
   info("Prefixes always win: > runs a command, ? searches the web, @claude, @codex or @ollama asks that assistant (you review the prompt before it's sent)."),
   ("SEARCH ENGINE".into(),strip(SearchEngine::ALL.into_iter().map(|e|(e.name(),Hit::Engine(e),c.engine==e)).collect())),
   info(match c.engine {
       SearchEngine::Custom if c.search_url.contains("%s") => format!("Searches go to {} (%s is where your words go).", c.search_url),
       SearchEngine::Custom => "Custom needs an address with %s where the words go, like https://example.com/search?q=%s. Until then, Google answers.".to_string(),
       e => format!("Web searches go to {}.", e.name()),
   }),
   (String::new(),buttons(vec![("Set a custom search address",Hit::EditSearchUrl)])),
   ("HOW IT LOOKS".into(),Control::Section),
   ("LAYOUT".into(),strip(vec![("Full width",Hit::Layout(0),c.wide),("Tighter rows",Hit::Layout(1),c.compact),("Line near the top",Hit::Layout(2),c.top),("Hints at the bottom",Hit::Layout(3),c.hints)])),
   info("Each is on or off. Hints at the bottom shows where Enter will go (shell, web, assistant) and lets you click one to switch."),
   ("SAVED COMMANDS".into(),Control::Section)];
        rows.extend(self.saved_options());
        rows.push(("YOUR COLLECTION".into(), buttons(vec![("Manage saved commands",Hit::SavedLibrary),("Add a command",Hit::Pin)])));
        rows.push((String::new(), buttons(vec![("Open Home", Hit::Home)])));
        rows
    }
    fn saved_options(&self) -> Vec<(String,Control)> {
        let c=&self.behavior.prompt;
        vec![("SHOW IN SUGGESTIONS".into(),strip(vec![("The command under its name",Hit::SavedOption(0),c.saved_preview),("A “Saved commands” entry",Hit::SavedOption(1),c.saved_library)])),
            ("PICKING A SAVED SHELL COMMAND".into(),strip(vec![("Puts it on the line to review",Hit::SavedRun(false),!c.saved_run),("Runs it right away",Hit::SavedRun(true),c.saved_run)])),
            info("Saved items keep their bookmark mark and action label. Insert opens a terminal with the command ready to edit; Run executes it. URLs open normally and assistant prompts still open a review.")]
    }
    pub(super) fn saved_settings(&self) -> Vec<(String,Control)> {
        let c=&self.behavior.prompt;
        let source=c.ordered().into_iter().find(|s|s.source==Source::Saved).unwrap();
        let mut rows=vec![info("Your reusable commands, links and assistant prompts. Name them, keep the command visible, and choose where they appear."),
            ("COLLECTION".into(),buttons(vec![("Add command",Hit::Pin),("Open Home",Hit::Home)])),
            (format!("VISIBILITY · {} SUGGESTIONS",source.count),strip(vec![("Before typing",Hit::Source(Source::Saved,0),source.home),("While searching",Hit::Source(Source::Saved,1),source.search),("Fewer",Hit::Count(Source::Saved,-1),false),("More",Hit::Count(Source::Saved,1),false)]))];
        let mut options=rows.split_off(2);
        options.extend(self.saved_options());
        rows.push((format!("YOUR COMMANDS · {}",c.saved.len()),Control::Caption));
        if c.saved.is_empty(){rows.push(info("Save your first command. Try > cargo test, a project folder, a website, or @codex review the changes. Saving never executes it."));}
        for (i,_) in c.saved.iter().enumerate(){
            rows.push((String::new(),Control::SavedCommand(i)));
        }
        rows.push(("PALETTE OPTIONS".into(),Control::Caption));
        rows.extend(options);
        rows
    }
    pub(super) fn assistants_settings(&self) -> Vec<(String, Control)> {
        let mut rows=vec![info("Claude, Codex and Ollama, reachable from one workspace. Start a real terminal session, choose where prompts go, and keep ongoing work in Hatch."),
   (String::new(),strip(["Connections","Work","Context","Advanced"].into_iter().enumerate().map(|(i,n)|(n,Hit::AssistantTab(i),self.assistants.tab==i)).collect()))];
        match self.assistants.tab {
            0 => {
                rows.push(("INTELLIGENCE".into(), Control::Intelligence));
                rows.push(info("One level for every launch. Claude on Auto picks its model from it and every level sets its effort; Codex gets the matching reasoning effort. Drag the ring, tap either side of it, or drag around the dial in the preview; click the nucleus for the next Claude model. Launching, ⌥← and ⌥→ turn it."));
                rows.push((
                    "CONNECTIONS".into(),
                    buttons(vec![(
                        if self.assistants.pending.is_some() {
                            "Checking…"
                        } else {
                            "Check connections"
                        },
                        Hit::Check,
                    )]),
                ));
                rows.push(info(if self.assistants.pending.is_some(){"Checking CLI versions and authentication or service availability. No model prompt is sent.".into()}else{self.assistants.checked_at.map(|at|format!("Last checked {} seconds ago. This checks account presence and service access; a model response is verified when you start work.",crate::clock::since(at).as_secs())).unwrap_or("Check connections to verify installed commands, sign-in and Ollama models. Your existing CLI accounts stay with their providers.".into())}));
                for i in 0..3 {
                    let e = &self.assistants.entries[i];
                    let config = &self.behavior.assistants.providers[i];
                    let found = assistants::resolve(assistants::BINS[i], &config.executable);
                    let status = if e.checked {
                        e.status.clone()
                    } else if found.is_some() {
                        "CLI found · connection not checked".into()
                    } else {
                        "CLI not found · install or choose its location".into()
                    };
                    rows.push((assistants::NAMES[i].to_uppercase(), Control::Caption));
                    rows.push(info(format!(
                        "{}{}",
                        status,
                        if e.version.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", e.version)
                        }
                    )));
                    rows.push((
                        format!(
                            "MODEL · {}",
                            if config.model.is_empty() {
                                match i {
                                    0 => "auto · follows intelligence",
                                    1 => "provider default",
                                    _ => "choose a model",
                                }
                            } else {
                                &config.model
                            }
                        ),
                        buttons(if i == 2 {
                            vec![
                                ("Choose model", Hit::Edit(Field::Model(2))),
                                ("Browse models", Hit::Setup(2, 3)),
                            ]
                        } else {
                            vec![("Choose model", Hit::Edit(Field::Model(i as u8)))]
                        }),
                    ));
                    rows.push((
                        String::new(),
                        strip(vec![
                            ("Start session", Hit::Draft(i as u8), false),
                            (
                                "Use by default",
                                Hit::Default(i as u8),
                                self.default_assistant() as usize == i,
                            ),
                            (
                                if i == 2 { "Start service" } else { "Sign in" },
                                Hit::Setup(i as u8, 1),
                                false,
                            ),
                            ("Install guide", Hit::Setup(i as u8, 0), false),
                        ]),
                    ));
                }
            }
            1 => {
                rows.push((
                    "START WORK".into(),
                    buttons(vec![
                        ("New task", Hit::Template(0)),
                        ("Plan a change", Hit::Template(1)),
                        ("Review changes", Hit::Template(2)),
                    ]),
                ));
                rows.push(info(format!("Starting folder: {}. Each launch shows the assistant, command and destination first. Sessions remain real terminals: type, interrupt, resume, or move them to Hatch.",self.assistant_folder())));
                rows.push((
                    "ONGOING WORK".into(),
                    buttons(vec![("Open Hatch", Hit::Work)]),
                ));
                for (i, t) in self.tabs.iter().enumerate() {
                    if matches!(t.left, Pane::Term(_)) {
                        rows.push((
                            t.title(),
                            buttons(vec![("Resume terminal", Hit::Session(i))]),
                        ));
                    }
                }
            }
            2 => {
                rows.push((
                    "ASK PANEL · CONTEXT".into(),
                    Control::Choice(
                        crate::askctx::Ctx::ALL
                            .iter()
                            .map(|&c| {
                                (
                                    c.key().to_uppercase(),
                                    super::Hit::AskCtx(c),
                                    self.behavior.ask_ctx.iter().any(|k| k == c.key()),
                                )
                            })
                            .collect(),
                    ),
                ));
                rows.push(info("These are the default context chips for questions in nus's Ask panel. You can change them before each question. Direct terminal sessions use the CLI's own project context and instructions."));
                rows.push((
                    "BROWSER TOOL PERMISSIONS".into(),
                    Control::Choice(vec![
                        (
                            "Ask first".into(),
                            super::Hit::Hands(HandsMode::Ask),
                            self.behavior.hands == HandsMode::Ask,
                        ),
                        (
                            "Allow".into(),
                            super::Hit::Hands(HandsMode::Always),
                            self.behavior.hands == HandsMode::Always,
                        ),
                        (
                            "Off".into(),
                            super::Hit::Hands(HandsMode::Never),
                            self.behavior.hands == HandsMode::Never,
                        ),
                    ]),
                ));
                rows.push(info("These permissions govern nus browser actions. They do not override Claude or Codex's own file and shell approval settings."));
                rows.push((
                    "SUBMISSIONS".into(),
                    Control::Choice(vec![(
                        "Confirm submit".into(),
                        super::Hit::HandsSubmit(!self.behavior.hands_confirm_submit),
                        self.behavior.hands_confirm_submit,
                    )]),
                ));
                rows.push((
                    "MEMORY".into(),
                    buttons(vec![("Read and edit memory", Hit::Memory)]),
                ));
                let mem = crate::askctx::read_memory();
                rows.push(info(if mem.trim().is_empty(){"No nus memory saved. Memory is attached only when its context chip is enabled.".into()}else{format!("{} saved lines. Edit individual entries in the editor; changes apply to the next question.",mem.lines().filter(|l|!l.trim().is_empty()).count())}));
            }
            _ => {
                rows.push(info("Use full executable paths for nonstandard installations. A selected assistant never silently falls back to another provider."));
                for i in 0..3 {
                    let config = &self.behavior.assistants.providers[i];
                    let path = assistants::resolve(assistants::BINS[i], &config.executable)
                        .map(|p| p.display().to_string())
                        .unwrap_or("Not found".into());
                    rows.push((assistants::NAMES[i].to_uppercase(), Control::Info(path)));
                    rows.push((
                        String::new(),
                        buttons(vec![
                            ("Executable", Hit::Edit(Field::Executable(i as u8))),
                            ("Diagnose in terminal", Hit::Setup(i as u8, 2)),
                        ]),
                    ));
                }
                rows.push(("NUS TOOLS".into(),Control::Info("Connect the nus MCP server using your assistant's MCP settings. It exposes the adjacent browser's page, console and network tools. Shell sessions keep the provider's normal approval workflow.".into())));
                for i in 0..2 {
                    rows.push((
                        format!(
                            "{} · {}",
                            assistants::NAMES[i],
                            if self.assistants.entries[i].tools {
                                "nus tools registered"
                            } else {
                                "nus tools not verified"
                            }
                        ),
                        buttons(vec![("Review tool connection", Hit::Tools(i as u8))]),
                    ));
                }
                rows.push(("STATUS IN THE SIDEBAR".into(),Control::Info("Hooks let Claude and Codex tell nus what they are doing — working, waiting on a permission, done — so the tab says so and you can answer from the sidebar. nus adds its hooks beside your own and takes only those out again; outside nus they do nothing.".into())));
                for i in 0..2u8 {
                    let on = assistants::hooks_connected(i);
                    rows.push((
                        format!("{} · {}", assistants::NAMES[i as usize], if on { "connected" } else { "not connected" }),
                        buttons(vec![if on { ("Disconnect", Hit::Hooks(i, false)) } else { ("Connect", Hit::Hooks(i, true)) }]),
                    ));
                }
                rows.push(("SAVED INSTRUCTIONS".into(),Control::Info("Claude and Codex load their own workspace instruction files. nus skills can add reusable prompts and context to the Ask panel.".into())));
                for s in self.rules.skills() {
                    rows.push(info(format!("{} · {}", s.name, s.prompt)));
                }
                rows.push((
                    String::new(),
                    Control::Buttons(vec![(
                        "Edit nus skills".into(),
                        icons::CODE,
                        super::Hit::OpenRules,
                    )]),
                ));
            }
        }
        rows
    }
    pub(super) fn fonts_settings(&self) -> Vec<(String, Control)> {
        let b = &self.behavior;
        let c = &b.typography;
        let mut rows=vec![info("Choose a pairing, then tune the interface, terminal and editor independently. Changes are live and saved automatically. Terminal and editor choices keep fixed-width columns."),
   ("PAIRINGS".into(),buttons(vec![("Classic",Hit::Pairing(0)),("Clear",Hit::Pairing(1)),("Areal",Hit::Pairing(2)),("Expressive",Hit::Pairing(3))])),
   ("LIVE TYPE SPECIMEN".into(),Control::FontProof)];
        for role in 0..3u8 {
            let name = ["INTERFACE", "TERMINAL", "EDITOR"][role as usize];
            let family = match role {
                0 => b.ui_font,
                1 => b.term_font,
                _ => c.editor_family,
            };
            let weight = match role {
                0 => b.ui_weight,
                1 => b.term_weight,
                _ => c.editor_weight,
            };
            rows.push((name.into(), Control::Caption));
            rows.push((
                "FAMILY".into(),
                Control::Strip(
                    (if role == 0 {
                        Family::ALL.to_vec()
                    } else {
                        Family::MONO.to_vec()
                    })
                    .into_iter()
                    .map(|f| {
                        (
                            f.name().into(),
                            match role {
                                0 => super::Hit::UiFont(f),
                                1 => super::Hit::TermFont(f),
                                _ => hit(Hit::EditorFont(f)),
                            },
                            family == f && c.system[role as usize].is_empty(),
                        )
                    })
                    .collect(),
                ),
            ));
            rows.push((
                format!(
                    "INSTALLED · {}",
                    if c.system[role as usize].is_empty() {
                        "bundled font"
                    } else {
                        &c.system[role as usize]
                    }
                ),
                buttons(vec![(
                    "Choose installed font",
                    Hit::Edit(Field::Font(role)),
                )]),
            ));
            rows.push((
                "WEIGHT".into(),
                Control::Strip(
                    Weight::ALL
                        .into_iter()
                        .map(|w| {
                            (
                                w.name().into(),
                                match role {
                                    0 => super::Hit::UiWeight(w),
                                    1 => super::Hit::TermWeight(w),
                                    _ => hit(Hit::EditorWeight(w)),
                                },
                                w == weight,
                            )
                        })
                        .collect(),
                ),
            ));
            rows.push((
                if role == 0 {
                    format!("SIZE · {}%", (c.ui_scale * 100.0).round())
                } else {
                    format!(
                        "SIZE · {} PT",
                        if role == 1 {
                            c.terminal_size
                        } else {
                            c.editor_size
                        }
                    )
                },
                buttons(vec![
                    ("−", Hit::TypeSize(role, -1)),
                    ("+", Hit::TypeSize(role, 1)),
                ]),
            ));
            if role > 0 {
                rows.push((
                    format!(
                        "LINE SPACING · {:.2}×",
                        if role == 1 {
                            c.terminal_line
                        } else {
                            c.editor_line
                        }
                    ),
                    buttons(vec![
                        ("−", Hit::TypeLine(role, -1)),
                        ("+", Hit::TypeLine(role, 1)),
                    ]),
                ));
            }
            if role == 1 {
                rows.push((
                    format!("COLUMN SPACING · {:.2} PX", c.terminal_spacing),
                    buttons(vec![("−", Hit::Tracking(-1)), ("+", Hit::Tracking(1))]),
                ));
            }
        }
        rows.push(info("Bundled weights are real font files. Installed fonts use the closest available weight; install additional weights through your operating system. Missing characters use system fallback fonts."));
        rows.push((
            String::new(),
            buttons(vec![("Reset typography", Hit::ResetType)]),
        ));
        rows
    }
    pub(super) fn draw_type_proof(&mut self, scene: &mut Scene, r: Rect, prompt: bool) {
        scene.rect(r, self.theme.paper);
        scene.outline(r, self.px(1.0), self.theme.dim);
        let x = r.x + self.px(16.0);
        let width = r.w - self.px(32.0);
        let mut y = r.y + self.px(26.0);
        if prompt {
            let st = self.ui();
            self.fonts.draw(scene, st, x, y, ">  Your prompt");
            y += self.px(20.0);
            scene.hline(x, y, width, self.px(1.0), self.theme.dim);
            y += self.px(24.0);
            let rows = self.prompt_rows("");
            if rows.is_empty() {
                self.fonts
                    .draw(scene, st, x, y, "Only the input. Type to search.");
            }
            for row in rows.into_iter().take(4) {
                let text = self.fit(st, &row.text, width);
                self.fonts.draw(scene, st, x, y, &text);
                y += self.px(28.0);
            }
            return;
        }
        let examples = [
            (
                self.f.ui,
                self.ui().px,
                "Interface · nus / Workspace / Settings",
                self.theme.ink,
            ),
            (
                self.f.term,
                self.terminal_px(),
                "$ cargo test   24 passed   0 failed",
                self.surface.signal,
            ),
            (
                self.f.editor,
                self.behavior.typography.editor_size * self.scale * 96.0 / 72.0,
                "fn main() { println!(\"hello, nus\"); }",
                self.theme.ink,
            ),
        ];
        for (font, px, text, color) in examples {
            let st = Style {
                font,
                px,
                color,
                tracking: 0.0,
            };
            let text = self.fit(st, text, width);
            self.fonts.draw(scene, st, x, y, &text);
            y += self.px(46.0);
        }
        let st = Style {
            font: self.f.term,
            px: self.terminal_px(),
            color: self.theme.dim,
            tracking: 0.0,
        };
        let text = self.fit(st, "0O  1lI  {} [] ()  => !=  ~/nus  → ✓", width);
        self.fonts.draw(scene, st, x, y, &text);
    }
}
