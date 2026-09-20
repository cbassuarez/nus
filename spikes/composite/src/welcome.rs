//! The welcome page: a full tab, not a PTY — the whole of nus laid out
//! as a ruled document you can act from. Every row has a chord you can
//! read and, where it makes sense, a TRY button that does the thing:
//! opens the palette, runs a demo in a shell, lights hints, opens the
//! studio. The first section is a checklist that reads the real state
//! (shell integration per profile, default browser, login item, the
//! look). It opens on first launch, from F1, the palette, and settings.

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use std::sync::Arc;

use crate::app::{Caps, fade, hover_key, App, Pane, PaletteMode};
use nus_render::theme::metric as m;

/// What a welcome-page control does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Palette(PaletteMode),
    NewShell,
    NewTab,
    Prompt,
    Split,
    Studio,
    Settings(usize),
    Reader,
    Hints,
    Find,
    Demo(&'static str),
    Rules,
    DefaultBrowser,
    LoginItem,
    NewWindow,
    Rename,
    RenameTab,
    Close,
    /// Terminal first, or browser first.
    Lead(crate::settings::Lead),
    /// The profile card.
    Me,
    Scroll(f32),
    /// GET or REMOVE an optional tool by id.
    Bundle(String),
}

/// One row: chord (may be empty), title, what it does, an optional action.
struct Row {
    chord: String,
    title: String,
    what: String,
    act: Option<(&'static str, Act)>,
}

fn row(chord: impl Into<String>, title: impl Into<String>, what: impl Into<String>, act: Option<(&'static str, Act)>) -> Row {
    Row { chord: chord.into(), title: title.into(), what: what.into(), act }
}

/// The platform's chord prefix in words.
fn k(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("⌘⇧{key}") } else { format!("CTRL+SHIFT+{key}") }
}

impl App {
    /// Open the welcome page as its own tab (or go to it).
    pub(crate) fn open_welcome(&mut self) {
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Hints(_))) {
            return self.activate(i);
        }
        let tab = self.make_tab(Pane::Hints(crate::app::HintsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), scroll: 0.0 }), None);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1);
        self.layout();
    }

    /// Shared desktop/notice mark, made once per look.
    pub(crate) fn desktop_icon(&mut self) -> Option<Arc<wgpu::BindGroup>> {
        let key = (self.theme.mode, self.surface.signal);
        if let Some((k, b)) = &self.welcome_icon_tex {
            if *k == key {
                return Some(b.clone());
            }
        }
        let size = 96;
        let rgba = nus_render::dock_icon::render(size, self.surface.signal, nus_render::dock_icon::Face::Newsreader);
        let bgra: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("desktop icon"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bgra,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
        let b = (self.bind_texture)(&tex);
        self.welcome_icon_tex = Some((key, b.clone()));
        Some(b)
    }

    /// The sections, with live state folded in.
    fn welcome_sections(&self) -> Vec<(&'static str, &'static str, Vec<Row>)> {
        use crate::settings::{LOOK_PRESETS, SEC_LOOK, SEC_SOUND, SEC_STARTUP, SEC_TERMINAL, SEC_BROWSER, RULES};
        let ink = self.theme.mode == nus_render::Mode::Ink;
        let default_browser = crate::little::registered();
        let login = crate::little::login_item_registered();
        let lead = self.behavior.lead;
        let me_row = match &self.me {
            Some(me) => row("", "Your profile", format!("{} · {} with nus · a folder on this machine, no account, no telemetry", me.name, me.day_word()), Some(("OPEN", Act::Me))),
            None => row("", "Your profile", "not set up yet · a name, a face, this device's name · it lives in a folder here; no account, no server, nothing counted", Some(("SET UP", Act::Me))),
        };
        let mut start = vec![
            me_row,
            match lead {
                crate::settings::Lead::Terminal => row("", "Terminal first", "An empty prompt starts a shell. New tabs use the destination chosen in Startup.", Some(("BROWSER FIRST", Act::Lead(crate::settings::Lead::Browser)))),
                crate::settings::Lead::Browser => row("", "Browser first", "An empty prompt opens the atlas. New tabs use the destination chosen in Startup.", Some(("TERMINAL FIRST", Act::Lead(crate::settings::Lead::Terminal)))),
            },
            row("", "The look", format!("{} · {} · {} carapace · the chip in the footer hot-swaps on hover", self.preset_name.to_lowercase(), if ink { "ink" } else { "paper" }, self.surface.shell.name()), Some(("OPEN THE STUDIO", Act::Studio))),
            row("", "Default browser", if default_browser { "nus is your default browser · links from other apps open in the little window".to_string() } else { "not yet · links from other apps would open here in a little window".to_string() }, if default_browser { None } else { Some(("MAKE DEFAULT", Act::DefaultBrowser)) }),
            row("", "Start on login", if login { "on · nus starts with the system".to_string() } else { "off".to_string() }, Some((if login { "TURN OFF" } else { "TURN ON" }, Act::LoginItem))),
        ];
        for p in self.profiles.iter().take(4) {
            let kind = crate::shell::kind_of(&p.program);
            let on = self.behavior.shell_integration && !matches!(kind, crate::shell::Kind::Other);
            start.push(row("", if on { "Shell integration" } else { "Shell integration · off" }, format!("{} · {}", p.name, crate::shell::describe(kind)), Some(("TERMINAL SETTINGS", Act::Settings(SEC_TERMINAL)))));
        }
        let shell = vec![
            row("ENTER", "A URL at the prompt", "type a URL at a fresh prompt and Enter opens it beside; Ctrl+Enter runs it in the shell", Some(("TRY", Act::Demo("https://docs.rs/wgpu")))),
            row(k("↑ / ↓"), "Jump between prompts", "each command is a block: a hairline where it starts, an × code when it failed, DONE when a long one finishes", Some(("TRY", Act::Demo("git log --oneline -3")))),
            row(k("C"), "Copy the last output", "or the selection when there is one · right-click copies a selection, pastes otherwise", None),
            row(k("V"), "Paste, carefully", "bracketed when the program asks; many lines or control characters ask first", None),
            row(k("O"), "Hints", "labels over every URL, path and hash on screen · type one: URLs open, the rest copies", Some(("TRY", Act::Hints))),
            row(k("F"), "Find in scrollback", "a band; Enter next, Shift+Enter back; the scrollbar's ticks are the prompts", Some(("TRY", Act::Find))),
            row("→ / END", "Predictions", "the history entry that continues your line ghosts after the caret; Right or End accepts · tokens colour as you type", Some(("TRY", Act::Demo("git st")))),
            row("CLICK", "Select and move", "double-click a word, triple a line, Ctrl+triple a command's output · at a prompt a click moves the caret", None),
            row("", "Hover a block", "a gutter rule and two chips: copy its output, run it again", None),
            row("", "Images", "Kitty and iTerm2 image protocols draw right in the shell (icat, timg, chafa)", None),
        ];
        let pages = vec![
            row(k("T"), "New tab", "opens the start page selected in Startup; hold or right-click NEW TAB to choose a shell or page", Some(("TRY", Act::NewTab))),
            row(k("K"), "The palette", "tabs, ports, history, commands to run again, settings, ask an assistant", Some(("TRY", Act::Palette(PaletteMode::Go)))),
            row(k("L"), "Address", "history ranked by visits and recency; a search when it isn't a URL", Some(("TRY", Act::Palette(PaletteMode::Url)))),
            row(k("D"), "Split", "a page beside the shell, or two of anything", Some(("TRY", Act::Split))),
            row(k("R"), "Reader", "the article, set in Newsreader on our paper; the page keeps living underneath", Some(("TRY", Act::Reader))),
            row(k("F"), "Find in page", "the same band, on a page", None),
            row("", "Downloads", format!("straight to {} · the list rises from the footer; click one to reveal it", crate::browser::downloads_dir().to_string_lossy().replace(char::from(92), "/")), None),
            row("", "Permissions", "a page asking for the camera, location or notifications gets a band, not a dialog", None),
            row("", "Content blocking", format!("{} ad, tracker and analytics hosts refused · add yours to profile/blocklist.txt", crate::browser::blocklist_len()), Some(("BROWSER SETTINGS", Act::Settings(SEC_BROWSER)))),
            row("", "Sleep and archive", format!("idle pages sleep after {} minutes and archive after {} hours; pinned tabs and shells never", self.behavior.sleep_after_min, self.behavior.archive_after_h), None),
            row("ESC", "Little nus", "links from other apps open in a small floating window; Ctrl+Shift+O keeps one as a tab", None),
        ];
        let windows = vec![
            row("SHIFT+F2", "Name this window", format!("“{}” · auto-named from the git root or the host; the name is in the title bar and Alt-Tab", self.window_name()), Some(("RENAME", Act::Rename))),
            row("CTRL+N", "New window", "windows own their tabs; the rail or the header lists them", Some(("OPEN ONE", Act::NewWindow))),
            row("CTRL+1–9", "Tabs by number", "Ctrl+` goes back to the last one; Ctrl+PgUp / PgDn walk them", None),
            row("", "Containers", "named cookie jars: “container” in the palette lists them, switches this window's (its square wears the colour; new windows inherit it), reopens a page in one, or makes a new one — sign-ins stay apart", None),
            row("CTRL+SHIFT+F11", "Focus", "the page (or shell) alone in the window: no strip, no sidebar, no rows · the same chord leaves", None),
            row(k("B"), "Compact", "a 48px column of icons; the top strip hides until the pointer reaches the top; hover a row for its name", None),
            row(k("S"), "The sidebar", "pin it, or let it slide in from the edge · BAR or RAIL header under settings", None),
            row("", "Site panel", "the gear at the end of a page's URL row: zoom (remembered), autoplay, JavaScript, cookies, boosts, blocking and the permissions this site was given · per host", None),
            row("", "Folders", "under the tabs: GITHUB (open pull requests, via gh), PORTS (what's listening), lists from rules.luau, and your own — SAVE TO FOLDER in a page's menu; a saved page never archives", None),
            row(k("?"), "Ask", "a small panel beside the shell: one line in, a few commands out — INSERT at the prompt, RUN, or COPY; the shell, folder and last output go along · claude, codex, copilot, ollama or ANTHROPIC_API_KEY", None),
            row(k("K"), "Chains", "named lists of palette commands in rules.luau — open pages, run commands, tile — and every settings row, by name", None),
            row("ALT+CLICK", "Peek", "a link floats over the page instead of leaving it · Esc closes, Ctrl+Enter keeps it in the stack", None),
            row("", "Split panes", "near a pane's corner the controls bloom: move (drag it onto a sidebar row, or NEW TAB), swap, solo, to its own tab, close · the rule between the panes lights as you near it and drags", None),
            row(k("D"), "Tiles", "Ctrl+click two to four rows, then tile them: side by side, an L, or a grid · drag the rules · Ctrl+Alt+arrows walk the tiles, with Shift they swap", None),
            row("F2", "Name a tab", "right-click a tab for its menu: rename, an emoji or short string as its icon, a colour, pin, close", Some(("RENAME THIS TAB", Act::RenameTab))),
            row("", "Stacks", "pages a page opens sit under it, as deep as they go; fold with the caret, Ctrl+Shift+- folds all; drag a tab onto another to nest it", None),
            row("", "Rules", "profile/rules.luau colours new tabs and windows, boosts pages, picks sounds", Some(("OPEN RULES", Act::Rules))),
        ];
        let look = vec![
            row("", "The studio", "a live proof of the window, presets as cards, tokens as tiles, a real picker", Some(("OPEN", Act::Settings(SEC_LOOK)))),
            row("", "Sound", "seventeen cues on fourteen events, synthesised in the app", Some(("OPEN", Act::Settings(SEC_SOUND)))),
            row("", "Startup", "how the window comes up, the splash, what follows, the atlas", Some(("OPEN", Act::Settings(SEC_STARTUP)))),
            row("", "Cursor", "shape, blink, colour, glide or comet, the pointer over the chrome", Some(("OPEN", Act::Settings(SEC_LOOK)))),
            row("F11", "Fullscreen", "the sidebar follows the rule you set for it", None),
        ];
        let _ = (LOOK_PRESETS, RULES);
        // Optional tools: what the machine could have, fetched on request.
        use crate::bundles::State;
        let tools: Vec<Row> = crate::bundles::list()
            .into_iter()
            .map(|b| {
                let state = self.bundle_state(&b);
                let (status, button): (String, Option<&'static str>) = match &state {
                    State::Installed => ("installed".into(), Some("REMOVE")),
                    State::Fetching => ("fetching…".into(), None),
                    State::Failed(e) => (format!("failed · {e}"), Some("TRY AGAIN")),
                    State::Soon => ("coming with the first release".into(), None),
                    State::NoPlatform => ("not for this platform yet".into(), None),
                    State::Absent => (format!("about {} MB · profile/{}", b.size_mb, if b.into.is_empty() { "—".to_string() } else { b.into.clone() }), Some("GET")),
                };
                let what = format!("{} · {}", b.about, status);
                row(b.kind.caps(), b.name.clone(), what, button.map(|w| (w, Act::Bundle(b.id.clone()))))
            })
            .collect();
        vec![
            ("START HERE", "What's on, what isn't, and where to change it.", start),
            ("OPTIONAL TOOLS", "Do you want these? Nothing arrives unless you say; each is a folder under profile/ you can delete.", tools),
            ("THE SHELL", "Prompt marks make the shell legible: blocks, jumps, hints, predictions.", shell),
            ("PAGES", "A browser under the same carapace, with the same rules.", pages),
            ("WINDOWS AND TABS", "A window owns its tabs. Everyone knows what a window is.", windows),
            ("THE LOOK", "Broadsheet's tokens are fixed; everything around them is yours.", look),
        ]
    }

    /// Draw the page into `r`, scrolled by the pane's offset.
    pub(crate) fn draw_welcome(&mut self, scene: &mut Scene, r: Rect, scroll: f32) -> f32 {
        let t = self.theme.clone();
        let ink = t.ink;
        let signal = self.surface.signal;
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let dim = Style { color: fade(ink, 0.76), ..ui };
        self.welcome_hits.clear();
        self.welcome_shapes.clear();
        let mut pieces:Vec<[f32;4]>=Vec::new();
        let sc=self.scale;
        let piece=|i:f32,cx:f32,cy:f32,size:f32| [i,(cx-r.x)/sc,(cy-r.y)/sc,size];
        scene.layer(Some(r));
        let pad = self.px(36.0).min(r.w * 0.06);
        let width = (r.w - pad * 2.0).min(self.px(900.0));
        let x = r.x + pad;
        let mut y = r.y + self.px(30.0) - scroll;
        let narrow = width < self.px(540.0);
        let title = Style { font: self.f.wordmark, px: self.px(if narrow { 38.0 } else { 52.0 }), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, title, x, y + title.px, "nus");
        let close_text="GET STARTED";
        let close_w=(self.fonts.measure(label,close_text)+self.px(20.0)).min(width*0.6);
        let close = Rect::new(x + width - close_w, y, close_w, self.px(28.0));
        scene.outline(close, self.px(1.0), ink);
        self.fonts.draw(scene, label, close.x+self.px(10.0), close.y+self.px(19.0), close_text);
        self.welcome_hits.push((close, Act::Close));
        y += title.px + self.px(20.0);
        let headline = Style { font: self.f.serif, px: self.px(if narrow {24.0} else {32.0}), color: ink, tracking: 0.0 };
        for line in crate::reader::wrap(&self.fonts,headline,"Make yourself at home.",if narrow {width} else {width*0.69}) {
            self.fonts.draw(scene,headline,x,y+headline.px,&line);y+=headline.px+self.px(3.0);
        }
        y += self.px(18.0);
        for text in crate::reader::wrap(&self.fonts, ui, "Your profile, your look, your workspace. Choose where to begin.", if narrow {width} else {width * 0.66}) {
            self.fonts.draw(scene, dim, x, y, &text); y += self.px(21.0);
        }
        // Place the existing vector pieces in page whitespace, not one box.
        if !narrow {
            pieces.push(piece(3.0,x+width*0.48,r.y+self.px(54.0)-scroll,0.32));
            pieces.push(piece(5.0,x+width*0.8,r.y+self.px(115.0)-scroll,0.38));
            pieces.push(piece(4.0,x+width*0.91,r.y+self.px(210.0)-scroll,0.30));
            pieces.push(piece(1.0,x+width*0.75,r.y+self.px(187.0)-scroll,0.24));
        } else {
            y+=self.px(22.0);
            pieces.push(piece(3.0,x+width*0.3,y,0.25));
            pieces.push(piece(5.0,x+width*0.78,y,0.30));
            y+=self.px(36.0);
        }
        y += self.px(24.0);
        self.fonts.draw(scene,strong,x,y,"YOUR STARTING POINTS");
        y += self.px(18.0);
        let cards = [
            ("YOUR PROFILE", "Name, picture and device.", Act::Me),
            ("PROMPT PALETTE", "Open a page or run a command.", Act::Prompt),
            ("THEMES", "Try the themes already in nus.", Act::Studio),
            ("STARTUP", "Choose new tabs and windows.", Act::Settings(2)),
            ("SETTINGS", "Make your workspace work for you.", Act::Settings(3)),
            ("SHORTCUTS", "Learn the everyday keys.", Act::Settings(11)),
        ];
        let columns = if width >= self.px(560.0) {3} else if width >= self.px(350.0) {2} else {1};
        let gap = self.px(16.0);
        let cw=(width-gap*(columns-1) as f32)/columns as f32;
        let ch=self.px(152.0);
        let card_count = cards.len();
        for (i,(name,note,act)) in cards.into_iter().enumerate() {
            let card=Rect::new(x+(i%columns) as f32*(cw+gap),y+(i/columns) as f32*(ch+gap),cw,ch);
            if card.bottom()<r.y || card.y>r.bottom() {continue;}
            let hot=card.contains(self.mouse.0,self.mouse.1);
            let duration=self.motion.dur(130.0);
            let lift_px=self.px(2.0);
            let hover=self.hovers.entry(hover_key("onboarding-card",i)).or_insert_with(|| crate::app::Hover {alpha:crate::anim::Anim::at(0.0),pulse:crate::anim::Anim::at(1.0),hot:false,since:std::time::Instant::now()});
            if hover.hot!=hot {hover.hot=hot;hover.alpha.go(if hot {1.0} else {0.0},duration);}
            let lift=lift_px*hover.alpha.value();
            if hover.alpha.active() {self.dirty=true;}
            let b=Rect::new(card.x-lift,card.y-lift,card.w,card.h);
            scene.rect(Rect::new(b.x+self.px(4.0),b.y+self.px(4.0),b.w,b.h),if hot {signal} else {ink});
            scene.rect(b,t.paper);scene.outline(b,self.px(2.0),ink);
            let picture=Rect::new(b.x+self.px(14.0),b.y+self.px(14.0),b.w-self.px(28.0),self.px(52.0));
            // The profile face, prompt thumbnail, preset card and Phosphor
            // icons are the same objects used by their destination pages.
            match i {
                0 => {
                    let face=self.me.as_ref().map(|me|me.face.clone()).unwrap_or(crate::me::Face::Initial);
                    let name=self.me_name();
                    let d=picture.h;
                    self.draw_face(scene,Rect::new(picture.x+(picture.w-d)/2.0,picture.y,d,d),&face,&name);
                }
                1 => self.draw_pic(scene,picture,crate::settings::Pic::StartPrompt),
                2 => {
                    if let Some(theme)=crate::themes::all().into_iter().find(|theme|theme.name==self.preset_name).or_else(||crate::themes::all().into_iter().next()) {
                        let ramp=theme.surface.ramp(ink);
                        let outer=scene.clip();scene.layer(Some(picture.intersect(&r)));
                        self.draw_card(scene,picture,"",&ramp,theme.surface.signal,theme.surface.angle,false,Some((theme.paper.paper,theme.paper.ink,theme.ink.paper,theme.ink.ink)));
                        scene.layer(outer);
                    }
                }
                3 => self.draw_pic(scene,picture,crate::settings::Pic::NewPrompt),
                _ => {
                    let icon=if i==4 {nus_render::text::icons::SLIDERS} else {nus_render::text::icons::KEYBOARD};
                    self.fonts.draw_icon(scene,icon,self.px(32.0),picture.x+(picture.w-self.px(32.0))/2.0,picture.y+self.px(10.0),ink);
                }
            }
            let name=self.fit(strong,name,b.w-self.px(28.0));
            self.fonts.draw(scene,strong,b.x+self.px(14.0),b.y+self.px(91.0),&name);
            for (j,line) in crate::reader::wrap(&self.fonts,ui,note,b.w-self.px(28.0)).into_iter().take(2).enumerate() {self.fonts.draw(scene,dim,b.x+self.px(14.0),b.y+self.px(115.0)+j as f32*self.px(19.0),&line);}
            self.welcome_hits.push((card,act));
        }
        y += card_count.div_ceil(columns) as f32*(ch+gap)+self.px(22.0);
        pieces.push(piece(6.0,x+width*0.28,y+self.px(8.0),0.34));
        pieces.push(piece(9.0,x+width*0.85,y+self.px(8.0),0.44));
        y+=self.px(48.0);
        let intro="Explore the guide below. Each button opens the feature it describes. You can return here any time with F1.";
        for line in crate::reader::wrap(&self.fonts,ui,intro,width) {self.fonts.draw(scene,dim,x,y,&line);y+=self.px(21.0);}
        y+=self.px(22.0);
        for (section,(name,lede,rows)) in self.welcome_sections().into_iter().enumerate() {
            scene.hline(x,y,width,self.px(m::STRUCTURE),ink);y+=self.px(36.0);
            let kind=[2.0,7.0,8.0,5.0,4.0,9.0][section%6];
            pieces.push(piece(kind,x+width-self.px(34.0),y-self.px(10.0),if kind==4.0 {0.20} else {0.25}));
            self.fonts.draw(scene,strong,x,y,name);y+=self.px(24.0);
            for line in crate::reader::wrap(&self.fonts,ui,lede,width) {self.fonts.draw(scene,dim,x,y,&line);y+=self.px(20.0);}
            y+=self.px(12.0);
            for row in rows {
                let title=self.fit(strong,&row.title.caps(),width);
                self.fonts.draw(scene,strong,x,y,&title);y+=self.px(22.0);
                if !row.chord.is_empty() { self.fonts.draw(scene,Style{color:signal,..label},x,y,&row.chord); y+=self.px(21.0); }
                for line in crate::reader::wrap(&self.fonts,ui,&row.what,width) {self.fonts.draw(scene,dim,x,y,&line);y+=self.px(20.0);}
                if let Some((text,act))=row.act {
                    let b=Rect::new(x,y+self.px(4.0),(self.fonts.measure(strong,text)+self.px(24.0)).min(width),self.px(28.0));
                    let hot=b.contains(self.mouse.0,self.mouse.1);
                    scene.rect(b,if hot {ink} else {t.paper});scene.outline(b,self.px(1.0),ink);
                    self.fonts.draw(scene,Style{color:if hot {t.paper} else {ink},..strong},b.x+self.px(12.0),b.y+self.px(19.0),text);
                    self.welcome_hits.push((b,act));y+=self.px(42.0);
                }
                y+=self.px(18.0);scene.hline(x,y,width,self.px(1.0),t.tint);y+=self.px(28.0);
            }
        }
        y+=self.px(24.0);
        let reduced=self.motion.reduced();
        let modal=self.me_card.open;
        let pointer=if !reduced && r.contains(self.mouse.0,self.mouse.1) {Some(((self.mouse.0-r.x)/sc,(self.mouse.1-r.y)/sc))} else {None};
        for p in &pieces {
            let (w,h)=match p[0] as usize {1=>(320.0,320.0),2=>(210.0,210.0),3=>(440.0,110.0),4=>(300.0,220.0),5=>(170.0,85.0),6=>(380.0,44.0),7=>(78.0,78.0),8=>(250.0,26.0),_=>(84.0,84.0)};
            let hit=Rect::new(r.x+(p[1]-w*p[3]*0.5)*sc,r.y+(p[2]-h*p[3]*0.5)*sc,w*p[3]*sc,h*p[3]*sc).intersect(&r);
            if hit.w>0.0 && hit.h>0.0 {self.welcome_shapes.push(hit);}
        }
        let taps=std::mem::take(&mut self.welcome_taps).into_iter().map(|(x,y)|((x-r.x)/sc,(y-r.y)/sc)).collect();
        let env=crate::art::Env {w:r.w/sc,h:r.h/sc,pieces,pointer,taps,face:if t.mode==nus_render::Mode::Ink {"ink".into()} else {"paper".into()},paper:t.paper,ink,signal,dim:t.dim,tint:t.tint,scale:sc,..Default::default()};
        let (started,art)=self.welcome_art.get_or_insert_with(|| (std::time::Instant::now(),crate::art::Art::open("memphis")));
        if modal {*started=std::time::Instant::now();}
        let elapsed=started.elapsed().as_secs_f32();
        let cmds=art.frame_at(env,if reduced || modal {3.0} else {elapsed});
        self.draw_art_cmds_scaled(scene,r,cmds,sc);
        if !reduced && !modal && (elapsed<3.0 || self.welcome_anim_until.is_some_and(|until|until>std::time::Instant::now())) {self.dirty=true;}
        self.welcome_hits.iter_mut().for_each(|(hit,_)| *hit=hit.intersect(&r));
        self.welcome_hits.retain(|(hit,_)|hit.w>0.0 && hit.h>0.0);
        scene.layer(None);
        y+scroll-r.y
    }

    /// A click on the page. Returns true when consumed.
    pub(crate) fn welcome_click(&mut self, x: f32, y: f32) -> bool {
        // The welcome tab can remain open behind any other tab. Its last
        // painted hit boxes must never intercept that tab's pointer events.
        let visible = self.tabs.get(self.active).is_some_and(|tab| {
            std::iter::once(&tab.left).chain(tab.right.as_ref()).any(|pane| {
                matches!(pane, crate::app::Pane::Hints(p) if p.rect.contains(x, y))
            })
        });
        if !visible { return false; }
        let Some((_, act)) = self.welcome_hits.iter().find(|(r, _)| r.contains(x, y)).cloned() else {
            if !self.motion.reduced() && self.welcome_shapes.iter().any(|r|r.contains(x,y)) {
                self.welcome_taps.push((x,y));self.welcome_anim_until=Some(std::time::Instant::now()+std::time::Duration::from_millis(700));self.dirty=true;return true;
            }
            return false;
        };
        self.welcome_act(act);
        true
    }

    pub(crate) fn welcome_act(&mut self, act: Act) {
        use crate::settings::Hit;
        self.play_event("control.press");
        match act {
            Act::Palette(m) => self.open_palette(m),
            Act::NewTab => self.open_start_page(false),
            Act::Prompt => self.open_home(),
            Act::NewShell => {
                let p = self.behavior.default_profile;
                self.new_tab(p);
            }
            Act::Split => {
                // A shell with a page beside it, so the split shows.
                let p = self.behavior.default_profile;
                self.new_tab(p);
                if let Some(w) = self.new_web_pane("https://docs.rs/wgpu/latest/wgpu/") {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.right = Some(Pane::Web(w));
                    }
                    self.layout();
                }
            }
            Act::Studio => {
                self.open_settings();
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = crate::settings::SEC_LOOK;
                }
                self.look_tab = crate::settings::LOOK_PRESETS;
            }
            Act::Settings(k) => {
                self.open_settings();
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = k;
                }
            }
            Act::Reader => {
                self.open_url("https://en.wikipedia.org/wiki/Monterey_Bay_Aquarium", true);
                self.welcome_pending = Some((Act::Reader, std::time::Instant::now()));
            }
            Act::Hints => {
                self.run_in_shell("echo https://docs.rs/wgpu ./src/main.rs 9058fca1");
                self.welcome_pending = Some((Act::Hints, std::time::Instant::now()));
            }
            Act::Find => {
                self.run_in_shell("git log --oneline -8");
                self.welcome_pending = Some((Act::Find, std::time::Instant::now()));
            }
            Act::Demo(cmd) => self.run_in_shell(cmd),
            Act::Rules => self.apply_setting(Hit::OpenRules, 0.0),
            Act::DefaultBrowser => self.apply_setting(Hit::MakeDefault, 0.0),
            Act::LoginItem => {
                let on = !crate::little::login_item_registered();
                self.apply_setting(Hit::LoginItem(on), 0.0);
            }
            Act::NewWindow => self.new_window_request = true,
            Act::Me => self.open_me_card(),
            Act::Lead(l) => {
                self.apply_setting(Hit::Lead(l), 0.0);
                self.notice(match l {
                    crate::settings::Lead::Terminal => "An empty prompt now starts a shell.",
                    crate::settings::Lead::Browser => "An empty prompt now opens the atlas.",
                });
            }
            Act::Rename => self.open_palette(PaletteMode::Rename),
            Act::RenameTab => self.open_palette(PaletteMode::RenameTab(self.active)),
            Act::Close => self.dismiss_hints(),
            Act::Bundle(id) => self.bundle_toggle(&id),
            Act::Scroll(_) => {}
        }
        self.save_prefs();
        self.dirty = true;
    }
}

