//! A language server client. One [`Client`] per running server: the
//! process is spawned over stdio, a reader thread turns its frames into
//! [`Event`]s on a channel, a writer thread drains ours. Requests are
//! fire-and-forget with an id; the answer comes back as
//! [`Event::Response`] and the host matches it up. Nothing here blocks
//! the caller.
//!
//! The wire format is JSON-RPC 2.0 with `Content-Length` headers, per the
//! protocol; types are `lsp_types`. Rope-backed editor documents use incremental
//! sync when supported; full-sync servers retain their protocol behavior.

pub mod registry;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

pub use lsp_types;
use lsp_types::*;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub type RequestId = i64;

/// What the server told us, delivered on the client's channel.
#[derive(Debug)]
pub enum Event {
    /// `initialize` answered; the server is ready for documents.
    Initialized(Box<ServerCapabilities>),
    /// The answer to one of our requests: the method we sent, and its
    /// result or the server's error.
    Response {
        id: RequestId,
        method: &'static str,
        result: Result<Value, ResponseError>,
    },
    Diagnostics(PublishDiagnosticsParams),
    /// `$/progress` and `window/showMessage`, flattened to a line for a
    /// status lamp.
    Status(String),
    Log(String),
    /// The server process is gone (exit status when known).
    Exited(Option<i32>),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// A document the client is syncing, by version.
struct Doc {
    version: i32,
    language: String,
}

/// The documents as the server last saw them, keyed by URL.
type Snapshots = HashMap<Url, ropey::Rope>;

/// Work that needs the snapshots, run once when the message is sent.
type Defer = Box<dyn FnOnce(&mut Snapshots) -> Value + Send>;

enum Outbound {
    Ready(Value),
    Deferred(Defer),
}
impl Outbound {
    fn value(self, snapshots: &mut Snapshots) -> Value {
        match self {
            Self::Ready(v) => {
                if v["method"] == "textDocument/didClose" {
                    if let Some(uri) = v
                        .pointer("/params/textDocument/uri")
                        .and_then(Value::as_str)
                        .and_then(|u| Url::parse(u).ok())
                    {
                        snapshots.remove(&uri);
                    }
                }
                v
            }
            Self::Deferred(f) => f(snapshots),
        }
    }
}

/// Find one replacement using shared rope chunks. Comparing unchanged
/// chunks uses slice equality, rather than allocating/serializing the file.
fn rope_change(old: &ropey::Rope, new: &ropey::Rope) -> TextDocumentContentChangeEvent {
    let mut prefix = 0;
    for (a, b) in old.chunks().zip(new.chunks()) {
        if a == b {
            prefix += a.len();
        } else {
            prefix += a.bytes().zip(b.bytes()).take_while(|(a, b)| a == b).count();
            break;
        }
    }
    let start = old.byte_to_char(prefix);
    prefix = old.char_to_byte(start);
    let mut suffix = 0;
    for (a, b) in old
        .chunks_at_byte(old.len_bytes())
        .0
        .reversed()
        .zip(new.chunks_at_byte(new.len_bytes()).0.reversed())
    {
        if a == b {
            suffix += a.len();
        } else {
            suffix += a
                .bytes()
                .rev()
                .zip(b.bytes().rev())
                .take_while(|(a, b)| a == b)
                .count();
            break;
        }
    }
    suffix = suffix
        .min(old.len_bytes() - prefix)
        .min(new.len_bytes() - prefix);
    // Round the end forward to a complete character if two different Unicode
    // characters share their final UTF-8 byte(s).
    let mut old_end = old.len_bytes() - suffix;
    while old.char_to_byte(old.byte_to_char(old_end)) != old_end {
        old_end += 1;
        suffix -= 1;
    }
    let new_end = new.len_bytes() - suffix;
    let end = old.byte_to_char(old_end);
    let position = |at| {
        let line = old.char_to_line(at);
        Position::new(
            line as u32,
            (old.char_to_utf16_cu(at) - old.char_to_utf16_cu(old.line_to_char(line))) as u32,
        )
    };
    TextDocumentContentChangeEvent {
        range: Some(Range::new(position(start), position(end))),
        range_length: None,
        text: new.byte_slice(prefix..new_end).to_string(),
    }
}

pub struct Client {
    child: Arc<Mutex<Option<Child>>>,
    tx: SyncSender<Outbound>,
    next_id: AtomicI64,
    /// Method names by outstanding request id, so responses can say
    /// what they answer.
    pending: Arc<Mutex<HashMap<RequestId, &'static str>>>,
    docs: Mutex<HashMap<Url, Doc>>,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub name: String,
    pub root: PathBuf,
}

impl Client {
    /// Spawn `cmd args…` with `root` as its workspace and send
    /// `initialize`. Events arrive on the returned receiver, the first
    /// being [`Event::Initialized`] once the server answers.
    pub fn spawn(
        name: &str,
        cmd: &Path,
        args: &[String],
        root: &Path,
    ) -> anyhow::Result<(Client, Receiver<Event>)> {
        Self::spawn_with_env(name, cmd, args, root, &[])
    }

