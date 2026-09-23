//! Language servers on the app: one per (server, workspace root), spawned
//! on the first file that wants it, drained once a loop. Requests from
//! the editor are remembered by id; answers are routed to the buffer they
//! were about. Also the FILES folder in the sidebar: the active file's
//! project tree, dirs expanding on click.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use nus_lsp::lsp_types::{self as lt, Url};
use nus_lsp::{Client, Event, RequestId};

use crate::app::{App, Pane};
use crate::editor::{buffer_with, Completion, HoverBox, Pending};
use crate::folders::{Folder, Item, Kind};

pub struct Server {
    pub client: Client,
    pub rx: Receiver<Event>,
    pub status: String,
    pub log: Vec<String>,
    pub gone: bool,
}

#[derive(Default)]
pub struct Servers {
    /// By `command@root`.
    pub map: HashMap<String, Server>,
    pub pending: HashMap<(String, RequestId), Pending>,
    /// Servers we tried and couldn't start, with why (shown once).
    pub failed: HashMap<String, String>,
}

/// Where the profile's fetched tools live.
pub fn bin_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("bin")
}

/// PowerShell Editor Services: the bundle unzips to bin/pses; the server
/// is its Start-EditorServices.ps1 run by pwsh (or Windows PowerShell).
fn pses_launch() -> Option<(PathBuf, Vec<String>)> {
    let dir = bin_dir().join("pses");
    let script = ["PowerShellEditorServices/Start-EditorServices.ps1", "Start-EditorServices.ps1"].iter().map(|s| dir.join(s)).find(|p| p.is_file())?;
    let host = nus_lsp::registry::resolve("pwsh", None).or_else(|| nus_lsp::registry::resolve("powershell", None))?;
    let profile = std::env::current_dir().unwrap_or_default().join("profile");
    let args: Vec<String> = [
        "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", &script.display().to_string(),
        "-Stdio", "-HostName", "nus", "-HostProfileId", "nus", "-HostVersion", env!("CARGO_PKG_VERSION"),
        "-BundledModulesPath", &dir.display().to_string(),
        "-LogPath", &profile.join("pses.log").display().to_string(),
        "-SessionDetailsPath", &profile.join("pses-session.json").display().to_string(),
        "-LogLevel", "None",
    ].iter().map(|s| s.to_string()).collect();
    Some((host, args))
}

impl App {
    /// The server key for a file, starting it if need be.
    fn lsp_key_for(&mut self, path: &Path) -> Option<String> {
        if crate::protected_state::is_private_path(path){return None;}
        let server = nus_lsp::registry::server_for(path)?;
        let root = nus_lsp::registry::root_for(server, path);
        self.lsp_key_for_server(server, &root, false)
    }

    /// The key for `server` at `root`, spawning it on first use. `quiet`
    /// keeps a missing server out of the notices (the prompt line asks
    /// every shell).
    pub(crate) fn lsp_key_for_server(&mut self, server: &nus_lsp::registry::Server, root: &Path, quiet: bool) -> Option<String> {
        let key = format!("{}@{}", server.command, root.display());
        if self.lsp.map.contains_key(&key) {
            return Some(key);
        }
        if self.lsp.failed.contains_key(&key) {
            return None;
        }
        let missing = format!("{} not installed · GET it on the welcome page", server.command);
        let (bin, args): (PathBuf, Vec<String>) = if server.command == "powershell-editor-services" {
            // A script, not a binary: Start-EditorServices.ps1 through pwsh.
            match pses_launch() {
                Some(v) => v,
                None => {
                    self.lsp.failed.insert(key.clone(), missing.clone());
                    if !quiet {
                        self.notice(&missing);
                    }
                    return None;
                }
            }
        } else {
            match nus_lsp::registry::resolve(server.command, Some(&bin_dir())) {
                Some(bin) => (bin, server.args.iter().map(|s| s.to_string()).collect()),
                None => {
                    self.lsp.failed.insert(key.clone(), missing.clone());
                    if !quiet {
                        self.notice(&missing);
                    }
                    return None;
                }
            }
        };
        match Client::spawn(server.command, &bin, &args, root) {
            Ok((client, rx)) => {
                self.lsp.map.insert(key.clone(), Server { client, rx, status: "starting".into(), log: Vec::new(), gone: false });
                Some(key)
            }
            Err(e) => {
                self.lsp.failed.insert(key.clone(), e.to_string());
                if !quiet {
                    self.notice(&format!("{}: {e}", server.command));
                }
                None
            }
        }
    }

