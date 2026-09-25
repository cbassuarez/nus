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
//!   toastfixture icon|Words|detail|act   a toast, drawn for real (icon "problem" for one of those)
//!   toastpress hover|chip|cell   the pointer on its chip, a press on the chip, or on its cell
//!   asserttoast <text> | asserttoastgone   what the toast says, or that none is up
//!   lsplog                     each language server's key and log lines, to stderr
//!   awaitload <ms>             hold until the focused page stops loading (or ms pass); report it
//!   cmdsel <from> <to> | assertcmd <text>   select characters of the command being typed; check the command line
//!   link allow | deny          answer the link band on the focused shell
//!   newtab                     open the configured start page in a new tab
//!   startpage prompt|home|last|layout [url or layout name]   set the start page
//!   assertpane home|web|term|settings   assert the focused pane kind
//!   eval <js> · assertreply <text>   run js on the focused page; after a wait, check its answer
//!   asserttabs <count>          assert the number of tabs
//!   newwindowlook prompt|shell|launch   set new-window behavior
//!   newwindow                  a second window
//!
//!   awaitperf metric_name     wait for an NUS_PERF sample; external timeout required
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
    /// The last `eval`, for `assertreply`.
    reply: Option<i32>,
    bench_started: Option<(String, Instant, u64, Option<(std::path::PathBuf, u64)>)>,
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
        Some(Shot { steps, next: 0, until: None, reply: None, bench_started: None, out, face, pending: None, done: false, rec: None, settle: None })
    }
}