    /// [`Client::spawn`] with extra environment for the server.
    pub fn spawn_with_env(
        name: &str,
        cmd: &Path,
        args: &[String],
        root: &Path,
        env: &[(&str, &str)],
    ) -> anyhow::Result<(Client, Receiver<Event>)> {
        let mut child = Command::new(cmd)
            .args(args)
            .envs(env.iter().copied())
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("{}: {e}", cmd.display()))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let (ev_tx, ev_rx) = sync_channel::<Event>(32);
        let (out_tx, out_rx) = sync_channel::<Outbound>(32);
        let pending: Arc<Mutex<HashMap<RequestId, &'static str>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let child = Arc::new(Mutex::new(Some(child)));

        // Writer: frames what the host queues.
        std::thread::Builder::new()
            .name(format!("lsp-{name}-out"))
            .spawn(move || {
                let mut w = std::io::BufWriter::new(stdin);
                let mut snapshots = HashMap::new();
                while let Ok(v) = out_rx.recv() {
                    let body = v.value(&mut snapshots).to_string();
                    if write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)
                        .and_then(|_| w.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;

        // Stderr: the server's own log, one line per event.
        {
            let ev_tx = ev_tx.clone();
            std::thread::Builder::new()
                .name(format!("lsp-{name}-err"))
                .spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    while let Some(line) = bounded_line(&mut reader, 8192) {
                        if ev_tx.send(Event::Log(line)).is_err() {
                            break;
                        }
                    }
                })?;
        }

        // Reader: frames in, events out; answers the server's own requests.
        {
            let ev_tx = ev_tx.clone();
            let out_tx = out_tx.clone();
            let pending = pending.clone();
            let child = child.clone();
            std::thread::Builder::new()
                .name(format!("lsp-{name}-in"))
                .spawn(move || {
                    let mut r = BufReader::new(stdout);
                    while let Some(msg) = read_frame(&mut r) {
                        handle_incoming(msg, &ev_tx, &out_tx, &pending);
                    }
                    let code = child
                        .lock()
                        .ok()
                        .and_then(|mut c| {
                            c.as_mut().and_then(|c| {
                                // EOF/malformed framing: terminate a server that left its
                                // process alive. Never wait on a live child while holding
                                // the mutex that shutdown needs to kill it.
                                let _ = c.kill();
                                c.wait().ok()
                            })
                        })
                        .and_then(|s| s.code());
                    let _ = ev_tx.send(Event::Exited(code));
                })?;
        }

        let client = Client {
            child,
            tx: out_tx,
            next_id: AtomicI64::new(1),
            pending,
            docs: Mutex::new(HashMap::new()),
            capabilities: Mutex::new(None),
            name: name.to_string(),
            root: root.to_path_buf(),
        };
        client.initialize();
        Ok((client, ev_rx))
    }