    /// A buffer was opened in an editor pane: tell its server.
    pub(crate) fn lsp_open_buffer(&mut self, ti: usize, right: bool, bi: usize) {
        let (path, uri, language, text, already) = {
            let Some(tab) = self.tabs.get(ti) else { return };
            let pane = if right {
                tab.right.as_ref()
            } else {
                Some(&tab.left)
            };
            let Some(Pane::Editor(e)) = pane else { return };
            let Some(b) = e.buffers.get(bi) else { return };
            let (Some(p), Some(u)) = (b.path.clone(), b.uri.clone()) else {
                return;
            };
            if !b.ready() || b.text.len_bytes() > 8 * 1024 * 1024 { return; }
            (p, u, b.language, b.text.clone(), b.in_lsp)
        };
        if already {
            return;
        }
        let Some(key) = self.lsp_key_for(&path) else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(ti) {
            let pane = if right { tab.right.as_mut() } else { Some(&mut tab.left) };
            if let Some(Pane::Editor(e)) = pane { if let Some(b) = e.buffers.get_mut(bi) { b.lsp_key = Some(key.clone()); } }
        }
        let Some(s) = self.lsp.map.get(&key) else { return; };
        if s.client.is_ready() {
            s.client.did_open_rope(uri, language, text);
            if let Some(tab) = self.tabs.get_mut(ti) {
                let pane = if right {
                    tab.right.as_mut()
                } else {
                    Some(&mut tab.left)
                };
                if let Some(Pane::Editor(e)) = pane {
                    if let Some(b) = e.buffers.get_mut(bi) {
                        b.in_lsp = true;
                        b.synced_revision = b.revision;
                    }
                }
            }
        }
        // Not ready yet: poll_lsp opens every unopened buffer on Initialized.
    }

    /// The key and client for the focused editor's active buffer.
    fn lsp_for_focused(&mut self) -> Option<(String, Url)> {
        let e = self.focused_editor()?;
        let b = e.buf()?;
        if !b.ready() || b.text.len_bytes() > 8 * 1024 * 1024 { return None; }
        Some((b.lsp_key.clone()?, b.uri.clone()?))
    }

    /// Push the buffer's text to the server if it changed.
    pub(crate) fn editor_synced(&mut self) {
        let Some((key, uri)) = self.lsp_for_focused() else {
            return;
        };
        let text = {
            let Some(e) = self.focused_editor() else {
                return;
            };
            let Some(b) = e.buf_mut() else { return };
            if !b.in_lsp || b.revision == b.synced_revision {
                return;
            }
            b.synced_revision = b.revision;
            b.text.clone()
        };
        if let Some(s) = self.lsp.map.get(&key) {
            s.client.did_change_rope(uri, text);
        }
    }

    /// Send a request about the caret: hover, completion, definition, format.
    pub(crate) fn editor_request(&mut self, kind: &str) {
        let Some((key, uri)) = self.lsp_for_focused() else {
            return;
        };
        let (pos, at, ready) = {
            let Some(e) = self.focused_editor() else {
                return;
            };
            let Some(b) = e.buf() else { return };
            (crate::editor_work::position(&b.text, b.cursor), b.cursor, b.in_lsp)
        };
        if !ready {
            return;
        }
        let Some(s) = self.lsp.map.get(&key) else {
            return;
        };
        let caps = s.client.capabilities.lock().ok().and_then(|c| c.clone());
        let (id, pending) = match kind {
            "hover" => (s.client.hover(uri.clone(), pos), Pending::Hover { uri, at }),
            "completion" => {
                if caps
                    .as_ref()
                    .is_some_and(|c| c.completion_provider.is_none())
                {
                    return;
                }
                (
                    s.client.completion(uri.clone(), pos, None),
                    Pending::Completion { uri },
                )
            }
            "definition" => (
                s.client.definition(uri.clone(), pos),
                Pending::Definition { uri },
            ),
            "format" => {
                if caps
                    .as_ref()
                    .is_some_and(|c| c.document_formatting_provider.is_none())
                {
                    self.notice("this server does not format");
                    return;
                }
                (
                    s.client.formatting(uri.clone(), 4, true),
                    Pending::Format {
                        uri,
                        then_save: false,
                    },
                )
            }
            _ => return,
        };
        self.lsp.pending.insert((key, id), pending);
    }

