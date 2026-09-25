//! `nus hook` — an assistant's hooks, reporting to the pane they run in.
//!
//!   nus hook claude                 an event on stdin (Claude Code hooks)
//!   nus hook codex '<json>'         Codex's notify program: JSON as the argument
//!   nus hook install claude|codex   add these hooks to the assistant's config
//!   nus hook uninstall claude|codex take them out again
//!
//! A hook never slows the assistant down or changes what it does: it
//! prints nothing, gives up on nus after a moment, and always exits 0.
//! Outside a nus shell (no NUS_PANE) it returns at once. Only a few
//! fields travel, clipped: never whole tool inputs, prompts or files.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use serde_json::{json, Map, Value};

/// What a hook command in the assistant's config says. It checks for nus
/// itself, so the same config is harmless outside a nus shell.
pub const CLAUDE_COMMAND: &str = r#"[ -n "$NUS_CLI" ] && "$NUS_CLI" hook claude || true"#;
const CODEX_SCRIPT: &str = r#"[ -n "$NUS_CLI" ] && "$NUS_CLI" hook codex "$1" || true"#;

/// The Claude Code events nus listens to; tool events match every tool.
const CLAUDE_EVENTS: [(&str, bool); 7] = [
    ("SessionStart", false),
    ("UserPromptSubmit", false),
    ("PreToolUse", true),
    ("PostToolUse", true),
    ("Notification", false),
    ("Stop", false),
    ("SessionEnd", false),
];

pub fn run(words: &[String]) -> ExitCode {
    match words.first().map(String::as_str) {
        Some("install") => return install(words.get(1).map(String::as_str), true),
        Some("uninstall") => return install(words.get(1).map(String::as_str), false),
        _ => {}
    }
    // Whatever happens from here on, the assistant hears success.
    let _ = forward(words);
    ExitCode::SUCCESS
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn forward(words: &[String]) -> Option<()> {
    let pane = std::env::var("NUS_PANE").ok().filter(|p| !p.is_empty())?;
    let agent = words.first()?.as_str();
    let mut args = Map::new();
    args.insert("pane".into(), json!(pane));
    args.insert("pid".into(), json!(std::process::id()));
    args.insert("agent".into(), json!(agent));
    match agent {
        "claude" => {
            let mut raw = String::new();
            std::io::stdin()
                .take(1 << 20)
                .read_to_string(&mut raw)
                .ok()?;
            let ev: Value = serde_json::from_str(&raw).ok()?;
            claude_fields(&ev, &mut args);
        }
        "codex" => {
            let ev: Value = serde_json::from_str(words.get(1)?).ok()?;
            let s = |k: &str| ev.get(k).and_then(Value::as_str);
            args.insert("event".into(), json!(s("type")?));
            if let Some(m) = s("last-assistant-message") {
                args.insert("message".into(), json!(clip(m, 400)));
            }
        }
        _ => return None,
    }
    send(Value::Object(args))
}

/// The fields of a Claude Code hook event nus uses, and nothing else.
pub fn claude_fields(ev: &Value, args: &mut Map<String, Value>) {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let mut put = |k: &str, v: Option<String>, n: usize| {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            args.insert(k.into(), json!(clip(&v, n)));
        }
    };
    put("event", s(ev, "hook_event_name"), 64);
    put("session", s(ev, "session_id"), 80);
    put("cwd", s(ev, "cwd"), 400);
    put("message", s(ev, "message"), 400);
    put("tool", s(ev, "tool_name"), 64);
    let input = ev.get("tool_input").cloned().unwrap_or(Value::Null);
    put(
        "file",
        s(&input, "file_path")
            .or_else(|| s(&input, "notebook_path"))
            .or_else(|| s(&input, "path")),
        400,
    );
    put("command", s(&input, "command"), 300);
    put("pattern", s(&input, "pattern"), 120);
    put("url", s(&input, "url"), 300);
    put("query", s(&input, "query"), 200);
}

/// One request to the running nus, briefly: an assistant must not wait on us.
fn send(args: Value) -> Option<()> {
    let path = std::env::var_os("NUS_INSTANCE").map(PathBuf::from)?;
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let token = lines.next()?.trim().to_string();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(250)).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(250))).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    let req = json!({ "token": token, "cmd": "agent", "args": args, "protocol": nus_compat::CLI_PROTOCOL });
    writeln!(s, "{req}").ok()?;
    // Wait for the answer so the event is in before the next one; ignore it.
    let mut buf = [0u8; 256];
    let _ = s.read(&mut buf);
    Some(())
}

// ── Installing ───────────────────────────────────────────────────────────

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn claude_settings() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(d) => Some(PathBuf::from(d).join("settings.json")),
        None => Some(home()?.join(".claude").join("settings.json")),
    }
}

fn codex_config() -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME") {
        Some(d) => Some(PathBuf::from(d).join("config.toml")),
        None => Some(home()?.join(".codex").join("config.toml")),
    }
}

