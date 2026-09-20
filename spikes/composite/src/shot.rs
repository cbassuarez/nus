//! The app photographs itself.
//!
//! `NUS_SHOT=<script>` runs a step list on the app's own event loop — the
//! same functions the chords and clicks call, not synthetic input — and at
//! each `shot` step draws the scene into an offscreen texture and writes it
//! as a PNG. Nothing here touches the OS's input or focus, so it can run
//! while the machine is in use. `NUS_MODE` picks the face; the file is
//! `<name>-<face>.png` in `NUS_SHOT_OUT` (default `docs/media`).
//!
//! The script is one step per line, `verb rest`; `#` starts a comment:
//!
//!   wait 800                   milliseconds
//!   url http://localhost:8000/ open in the split, as the URL rule does
//!   tab https://…              open as a new tab
//!   newshell                   a new shell tab, in front
//!   shell git status           type into the shell at its prompt and run
//!   line cargo te              type into the shell without running
//!   palette go cargo te        the palette (go | new | url | place) with a query
//!   enter                      commit the palette
//!   ask why did that fail?     the ask panel, with the question sent
//!   theme nord                 a stock theme by name
//!   board | compact | atlas | settings | devtools | reader | split | sidebar
//!   settingsat 2               open settings at a section (2 is STARTUP)
//!   settingsscroll 900         scroll the open settings page to 900 logical px
//!   hover 40 200               the pointer at logical px from the top-left
//!   click 900 500              a left click there
//!   rclick 900 500             a right click there (the page's menu)
//!   altclick 900 500           with Alt held (a peek)
//!   srcclick 900 500           with Alt+Shift held (click to source)
//!   shot window                capture the whole window
//!   shot hero 0.5 0.06 0.5 0.94   capture a fraction [x y w h] of it
//!   focus shell | page         which half of the split has the focus
//!   erase 8                    backspaces to the shell, undoing a `line`
//!   close                      the palette, ask, board and atlas, whichever is up
//!   restore                    the last session, as the atlas would
//!   hands allow | deny | host  answer the hands band on the active page
//!   timeline                   toggle the timeline on the active tab
//!   tl left | right | b | home  a key to the timeline
//!   share                      the active tab as a replay file, opened as a tab
//!   ctrlc                      Ctrl+C to the shell
//!   home | hometype <text> | homeclear | homeenter   the prompt: open it, type into it, empty it, commit
//!   mecard [sync|folder|forge|token|key|next|back]   the profile card: the view, a step of the walk, or a press
//!   files | bind <folder> | treeclick <row>   the sidebar's FILES page: turn it, bind the window, click a row
//!   news <unix seconds>        the prompt tells what happened since then (while you were away)
//!   homelook plate | line | art <key>   HOME: the prompt under the plate, the line alone, or an art behind it
//!   other https://…            a tab opened by something other than you (TABS · OPENED BY OTHERS)
//!   copyurl                    the focused page's url to the clipboard, with its toast
//!   link allow | deny          answer the link band on the focused shell
//!   newtab                     open the configured start page in a new tab
//!   startpage prompt|home|last|layout [url or layout name]   set the start page
//!   assertpane home|web|term|settings   assert the focused pane kind
//!   asserttabs <count>          assert the number of tabs
//!   newwindowlook prompt|shell|launch   set new-window behavior
//!   newwindow                  a second window
//!
//! NUS_SHOT_DIR isolates a macOS bundle's profile during scripted checks.
//!   quit                       (implicit at the end)

use std::path::PathBuf;
use std::time::{Duration, Instant};

use winit::event::{ElementState, MouseButton};
use winit::keyboard::ModifiersState;

use crate::app::{App, PaletteMode, Pane};

pub struct Shot {
    steps: Vec<String>,
    next: usize,
    until: Option<Instant>,
    out: PathBuf,
    face: &'static str,
    /// A capture asked for by the last step, taken after the next draw.
    pub pending: Option<(String, Option<[f32; 4]>)>,
    pub done: bool,
}

impl Shot {
    /// From `NUS_SHOT`, if set.
    pub fn from_env() -> Option<Shot> {
        Shot::from_var("NUS_SHOT")
    }

    /// A second window runs `NUS_SHOT2` (else nothing), so the first
    /// window's script can open one and photograph it.
    pub fn from_env_secondary() -> Option<Shot> {
        Shot::from_var("NUS_SHOT2")
    }

    fn from_var(var: &str) -> Option<Shot> {
        let path = std::env::var_os(var)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("NUS_SHOT: {}: {e}", path.to_string_lossy());
                return None;
            }
        };
        let steps = text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        let out = std::env::var_os("NUS_SHOT_OUT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("docs/media"));
        let face = if std::env::var("NUS_MODE").ok().as_deref() == Some("paper") { "paper" } else { "ink" };
        Some(Shot { steps, next: 0, until: None, out, face, pending: None, done: false })
    }
}

