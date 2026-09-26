//! Remote control: a JSON-lines protocol on the instance port, in kitty's
//! shape. One request per line — `{"token": "…", "cmd": "ls", "args": {…}}`
//! — one reply per line — `{"ok": true, "result": …}` or `{"ok": false,
//! "error": "…"}`. The token is written beside the port in
//! `profile/instance` at launch; anything without it is refused. The
//! `nus` CLI (crates/cli) speaks it; rules can too, through `nus.run`.
//!
//! Verbs: ls · open · edit · launch · split · send-text · focus · close ·
//! theme · look · ports · hatch · block · ask · raise · version.

use std::sync::mpsc::Sender;

use serde_json::{json, Value};

use crate::app::{App, Pane};

/// Which door a request came through. Both are authenticated, but they are
/// not the same strength: the CLI's token is a 0600 file on this machine,
/// while the phone's travels in a URL to a device on the network and may be
/// read over someone's shoulder. So the phone gets the verbs its page needs
/// and nothing else, checked here rather than trusted to the caller.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// The instance socket: the `nus` CLI, launch handoff, rules.
    Cli,
    /// The page served to the phone.
    Phone,
}

impl Origin {
    /// Everything the phone's page is allowed to ask for. Adding a verb
    /// here gives a device on the network a new power, so the list is
    /// meant to be read in full before it grows.
    pub const PHONE_VERBS: [&'static str; 2] = ["front", "hands-answer"];

    pub fn allows(self, cmd: &str) -> bool {
        match self {
            Origin::Cli => true,
            Origin::Phone => Self::PHONE_VERBS.contains(&cmd),
        }
    }
}

/// A request from the wire, with where its answer goes.
pub struct Request {
    pub cmd: String,
    pub args: Value,
    pub reply: Sender<Value>,
    /// Which door it came through; see `Origin`.
    pub origin: Origin,
}

/// An answer that waits on the page: a CDP reply by id, or a capture
/// after the next draw.
pub struct Deferred {
    pub reply: Sender<Value>,
    pub what: DeferredWhat,
    pub since: std::time::Instant,
}

pub enum DeferredWhat {
    /// `(tab, right pane?, cdp id)` and how to shape the reply.
    Cdp(usize, bool, i32, Shape),
    /// The pane's pixels, cropped from the next frame: `(tab, right?)`.
    Shot(usize, bool),
}

#[derive(Clone, Copy)]
pub enum Shape {
    /// `result.result.value` as-is.
    Value,
    /// The reader's JSON: title, url, text and headings.
    Article,
}

/// The per-launch token: random, written with the port.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS randomness unavailable; refusing an insecure control token");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl App {
    /// Answer one request on the app's loop. Everything here is what the
    /// palette or a chord would do; nothing is only reachable this way.
    /// A request from the instance port: answered now, or parked in
    /// `deferred` until the page answers (a CDP reply, a capture).
    pub fn remote_request(&mut self, req: Request) {
        if crate::private::enabled() { let _ = req.reply.send(json!({"ok":false,"error":"Remote control is unavailable in incognito"})); return; }
        // The door decides what may be asked, before anything is done.
        if !req.origin.allows(&req.cmd) {
            let _ = req.reply.send(json!({"ok":false,"error":format!("{} is not available from the phone", req.cmd)}));
            return;
        }
        // Hands answer through the band, on their own time.
        if req.cmd == "hands" {
            return self.hands_request(&req.args, req.reply);
        }
        match self.remote(&req.cmd, &req.args) {
            Ok(v) if v.get("__deferred").is_some() => {
                // `remote` left the description of what to wait for.
                let what = self.take_deferred_what(&v);
                match what {
                    Some(what) => self.deferred.push(Deferred { reply: req.reply, what, since: crate::clock::now() }),
                    None => {
                        let _ = req.reply.send(json!({ "ok": false, "error": "nothing to wait for" }));
                    }
                }
            }
            Ok(mut result) => {
                // Credential-management verbs intentionally return a key to
                // the local user. Content-reading verbs always sanitize.
                if !(req.cmd=="sync" && req.args["do"]=="key") {crate::secrets::scrub_json(&mut result);}
                let _ = req.reply.send(json!({ "ok": true, "result": result }));
            }
            Err(e) => {
                let _ = req.reply.send(json!({ "ok": false, "error": crate::secrets::scrub(&e).text }));
            }
        }
    }

