//! The phone's nus: a page this window serves on the LAN, for the
//! machine that only has a browser. The front page — what ran and
//! failed while you were away, what is listening, what is asking for
//! hands, the tabs — and a line to ask the assistant; hands are answered
//! from the phone. Off unless SYNC · THE PHONE turns it on; a token in
//! the URL, made when it is turned on; plain HTTP on the local network
//! and nothing else. No app to ship: the phone's own browser.
//!
//! The server is a thread on a socket; everything it knows comes from the
//! same request channel `nus` remote control uses (a `front` verb, a
//! `hands-answer` verb), so nothing here reaches the app any other way.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;

use serde_json::Value;

use crate::little::Inbound;

#[derive(Clone)]
pub struct Phone {
    pub port: u16,
    pub token: String,
    /// The LAN address the phone should use, if one could be told.
    pub host: String,
    alive: Arc<AtomicBool>,
    worker: Arc<std::sync::Mutex<Option<std::thread::JoinHandle<()>>>>,
}

static PHONE: std::sync::Mutex<Option<Phone>> = std::sync::Mutex::new(None);

/// The server, if it is up.
pub fn current() -> Option<Phone> {
    PHONE.lock().ok().and_then(|g| g.clone())
}

/// Revoke the address immediately and release the listening socket.
pub fn stop() {
    if let Ok(mut guard) = PHONE.lock() {
        if let Some(phone) = guard.take() {
            phone.alive.store(false, Ordering::Release);
            if let Ok(mut worker) = phone.worker.lock() { if let Some(worker) = worker.take() { let _ = worker.join(); } }
        }
    }
    let _ = std::fs::remove_file(std::env::current_dir().unwrap_or_default().join("profile/phone"));
}

/// Turn it on once per process; on again is the same server.
pub fn start(tx: Sender<Inbound>) -> Option<Phone> {
    if let Some(p) = current() {
        return Some(p);
    }
    let p = serve(tx)?;
    if let Ok(mut g) = PHONE.lock() {
        *g = Some(p.clone());
    }
    // The address beside the instance file, for the CLI and for you.
    let dir = std::env::current_dir().unwrap_or_default().join("profile");
    let _ = std::fs::write(dir.join("phone"), p.url());
    Some(p)
}

impl Phone {
    pub fn url(&self) -> String {
        format!("http://{}:{}/?t={}", self.host, self.port, self.token)
    }
}

/// The address this machine has on its network: what a UDP socket would
/// leave from toward the wider internet (no packet is sent).
pub fn lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

/// Serve on every interface; the port is the OS's pick.
pub fn serve(tx: Sender<Inbound>) -> Option<Phone> {
    let l = TcpListener::bind("0.0.0.0:0").ok()?;
    let port = l.local_addr().ok()?.port();
    let token = crate::remote::new_token();
    let t2 = token.clone();
    l.set_nonblocking(true).ok()?;
    let alive = Arc::new(AtomicBool::new(true));
    let running = alive.clone();
    let worker = std::thread::Builder::new()
        .name("nus-phone".into())
        .spawn(move || {
            while running.load(Ordering::Acquire) {
                match l.accept() {
                    Ok((conn, _)) => {
                        let tx = tx.clone();
                        let token = t2.clone();
                        let running = running.clone();
                        std::thread::spawn(move || handle(conn, tx, &token, &running));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(30)),
                    Err(_) => break,
                }
            }
        })
        .ok()?;
    Some(Phone { port, token, host: lan_ip(), alive, worker: Arc::new(std::sync::Mutex::new(Some(worker))) })
}

