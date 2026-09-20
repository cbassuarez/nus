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
//!   settle                     hold until the picture stops moving
//!   window 1600 1000           the window at an exact logical size
//!
//! A recording — the film's footage — is frames on a fixed clock:
//!
//!   record shell-min 4.2       4.2 s at 60 fps → <out>/shell-min/f00000.png…
//!   at 1.579 type git ch 10    a step at that clip time, typed at 10 cps
//!   at 2.763 key down          a chord, through the app's own key handling
//!   at 2.8 await-lsp           the clock stops until the server answers
//!   at 3.0 await-paint         …or until the page paints again
//!
//! `at` lines follow their `record` line and are its schedule; every other
//! verb works inside one. While a recording runs, the clock advances
//! exactly 1/60 s per written frame however long the machine took, so a
//! take lands on the same frames every time (clock.rs).
//!
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
//!   pip                        this tab's video in the floating window
//!   pippoint 240 130           the pointer inside it, in its own pixels
//!   shotpip pip-paused         the floating window's own frame, as a PNG
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
    /// The recording in progress, if any.
    rec: Option<Rec>,
    /// Waiting for the picture to stop moving: the last frame's hash, how
    /// many frames have matched it, and when to give up.
    settle: Option<(u64, u8, Instant)>,
}

/// A clip being recorded: a PNG per frame on the virtual clock, with the
/// steps that are due written against clip time, not wall time.
struct Rec {
    name: String,
    dir: PathBuf,
    /// How many frames the clip is, and which one comes next.
    frames: u64,
    frame: u64,
    /// `at <seconds> <step>`, soonest last (popped off the end).
    at: Vec<(f64, String)>,
    /// Characters still to type, the seconds between them, and the clip
    /// time the next one goes at.
    typing: Option<(std::collections::VecDeque<char>, f64, f64)>,
    /// What the clock is waiting for. While this is set, nothing moves and
    /// no frame is written: Chromium and the language servers are not on
    /// the clock and never will be.
    hold: Option<Hold>,
    began: std::time::Instant,
}

/// Why the clock is standing still.
enum Hold {
    /// Until the focused page paints again (its paint count passes this).
    Paint(u64),
    /// Until the pending language-server answer is on screen.
    Lsp,
}