    /// Hover at a char index (the pointer's rest), not the caret.
    pub(crate) fn editor_hover_at(&mut self, at: usize) {
        let Some((key, uri)) = self.lsp_for_focused() else {
            return;
        };
        let (pos, ready) = {
            let Some(e) = self.focused_editor() else {
                return;
            };
            let Some(b) = e.buf() else { return };
            (crate::editor_work::position(&b.text, at), b.in_lsp)
        };
        if !ready {
            return;
        }
        let Some(s) = self.lsp.map.get(&key) else {
            return;
        };
        if s.client
            .capabilities
            .lock()
            .ok()
            .and_then(|c| c.clone())
            .is_some_and(|c| c.hover_provider.is_none())
        {
            return;
        }
        let id = s.client.hover(uri.clone(), pos);
        self.lsp
            .pending
            .insert((key, id), Pending::Hover { uri, at });
    }

    /// Ctrl+S: format through the server when it can, then write.
    pub(crate) fn editor_save(&mut self) {
        if self.focused_editor().and_then(|e| e.buf()).is_some_and(|b| !b.ready()) { return; }
        let can_format = self.lsp_for_focused().and_then(|(key, uri)| {
            let s = self.lsp.map.get(&key)?;
            let caps = s.client.capabilities.lock().ok()?.clone()?;
            let ready = self.tabs.get(self.active).and_then(|t| {
                let e = match t.focused_ref() {
                    Pane::Editor(e) => e,
                    _ => return None,
                };
                e.buf().map(|b| b.in_lsp)
            })?;
            (caps.document_formatting_provider.is_some() && ready && self.behavior.format_on_save)
                .then(|| {
                    let id = s.client.formatting(uri.clone(), 4, true);
                    (key, uri, id)
                })
        });
        match can_format {
            Some((key, uri, id)) => {
                self.lsp.pending.insert(
                    (key, id),
                    Pending::Format {
                        uri,
                        then_save: true,
                    },
                );
                if let Some(b) = self.focused_editor().and_then(|e| e.buf_mut()) {
                    b.save_pending = Some(crate::clock::now());
                }
            }
            None => self.editor_write(),
        }
    }