/// One request, answered in full. Only what the page needs: GET /, POST
/// /ask, POST /hands; the token on every one.
fn handle(mut conn: TcpStream, tx: Sender<Inbound>, token: &str, alive: &AtomicBool) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(conn.try_clone().ok().unwrap_or_else(|| conn.try_clone().unwrap()));
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h == "\r\n" || h == "\n" || h.is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len.min(64 * 1024)];
    if len > 0 {
        let _ = reader.read_exact(&mut body);
    }
    let body = String::from_utf8_lossy(&body).to_string();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut form = parse_form(query);
    if method == "POST" {
        form.extend(parse_form(&body));
    }
    if !alive.load(Ordering::Acquire) || form.get("t").map(String::as_str) != Some(token) {
        return respond(&mut conn, 403, "text/plain", "no");
    }
    let ask = |cmd: &str, args: Value| -> Value {
        if !alive.load(Ordering::Acquire) { return Value::Null; }
        let (rtx, rrx) = channel();
        if tx.send(Inbound::Request(crate::remote::Request { cmd: cmd.into(), args, reply: rtx })).is_err() {
            return Value::Null;
        }
        // The wire's shape: {"ok": true, "result": …}.
        let mut v = rrx.recv_timeout(Duration::from_secs(10)).unwrap_or(Value::Null);
        if let Some(r) = v.get_mut("result") {
            return r.take();
        }
        v
    };
    match (method.as_str(), path) {
        ("GET", "/") => {
            let front = ask("front", Value::Null);
            respond(&mut conn, 200, "text/html; charset=utf-8", &front_html(&front, token));
        }
        ("POST", "/ask") => {
            let q = form.get("q").cloned().unwrap_or_default();
            let answer = if q.trim().is_empty() { Err("nothing asked".to_string()) } else { crate::ask::answer(&q) };
            respond(&mut conn, 200, "text/html; charset=utf-8", &answer_html(&q, answer, token));
        }
        ("POST", "/hands") => {
            let tab = form.get("tab").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let right = form.get("right").map(|v| v == "1").unwrap_or(false);
            let a = form.get("a").cloned().unwrap_or_else(|| "deny".into());
            let _ = ask("hands-answer", serde_json::json!({ "tab": tab, "right": right, "answer": a }));
            respond(&mut conn, 303, "text/plain", &format!("/?t={token}"));
        }
        _ => respond(&mut conn, 404, "text/plain", "not here"),
    }
}

fn parse_form(s: &str) -> std::collections::HashMap<String, String> {
    s.split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((unescape(k), unescape(v)))
        })
        .collect()
}

fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn respond(conn: &mut TcpStream, code: u16, kind: &str, body: &str) {
    let reason = match code { 200 => "OK", 303 => "See Other", 403 => "Forbidden", _ => "Not Found" };
    let extra = if code == 303 { format!("Location: {body}\r\n") } else { String::new() };
    let body = if code == 303 { "" } else { body };
    let _ = write!(conn, "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n{extra}\r\n{body}", body.len());
    let _ = conn.flush();
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The page's dress: paper, ink, signal, mono; big targets; no script.
const CSS: &str = "body{margin:0;background:#fff;color:#141413;font:15px/1.5 ui-monospace,Menlo,Consolas,monospace;padding:0 16px 40px}\
h1{font:italic 28px Georgia,serif;margin:18px 0 2px}.dim{color:#6b6862;font-size:12px;letter-spacing:.08em;text-transform:uppercase}\
.cap{color:#c8102e;font-size:11px;letter-spacing:.12em;text-transform:uppercase;margin:22px 0 6px}\
.row{display:flex;gap:10px;align-items:baseline;min-height:44px;border-bottom:1px solid #e4e4e4;padding:8px 0}.row .k{width:1.4em;color:#6b6862}\
form{margin:0}input[type=text]{width:100%;box-sizing:border-box;font:inherit;padding:12px;border:1px solid #141413;background:#fff;border-radius:0}\
button{font:inherit;font-weight:600;letter-spacing:.06em;text-transform:uppercase;padding:12px 16px;border:2px solid #141413;background:#fff;color:#141413;box-shadow:3px 3px 0 #141413;min-height:44px;margin:8px 8px 0 0}\
button.go{background:#c8102e;color:#fff}pre{white-space:pre-wrap;background:#fff;border:1px solid #141413;padding:12px}\
a{color:#c8102e}.bar{height:6px;background:#c8102e;margin:0 -16px}";

fn front_html(f: &Value, token: &str) -> String {
    let name = f.get("name").and_then(Value::as_str).unwrap_or("nus");
    let rows = |key: &str| -> String {
        f.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|r| format!("<div class=row><span class=k>{}</span><span>{}</span></div>", esc(r.get("mark").and_then(Value::as_str).unwrap_or("·")), esc(r.get("text").and_then(Value::as_str).unwrap_or("")))).collect::<Vec<_>>().join(""))
            .unwrap_or_default()
    };
    let mut hands = String::new();
    if let Some(a) = f.get("hands").and_then(Value::as_array) {
        for h in a {
            let (tab, right) = (h.get("tab").and_then(Value::as_u64).unwrap_or(0), h.get("right").and_then(Value::as_bool).unwrap_or(false));
            hands.push_str(&format!(
                "<div class=row><span class=k>✋</span><span>{} · {}</span></div><form method=post action=/hands><input type=hidden name=t value=\"{}\"><input type=hidden name=tab value={}><input type=hidden name=right value={}><button class=go name=a value=allow>allow</button><button name=a value=deny>deny</button></form>",
                esc(h.get("who").and_then(Value::as_str).unwrap_or("")),
                esc(h.get("what").and_then(Value::as_str).unwrap_or("")),
                esc(token),
                tab,
                if right { 1 } else { 0 }
            ));
        }
    }
    let news = rows("news");
    let ports = rows("ports");
    let tabs = rows("tabs");
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><meta http-equiv=refresh content=20><title>{name} · nus</title><style>{CSS}</style></head><body><div class=bar></div><h1>{name}</h1><div class=dim>nus · this window, from here</div>\
<form method=post action=/ask><div class=cap>ask</div><input type=hidden name=t value=\"{t}\"><input type=text name=q placeholder=\"how do I …\" autocomplete=off><button class=go>ask</button></form>\
{hands_cap}{hands}\
<div class=cap>while you were away</div>{news_or}\
<div class=cap>listening</div>{ports_or}\
<div class=cap>tabs</div>{tabs}\
</body></html>",
        name = esc(name),
        t = esc(token),
        hands_cap = if hands.is_empty() { "" } else { "<div class=cap>hands</div>" },
        news_or = if news.is_empty() { "<div class=row><span class=k>·</span><span>nothing to tell</span></div>".to_string() } else { news },
        ports_or = if ports.is_empty() { "<div class=row><span class=k>·</span><span>nothing listening</span></div>".to_string() } else { ports },
    )
}

fn answer_html(q: &str, a: Result<String, String>, token: &str) -> String {
    let body = match a {
        Ok(text) => format!("<pre>{}</pre>", esc(&text)),
        Err(e) => format!("<div class=row><span class=k>✗</span><span>{}</span></div>", esc(&e)),
    };
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>ask · nus</title><style>{CSS}</style></head><body><div class=bar></div><h1>ask</h1><div class=dim>{q}</div>{body}<p><a href=\"/?t={t}\">← the front page</a></p></body></html>",
        q = esc(q),
        t = esc(token)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_unescape() {
        let f = parse_form("t=abc&q=how+do+I+%3Cls%3E");
        assert_eq!(f.get("t").map(String::as_str), Some("abc"));
        assert_eq!(f.get("q").map(String::as_str), Some("how do I <ls>"));
    }

    #[test]
    fn the_front_page_carries_the_token_and_the_rows() {
        let f = serde_json::json!({ "name": "nus", "news": [{ "mark": "✗", "text": "cargo test · exit 1" }], "ports": [], "tabs": [], "hands": [{ "tab": 2, "right": true, "who": "claude", "what": "click" }] });
        let html = front_html(&f, "tok");
        assert!(html.contains("cargo test · exit 1"));
        assert!(html.contains("value=\"tok\""));
        assert!(html.contains("name=tab value=2"));
        assert!(html.contains("nothing listening"));
    }
}