    fn take_deferred_what(&mut self, v: &Value) -> Option<DeferredWhat> {
        let d = v.get("__deferred")?;
        let tab = d.get("tab")?.as_u64()? as usize;
        let right = d.get("right").and_then(|r| r.as_bool()).unwrap_or(false);
        match d.get("kind")?.as_str()? {
            "cdp" => {
                let id = d.get("id")?.as_i64()? as i32;
                let shape = if d.get("shape").and_then(|s| s.as_str()) == Some("article") { Shape::Article } else { Shape::Value };
                Some(DeferredWhat::Cdp(tab, right, id, shape))
            }
            "shot" => Some(DeferredWhat::Shot(tab, right)),
            _ => None,
        }
    }

    /// Answer what the page has answered; give up after ten seconds.
    pub fn poll_deferred(&mut self) {
        if self.deferred.is_empty() {
            return;
        }
        let mut still = Vec::new();
        for d in std::mem::take(&mut self.deferred) {
            match &d.what {
                DeferredWhat::Cdp(tab, right, id, shape) => {
                    let reply = self.tabs.get(*tab).and_then(|t| if *right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| match p {
                        Pane::Web(w) => w.tab.take_reply(*id),
                        _ => None,
                    });
                    match reply {
                        Some(v) => {
                            let value = v.pointer("/result/value").cloned().unwrap_or(Value::Null);
                            let mut result = match shape {
                                Shape::Value => value,
                                Shape::Article => {
                                    let json = value.as_str().unwrap_or("");
                                    match crate::reader::Article::parse(json) {
                                        Some(a) => {
                                            use crate::reader::Block as B;
                                            let text: Vec<String> = a
                                                .blocks
                                                .iter()
                                                .map(|b| match b {
                                                    B::Heading(n, t) => format!("{} {t}", "#".repeat((*n).clamp(1, 6) as usize)),
                                                    B::Para(t) | B::Caption(t) => t.clone(),
                                                    B::Pre(t) => format!("```\n{t}\n```"),
                                                    B::Item(t) => format!("- {t}"),
                                                    B::Quote(t) => format!("> {t}"),
                                                    B::Image(alt, src) => format!("![{alt}]({src})"),
                                                    B::Link(text, url) => format!("[{text}]({url})"),
                                                })
                                                .collect();
                                            json!({ "title": a.title, "byline": a.byline, "when": a.when, "text": text.join("\n\n") })
                                        }
                                        None => json!({ "text": "" }),
                                    }
                                }
                            };
                            crate::secrets::scrub_json(&mut result);
                            let _ = d.reply.send(json!({ "ok": true, "result": result }));
                        }
                        None if crate::clock::since(d.since).as_secs() > 10 => {
                            let _ = d.reply.send(json!({ "ok": false, "error": "the page did not answer" }));
                        }
                        None => still.push(d),
                    }
                }
                DeferredWhat::Shot(..) => still.push(d),
            }
        }
        self.deferred = still;
    }

    /// The web pane a request means: `tab` (1-based) or the active tab; the
    /// page on either side of it.
    fn page_pane(&self, args: &Value) -> Result<(usize, bool), String> {
        let i = args.get("tab").and_then(|t| t.as_u64()).map(|t| (t as usize).saturating_sub(1)).unwrap_or(self.active);
        let tab = self.tabs.get(i).ok_or("no such tab")?;
        if matches!(tab.left, Pane::Web(_)) {
            return Ok((i, false));
        }
        if matches!(tab.right, Some(Pane::Web(_))) {
            return Ok((i, true));
        }
        Err("no page on that tab".into())
    }

