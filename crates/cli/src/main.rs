//! `nus` — drive a running nus from a shell or a script.
//!
//!   nus ls                          the space, its tabs and panes (JSON with --json)
//!   nus open <url> [--split]        a page, in a tab or beside this shell
//!   nus edit <file> [--split]       a file, in the editor
//!   nus launch [--profile P] [--cwd D] [--run CMD] [--split]
//!   nus send-text <text> [--tab N] [--right] [--enter]
//!   nus focus <tab>                 by number
//!   nus close [<tab>] [--force]
//!   nus theme [<name>]              the stock themes, or apply one
//!   nus look [ink|paper] [--signal #rrggbb]
//!   nus ports                       what's listening, as the board sees it
//!   nus hatch [toggle|show|hide|work|list|open --window ID --tab-id ID [--right]|hoist|land|quit]
//!   nus block [last|all] [--tab N]  a shell's blocks, command and output
//!   nus ask <question…>             the assistant, beside this shell
//!   nus raise                       bring the window up
//!   nus version
//!
//! It finds the running instance through `profile/instance` next to the
//! app (or NUS_INSTANCE=<path>), which carries the port and the token.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{json, Value};

fn instance_file() -> PathBuf {
    if let Some(p) = std::env::var_os("NUS_INSTANCE") {
        return PathBuf::from(p);
    }
    // Beside the executable, then the working directory: the app writes
    // profile/instance under its own cwd.
    let candidates = [
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("profile").join("instance"))),
        std::env::current_dir()
            .ok()
            .map(|d| d.join("profile").join("instance")),
        std::env::current_dir().ok().map(|d| {
            d.join("spikes")
                .join("composite")
                .join("profile")
                .join("instance")
        }),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("profile/instance"))
}

fn connect() -> Result<(TcpStream, String), String> {
    let text = std::fs::read_to_string(instance_file())
        .map_err(|_| "nus is not running (no profile/instance)".to_string())?;
    let mut lines = text.lines();
    let port: u16 = lines
        .next()
        .unwrap_or("")
        .trim()
        .parse()
        .map_err(|_| "profile/instance has no port".to_string())?;
    let token = lines.next().unwrap_or("").trim().to_string();
    if token.is_empty() {
        return Err("this nus predates remote control · restart it".into());
    }
    let s = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("nus is not answering on {port}: {e}"))?;
    Ok((s, token))
}

fn call(cmd: &str, args: Value) -> Result<Value, String> {
    let (mut s, token) = connect()?;
    let req = json!({ "token": token, "cmd": cmd, "args": args });
    writeln!(s, "{req}").map_err(|e| e.to_string())?;
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(line.trim()).map_err(|e| format!("bad reply: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_string())
    }
}

/// Split `--key value` and `--flag` out of the words.
fn parse(args: &[String]) -> (Vec<String>, serde_json::Map<String, Value>) {
    let mut words = Vec::new();
    let mut opts = serde_json::Map::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(k) = a.strip_prefix("--") {
            let key = k.replace('-', "_");
            let takes_value = matches!(
                k,
                "tab" | "tab-id" | "window" | "profile" | "cwd" | "run" | "signal" | "which"
            );
            if takes_value && i + 1 < args.len() {
                let v = &args[i + 1];
                let val = v
                    .parse::<u64>()
                    .map(Value::from)
                    .unwrap_or_else(|_| Value::String(v.clone()));
                opts.insert(key, val);
                i += 2;
                continue;
            }
            opts.insert(key, Value::Bool(true));
        } else {
            words.push(a.clone());
        }
        i += 1;
    }
    (words, opts)
}

fn print_ls(v: &Value) {
    let space = v.get("space").and_then(Value::as_str).unwrap_or("");
    let theme = v.get("theme").and_then(Value::as_str).unwrap_or("");
    println!("{space} · {theme}");
    for t in v
        .get("tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let active = if t.get("active").and_then(Value::as_bool) == Some(true) {
            "*"
        } else {
            " "
        };
        let label = t.get("label").and_then(Value::as_str).unwrap_or("");
        let title = t.get("title").and_then(Value::as_str).unwrap_or("");
        let hatch = if t.get("hatch").and_then(Value::as_bool) == Some(true) {
            " · hatch"
        } else {
            ""
        };
        let pane = |p: &Value| -> String {
            match p.get("kind").and_then(Value::as_str) {
                Some("shell") => format!(
                    "shell {}",
                    p.get("cwd").and_then(Value::as_str).unwrap_or("")
                ),
                Some("page") => format!(
                    "page {}",
                    p.get("url").and_then(Value::as_str).unwrap_or("")
                ),
                Some("editor") => format!(
                    "editor {}",
                    p.get("path").and_then(Value::as_str).unwrap_or("")
                ),
                Some(k) => k.to_string(),
                None => String::new(),
            }
        };
        let left = t.get("left").map(pane).unwrap_or_default();
        let right = t
            .get("right")
            .filter(|r| !r.is_null())
            .map(|r| format!(" | {}", pane(r)))
            .unwrap_or_default();
        println!("{active} {label}  {title}{hatch}\n     {left}{right}");
    }
}