impl App {
    /// Demos that need a moment: hints and find once the shell is back at
    /// a prompt, reader once the page has loaded.
    pub(crate) fn welcome_tick(&mut self) {
        let Some((act, since)) = self.welcome_pending.clone() else { return };
        if since.elapsed().as_secs_f32() > 12.0 {
            self.welcome_pending = None;
            return;
        }
        match act {
            Act::Hints | Act::Find => {
                let ready = self.tabs.get_mut(self.active).map(|t| matches!(t.focused(), Pane::Term(p) if p.term.at_prompt() && p.running_since.is_none())).unwrap_or(false);
                if ready && since.elapsed().as_millis() > 700 {
                    self.welcome_pending = None;
                    if act == Act::Hints {
                        self.term_hints_open();
                    } else {
                        self.term_search_open();
                    }
                }
            }
            Act::Reader => {
                let loaded = self.tabs.get(self.active).and_then(|t| match (&t.left, &t.right) {
                    (Pane::Web(w), _) | (_, Some(Pane::Web(w))) => Some(!w.tab.shared.borrow().loading && w.reader.is_none()),
                    _ => None,
                }).unwrap_or(false);
                if loaded && since.elapsed().as_millis() > 900 {
                    self.welcome_pending = None;
                    self.toggle_reader();
                }
            }
            _ => self.welcome_pending = None,
        }
    }
}