    fn web_pane(&self, tab: usize, right: bool) -> Option<&crate::app::WebPane> {
        self.tabs.get(tab).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }).and_then(|p| match p {
            Pane::Web(w) => Some(w),
            _ => None,
        })
    }

    pub fn remote(&mut self, cmd: &str, args: &Value) -> Result<Value, String> {
        if crate::private::enabled() { return Err("Remote control is unavailable in incognito".into()); }
        let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |k: &str| args.get(k).and_then(Value::as_u64).map(|v| v as usize);
        let b = |k: &str| args.get(k).and_then(Value::as_bool).unwrap_or(false);
        match cmd {
            "version" => Ok(json!({ "nus": crate::updates::CURRENT })),
            // git's credential helper (`nus credential get`): only when the
            // setting lets git use the sign-in, only over https, only for
            // the signed-in forge's host.
            "credential" => {
                let host = s("host").unwrap_or_default();
                let https = s("protocol").as_deref() == Some("https");
                match (self.behavior.forge_git && https).then(|| crate::forge::credential_for(&host)).flatten() {
                    Some((username, password)) => Ok(json!({ "username": username, "password": password })),
                    None => Ok(json!({})),
                }
            }
            "raise" => {
                self.hatch_state.main_hidden = false;
                self.window.set_visible(true);
                self.window.focus_window();
                Ok(Value::Null)
            }
            "ls" => {
                let tabs: Vec<Value> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.peek.is_none())
                    .map(|(i, t)| {
                        let pane = |p: &Pane| match p {
                            Pane::Term(t) => json!({ "kind": "shell", "title": t.title, "cwd": t.term.cwd, "pid": t.pty.pid(), "profile": self.profiles.get(t.profile).map(|p| p.name.clone()) }),
                            Pane::Web(w) => {
                                let sh = w.tab.shared.borrow();
                                json!({ "kind": "page", "title": sh.title, "url": sh.url })
                            }
                            Pane::Editor(e) => json!({ "kind": "editor", "title": e.title(), "path": e.buf().and_then(|b| b.path.as_ref()).map(|p| p.display().to_string()) }),
                            Pane::Settings(_) => json!({ "kind": "settings" }),
                            Pane::Hints(_) => json!({ "kind": "welcome" }),
                            Pane::Home(_) => json!({ "kind": "home" }),
                            Pane::Ports(_) => json!({ "kind": "ports" }),
            Pane::Downloads(_) => json!({ "kind": "downloads" }),
                        };
                        json!({
                            "index": i + 1,
                            "id": t.id,
                            "active": i == self.active,
                            "label": self.tab_label(i),
                            "title": t.title(),
                            "name": t.name,
                            "pinned": t.pinned,
                            "hatch": t.hatch,
                            "left": pane(&t.left),
                            "right": t.right.as_ref().map(pane),
                        })
                    })
                    .collect();
                Ok(json!({ "space": self.space_name, "signal": crate::surface::hex(self.surface.signal), "theme": if self.theme.mode == nus_render::Mode::Ink { "ink" } else { "paper" }, "tabs": tabs }))
            }
            "open" => {
                let Some(url) = s("url") else { return Err("open needs url".into()) };
                if url.ends_with(".nus.luau") && std::path::Path::new(&url).is_file() {
                    self.open_layout(std::path::Path::new(&url));
                    return Ok(json!({ "tab": self.active + 1, "layout": true }));
                }
                let new_tab = !b("split");
                if new_tab {
                    self.open_url_by_other(&url);
                } else {
                    self.open_url(&url, false);
                }
                Ok(json!({ "tab": self.active + 1 }))
            }
            "sync" => {
                match s("do").as_deref().unwrap_or("now") {
                    "now" => {
                        self.sync_now();
                        Ok(json!({ "started": true }))
                    }
                    "key" => Ok(json!({ "key": crate::syncui::make_key() })),
                    "join" => {
                        let Some(w) = s("key") else { return Err("join needs key".into()) };
                        if nus_sync::decode_key(&w).is_none() {
                            return Err("that isn't a nus key".into());
                        }
                        crate::syncui::write_key(&w);
                        self.sync_now();
                        Ok(Value::Null)
                    }
                    "status" => Ok(json!({ "status": self.sync_status(), "ready": self.sync_ready() })),
                    "folder" => {
                        self.behavior.sync_folder = s("path").unwrap_or_default();
                        self.save_prefs();
                        Ok(Value::Null)
                    }
                    "git" => {
                        self.behavior.sync_git = s("remote").unwrap_or_default();
                        self.save_prefs();
                        Ok(Value::Null)
                    }
                    other => Err(format!("sync: now · key · join · status · folder · git, not {other}")),
                }
            }
            "layout" => {
                match s("save") {
                    Some(name) => {
                        self.save_layout(&name);
                        Ok(Value::Null)
                    }
                    None => Ok(json!({ "layouts": crate::layout_file::saved().iter().map(|(n, p)| json!({ "name": n, "path": p.display().to_string() })).collect::<Vec<_>>(), "current": crate::layout_file::render(&self.current_layout()) })),
                }
            }
            "edit" => {
                let Some(p) = s("path") else { return Err("edit needs path".into()) };
                self.open_file(std::path::Path::new(&p), b("split"));
                Ok(json!({ "tab": self.active + 1 }))
            }
            "launch" => {
                let profile = s("profile").and_then(|n| self.profiles.iter().position(|p| p.name.eq_ignore_ascii_case(&n))).unwrap_or(self.behavior.default_profile);
                if b("split") {
                    match self.new_term_pane(true, profile) {
                        Ok(mut t) => {
                            if let Some(cwd) = s("cwd") {
                                let _ = t.pty.write(format!("cd \"{cwd}\"\r").as_bytes());
                            }
                            if let Some(run) = s("run") {
                                let _ = t.pty.write(format!("{run}\r").as_bytes());
                            }
                            let tab = &mut self.tabs[self.active];
                            tab.right = Some(Pane::Term(t));
                            tab.focus_right = true;
                            self.apply_term_resizes(false);
                            self.layout();
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                } else {
                    self.new_tab(profile);
                    if let Some(Pane::Term(t)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                        if let Some(cwd) = s("cwd") {
                            let _ = t.pty.write(format!("cd \"{cwd}\"\r").as_bytes());
                        }
                        if let Some(run) = s("run") {
                            let _ = t.pty.write(format!("{run}\r").as_bytes());
                        }
                    }
                }
                self.dirty = true;
                Ok(json!({ "tab": self.active + 1 }))
            }
            "split" => {
                self.toggle_split();
                Ok(Value::Null)
            }
            "ssh" => {
                let Some(host) = s("host") else { return Err("ssh needs host".into()) };
                // A profile for this host, added if ~/.ssh/config didn't have it.
                let name = format!("ssh:{host}");
                let idx = match self.profiles.iter().position(|p| p.name == name) {
                    Some(i) => i,
                    None => {
                        self.profiles.push(nus_pty::Profile::ssh(&host));
                        self.profiles.len() - 1
                    }
                };
                if b("split") {
                    match self.new_term_pane(true, idx) {
                        Ok(t) => {
                            let tab = &mut self.tabs[self.active];
                            tab.right = Some(Pane::Term(t));
                            tab.focus_right = true;
                            self.apply_term_resizes(false);
                            self.layout();
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                } else {
                    self.new_tab(idx);
                }
                self.dirty = true;
                Ok(json!({ "tab": self.active + 1 }))
            }
            "send-text" => {
                let Some(text) = s("text") else { return Err("send-text needs text".into()) };
                let i = n("tab").map(|t| t.saturating_sub(1)).unwrap_or(self.active);
                let Some(tab) = self.tabs.get_mut(i) else { return Err("no such tab".into()) };
                let pane = if b("right") { tab.right.as_mut() } else { Some(&mut tab.left) };
                match pane {
                    Some(Pane::Term(t)) => {
                        let text = if b("enter") { format!("{text}\r") } else { text };
                        let _ = t.pty.write(text.as_bytes());
                        Ok(Value::Null)
                    }
                    _ => Err("that pane is not a shell".into()),
                }
            }
            "focus" => {
                let Some(i) = n("tab").map(|t| t.saturating_sub(1)) else { return Err("focus needs tab".into()) };
                if i >= self.tabs.len() {
                    return Err("no such tab".into());
                }
                self.activate(i);
                self.window.focus_window();
                Ok(Value::Null)
            }
            "close" => {
                if let Some(i) = n("tab").map(|t| t.saturating_sub(1)) {
                    if i >= self.tabs.len() {
                        return Err("no such tab".into());
                    }
                    self.activate(i);
                }
                self.close_tabs(b("force"));
                Ok(Value::Null)
            }
            "theme" => {
                let stock = crate::themes::stock();
                let Some(name) = s("name") else {
                    return Ok(json!({ "themes": stock.iter().map(|t| t.name.clone()).collect::<Vec<_>>() }));
                };
                match stock.iter().find(|t| t.name.eq_ignore_ascii_case(&name)) {
                    Some(t) => {
                        let t = t.clone();
                        self.apply_theme(&t);
                        self.save_prefs();
                        Ok(Value::Null)
                    }
                    None => Err(format!("no theme named {name}")),
                }
            }
            "look" => {
                let mode = s("mode");
                match mode.as_deref() {
                    Some("ink") => self.set_mode(nus_render::Mode::Ink),
                    Some("paper") => self.set_mode(nus_render::Mode::Paper),
                    Some(other) => return Err(format!("mode is ink or paper, not {other}")),
                    None => {}
                }
                if let Some(sig) = s("signal").and_then(|h| crate::surface::parse_hex(&h)) {
                    self.surface.signal = sig;
                    self.rebuild_theme();
                    self.refresh_icon();
                }
                self.save_prefs();
                self.dirty = true;
                Ok(Value::Null)
            }
            "ports" => {
                let rows: Vec<Value> = self
                    .board
                    .rows
                    .iter()
                    .map(|r| json!({ "port": r.port, "pid": r.pid, "process": r.process, "group": format!("{:?}", r.group).to_lowercase(), "exposed": r.exposed, "tab": r.tab, "title": r.title(), "url": r.url() }))
                    .collect();
                Ok(json!({ "ports": rows }))
            }
            "hatch" => {
                match s("do").as_deref().unwrap_or("toggle") {
                    "toggle" => {let _=self.proxy.send_event(crate::UserEvent::Hatch);},
                    "show" => {let _=self.proxy.send_event(crate::UserEvent::HatchShow);},
                    "work" => { let _=self.proxy.send_event(crate::UserEvent::HatchWork); return Ok(json!({"work": self.hatch_state.work})); },
                    "list" => return Ok(json!({"work": self.hatch_state.work})),
                    "open" => {
                        let tab = args.get("tab_id").and_then(Value::as_u64).ok_or("hatch open requires --tab-id from hatch list")?;
                        let window = args.get("window").and_then(Value::as_u64).unwrap_or(u64::from(self.window.id()));
                        let target = crate::hatch_work::Target {window,tab,right:b("right")};
                        if !self.hatch_state.work.iter().any(|i|i.target==target) {return Err("That session is no longer available".into());}
                        let _ = self.proxy.send_event(crate::UserEvent::HatchSelect(target));
                    }
                    "quit" => { let _=self.proxy.send_event(crate::UserEvent::HatchQuit); },
                    "hide" => {let _=self.proxy.send_event(crate::UserEvent::HatchHide);},
                    "hoist" => self.hoist(),
                    "land" => self.land(),
                    other => return Err(format!("hatch: toggle · show · hide · work · list · open · hoist · land · quit, not {other}")),
                }
                Ok(Value::Null)
            }
            "block" => {
                let i = n("tab").map(|t| t.saturating_sub(1)).unwrap_or(self.active);
                let Some(tab) = self.tabs.get(i) else { return Err("no such tab".into()) };
                let Pane::Term(t) = &tab.left else { return Err("that tab is not a shell".into()) };
                let blocks = t.blocks();
                let which = s("which").unwrap_or_else(|| "last".into());
                let picked: Vec<&crate::blocks::Block> = match which.as_str() {
                    "all" => blocks.iter().collect(),
                    _ => blocks.last().into_iter().collect(),
                };
                let out: Vec<Value> = picked
                    .into_iter()
                    .map(|b| json!({ "cmd": b.cmd, "exit": b.exit, "lines": b.lines(), "running": b.running, "output": t.block_output_text(b.start) }))
                    .collect();
                Ok(json!({ "blocks": out }))
            }
            // The phone's front page: what the prompt's news says, what listens, who asks for hands, the tabs.
            "front" => {
                let news: Vec<Value> = self.news_rows().iter().map(|r| json!({ "mark": r.num, "text": r.text })).collect();
                let ports: Vec<Value> = self.ports.iter().filter(|p| p.port >= 1024).map(|p| json!({ "mark": "☍", "text": if p.process.is_empty() { format!("localhost:{}", p.port) } else { format!("localhost:{} · {}", p.port, p.process.to_lowercase()) } })).collect();
                let tabs: Vec<Value> = self.tabs.iter().enumerate().filter(|(_, t)| t.peek.is_none() && !t.hatch).map(|(i, t)| json!({ "mark": format!("{}", i + 1), "text": t.title() })).collect();
                let mut hands = Vec::new();
                for (i, t) in self.tabs.iter().enumerate() {
                    for (right, p) in [(false, Some(&t.left)), (true, t.right.as_ref())] {
                        if let Some(Pane::Web(w)) = p {
                            if let Some(a) = &w.hands.ask {
                                hands.push(json!({ "tab": i, "right": right, "who": a.who, "what": a.what.label() }));
                            }
                        }
                    }
                }
                Ok(json!({ "name": self.window_name(), "news": news, "ports": ports, "tabs": tabs, "hands": hands }))
            }
            "hands-answer" => {
                let tab = n("tab").ok_or("hands-answer needs tab")?;
                let right = b("right");
                let a = match s("answer").as_deref() { Some("allow") => crate::hands::Answer::Allow, Some("host") => crate::hands::Answer::AllowHost, _ => crate::hands::Answer::Deny };
                self.hands_answer(tab, right, a);
                Ok(Value::Null)
            }
            "ask" => {
                let Some(q) = s("q") else { return Err("ask needs q".into()) };
                self.ask_from_remote(&q);
                Ok(Value::Null)
            }
            // Eyes: the page beside the shell, as the assistant reads it.
            "page" => {
                let what = s("what").unwrap_or_else(|| "text".into());
                match what.as_str() {
                    "open" => {
                        let url = s("url").ok_or("open needs a url")?;
                        let beside = args.get("beside").and_then(|b| b.as_bool()).unwrap_or(true);
                        if beside {
                            self.open_url(&url, false);
                        } else {
                            self.open_url_by_other(&url);
                        }
                        Ok(json!({ "opened": url }))
                    }
                    "text" | "dom" | "console" | "network" | "screenshot" | "info" => {
                        let (tab, right) = self.page_pane(args)?;
                        let w = self.web_pane(tab, right).ok_or("no page")?;
                        match what.as_str() {
                            "info" => {
                                let sh = w.tab.shared.borrow();
                                Ok(json!({ "tab": tab + 1, "url": sh.url, "title": sh.title, "loading": sh.loading }))
                            }
                            "text" => {
                                let id = w.tab.eval_reply(crate::reader::EXTRACT_JS);
                                Ok(json!({ "__deferred": { "kind": "cdp", "tab": tab, "right": right, "id": id, "shape": "article" } }))
                            }
                            "dom" => {
                                let sel = s("selector").unwrap_or_else(|| "body".into());
                                let expr = format!("(function(){{ const n = document.querySelector({}); return n ? n.outerHTML.slice(0, 200000) : null; }})()", serde_json::to_string(&sel).unwrap_or_default());
                                let id = w.tab.eval_reply(&expr);
                                Ok(json!({ "__deferred": { "kind": "cdp", "tab": tab, "right": right, "id": id, "shape": "value" } }))
                            }
                            "console" | "network" => {
                                let want_console = what == "console";
                                let sh = w.tab.shared.borrow();
                                let n = n("limit").unwrap_or(100);
                                let rows: Vec<Value> = sh.log.iter().rev().filter(|e| (e.get("kind").and_then(|k| k.as_str()) == Some("console")) == want_console).take(n).cloned().collect();
                                Ok(json!({ "entries": rows.into_iter().rev().collect::<Vec<_>>() }))
                            }
                            _ => Ok(json!({ "__deferred": { "kind": "shot", "tab": tab, "right": right } })),
                        }
                    }
                    other => Err(format!("page: text · dom · console · network · screenshot · info · open, not {other}")),
                }
            }
            // Share: the tab as one HTML file that replays anywhere.
            "share" => {
                let i = n("tab").map(|t| t.saturating_sub(1)).unwrap_or(self.active);
                let path = self.share_replay(i)?;
                Ok(json!({ "path": path.display().to_string() }))
            }
            // Held shells: ls · attach <id> · kill <id>.
            "hold" => {
                let dir = crate::app::App::hold_dir();
                match s("what").as_deref().unwrap_or("ls") {
                    "ls" => {
                        let loose: Vec<String> = self.held_loose().into_iter().map(|i| i.id).collect();
                        let all: Vec<Value> = nus_pty::hold::Info::all(&dir)
                            .into_iter()
                            .map(|i| json!({ "id": i.id, "pid": i.pid, "program": i.program, "cwd": i.cwd, "started": i.started, "attached": !loose.contains(&i.id) }))
                            .collect();
                        Ok(json!({ "held": all }))
                    }
                    "attach" => {
                        let id = s("id").ok_or("attach needs an id")?;
                        let info = nus_pty::hold::Info::read(&dir, &id).ok_or("no such holder")?;
                        if !info.alive(&dir) {
                            return Err("that holder is gone".into());
                        }
                        self.attach_held(info);
                        Ok(json!({ "attached": id }))
                    }
                    "kill" => {
                        let id = s("id").ok_or("kill needs an id")?;
                        let info = nus_pty::hold::Info::read(&dir, &id).ok_or("no such holder")?;
                        // Attach briefly to say kill; the holder exits with its child.
                        match nus_pty::hold::Client::attach(info, || {}) {
                            Ok(mut c) => {
                                c.kill();
                                Ok(json!({ "killed": id }))
                            }
                            Err(e) => Err(format!("kill: {e}")),
                        }
                    }
                    other => Err(format!("hold: ls · attach · kill, not {other}")),
                }
            }
            // The journal: what ran in a folder (the focused shell's by default).
            "log" => {
                let cwd = s("cwd").or_else(|| self.focused_cwd()).unwrap_or_default();
                let limit = n("limit").unwrap_or(50);
                let out: Vec<Value> = crate::journal::entries(&cwd, limit)
                    .into_iter()
                    .map(|e| json!({ "cmd": e.cmd, "cwd": e.cwd, "start": e.start, "ms": e.ms, "exit": e.exit, "tab": e.tab, "shell": e.shell }))
                    .collect();
                Ok(json!({ "cwd": cwd, "entries": out, "folders": crate::journal::folders().into_iter().map(|(c, n)| json!({ "cwd": c, "commands": n })).collect::<Vec<_>>() }))
            }
            other => Err(format!("unknown command {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Origin;

    /// The phone's page needs two verbs. Everything that reaches the shell,
    /// the filesystem, the assistant or the pages has to stay behind the
    /// CLI's door, whatever a future endpoint happens to send.
    #[test]
    fn the_phone_reaches_only_its_own_page() {
        for cmd in Origin::PHONE_VERBS {
            assert!(Origin::Phone.allows(cmd), "the phone's page needs {cmd}");
        }
        for cmd in [
            "launch", "send-text", "ask", "edit", "open", "page", "block",
            "hatch", "sync", "layout", "ls", "raise", "close", "split",
            "focus", "theme", "look", "ports", "version", "hands", "credential",
        ] {
            assert!(!Origin::Phone.allows(cmd), "the phone could reach {cmd}");
            assert!(Origin::Cli.allows(cmd), "the CLI lost {cmd}");
        }
    }
}