fn install(agent: Option<&str>, on: bool) -> ExitCode {
    let result = match agent {
        Some("claude") => claude_settings()
            .ok_or("no home folder".to_string())
            .and_then(|p| {
                edit_file(&p, |t| {
                    if on {
                        claude_install(t)
                    } else {
                        claude_uninstall(t)
                    }
                })
            }),
        Some("codex") => codex_config()
            .ok_or("no home folder".to_string())
            .and_then(|p| {
                edit_file(&p, |t| {
                    if on {
                        codex_install(t)
                    } else {
                        codex_uninstall(t)
                    }
                })
            }),
        _ => Err("nus hook install claude | codex".into()),
    };
    match result {
        Ok(said) => {
            println!("{said}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nus hook: {e}");
            ExitCode::from(1)
        }
    }
}

/// Read, change, and — when it changed — keep the original beside it once
/// and write the new one. The function says what it did.
fn edit_file(
    path: &std::path::Path,
    change: impl Fn(&str) -> Result<(String, String), String>,
) -> Result<String, String> {
    let before = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let (after, said) = change(&before)?;
    if after == before {
        return Ok(format!("{said} · {} unchanged", path.display()));
    }
    if !before.is_empty() {
        let backup = path.with_extension(format!(
            "{}.nus-backup",
            path.extension().and_then(|e| e.to_str()).unwrap_or("bak")
        ));
        if !backup.exists() {
            std::fs::write(&backup, &before).map_err(|e| format!("{}: {e}", backup.display()))?;
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, &after).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{said} · {}", path.display()))
}

fn ours(entry: &Value) -> bool {
    entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("hook claude") && c.contains("NUS_CLI"))
}

/// Claude Code's settings with nus's hooks added. A file without hooks
/// gets them inserted as text, the rest byte for byte as it was; one with
/// hooks of its own is merged and rewritten (its backup is kept).
pub fn claude_install(text: &str) -> Result<(String, String), String> {
    let mut root: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text)
            .map_err(|e| format!("settings.json does not parse ({e}); nothing changed"))?
    };
    let obj = root
        .as_object_mut()
        .ok_or("settings.json is not an object; nothing changed")?;
    let entry = |tools: bool| {
        let mut e =
            json!({ "hooks": [{ "type": "command", "command": CLAUDE_COMMAND, "timeout": 5 }] });
        if tools {
            e["matcher"] = json!("*");
        }
        e
    };
    if !obj.contains_key("hooks") {
        let hooks: Map<String, Value> = CLAUDE_EVENTS
            .iter()
            .map(|&(ev, tools)| (ev.to_string(), json!([entry(tools)])))
            .collect();
        let block = serde_json::to_string_pretty(&json!({ "hooks": hooks })).unwrap_or_default();
        // `{ "hooks": … }` → the member, indented one level.
        let inner: Vec<&str> = block.lines().collect();
        let member = inner[1..inner.len() - 1].join("\n");
        let out = if obj.is_empty() {
            format!("{{\n{member}\n}}\n")
        } else {
            let end = text
                .rfind('}')
                .ok_or("settings.json has no closing brace")?;
            let head = text[..end].trim_end();
            format!("{head},\n{member}\n}}{}", &text[end + 1..])
        };
        serde_json::from_str::<Value>(&out)
            .map_err(|e| format!("could not insert hooks safely ({e}); nothing changed"))?;
        return Ok((
            out,
            format!(
                "added nus hooks for {} Claude Code events",
                CLAUDE_EVENTS.len()
            ),
        ));
    }
    let hooks = obj
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .ok_or("settings.json has hooks that are not an object; nothing changed")?;
    let mut added = 0;
    for &(ev, tools) in &CLAUDE_EVENTS {
        let list = hooks.entry(ev).or_insert_with(|| json!([]));
        let Some(list) = list.as_array_mut() else {
            return Err(format!("hooks.{ev} is not a list; nothing changed"));
        };
        let present = list.iter().any(|e| {
            e.get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|h| h.iter().any(ours))
        });
        if !present {
            list.push(entry(tools));
            added += 1;
        }
    }
    let out = format!(
        "{}\n",
        serde_json::to_string_pretty(&root).unwrap_or_default()
    );
    let said = if added == 0 {
        "nus hooks already present".to_string()
    } else {
        format!("added nus hooks for {added} Claude Code events (file rewritten, keys sorted; backup kept)")
    };
    Ok((if added == 0 { text.to_string() } else { out }, said))
}

/// The same settings with nus's hooks, and only those, taken out.
pub fn claude_uninstall(text: &str) -> Result<(String, String), String> {
    if text.trim().is_empty() {
        return Ok((text.to_string(), "no settings".into()));
    }
    let mut root: Value = serde_json::from_str(text)
        .map_err(|e| format!("settings.json does not parse ({e}); nothing changed"))?;
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok((text.to_string(), "no nus hooks".into()));
    };
    let mut removed = 0;
    for list in hooks.values_mut().filter_map(Value::as_array_mut) {
        for e in list.iter_mut() {
            if let Some(h) = e.get_mut("hooks").and_then(Value::as_array_mut) {
                let n = h.len();
                h.retain(|c| !ours(c));
                removed += n - h.len();
            }
        }
        list.retain(|e| {
            e.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|h| !h.is_empty())
        });
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|l| !l.is_empty()));
    if hooks.is_empty() {
        root.as_object_mut().map(|o| o.remove("hooks"));
    }
    if removed == 0 {
        return Ok((text.to_string(), "no nus hooks".into()));
    }
    Ok((
        format!(
            "{}\n",
            serde_json::to_string_pretty(&root).unwrap_or_default()
        ),
        format!("removed {removed} nus hooks"),
    ))
}