    fn initialize(&self) {
        let root_uri = Url::from_directory_path(&self.root).ok();
        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: root_uri.clone(),
            workspace_folders: root_uri.map(|uri| {
                vec![WorkspaceFolder {
                    name: self
                        .root
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    uri,
                }]
            }),
            client_info: Some(ClientInfo {
                name: "nus".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    synchronization: Some(TextDocumentSyncClientCapabilities {
                        did_save: Some(true),
                        ..Default::default()
                    }),
                    completion: Some(CompletionClientCapabilities {
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(false),
                            documentation_format: Some(vec![
                                MarkupKind::PlainText,
                                MarkupKind::Markdown,
                            ]),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    hover: Some(HoverClientCapabilities {
                        content_format: Some(vec![MarkupKind::PlainText, MarkupKind::Markdown]),
                        ..Default::default()
                    }),
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(false),
                        ..Default::default()
                    }),
                    definition: Some(GotoCapability::default()),
                    formatting: Some(DynamicRegistrationClientCapabilities::default()),
                    ..Default::default()
                }),
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    ..Default::default()
                }),
                workspace: Some(WorkspaceClientCapabilities {
                    configuration: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        self.request("initialize", params);
    }

    fn send(&self, v: Value) {
        self.enqueue(Outbound::Ready(v));
    }

    fn enqueue(&self, v: Outbound) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(v) {
            // A wedged server must not freeze typing or collect unlimited
            // full-document updates. End it and report its exit to the host.
            tracing::warn!("language server stopped reading; ending {}", self.name);
            if let Ok(mut child) = self.child.lock() {
                if let Some(child) = child.as_mut() {
                    let _ = child.kill();
                }
            }
        }
    }

    /// Send a request; the answer arrives as [`Event::Response`] with this id.
    pub fn request<P: serde::Serialize>(&self, method: &'static str, params: P) -> RequestId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut p) = self.pending.lock() {
            if p.len() >= 512 {
                if let Some(oldest) = p.keys().min().copied() {
                    p.remove(&oldest);
                }
            }
            p.insert(id, method);
        }
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    pub fn notify<P: serde::Serialize>(&self, method: &str, params: P) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// The host calls this on [`Event::Initialized`]: completes the
    /// handshake and remembers what the server can do.
    pub fn initialized(&self, caps: ServerCapabilities) {
        if let Ok(mut c) = self.capabilities.lock() {
            *c = Some(caps);
        }
        self.notify("initialized", InitializedParams {});
    }

    pub fn is_ready(&self) -> bool {
        self.capabilities
            .lock()
            .map(|c| c.is_some())
            .unwrap_or(false)
    }

    // --- documents (full sync) ---