impl App {
    /// One step per call, when the last wait is over and no capture is
    /// outstanding. Called from the main loop before the frame.
    pub fn shot_tick(&mut self) {
        let Some(s) = self.shot.as_mut() else { return };
        if s.done || s.pending.is_some() {
            return;
        }
        if s.until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        s.until = None;
        let Some(step) = s.steps.get(s.next).cloned() else {
            s.done = true;
            return;
        };
        s.next += 1;
        let (verb, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
        let rest = rest.trim();
        eprintln!("shot: {step}");
        match verb {
            "dockattention" => {let _=self.proxy.send_event(crate::UserEvent::DockAttention);}
            "wait" => {
                let ms: u64 = rest.parse().unwrap_or(500);
                if let Some(s) = self.shot.as_mut() {
                    s.until = Some(Instant::now() + Duration::from_millis(ms));
                }
            }
            "url" => self.open_url(rest, false),
            "tab" => self.open_url(rest, true),
            "newshell" => {
                let p = self.behavior.default_profile;
                self.run(crate::app::Action::NewTerminal(p));
            }
            "shell" => self.shot_type(&format!("{rest}\r")),
            "line" => self.shot_type(rest),
            "erase" => {
                let n: usize = rest.parse().unwrap_or(1);
                self.shot_type(&"".repeat(n));
            }
            "focus" => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.focus_right = rest == "page" && t.right.is_some();
                }
                self.layout();
            }
            "palette" => {
                let (mode, query) = rest.split_once(' ').unwrap_or((rest, ""));
                let mode = match mode {
                    "new" => PaletteMode::New,
                    "url" => PaletteMode::Url,
                    "place" => PaletteMode::Place,
                    "settings" => PaletteMode::Settings,
                    _ => PaletteMode::Go,
                };
                self.open_palette(mode);
                if let Some((_, q)) = self.palette.as_mut() {
                    q.push_str(query);
                }
                self.palette_sel = 0;
            }
            "ask" => {
                // Ask sits beside a shell: the shell must hold the focus.
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.focus_right = false;
                }
                if self.ask_term().map_or(true, |t| t.ask.is_none()) {
                    self.toggle_ask();
                }
                if let Some(ask) = self.ask_term().and_then(|t| t.ask.as_mut()) {
                    ask.input = rest.to_string();
                }
                self.ask_send();
            }
            "theme" => {
                let want = rest.to_lowercase();
                if let Some(t) = crate::themes::all().into_iter().find(|t| t.name.to_lowercase() == want) {
                    self.apply_theme(&t);
                } else {
                    eprintln!("shot: no theme named {rest}");
                }
            }
            "board" => self.open_board(),
            "restore" => self.restore_session_pub(),
            "timeline" => self.toggle_timeline(),
            "tl" => {
                use winit::keyboard::{Key, NamedKey};
                let k = match rest {
                    "left" => Key::Named(NamedKey::ArrowLeft),
                    "right" => Key::Named(NamedKey::ArrowRight),
                    "home" => Key::Named(NamedKey::Home),
                    "end" => Key::Named(NamedKey::End),
                    "esc" => Key::Named(NamedKey::Escape),
                    _ => Key::Character("b".into()),
                };
                self.timeline_key(&k);
            }
            "share" => self.run(crate::app::Action::ShareReplay),
            "home" => self.open_home(),
            "hometype" => {
                let i = self.active;
                if let Some(Pane::Home(h)) = self.tabs.get_mut(i).map(|t| &mut t.left) {
                    h.input.push_str(rest);
                }
            }
            "homeclear" => {
                let i = self.active;
                if let Some(Pane::Home(h)) = self.tabs.get_mut(i).map(|t| &mut t.left) {
                    h.input.clear();
                    h.sel = 0;
                }
            }
            "homeenter" => self.home_commit_pub(),
            "news" => {
                self.news.since = Some(rest.trim().parse::<u64>().unwrap_or(0));
                self.news.made = None;
            }
            "files" => self.toggle_files(),
            "bind" => self.bind_workspace(if rest.trim().is_empty() { None } else { Some(std::path::PathBuf::from(rest.trim())) }),
            "treeclick" => {
                let k = rest.trim().parse::<usize>().unwrap_or(0);
                self.tree_click(k);
            }
            "mecard" => match rest.trim() {
                "next" => self.me_next_pub(),
                "back" => self.me_back_pub(),
                "sync" => self.open_me_card_at(crate::me::Step::Sync),
                "folder" => self.open_me_card_at(crate::me::Step::Folder),
                "forge" => self.open_me_card_at(crate::me::Step::Forge),
                "token" => self.open_me_card_at(crate::me::Step::ForgeToken),
                "key" => self.open_me_card_at(crate::me::Step::Key),
                _ => self.open_me_card(),
            },
            "enter" => self.palette_commit(),
            "other" => self.open_url_by_other(rest),
            "copyurl" => {
                self.copy_page_url();
            }
            "homelook" => {
                use crate::settings::HomeLook;
                let mut words = rest.split_whitespace();
                self.behavior.home_look = match words.next() {
                    Some("plate") => HomeLook::Plate,
                    Some("art") => {
                        if let Some(key) = words.next() {
                            self.behavior.home_art = key.to_string();
                        }
                        HomeLook::Art
                    }
                    _ => HomeLook::Line,
                };
                self.dirty = true;
            }
            "link" => {
                use winit::keyboard::{Key, NamedKey};
                let k = if rest == "deny" { Key::Named(NamedKey::Escape) } else { Key::Named(NamedKey::Enter) };
                self.link_band_key(&k);
            }
            "newtab" => self.open_start_page(false),
            "startpage" => {
                use crate::settings::{Hit, Then};
                let (kind, value) = rest.split_once(' ').unwrap_or((rest, ""));
                let page = match kind {
                    "home" => { self.behavior.home_url = value.into(); Then::HomePage }
                    "last" => Then::LastPage,
                    "layout" => { self.behavior.then_layout = value.into(); Then::Layout }
                    "prompt" => Then::Prompt,
                    _ => panic!("unknown start page: {kind}"),
                };
                self.apply_setting(Hit::Then(page), 0.0);
                self.save_prefs();
            }
            "assertpane" => {
                let kind = self.tabs.get(self.active).map(|t| match &t.left {
                    Pane::Home(_) => "home", Pane::Web(_) => "web", Pane::Term(_) => "term", Pane::Settings(_) => "settings", Pane::Hints(_) => "welcome", Pane::Downloads(_) => "downloads", _ => "other",
                }).unwrap_or("missing");
                assert_eq!(kind, rest, "focused pane at script step {}", s.next);
            }
            "asserturl" => {
                let Some(Pane::Web(web)) = self.tabs.get(self.active).map(|t| &t.left) else { panic!("expected web page") };
                assert_eq!(web.tab.shared.borrow().url, rest);
            }
            "asserttabs" => assert_eq!(self.tabs.len(), rest.parse::<usize>().expect("tab count")),
            "newwindowlook" => {
                self.behavior.new_window = match rest {
                    "prompt" => crate::settings::NewWindow::Prompt,
                    "shell" => crate::settings::NewWindow::Shell,
                    "launch" => crate::settings::NewWindow::Launch,
                    _ => panic!("unknown new-window behavior"),
                };
                self.save_prefs();
            }
            "newwindow" => self.run(crate::app::Action::NewWindow),
            "hands" => {
                let a = match rest {
                    "deny" => crate::hands::Answer::Deny,
                    "host" => crate::hands::Answer::AllowHost,
                    _ => crate::hands::Answer::Allow,
                };
                let i = self.active;
                let right = self.tabs.get(i).is_some_and(|t| !matches!(t.left, Pane::Web(_)));
                self.hands_answer(i, right, a);
            }
            "ctrlc" => self.shot_type(""),
            "compact" => self.toggle_compact(),
            "atlas" => self.open_start(),
            "settings" => self.open_settings(),
            "hatchname" => self.tabs[self.active].name=Some(rest.into()),
            "hatchwork" => self.show_hatch_work(),
            "menudrawer"=>self.toggle_menu_drawer(None),
            "menutray"=>{let _=self.proxy.send_event(crate::UserEvent::MenuDrawer(None));},
            "menufooter"=>{let r=self.menu_drawer.footer.expect("drawer footer icon");self.mouse_moved(r.x+r.w/2.0,r.y+r.h/2.0);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);},
            "menuassert"=>{
                let visible=self.menu_drawer.window.as_ref().is_some_and(|d|d.visible);assert_eq!(visible,rest!="hidden");
                if visible{let d=self.menu_drawer.window.as_ref().unwrap();assert!(d.target.size.0<=d.monitor.2&&d.target.size.1<=d.monitor.3);let p=d.window.outer_position().unwrap();assert!(p.x>=d.monitor.0&&p.y>=d.monitor.1&&p.x+d.target.size.0 as i32<=d.monitor.0+d.monitor.2 as i32&&p.y+d.target.size.1 as i32<=d.monitor.1+d.monitor.3 as i32,"drawer escaped its display");}
            },
            "menuprivacy"=>{let d=self.menu_drawer.window.as_ref().unwrap();assert!(!d.hits.iter().any(|(_,_,label,_)|label.contains(rest)),"private name in drawer accessibility labels");},
            "menuclick"=>{
                self.menu_drawer_frame();let d=self.menu_drawer.window.as_mut().expect("drawer exists");let (r,_,_,content)=d.hits.iter().find(|(_,hit,_,_)|format!("{hit:?}").starts_with(rest)).cloned().expect("drawer action exists");
                if content {d.scroll=(d.scroll+(r.y-d.viewport.y).max(0.0)).min(d.reach);}self.menu_drawer_frame();
                let d=self.menu_drawer.window.as_mut().unwrap();let(r,_,_,_)=d.hits.iter().find(|(_,hit,_,_)|format!("{hit:?}").starts_with(rest)).unwrap();d.pos=(r.x+r.w/2.0,r.y+r.h/2.0);
                self.menu_drawer_event(winit::event::WindowEvent::MouseInput{device_id:winit::event::DeviceId::dummy(),state:ElementState::Released,button:MouseButton::Left});
            },
            "menushot"=>{
                self.menu_drawer_frame();let d=self.menu_drawer.window.as_ref().expect("drawer exists");let bytes=self.gpu.snapshot(d.target.size,&d.scene,self.theme.paper);let shot=self.shot.as_ref().unwrap();std::fs::create_dir_all(&shot.out).unwrap();let path=shot.out.join(format!("{rest}-{}.png",shot.face));let mut enc=png::Encoder::new(std::fs::File::create(&path).unwrap(),d.target.size.0,d.target.size.1);enc.set_color(png::ColorType::Rgba);enc.set_depth(png::BitDepth::Eight);enc.write_header().unwrap().write_image_data(&bytes).unwrap();eprintln!("MENU SHOT: {}",path.display());
            },
            "hatchtoggle" => {let _=self.proxy.send_event(crate::UserEvent::Hatch);},
            "hatchhide" => self.hide_hatch(),
            "hatchshow" => self.show_hatch(),
            "hatchterminal" => self.hatch_click(crate::hatch::Hit::Terminal),
            "hatchclick" => {
                let hit=match rest {"work"=>crate::hatch::Hit::Work,"terminal"=>crate::hatch::Hit::Terminal,"pin"=>crate::hatch::Hit::Pin,"expand"=>crate::hatch::Hit::Land,"new"=>crate::hatch::Hit::New,_=>panic!("unknown hatch click")};
                let rect=self.hatch.as_ref().unwrap().hits.iter().find(|(_,h)|*h==hit).map(|(r,_)|*r).expect("hatch control visible");
                self.hatch_mouse(MouseButton::Left,ElementState::Pressed,(rect.x+rect.w/2.0,rect.y+rect.h/2.0));
            }
            "hatchassertstatus" => {
                let rows=crate::hatch_work::collect(self);
                assert!(rows.iter().any(|r|format!("{:?}",r.status).eq_ignore_ascii_case(rest)),"missing {rest}: {rows:?}");
            }
            "hatchassertvisible" => assert_eq!(self.hatch.as_ref().is_some_and(|h|h.visible),rest=="true"),
            "hatchbadgeshot" => {
                let badge=self.hatch_state.badge.as_ref().expect("island exists");
                let bytes=self.gpu.snapshot(badge.target.size,&badge.scene,[0.0;4]);
                let shot=self.shot.as_ref().unwrap();std::fs::create_dir_all(&shot.out).unwrap();
                let path=shot.out.join(format!("{rest}-{}.png",shot.face));
                let mut enc=png::Encoder::new(std::fs::File::create(&path).unwrap(),badge.target.size.0,badge.target.size.1);
                enc.set_color(png::ColorType::Rgba);enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&bytes).unwrap();
            }
            "hatchshot" => {
                self.hatch_frame();
                let h=self.hatch.as_ref().expect("hatch exists");
                let bytes=self.gpu.snapshot(h.target.size,&h.scene,self.theme.paper);
                let shot=self.shot.as_ref().unwrap();std::fs::create_dir_all(&shot.out).unwrap();
                let path=shot.out.join(format!("{rest}-{}.png",shot.face));
                let mut enc=png::Encoder::new(std::fs::File::create(&path).unwrap(),h.target.size.0,h.target.size.1);
                enc.set_color(png::ColorType::Rgba);enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&bytes).unwrap();
                eprintln!("HATCH SHOT: {}",path.display());
            }
            "hatchcheck" => {
                let target=crate::hatch_work::collect(self).first().expect("a real shell").target;
                let before=self.tabs.iter().find(|t|t.id==target.tab).unwrap();
                let Pane::Term(term)=&before.left else {panic!("shell")};
                let pid=term.pty.pid();let cwd=term.term.cwd.clone();
                let count=self.tabs.len();
                assert!(self.open_hatch_target(target));
                self.hatch_click(crate::hatch::Hit::Work);
                self.hide_hatch();self.show_hatch();
                assert!(self.open_hatch_target(target));
                self.apply_setting(crate::settings::Hit::HatchLook(crate::settings::HatchLook::Card),0.0);
                self.apply_setting(crate::settings::Hit::HatchLook(crate::settings::HatchLook::Sheet),0.0);
                let tab=self.tabs.iter().find(|t|t.id==target.tab).unwrap();
                let Pane::Term(term)=&tab.left else {panic!("shell")};
                assert_eq!(pid,term.pty.pid(),"handoff restarted shell");assert_eq!(cwd,term.term.cwd,"handoff changed cwd");
                assert!(self.tabs.len()<=count+1,"unexpected sessions created");
                self.land();assert!(!self.tabs.iter().find(|t|t.id==target.tab).unwrap().hatch);
                assert!(self.open_hatch_target(target));
                let mut stale=target;stale.tab=u64::MAX;assert!(!self.open_hatch_target(stale));
                eprintln!("HATCH CHECK: real PTY identity, cwd, work/terminal switching, presentation, land, stale targets passed");
            }
            "hatchbackground" => {
                self.behavior.hatch_background=true;self.hide_hatch_inner(false);
                let _=self.proxy.send_event(crate::UserEvent::WindowControl(self.window.id(),0));
            }
            "hatchassertbackground" => assert!(self.hatch_state.main_hidden),
            "hatchopen" => {
                let target=self.hatch_state.work.iter().find(|i|i.title==rest).expect("named real session").target;
                self.hatch_click(crate::hatch::Hit::Job(target));
            }
            "hatchassertsession" => assert_eq!(self.tabs[self.hatch_tab().expect("session in hatch")].name.as_deref(),Some(rest)),
            "hatchnarrow" => {
                let h=self.hatch.as_ref().unwrap();let scale=h.window.scale_factor();
                let _=self.proxy.send_event(crate::UserEvent::HatchResize(self.window.id(),(360.0*scale) as u32,(420.0*scale) as u32));
            }
            "hatchassertnarrow" => {
                let h=self.hatch.as_ref().unwrap();
                assert_eq!(h.target.size.0,(360.0*h.window.scale_factor()) as u32,"render surface did not resize");
                assert_eq!(h.window.inner_size().width,h.target.size.0,"native and rendered widths differ");
            }
            "hatchoptionscheck" => {
                use crate::settings::{Hit,HatchLook};
                self.apply_setting(Hit::HatchAutohide(true),0.0);
                self.hatch.as_mut().unwrap().pinned=true;self.hatch_focus(false);
                assert!(!self.hatch.as_ref().unwrap().hiding,"pinned Hatch dismissed");
                self.hatch.as_mut().unwrap().pinned=false;self.hatch_focus(false);
                assert!(self.hatch.as_ref().is_some_and(|h|h.hiding||!h.visible),"blur did not dismiss");
                self.show_hatch();self.apply_setting(Hit::HatchLook(HatchLook::Card),0.0);
                self.apply_setting(Hit::HatchDim(true),0.0);
                self.apply_setting(Hit::HatchNotify(true),0.0);
                self.apply_setting(Hit::HatchStatus(false),0.0);
            }
            "hatchassertnotch" => {
                let hatch=self.hatch.as_ref().expect("Hatch window");
                let top=crate::hatch_native::top_area(hatch.mon.0,hatch.mon.1,hatch.mon.4);
                if let Some(notch)=top.notch {
                    let position=hatch.window.outer_position().expect("native position");
                    assert_eq!(position.y,hatch.mon.1+notch.height as i32,"dropdown is below the menu bar instead of joined to the notch");
                    assert_eq!(position.x+hatch.size.0 as i32/2,hatch.mon.0+notch.left+notch.width as i32/2,"dropdown is not centered on the camera housing");
                    let badge=self.hatch_state.badge.as_ref().expect("island connector");
                    assert!(badge.visible,"island connector missing");
                    assert_eq!(badge.window.outer_position().unwrap().y,hatch.mon.1,"island must start at screen.frame, not visibleFrame");
                    eprintln!("HATCH NOTCH: {:?}, native dropdown {:?}, native island {:?}",top,position,badge.window.outer_position());
                } else {eprintln!("HATCH NOTCH: no camera cutout on this display; top-edge fallback");}
            }
            "hatchassertshade" => assert!(self.hatch_state.shade.as_ref().is_some_and(|s|s.visible)),
            "hatchassertnotice" => {
                let (item,_) = self.hatch_state.completion.as_ref().expect("real completion notice");
                assert!(matches!(item.status,crate::hatch_work::Status::Finished|crate::hatch_work::Status::Failed));
                assert!(self.hatch_state.badge.as_ref().is_some_and(|b|b.visible),"notice not visible");
                assert!(!self.hatch.as_ref().is_some_and(|h|h.visible),"completion opened Hatch");
                assert!(!self.hatch_state.badge.as_ref().unwrap().window.has_focus(),"completion notice stole focus");
            }
            "hatchassertquiet" => assert!(!self.hatch_state.badge.as_ref().is_some_and(|b|b.visible),"disabled status stayed visible"),
            "hatchsize" => {
                use crate::settings::{Hit,HatchLook};
                self.apply_setting(Hit::HatchAutohide(false),0.0);
                self.apply_setting(Hit::HatchLook(HatchLook::Card),0.0);
                self.apply_setting(Hit::HatchSize(rest.parse().unwrap()),0.0);
                if self.hatch.is_none() {self.toggle_hatch();}
            }
            "asserthatchsize" => {
                let hatch=self.hatch.as_ref().expect("hatch created");
                let expected=rest.parse::<f32>().unwrap()/100.0;
                let actual=hatch.window.inner_size().height as f32/hatch.mon.3 as f32;
                assert!((actual-expected).abs()<0.02,"hatch height {actual} did not follow {expected}");
            }
            "assertnoshells" => {
                assert!(!self.tabs.iter().any(|t| matches!(t.left,Pane::Term(_)) || matches!(t.right,Some(Pane::Term(_)))), "unexpected shell tab");
                assert!(self.held_loose().is_empty(), "launch created a hidden held shell");
            }
            "appearance" => {
                self.apply_setting(crate::settings::Hit::Theme(Some(rest=="ink")),0.0);
                self.save_prefs();
            }
            "assertappearance" => assert_eq!(if self.theme.mode==nus_render::Mode::Ink {"ink"} else {"paper"},rest),
            "settingseek" => {
                if !self.settings_hits.iter().any(|(_,h)|format!("{h:?}").starts_with(rest)) {
                    let Pane::Settings(page)=&mut self.tabs[self.active].left else {panic!("not settings")};
                    let next=(page.scroll+page.rect.h*0.6).min((self.settings_reach-page.rect.h+self.scale*48.0).max(0.0));
                    assert!(next>page.scroll,"setting not found: {rest}");page.scroll=next;
                    let script=self.shot.as_mut().unwrap();script.next-=1;script.until=Some(Instant::now()+Duration::from_millis(80));
                    self.dirty=true;
                }
            }
            "sliderdrag" => {
                let v=rest.parse::<f32>().unwrap();
                let (rect,kind,x,w)=self.settings_hits.iter().find_map(|(r,h)|match h {crate::settings::Hit::Slider(k,x,w)=>Some((*r,*k,*x,*w)),_=>None}).expect("visible slider");
                self.mouse_moved(rect.x+rect.w/2.0,rect.y+rect.h/2.0);
                self.mouse_button(MouseButton::Left,ElementState::Pressed);
                self.mouse_moved(x+w*v,rect.y+rect.h/2.0);
                self.mouse_button(MouseButton::Left,ElementState::Released);
                assert!((self.slider_value(kind)-v).abs()<0.02,"slider did not follow drag");
            }
            "settingclick" => {
                let (rect,_) = self.settings_hits.iter().find(|(_,hit)|format!("{hit:?}")==rest).copied().unwrap_or_else(||panic!("setting not visible: {rest}"));
                self.mouse_moved(rect.x+rect.w/2.0,rect.y+rect.h/2.0);
                self.mouse_button(MouseButton::Left,ElementState::Pressed);
                self.mouse_button(MouseButton::Left,ElementState::Released);
            }
            "assertchoice" => {
                let Pane::Settings(p)=&self.tabs[self.active].left else {panic!("not settings")};
                assert!(self.setting_states(p.section).iter().any(|(hit,on)|format!("{hit:?}")==rest && *on),"choice is not selected: {rest}");
            }
            "welcomeclick" => {
                let (rect,_) = self.welcome_hits.iter().find(|(_,act)|format!("{act:?}")==rest).cloned().unwrap_or_else(||panic!("welcome action not visible: {rest}"));
                self.mouse_moved(rect.x+rect.w/2.0,rect.y+rect.h/2.0);
                self.mouse_button(MouseButton::Left,ElementState::Pressed);
                self.mouse_button(MouseButton::Left,ElementState::Released);
            }
            "searchcheck" => {
                for (query,expected) in [("locatoin","YOUR LOCATION"),("new tab","START PAGE"),("font wieght","WEIGHT"),("rounded corners","RADIUS"),("theme favorites","FOOTER"),("replay","REPLAY"),("areal","FONT")] {
                    let rows=self.search_settings(query);
                    assert!(rows.iter().take(3).any(|r|r.text.to_uppercase().contains(expected)),"{query}: {:?}",rows.iter().map(|r|r.text.clone()).collect::<Vec<_>>());
                }
                assert!(matches!(self.search_settings("zzzz impossible")[0].action,crate::app::Action::Noop));
            }
            "assertsearch" => {
                let (_,q)=self.palette.as_ref().expect("search open");
                let results=self.search_settings(q);
                assert!(results[0].text.to_lowercase().contains(&rest.to_lowercase()),"top result: {}",results[0].text);
            }
            "assertrevealed" => {
                assert!(self.settings_target.is_none(),"search did not scroll to its row");
                let (sec,_,_,_)=self.settings_highlight.expect("search highlight");
                let Pane::Settings(p)=&self.tabs[self.active].left else {panic!("not settings")};
                assert_eq!(p.section,sec);
            }
            "assertplace" => {
                assert_eq!(self.place().is_some(),rest=="set");
                if rest=="unset" { assert!(self.behavior.place.is_none()); }
            }
            "fontcheck" => {
                use crate::fonts::{Family,Weight};
                let saved=self.behavior.clone();
                for family in Family::ALL {for weight in Weight::ALL {
                    self.apply_setting(crate::settings::Hit::UiFont(family),0.0);
                    self.apply_setting(crate::settings::Hit::UiWeight(weight),0.0);
                    self.save_prefs();
                    assert!(self.setting_states(0).iter().any(|(h,on)|*h==crate::settings::Hit::UiWeight(weight) && *on));
                    assert_eq!(crate::prefs::Prefs::load().behavior.unwrap().ui_weight,weight);
                }}
                for family in Family::MONO {for weight in Weight::ALL {
                    self.apply_setting(crate::settings::Hit::TermFont(family),0.0);
                    self.apply_setting(crate::settings::Hit::TermWeight(weight),0.0);
                    self.save_prefs();
                    assert_eq!(crate::prefs::Prefs::load().behavior.unwrap().term_font,family);
                }}
                self.behavior=saved;self.apply_fonts();self.save_prefs();
            }
            "footer" => {
                self.sidebar=true;self.layout();self.open_look_menu();
                if let Some((r,_))=self.side_hits.iter().find(|(_,h)|*h==crate::app::SideHit::Look) {self.mouse=(r.x+r.w/2.0,r.y+r.h/2.0);}
            }
            "footeradd" => {
                let names=crate::themes::all().into_iter().map(|t|t.name).collect();
                self.behavior.footer_themes=Some(names);self.save_prefs();
            }
            "footercheck" => {
                let r=self.look_rect.expect("footer menu open");let sb=self.sidebar_rect();
                assert!(r.x>=sb.x && r.right()<=sb.right()+1.0,"footer exceeds sidebar");
                let hits:Vec<_>=self.side_hits.iter().filter(|(_,h)|matches!(h,crate::app::SideHit::LookPreset(_))).collect();
                assert_eq!(hits.len(),9,"nine visible slots");
                for (tile,_) in hits { assert_eq!(tile.intersect(&r),*tile,"tile escapes menu"); }
            }
            "themeoutsideclick"=>{let sb=self.sidebar_rect();let (r,index)=self.side_hits.iter().find_map(|(r,h)|if let crate::app::SideHit::LookPreset(i)=h{(!sb.contains(r.x+r.w*0.5,r.y+r.h*0.5)).then_some((*r,*i))}else{None}).expect("theme card outside narrow sidebar");let name=crate::themes::all()[index].name.clone();self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);assert_eq!(self.preset_name,name);}
            "footerscroll" => {
                let r=self.look_rect.expect("footer menu");self.mouse=(r.x+r.w/2.0,r.y+r.h/2.0);
                self.wheel(winit::event::MouseScrollDelta::LineDelta(0.0,-6.0));
                assert!(self.look_scroll>0.0,"footer scroll did not move");
            }
            "radius" => {self.surface.shell_radius=rest.parse().unwrap();self.save_prefs();self.dirty=true;}
            "looktab" => {self.open_settings_at(0,Some(rest.parse().unwrap()));}
            "welcomescroll" => {
                if let Pane::Hints(p)=&mut self.tabs[self.active].left {p.scroll=rest.parse::<f32>().unwrap()*self.scale;}
            }
            "welcometap" => {
                let hit=*self.welcome_shapes.first().expect("live welcome vectors");
                assert!(self.welcome_click(hit.x+hit.w/2.0,hit.y+hit.h/2.0));
            }
            "download" => {let tab=&self.tabs[self.active];let pane=if tab.focus_right{tab.right.as_ref().unwrap_or(&tab.left)}else{&tab.left};let Pane::Web(w)=pane else{panic!("download needs web page")};w.tab.download(rest);}
            "downloads" => {if rest=="modal"{self.side_action(crate::app::SideHit::Downloads,false);}else{self.open_downloads();}}
            "downloadmode" => {let mode=match rest{"all"=>crate::downloads::Rename::All,"selective"=>crate::downloads::Rename::Selective,_=>crate::downloads::Rename::Off};self.apply_setting(crate::settings::Hit::DownloadRename(mode),0.0);self.save_prefs();}
            "downloadclick"=>{let (r,_)=self.download_ui.hits.iter().find(|(_,h)|h.label().to_lowercase().starts_with(&rest.to_lowercase())).copied().expect("download button");self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);}
            "downloadact" => {let d=crate::downloads::list().into_iter().find(|d|d.active()).expect("active download");let h=match rest{"pause"=>crate::downloads::Hit::Pause(d.key),"resume"=>crate::downloads::Hit::Resume(d.key),_=>crate::downloads::Hit::Cancel(d.key)};self.download_action(h);}
            "downloadassert" => {let rows=crate::downloads::list();match rest{
                "paused"=>assert!(rows.iter().any(|d|d.paused&&d.active())),
                "active"=>assert!(rows.iter().any(|d|d.active()&&d.received>0&&d.received<d.total)),
                "complete"=>assert!(rows.iter().any(|d|d.done&&std::path::Path::new(&d.path).is_file())),
                "cancelled"=>assert!(rows.iter().any(|d|d.cancelled)),
                "unique"=>{let paths:std::collections::HashSet<_>=rows.iter().map(|d|&d.path).collect();assert_eq!(paths.len(),rows.len());},
                name=>assert!(rows.iter().any(|d|d.name==name),"missing {name}: {rows:?}"),
            }}
            "downloadhover"=>{let r=self.download_ui.anchor.expect("footer download icon");self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);}
            "sidebarwidth"=>{self.sidebar=true;self.sidebar_hover=true;self.sidebar_leave=None;self.sidebar_shift=0.0;self.sidebar_rules.compact=false;self.sidebar_rules.width=rest.parse().unwrap();self.layout();self.save_prefs();}
            "smallsidetype"=>{self.sidebar_rules.small_tabs=match rest{"icons"=>crate::sidebar::SmallTabs::Icons,"preview"=>crate::sidebar::SmallTabs::Preview,_=>crate::sidebar::SmallTabs::Favicons};self.layout();}
            "sidebardrag"=>{let sb=self.sidebar_rect();let x=if self.sidebar_right(){sb.x}else{sb.right()};self.mouse_moved(x,sb.y+self.px(120.0));self.mouse_button(MouseButton::Left,ElementState::Pressed);assert!(self.sidebar_resize.is_some());let width=rest.parse::<f32>().unwrap()*self.scale;let x=if self.sidebar_right(){sb.right()-width}else{sb.x+width};self.mouse_moved(x,sb.y+self.px(120.0));self.mouse_button(MouseButton::Left,ElementState::Released);assert!(self.sidebar_resize.is_none());assert!((self.sidebar_w()-width).abs()<1.1);}
            "sidebarcheck"=>{let sb=self.sidebar_rect();for (r,h) in &self.side_hits{if matches!(h,crate::app::SideHit::Profile|crate::app::SideHit::Settings|crate::app::SideHit::Files|crate::app::SideHit::Downloads|crate::app::SideHit::MenuDrawer|crate::app::SideHit::Look){assert!(r.x>=sb.x-1.0&&r.right()<=sb.right()+1.0&&r.y>=sb.y&&r.bottom()<=sb.bottom()+1.0,"footer target escapes: {h:?} {r:?}");}}assert!(self.download_ui.anchor.is_some());}
            "footerdrag"=>{let sb=self.sidebar_rect();let y=sb.bottom()-self.sidebar_footer_h();self.mouse_moved(sb.x+sb.w*0.5,y);self.mouse_button(MouseButton::Left,ElementState::Pressed);assert_eq!(self.sidebar_resize,Some(crate::sidebar::Resize::Footer));self.mouse_moved(sb.x+sb.w*0.5,y-self.px(12.0));self.mouse_button(MouseButton::Left,ElementState::Released);assert!(self.sidebar_rules.footer_row>32.0);}
            "gridcheck"=>{let tab=&self.tabs[self.active];let Some(right)=&tab.right else{panic!("expected split")};let a=tab.left.rect();let b=right.rect();assert_eq!(a.y,b.y);assert_eq!(a.bottom(),b.bottom());for p in [&tab.left,right]{let r=p.rect();assert_eq!(r.x,r.x.round());assert_eq!(r.y,r.y.round());match p{Pane::Term(t)=>assert_eq!(t.origin.1-self.px(16.0),r.y+self.header_h()),Pane::Web(w)=>{assert_eq!(w.page.y,r.y+self.header_h());assert_eq!(w.page.bottom()+self.px(nus_render::theme::metric::PANE_FOOTER),r.bottom());},_=>{}}}}
            "assertsection"=>{let Pane::Settings(p)=&self.tabs[self.active].left else{panic!("not settings")};assert_eq!(p.section,rest.parse::<usize>().unwrap());assert!(!self.me_card.open,"hidden Welcome intercepted navigation");},
            "promptcheck"=>{
                use crate::app::Action;
                let c=self.behavior.prompt.clone();
                for preset in crate::prompt::Preset::ALL {
                    self.behavior.prompt=crate::prompt::Config::preset(preset);
                    for query in ["", "cargo test", "nus.dev", "@codex review this", "? hello", "home"] {
                        let home=self.prompt_rows(query);let palette=self.palette_rows(PaletteMode::Go,query);
                        assert_eq!(home.iter().map(|r|&r.action).collect::<Vec<_>>(),palette.iter().map(|r|&r.action).collect::<Vec<_>>());
                        if query=="home" {assert!(matches!(home[0].action,Action::Home));}
                        if query=="@codex review this" {assert!(matches!(&home[0].action,Action::AssistantDraft(1,p) if p=="review this"));}
                    }
                    let rows=self.prompt_rows("");assert!(!rows.iter().any(|r|r.text.contains("fold every stack")));
                    if preset==crate::prompt::Preset::Minimal{assert!(rows.is_empty());}
                }
                self.behavior.prompt=c;
            },
            "assertpromptfirst"=>{let Some((mode,input))=&self.palette else{panic!("palette not open")};let rows=self.palette_rows(*mode,input);assert!(rows.first().is_some_and(|r|r.text.contains(rest)),"unexpected route: {:?}",rows.iter().map(|r|&r.text).collect::<Vec<_>>());},
            "assertconnections"=>{assert!(self.assistants.pending.is_none(),"connection checks did not finish");for entry in &self.assistants.entries{assert!(entry.checked);assert!(entry.path.is_some());assert!(!entry.version.is_empty());}assert_eq!(self.assistants.entries[2].models,vec!["test-model:latest"],"connections: {:?}",self.assistants.entries);},
            "assertfonts"=>{let c=&self.behavior.typography;let Pane::Term(t)=&self.tabs[self.active].left else{panic!("not terminal")};assert!((t.grid.px-self.terminal_px()).abs()<0.01);let plain=self.fonts.metrics(self.f.term,self.terminal_px());assert!(t.grid.metrics.advance>=plain.advance+c.terminal_spacing*self.scale-0.01);assert!(t.grid.metrics.line_height>=plain.line_height);},
            "assistantdraft"=>{let (id,q)=rest.split_once(' ').unwrap_or((rest,""));self.draft_assistant(id.parse().unwrap(),q);},
            "reviewbounds"=>{assert!(matches!(self.palette,Some((PaletteMode::Assistant(_),_))));let size=self.window.inner_size();for r in self.palette_hits.iter().filter(|r|r.h>0.0){assert!(r.x>=0.0&&r.right()<=size.width as f32&&r.y>=0.0&&r.bottom()<=size.height as f32,"review row outside window: {r:?}");}if rest=="scrollable"{assert!(self.palette_scroll_max>0.0);}},
            "reviewscroll"=>{self.wheel(winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0,-rest.parse::<f64>().unwrap())));},
            "settingsbounds"=>{let Pane::Settings(p)=&self.tabs[self.active].left else{panic!("not settings")};for (r,h) in &self.settings_hits{assert!(r.x>=p.rect.x-1.0&&r.right()<=p.rect.right()+1.0&&r.bottom()<=p.rect.bottom()+1.0,"escaped hit {h:?} {r:?} {:?}",p.rect);}},
            "settingscheck" => self.check_settings_bindings(),
            "assertprofile" => assert_eq!(self.me_card.open, rest == "open"),
            "closeprofile" => self.close_me_card(),
            "welcome" => self.open_welcome(),
            "welcomedismiss" => self.dismiss_hints(),
            "phonecheck" => {
                let tx = self.inbound.clone().expect("instance server");
                let first = crate::phone::start(tx.clone()).expect("phone listener");
                crate::phone::stop();
                assert!(crate::phone::current().is_none());
                assert!(std::net::TcpStream::connect(("127.0.0.1", first.port)).is_err(), "listener must close when disabled");
                assert!(!std::path::Path::new("profile/phone").exists());
                let second = crate::phone::start(tx).expect("phone listener restarted");
                assert_ne!(first.token, second.token, "old phone token must be revoked");
                crate::phone::stop();
            },
            "settingsat" => {
                let sec = rest.trim().parse::<usize>().unwrap_or(0);
                self.run(crate::app::Action::SettingsAt(sec, None));
            }
            "settingsscroll" => {
                // Scroll the open settings page to a logical offset.
                let y = rest.trim().parse::<f32>().unwrap_or(0.0) * self.scale;
                if let Some(crate::app::Pane::Settings(sp)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    sp.scroll = y;
                }
                self.dirty = true;
            }
            "devtools" => self.toggle_devtools(),
            "reader" => self.toggle_reader(),
            "split" => self.divide(),
            "sidebar" => self.run(crate::app::Action::ToggleSidebar),
            "close" => {
                self.palette = None;
                self.start = None;
                if self.board.open {
                    self.close_board();
                }
                if let Some(t) = self.ask_term() {
                    t.ask = None;
                }
                self.layout();
            }
            "hover" | "click" | "rclick" | "altclick" | "srcclick" => {
                let mut it = rest.split_whitespace().filter_map(|n| n.parse::<f32>().ok());
                let (x, y) = (it.next().unwrap_or(0.0) * self.scale, it.next().unwrap_or(0.0) * self.scale);
                if verb == "altclick" {
                    self.modifiers(ModifiersState::ALT);
                }
                if verb == "srcclick" {
                    self.modifiers(ModifiersState::ALT | ModifiersState::SHIFT);
                }
                self.mouse_moved(x, y);
                if verb == "rclick" {
                    self.mouse_button(MouseButton::Right, ElementState::Pressed);
                    self.mouse_button(MouseButton::Right, ElementState::Released);
                } else if verb != "hover" {
                    self.mouse_button(MouseButton::Left, ElementState::Pressed);
                    self.mouse_button(MouseButton::Left, ElementState::Released);
                }
                if verb == "altclick" || verb == "srcclick" {
                    self.modifiers(ModifiersState::empty());
                }
            }
            "shot" => {
                let mut it = rest.split_whitespace();
                let name = it.next().unwrap_or("shot").to_string();
                let f: Vec<f32> = it.filter_map(|n| n.parse().ok()).collect();
                let crop = (f.len() == 4).then(|| [f[0], f[1], f[2], f[3]]);
                if let Some(s) = self.shot.as_mut() {
                    s.pending = Some((name, crop));
                }
            }
            "quit" => {
                if let Some(s) = self.shot.as_mut() {
                    s.done = true;
                }
            }
            other => eprintln!("shot: unknown step `{other}`"),
        }
        self.dirty = true;
    }

    /// Text into the shell of the active tab, or the first shell.
    fn shot_type(&mut self, text: &str) {
        let i = match self.tabs.get(self.active).map(|t| &t.left) {
            Some(Pane::Term(_)) => Some(self.active),
            _ => self.tabs.iter().position(|t| matches!(t.left, Pane::Term(_))),
        };
        if let Some(Pane::Term(t)) = i.and_then(|i| self.tabs.get_mut(i)).map(|t| &mut t.left) {
            let _ = t.pty.write(text.as_bytes());
        }
    }

    /// After a draw: the pending capture, from the scene just drawn.
    pub fn shot_capture(&mut self, clear: [f32; 4]) {
        let Some((name, crop)) = self.shot.as_mut().and_then(|s| s.pending.take()) else { return };
        let (w, h) = self.target.size;
        let crop_px = crop.map(|[x, y, cw, ch]| ((x * w as f32) as u32, (y * h as f32) as u32, ((cw * w as f32) as u32).max(1), ((ch * h as f32) as u32).max(1)));
        let Some(s) = self.shot.as_ref() else { return };
        let _ = std::fs::create_dir_all(&s.out);
        let path = s.out.join(format!("{name}-{}.png", s.face));
        match self.snapshot_png(clear, crop_px, &path) {
            Ok((cw, ch)) => eprintln!("shot: wrote {} ({cw}×{ch} px at {}×)", path.display(), self.scale),
            Err(e) => eprintln!("shot: {}: {e}", path.display()),
        }
    }

    /// The frame just drawn, cropped to `crop` (x, y, w, h in px) or whole,
    /// as a PNG at `path`. Returns the written size.
    pub(crate) fn snapshot_png(&mut self, clear: [f32; 4], crop: Option<(u32, u32, u32, u32)>, path: &std::path::Path) -> Result<(u32, u32), String> {
        let (w, h) = self.target.size;
        let rgba = self.gpu.snapshot((w, h), &self.scene, clear);
        let (x0, y0, cw, ch) = match crop {
            Some((x, y, cw, ch)) => (x.min(w - 1), y.min(h - 1), cw.max(1).min(w - x.min(w - 1)), ch.max(1).min(h - y.min(h - 1))),
            None => (0, 0, w, h),
        };
        let mut px = Vec::with_capacity((cw * ch * 4) as usize);
        for row in y0..y0 + ch {
            let start = ((row * w + x0) * 4) as usize;
            px.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
        }
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), cw, ch);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut wr| wr.write_image_data(&px)).map_err(|e| e.to_string())?;
        Ok((cw, ch))
    }

    /// Page screenshots a remote request asked for: the pane's pixels from
    /// the frame just drawn, to profile/shots/, the path in the answer.
    pub(crate) fn deferred_shots(&mut self, clear: [f32; 4]) {
        if !self.deferred.iter().any(|d| matches!(d.what, crate::remote::DeferredWhat::Shot(..))) {
            return;
        }
        let dir = std::env::current_dir().unwrap_or_default().join("profile").join("shots");
        let _ = std::fs::create_dir_all(&dir);
        let mut keep = Vec::new();
        for d in std::mem::take(&mut self.deferred) {
            let crate::remote::DeferredWhat::Shot(tab, right) = d.what else {
                keep.push(d);
                continue;
            };
            let rect = self.tabs.get(tab).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| match p {
                Pane::Web(w) => Some(w.page),
                _ => None,
            });
            let Some(r) = rect else {
                let _ = d.reply.send(serde_json::json!({ "ok": false, "error": "no page" }));
                continue;
            };
            let path = dir.join(format!("page-{}.png", crate::journal::now()));
            let crop = (r.x.max(0.0) as u32, r.y.max(0.0) as u32, r.w.max(1.0) as u32, r.h.max(1.0) as u32);
            match self.snapshot_png(clear, Some(crop), &path) {
                Ok((w, h)) => {
                    let _ = d.reply.send(serde_json::json!({ "ok": true, "result": { "path": path.display().to_string(), "width": w, "height": h, "scale": self.scale } }));
                }
                Err(e) => {
                    let _ = d.reply.send(serde_json::json!({ "ok": false, "error": e }));
                }
            }
        }
        self.deferred = keep;
    }
}
