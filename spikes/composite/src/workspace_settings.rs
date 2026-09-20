//! Deep controls for assistants, typography and the shared prompt, in nus's native settings.
use super::*;
use crate::{
    assistants::{self, Field},
    fonts::{Family, Weight},
    prompt::{Preset, Route, Source},
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    AssistantTab(usize),
    Tools(u8),
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
    Layout(u8),
    Pin,
    Unpin(usize),
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
        Hit::Layout(k) => [
            "Wide prompt",
            "Compact suggestions",
            "Prompt near top",
            "Show route keys at the foot of Home",
        ][k as usize]
            .into(),
        Hit::Pin => "Add a saved prompt shortcut".into(),
        Hit::Unpin(i) => format!("Remove saved shortcut {}", i + 1),
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
                let saved = std::mem::take(&mut self.behavior.prompt.saved);
                self.behavior.prompt = crate::prompt::Config::preset(p);
                self.behavior.prompt.saved = saved;
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
            Hit::Unpin(i) => {
                if i < self.behavior.prompt.saved.len() {
                    self.behavior.prompt.saved.remove(i);
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
        let mut rows=vec![info("Your Home prompt and command palette share these sources and routing rules. Choose a starting point, then shape the mix."),
   ("STARTING POINT".into(),strip(Preset::ALL.into_iter().map(|p|(p.name(),Hit::PromptPreset(p),c.matches(p))).collect())),
   ("HOME PREVIEW".into(),Control::PromptProof),
   ("ENTER WITH NO SUGGESTION SELECTED".into(),strip(Route::ALL.into_iter().map(|r|(r.name(),Hit::Route(r),c.route==r)).collect())),
   info("Explicit routes always work: > command, ? web search, @claude prompt, @codex prompt, @ollama prompt. Assistant prompts open a review before sending. Home always opens this surface."),
   ("LAYOUT".into(),strip(vec![("Wide",Hit::Layout(0),c.wide),("Compact",Hit::Layout(1),c.compact),("Near top",Hit::Layout(2),c.top),("Route keys",Hit::Layout(3),c.hints)])),
   info("Route keys are the marks at the foot of Home: a shell, a page, an assistant, the rows. The one Enter would take is lit, the pointer names each, and a click puts its prefix on the line. Turn them off for a bare line."),
   (format!("HOME · {} RESULTS",c.home_limit),buttons(vec![("Fewer",Hit::Limit(true,-1)),("More",Hit::Limit(true,1))])),
   (format!("SEARCH · {} RESULTS",c.search_limit),buttons(vec![("Fewer",Hit::Limit(false,-1)),("More",Hit::Limit(false,1))])),
   ("SOURCES · IN DISPLAY ORDER".into(),Control::Caption),info("Home controls the suggestions before you type. Search controls matching suggestions as you type. Empty sources take no space. Reorder with Up and Down; the limit applies per source.")];
        for s in c.ordered() {
            rows.push((
                format!("{} · LIMIT {}", s.source.name().to_uppercase(), s.count),
                strip(vec![
                    ("Home", Hit::Source(s.source, 0), s.home),
                    ("Search", Hit::Source(s.source, 1), s.search),
                    ("Up", Hit::Move(s.source, -1), false),
                    ("Down", Hit::Move(s.source, 1), false),
                    ("−", Hit::Count(s.source, -1), false),
                    ("+", Hit::Count(s.source, 1), false),
                ]),
            ));
        }
        rows.push((
            "SAVED SHORTCUTS".into(),
            buttons(vec![("Add shortcut", Hit::Pin)]),
        ));
        rows.push(info("Save a route such as > cargo test, ? release notes, @codex review the changes, or a website address. Nothing runs when a shortcut is saved."));
        for (i, s) in c.saved.iter().enumerate() {
            rows.push(info(s));
            rows.push((String::new(), buttons(vec![("Remove", Hit::Unpin(i))])));
        }
        rows.push((String::new(), buttons(vec![("Open Home", Hit::Home)])));
        rows
    }
    pub(super) fn assistants_settings(&self) -> Vec<(String, Control)> {
        let mut rows=vec![info("Claude, Codex and Ollama, reachable from one workspace. Start a real terminal session, choose where prompts go, and keep ongoing work in Hatch."),
   (String::new(),strip(["Connections","Work","Context","Advanced"].into_iter().enumerate().map(|(i,n)|(n,Hit::AssistantTab(i),self.assistants.tab==i)).collect()))];
        match self.assistants.tab {
            0 => {
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
                                if i == 2 {
                                    "choose a model"
                                } else {
                                    "provider default"
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