fn codex_line() -> String {
    format!(
        "notify = [\"sh\", \"-c\", {}, \"nus-hook\"]",
        serde_json::to_string(CODEX_SCRIPT).unwrap_or_default()
    )
}

/// Codex's config with nus as its notify program. A top-level key goes
/// before the first table, so it goes first; Codex takes one notify
/// program, so one that is already set is left alone.
pub fn codex_install(text: &str) -> Result<(String, String), String> {
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with('[') {
            break;
        }
        if l.starts_with("notify") && l[6..].trim_start().starts_with('=') {
            return if l.contains("hook codex") {
                Ok((text.to_string(), "nus notify already set".into()))
            } else {
                Err("config.toml already has a notify program; nus leaves it alone".into())
            };
        }
    }
    Ok((format!("# nus: turn-complete events to the pane this ran in (nus hook uninstall codex)\n{}\n{text}", codex_line()), "set nus as Codex's notify program".into()))
}

pub fn codex_uninstall(text: &str) -> Result<(String, String), String> {
    let mut removed = false;
    let out: Vec<&str> = text
        .lines()
        .filter(|l| {
            let drop = (l.trim_start().starts_with("notify") && l.contains("hook codex"))
                || l.starts_with("# nus: turn-complete events");
            removed |= drop;
            !drop
        })
        .collect();
    if !removed {
        return Ok((text.to_string(), "no nus notify".into()));
    }
    let mut out = out.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok((out, "removed nus as Codex's notify program".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_hooks_go_in_as_text_and_come_out_clean() {
        let mine =
            "{\n  \"model\": \"opus\",\n  \"permissions\": { \"allow\": [\"Bash(ls)\"] }\n}\n";
        let (with, said) = claude_install(mine).unwrap();
        assert!(said.contains("7"));
        // What was there is untouched, in its order and its formatting.
        assert!(with.starts_with("{\n  \"model\": \"opus\",\n  \"permissions\": { \"allow\": [\"Bash(ls)\"] },\n  \"hooks\": {"));
        let v: Value = serde_json::from_str(&with).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], CLAUDE_COMMAND);
        assert!(v["hooks"]["Stop"][0].get("matcher").is_none());
        // Twice is once.
        let (again, said) = claude_install(&with).unwrap();
        assert_eq!(again, with);
        assert!(said.contains("already"));
        let (without, _) = claude_uninstall(&with).unwrap();
        let v: Value = serde_json::from_str(&without).unwrap();
        assert_eq!(v, serde_json::from_str::<Value>(mine).unwrap());
    }

    #[test]
    fn claude_hooks_merge_with_the_users_own() {
        let mine = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#;
        let (with, _) = claude_install(mine).unwrap();
        let v: Value = serde_json::from_str(&with).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        let (without, _) = claude_uninstall(&with).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&without).unwrap(),
            serde_json::from_str::<Value>(mine).unwrap()
        );
        assert!(claude_install("{ not json").is_err());
        assert!(claude_install("").unwrap().0.contains("\"SessionStart\""));
    }

    #[test]
    fn codex_notify_goes_first_and_respects_one_already_set() {
        let mine = "model = \"gpt-5\"\n\n[profiles.fast]\nmodel = \"o4\"\n";
        let (with, _) = codex_install(mine).unwrap();
        assert!(with.contains("notify = [\"sh\", \"-c\", \"[ -n \\\"$NUS_CLI\\\" ]"));
        assert!(with.find("notify").unwrap() < with.find("[profiles").unwrap());
        assert!(with.ends_with(mine));
        assert_eq!(codex_install(&with).unwrap().0, with);
        assert_eq!(codex_uninstall(&with).unwrap().0, mine);
        assert!(codex_install("notify = [\"terminal-notifier\"]\n").is_err());
    }

    #[test]
    fn only_the_fields_nus_uses_travel() {
        let ev = json!({
            "hook_event_name": "PreToolUse", "session_id": "s1", "cwd": "/x",
            "transcript_path": "/secret/transcript.jsonl",
            "tool_name": "Bash", "tool_input": { "command": "cargo test", "env": {"TOKEN": "t"} }
        });
        let mut args = Map::new();
        claude_fields(&ev, &mut args);
        assert_eq!(args["event"], "PreToolUse");
        assert_eq!(args["command"], "cargo test");
        assert!(!args.contains_key("transcript_path"));
        assert!(!serde_json::to_string(&args).unwrap().contains("TOKEN"));
    }
}