    pub fn did_open(&self, uri: Url, language: &str, text: &str) {
        if let Ok(mut d) = self.docs.lock() {
            d.insert(
                uri.clone(),
                Doc {
                    version: 1,
                    language: language.into(),
                },
            );
        }
        self.notify(
            "textDocument/didOpen",
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: language.into(),
                    version: 1,
                    text: text.into(),
                },
            },
        );
    }

    /// Editor snapshots are O(1) rope clones. Expanding and serializing their
    /// text belongs to the writer, never the input or render thread.
    pub fn did_open_rope(&self, uri: Url, language: &str, text: ropey::Rope) {
        let language = language.to_owned();
        if let Ok(mut docs) = self.docs.lock() {
            docs.insert(
                uri.clone(),
                Doc {
                    version: 1,
                    language: language.clone(),
                },
            );
        }
        self.enqueue(Outbound::Deferred(Box::new(move |snapshots| {
            let body = json!({
                "jsonrpc":"2.0", "method":"textDocument/didOpen", "params": {
                    "textDocument": { "uri":uri, "languageId":language, "version":1, "text":text.to_string() }
                }
            });
            snapshots.insert(uri, text);
            body
        })));
    }

    pub fn did_change_rope(&self, uri: Url, text: ropey::Rope) -> i32 {
        let version = self
            .docs
            .lock()
            .map(|mut docs| {
                let doc = docs.entry(uri.clone()).or_insert(Doc {
                    version: 0,
                    language: String::new(),
                });
                doc.version += 1;
                doc.version
            })
            .unwrap_or(1);
        let incremental = self
            .capabilities
            .lock()
            .ok()
            .and_then(|c| c.as_ref().and_then(|c| c.text_document_sync.clone()))
            .is_some_and(|sync| match sync {
                TextDocumentSyncCapability::Kind(k) => k == TextDocumentSyncKind::INCREMENTAL,
                TextDocumentSyncCapability::Options(o) => {
                    o.change == Some(TextDocumentSyncKind::INCREMENTAL)
                }
            });
        self.enqueue(Outbound::Deferred(Box::new(move |snapshots| {
            let change = if incremental {
                snapshots.get(&uri).map(|old| rope_change(old, &text))
            } else {
                None
            }
            .unwrap_or_else(|| TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            });
            snapshots.insert(uri.clone(), text);
            json!({
                "jsonrpc":"2.0", "method":"textDocument/didChange", "params": {
                    "textDocument": { "uri":uri, "version":version }, "contentChanges": [change]
                }
            })
        })));
        version
    }

    /// The whole text again; returns the new version.
    pub fn did_change(&self, uri: Url, text: &str) -> i32 {
        let version = match self.docs.lock() {
            Ok(mut d) => {
                let doc = d.entry(uri.clone()).or_insert(Doc {
                    version: 0,
                    language: String::new(),
                });
                doc.version += 1;
                doc.version
            }
            Err(_) => 1,
        };
        self.notify(
            "textDocument/didChange",
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri, version },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.into(),
                }],
            },
        );
        version
    }

    pub fn did_save(&self, uri: Url, text: Option<&str>) {
        self.notify(
            "textDocument/didSave",
            DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
                text: text.map(String::from),
            },
        );
    }

    pub fn did_close(&self, uri: Url) {
        if let Ok(mut d) = self.docs.lock() {
            d.remove(&uri);
        }
        self.notify(
            "textDocument/didClose",
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            },
        );
    }

    /// Release server-side document state when tabs or buffers are closed.
    pub fn retain_documents(&self, live: &std::collections::HashSet<Url>) {
        let closed: Vec<_> = self
            .docs
            .lock()
            .map(|docs| {
                docs.keys()
                    .filter(|uri| !live.contains(*uri))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for uri in closed {
            self.did_close(uri);
        }
    }

    pub fn language_of(&self, uri: &Url) -> Option<String> {
        self.docs
            .lock()
            .ok()
            .and_then(|d| d.get(uri).map(|d| d.language.clone()))
    }

    // --- requests ---

    pub fn hover(&self, uri: Url, pos: Position) -> RequestId {
        self.request(
            "textDocument/hover",
            HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos,
                },
                work_done_progress_params: Default::default(),
            },
        )
    }

    pub fn completion(&self, uri: Url, pos: Position, trigger: Option<String>) -> RequestId {
        self.request(
            "textDocument/completion",
            CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: Some(CompletionContext {
                    trigger_kind: if trigger.is_some() {
                        CompletionTriggerKind::TRIGGER_CHARACTER
                    } else {
                        CompletionTriggerKind::INVOKED
                    },
                    trigger_character: trigger,
                }),
            },
        )
    }

    pub fn definition(&self, uri: Url, pos: Position) -> RequestId {
        self.request(
            "textDocument/definition",
            GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        )
    }

    pub fn formatting(&self, uri: Url, tab_size: u32, insert_spaces: bool) -> RequestId {
        self.request(
            "textDocument/formatting",
            DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                options: FormattingOptions {
                    tab_size,
                    insert_spaces,
                    ..Default::default()
                },
                work_done_progress_params: Default::default(),
            },
        )
    }

    pub fn references(&self, uri: Url, pos: Position) -> RequestId {
        self.request(
            "textDocument/references",
            ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            },
        )
    }

    pub fn rename(&self, uri: Url, pos: Position, new_name: &str) -> RequestId {
        self.request(
            "textDocument/rename",
            RenameParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos,
                },
                new_name: new_name.into(),
                work_done_progress_params: Default::default(),
            },
        )
    }

    /// Ask the server to stop: `shutdown`, then `exit`. The process is
    /// killed if it lingers; [`Event::Exited`] follows either way.
    pub fn shutdown(&self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        let child = self.child.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Ok(mut c) = child.lock() {
                if let Some(c) = c.as_mut() {
                    let _ = c.kill();
                }
            }
        });
    }

    /// A typed view of a response's result.
    pub fn parse<T: DeserializeOwned>(result: Value) -> Option<T> {
        serde_json::from_value(result).ok()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            if let Some(c) = c.as_mut() {
                let _ = c.kill();
            }
        }
    }
}

