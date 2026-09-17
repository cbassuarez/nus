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

/// A request from the wire, with where its answer goes.
pub struct Request {
    pub cmd: String,
    pub args: Value,
    pub reply: Sender<Value>,
}

/// The per-launch token: random, written with the port.
pub fn new_token() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id() as u128;
    let mut x = t ^ (pid << 64) ^ 0x9e37_79b9_7f4a_7c15_9e37_79b9_7f4a_7c15u128;
    let mut out = String::new();
    for _ in 0..24 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let c = b"abcdefghijklmnopqrstuvwxyz0123456789"[(x % 36) as usize];
        out.push(c as char);
    }
    out
}

impl App {
    /// Answer one request on the app's loop. Everything here is what the
    /// palette or a chord would do; nothing is only reachable this way.
    pub fn remote(&mut self, cmd: &str, args: &Value) -> Result<Value, String> {
        let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
        let n = |k: &str| args.get(k).and_then(Value::as_u64).map(|v| v as usize);
        let b = |k: &str| args.get(k).and_then(Value::as_bool).unwrap_or(false);
        match cmd {
            "version" => Ok(json!({ "nus": env!("CARGO_PKG_VERSION") })),
            "raise" => {
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
                            Pane::Ports(_) => json!({ "kind": "ports" }),
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
                self.open_url(&url, new_tab);
                Ok(json!({ "tab": self.active + 1 }))
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
                    "toggle" => self.toggle_hatch(),
                    "show" => self.show_hatch(),
                    "hide" => self.hide_hatch(),
                    "hoist" => self.hoist(),
                    "land" => self.land(),
                    other => return Err(format!("hatch: toggle · show · hide · hoist · land, not {other}")),
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
            "ask" => {
                let Some(q) = s("q") else { return Err("ask needs q".into()) };
                self.ask_from_remote(&q);
                Ok(Value::Null)
            }
            other => Err(format!("unknown command {other}")),
        }
    }
}