    /// Write the active buffer to disk.
    pub(crate) fn editor_write(&mut self) {
        let written = {
            let Some(e) = self.focused_editor() else {
                return;
            };
            let Some(b) = e.buf_mut() else { return };
            if !b.ready() { return; }
            let Some(path) = b.path.clone() else { return };
            let text = b.text.to_string();
            match if crate::protected_state::is_private_path(&path){crate::protected_state::write(&path,text.as_bytes())}else{std::fs::write(&path,text.as_bytes())} {
                Ok(()) => {
                    b.dirty = false;
                    b.save_pending = None;
                    Ok((path, b.uri.clone(), text))
                }
                Err(err) => Err(format!("could not save · {err}")),
            }
        };
        match written {
            Ok((path, uri, text)) => {
                self.notice(&format!(
                    "saved · {}",
                    path.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
                if let (Some(key), Some(uri)) = (self.lsp_key_for(&path), uri) {
                    if let Some(s) = self.lsp.map.get(&key) {
                        s.client.did_save(uri, Some(&text));
                    }
                }
                self.play_event("toggle");
            }
            Err(e) => self.notice(&e),
        }
    }

    /// Closed documents must leave the server too. Servers with no clients
    /// are dropped, releasing their subprocess, pipes, diagnostics and index.
    pub(crate) fn trim_language_servers(&mut self) {
        let mut live: HashMap<String, HashSet<Url>> = HashMap::new();
        for pane in self.tabs.iter().flat_map(|t|std::iter::once(&t.left).chain(t.right.as_ref())) {
            match pane {
                Pane::Term(t) => if let Some(l)=&t.plsp {live.entry(l.key.clone()).or_default().insert(l.uri.clone());},
                Pane::Editor(e) => for buffer in &e.buffers {
                    if let (Some(path),Some(uri))=(&buffer.path,&buffer.uri) {
                        if let Some(server)=nus_lsp::registry::server_for(path) {
                            let root=nus_lsp::registry::root_for(server,path);
                            live.entry(format!("{}@{}",server.command,root.display())).or_default().insert(uri.clone());
                        }
                    }
                },
                _=>{}
            }
        }
        self.lsp.map.retain(|key,server| {
            if let Some(uris)=live.get(key) {server.client.retain_documents(uris);true} else {false}
        });
        self.lsp.pending.retain(|(key,_),_|self.lsp.map.contains_key(key));
        if self.lsp.pending.len()>512 {
            let mut keys:Vec<_>=self.lsp.pending.keys().cloned().collect();
            keys.sort_by_key(|(_,id)|*id);
            for key in keys.into_iter().take(self.lsp.pending.len()-512) {self.lsp.pending.remove(&key);}
        }
        if self.lsp.failed.len()>128 {self.lsp.failed.clear();}
    }

    /// Once a loop: drain every server's events.
    pub(crate) fn poll_lsp(&mut self) {
        let keys: Vec<String> = self.lsp.map.keys().cloned().collect();
        let mut dirty = false;
        for key in keys {
            let mut events = Vec::new();
            if let Some(s) = self.lsp.map.get(&key) {
                for _ in 0..32 {
                    let Ok(ev)=s.rx.try_recv() else {break;};
                    events.push(ev);
                }
            }
            for ev in events {
                dirty = true;
                match ev {
                    Event::Initialized(caps) => {
                        if let Some(s) = self.lsp.map.get_mut(&key) {
                            s.client.initialized(*caps);
                            s.status = String::new();
                        }
                        self.lsp_open_all(&key);
                    }
                    Event::Diagnostics(p) => {
                        if self.prompt_lsp_diags(&p.uri, p.diagnostics.clone()) {
                            continue;
                        }
                        for tab in &mut self.tabs {
                            if let Some((e, i)) = buffer_with(tab, &p.uri) {
                                e.buffers[i].diags = p.diagnostics.clone();
                            }
                        }
                    }
                    Event::Status(line) => {
                        if let Some(s) = self.lsp.map.get_mut(&key) {
                            s.status = line;
                        }
                    }
                    Event::Log(line) => {
                        if let Some(s) = self.lsp.map.get_mut(&key) {
                            s.log.push(line);
                            if s.log.len() > 200 {
                                s.log.remove(0);
                            }
                        }
                    }
                    Event::Exited(code) => {
                        let why = self
                            .lsp
                            .map
                            .get(&key)
                            .and_then(|s| s.log.last().cloned())
                            .unwrap_or_default();
                        let line = format!(
                            "{} exited{}{}",
                            key.split('@').next().unwrap_or(&key),
                            code.map(|c| format!(" ({c})")).unwrap_or_default(),
                            if why.is_empty() {
                                String::new()
                            } else {
                                format!(" · {why}")
                            }
                        );
                        tracing::warn!("lsp: {line}");
                        if let Some(s) = self.lsp.map.get_mut(&key) {
                            s.gone = true;
                            s.status = line.clone();
                        }
                        // Remember why, so the status row can say so instead of retrying every open.
                        self.lsp.failed.insert(key.clone(), line.clone());
                        self.notice(&line);
                        for tab in &mut self.tabs {
                            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                                if let Pane::Editor(e) = p {
                                    for b in &mut e.buffers {
                                        b.in_lsp = false;
                                    }
                                }
                            }
                        }
                    }
                    Event::Response { id, result, .. } => {
                        let Some(p) = self.lsp.pending.remove(&(key.clone(), id)) else {
                            continue;
                        };
                        self.lsp_answer(p, result);
                    }
                }
            }
        }
        // Drop servers that have gone, so a later open restarts them.
        self.lsp.map.retain(|_, s| !s.gone);
        // Each editor's status line follows its buffer's server.
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                if let Pane::Editor(e) = p {
                    let status = e.buf().and_then(|b| b.lsp_key.as_ref()).and_then(|key| {
                        let command = key.split('@').next().unwrap_or(key);
                        if let Some(why) = self.lsp.failed.get(key) {
                            return Some(why.clone());
                        }
                        let s = self.lsp.map.get(key)?;
                        // Progress lines can be long paths; keep the tail short.
                        let st: String = s.status.chars().take(48).collect();
                        let st = if st.len() < s.status.len() {
                            format!("{st}…")
                        } else {
                            st
                        };
                        Some(if st.is_empty() {
                            command.to_string()
                        } else {
                            format!("{} · {}", command, st)
                        })
                    });
                    let status = status.unwrap_or_default();
                    if e.status != status {
                        e.status = status;
                        dirty = true;
                    }
                }
            }
        }
        if dirty {
            self.dirty = true;
        }
    }

    /// After a server comes up: open every buffer of its languages.
    fn lsp_open_all(&mut self, key: &str) {
        let Some(s) = self.lsp.map.get(key) else {
            return;
        };
        let (command, root) = key.split_once('@').unwrap_or((key, ""));
        for tab in &mut self.tabs {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                let Pane::Editor(e) = p else { continue };
                for b in &mut e.buffers {
                    if b.in_lsp || !b.ready() || b.text.len_bytes() > 8 * 1024 * 1024 {
                        continue;
                    }
                    let (Some(path), Some(uri)) = (&b.path, b.uri.clone()) else {
                        continue;
                    };
                    let Some(server) = nus_lsp::registry::server_for(path) else {
                        continue;
                    };
                    if server.command != command
                        || nus_lsp::registry::root_for(server, path)
                            .display()
                            .to_string()
                            != root
                    {
                        continue;
                    }
                    s.client.did_open_rope(uri, b.language, b.text.clone());
                    b.synced_revision = b.revision;
                    b.in_lsp = true;
                }
            }
        }
    }

    /// Route an answer to what asked for it.
    fn lsp_answer(
        &mut self,
        p: Pending,
        result: Result<serde_json::Value, nus_lsp::ResponseError>,
    ) {
        let result = match result {
            Ok(v) => v,
            Err(e) => {
                match p {
                    Pending::Format { then_save: true, .. } => self.editor_write(),
                    Pending::PromptCompletion { .. } => {}
                    _ => self.notice(&format!("server: {e}")),
                }
                return;
            }
        };
        match p {
            Pending::PromptCompletion { uri } => {
                let items: Vec<lt::CompletionItem> = match Client::parse::<lt::CompletionResponse>(result) {
                    Some(lt::CompletionResponse::Array(a)) => a,
                    Some(lt::CompletionResponse::List(l)) => l.items,
                    None => Vec::new(),
                };
                self.prompt_lsp_items(&uri, items);
            }
            Pending::Hover { uri, at } => {
                let Some(h): Option<lt::Hover> = Client::parse(result) else {
                    return;
                };
                let text = hover_text(h.contents);
                if text.trim().is_empty() {
                    return;
                }
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    if let Some((e, i)) = buffer_with(tab, &uri) {
                        if i == e.active {
                            e.hover = Some(HoverBox { text, at });
                        }
                    }
                }
            }
            Pending::Completion { uri } => {
                let items: Vec<lt::CompletionItem> =
                    match Client::parse::<lt::CompletionResponse>(result) {
                        Some(lt::CompletionResponse::Array(a)) => a,
                        Some(lt::CompletionResponse::List(l)) => l.items,
                        None => Vec::new(),
                    };
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    if let Some((e, i)) = buffer_with(tab, &uri) {
                        if i != e.active {
                            return;
                        }
                        let b = &e.buffers[i];
                        let (a, _) = b.word_at(b.cursor);
                        let prefix: String = b.text.slice(a..b.cursor).to_string().to_lowercase();
                        let mut items: Vec<lt::CompletionItem> = items
                            .into_iter()
                            .filter(|it| {
                                prefix.is_empty()
                                    || it
                                        .filter_text
                                        .as_deref()
                                        .unwrap_or(&it.label)
                                        .to_lowercase()
                                        .starts_with(&prefix)
                            })
                            .collect();
                        items.sort_by(|x, y| {
                            x.sort_text
                                .as_deref()
                                .unwrap_or(&x.label)
                                .cmp(y.sort_text.as_deref().unwrap_or(&y.label))
                        });
                        items.truncate(60);
                        e.completion = if items.is_empty() {
                            None
                        } else {
                            Some(Completion {
                                items,
                                sel: 0,
                                at: a,
                                scroll: 0,
                            })
                        };
                    }
                }
            }
            Pending::Definition { .. } => {
                let loc = match Client::parse::<lt::GotoDefinitionResponse>(result) {
                    Some(lt::GotoDefinitionResponse::Scalar(l)) => Some((l.uri, l.range)),
                    Some(lt::GotoDefinitionResponse::Array(v)) => {
                        v.into_iter().next().map(|l| (l.uri, l.range))
                    }
                    Some(lt::GotoDefinitionResponse::Link(v)) => v
                        .into_iter()
                        .next()
                        .map(|l| (l.target_uri, l.target_selection_range)),
                    None => None,
                };
                let Some((uri, range)) = loc else {
                    self.notice("no definition found");
                    return;
                };
                let Ok(path) = uri.to_file_path() else { return };
                self.open_file(&path, false);
                if let Some(e) = self.focused_editor() {
                    let rows = e.rows;
                    if let Some(b) = e.buf_mut() {
                        if b.ready() { b.cursor = crate::editor_work::offset(&b.text, range.start); }
                        else { b.pending_position = Some(range.start); }
                        b.anchor = None;
                        // Centre it.
                        b.scroll = (range.start.line as usize).saturating_sub(rows / 3);
                    }
                    e.reveal();
                }
            }
            Pending::Format { uri, then_save } => {
                let edits: Vec<lt::TextEdit> = Client::parse(result).unwrap_or_default();
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    if let Some((e, i)) = buffer_with(tab, &uri) {
                        let b = &mut e.buffers[i];
                        if !edits.is_empty() {
                            let mut text = b.text.to_string();
                            let mut sorted: Vec<(usize, usize, String)> = edits
                                .iter()
                                .map(|te| {
                                    (
                                        nus_lsp::offset_of(&text, te.range.start),
                                        nus_lsp::offset_of(&text, te.range.end),
                                        te.new_text.clone(),
                                    )
                                })
                                .collect();
                            sorted.sort_by(|x, y| y.0.cmp(&x.0));
                            let chars: Vec<char> = text.chars().collect();
                            let mut out = chars;
                            for (a, z, s) in sorted {
                                let z = z.min(out.len()).max(a);
                                out.splice(a..z, s.chars());
                            }
                            text = out.into_iter().collect();
                            b.replace_all(&text);
                        }
                        b.save_pending = None;
                    }
                }
                if then_save {
                    self.editor_write();
                } else {
                    self.notice("formatted");
                }
                self.editor_synced();
            }
        }
        self.dirty = true;
    }

    // --- the FILES folder ---

    /// Point the FILES folder at the project of a file.
    pub(crate) fn files_root_from(&mut self, path: &Path) {
        self.files_listing = None;
        let path = path.to_path_buf();
        let old_root = self.files_root.clone();
        let old_open = self.files_open.clone();
        self.files_listing = crate::work::Task::start(move |cancel| {
            let markers = [".git", "Cargo.toml", "package.json", "pyproject.toml", "go.mod"];
            let dir = path.parent().unwrap_or(&path);
            let root = dir.ancestors().find(|a| markers.iter().any(|m| a.join(m).exists())).unwrap_or(dir).to_path_buf();
            let mut open = if old_root.as_ref() == Some(&root) { old_open } else { HashSet::new() };
            for anc in dir.ancestors().take_while(|a| *a != root) { open.insert(anc.to_path_buf()); }
            let mut items = Vec::new();
            walk(&root,0,&open,&mut items,cancel);
            (root,open,items)
        });
    }

    pub(crate) fn refresh_files_folder(&mut self) {
        let Some(root) = self.files_root.clone() else { return; };
        self.files_listing = None;
        let open = self.files_open.clone();
        self.files_listing = crate::work::Task::start(move |cancel| {
            let mut items = Vec::new();
            walk(&root,0,&open,&mut items,cancel);
            (root,open,items)
        });
    }

    pub(crate) fn poll_files_folder(&mut self) {
        let Some(job) = &self.files_listing else {return};
        let (root,open,items) = match job.take() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {self.files_listing=None; return;}
            Err(_) => return,
        };
        self.files_listing = None;
        self.files_root = Some(root.clone());
        self.files_open = open;
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "FILES".into());
        match self.folders.iter_mut().find(|f| f.kind == Kind::Files) {
            Some(f) => {
                f.items = items;
                f.name = name;
            }
            None => {
                let at = self
                    .folders
                    .iter()
                    .position(|f| f.kind == Kind::Ports)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                self.folders.insert(
                    at,
                    Folder {
                        id: 3,
                        name,
                        kind: Kind::Files,
                        items,
                        open: true,
                        note: String::new(),
                    },
                );
            }
        }
        self.dirty = true;
    }

    /// A FILES row was clicked: a dir toggles, a file opens.
    pub(crate) fn files_click(&mut self, url: &str) -> bool {
        if let Some(p) = url.strip_prefix("dir://") {
            let p = PathBuf::from(p);
            if !self.files_open.remove(&p) {
                self.files_open.insert(p);
            }
            self.refresh_files_folder();
            return true;
        }
        if let Some(p) = url.strip_prefix("edit://") {
            let p = PathBuf::from(p);
            self.open_file(&p, false);
            return true;
        }
        false
    }
}

