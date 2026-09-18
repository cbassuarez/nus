//! `nus mcp`: the assistant's eyes and hands, as an MCP server on stdio.
//!
//! Model Context Protocol, JSON-RPC 2.0 one message per line. Each tool is
//! a verb of the instance-port protocol (`remote.rs` in the app), so the
//! assistant in a shell reads the page beside it as you see it, logged in
//! as you, and — under ASSISTANTS · HANDS — acts on it. Register it once:
//!
//!   claude mcp add nus -- nus mcp
//!   codex mcp add nus -- nus mcp
//!
//! No async, no framework: a read loop, a match, a write.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// The tools, as MCP lists them: name, description, input schema, and the
/// instance-port verb + fixed args behind each.
fn tools() -> Vec<(Value, &'static str, Value)> {
    let tab = json!({ "type": "integer", "description": "Tab number (1-based); the active tab by default." });
    let t = |name: &str, desc: &str, props: Value, required: Vec<&str>, cmd: &'static str, fixed: Value| {
        (
            json!({ "name": name, "description": desc, "inputSchema": { "type": "object", "properties": props, "required": required } }),
            cmd,
            fixed,
        )
    };
    vec![
        t("nus_tabs", "The tabs open in nus: shells (with cwd), pages (with url and title), editors. Tab numbers are 1-based.", json!({}), vec![], "ls", json!({})),
        t("nus_page_info", "The page beside the shell: its url, title, and whether it is loading.", json!({ "tab": tab }), vec![], "page", json!({ "what": "info" })),
        t("nus_page_text", "The page's readable text (the reader's extraction: headings, paragraphs, code), as the user sees it, logged in as they are.", json!({ "tab": tab }), vec![], "page", json!({ "what": "text" })),
        t("nus_page_dom", "The outer HTML of the first element matching a CSS selector (default body), up to 200 KB.", json!({ "tab": tab, "selector": { "type": "string", "description": "A CSS selector, e.g. 'main' or '#app form'." } }), vec![], "page", json!({ "what": "dom" })),
        t("nus_page_console", "The page's console: log, warn, error calls and uncaught exceptions, newest last.", json!({ "tab": tab, "limit": { "type": "integer" } }), vec![], "page", json!({ "what": "console" })),
        t("nus_page_network", "The page's requests and responses (method, url, status, mime), newest last.", json!({ "tab": tab, "limit": { "type": "integer" } }), vec![], "page", json!({ "what": "network" })),
        t("nus_page_screenshot", "A screenshot of the page pane as it is drawn right now, from nus's own render target.", json!({ "tab": tab }), vec![], "page", json!({ "what": "screenshot" })),
        t("nus_page_open", "Open a URL in nus, beside the shell (default) or as a new tab.", json!({ "url": { "type": "string" }, "beside": { "type": "boolean", "description": "true: in the split beside the shell; false: a new tab." } }), vec!["url"], "page", json!({ "what": "open" })),
        t("nus_page_click", "Click the page at a CSS selector (its centre) or at x,y in CSS pixels. A hand: subject to ASSISTANTS · HANDS; the user can take over at any time.", json!({ "tab": tab, "selector": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" } }), vec![], "hands", json!({ "what": "click" })),
        t("nus_page_type", "Type text into the page's focused element (a hand).", json!({ "tab": tab, "text": { "type": "string" }, "enter": { "type": "boolean", "description": "Press Enter after the text." } }), vec!["text"], "hands", json!({ "what": "type" })),
        t("nus_page_scroll", "Scroll the page by dy CSS pixels (a hand).", json!({ "tab": tab, "dy": { "type": "number" } }), vec!["dy"], "hands", json!({ "what": "scroll" })),
        t("nus_page_navigate", "Navigate the page beside the shell to a URL (a hand).", json!({ "tab": tab, "url": { "type": "string" } }), vec!["url"], "hands", json!({ "what": "navigate" })),
        t("nus_block", "A shell's last command block (or all of them): command, exit code, and output.", json!({ "tab": tab, "which": { "type": "string", "enum": ["last", "all"] } }), vec![], "block", json!({})),
        t("nus_ports", "What is listening on this machine right now, with the process that owns each port.", json!({}), vec![], "ports", json!({})),
        t("nus_log", "The journal: commands that ran in a folder, across restarts, with when, how long, and exit codes.", json!({ "cwd": { "type": "string" }, "limit": { "type": "integer" } }), vec![], "log", json!({})),
        t("nus_hold", "Held shells (shells that outlive the app): list, attach one as a tab, or kill one.", json!({ "what": { "type": "string", "enum": ["ls", "attach", "kill"] }, "id": { "type": "string" } }), vec![], "hold", json!({})),
    ]
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() / 3 * 4 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// One tool call: the verb, its answer as MCP content.
fn call_tool(name: &str, args: &Value, call: &dyn Fn(&str, Value) -> Result<Value, String>) -> Value {
    let Some((_, cmd, fixed)) = tools().into_iter().find(|(t, _, _)| t.get("name").and_then(Value::as_str) == Some(name)) else {
        return json!({ "content": [{ "type": "text", "text": format!("no tool named {name}") }], "isError": true });
    };
    let mut merged = fixed.as_object().cloned().unwrap_or_default();
    // Who is asking: the assistants say so in their environment.
    let who = if std::env::var_os("CLAUDECODE").is_some() || std::env::var_os("CLAUDE_CODE").is_some() {
        "claude"
    } else if std::env::vars().any(|(k, _)| k.starts_with("CODEX_")) {
        "codex"
    } else if std::env::var_os("GEMINI_CLI").is_some() {
        "gemini"
    } else {
        "the assistant"
    };
    merged.insert("who".into(), Value::String(who.into()));
    if let Some(o) = args.as_object() {
        for (k, v) in o {
            merged.insert(k.clone(), v.clone());
        }
    }
    match call(cmd, Value::Object(merged)) {
        Ok(v) => {
            // A screenshot comes back as a file: send the pixels.
            if name == "nus_page_screenshot" {
                if let Some(path) = v.get("path").and_then(Value::as_str) {
                    if let Ok(bytes) = std::fs::read(path) {
                        return json!({ "content": [{ "type": "image", "data": base64(&bytes), "mimeType": "image/png" }, { "type": "text", "text": format!("{}×{} px at {}×", v.get("width").and_then(Value::as_u64).unwrap_or(0), v.get("height").and_then(Value::as_u64).unwrap_or(0), v.get("scale").and_then(Value::as_f64).unwrap_or(1.0)) }] });
                    }
                }
            }
            // Text answers read better as text; the rest as JSON.
            let text = match v.get("text").and_then(Value::as_str) {
                Some(t) if v.as_object().is_some_and(|o| o.len() <= 4) => {
                    let title = v.get("title").and_then(Value::as_str).unwrap_or("");
                    if title.is_empty() { t.to_string() } else { format!("# {title}\n\n{t}") }
                }
                _ => serde_json::to_string_pretty(&v).unwrap_or_default(),
            };
            json!({ "content": [{ "type": "text", "text": text }] })
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

/// The server: until stdin closes.
pub fn serve(call: &dyn Fn(&str, Value) -> Result<Value, String>) {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let reply = |out: &mut std::io::StdoutLock, id: Value, result: Value| {
        let _ = writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        let _ = out.flush();
    };
    let fail = |out: &mut std::io::StdoutLock, id: Value, code: i64, msg: &str| {
        let _ = writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }));
        let _ = out.flush();
    };
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            fail(&mut out, Value::Null, -32700, "parse error");
            continue;
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "initialize" => {
                let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2024-11-05");
                reply(&mut out, id, json!({ "protocolVersion": version, "capabilities": { "tools": {} }, "serverInfo": { "name": "nus", "version": env!("CARGO_PKG_VERSION") } }));
            }
            "notifications/initialized" | "notifications/cancelled" => {}
            "ping" => reply(&mut out, id, json!({})),
            "tools/list" => reply(&mut out, id, json!({ "tools": tools().into_iter().map(|(t, _, _)| t).collect::<Vec<_>>() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = call_tool(name, &args, call);
                reply(&mut out, id, result);
            }
            _ if id.is_null() => {}
            _ => fail(&mut out, id, -32601, "method not found"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn tools_map_to_verbs() {
        let calls = std::cell::RefCell::new(Vec::new());
        let call = |cmd: &str, args: Value| {
            calls.borrow_mut().push((cmd.to_string(), args));
            Ok(json!({ "title": "T", "text": "hello" }))
        };
        let r = call_tool("nus_page_text", &json!({ "tab": 2 }), &call);
        assert_eq!(r["content"][0]["text"], "# T\n\nhello");
        let c = calls.borrow();
        assert_eq!(c[0].0, "page");
        assert_eq!(c[0].1["what"], "text");
        assert_eq!(c[0].1["tab"], 2);
        let bad = call_tool("nope", &json!({}), &call);
        assert_eq!(bad["isError"], true);
    }
}