impl App {
    /// Semantic waits for real-clock workflow runs. No sleeps or fixture ports
    /// are substituted for app behavior; the parent harness owns the timeout.
    fn benchmark_ready(&mut self, step: &str) -> bool {
        let (verb, rest) = step.split_once(' ').unwrap_or((step, ""));
        match verb {
            "awaitbundle"=>{assert!(!self.jobs.failed.contains_key(rest),"tool failed: {:?}",self.jobs.failed.get(rest));!self.jobs.running.iter().any(|id|id==rest)},
            "awaitfile" => std::path::Path::new(rest).is_file(),
            "awaitpage" => {
                let Some(Pane::Web(w)) = self.tabs.get(self.active).map(|t| t.focused_ref()) else { return false };
                let s = w.tab.shared.borrow();
                s.title == rest && s.paints > 0
            },
            "awaitreply" => {
                let id = self.shot.as_ref().and_then(|s| s.reply).expect("eval first");
                let Some(Pane::Web(w)) = self.tabs.get(self.active).map(|t| t.focused_ref()) else { panic!("awaitreply needs page") };
                let Some(v) = w.tab.take_reply(id) else { return false };
                let text = v.pointer("/result/value").map(|x| match x { serde_json::Value::String(s) => s.clone(), other => other.to_string() }).unwrap_or_else(|| v.to_string());
                assert_eq!(text, rest, "workflow page oracle failed");
                true
            },
            "awaitportowner" => {
                let mut parts = rest.split_whitespace();
                let port: u16 = parts.next().unwrap().parse().unwrap();
                let index: usize = parts.next().unwrap().parse().unwrap();
                let pid_path = parts.next().unwrap();
                let Ok(text) = std::fs::read_to_string(pid_path) else { return false };
                let pid: u32 = text.trim().parse().expect("fixture pid");
                let id = self.tabs.get(index).expect("owner tab").id;
                self.board.rows.iter().any(|r| r.port == port && r.pid == pid && r.tab == Some(id))
            },
            _ => true,
        }
    }

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
        // Wait for a semantic event without blocking the UI or clearing startup
        // samples. The external native harness supplies the hard timeout.
        if let Some(metric) = step.strip_prefix("awaitperf ") {
            assert!(crate::perf::enabled(), "awaitperf requires NUS_PERF=1");
            if !crate::perf::has_samples(metric.trim()) {
                self.dirty = true;
                return;
            }
        }
        if !self.benchmark_ready(&step) {
            self.dirty = true;
            return;
        }
        self.shot.as_mut().unwrap().next += 1;
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
            "awaitbundle" | "awaitfile" | "awaitpage" | "awaitreply" | "awaitportowner" => {},
            "benchbegin" => {
                assert!(crate::perf::enabled() && !crate::clock::recording(), "benchmark requires real-clock NUS_PERF");
                let s = self.shot.as_mut().unwrap();
                assert!(s.bench_started.is_none(), "nested benchmark");
                let (name, load) = rest.split_once(' ').map_or((rest, None), |(name, path)| {
                    let path = std::path::PathBuf::from(path);
                    let count = std::fs::read_to_string(&path).expect("load counter").trim().parse::<u64>().expect("load count");
                    (name, Some((path, count)))
                });
                s.bench_started = Some((name.into(), Instant::now(), self.frames, load));
            },
            "benchend" => {
                let (name, began, frames, load) = self.shot.as_mut().unwrap().bench_started.take().expect("benchbegin first");
                assert_eq!(name, rest);
                assert!(self.frames > frames, "no application frame during workflow");
                if let Some((path, before)) = load {
                    let after = std::fs::read_to_string(path).expect("load counter").trim().parse::<u64>().expect("load count");
                    assert!(after > before, "background output did not advance during workflow");
                    eprintln!("WORKFLOW_LOAD: {before} -> {after}");
                }
                let metric = match rest {
                    "edit-verify" => "workflow_edit_verify_v1",
                    "port-conflict" => "workflow_port_conflict_v1",
                    "project-switch" => "workflow_project_switch_v1",
                    _ => panic!("unknown benchmark"),
                };
                crate::perf::record(metric, began.elapsed().as_secs_f64() * 1000.0);
            },
            "benchtab" => self.run(crate::app::Action::SwitchTab(rest.parse().expect("tab index"))),
            "benchportjump" => {
                let port: u16 = rest.parse().unwrap();
                let key = self.board.rows.iter().find(|r| r.port == port && r.pid != 0 && r.tab.is_some()).expect("owned port").key.clone();
                self.ports_act(&key, crate::ports::Act::Jump);
            },
            "asserteditorpath" => {
                let b = self.focused_editor().and_then(|e| e.buf()).expect("editor buffer");
                assert_eq!(b.path.as_deref(), Some(std::path::Path::new(rest)));
            },
            "privatecheck" => {
                assert!(crate::private::enabled());
                assert!(!self.behavior.remember && self.recorder.is_none() && self.hotkey.is_none());
                assert!(self.recent.is_empty() && self.last_session.is_none());
                assert_eq!(self.window_name(), "Incognito");
                assert!(self.new_term_pane_at(false, 0, None).is_err());
                assert!(self.remote("ls", &serde_json::Value::Null).is_err());
                let mut p=crate::sites::prefs("private-canary.invalid");p.zoom=125;
                crate::sites::set("private-canary.invalid", p);
                crate::sites::remember("https://private-canary.invalid", "camera", true);
                self.save_session(); self.save_prefs();
                let profile=std::env::current_dir().unwrap().join("profile");
                assert!(!crate::private::downloads_dir().unwrap().starts_with(profile.parent().unwrap()), "download destination must survive private cleanup");
                for file in ["session.json","recent.json","downloads.json","sites.json","permissions.json","instance","phone"] {assert!(!profile.join(file).exists(),"private persistence: {file}");}
                let mut dirs=vec![profile.clone()];
                while let Some(dir)=dirs.pop() {
                    for entry in std::fs::read_dir(dir).unwrap() {
                        let entry=entry.unwrap();let kind=entry.file_type().unwrap();
                        if kind.is_dir() {dirs.push(entry.path());}
                        else if kind.is_file() {
                            let bytes=std::fs::read(entry.path()).unwrap();
                            assert!(!bytes.windows(b"private-canary".len()).any(|s| s==b"private-canary"), "private browser data was written to disk: {:?}", entry.path());
                        }
                    }
                }
                let out=std::path::PathBuf::from(std::env::var_os("NUS_SHOT_DIR").expect("isolated native check"));
                std::fs::write(out.join("private-root.txt"),profile.parent().unwrap().display().to_string()).unwrap();
            }
            "assertdevtools" => {
                let Some(Pane::Web(w))=self.tabs.get(self.active).map(|t|t.focused_ref()) else {panic!("page expected")};
                assert_eq!(w.tab.has_devtools(), rest=="open");
            }
            "permissioncheck" => {
                assert_eq!(crate::sites::remembered(rest, "camera"), None, "legacy host grant was inherited");
                crate::sites::remember(rest, "camera", true);
                assert_eq!(crate::sites::remembered(rest, "camera"), Some(true));
                let mut other=url::Url::parse(rest).unwrap();
                let port=other.port_or_known_default().unwrap();
                other.set_port(Some(if port==65535 {65534} else {port+1})).unwrap();
                assert_eq!(crate::sites::remembered(other.as_str(), "camera"), None);
                if let Some(Pane::Web(w))=self.tabs.get_mut(self.active).map(|t|t.focused()) {w.site_panel=true;}
                self.dirty=true;
            }
            "assertwindows" => assert_eq!(self.windows.len(), rest.parse::<usize>().unwrap(), "native window count"),
            "keychaincheck" => {
                use cef::ImplCommandLine;
                assert!(crate::browser_runtime::ensure());
                let cl=cef::command_line_get_global().unwrap();
                let mock=cl.has_switch(Some(&"use-mock-keychain".into()))!=0;
                assert_eq!(mock, cfg!(target_os="macos") && std::env::var_os("NUS_TEST_REAL_KEYCHAIN").is_none(), "Keychain mode");
                assert_eq!(cl.has_switch(Some(&"remote-debugging-port".into())), 0, "unexpected debugging listener");
            }
            "supportcheck" => {
                use crate::application_menu::{Command, ITEMS};
                for command in [Command::ReportBug,Command::RequestFeature,Command::NewPrivateWindow] {assert!(ITEMS.iter().any(|i|i.command==command));}
                for kind in [crate::support::Kind::Bug,crate::support::Kind::Feature] {
                    assert!(crate::support::issue_url(kind).starts_with("https://github.com/cbassuarez/nus/issues/new?"));
                }
                self.open_settings_at(14,None); self.redraw();
                for kind in [crate::support::Kind::Bug,crate::support::Kind::Feature] {assert!(self.settings_hits.iter().any(|(_,h)|*h==crate::settings::Hit::Report(kind)),"report button missing");}
            }
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
            // Finish Work: click the coffee; what the header shows; and what
            // the OS itself lists (macOS: pmset -g assertions).
            "finishwork" => self.crumb_action(crate::app::CrumbHit::FinishWork),
            "assertfinish" => {
                use crate::finish_work::Phase;
                let phase = crate::finish_work::view().phase;
                let got = match phase { Phase::Unavailable => "unavailable".to_string(), Phase::Ready(n) => format!("ready {n}"), Phase::Holding(n) => format!("holding {n}"), Phase::SafetyReleased(_) => "released".to_string() };
                assert_eq!(got, rest.trim(), "finish work phase");
                eprintln!("FINISH WORK CHECK PASSED {got}");
            }
            "assertfinishshown" => {
                let shown = self.crumb_hits.iter().any(|(_, h)| *h == crate::app::CrumbHit::FinishWork);
                assert_eq!(shown, rest.trim() == "yes", "finish work control shown");
            }
            "assertwake" => {
                let out = std::process::Command::new("pmset").args(["-g", "assertions"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
                let held = out.contains("nus is finishing");
                assert_eq!(held, rest.trim() == "on", "native wake assertion; pmset said:\n{out}");
                eprintln!("WAKE CHECK PASSED {}", rest.trim());
            }
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
            "portactionscheck" => {
                assert!(std::env::var_os("NUS_PORTS_FIXTURE").is_some());
                let profile = self.behavior.default_profile;
                let left = self.new_term_pane(false, profile).expect("fixture left terminal");
                let right = self.new_term_pane(true, profile).expect("fixture right terminal");
                let pids = (left.pty.pid(), right.pty.pid());
                let tab = self.make_tab(Pane::Term(left), Some(Pane::Term(right)));
                let owner = self.tabs.len();
                let id = tab.id;
                self.tabs.push(tab);
                let command = "printf nus-port-regression";
                let mut row = crate::ports::Remembered {port:3000,process:"fixture".into(),command:command.into(),cwd:std::env::current_dir().unwrap().display().to_string(),last_seen:0}.row();
                row.group = crate::ports::Group::Mine;
                row.tab = Some(id);
                let key = row.key.clone();
                self.board.rows = vec![row];
                let count = self.tabs.len();
                self.ports_act(&key, crate::ports::Act::Again);
                assert_eq!(self.tabs.len(), count+1, "rerun must use a fresh terminal");
                let Pane::Term(t) = &mut self.tabs[self.active].left else {panic!("new command terminal")};
                assert_eq!(t.type_at_prompt.take(), Some(format!("{command}\r")));
                self.ports_act(&key, crate::ports::Act::Tunnel);
                assert_eq!(self.tabs.len(), count+2, "occupied split must create a tunnel tab");
                let tunnel_id = self.tabs[self.active].id;
                let Pane::Term(t) = &mut self.tabs[self.active].left else {panic!("new tunnel terminal")};
                // Inspect and clear before returning to the event loop. This
                // regression fixture never starts an actual public tunnel.
                assert!(t.type_at_prompt.take().is_some());
                assert_eq!(self.board.rows[0].tunnel.as_ref().unwrap().tab, tunnel_id);
                let tab = &self.tabs[owner];
                let Pane::Term(left) = &tab.left else {panic!("preserved left terminal")};
                let Some(Pane::Term(right)) = &tab.right else {panic!("preserved right terminal")};
                assert_eq!((left.pty.pid(),right.pty.pid()),pids);
            },
            "boardfixture"=>{
                assert!(std::env::var_os("NUS_PORTS_FIXTURE").is_some());
                self.board.rows=(0..24).map(|i|{
                    let mut row=crate::ports::Remembered{port:3000+i,process:if i%3==0{"node"}else{"python"}.into(),command:"npm run dev -- --host 127.0.0.1".into(),cwd:"/fixture/workspace".into(),last_seen:0}.row();
                    row.group=if i<12{crate::ports::Group::Mine}else{crate::ports::Group::Others};row.name=Some(["nus workspace","design preview","local api"][i as usize%3].into());
                    row.bound=if i%4==0{"0.0.0.0"}else{"127.0.0.1"}.into();row.exposed=i%4==0;
                    row.started=Some(std::time::SystemTime::now()-Duration::from_secs(3720+i as u64*90));row
                }).collect();self.board.polls=1;self.board.last=Some(crate::clock::now());self.board.ghosts.clear();self.dirty=true;
            },
            "boardpage"=>self.expand_board(),
            "stationchange"=>{
                assert!(std::env::var_os("NUS_PORTS_FIXTURE").is_some());
                self.board.rows[0].name=Some("updated preview".into());
                self.board.rows[1].name=Some("updated api".into());
                self.dirty=true;
            },
            "stationcheck"=>{
                let a=&self.board.flaps[&self.board.rows[0].key];let b=&self.board.flaps[&self.board.rows[1].key];
                assert!(a.after[1].contains("UPDATED"));assert!(b.after[1].contains("UPDATED"));
                let gap=if a.at>b.at{a.at-b.at}else{b.at-a.at};
                assert!(gap<Duration::from_millis(40),"rows must not queue behind each other");
                let steady=&self.board.flaps[&self.board.rows[2].key];
                assert!(a.at>steady.at+Duration::from_millis(500),"unchanged row must keep its own clock");
            },
            "boardbounds"=>{
                let body=self.board.viewport;assert!(body.h>0.0);
                for (r,hit) in &self.board.hits {
                    let bound=if matches!(hit,crate::ports::Hit::Close|crate::ports::Hit::Expand|crate::ports::Hit::Grouping){self.board.rect}else{body};
                    assert!(r.x>=bound.x-1.0&&r.y>=bound.y-1.0&&r.right()<=bound.right()+1.0&&r.bottom()<=bound.bottom()+1.0,"ports hit escaped {hit:?}: {r:?} vs {bound:?}");
                }
            },
            "boardselectionvisible"=>{let key=self.board.sel.as_ref().expect("ports selection");assert!(self.board.hits.iter().any(|(r,h)|r.h>0.0&&matches!(h,crate::ports::Hit::Row(k)if k==key)),"keyboard selection must stay visible");},
            "boardscroll"=>{let r=self.board.viewport;let before=self.board.scroll;self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);self.wheel(winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0,-rest.parse::<f64>().unwrap()*self.scale as f64)));assert!(self.board.scroll>before,"ports wheel must scroll the page");},
            "boarddetail"=>{self.board.expanded=self.board.rows.first().map(|r|r.key.clone());self.board.sel=self.board.expanded.clone();self.dirty=true;},
            "pinclick"=>{
                let r=self.side_hits.iter().rev().find_map(|(r,h)|match h{crate::app::SideHit::Pinned(act) if format!("{act:?}")==rest=>Some(*r),_=>None}).expect("visible pin control");
                self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);
            },
            "pindrag"=>{
                let (from,to)=rest.split_once(' ').unwrap();let from:usize=from.parse().unwrap();let to:usize=to.parse().unwrap();
                let pos=|k|self.side_hits.iter().find_map(|(r,h)|(*h==crate::app::SideHit::Pinned(crate::pins::Act::Open(k))).then_some((r.x+r.w*0.4,r.y+r.h*0.5))).unwrap();
                let (x,y)=pos(from);let (tx,ty)=pos(to);
                self.mouse_moved(x,y);self.mouse_button(MouseButton::Left,ElementState::Pressed);assert!(self.pins.drag.is_some(),"pin drag not armed: {from} at {x},{y}");self.mouse_moved(tx,ty);assert!(self.pins.drag.is_some_and(|(_,_,moved)|moved));self.mouse_button(MouseButton::Left,ElementState::Released);
            },
            "pinsscroll"=>{let r=self.pins.rect;self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);self.wheel(winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0,-10000.0)));assert!(self.pins.scroll>0.0);},
            "livefoldersassert"=>{let names=self.folders.iter().filter(|f|matches!(f.kind,crate::folders::Kind::Github|crate::folders::Kind::Ports)).map(|f|f.name.as_str()).collect::<Vec<_>>().join("|");assert_eq!(names,rest);},
            "pinsassert"=>{let titles=self.pins.items.iter().map(|p|p.title.as_str()).collect::<Vec<_>>().join("|");assert_eq!(titles,rest);},
            "pinsbounds"=>{
                let sb=self.list_rect();for(r,h)in &self.side_hits{if matches!(h,crate::app::SideHit::Pinned(_)){assert!(r.x>=sb.x&&r.right()<=sb.right()&&r.y>=sb.y&&r.bottom()<=self.sidebar_geometry().foot_y,"pin control escaped {h:?}: {r:?}");}}
            },
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
            "pipfocus"=>self.focus_changed(rest=="on"),
            "pipassert"=>assert_eq!(self.pip.is_some(),rest=="open"),
            "pipclose"=>self.close_pip(),
            "pipaspect"=>{
                let expected:f64=rest.parse().unwrap();let p=self.pip.as_ref().expect("PiP exists");
                assert!((p.aspect-expected).abs()<0.00001,"wrong stream ratio: {}",p.aspect);
                assert!((p.cur.w/p.cur.h-expected).abs()<0.00001,"logical geometry drifted: {:?}",p.cur);
                let size=p.window.inner_size();
                assert!((size.width as f64-size.height as f64*expected).abs()<=(1.0+expected)*0.5+0.01,"native window has wrong shape: {:?}",size);
                eprintln!("PIP_ASPECT {expected} NATIVE {size:?}");
            },
            "pipnativesize"=>{
                let (w,h)=rest.split_once(' ').unwrap();let (w,h):(f64,f64)=(w.parse().unwrap(),h.parse().unwrap());
                let p=self.pip.as_ref().unwrap();let _=p.window.request_inner_size(winit::dpi::LogicalSize::new(w,h));
            },
            "readingcontext"=>{
                let id=self.library_rows(rest).first().expect("matching reading item").id.clone();
                let Some(Pane::Home(h))=self.tabs.get(self.active).map(|t|t.focused_ref())else{panic!("library")};
                let rect=h.library_ui.hits.iter().find(|(_,hit)|matches!(hit,crate::library::Hit::Row(k) if *k==id)).unwrap().0;
                self.mouse_moved(rect.x+rect.w/2.0,rect.y+rect.h/2.0);
                self.mouse_button(MouseButton::Right,ElementState::Pressed);self.mouse_button(MouseButton::Right,ElementState::Released);
            },
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
                "import" => self.open_me_card_at(crate::me::Step::Import),
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
            // The Ledger, end to end: stand-ins named claude, codex and aider
            // run in real shells. claude reports through the real `nus hook`
            // ($NUS_CLI, $NUS_PANE, the instance socket), then waits for one
            // raw keypress and writes down what it got; aider sends OSC 777.
            "ledgerprep" => {
                let h = |ev: &str| format!("printf '%s' '{ev}' | \"$NUS_CLI\" hook claude\n");
                let mut claude = String::from("#!/bin/sh\n");
                for ev in [
                    r#"{"hook_event_name":"SessionStart","session_id":"s1"}"#,
                    r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1"}"#,
                    r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"/w/crates/vt/src/term.rs"}}"#,
                    r#"{"hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{"file_path":"/w/crates/vt/src/term.rs"}}"#,
                    r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"cargo test --workspace"}}"#,
                    r#"{"hook_event_name":"Notification","message":"Claude needs your permission to use Bash"}"#,
                ] { claude.push_str(&h(ev)); }
                claude.push_str("stty raw -echo; a=$(dd bs=1 count=1 2>/dev/null); stty sane\nprintf 'got:%s' \"$a\" > \"$(dirname \"$0\")/../answer.txt\"\nsleep 60\n");
                let mut done = String::from("#!/bin/sh\n");
                for ev in [
                    r#"{"hook_event_name":"UserPromptSubmit"}"#,
                    r#"{"hook_event_name":"PostToolUse","tool_name":"Write","tool_input":{"file_path":"/w/site/index.html"}}"#,
                    r#"{"hook_event_name":"PostToolUse","tool_name":"Edit","tool_input":{"file_path":"/w/site/app.css"}}"#,
                    r#"{"hook_event_name":"Stop"}"#,
                ] { done.push_str(&h(ev)); }
                done.push_str("sleep 60\n");
                let aider = "#!/bin/sh\nsleep 1\nprintf '\\033]777;notify;aider;Add these files to the chat?\\007'\nsleep 60\n";
                let codex = "#!/bin/sh\nsleep 60\n";
                let _ = std::fs::create_dir_all("agents/done");
                for (path, text) in [("agents/claude", claude.as_str()), ("agents/done/claude", done.as_str()), ("agents/codex", codex), ("agents/aider", aider)] {
                    std::fs::write(path, text).unwrap();
                    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap(); }
                }
                let home = self.active;
                // Shells open in the home folder: absolute paths, and the
                // answer lands beside the scripts.
                let here = std::env::current_dir().unwrap();
                for cmd in ["agents/claude", "agents/codex", "agents/done/claude", "agents/aider"] {
                    self.new_tab(self.behavior.default_profile);
                    if let Pane::Term(t) = &mut self.tabs[self.active].left {
                        t.type_at_prompt = Some(format!("{}\r", here.join(cmd).display()));
                    }
                }
                self.activate(home);
                self.dirty = true;
            },
            "ledgercheck" => {
                use crate::agent::{Answer, Phase};
                let find = |app: &App, name: &str, phase: Phase| app.tabs.iter().position(|t| t.agent().is_some_and(|a| a.name == name && a.phase == phase));
                let claude = find(self, "claude", Phase::Waiting).unwrap_or_else(|| panic!("claude is not waiting: {:?}", self.tabs.iter().map(|t| t.agent().map(|a| (a.name.clone(), a.phase))).collect::<Vec<_>>()));
                let a = self.tabs[claude].agent().unwrap().clone();
                assert!(a.hooked, "claude's state did not come from its hooks");
                assert_eq!(a.session.as_deref(), Some("s1"));
                assert_eq!(a.ask.as_ref().map(|k| (k.tool.as_str(), k.what.as_str())), Some(("Bash", "cargo test --workspace")));
                assert_eq!(a.touched, vec!["/w/crates/vt/src/term.rs".to_string()]);
                let panes: Vec<String> = self.tabs.iter().filter_map(|t| match &t.left { Pane::Term(p) => Some(format!("{:?} marks={:?} blocks={:?}", p.program, p.term.marks.iter().rev().take(4).map(|m| m.kind).collect::<Vec<_>>(), p.blocks().last().map(|b| (b.cmd.clone(), b.running)))), _ => None }).collect();
                assert!(find(self, "codex", Phase::Working).is_some(), "codex is not working: {panes:#?}");
                let done = find(self, "claude", Phase::Done).expect("the second claude is not done");
                assert_eq!(self.tabs[done].agent().unwrap().touched.len(), 2);
                let aider = find(self, "aider", Phase::Waiting).expect("aider's OSC 777 did not make it wait");
                assert_eq!(self.tabs[aider].agent().unwrap().reason.as_deref(), Some("aider · Add these files to the chat?"));
                assert_eq!(self.agent_counts(), (2, 1));
                assert!(self.crumb_hits.iter().any(|(_, h)| *h == crate::app::CrumbHit::Agents), "the strip does not count assistants");
                assert!(!self.side_hits.iter().any(|(_, h)| matches!(h, crate::app::SideHit::Answer(i, _) if *i == aider)), "aider has no answers nus knows");
                let (r, _) = *self.side_hits.iter().find(|(_, h)| *h == crate::app::SideHit::Answer(claude, Answer::Allow)).expect("no ALLOW in claude's row");
                self.mouse_moved(r.x + r.w * 0.5, r.y + r.h * 0.5);
                self.mouse_button(MouseButton::Left, ElementState::Pressed);
                self.mouse_button(MouseButton::Left, ElementState::Released);
                assert_eq!(self.tabs[claude].agent().map(|a| a.phase), Some(Phase::Working), "ALLOW did not move claude on");
            },
            "ledgeranswer" => {
                assert_eq!(std::fs::read_to_string("answer.txt").unwrap_or_default(), "got:1", "claude's prompt did not receive the allow key");
            },
            // Home with another tab open: the row clicked is the row taken.
            // (Rows naming other tabs were missing while the frame drew.)
            "homeclickprep" => {
                let mut c=crate::prompt::Config::default();
                for s in c.sources.iter_mut(){s.home=matches!(s.source,crate::prompt::Source::Sessions|crate::prompt::Source::Assistants);}
                self.behavior.prompt=c;
                self.new_tab(self.behavior.default_profile);
                self.open_home();
                self.dirty=true;
            },
            "homeclickcheck" => {
                let Pane::Home(h)=&self.tabs[self.active].left else{panic!("home expected")};
                let Some((drawn,rows))=h.shown.clone() else{panic!("home rows not gathered")};
                assert!(drawn.is_empty());
                assert!(rows.iter().any(|r|r.text.starts_with("Resume")),"drawn rows lack the other tab: {:?}",rows.iter().map(|r|&r.text).collect::<Vec<_>>());
                let k=rows.iter().position(|r|r.text.contains("Claude")).expect("Claude row");
                let (r,_)=*h.hits.iter().find(|(_,i)|*i==k).expect("Claude row drawn");
                self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);
                self.mouse_button(MouseButton::Left,ElementState::Pressed);
                self.mouse_button(MouseButton::Left,ElementState::Released);
                assert!(matches!(self.palette.as_ref().map(|(m,_)|m),Some(PaletteMode::Assistant(0))),"the Claude row did not open Claude's prompt");
                self.palette=None;
            },
            // The nus button: down is home, down again is back; leaving home
            // for anything else closes the loop.
            "homelatchcheck" => {
                use crate::app::CrumbHit;
                self.new_tab(self.behavior.default_profile);
                let from=self.tabs[self.active].id;
                let before=self.tabs.len();
                self.crumb_action(CrumbHit::Nus);
                assert!(matches!(&self.tabs[self.active].left,Pane::Home(_)),"nus did not bring home up");
                assert!(self.home_latch.is_some(),"nus did not latch");
                self.crumb_action(CrumbHit::Nus);
                assert_eq!(self.tabs[self.active].id,from,"nus again did not go back");
                assert!(self.home_latch.is_none());
                assert_eq!(self.tabs.len(),before,"the home made for the press stayed");
                // Back (the mouse's) does the same.
                self.crumb_action(CrumbHit::Nus);
                assert!(self.home_latch_back());
                assert_eq!(self.tabs[self.active].id,from);
                // Going somewhere closes the loop: nus then opens home again.
                self.crumb_action(CrumbHit::Nus);
                let home=self.tabs[self.active].id;
                let i=self.tabs.iter().position(|t|t.id==from).unwrap();
                self.activate(i);
                self.tend_home_latch();
                assert!(self.home_latch.is_none(),"leaving home kept the latch");
                self.crumb_action(CrumbHit::Nus);
                assert_eq!(self.tabs[self.active].id,home,"nus did not return to the one home");
                self.crumb_action(CrumbHit::Nus);
            },
            "startpage" => {
                use crate::settings::{Hit, Then};
                let (kind, value) = rest.split_once(' ').unwrap_or((rest, ""));
                let page = match kind {
                    "home" => { self.behavior.home_url = value.into(); Then::HomePage }
                    "last" => Then::LastPage,
                    "layout" => { self.behavior.then_layout = value.into(); Then::Layout }
                    "prompt" => Then::Prompt,
                    "palette" => Then::Palette,
                    _ => panic!("unknown start page: {kind}"),
                };
                self.apply_setting(Hit::Then(page), 0.0);
                self.save_prefs();
            }
            "assertpalette" => assert_eq!(self.palette.is_some(), rest == "open"),
            "assertpane" => {
                let kind = self.tabs.get(self.active).map(|t| match &t.left {
                    Pane::Home(_) => "home", Pane::Web(_) => "web", Pane::Term(_) => "term", Pane::Editor(_) => "editor", Pane::Settings(_) => "settings", Pane::Hints(_) => "welcome", Pane::Downloads(_) => "downloads", _ => "other",
                }).unwrap_or("missing");
                assert_eq!(kind, rest, "focused pane at step `{step}`");
            }
            // `eval <js>`: run it on the focused page and keep the answer;
            // `assertreply <text>` (after a `wait`) checks it came back
            // with that text in it. For checks that look inside a page.
            "eval" => {
                let tab = &self.tabs[self.active];
                let pane = if tab.focus_right { tab.right.as_ref().unwrap_or(&tab.left) } else { &tab.left };
                let Pane::Web(w) = pane else { panic!("eval needs a page") };
                let id = w.tab.eval_reply(rest);
                if let Some(s) = self.shot.as_mut() {
                    s.reply = Some(id);
                }
            }
            "assertreply" => {
                let id = self.shot.as_ref().and_then(|s| s.reply).expect("eval first");
                let tab = &self.tabs[self.active];
                let pane = if tab.focus_right { tab.right.as_ref().unwrap_or(&tab.left) } else { &tab.left };
                let Pane::Web(w) = pane else { panic!("assertreply needs a page") };
                let v = w.tab.take_reply(id).expect("the page did not answer in time; wait longer before assertreply");
                let text = v.pointer("/result/value").map(|x| match x { serde_json::Value::String(s) => s.clone(), other => other.to_string() }).unwrap_or_else(|| v.to_string());
                assert!(text.contains(rest), "page said {text:?}, expected {rest:?}");
                eprintln!("shot: reply {text}");
            }
            "asserturl" => {
                let Some(Pane::Web(web)) = self.tabs.get(self.active).map(|t| &t.left) else { panic!("expected web page") };
                assert_eq!(web.tab.shared.borrow().url, rest);
            }
            "appmenu" => {
                let command=crate::application_menu::ITEMS.iter().find(|i|format!("{:?}",i.command)==rest).expect("menu command").command;
                let _=self.proxy.send_event(crate::UserEvent::ApplicationMenuCheck(command));
            }
            "memorypressure" => {
                let level = match rest { "warning" => crate::memory_pressure::Level::Warning, "critical" => crate::memory_pressure::Level::Critical, _ => panic!("memorypressure requires warning or critical") };
                self.reclaim_memory(level);
            }
            "memory" => eprintln!("MEMORY {} {}", rest, crate::perf::memory_snapshot()),
            "awaitperf" => assert!(crate::perf::has_samples(rest), "missing performance metric: {rest}"),
            "perfreset" => crate::perf::reset(),
            "perfstats" => eprintln!("PERF {} {}", rest, crate::perf::snapshot()),
            "assertpresented" => assert!(self.frames > 0, "no frame has been presented"),
            "asserteditorready" => {
                let b = self.focused_editor().and_then(|e| e.buf()).expect("editor buffer");
                assert!(b.ready(), "file is still loading: {:?}", b.load_error);
                if !rest.is_empty() { assert_eq!(b.text.len_bytes(), rest.parse::<usize>().unwrap()); }
            }
            "editorfind" => {
                let e = self.focused_editor().expect("editor");
                e.find = Some(crate::editor::Find {query:rest.into(),replace:String::new(),in_replace:false,with_replace:false,matches:Vec::new(),current:0,truncated:false});
                e.refind(); self.dirty = true;
            }
            "assertfind" => {
                let f = self.focused_editor().and_then(|e| e.find.as_ref()).expect("find");
                assert_eq!(f.matches.len(), rest.parse::<usize>().unwrap());
            }
            "editorcursor" => {
                let b = self.focused_editor().and_then(|e| e.buf_mut()).expect("editor");
                b.cursor = if rest == "end" { b.len_chars() } else {rest.parse().unwrap()};
                self.focused_editor().unwrap().reveal(); self.dirty = true;
            }
            "openfile" => self.run(crate::app::Action::OpenFile(rest.into())),
            "hatchkey" => {self.in_hatch(|a|a.shot_key(rest));},
            "asserthatchzoom" => {let i=self.hatch_tab().expect("Hatch session");let Pane::Term(t)=self.tabs[i].focused_ref() else {panic!("Hatch terminal")};assert_eq!(t.zoom,rest.parse::<u32>().unwrap());},
            "trafficcheck" => self.check_traffic_lights(),
            "trafficfullscreen" => {let _=self.proxy.send_event(crate::UserEvent::WindowControl(self.window.id(),2));},
            "trafficsize" => {let (w,h)=rest.split_once(' ').unwrap();let _=self.window.request_inner_size(winit::dpi::LogicalSize::new(w.parse::<f64>().unwrap(),h.parse::<f64>().unwrap()));let size=self.window.inner_size();self.resize(size.width,size.height);},
            "assertchrome" => {
                let pane=self.tabs[self.active].focused_ref();
                assert!((pane.rect().y-self.strip_rect().bottom()).abs()<40.0*self.scale,"page zoom changed window chrome");
                assert_eq!(self.px(10.0),(10.0*self.scale).round(),"page zoom escaped its drawing scope");
            }
            "zoomprobe" => {
                use cef::ImplBrowserHost;
                if rest == "start" {
                    self.zoom_focused(1);
                    self.zoom_focused(1);
                    self.zoom_focused(1);
                }
                let Pane::Web(w) = self.tabs[self.active].focused_ref() else { panic!("zoomprobe needs a page") };
                let host = w.tab.host().unwrap();
                match rest {
                    "start" => {
                        assert_eq!(w.tab.zoom_percent(), 150, "rapid presses must accumulate toward the target");
                        assert!(w.tab.shared.borrow().zoom_motion.is_some());
                        w.tab.zoom_to(150, 1.0);
                    }
                    "middle" => {
                        let actual = 1.2_f64.powf(host.zoom_level()) * 100.0;
                        assert!(actual > 100.0 && actual < 150.0, "expected a real intermediate page zoom, got {actual}");
                        assert_eq!(w.tab.zoom_percent(), 150);
                    }
                    "end" => {
                        assert!(w.tab.shared.borrow().zoom_motion.is_none());
                        assert_eq!((1.2_f64.powf(host.zoom_level()) * 100.0).round() as u32, 150);
                        let s = w.tab.shared.borrow();
                        let expected = ((s.size.0 * s.scale).round() as u32, (s.size.1 * s.scale).round() as u32);
                        assert_eq!(s.paint_size, expected, "page backing texture must match physical pixel density");
                    }
                    "reduce" => {
                        w.tab.zoom_to(200, 1.0);
                        w.tab.tick_zoom(true);
                        assert!(w.tab.shared.borrow().zoom_motion.is_none());
                        assert_eq!(w.tab.zoom_percent(), 200);
                        w.tab.zoom_to(100, 0.0);
                    }
                    "dpi" => {
                        w.tab.set_scale(1.0);
                        w.tab.set_scale(self.scale);
                        assert_eq!(w.tab.shared.borrow().scale, self.scale);
                    }
                    _ => panic!("unknown zoom probe"),
                }
            }
            "assertzoom" => {
                use cef::ImplBrowserHost;
                let percent=match self.tabs[self.active].focused_ref() {
                    Pane::Web(w)=>(1.2_f64.powf(w.tab.host().unwrap().zoom_level())*100.0).round() as u32,
                    Pane::Term(t)=>t.zoom, Pane::Editor(e)=>e.zoom, _=>self.ui_zoom,
                };
                assert_eq!(percent,rest.parse::<u32>().unwrap());
            }
            // Exercise the real idle policy without a minute-long fixture wait.
            "ageinactivetabs" => {
                let age=Duration::from_secs(rest.parse::<u64>().unwrap());
                for (i,tab) in self.tabs.iter_mut().enumerate() {if i!=self.active {tab.last_active=crate::clock::now()-age;}}
                self.last_tend=crate::clock::now()-Duration::from_secs(6);
                self.tend_idle_tabs();
            }
            "activatetab" => self.activate(rest.parse().unwrap()),
            "assertsleep" => {
                let (i,asleep)=rest.split_once(' ').unwrap();
                let Pane::Web(w)=&self.tabs[i.parse::<usize>().unwrap()].left else {panic!("expected page")};
                assert_eq!(w.asleep.is_some(),asleep=="true","tab suspension");
            }
            "assertbrowsers" => {assert_eq!(crate::browser::live_count(),rest.parse::<usize>().unwrap(),"CEF browser lifecycle");}
            "resourcestats" => {
                let streams=self.recorder.as_ref().map_or(0,|r|crate::storage::files(&r.dir,true).iter().map(|e|e.bytes).sum::<u64>());
                let rss=std::process::Command::new("ps").args(["-o","rss=","-p",&std::process::id().to_string()]).output().ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
                eprintln!("RESOURCE_STATS {rest} browsers={} replay_bytes={} tabs={} rss_kib={}",crate::browser::live_count(),streams,self.tabs.len(),rss);
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
            // Hold until the focused page stops loading, or the timeout (ms)
            // passes; then say how long, where it ended and what was blocked.
            "awaitload" => {
                static START: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
                let timeout = rest.trim().parse::<u64>().unwrap_or(30000);
                let start = *START.lock().unwrap().get_or_insert_with(crate::clock::now);
                let state = self.tabs.get(self.active).and_then(|t| match t.focused_ref() { Pane::Web(w) => { let s = w.tab.shared.borrow(); Some((s.loading, s.url.clone(), w.tab.blocked())) }, _ => None });
                let (loading, url, blocked) = state.unwrap_or((true, String::new(), 0));
                let ms = crate::clock::since(start).as_millis() as u64;
                if loading && ms < timeout {
                    let script = self.shot.as_mut().unwrap();
                    script.next -= 1;
                    script.until = Some(crate::clock::now() + Duration::from_millis(100));
                } else {
                    *START.lock().unwrap() = None;
                    eprintln!("shot: load {} {ms}ms blocked={blocked} url={url}", if loading { "STUCK" } else { "done" });
                }
            }
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
            "settingclick" | "settinghover" => {
                let (rect,_) = self.settings_hits.iter().find(|(_,hit)|format!("{hit:?}")==rest).copied().unwrap_or_else(||panic!("setting not visible: {rest}"));
                self.mouse_moved(rect.x+rect.w/2.0,rect.y+rect.h/2.0);
                if verb == "settingclick" {
                    self.mouse_button(MouseButton::Left,ElementState::Pressed);
                    self.mouse_button(MouseButton::Left,ElementState::Released);
                }
            }
            "assertchoice" => {
                let Pane::Settings(p)=&self.tabs[self.active].left else {panic!("not settings")};
                assert!(self.setting_states(p.section).iter().any(|(hit,on)|format!("{hit:?}")==rest && *on),"choice is not selected: {rest}");
            }
            "welcomebounds" => {
                for (i,(a,act)) in self.welcome_hits.iter().enumerate() {for (b,other) in self.welcome_hits.iter().skip(i+1) {
                    let overlap=a.intersect(b);assert!(overlap.w<1.0 || overlap.h<1.0,"overlapping Welcome controls: {act:?} / {other:?}");
                }}
            },
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
                assert_eq!(art.backdrop != crate::art::Backdrop::Theme, matches!(rest,"sky"|"space"));
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
            "savedfixture"=>{
                let c=&mut self.behavior.prompt;
                c.saved=vec!["> cargo test --workspace".into(),"> git status --short".into(),"https://docs.rs".into(),"@codex Review the current changes".into()];
                c.saved_names=c.saved.iter().cloned().zip(["Test the workspace","Working tree","Rust documentation","Review changes"].map(String::from)).collect();
                c.saved_preview=true;c.saved_library=true;c.saved_run=false;self.save_prefs();self.dirty=true;
            },
            "savedcheck"=>{
                use crate::app::Action;
                let before=self.behavior.prompt.clone();
                assert!(matches!(self.prompt_rows("Test the workspace")[0].action,Action::SavedUse(0,false)));
                self.behavior.prompt.sources=self.behavior.prompt.ordered();
                let source=self.behavior.prompt.sources.iter_mut().find(|s|s.source==crate::prompt::Source::Saved).unwrap();source.home=false;source.search=false;
                assert!(self.prompt_rows("").iter().all(|r|!r.num.starts_with("saved")));
                assert!(self.prompt_rows("Test the workspace").iter().all(|r|!matches!(r.action,Action::SavedUse(..))));
                self.behavior.prompt=before;
                self.saved_edit(0,true,"Run workspace tests".into());assert!(matches!(self.prompt_rows("Run workspace tests")[0].action,Action::SavedUse(0,false)));
                self.saved_edit(0,true,"Test the workspace".into());
            },
            "savedinsertprobe"=>{
                let path=std::env::current_dir().unwrap().join("insert-must-not-execute");
                let value=format!("> touch '{}'",path.display().to_string().replace('\'',"'\\''"));
                let i=self.behavior.prompt.saved.len();self.behavior.prompt.saved.push(value);self.saved_use(i,false);self.behavior.prompt.saved.pop();
            },
            "savedinsertcheck"=>{
                assert!(!std::env::current_dir().unwrap().join("insert-must-not-execute").exists(),"Insert executed the saved command");
                let Pane::Term(t)=self.tabs[self.active].focused_ref()else{panic!("Insert must open a shell")};assert!(t.type_at_prompt.is_none(),"command never reached the shell prompt");
            },
            "reelat"=>{self.me_card.import.phase=rest.parse().unwrap();self.me_card.import.focus=1;self.dirty=true;},
            "importsource"=>self.import_hit(crate::me::CardHit::ImportOpen),
            "importfixture"=>{self.open_me_card_at(crate::me::Step::ImportReview);self.me_card.import.source=0;self.me_card.import.plan=Some(crate::import_flow::parse(0,r#"<a href="https://docs.rs">Rust documentation</a><a href="https://nus.dev">nus</a>"#).unwrap());},
            "importapplycheck"=>{self.import_hit(crate::me::CardHit::ImportApply);assert!(self.folders.iter().any(|f|f.name=="Imported from Arc"&&f.items.len()==2));assert!(self.me_card.import.plan.is_none());},
            "assertpromptfirst"=>{let Some((mode,input))=&self.palette else{panic!("palette not open")};let rows=self.palette_rows(*mode,input);assert!(rows.first().is_some_and(|r|r.text.contains(rest)),"unexpected route: {:?}",rows.iter().map(|r|&r.text).collect::<Vec<_>>());},
            "assertconnections"=>{assert!(self.assistants.pending.is_none(),"connection checks did not finish");for entry in &self.assistants.entries{assert!(entry.checked);assert!(entry.path.is_some());assert!(!entry.version.is_empty());}assert_eq!(self.assistants.entries[2].models,vec!["test-model:latest"],"connections: {:?}",self.assistants.entries);},
            "assertfonts"=>{let c=&self.behavior.typography;let Pane::Term(t)=&self.tabs[self.active].left else{panic!("not terminal")};assert!((t.grid.px-self.terminal_px()).abs()<0.01);let plain=self.fonts.metrics(self.f.term,self.terminal_px());assert!(t.grid.metrics.advance>=plain.advance+c.terminal_spacing*self.scale-0.01);assert!(t.grid.metrics.line_height>=plain.line_height);},
            // The intelligence ring and dial, driven like a pointer would.
            "intelturn"=>{let dx:f32=rest.parse().unwrap();let (r,_)=*self.intel.hits.iter().find(|(_,p)|matches!(p,crate::intelligence::Part::Ring{..})).expect("no ring drawn");let (x,y)=(r.x+r.w*0.5,r.y+r.h*0.5);assert!(self.intel_mouse(true,x,y));for i in 1..=12{self.intel_move(x+dx*self.scale*i as f32/12.0,y);std::thread::sleep(std::time::Duration::from_millis(16));}self.intel_mouse(false,x+dx*self.scale,y);},
            "inteltap"=>{let (r,_)=*self.intel.hits.iter().find(|(_,p)|matches!(p,crate::intelligence::Part::Ring{..})).expect("no ring drawn");let x=if rest=="left"{r.x+r.w*0.2}else{r.x+r.w*0.8};assert!(self.intel_mouse(true,x,r.y+r.h*0.5));self.intel_mouse(false,x,r.y+r.h*0.5);},
            "intelspin"=>{let deg:f32=rest.parse::<f32>().unwrap().to_radians();let Some(crate::intelligence::Part::Atom{cx,cy,m})=self.intel.hits.iter().map(|(_,p)|*p).find(|p|matches!(p,crate::intelligence::Part::Atom{..})) else{panic!("no atom drawn")};let (x,y)=(cx+deg.cos()*m*0.38,cy-deg.sin()*m*0.38);assert!(self.intel_mouse(true,x,y));self.intel_move(x,y);self.intel_mouse(false,x,y);},
            "intelnucleus"=>{let Some(crate::intelligence::Part::Atom{cx,cy,..})=self.intel.hits.iter().map(|(_,p)|*p).find(|p|matches!(p,crate::intelligence::Part::Atom{..})) else{panic!("no atom drawn")};assert!(self.intel_mouse(true,cx,cy));self.intel_mouse(false,cx,cy);},
            "assertintel"=>{let mut a=rest.split_whitespace();assert_eq!(self.intelligence().to_string(),a.next().unwrap(),"intelligence level");if let Some(m)=a.next(){let m=if m=="auto"{""}else{m};assert_eq!(self.behavior.assistants.providers[0].model,m,"claude model");}},
            "assertcommand"=>{let (id,want)=rest.split_once(' ').unwrap();let c=self.assistant_command(id.parse().unwrap(),"hi").unwrap();assert!(c.contains(want),"command {c:?} lacks {want:?}");},
            "assistantdraft"=>{let (id,q)=rest.split_once(' ').unwrap_or((rest,""));self.draft_assistant(id.parse().unwrap(),q);},
            "reviewbounds"=>{assert!(matches!(self.palette,Some((PaletteMode::Assistant(_),_))));let size=self.window.inner_size();for r in self.palette_hits.iter().filter(|r|r.h>0.0){assert!(r.x>=0.0&&r.right()<=size.width as f32&&r.y>=0.0&&r.bottom()<=size.height as f32,"review row outside window: {r:?}");}if rest=="scrollable"{assert!(self.palette_scroll_max>0.0);}},
            "reviewscroll"=>{self.wheel(winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0,-rest.parse::<f64>().unwrap())));},
            "settingsbounds"=>{let Pane::Settings(p)=&self.tabs[self.active].left else{panic!("not settings")};for (r,h) in &self.settings_hits{assert!(r.x>=p.rect.x-1.0&&r.right()<=p.rect.right()+1.0&&r.bottom()<=p.rect.bottom()+1.0,"escaped hit {h:?} {r:?} {:?}",p.rect);}},
            "mercuryclaim" => {self.claim_mercury();assert!(crate::mercury::earned());},
            "mercurystate" => {
                match rest {
                    "absent" => assert!(self.me_card.mercury_reveal.is_none()),
                    "moving" => assert!(self.me_card.mercury_reveal.is_some()),
                    "closed" => {assert!(self.me_card.mercury_reveal.is_none());assert!(!self.me_card.open);assert!(matches!(self.tabs[self.active].focused_ref(),Pane::Settings(_)));},
                    _ => panic!("unknown Mercury state"),
                }
            },
            "mercurypresentationcheck" => {
                use crate::me::CardHit;
                assert!(self.me_card.mercury_reveal.is_some());
                let expected=1;
                assert_eq!(self.me_card.hits.len(),expected);
                let (w,h)=(self.target.size.0 as f32,self.target.size.1 as f32);
                for (rect,hit) in &self.me_card.hits {
                    assert!(matches!(hit,CardHit::MercuryDone));
                    assert!(rect.x>=0.0&&rect.y>=0.0&&rect.right()<=w&&rect.bottom()<=h,"escaped Mercury target: {rect:?}");
                }
                let tree=self.access_tree();
                assert_eq!(tree.nodes.len(),expected+1,"underlying controls leaked into Mercury dialog");
                assert_eq!(self.access_map.len(),expected);
            },
            "mercuryexport" => {
                assert!(crate::mercury::earned());
                std::fs::create_dir_all(rest).unwrap();
                for size in [16,32,64,128,256,512] {
                    let pixels=crate::mercury::icon(size);
                    std::fs::write(std::path::Path::new(rest).join(format!("mercury-{size}.png")),nus_render::icon::png(&pixels,size,size)).unwrap();
                }
            },
            "mercurycontinue" => {
                let rect=self.me_card.hits.iter().find(|(_,h)|*h==crate::me::CardHit::MercuryDone).expect("Continue button").0;
                self.mouse_moved(rect.x+rect.w*0.5,rect.y+rect.h*0.5);
                self.mouse_button(MouseButton::Left,ElementState::Pressed);
                self.mouse_button(MouseButton::Left,ElementState::Released);
                assert!(self.me_card.mercury_reveal.is_none());
            },
            "mercurycheck" => {
                assert!(crate::mercury::earned());
                assert!(std::path::Path::new("profile/mercury.json").is_file());
                assert!(crate::mercury::label().to_lowercase().contains("earned"));
                assert_eq!(crate::mercury::date(crate::mercury::CUTOFF),"2027-01-01");
            },
            "vaultcheck" => {
                let path=std::env::current_dir().unwrap().join("profile/memory.md");
                crate::protected_state::write(&path,b"synthetic vault canary").unwrap();
                assert_eq!(crate::protected_state::read_text(&path).unwrap(),"synthetic vault canary");
                let bytes=std::fs::read(path).unwrap();assert!(bytes.starts_with(b"NUSENC01"));
                assert!(!bytes.windows(9).any(|b|b==b"synthetic"));
            },
            "heldvaultcheck" => {
                let dir=Self::hold_dir();
                let profile=nus_pty::Profile{name:"vault probe".into(),program:"/bin/sh".into(),args:vec!["-c".into(),"printf held-vault-canary; sleep 10".into()],cwd:None,env:Vec::new()};
                let pty=nus_pty::Pty::spawn_held(&profile,80,24,&dir,||{}).expect("encrypted held process");
                let id=pty.held_id().unwrap().to_owned();let info=nus_pty::hold::Info::read(&dir,&id).unwrap();
                let bytes=std::fs::read(nus_pty::hold::Info::path(&dir,&id)).unwrap();
                assert!(bytes.starts_with(b"NUSENC01"));assert!(!bytes.windows(info.token.len()).any(|b|b==info.token.as_bytes()));
                pty.detach();std::thread::sleep(Duration::from_millis(80));
                let mut resumed=nus_pty::Pty::attach(info,80,24,||{}).expect("reattach with decrypted token");
                std::thread::sleep(Duration::from_millis(80));
                let output=resumed.take_output();resumed.kill();assert!(String::from_utf8_lossy(&output).contains("held-vault-canary"));
            },
            "updatewarningcheck" => {
                assert!(crate::updates::status().confirming);
                let bottom=self.settings_hits.iter().find(|(_,h)|matches!(h,crate::settings::Hit::UpdateConfirm)).expect("update confirmation button").0.bottom();
                if let Some(Pane::Settings(p))=self.tabs.get_mut(self.active).map(|t|&mut t.left){p.scroll+=(bottom-p.rect.bottom()+self.scale*32.0).max(0.0);}self.dirty=true;
            },
            "updateready" => {crate::updates::preview_warning();crate::updates::confirm(false);self.dirty=true;},
            "updateheaderclick" => {
                assert!(crate::updates::status().available);
                let r=self.crumb_hits.iter().find(|(_,h)|*h==crate::app::CrumbHit::Updates).expect("header update icon").0;
                for (other,hit) in &self.crumb_hits{if matches!(hit,crate::app::CrumbHit::Search|crate::app::CrumbHit::Start){assert!(r.right()<=other.x||other.right()<=r.x,"header buttons overlap");}}
                self.mouse_moved(r.x+r.w/2.0,r.y+r.h/2.0);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);
                assert!(crate::updates::status().confirming);
            },
            "updatereview" => {crate::updates::preview_warning();self.dirty=true;},
            "bundle"=>self.bundle_toggle(rest),
            "assertbundle"=>{let b=crate::bundles::list().into_iter().find(|b|b.id==rest).unwrap();assert_eq!(self.bundle_state(&b),crate::bundles::State::Installed);for entry in &b.entrypoints{let stem=std::path::Path::new(entry).file_stem().unwrap().to_str().unwrap();assert!(crate::bundles::resolve(stem).is_some());}},
            "asserttoast"=>{let t=self.toast.as_ref().expect("toast expected");assert!(format!("{} {}",t.words,t.detail).contains(rest),"toast: {} {}",t.words,t.detail);},
            "noticefixture"=>self.notice(nus_render::text::icons::CHECK,rest,""),
            "toastfixture"=>{
                // icon|Words|detail|act — icon: copy totab download ports palette check, or problem; act: tab download retry
                let f:Vec<&str>=rest.splitn(4,'|').collect();let g=|i:usize|f.get(i).copied().unwrap_or("").trim().to_string();
                use nus_render::text::icons;
                let icon=match g(0).as_str(){"copy"=>icons::COPY,"totab"=>icons::TO_TAB,"download"=>icons::DOWNLOAD,"ports"=>icons::PORTS,"palette"=>icons::PALETTE,"history"=>icons::HISTORY,"search"=>icons::SEARCH,"folder"=>icons::FOLDER,"eyeslash"=>icons::EYE_SLASH,_=>icons::CHECK};
                let act=match g(3).as_str(){"tab"=>Some(crate::toast::Act::GoTab(0)),"download"=>Some(crate::toast::Act::RevealDownload(0)),"retry"=>Some(crate::toast::Act::RetryInstall(String::new())),_=>None};
                if g(0)=="problem"{self.toast_problem(g(1),g(2),act);}else{self.toast(icon,g(1),g(2),act);}
            },
            // The toast: the pointer over its chip (hover), a press on the
            // chip (chip), or a press on its tone cell, which puts it away (cell).
            "toastpress"=>{let t=self.toast.as_ref().expect("toast expected");let (r,cell)=(t.chip,t.rect);let (x,y)=if rest=="cell"{(cell.x+cell.h/2.0,cell.y+cell.h/2.0)}else{assert!(r.w>0.0,"no chip");(r.x+r.w/2.0,r.y+r.h/2.0)};self.mouse_moved(x,y);if rest!="hover"{self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);}},
            "asserttoastgone"=>assert!(self.toast.is_none(),"toast still up"),
            // Each running language server, by command@root, and what it logged.
            "lsplog"=>{for (key,s) in &self.lsp.map{eprintln!("shot: lsp {key} ({} lines)",s.log.len());for l in &s.log{eprintln!("shot: lsp   {l}");}}},
            // The command being typed: select characters [from, to) of it,
            // or check what the shell holds there now.
            "cmdsel"=>{let mut n=rest.split_whitespace().filter_map(|v|v.parse::<usize>().ok());let (from,to)=(n.next().unwrap_or(0),n.next().unwrap_or(1));if let Some(t)=self.focused_term(){let m=*t.term.marks.iter().rev().find(|m|m.kind==nus_vt::MarkKind::CommandStart).expect("a prompt with marks");t.sel=Some(crate::termui::Selection{anchor:(m.line,m.col+from),head:(m.line,m.col+to-1),zone:crate::termui::Zone::Cell,dragging:false});self.dirty=true;}},
            "assertcmd"=>{let t=self.focused_term().expect("a shell");let m=*t.term.marks.iter().rev().find(|m|m.kind==nus_vt::MarkKind::CommandStart).expect("a prompt with marks");let cols=t.term.cols();let got=t.term.text_range((m.line,m.col),(m.line,cols-1));assert_eq!(got.trim_end(),rest.trim(),"the command line");},
            "assertloadidle"=>{let Pane::Web(w)=self.tabs[self.active].focused_ref()else{panic!("web")};assert!(!w.tab.shared.borrow().loading);assert!(w.load_fade.value()<=0.001);assert!(w.load_since.is_none());assert_eq!(w.load.target,0.0);},
            "iconsettings"=>{self.open_settings();self.look_tab=crate::settings::LOOK_APP_ICON;if let Some(Pane::Settings(p))=self.tabs.get_mut(self.active).map(|t|t.focused()){p.section=crate::settings::SEC_LOOK;p.scroll=0.0;}self.dirty=true;},
            "iconchoose"=>{let choice=crate::app_icon::Choice::ALL.into_iter().find(|c|c.name()==rest).unwrap();let r=self.settings_hits.iter().find(|(_,h)|*h==crate::settings::Hit::AppIcon(choice)).expect("icon card visible").0;self.mouse_moved(r.x+r.w/2.0,r.y+r.h/2.0);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);assert_eq!(self.behavior.app_icon,choice);assert_eq!(crate::app_icon::selected(),choice);},
            "asserticon"=>assert_eq!(self.behavior.app_icon.name(),rest),
            "sidebarhoverprobe"=>{
                self.sidebar=false;self.focus=false;self.sidebar_rules.hover_from=crate::surface::HoverFrom::InsideWindow;self.sidebar_rules.side=crate::surface::Side::Left;self.sidebar_hover=false;self.sidebar_leave=None;self.layout();
                let y=self.content_rect().y+self.px(100.0);
                self.mouse_moved(self.px(300.0),y);self.mouse_moved(self.px(1.0),y);assert!(self.sidebar_hover,"content handler swallowed edge hover");
                self.cursor_left();assert!(self.sidebar_leave.is_some());
                self.mouse_moved(self.px(100.0),y);self.mouse_moved(self.px(1.0),y);assert!(self.sidebar_leave.is_none(),"reentry must cancel stale hide timer");
                self.focus_changed(false);self.focus_changed(true);self.mouse_moved(self.px(300.0),y);self.mouse_moved(self.px(1.0),y);assert!(self.sidebar_hover,"hover after focus return");
                self.sidebar_rules.side=crate::surface::Side::Right;self.sidebar_hover=false;self.mouse_moved(self.px(300.0),y);self.mouse_moved(self.target.size.0 as f32-self.px(1.0),y);assert!(self.sidebar_hover,"right edge hover");
                self.sidebar_rules.side=crate::surface::Side::Left;self.sidebar_hover=false;self.sidebar_leave=None;self.layout();
            },
            "settingscheck" => self.check_settings_bindings(),
            "assertprofile" => assert_eq!(self.me_card.open, rest == "open"),
            "loadbarfixture" => {
                let progress: f32 = rest.parse().expect("progress");
                self.load_bar.style=crate::anim::BarStyle::Radiance;
                let Pane::Web(w)=self.tabs[self.active].focused() else {panic!("loadbar needs web")};
                w.load=crate::anim::Follow::new(progress.clamp(0.0,1.0));
                w.load_reported=progress.clamp(0.0,1.0);
                w.load_fade=crate::anim::Anim::at(1.0);
                w.load_since=Some(crate::clock::now());
                let mut shared=w.tab.shared.borrow_mut();shared.loading=progress<1.0;shared.progress=progress as f64;
                self.dirty=true;
            },
            "assertloadbar" => {
                let Pane::Web(w)=self.tabs[self.active].focused_ref() else {panic!("loadbar needs web")};
                let expected:f32=rest.parse().unwrap();
                assert!((w.load.target-expected).abs()<0.001,"loading target drifted: {}",w.load.target);
            },
            "asserthdr" => assert_eq!(self.target.hdr(),rest=="enabled"),
            "hdrpixels" => { let peak=self.gpu.verify_hdr_signal().expect("native HDR pixels");eprintln!("HDR source readback: {peak:.3} x SDR white");self.dirty=true; },
            // On a fresh install the card can only be finished, not closed:
            // this walks it through with the defaults and Welcome follows.
            "closeprofile" => if self.me_card.first { self.finish_first_walk_now() } else { self.close_me_card() },
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
            "document" => self.open_from_tree(std::path::Path::new(rest),false),
            "devtools" => self.toggle_devtools(),
            "libraryclick" => {
                let Some(Pane::Home(h))=self.tabs.get(self.active).map(|t|t.focused_ref()) else{panic!("library expected")};
                let r=h.library_ui.hits.iter().find(|(_,hit)|format!("{hit:?}")==rest).map(|(r,_)|*r).expect("visible library control");
                self.mouse_moved(r.x+r.w*0.5,r.y+r.h*0.5);self.mouse_button(MouseButton::Left,ElementState::Pressed);self.mouse_button(MouseButton::Left,ElementState::Released);
            },
            "readingbounds" => {
                let Some(Pane::Home(h))=self.tabs.get(self.active).map(|t|t.focused_ref()) else{panic!("library expected")};
                assert!(h.library);
                for (r,hit) in &h.library_ui.hits {assert!(r.x>=h.rect.x-1.0 && r.y>=h.rect.y-1.0 && r.right()<=h.rect.right()+1.0 && r.bottom()<=h.rect.bottom()+1.0,"library control outside pane: {hit:?}");}
                if let Some(reading)=&h.reading {let viewport=reading.reader.saved.viewport.expect("reader viewport");assert!(viewport.h>h.rect.h*0.6,"reader chrome consumed the page");}
            },
            "library" => self.open_library(),
            "savereading" => self.save_reading(),
            "readingscroll" => self.library_scroll(-rest.parse::<f32>().unwrap()*self.scale),
            "readingopen" => {
                let e=self.library_rows(rest).first().cloned().expect("matching saved reading");self.read_saved(&e.id);
            },
            "readingassert" => {
                let (count,words)=rest.split_once(' ').unwrap();
                assert_eq!(self.library.entries.len(),count.parse::<usize>().unwrap());
                assert!(self.library.entries.values().map(|e|e.words).sum::<usize>()>=words.parse::<usize>().unwrap());
            },
            "readingprogress" => {
                let p=self.library.entries.values().map(|e|e.progress).fold(0.0,f32::max);
                assert!(p>=rest.parse::<f32>().unwrap(),"reading progress {p}");
            },
            "readingback" => {
                if let Some(crate::app::Pane::Home(h))=self.tabs.get_mut(self.active).map(|t|&mut t.left){h.reading=None;}self.library.flush(true);self.dirty=true;
            },
            "arrival" => {
                let mut sp=crate::splash::Splash::new();sp.arrival=true;sp.begun=true;
                sp.started=crate::clock::now()-std::time::Duration::from_secs_f32(rest.parse().unwrap());self.splash=Some(sp);self.dirty=true;
            },
            "arrivalcheck" => {
                assert_eq!(self.arriving(),rest=="active");
                if self.arriving() {assert!(self.target.translucent(),"arrival requires a transparent compositor");}
                eprintln!("ARRIVAL CHECK PASSED {rest}");
            },
            "trafficpress" => self.press_traffic_light(rest.parse().unwrap()),
            "trafficstate" => {
                assert_eq!(self.window.fullscreen().is_some(),rest=="fullscreen");
                assert_eq!(self.fullscreen,rest=="fullscreen");
            },
            "reader" => self.toggle_reader(),
            "split" => self.divide(),
            // The pane director, on the active tab: `pane swap`, `pane solo right`,
            // `pane both`, `pane totab right`, `pane kill left`, `pane split`,
            // `pane join 1` (the active tab's right pane onto tab 1), `pane undo`.
            "pane" => {
                use crate::director::Op;
                let tab=self.tabs[self.active].id;
                let mut w=rest.split_whitespace();
                let verb=w.next().unwrap_or("");
                let right=w.next().map(|s|s=="right");
                match verb {
                    "undo"=>self.pane_undo(),
                    "redo"=>self.pane_redo(),
                    "swap"=>{self.direct(Op::Swap{tab});},
                    "split"=>{self.direct(Op::Split{tab});},
                    "solo"=>{self.direct(Op::Solo{tab,solo:true,right:right.unwrap_or(true)});},
                    "both"=>{let r=self.tabs[self.active].focus_right;self.direct(Op::Solo{tab,solo:false,right:r});},
                    "totab"=>{self.direct(Op::ToTab{tab,right:right.unwrap_or(true)});},
                    "kill"=>{self.direct(Op::Kill{tab,right:right.unwrap_or(true)});},
                    "width"=>{let v=rest.split_whitespace().nth(1).and_then(|v|v.parse().ok());self.direct(Op::SplitWidth{tab,w:v});},
                    "join"=>{let to=rest.split_whitespace().nth(1).and_then(|v|v.parse::<usize>().ok()).and_then(|k|self.tabs.get(k)).map(|t|t.id).expect("pane join <tab index>");self.direct(Op::Join{from:tab,right:true,to,side_right:true});},
                    other=>panic!("pane: unknown verb {other}"),
                }
                self.dirty=true;
            }
            // `tile 1 2 3`: those tabs selected and tiled with the first, as
            // Ctrl+click and Ctrl+Shift+D would; a shown tiling grows.
            "tile" => {
                let idx:Vec<usize>=rest.split_whitespace().filter_map(|k|k.parse().ok()).collect();
                if let Some(&first)=idx.first() { if !self.tiling_shown() { self.activate(first); } }
                self.selected=idx.into_iter().collect();
                self.tile_selected();
            }
            // `tileshape L`: the shown tiling's shape, as rows of tiles by
            // their top edge: "L" is 1+2, "grid" 2+2, "row" all on one.
            "asserttileshape" => {
                let rects=self.tile_rects();
                let mut tops:Vec<i32>=rects.iter().map(|(_,r)|r.y.round() as i32).collect();tops.sort();tops.dedup();
                let full=rects.iter().filter(|(_,r)|(r.h-self.content_rect().h).abs()<1.0).count();
                let shape=match (rects.len(),tops.len(),full) {(2,1,2)=>"row",(3,2,1)=>"L",(4,2,0)=>"grid",(n,_,_)=>if n==0{"none"}else{"other"}};
                assert_eq!(shape,rest,"tile shape: have {shape} ({} tiles)",rects.len());
                eprintln!("TILES OK {rest}");
            }
            // `droptab 2 0.9 0.5`: tab 2 dragged by its row and let go at that
            // fraction of the content; `droppane right 0.5 0.95` the same for
            // the active tab's right (or left) pane. The zone must exist.
            "droptab" | "droppane" => {
                let mut w=rest.split_whitespace();
                let first=w.next().unwrap_or("");
                let n:Vec<f32>=w.filter_map(|v|v.parse().ok()).collect();
                let c=self.content_rect();
                let (x,y)=(c.x+c.w*n[0],c.y+c.h*n[1]);
                let what=if verb=="droptab" {crate::pane_mode::Dragging::Tab(first.parse().expect("droptab <tab index>"))} else {crate::pane_mode::Dragging::Pane{tab:self.active,right:first=="right"}};
                let d=self.drop_at(x,y,what).unwrap_or_else(||panic!("{verb} {rest}: no drop zone at ({x},{y}); content {c:?}; slots {:?}",self.slots()));
                eprintln!("DROP {} {:?}",d.words,d.zone);
                self.apply_drop(what,d);
            }
            // `send new` | `send other`: the focused pane (or tab) to a new
            // window, or to the first other window.
            "send" => {
                let dest=match rest {
                    "new"=>crate::send::Dest::New,
                    _=>crate::send::Dest::Window(self.other_windows().first().expect("send other: no other window").id),
                };
                self.send_focused(dest);
            }
            // `assertshell MOVED-7`: the focused shell's screen shows it.
            "assertshell" => {
                let text=match self.tabs[self.active].focused_ref() {crate::app::Pane::Term(t)=>t.term.grid().text(),_=>panic!("assertshell: not a shell")};
                assert!(text.contains(rest),"assertshell: {rest:?} not on screen:\n{text}");
                eprintln!("SHELL OK {rest}");
            }
            // `savelayout trio` / `openlayout trio`: profile/layouts/<name>.nus.luau,
            // as the palette's SAVE LAYOUT and the saved list do it.
            "savelayout" => self.run(crate::app::Action::SaveLayout(rest.into())),
            "openlayout" => {
                let path=std::env::current_dir().unwrap_or_default().join("profile/layouts").join(format!("{rest}.nus.luau"));
                let text=std::fs::read_to_string(&path).unwrap_or_else(|e|panic!("openlayout: {}: {e}",path.display()));
                eprintln!("LAYOUT FILE {rest}:\n{text}");
                self.run(crate::app::Action::OpenLayout(path.display().to_string()));
            }
            // `assertlayout tabs=2 active=1 split=yes left=term right=web solo=no width=400`
            "assertlayout" => {
                let t=&self.tabs[self.active];
                let kind=|p:&crate::app::Pane|match p{crate::app::Pane::Term(_)=>"term",crate::app::Pane::Web(_)=>"web",crate::app::Pane::Home(_)=>"home",crate::app::Pane::Hints(_)=>"welcome",crate::app::Pane::Settings(_)=>"settings",crate::app::Pane::Editor(_)=>"editor",crate::app::Pane::Ports(_)=>"ports",crate::app::Pane::Downloads(_)=>"downloads"};
                for pair in rest.split_whitespace() {
                    let (k,v)=pair.split_once('=').expect("assertlayout key=value");
                    let have=match k {
                        "tabs"=>self.tabs.iter().filter(|t|!t.hatch&&t.peek.is_none()).count().to_string(),
                        "active"=>self.active.to_string(),
                        "split"=>if t.right.is_some(){"yes".into()}else{"no".into()},
                        "left"=>kind(&t.left).into(),
                        "right"=>t.right.as_ref().map(kind).unwrap_or("none").into(),
                        "solo"=>if t.solo{"yes".into()}else{"no".into()},
                        "focus"=>if t.focus_right{"right".into()}else{"left".into()},
                        "width"=>t.split_w.map(|w|format!("{w:.0}")).unwrap_or("default".into()),
                        "tiled"=>self.tiled().len().to_string(),
                        "mode"=>if self.pane_mode{"on".into()}else{"off".into()},
                        other=>panic!("assertlayout: unknown key {other}"),
                    };
                    assert_eq!(have,v,"assertlayout {k}: have {have}, want {v}");
                }
                eprintln!("LAYOUT OK {rest}");
            }
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
            "f10" => named(NamedKey::F10, KeyCode::F10),
            "plus" => Some((WKey::Character("+".into()),KeyCode::Equal)),
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
        let rgba = if self.arriving() { self.gpu.snapshot_alpha((w, h), &self.scene, clear) }
            else { self.gpu.snapshot((w, h), &self.scene, clear) };
        let (x0, y0, cw, ch) = match crop {
            Some((x, y, cw, ch)) => (x.min(w - 1), y.min(h - 1), cw.max(1).min(w - x.min(w - 1)), ch.max(1).min(h - y.min(h - 1))),
            None => (0, 0, w, h),
        };
        let mut px = Vec::with_capacity((cw * ch * 4) as usize);
        for row in y0..y0 + ch {
            let start = ((row * w + x0) * 4) as usize;
            px.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
        }
        let mut encoded=Vec::new();
        let mut enc = png::Encoder::new(&mut encoded, cw, ch);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut wr| wr.write_image_data(&px)).map_err(|e| e.to_string())?;
        if path.starts_with(crate::replay::dir()) {crate::protected_state::write(path,&encoded).map_err(|e|e.to_string())?;}
        else {std::fs::write(path,&encoded).map_err(|e|e.to_string())?;}
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