fn walk(dir: &Path, depth: usize, open: &HashSet<PathBuf>, out: &mut Vec<Item>, cancel: &std::sync::atomic::AtomicUsize) {
    if depth > 8 || out.len() >= 600 || cancel.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(bool, String, PathBuf)> = rd
        .flatten()
        .take(4000)
        .map(|e| {
            let p = e.path();
            (e.file_type().is_ok_and(|t| t.is_dir()), e.file_name().to_string_lossy().into_owned(), p)
        })
        .filter(|(_, n, _)| {
            !matches!(
                n.as_str(),
                ".git" | "node_modules" | "target" | "__pycache__" | ".venv"
            ) && !n.starts_with(".DS")
        })
        .collect();
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    let indent = "\u{2007}\u{2007}".repeat(depth);
    for (is_dir, name, p) in entries {
        if out.len() >= 600 || cancel.load(std::sync::atomic::Ordering::Relaxed) != 0 {break;}
        if is_dir {
            let opened = open.contains(&p);
            out.push(Item {
                title: format!("{indent}{} {name}", if opened { "▾" } else { "▸" }),
                url: format!("dir://{}", p.display()),
                detail: String::new(),
            });
            if opened {
                walk(&p, depth + 1, open, out, cancel);
            }
        } else {
            out.push(Item {
                title: format!("{indent}\u{2007}\u{2007}{name}"),
                url: format!("edit://{}", p.display()),
                detail: String::new(),
            });
        }
    }
}

/// Hover contents → plain lines (code fences dropped, markdown left as is).
fn hover_text(c: lt::HoverContents) -> String {
    fn marked(m: lt::MarkedString) -> String {
        match m {
            lt::MarkedString::String(s) => s,
            lt::MarkedString::LanguageString(l) => l.value,
        }
    }
    let raw = match c {
        lt::HoverContents::Scalar(m) => marked(m),
        lt::HoverContents::Array(v) => v.into_iter().map(marked).collect::<Vec<_>>().join("\n\n"),
        lt::HoverContents::Markup(m) => m.value,
    };
    raw.lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .map(|l| l.replace('\t', "    "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
