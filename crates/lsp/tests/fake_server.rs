//! The client against a server we control: this test binary, re-run with
//! NUS_FAKE_LSP set, answers initialize and hover, publishes a diagnostic
//! on didOpen, and honours shutdown.

use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

use nus_lsp::lsp_types::*;
use nus_lsp::{Client, Event};
use serde_json::{json, Value};

fn serve() -> ! {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut r = stdin.lock();
    let mut w = stdout.lock();
    let send = |w: &mut std::io::StdoutLock, v: Value| {
        let b = v.to_string();
        write!(w, "Content-Length: {}\r\n\r\n{}", b.len(), b).unwrap();
        w.flush().unwrap();
    };
    loop {
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).unwrap() == 0 {
                std::process::exit(0);
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut body = vec![0u8; len];
        r.read_exact(&mut body).unwrap();
        let msg: Value = serde_json::from_slice(&body).unwrap();
        let method = msg["method"].as_str().unwrap_or("");
        let id = msg.get("id").cloned();
        match method {
            "initialize" => {
                send(
                    &mut w,
                    json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{"hoverProvider":true,"textDocumentSync":1}}}),
                );
                // A server-to-client request, to prove we answer those.
                send(
                    &mut w,
                    json!({"jsonrpc":"2.0","id":"cfg","method":"workspace/configuration","params":{"items":[{"section":"x"}]}}),
                );
            }
            "textDocument/didOpen" => {
                let uri = msg["params"]["textDocument"]["uri"].clone();
                send(
                    &mut w,
                    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"severity":1,"message":"fake"}]}}),
                );
            }
            "textDocument/didClose" => send(&mut w, json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"document released"}})),
            "textDocument/hover" => {
                let ch = msg["params"]["position"]["character"].as_u64().unwrap();
                send(
                    &mut w,
                    json!({"jsonrpc":"2.0","id":id,"result":{"contents":{"kind":"plaintext","value":format!("hover at {ch}")}}}),
                );
            }
            "shutdown" => send(&mut w, json!({"jsonrpc":"2.0","id":id,"result":null})),
            "exit" => std::process::exit(0),
            _ => {}
        }
        if method.is_empty() && msg.get("id") == Some(&json!("cfg")) {
            // The client's answer to our configuration request.
            assert_eq!(msg["result"], json!([null]));
        }
    }
}

fn wait_for<F: FnMut(&Event) -> bool>(rx: &std::sync::mpsc::Receiver<Event>, mut f: F) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let ev = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("event before timeout");
        if f(&ev) {
            return ev;
        }
    }
}

#[test]
fn talks_to_a_server() {
    if std::env::var_os("NUS_FAKE_LSP").is_some() {
        serve();
    }
    let exe = std::env::current_exe().unwrap();
    let root = std::env::temp_dir();
    let (client, rx) = Client::spawn_with_env(
        "fake",
        &exe,
        &["--nocapture".into()],
        &root,
        &[("NUS_FAKE_LSP", "1")],
    )
    .unwrap();

    let caps = match wait_for(&rx, |e| matches!(e, Event::Initialized(_))) {
        Event::Initialized(c) => c,
        _ => unreachable!(),
    };
    assert_eq!(
        caps.hover_provider,
        Some(HoverProviderCapability::Simple(true))
    );
    client.initialized(*caps);
    assert!(client.is_ready());

    let uri = Url::from_file_path(root.join("x.rs")).unwrap();
    client.did_open(uri.clone(), "rust", "let x = 1;");
    let diags = match wait_for(&rx, |e| matches!(e, Event::Diagnostics(_))) {
        Event::Diagnostics(d) => d,
        _ => unreachable!(),
    };
    assert_eq!(diags.uri, uri);
    assert_eq!(diags.diagnostics[0].message, "fake");

    let id = client.hover(uri.clone(), Position::new(0, 4));
    let ev = wait_for(
        &rx,
        |e| matches!(e, Event::Response { id: i, .. } if *i == id),
    );
    let Event::Response { method, result, .. } = ev else {
        unreachable!()
    };
    assert_eq!(method, "textDocument/hover");
    let hover: Hover = Client::parse(result.unwrap()).unwrap();
    match hover.contents {
        HoverContents::Markup(m) => assert_eq!(m.value, "hover at 4"),
        other => panic!("{other:?}"),
    }

    assert_eq!(client.did_change(uri.clone(), "let x = 2;"), 2);
    client.retain_documents(&Default::default());
    assert!(client.language_of(&uri).is_none());
    wait_for(&rx, |e| matches!(e, Event::Log(s) if s == "document released"));
    client.shutdown();
    wait_for(&rx, |e| matches!(e, Event::Exited(_)));
}