/// A hold that never comes is a bug, not a hang: after this much real
/// time the recording carries on and says so.
const FUSE: Duration = Duration::from_secs(10);

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
        Some(Shot { steps, next: 0, until: None, out, face, pending: None, done: false, rec: None, settle: None })
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
        if s.settle.is_some() {
            return self.settle_tick();
        }
        if s.rec.is_some() {
            return self.record_frame();
        }
        if s.until.is_some_and(|t| crate::clock::now() < t) {
            return;
        }
        s.until = None;
        let Some(step) = s.steps.get(s.next).cloned() else {
            s.done = true;
            return;
        };
        s.next += 1;
        self.shot_step(&step);
    }

    /// One step, from the script or from a recording's `at` schedule.
    pub(crate) fn shot_step(&mut self, step: &str) {
        let (verb, rest) = step.split_once(' ').unwrap_or((step, ""));
        let rest = rest.trim();
        if !crate::clock::recording() {
            eprintln!("shot: {step}");
        }
        match verb {
            "background" => {
                #[cfg(target_os="macos")]
                if let Some(mtm)=objc2::MainThreadMarker::new(){objc2_app_kit::NSApplication::sharedApplication(mtm).hide(None);}
                #[cfg(not(target_os="macos"))]
                self.window.set_minimized(true);
            }
            "input"=>{for c in rest.chars(){self.type_char(c);}},
            "downloadquery"=>assert_eq!(self.download_ui.query,rest),
            "downloadmatches"=>{let count=crate::downloads::list().iter().filter(|d|crate::downloads::matches(d,&self.download_ui.query)).count();assert_eq!(count,rest.parse::<usize>().unwrap());},
            "downloadbounds"=>{let full=nus_render::Rect::new(0.0,0.0,self.target.size.0 as f32,self.target.size.1 as f32);let area=self.download_ui.rect.unwrap_or(full);for(r,hit)in &self.download_ui.hits{assert!(r.x>=area.x&&r.y>=area.y&&r.right()<=area.right()+1.0&&r.bottom()<=area.bottom()+1.0,"download hit outside surface: {hit:?} {r:?}");}},
            "dockattention" => {let _=self.proxy.send_event(crate::UserEvent::DockAttention);}
            "wait" => {
                let ms: u64 = rest.parse().unwrap_or(500);
                if let Some(s) = self.shot.as_mut() {
                    s.until = Some(crate::clock::now() + Duration::from_millis(ms));
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
            "historycheck"=>{
                let tl=self.timeline.as_ref().expect("history open");assert!(!tl.snapshot);assert!(tl.count()>=3,"commands recorded: {}",tl.count());assert!(!tl.ranges.is_empty());assert!(tl.list_area.w>0.0&&tl.detail_area.w>0.0);assert!(tl.detail_max>0.0,"history should scroll");
                for (r,hit) in &tl.hits {assert!(r.x>=tl.area.x-1.0&&r.right()<=tl.area.right()+1.0&&r.y>=tl.area.y-1.0&&r.bottom()<=tl.area.bottom()+1.0,"history control outside pane: {hit:?} {r:?} {:?}",tl.area);}
                assert!(tl.records.iter().any(|v|v["output"].as_str().is_some_and(|o|o.contains("history-output"))),"recorded output missing");
            },
            "historysearch"=>{if let Some(tl)=&mut self.timeline{tl.query=rest.into();tl.reveal=true;}self.dirty=true;},
            "historyassertmatches"=>{let tl=self.timeline.as_ref().unwrap();assert_eq!(tl.matches().len(),rest.parse::<usize>().unwrap());},
            "historysnapshot"=>self.timeline_action(crate::replay::HistoryHit::Snapshot),
            "historymap"=>{
                let tl=self.timeline.as_ref().unwrap();let map=tl.list_area;let before=tl.detail_scroll;
                self.mouse_moved(map.x+map.w*0.5,map.y+map.h*0.1);self.mouse_button(MouseButton::Left,ElementState::Pressed);
                self.mouse_moved(map.x+map.w*0.5,map.y+map.h*0.8);self.mouse_button(MouseButton::Left,ElementState::Released);
                let tl=self.timeline.as_ref().unwrap();assert!(tl.map_drag.is_none());assert!(tl.detail_scroll>=0.0&&tl.detail_scroll<=tl.detail_max);assert!((tl.detail_scroll-before).abs()>1.0,"map did not move");
            },
            "historyexport"=>{let path=self.share_replay(self.active).unwrap();eprintln!("HISTORY_EXPORT {}",path.display());},
            "foreground"=>{
                #[cfg(target_os="macos")]
                if let Some(mtm)=objc2::MainThreadMarker::new(){objc2_app_kit::NSApplication::sharedApplication(mtm).unhide(None);}
                self.window.set_minimized(false);self.window.set_visible(true);self.window.focus_window();
            },
            "pickavatar"=>self.pick_avatar(),
            "assertpicker"=>assert_eq!(self.avatar_pick.is_some(),rest=="open"),
            "pipkeys"=>{
                use winit::keyboard::{Key,NamedKey,PhysicalKey,KeyCode};
                self.pip_focus(true);
                let named=match rest {"left"=>NamedKey::ArrowLeft,"right"=>NamedKey::ArrowRight,"tab"=>NamedKey::Tab,"space"=>NamedKey::Space,_=>panic!("unknown PiP key")};
                self.pip_key(&crate::app::KeyIn{physical_key:PhysicalKey::Code(KeyCode::ArrowLeft),logical_key:Key::Named(named),text:None,state:ElementState::Pressed,repeat:false});
            },
            "videostart"=>{if let Pane::Web(w)=&self.tabs[self.active].left {w.tab.eval("document.querySelector('video').pause(); document.querySelector('video').currentTime=30; __nus.report()");}},
            "assertvideotime"=>{let Pane::Web(w)=&self.tabs[self.active].left else{panic!("web expected")};let v=w.tab.video().expect("video");assert!((v.t-rest.parse::<f64>().unwrap()).abs()<0.2,"video time {}",v.t);},
            "pipskip"=>{self.behavior.pip_skip_seconds=rest.parse().unwrap();self.save_prefs();},
            "pipskipkeyboard"=>{
                let index=self.settings_hits.iter().position(|(_,h)|matches!(h,crate::settings::Hit::Slider(crate::settings::Slider::PipSkip,_,_))).expect("skip slider visible");
                self.settings_focus=Some(index);let before=self.behavior.pip_skip_seconds;
                let key=crate::app::KeyIn{physical_key:winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::ArrowRight),logical_key:winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowRight),text:None,state:ElementState::Pressed,repeat:false};
                assert!(self.settings_key(&key));assert_eq!(self.behavior.pip_skip_seconds,before+1);
                assert_eq!(crate::prefs::Prefs::load().behavior.unwrap().pip_skip_seconds,before+1);
            },
            "piptransportcheck"=>{
                self.pip_cursor_moved(100.0,100.0);self.pip_focus(true);
                let p=self.pip.as_mut().expect("PiP");p.last_frame=crate::clock::now()-std::time::Duration::from_millis(20);
                self.pip_frame();let p=self.pip.as_ref().unwrap();
                for hit in [crate::pip::Hit::Back,crate::pip::Hit::Forward] {assert!(p.hits.iter().any(|(_,h)|*h==hit),"missing skip control");}
                eprintln!("PIP_FIRST_PICTURE_MS {:?} RECT {:?} AREA {:?}",p.first_frame_ms,p.cur,p.area);assert!(p.first_frame_ms.is_some(),"no video frame presented");
            },
            "pipforeground"=>{self.pip.as_ref().unwrap().window.focus_window();},
            "pipretarget"=>{let p=self.pip.as_ref().unwrap();let (id,rect,tab,right)=(p.window.id(),p.cur,p.tab,p.right);assert!(self.retarget_pip(tab,right));let p=self.pip.as_ref().unwrap();assert_eq!(id,p.window.id());assert_eq!(rect,p.cur);},
            "uilabels"=>self.check_ui_labels(),
            "pipcheck"=>{
                let before=self.pip.as_ref().expect("PiP exists").cur;self.pip_wheel(winit::event::MouseScrollDelta::LineDelta(0.0,0.0));assert_eq!(self.pip.as_ref().unwrap().cur,before,"zero scroll changed PiP");
                self.pip_pinch(0.01);let after=self.pip.as_ref().unwrap().cur;assert!(after.w>before.w&&after.w<before.w*1.02,"small gesture must be proportional");
                self.pip_wheel(winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0,-2.0)));assert!(self.pip.as_ref().unwrap().cur.w<after.w);
                let p=self.pip.as_ref().unwrap();let a=p.area;assert!(p.cur.x>=a.x&&p.cur.y>=a.y&&p.cur.x+p.cur.w<=a.x+a.w+1.0&&p.cur.y+p.cur.h<=a.y+a.h+1.0);
                eprintln!("PIP_GEOMETRY {:?} AREA {:?}",p.cur,a);
            },
            "pipedge"=>{
                let p=self.pip.as_ref().unwrap();let scale=p.window.scale_factor();let w=p.target.size.0 as f64;let h=p.target.size.1 as f64;let before=p.cur;
                self.pip_cursor_moved(w-2.0*scale,h-2.0*scale);self.pip_mouse(MouseButton::Left,ElementState::Pressed);self.pip_cursor_moved(w+18.0*scale,h+8.0*scale);self.pip_mouse(MouseButton::Left,ElementState::Released);
                let after=self.pip.as_ref().unwrap().cur;assert!(after.w>before.w);assert!((after.w/after.h-before.w/before.h).abs()<0.02);assert!(!self.pip.as_ref().unwrap().pressed);
            },

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
            // The floating window for this tab's video, as the palette's
            // PICTURE IN PICTURE does it.
            "pip" => self.run(crate::app::Action::Pip),
            // The pointer inside the floating window, in its own pixels,
            // so the controls can be photographed with one lit.
            "pippoint" => {
                let mut n = rest.split_whitespace().filter_map(|v| v.parse::<f32>().ok());
                if let (Some(x), Some(y)) = (n.next(), n.next()) {
                    if let Some(pip) = self.pip.as_mut() {
                        pip.pos = (x, y);
                        pip.inside = true;
                        pip.left_at = None;
                    }
                }
            }
            // The floating window's own frame, as a PNG beside the others.
            "shotpip" => {
                let name = if rest.is_empty() { "pip" } else { rest };
                self.pip_frame();
                let Some(out) = self.shot.as_ref().map(|s| (s.out.clone(), s.face)) else { return };
                let clear = self.theme.paper;
                let Some(pip) = self.pip.as_ref() else {
                    eprintln!("shot: shotpip: no floating window");
                    return;
                };
                let (w, h) = pip.target.size;
                let rgba = self.gpu.snapshot((w, h), &self.pip.as_ref().unwrap().scene, clear);
                let _ = std::fs::create_dir_all(&out.0);
                let path = out.0.join(format!("{name}-{}.png", out.1));
                match std::fs::File::create(&path).map_err(|e| e.to_string()).and_then(|f| {
                    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.write_header().and_then(|mut wr| wr.write_image_data(&rgba)).map_err(|e| e.to_string())
                }) {
                    Ok(()) => eprintln!("shot: wrote {} ({w}×{h} px)", path.display()),
                    Err(e) => eprintln!("shot: {}: {e}", path.display()),
                }
            }
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
                assert_eq!(kind, rest, "focused pane at step `{step}`");
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
                    let script=self.shot.as_mut().unwrap();script.next-=1;script.until=Some(crate::clock::now()+Duration::from_millis(80));
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
            "asserthomeart" => {
                assert!(matches!(self.tabs[self.active].left, Pane::Home(_)), "not a Home prompt");
                assert_eq!(self.behavior.home_look, crate::settings::HomeLook::Art);
                assert_eq!(self.behavior.home_art, rest);
                let art = self.art.as_ref().expect("Home artwork rendered");
                assert_eq!(art.key, rest);
                assert!(art.status.is_none(), "artwork failed: {:?}", art.status);
                assert_eq!(art.backdrop != crate::art::Backdrop::Theme, rest == "sky");
                let saved = crate::prefs::Prefs::load().behavior.expect("saved background choice");
                assert_eq!(saved.home_art, rest);
                assert_eq!(saved.home_look, crate::settings::HomeLook::Art);
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
            // SYNC · THE PHONE: this window served on the network, and
            // its address, so a script can put it on a phone (or in a tab).
            "phone" => {
                match rest {
                    "off" => {
                        crate::phone::stop();
                        self.behavior.phone = false;
                    }
                    _ => {
                        self.behavior.phone = true;
                        self.phone_on();
                        self.behavior.phone = crate::phone::current().is_some();
                        match crate::phone::current() {
                            Some(p) => eprintln!("shot: phone at {}", p.url()),
                            None => eprintln!("shot: phone did not start"),
                        }
                    }
                }
            }
            // The phone's own page, opened here as a tab: the same page the
            // phone gets, so a capture of it is the real thing.
            "phonetab" => {
                match crate::phone::current() {
                    Some(p) => self.open_url(&p.url(), true),
                    None => eprintln!("shot: phone is not on"),
                }
            }
            // A chip on a diff's hunk: `hunk stage 0`, `hunk apply 1`.
            // The click goes to the chip's own rectangle, so it is the
            // chip that runs, exactly as a hand would make it.
            "hunk" => {
                let (what, nth) = rest.split_once(' ').unwrap_or((rest, "0"));
                let nth: usize = nth.trim().parse().unwrap_or(0);
                let want = match what.trim().to_lowercase().as_str() {
                    "revert" => crate::diffs::Do::Revert,
                    "unstage" => crate::diffs::Do::Unstage,
                    "apply" => crate::diffs::Do::Apply,
                    _ => crate::diffs::Do::Stage,
                };
                let hit = self.tabs.get(self.active).into_iter().flat_map(|t| std::iter::once(&t.left).chain(t.right.as_ref())).find_map(|p| match p {
                    Pane::Term(t) => t.hunk_hits.iter().filter(|(_, _, _, d)| *d == want).nth(nth).map(|(r, _, _, _)| *r),
                    _ => None,
                });
                match hit {
                    Some(r) => {
                        self.mouse_moved(r.x + r.w / 2.0, r.y + r.h / 2.0);
                        self.mouse_button(MouseButton::Left, ElementState::Pressed);
                        self.mouse_button(MouseButton::Left, ElementState::Released);
                    }
                    None => eprintln!("shot: hunk: no {} chip #{nth} on screen", want.word()),
                }
            }
            // The on-screen rectangles of named elements, in logical px,
            // read from the accessibility tree: one node per hit target, so
            // what the film draws an overlay on is exactly what nus drew.
            //   marks file.json band=ASK chips="ALLOW ON THIS HOST"
            //   marks file.json all
            "marks" => {
                let (file, want) = rest.split_once(' ').unwrap_or((rest, "all"));
                let tree = self.access_tree();
                // The tree holds physical pixels; the film lays its overlays
                // out in the window's own logical ones.
                let sc = self.scale as f64;
                let labelled: Vec<(String, [f64; 4])> = tree
                    .nodes
                    .iter()
                    .filter_map(|(_, n)| {
                        let r = n.bounds()?;
                        let label = n.label().unwrap_or_default();
                        (!label.is_empty()).then(|| (label.to_string(), [r.x0 / sc, r.y0 / sc, (r.x1 - r.x0) / sc, (r.y1 - r.y0) / sc]))
                    })
                    .collect();
                let mut out = serde_json::Map::new();
                if want.trim() == "all" {
                    for (label, r) in &labelled {
                        out.insert(label.clone(), serde_json::json!(r));
                    }
                } else {
                    for (key, text) in marks_wanted(want) {
                        let hit = labelled.iter().find(|(l, _)| l.to_lowercase().contains(&text.to_lowercase()));
                        match hit {
                            Some((_, r)) => {
                                out.insert(key, serde_json::json!(r));
                            }
                            None => eprintln!("shot: marks: nothing labelled `{text}`"),
                        }
                    }
                }
                let path = self.shot.as_ref().map(|s| s.out.join(file)).unwrap_or_else(|| PathBuf::from(file));
                if let Some(d) = path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                match std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap_or_default()) {
                    Ok(()) => eprintln!("shot: wrote {} ({} marks)", path.display(), out.len()),
                    Err(e) => eprintln!("shot: {}: {e}", path.display()),
                }
            }
            // The frame in planes, for the shot that explodes the window:
            // the whole composite, the chrome with the panes cut out, and
            // each pane's own rectangle alone. One frame, masked four ways
            // (not four renders), so they line up to the pixel.
            "layers" => {
                let dir = self.shot.as_ref().map(|s| s.out.join(rest)).unwrap_or_else(|| PathBuf::from(rest));
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    eprintln!("shot: {}: {e}", dir.display());
                    return;
                }
                let (w, h) = self.target.size;
                let sc = self.scale;
                let px = |r: nus_render::Rect| [(r.x * sc) as i64, (r.y * sc) as i64, ((r.x + r.w) * sc) as i64, ((r.y + r.h) * sc) as i64];
                let tab = self.tabs.get(self.active);
                let term = tab.and_then(|t| std::iter::once(&t.left).chain(t.right.as_ref()).find(|p| matches!(p, Pane::Term(_))).map(|p| p.rect()));
                let web = tab.and_then(|t| std::iter::once(&t.left).chain(t.right.as_ref()).find_map(|p| match p {
                    Pane::Web(w) => Some(w.page),
                    _ => None,
                }));
                let header = self.strip_rect();
                let sidebar = self.sidebar_rect();
                let clear = self.shot_clear();
                let rgba = self.gpu.snapshot((w, h), &self.scene, clear);
                // Everything outside `keep` (or inside `cut`) goes clear.
                let mask = |keep: Option<[i64; 4]>, cut: &[[i64; 4]]| -> Vec<u8> {
                    let mut out = rgba.clone();
                    for y in 0..h as i64 {
                        for x in 0..w as i64 {
                            let inside = |r: &[i64; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
                            let drop = keep.as_ref().is_some_and(|k| !inside(k)) || cut.iter().any(inside);
                            if drop {
                                let i = ((y * w as i64 + x) * 4) as usize;
                                out[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                            }
                        }
                    }
                    out
                };
                let panes: Vec<[i64; 4]> = term.iter().chain(web.iter()).map(|r| px(*r)).collect();
                let mut wrote = Vec::new();
                for (name, data) in [
                    ("compositor.png", rgba.clone()),
                    ("native-ui.png", mask(None, &panes)),
                    ("terminal.png", term.map(|r| mask(Some(px(r)), &[])).unwrap_or_default()),
                    ("chromium.png", web.map(|r| mask(Some(px(r)), &[])).unwrap_or_default()),
                ] {
                    if data.is_empty() {
                        continue;
                    }
                    match write_png(&dir.join(name), w, h, &data) {
                        Ok(()) => wrote.push(name),
                        Err(e) => eprintln!("shot: {name}: {e}"),
                    }
                }
                let marks = serde_json::json!({
                    "header": px(header),
                    "sidebar": px(sidebar),
                    "terminal": term.map(px),
                    "chromium": web.map(px),
                    "scale": sc,
                    "size": [w, h],
                });
                let _ = std::fs::write(dir.join("marks.json"), serde_json::to_string_pretty(&marks).unwrap_or_default());
                eprintln!("shot: wrote {} ({})", dir.display(), wrote.join(", "));
            }
            // The glyph atlas as it sits on the GPU, as a grey PNG.
            "atlas_png" => {
                let (size, cov) = self.gpu.atlas_snapshot();
                let rgba: Vec<u8> = cov.iter().flat_map(|c| [255, 255, 255, *c]).collect();
                let path = self.shot.as_ref().map(|s| s.out.join(rest)).unwrap_or_else(|| PathBuf::from(rest));
                if let Some(d) = path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                match write_png(&path, size, size, &rgba) {
                    Ok(()) => eprintln!("shot: wrote {} ({size}×{size})", path.display()),
                    Err(e) => eprintln!("shot: {}: {e}", path.display()),
                }
            }
            // Hold the script until the picture stops moving, so a take
            // starts from the same still frame every time. Animations that
            // began while the app was coming up are over by then.
            "settle" => {
                let ms: u64 = rest.parse().unwrap_or(6000);
                if let Some(s) = self.shot.as_mut() {
                    s.settle = Some((0, 0, Instant::now() + Duration::from_millis(ms)));
                }
            }
            // The window at an exact logical size, so every capture of
            // every shot is the same shape (the standard's 1600×1000).
            "window" => {
                let (w, h) = rest.split_once(' ').unwrap_or(("1600", "1000"));
                let (w, h) = (w.trim().parse::<f64>().unwrap_or(1600.0), h.trim().parse::<f64>().unwrap_or(1000.0));
                if self.window.fullscreen().is_some() {
                    self.window.set_fullscreen(None);
                }
                // macOS un-zooms with an animation that restores the window's
                // own frame, so the size is asked for again after it, as a
                // step of its own.
                self.window.set_maximized(false);
                if let Some(s) = self.shot.as_mut() {
                    s.steps.insert(s.next, format!("windowsize {w} {h}"));
                    s.until = Some(crate::clock::now() + Duration::from_millis(500));
                }
            }
            "windowsize" => {
                let (w, h) = rest.split_once(' ').unwrap_or(("1600", "1000"));
                let (w, h) = (w.trim().parse::<f64>().unwrap_or(1600.0), h.trim().parse::<f64>().unwrap_or(1000.0));
                let _ = self.window.request_inner_size(winit::dpi::LogicalSize::new(w, h));
                if let Some(s) = self.shot.as_mut() {
                    s.until = Some(crate::clock::now() + Duration::from_millis(500));
                }
            }
            // A recording: from here the clock is the recorder's, one frame
            // of 1/60 s per PNG, and the `at` lines that follow are its
            // schedule. The script carries on when the clip is finished.
            "record" => {
                let (name, secs) = rest.split_once(' ').unwrap_or((rest, "3"));
                let secs: f64 = secs.trim().parse().unwrap_or(3.0);
                let name = name.trim().to_string();
                let (dir, at) = {
                    let Some(s) = self.shot.as_mut() else { return };
                    let dir = s.out.join(&name);
                    // Take the `at` lines that follow as this clip's schedule.
                    let mut at: Vec<(f64, String)> = Vec::new();
                    while let Some(line) = s.steps.get(s.next) {
                        let Some(tail) = line.strip_prefix("at ") else { break };
                        s.next += 1;
                        let (t, step) = tail.trim().split_once(' ').unwrap_or((tail.trim(), ""));
                        at.push((t.parse().unwrap_or(0.0), step.trim().to_string()));
                    }
                    // Soonest last: the pump pops off the end.
                    at.sort_by(|a, b| b.0.total_cmp(&a.0));
                    (dir, at)
                };
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    eprintln!("shot: {}: {e}", dir.display());
                    return;
                }
                let frames = (secs * 60.0).round().max(1.0) as u64;
                eprintln!("shot: recording {name}: {frames} frames ({secs:.3}s) → {}", dir.display());
                crate::clock::start();
                // Everything that free-runs from when the app came up — the
                // caret's blink, a texture's drift, an art's own time — is
                // rephased to the clip's first frame, or a take would carry
                // whatever phase the launch happened to leave it in.
                self.started = crate::clock::now();
                if let Some(s) = self.shot.as_mut() {
                    s.rec = Some(Rec { name, dir, frames, frame: 0, at, typing: None, hold: None, began: std::time::Instant::now() });
                }
            }
            // Type into whatever has the focus — a shell, the editor, the
            // palette, the prompt — a character at a time, on the clock.
            "type" => {
                let (text, cps) = match rest.rsplit_once(' ') {
                    Some((t, n)) if n.parse::<f64>().is_ok() => (t, n.parse::<f64>().unwrap_or(10.0)),
                    _ => (rest, 10.0),
                };
                let gap = 1.0 / cps.max(0.1);
                let chars: std::collections::VecDeque<char> = text.chars().collect();
                let at = self.rec_time();
                if let Some(r) = self.shot.as_mut().and_then(|s| s.rec.as_mut()) {
                    r.typing = Some((chars, gap, at));
                } else {
                    self.shot_type(text);
                }
            }
            // A chord, through the app's own key handling.
            "key" => self.shot_key(rest),
            // The clock stops until the page paints, or the language
            // server answers. No frame is written while it waits.
            "await-paint" => {
                let n = self.page_paints();
                if let Some(r) = self.shot.as_mut().and_then(|s| s.rec.as_mut()) {
                    r.hold = Some(Hold::Paint(n));
                    r.began = std::time::Instant::now();
                }
            }
            "await-lsp" => {
                if let Some(r) = self.shot.as_mut().and_then(|s| s.rec.as_mut()) {
                    r.hold = Some(Hold::Lsp);
                    r.began = std::time::Instant::now();
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

    /// The colour behind the frame, as the draw computes it.
    fn shot_clear(&self) -> [f32; 4] {
        let p = self.paper();
        [p[0], p[1], p[2], 1.0]
    }

    /// Six frames the same and the picture has settled. The frames are
    /// hashed off the GPU, not guessed at from `dirty`: an animation that
    /// has stopped asking for frames may still have one in flight.
    fn settle_tick(&mut self) {
        let (last, same, deadline) = match self.shot.as_ref().and_then(|s| s.settle) {
            Some(v) => v,
            None => return,
        };
        self.dirty = true;
        let (w, h) = self.target.size;
        let rgba = self.gpu.snapshot((w, h), &self.scene, [0.0; 4]);
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in rgba.iter().step_by(7) {
            hash = (hash ^ *b as u64).wrapping_mul(0x100_0000_01b3);
        }
        let same = if hash == last { same + 1 } else { 0 };
        let over = Instant::now() >= deadline;
        if same >= 6 || over {
            if over {
                eprintln!("shot: settle: still moving after the wait — carrying on");
            }
            if let Some(s) = self.shot.as_mut() {
                s.settle = None;
            }
            return;
        }
        if let Some(s) = self.shot.as_mut() {
            s.settle = Some((hash, same, deadline));
        }
    }

    /// The clip time of the frame about to be written.
    fn rec_time(&self) -> f64 {
        self.shot.as_ref().and_then(|s| s.rec.as_ref()).map_or(0.0, |r| r.frame as f64 / 60.0)
    }

    /// How many times the focused page has painted: `await-paint` waits on
    /// this, because Chromium runs on its own clock.
    fn page_paints(&self) -> u64 {
        let tab = self.tabs.get(self.active);
        let pane = tab.and_then(|t| if t.focus_right { t.right.as_ref() } else { Some(&t.left) });
        match pane.or_else(|| tab.map(|t| &t.left)) {
            Some(Pane::Web(w)) => w.tab.shared.borrow().paints,
            _ => self.tabs.iter().filter_map(|t| match &t.left {
                Pane::Web(w) => Some(w.tab.shared.borrow().paints),
                _ => None,
            }).max().unwrap_or(0),
        }
    }

    /// Whether a language server's answer is on screen: the prompt's menu
    /// with rows, or the editor showing a completion, hover or diagnostic.
    fn lsp_answered(&self) -> bool {
        self.tabs.iter().any(|t| {
            std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| match p {
                // The prompt's menu, or a diagnostic on the line.
                Pane::Term(tp) => tp.plsp.as_ref().is_some_and(|l| l.menu || !l.items.is_empty() || !l.diags.is_empty() || l.ghost.is_some()),
                // The editor's hover card, or its diagnostics.
                Pane::Editor(e) => e.hover.is_some() || e.completion.is_some() || e.buffers.get(e.active).is_some_and(|b| !b.diags.is_empty()),
                _ => false,
            })
        })
    }

    /// One frame of a recording: the steps that are due, the next
    /// character of anything being typed, then the frame itself.
    fn record_frame(&mut self) {
        // Held? Nothing moves — not the clock, not the count.
        let held = self.shot.as_ref().and_then(|s| s.rec.as_ref()).and_then(|r| r.hold.as_ref().map(|h| (match h { Hold::Paint(n) => *n, Hold::Lsp => 0 }, matches!(h, Hold::Paint(_)), r.began)));
        if let Some((mark, is_paint, began)) = held {
            // Real time: the clock is stopped while held, so it can never fire.
            let over = began.elapsed() >= FUSE;
            let done = if is_paint { self.page_paints() > mark } else { self.lsp_answered() };
            if !done && !over {
                self.dirty = true;
                return;
            }
            if over {
                eprintln!("shot: {} never came ({}s) — carrying on", if is_paint { "the paint" } else { "the language server" }, FUSE.as_secs());
            }
            if let Some(r) = self.shot.as_mut().and_then(|s| s.rec.as_mut()) {
                r.hold = None;
            }
        }
        let t = self.rec_time();
        // Steps due at or before this frame.
        loop {
            let due = match self.shot.as_ref().and_then(|s| s.rec.as_ref()).and_then(|r| r.at.last()) {
                Some((at, _)) if *at <= t + 1e-9 => true,
                _ => false,
            };
            if !due {
                break;
            }
            let Some(step) = self.shot.as_mut().and_then(|s| s.rec.as_mut()).and_then(|r| r.at.pop()).map(|(_, st)| st) else { break };
            eprintln!("shot: {t:7.3} {step}");
            self.shot_step(&step);
            // A step may have asked the clock to wait; the rest are still due.
            if self.shot.as_ref().and_then(|s| s.rec.as_ref()).is_some_and(|r| r.hold.is_some()) {
                self.dirty = true;
                return;
            }
        }
        // The next character of anything being typed.
        let ch = {
            let r = self.shot.as_mut().and_then(|s| s.rec.as_mut());
            match r.and_then(|r| r.typing.as_mut()) {
                Some((chars, gap, next)) if *next <= t + 1e-9 => {
                    let c = chars.pop_front();
                    *next = t + *gap;
                    c
                }
                _ => None,
            }
        };
        if let Some(c) = ch {
            self.type_char(c);
            if self.shot.as_ref().and_then(|s| s.rec.as_ref()).is_some_and(|r| r.typing.as_ref().is_some_and(|(q, _, _)| q.is_empty())) {
                if let Some(r) = self.shot.as_mut().and_then(|s| s.rec.as_mut()) {
                    r.typing = None;
                }
            }
        }
        // The frame: name it, let the draw write it, and move the clock on.
        let finished = {
            let Some(r) = self.shot.as_mut().and_then(|s| s.rec.as_mut()) else { return };
            let name = format!("{}/f{:05}.png", r.name, r.frame);
            r.frame += 1;
            let finished = r.frame >= r.frames;
            let n = r.frame;
            let total = r.frames;
            if n % 60 == 0 || finished {
                eprintln!("shot: {}/{total} frames", n);
            }
            (name, finished)
        };
        let (name, finished) = finished;
        if let Some(s) = self.shot.as_mut() {
            s.pending = Some((name, None));
        }
        crate::clock::tick();
        self.dirty = true;
        if finished {
            let (name, frames, dir) = {
                let r = self.shot.as_ref().and_then(|s| s.rec.as_ref());
                match r {
                    Some(r) => (r.name.clone(), r.frames, r.dir.clone()),
                    None => return,
                }
            };
            eprintln!("shot: {name} done — {frames} frames in {}", dir.display());
            crate::clock::stop();
            if let Some(s) = self.shot.as_mut() {
                s.rec = None;
            }
        }
    }

    /// One character, as a key press and release through the app's own
    /// handling — so it goes wherever the focus is.
    fn type_char(&mut self, c: char) {
        use winit::keyboard::{Key as WKey, NativeKeyCode, PhysicalKey, SmolStr};
        let text = SmolStr::new(c.to_string());
        let mut k = crate::app::KeyIn {
            physical_key: key_code(c).map_or(PhysicalKey::Unidentified(NativeKeyCode::Unidentified), PhysicalKey::Code),
            logical_key: if c == ' ' { WKey::Named(winit::keyboard::NamedKey::Space) } else { WKey::Character(text.clone()) },
            text: Some(text),
            state: ElementState::Pressed,
            repeat: false,
        };
        self.key_in(&k);
        k.state = ElementState::Released;
        self.key_in(&k);
    }

    /// `key cmd+s`, `key down`, `key enter` — the chord, through the app's
    /// own key handling, with the modifiers held for exactly that press.
    fn shot_key(&mut self, chord: &str) {
        use winit::keyboard::{Key as WKey, KeyCode, ModifiersState, NamedKey, PhysicalKey, SmolStr};
        let mut mods = ModifiersState::empty();
        let mut name = chord.trim();
        while let Some((m, rest)) = name.split_once('+') {
            match m.trim().to_lowercase().as_str() {
                "cmd" | "super" | "win" => mods |= ModifiersState::SUPER,
                "ctrl" | "control" => mods |= ModifiersState::CONTROL,
                "shift" => mods |= ModifiersState::SHIFT,
                "alt" | "opt" | "option" => mods |= ModifiersState::ALT,
                other => eprintln!("shot: key: no modifier `{other}`"),
            }
            name = rest.trim();
        }
        let named = |n: NamedKey, c: KeyCode| Some((WKey::Named(n), c));
        let parts = match name.to_lowercase().as_str() {
            "enter" | "return" => named(NamedKey::Enter, KeyCode::Enter),
            "tab" => named(NamedKey::Tab, KeyCode::Tab),
            "esc" | "escape" => named(NamedKey::Escape, KeyCode::Escape),
            "space" => named(NamedKey::Space, KeyCode::Space),
            "backspace" => named(NamedKey::Backspace, KeyCode::Backspace),
            "delete" => named(NamedKey::Delete, KeyCode::Delete),
            "up" => named(NamedKey::ArrowUp, KeyCode::ArrowUp),
            "down" => named(NamedKey::ArrowDown, KeyCode::ArrowDown),
            "left" => named(NamedKey::ArrowLeft, KeyCode::ArrowLeft),
            "right" => named(NamedKey::ArrowRight, KeyCode::ArrowRight),
            "home" => named(NamedKey::Home, KeyCode::Home),
            "end" => named(NamedKey::End, KeyCode::End),
            "pageup" => named(NamedKey::PageUp, KeyCode::PageUp),
            "pagedown" => named(NamedKey::PageDown, KeyCode::PageDown),
            "f12" => named(NamedKey::F12, KeyCode::F12),
            one if one.chars().count() == 1 => {
                let c = one.chars().next().unwrap_or('a');
                key_code(c).map(|code| (WKey::Character(SmolStr::new(c.to_string())), code))
            }
            other => {
                eprintln!("shot: key: no key `{other}`");
                None
            }
        };
        let Some((logical, code)) = parts else { return };
        // Typed text only when nothing is held: ⌘S is a chord, not an "s".
        let text = match (&logical, mods.is_empty()) {
            (WKey::Character(c), true) => Some(c.clone()),
            (WKey::Named(NamedKey::Space), true) => Some(SmolStr::new(" ")),
            _ => None,
        };
        let was = self.mods;
        self.mods = mods;
        let mut k = crate::app::KeyIn { physical_key: PhysicalKey::Code(code), logical_key: logical, text, state: ElementState::Pressed, repeat: false };
        self.key_in(&k);
        k.state = ElementState::Released;
        self.key_in(&k);
        self.mods = was;
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
        // A recording names its own frames; a still is named for its face.
        let seq = name.ends_with(".png");
        let path = if seq { s.out.join(&name) } else { s.out.join(format!("{name}-{}.png", s.face)) };
        match self.snapshot_png(clear, crop_px, &path) {
            // Frames are counted, not announced: a clip is hundreds of them.
            Ok((cw, ch)) => {
                if !seq {
                    eprintln!("shot: wrote {} ({cw}×{ch} px at {}×)", path.display(), self.scale);
                }
            }
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

/// The key a character sits on, for a scripted press. Only what a script
/// types needs to be here; anything else goes as text alone.
fn key_code(c: char) -> Option<winit::keyboard::KeyCode> {
    use winit::keyboard::KeyCode::*;
    Some(match c.to_ascii_lowercase() {
        'a' => KeyA, 'b' => KeyB, 'c' => KeyC, 'd' => KeyD, 'e' => KeyE, 'f' => KeyF,
        'g' => KeyG, 'h' => KeyH, 'i' => KeyI, 'j' => KeyJ, 'k' => KeyK, 'l' => KeyL,
        'm' => KeyM, 'n' => KeyN, 'o' => KeyO, 'p' => KeyP, 'q' => KeyQ, 'r' => KeyR,
        's' => KeyS, 't' => KeyT, 'u' => KeyU, 'v' => KeyV, 'w' => KeyW, 'x' => KeyX,
        'y' => KeyY, 'z' => KeyZ,
        '0' => Digit0, '1' => Digit1, '2' => Digit2, '3' => Digit3, '4' => Digit4,
        '5' => Digit5, '6' => Digit6, '7' => Digit7, '8' => Digit8, '9' => Digit9,
        ' ' => Space, '-' => Minus, '=' => Equal, '.' => Period, ',' => Comma,
        '/' => Slash, ';' => Semicolon, '\'' => Quote, '`' => Backquote,
        '[' => BracketLeft, ']' => BracketRight, '\\' => Backslash,
        _ => return None,
    })
}

/// `band=ASK chips="ALLOW ON THIS HOST"` as pairs.
fn marks_wanted(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let tail = rest[eq + 1..].trim_start();
        let (text, next) = if let Some(t) = tail.strip_prefix('"') {
            match t.find('"') {
                Some(end) => (&t[..end], &t[end + 1..]),
                None => (t, ""),
            }
        } else {
            match tail.find(' ') {
                Some(sp) => (&tail[..sp], &tail[sp..]),
                None => (tail, ""),
            }
        };
        out.push((key, text.to_string()));
        rest = next.trim_start();
    }
    out
}

/// RGBA to a PNG file.
fn write_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut wr| wr.write_image_data(rgba)).map_err(|e| e.to_string())
}
