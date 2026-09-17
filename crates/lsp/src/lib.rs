//! A language server client. One [`Client`] per running server: the
//! process is spawned over stdio, a reader thread turns its frames into
//! [`Event`]s on a channel, a writer thread drains ours. Requests are
//! fire-and-forget with an id; the answer comes back as
//! [`Event::Response`] and the host matches it up. Nothing here blocks
//! the caller.
//!
//! The wire format is JSON-RPC 2.0 with `Content-Length` headers, per the
//! protocol; types are `lsp_types`. Document sync is full-text: simple,
//! and every server supports it.

pub mod registry;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
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

pub struct Client {
    child: Arc<Mutex<Option<Child>>>,
    tx: Sender<Value>,
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

        let (ev_tx, ev_rx) = channel::<Event>();
        let (out_tx, out_rx) = channel::<Value>();
        let pending: Arc<Mutex<HashMap<RequestId, &'static str>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let child = Arc::new(Mutex::new(Some(child)));

        // Writer: frames what the host queues.
        std::thread::Builder::new()
            .name(format!("lsp-{name}-out"))
            .spawn(move || {
                let mut w = std::io::BufWriter::new(stdin);
                while let Ok(v) = out_rx.recv() {
                    let body = v.to_string();
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
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
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
                        .and_then(|mut c| c.as_mut().and_then(|c| c.wait().ok()))
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
        let _ = self.tx.send(v);
    }

    /// Send a request; the answer arrives as [`Event::Response`] with this id.
    pub fn request<P: serde::Serialize>(&self, method: &'static str, params: P) -> RequestId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut p) = self.pending.lock() {
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

/// One JSON-RPC frame: headers, blank line, body of `Content-Length` bytes.
fn read_frame<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
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
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn handle_incoming(
    msg: Value,
    ev: &Sender<Event>,
    out: &Sender<Value>,
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
                        let _ = ev.send(Event::Status(s.into()));
                    }
                    Value::Null
                }
                // registerCapability, workDoneProgress/create, applyEdit…: ok.
                _ => Value::Null,
            };
            let _ = out.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
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
                    let _ = ev.send(Event::Log(s.into()));
                }
            }
            "window/showMessage" => {
                if let Some(s) = msg.pointer("/params/message").and_then(Value::as_str) {
                    let _ = ev.send(Event::Status(s.into()));
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
                let _ = ev.send(Event::Status(line));
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