fn clipped(text: &str) -> String {
    let mut end = text.len().min(8192);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Read through one line, retaining only its bounded prefix.
fn bounded_line<R: BufRead>(r: &mut R, max: usize) -> Option<String> {
    let mut out = Vec::new();
    let mut read = false;
    loop {
        let part = r.fill_buf().ok()?;
        if part.is_empty() {
            break;
        }
        read = true;
        let end = part.iter().position(|b| *b == b'\n').map(|i| i + 1);
        let n = end.unwrap_or(part.len());
        out.extend_from_slice(&part[..n.min(max.saturating_sub(out.len()))]);
        r.consume(n);
        if end.is_some() {
            break;
        }
    }
    read.then(|| String::from_utf8_lossy(&out).trim_end().to_string())
}

/// One JSON-RPC frame: headers, blank line, body of `Content-Length` bytes.
fn read_frame<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut len: Option<usize> = None;
    loop {
        let line = bounded_line(r, 8192)?;
        if line.len() >= 8192 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            // A blank line before any header is noise (a chatty server, a
            // test harness); after one, it ends the headers.
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let len = len?;
    if len > 16 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn handle_incoming(
    msg: Value,
    ev: &SyncSender<Event>,
    out: &SyncSender<Outbound>,
    pending: &Arc<Mutex<HashMap<RequestId, &'static str>>>,
) {
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").cloned();
    match (method, id) {
        // The server asks us something: answer what keeps it happy.
        (Some(m), Some(id)) => {
            let result = match m {
                "workspace/configuration" => {
                    let n = msg
                        .pointer("/params/items")
                        .and_then(Value::as_array)
                        .map(|a| a.len())
                        .unwrap_or(1);
                    Value::Array(vec![Value::Null; n])
                }
                "workspace/workspaceFolders" => Value::Array(vec![]),
                "window/showMessageRequest" => {
                    if let Some(s) = msg.pointer("/params/message").and_then(Value::as_str) {
                        let _ = ev.send(Event::Status(clipped(s)));
                    }
                    Value::Null
                }
                // registerCapability, workDoneProgress/create, applyEdit…: ok.
                _ => Value::Null,
            };
            let _ = out.send(Outbound::Ready(
                json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            ));
        }
        // A notification.
        (Some(m), None) => match m {
            "textDocument/publishDiagnostics" => {
                if let Some(p) = msg
                    .get("params")
                    .cloned()
                    .and_then(|p| serde_json::from_value(p).ok())
                {
                    let _ = ev.send(Event::Diagnostics(p));
                }
            }
            "window/logMessage" => {
                if let Some(s) = msg.pointer("/params/message").and_then(Value::as_str) {
                    let _ = ev.send(Event::Log(clipped(s)));
                }
            }
            "window/showMessage" => {
                if let Some(s) = msg.pointer("/params/message").and_then(Value::as_str) {
                    let _ = ev.send(Event::Status(clipped(s)));
                }
            }
            "$/progress" => {
                let v = msg.pointer("/params/value");
                let title = v.and_then(|v| v.get("title")).and_then(Value::as_str);
                let message = v.and_then(|v| v.get("message")).and_then(Value::as_str);
                let kind = v
                    .and_then(|v| v.get("kind"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let line = match (kind, title, message) {
                    ("end", _, _) => String::new(),
                    (_, Some(t), Some(m)) => format!("{t} · {m}"),
                    (_, Some(t), None) => t.to_string(),
                    (_, None, Some(m)) => m.to_string(),
                    _ => String::new(),
                };
                let _ = ev.send(Event::Status(clipped(&line)));
            }
            _ => {}
        },
        // A response to us.
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return };
            let method = pending
                .lock()
                .ok()
                .and_then(|mut p| p.remove(&id))
                .unwrap_or("");
            if method == "initialize" {
                let caps = msg
                    .pointer("/result/capabilities")
                    .cloned()
                    .and_then(|c| serde_json::from_value(c).ok())
                    .unwrap_or_default();
                let _ = ev.send(Event::Initialized(Box::new(caps)));
                return;
            }
            let result = match msg.get("error") {
                Some(e) => Err(serde_json::from_value(e.clone()).unwrap_or(ResponseError {
                    code: -1,
                    message: e.to_string(),
                })),
                None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = ev.send(Event::Response { id, method, result });
        }
        _ => {}
    }
}

/// Character offset → LSP position (UTF-16 columns, as the protocol wants).
pub fn position_of(text: &str, char_idx: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in text.chars().enumerate() {
        if i == char_idx {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += c.len_utf16() as u32;
        }
    }
    Position {
        line,
        character: col,
    }
}

/// LSP position → character offset into `text` (clamped).
pub fn offset_of(text: &str, pos: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, c) in text.chars().enumerate() {
        if line == pos.line && col >= pos.character {
            return i;
        }
        if line > pos.line {
            return i;
        }
        if c == '\n' {
            if line == pos.line {
                return i;
            }
            line += 1;
            col = 0;
        } else {
            col += c.len_utf16() as u32;
        }
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_protocol_data_and_logs_are_bounded() {
        let mut invalid = std::io::Cursor::new(b"Content-Length: 999999999999\r\n\r\n");
        assert!(read_frame(&mut invalid).is_none());
        let mut log = std::io::Cursor::new(format!("{}\nnext\n", "x".repeat(1_000_000)));
        assert_eq!(bounded_line(&mut log, 128).unwrap().len(), 128);
        assert_eq!(bounded_line(&mut log, 128).unwrap(), "next");
        assert!(bounded_line(&mut log, 128).is_none());
    }

    #[test]
    fn positions_round_trip() {
        let t = "ab\ncdé\nf";
        assert_eq!(position_of(t, 0), Position::new(0, 0));
        assert_eq!(position_of(t, 3), Position::new(1, 0));
        assert_eq!(position_of(t, 6), Position::new(1, 3));
        assert_eq!(offset_of(t, Position::new(1, 3)), 6);
        assert_eq!(offset_of(t, Position::new(2, 0)), 7);
        assert_eq!(offset_of(t, Position::new(9, 9)), 8);
        assert_eq!(offset_of(t, Position::new(0, 99)), 2);
    }

    #[test]
    fn frames_parse() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let framed = format!(
            "Content-Length: {}\r\nContent-Type: application/json\r\n\r\n{}",
            body.len(),
            body
        );
        let mut r = std::io::Cursor::new(framed.into_bytes());
        let v = read_frame(&mut r).unwrap();
        assert_eq!(v["id"], 1);
        assert!(read_frame(&mut r).is_none());
    }
}

#[cfg(test)]
mod rope_sync_tests {
    use super::*;
    #[test]
    fn incremental_edits_preserve_unicode_crlf_and_chunk_boundaries() {
        for initial in ["α😀\r\nhello world".to_string(), "hello α😀\n".repeat(1000)] {
            let old = ropey::Rope::from_str(&initial);
            for at in [0, 2, old.len_chars() / 2, old.len_chars()] {
                for insertion in ["x", "😀", "\r\n", ""] {
                    let mut new = old.clone();
                    if at < new.len_chars() {
                        new.remove(at..at + 1);
                    }
                    new.insert(at, insertion);
                    let change = rope_change(&old, &new);
                    let range = change.range.unwrap();
                    let a = offset_of(&initial, range.start);
                    let b = offset_of(&initial, range.end);
                    let mut applied = old.clone();
                    applied.remove(a..b);
                    applied.insert(a, &change.text);
                    assert_eq!(applied, new);
                    assert!(change.text.len() < 5000);
                }
            }
        }
    }
}