mod mcp;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The MCP server: stdin to stdout until the assistant hangs up.
    if args.first().map(String::as_str) == Some("mcp") {
        mcp::serve(&|cmd, a| call(cmd, a));
        return ExitCode::SUCCESS;
    }
    let (words, mut opts) = parse(&args);
    let want_json = opts.remove("json").is_some();
    let Some(cmd) = words.first().cloned() else {
        eprintln!("{}", USAGE);
        return ExitCode::from(2);
    };
    let rest: Vec<String> = words[1..].to_vec();
    let (cmd, args): (&str, Value) = match cmd.as_str() {
        "ls" | "version" | "raise" | "ports" | "split" | "log" => {
            (cmd.as_str(), Value::Object(opts))
        }
        "hold" => {
            if let Some(w) = rest.first() {
                opts.insert("what".into(), Value::String(w.clone()));
            }
            if let Some(id) = rest.get(1) {
                opts.insert("id".into(), Value::String(id.clone()));
            }
            ("hold", Value::Object(opts))
        }
        "page" => {
            opts.insert(
                "what".into(),
                Value::String(rest.first().cloned().unwrap_or_else(|| "text".into())),
            );
            if let Some(sel) = rest.get(1) {
                opts.insert("selector".into(), Value::String(sel.clone()));
            }
            ("page", Value::Object(opts))
        }
        "open" => {
            opts.insert("url".into(), Value::String(rest.join(" ")));
            ("open", Value::Object(opts))
        }
        "edit" => {
            let p = rest.join(" ");
            let abs = std::fs::canonicalize(&p)
                .map(|a| a.to_string_lossy().trim_start_matches(r"\\?\").to_string())
                .unwrap_or(p);
            opts.insert("path".into(), Value::String(abs));
            ("edit", Value::Object(opts))
        }
        "launch" => {
            if opts.get("cwd").is_none() {
                if let Ok(d) = std::env::current_dir() {
                    opts.insert("cwd".into(), Value::String(d.display().to_string()));
                }
            }
            ("launch", Value::Object(opts))
        }
        "send-text" | "send" => {
            opts.insert("text".into(), Value::String(rest.join(" ")));
            ("send-text", Value::Object(opts))
        }
        "focus" => {
            if let Some(n) = rest.first().and_then(|s| s.parse::<u64>().ok()) {
                opts.insert("tab".into(), Value::from(n));
            }
            ("focus", Value::Object(opts))
        }
        "close" => {
            if let Some(n) = rest.first().and_then(|s| s.parse::<u64>().ok()) {
                opts.insert("tab".into(), Value::from(n));
            }
            ("close", Value::Object(opts))
        }
        "theme" => {
            if let Some(n) = rest.first() {
                opts.insert("name".into(), Value::String(rest.join(" ")));
                let _ = n;
            }
            ("theme", Value::Object(opts))
        }
        "look" => {
            if let Some(m) = rest.first() {
                opts.insert("mode".into(), Value::String(m.clone()));
            }
            ("look", Value::Object(opts))
        }
        "hatch" => {
            opts.insert(
                "do".into(),
                Value::String(rest.first().cloned().unwrap_or_else(|| "toggle".into())),
            );
            ("hatch", Value::Object(opts))
        }
        "ssh" => {
            opts.insert(
                "host".into(),
                Value::String(rest.first().cloned().unwrap_or_default()),
            );
            ("ssh", Value::Object(opts))
        }
        "sync" => {
            let what = rest.first().cloned().unwrap_or_else(|| "now".into());
            let arg = rest.get(1).cloned().unwrap_or_default();
            match what.as_str() {
                "join" => {
                    opts.insert("key".into(), Value::String(arg));
                }
                "folder" => {
                    opts.insert("path".into(), Value::String(arg));
                }
                "git" => {
                    opts.insert("remote".into(), Value::String(arg));
                }
                _ => {}
            }
            opts.insert("do".into(), Value::String(what));
            ("sync", Value::Object(opts))
        }
        "layout" => {
            if rest.first().map(String::as_str) == Some("save") {
                opts.insert(
                    "save".into(),
                    Value::String(rest.get(1).cloned().unwrap_or_else(|| "layout".into())),
                );
            }
            ("layout", Value::Object(opts))
        }
        "block" => {
            opts.insert(
                "which".into(),
                Value::String(rest.first().cloned().unwrap_or_else(|| "last".into())),
            );
            ("block", Value::Object(opts))
        }
        "ask" => {
            opts.insert("q".into(), Value::String(rest.join(" ")));
            ("ask", Value::Object(opts))
        }
        "help" | "-h" | "--help" => {
            println!("{}", USAGE);
            return ExitCode::SUCCESS;
        }
        other => {
            // A bare file or URL: open it, as `nus <file>` at a prompt does.
            if std::path::Path::new(other).is_file() {
                let abs = std::fs::canonicalize(other)
                    .map(|a| a.to_string_lossy().trim_start_matches(r"\\?\").to_string())
                    .unwrap_or(other.to_string());
                opts.insert("path".into(), Value::String(abs));
                ("edit", Value::Object(opts))
            } else if other.contains("://") || other.starts_with("localhost") {
                opts.insert("url".into(), Value::String(other.to_string()));
                ("open", Value::Object(opts))
            } else {
                eprintln!("nus: unknown command {other}\n{}", USAGE);
                return ExitCode::from(2);
            }
        }
    };
    match call(cmd, args) {
        Ok(v) => {
            if want_json
                || !matches!(
                    cmd,
                    "ls" | "version" | "ports" | "block" | "theme" | "layout" | "sync"
                )
            {
                if !v.is_null() {
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                }
            } else {
                match cmd {
                    "ls" => print_ls(&v),
                    "version" => println!(
                        "nus {}",
                        v.get("nus").and_then(Value::as_str).unwrap_or("?")
                    ),
                    "ports" => {
                        for p in v
                            .get("ports")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            println!(
                                "{:>5}  {:<24} {:<12} {}{}",
                                p.get("port").and_then(Value::as_u64).unwrap_or(0),
                                p.get("title").and_then(Value::as_str).unwrap_or(""),
                                p.get("process").and_then(Value::as_str).unwrap_or(""),
                                p.get("group").and_then(Value::as_str).unwrap_or(""),
                                if p.get("exposed").and_then(Value::as_bool) == Some(true) {
                                    " · exposed"
                                } else {
                                    ""
                                }
                            );
                        }
                    }
                    "block" => {
                        for b in v
                            .get("blocks")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            println!("$ {}", b.get("cmd").and_then(Value::as_str).unwrap_or(""));
                            print!("{}", b.get("output").and_then(Value::as_str).unwrap_or(""));
                            if let Some(e) = b.get("exit").and_then(Value::as_i64) {
                                println!("[exit {e}]");
                            }
                        }
                    }
                    "theme" => {
                        for t in v
                            .get("themes")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            println!("{}", t.as_str().unwrap_or(""));
                        }
                    }
                    "sync" => {
                        if let Some(k) = v.get("key").and_then(Value::as_str) {
                            println!("{k}");
                        } else if let Some(st) = v.get("status").and_then(Value::as_str) {
                            println!("{st}");
                        } else {
                            println!("ok");
                        }
                    }
                    "layout" => {
                        for l in v
                            .get("layouts")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            println!(
                                "{}  {}",
                                l.get("name").and_then(Value::as_str).unwrap_or(""),
                                l.get("path").and_then(Value::as_str).unwrap_or("")
                            );
                        }
                        if let Some(c) = v.get("current").and_then(Value::as_str) {
                            println!("\n-- this window, as a layout:\n{c}");
                        }
                    }
                    _ => {}
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nus: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: nus <command> [args] [--json]
  ls · open <url> [--split] · edit <file> [--split] · launch [--profile P] [--cwd D] [--run CMD] [--split]
  send-text <text> [--tab N] [--right] [--enter] · focus <tab> · close [<tab>] [--force]
  theme [<name>] · look [ink|paper] [--signal #rrggbb] · ports · hatch [toggle|show|hide|work|list|open --window ID --tab-id ID [--right]|hoist|land|quit]
  block [last|all] [--tab N] · ask <question> · raise · version
  layout · layout save <name> · open <file>.nus.luau · ssh <host> [--split]
  sync [now] · sync key · sync join <key> · sync status · sync folder <path> · sync git <remote>
  hold [ls|attach <id>|kill <id>] · log [--cwd D] [--limit N] · page [text|dom|console|network|screenshot|info] [--tab N]
  mcp · the MCP server on stdio: claude mcp add nus -- nus mcp
  a bare <file> or <url> opens it";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_and_words() {
        let a: Vec<String> = [
            "launch",
            "--profile",
            "pwsh",
            "--split",
            "--cwd",
            "C:\\x",
            "extra",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (w, o) = parse(&a);
        assert_eq!(w, vec!["launch", "extra"]);
        assert_eq!(o.get("profile").and_then(Value::as_str), Some("pwsh"));
        assert_eq!(o.get("split"), Some(&Value::Bool(true)));
        assert_eq!(o.get("cwd").and_then(Value::as_str), Some("C:\\x"));
        let (_, o) = parse(&["focus".into(), "--tab".into(), "3".into()]);
        assert_eq!(o.get("tab"), Some(&Value::from(3u64)));
    }
}
