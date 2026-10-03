//! `nus sync …` beyond now/key/join/status/folder/git: pairing, the paper
//! key, devices, rotation, conflict copies, restore, and a GitHub sign-in.
//!
//! The running nus does the work: the sync key and the forge token live in
//! its vault, and pairing runs inside it, so the key never passes through
//! this command, its arguments or its output (the paper key's words are the
//! one exception, and only to a terminal). This side shows codes, progress
//! and choices, and asks before anything that cannot be undone.
//!
//!   nus sync pair [<code>] [--to ADDR] [--discover] [--qr] [--yes]
//!   nus sync key --paper [--force]
//!   nus sync devices
//!   nus sync rotate [--yes]
//!   nus sync conflicts [diff <n> | keep <n> mine|theirs]
//!   nus sync restore <file> [--at 3h|2d|2026-10-01|2026-10-01T14:30]
//!   nus sync forge github [--gh]

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::ui::{took, Ui};

fn sync(args: Value) -> Result<Value, String> {
    crate::call("sync", args)
}

fn fail(ui: &Ui, why: &str) -> ExitCode {
    ui.fail(why);
    ExitCode::FAILURE
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// The words that are not flags or flag values.
fn words(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a == "--to" || a == "--at" {
            skip = true;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a.as_str());
        }
    }
    out
}

fn ask(prompt: &str) -> String {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn ago(at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(at);
    let s = now.saturating_sub(at);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

/// Wait for the app's job, a spinner on the step's line meanwhile.
fn wait_job(ui: &Ui, n: &str, name: &str, doing: &str, until: Duration) -> Result<Value, String> {
    let start = Instant::now();
    let mut i = 0;
    loop {
        let v = sync(json!({ "do": "job" }))?;
        if v.get("done").is_some() {
            if ui.tty {
                print!("\r\x1b[2K");
            }
            return Ok(v["result"].clone());
        }
        if start.elapsed() > until {
            return Err(format!("{name} took too long"));
        }
        if ui.tty {
            print!("\r");
            ui.row_inline(n, name, doing, &ui.spinner(i), &took(start));
        }
        i += 1;
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// The verbs this module answers; None for the rest (the plain ones).
pub fn run(args: &[String]) -> Option<ExitCode> {
    let verb = args.first().map(String::as_str)?;
    let rest = &args[1..];
    Some(match verb {
        "pair" => pair(rest),
        "key" if flag(rest, "--paper") => paper(rest),
        "devices" => devices(),
        "rotate" => rotate(rest),
        "conflicts" => conflicts(rest),
        "restore" => restore(rest),
        "forge" => forge(rest),
        _ => return None,
    })
}

// --- pairing ------------------------------------------------------------------

/// A QR code of `text`, two modules per character cell (half blocks), with a
/// quiet zone; ASCII where the terminal has no block glyphs.
fn qr(ui: &Ui, text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let w = code.width();
    let dark = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < w
            && code[(x as usize, y as usize)] == qrcode::Color::Dark
    };
    let q = 2i64;
    let mut out = String::new();
    let mut y = -q;
    while y < w as i64 + q {
        out.push_str("    ");
        for x in -q..w as i64 + q {
            let (a, b) = (dark(x, y), dark(x, y + 1));
            // Light modules drawn as ink would invert the code on a dark
            // terminal; drawing the dark modules works on both.
            out.push_str(if ui.utf() {
                match (a, b) {
                    (true, true) => "█",
                    (true, false) => "▀",
                    (false, true) => "▄",
                    _ => " ",
                }
            } else {
                match (a, b) {
                    (true, true) => "#",
                    (true, false) => "\"",
                    (false, true) => ".",
                    _ => " ",
                }
            });
        }
        out.push('\n');
        y += 2;
    }
    Some(out)
}

fn pair(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    let me = match sync(json!({ "do": "device" })) {
        Ok(v) => v,
        Err(e) => return fail(&ui, &e),
    };
    let keyed = me["keyed"] == true;
    let w = words(args);
    let mut code = w.first().map(|s| s.to_string());
    let mut to = opt(args, "--to").map(str::to_string);
    let discover = flag(args, "--discover");
    // No key here and no code: which side is this?
    if code.is_none() && !keyed && interactive() {
        println!("  This device has no sync key yet.");
        let a = ask(&format!(
            "  {} offer a new key from here, or {} receive one from a device that has it? [O/r] ",
            ui.bold("o"),
            ui.bold("r")
        ));
        if a.eq_ignore_ascii_case("r") {
            let c = ask("  The code the other device shows: ");
            if c.is_empty() {
                return fail(&ui, "no code");
            }
            code = Some(c);
            if to.is_none() && !discover {
                let t = ask("  Its address (like 192.168.1.20:51807): ");
                if !t.is_empty() {
                    to = Some(t);
                }
            }
        }
    }
    match code {
        Some(c) => receive(
            &ui,
            &c,
            to.as_deref(),
            discover,
            keyed,
            flag(args, "--yes") || flag(args, "-y"),
        ),
        None => offer(&ui, discover, flag(args, "--qr"), keyed),
    }
}

/// Ctrl+C while pairing: the app stops listening at once, rather than
/// holding the offer open until it lapses.
fn cancel_on_interrupt() {
    let _ = ctrlc::set_handler(|| {
        let _ = sync(json!({ "do": "pair-cancel" }));
        println!();
        std::process::exit(130);
    });
}

fn offer(ui: &Ui, discover: bool, show_qr: bool, keyed: bool) -> ExitCode {
    let start = Instant::now();
    let v = match sync(json!({ "do": "pair-offer", "discover": discover })) {
        Ok(v) => v,
        Err(e) => return fail(ui, &e),
    };
    cancel_on_interrupt();
    let code = v["code"].as_str().unwrap_or("");
    let addr = v["address"].as_str().unwrap_or("");
    if !keyed {
        ui.row(
            "01",
            "Key",
            "none here yet: made one",
            &ui.ok(),
            &took(start),
        );
    }
    ui.row("02", "Code", code, &ui.ok(), &took(start));
    let line = if discover {
        format!("nus sync pair {code} --discover")
    } else {
        format!("nus sync pair {code} --to {addr}")
    };
    println!();
    println!("  On the other device:");
    println!("    {}", ui.bold(&line));
    if show_qr {
        if let Some(q) = qr(ui, &line) {
            println!();
            print!("{q}");
        }
    }
    println!();
    ui.hint(&format!(
        "{} {} it works once and lapses in 3 minutes",
        if discover {
            "announced on this network (--discover)"
        } else {
            "nothing is announced on the network"
        },
        ui.g().dot
    ));
    println!();
    match wait_job(
        ui,
        "03",
        "Waiting",
        "for the other device",
        Duration::from_secs(200),
    ) {
        Ok(r) => {
            ui.row(
                "03",
                "Paired",
                &format!(
                    "{} has the key {} it syncs on its next run",
                    r["device"].as_str().unwrap_or("the other device"),
                    ui.g().dot
                ),
                &ui.ok(),
                &took(start),
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(ui, &e),
    }
}

fn receive(
    ui: &Ui,
    code: &str,
    to: Option<&str>,
    discover: bool,
    keyed: bool,
    yes: bool,
) -> ExitCode {
    if to.is_none() && !discover {
        return fail(ui, "give the address the other device shows: --to 192.168.1.20:51807 (or --discover on a home network)");
    }
    if keyed && !yes {
        if !interactive() {
            return fail(
                ui,
                "this device already has a sync key; receiving replaces it (add --yes)",
            );
        }
        println!("  This device already has a sync key. Receiving one replaces it; files sealed");
        println!("  with the old key stay readable only by devices that still hold it.");
        if !ask("  Replace it? [y/N] ").eq_ignore_ascii_case("y") {
            println!("  Nothing changed.");
            return ExitCode::SUCCESS;
        }
    }
    let start = Instant::now();
    if let Err(e) = sync(
        json!({ "do": "pair-receive", "code": code, "to": to.unwrap_or(""), "discover": discover, "replace": keyed }),
    ) {
        return fail(ui, &e);
    }
    cancel_on_interrupt();
    match wait_job(
        ui,
        "01",
        "Pairing",
        to.unwrap_or("finding the other device"),
        Duration::from_secs(140),
    ) {
        Ok(r) => {
            ui.row(
                "01",
                "Paired",
                &format!(
                    "the key from {} {} syncing now",
                    r["device"].as_str().unwrap_or("the other device"),
                    ui.g().dot
                ),
                &ui.ok(),
                &took(start),
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(ui, &e),
    }
}

// --- the key on paper --------------------------------------------------------------

fn paper(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    // A secret goes to a person, not a file or a pipe, unless insisted on.
    if !std::io::stdout().is_terminal() && !flag(args, "--force") {
        return fail(
            &ui,
            "the paper key is shown only on a terminal (--force to print it anyway)",
        );
    }
    let v = match sync(json!({ "do": "paper" })) {
        Ok(v) => v,
        Err(e) => return fail(&ui, &e),
    };
    let words: Vec<&str> = v["words"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    println!();
    println!("  {}", ui.bold("Your sync key, on paper"));
    println!(
        "  {}",
        ui.grey("Anyone with these 24 words can read your synced profile. Write them down;")
    );
    println!(
        "  {}",
        ui.grey("don't photograph them or keep them in a file. nus sync join takes them back.")
    );
    println!();
    for row in 0..6 {
        let mut line = String::from("  ");
        for col in 0..4 {
            let i = col * 6 + row;
            if let Some(w) = words.get(i) {
                line.push_str(&format!("{} {:<12}", ui.grey(&format!("{:>2}", i + 1)), w));
            }
        }
        println!("{line}");
    }
    println!();
    ExitCode::SUCCESS
}

// --- devices, rotation ------------------------------------------------------------

fn devices() -> ExitCode {
    let ui = Ui::new();
    let start = Instant::now();
    if let Err(e) = sync(json!({ "do": "devices" })) {
        return fail(&ui, &e);
    }
    let r = match wait_job(
        &ui,
        "01",
        "Devices",
        "reading the carriers",
        Duration::from_secs(60),
    ) {
        Ok(r) => r,
        Err(e) => return fail(&ui, &e),
    };
    let list = r["devices"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        ui.row(
            "01",
            "Devices",
            "none on the carriers yet",
            &ui.warn(),
            &took(start),
        );
        return ExitCode::SUCCESS;
    }
    for (k, d) in list.iter().enumerate() {
        let name = d["device"].as_str().unwrap_or("?");
        let nus = d["nus"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| format!(" {} nus {s}", ui.g().dot))
            .unwrap_or_default();
        let this = if d["this"] == true {
            format!(" {} this device", ui.g().dot)
        } else {
            String::new()
        };
        let detail = format!(
            "{} {} {} files{nus}{this}",
            ago(d["at"].as_u64().unwrap_or(0)),
            ui.g().dot,
            d["files"].as_u64().unwrap_or(0)
        );
        ui.row(&format!("{:02}", k + 1), name, &detail, &ui.ok(), "");
    }
    ExitCode::SUCCESS
}

fn rotate(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    if !flag(args, "--yes") {
        if !interactive() {
            return fail(&ui, "rotating the key needs a yes (add --yes)");
        }
        println!("  A new key is made and everything is sealed again under it. Every other");
        println!("  device's folder comes off the carriers: they read nothing new until they");
        println!("  pair again (nus sync pair). A git carrier's history keeps the old blobs,");
        println!("  sealed with the old key.");
        if ask("  Type rotate to go on: ") != "rotate" {
            println!("  Nothing changed.");
            return ExitCode::SUCCESS;
        }
    }
    let start = Instant::now();
    if let Err(e) = sync(json!({ "do": "rotate" })) {
        return fail(&ui, &e);
    }
    match wait_job(&ui, "01", "Rotate", "re-sealing", Duration::from_secs(600)) {
        Ok(r) => {
            ui.row(
                "01",
                "Rotate",
                &{
                    let n = r["removed"].as_u64().unwrap_or(0);
                    format!(
                        "new key {} {n} other device{} off the carriers",
                        ui.g().dot,
                        if n == 1 { "" } else { "s" }
                    )
                },
                &ui.ok(),
                &took(start),
            );
            if let Some(left) = r["unfinished"].as_array().filter(|a| !a.is_empty()) {
                ui.hint(&format!(
                    "not finished: {} · the next sync carries on",
                    left.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            ui.hint(&format!(
                "{} on each other device, and {} to write the new key down",
                ui.bold("nus sync pair"),
                ui.bold("nus sync key --paper")
            ));
            ExitCode::SUCCESS
        }
        Err(e) => fail(&ui, &e),
    }
}

// --- conflicts ---------------------------------------------------------------------

/// Lines only in `a` (-), only in `b` (+), in common ( ), by longest common
/// subsequence; past `limit` lines either side, a summary instead.
pub fn line_diff(a: &str, b: &str, limit: usize) -> Vec<(char, String)> {
    let (x, y): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    if x.len() > limit || y.len() > limit {
        return vec![(
            ' ',
            format!(
                "{} lines mine, {} theirs: too long to compare here",
                x.len(),
                y.len()
            ),
        )];
    }
    let (n, m) = (x.len(), y.len());
    let mut l = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if x[i] == y[j] {
                l[i + 1][j + 1] + 1
            } else {
                l[i + 1][j].max(l[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n && j < m {
        if x[i] == y[j] {
            out.push((' ', x[i].to_string()));
            i += 1;
            j += 1;
        } else if l[i + 1][j] >= l[i][j + 1] {
            out.push(('-', x[i].to_string()));
            i += 1;
        } else {
            out.push(('+', y[j].to_string()));
            j += 1;
        }
    }
    out.extend(x[i..].iter().map(|s| ('-', s.to_string())));
    out.extend(y[j..].iter().map(|s| ('+', s.to_string())));
    out
}

fn conflicts(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    let v = match sync(json!({ "do": "conflicts" })) {
        Ok(v) => v,
        Err(e) => return fail(&ui, &e),
    };
    let list = v["conflicts"].as_array().cloned().unwrap_or_default();
    let w = words(args);
    let pick = |n: Option<&&str>| -> Result<Value, String> {
        let k: usize = n
            .and_then(|s| s.parse().ok())
            .ok_or("which one: its number from nus sync conflicts")?;
        list.get(k.wrapping_sub(1))
            .cloned()
            .ok_or_else(|| format!("there is no conflict {k}"))
    };
    match w.first().copied() {
        None => {
            if list.is_empty() {
                println!("  No conflicts: sync has kept no other copies.");
                return ExitCode::SUCCESS;
            }
            for (k, c) in list.iter().enumerate() {
                let detail = format!(
                    "{} kept from {} {} {}",
                    ui.g().dot,
                    c["device"].as_str().unwrap_or("?"),
                    ui.g().dot,
                    ago(c["at"].as_u64().unwrap_or(0))
                );
                ui.row(
                    &format!("{:02}", k + 1),
                    c["file"].as_str().unwrap_or("?"),
                    &detail,
                    &ui.warn(),
                    "",
                );
            }
            ui.hint(&format!(
                "{} to compare, {} to settle",
                ui.bold("nus sync conflicts diff <n>"),
                ui.bold("nus sync conflicts keep <n> mine|theirs")
            ));
            ExitCode::SUCCESS
        }
        Some("diff") => {
            let c = match pick(w.get(1)) {
                Ok(c) => c,
                Err(e) => return fail(&ui, &e),
            };
            let d = match sync(json!({ "do": "conflict", "lost": c["lost"] })) {
                Ok(d) => d,
                Err(e) => return fail(&ui, &e),
            };
            println!(
                "  {} {} {} mine (-) {} theirs (+)",
                ui.bold(d["file"].as_str().unwrap_or("")),
                ui.g().dot,
                ui.grey("compared:"),
                ui.g().dot
            );
            for (k, line) in line_diff(
                d["mine"].as_str().unwrap_or(""),
                d["theirs"].as_str().unwrap_or(""),
                4000,
            ) {
                match k {
                    '-' => println!("  {}", ui.red(&format!("- {line}"))),
                    '+' => println!("  {}", ui.bold(&format!("+ {line}"))),
                    _ => println!("  {}", ui.grey(&format!("  {line}"))),
                }
            }
            ExitCode::SUCCESS
        }
        Some("keep") => {
            let c = match pick(w.get(1)) {
                Ok(c) => c,
                Err(e) => return fail(&ui, &e),
            };
            let side = w.get(2).copied().unwrap_or("");
            if side != "mine" && side != "theirs" {
                return fail(&ui, "keep mine or keep theirs");
            }
            match sync(json!({ "do": "resolve", "lost": c["lost"], "keep": side })) {
                Ok(r) => {
                    ui.row(
                        "01",
                        "Settled",
                        &format!(
                            "{} {} kept {side}",
                            r["file"].as_str().unwrap_or(""),
                            ui.g().dot
                        ),
                        &ui.ok(),
                        "",
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&ui, &e),
            }
        }
        Some(other) => fail(
            &ui,
            &format!("conflicts: list, diff <n> or keep <n> mine|theirs, not {other}"),
        ),
    }
}

// --- restore -----------------------------------------------------------------------

fn restore(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    let Some(file) = words(args).first().map(|s| s.to_string()) else {
        return fail(
            &ui,
            "which file: a profile path like rules.luau or themes/dusk.theme",
        );
    };
    let at = opt(args, "--at");
    let start = Instant::now();
    let mut ask_for = json!({ "do": "restore", "file": file });
    if let Some(at) = at {
        ask_for["at"] = json!(at);
    }
    if let Err(e) = sync(ask_for) {
        return fail(&ui, &e);
    }
    match wait_job(
        &ui,
        "01",
        "Restore",
        "reading the git carrier's history",
        Duration::from_secs(300),
    ) {
        Ok(r) if r.get("restored").is_some() => {
            ui.row(
                "01",
                "Restored",
                &format!(
                    "{file} as of {} {} from {}",
                    ago(r["restored"].as_u64().unwrap_or(0)),
                    ui.g().dot,
                    r["device"].as_str().unwrap_or("?")
                ),
                &ui.ok(),
                &took(start),
            );
            ui.hint("what was there is kept beside it as a .lost copy");
            ExitCode::SUCCESS
        }
        Ok(r) => {
            let list = r["versions"].as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                println!("  No versions of {file} on the carrier.");
                return ExitCode::SUCCESS;
            }
            for (k, v) in list.iter().enumerate() {
                ui.row(
                    &format!("{:02}", k + 1),
                    &ago(v["at"].as_u64().unwrap_or(0)),
                    &format!("from {}", v["device"].as_str().unwrap_or("?")),
                    "",
                    "",
                );
            }
            ui.hint(&format!(
                "{} to bring one back",
                ui.bold(&format!("nus sync restore {file} --at 2h"))
            ));
            ExitCode::SUCCESS
        }
        Err(e) => fail(&ui, &e),
    }
}

// --- forge -------------------------------------------------------------------------

fn forge(args: &[String]) -> ExitCode {
    let ui = Ui::new();
    if words(args).first().copied() != Some("github") {
        return fail(&ui, "nus sync forge github [--gh]");
    }
    let start = Instant::now();
    match sync(json!({ "do": "forge", "gh": flag(args, "--gh") })) {
        Ok(_) => {}
        Err(e) if e == "no-client" => return fail(
            &ui,
            "this build has no GitHub app id: sign in with the GitHub CLI's login instead (--gh)",
        ),
        Err(e) => return fail(&ui, &e),
    }
    let mut shown = false;
    let mut i = 0;
    loop {
        let v = match sync(json!({ "do": "forge-status" })) {
            Ok(v) => v,
            Err(e) => return fail(&ui, &e),
        };
        match v["phase"].as_str().unwrap_or("") {
            "code" if !shown => {
                shown = true;
                if ui.tty {
                    print!("\r\x1b[2K");
                }
                println!(
                    "  Open {} and enter",
                    ui.bold(
                        v["uri"]
                            .as_str()
                            .unwrap_or("https://github.com/login/device")
                    )
                );
                println!();
                println!("      {}", ui.bold(v["code"].as_str().unwrap_or("")));
                println!();
            }
            "done" => {
                if ui.tty {
                    print!("\r\x1b[2K");
                }
                ui.row(
                    "01",
                    "GitHub",
                    &format!(
                        "{} {} the git carrier",
                        v["repo"].as_str().unwrap_or(""),
                        ui.g().dot
                    ),
                    &ui.ok(),
                    &took(start),
                );
                return ExitCode::SUCCESS;
            }
            "failed" => return fail(&ui, v["error"].as_str().unwrap_or("the sign-in failed")),
            phase => {
                if ui.tty {
                    print!("\r");
                    let doing = match phase {
                        "verifying" => "signing in",
                        "making" => "finding nus-profile, or making it",
                        "code" => "waiting for the code to be entered",
                        _ => "starting",
                    };
                    ui.row_inline("01", "GitHub", doing, &ui.spinner(i), &took(start));
                }
            }
        }
        if start.elapsed() > Duration::from_secs(900) {
            return fail(&ui, "the sign-in took too long");
        }
        i += 1;
        std::thread::sleep(Duration::from_millis(400));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_show_what_each_side_has() {
        let d = line_diff("a\nb\nc", "a\nB\nc\nd", 100);
        assert_eq!(
            d,
            vec![
                (' ', "a".into()),
                ('-', "b".into()),
                ('+', "B".into()),
                (' ', "c".into()),
                ('+', "d".into())
            ]
        );
        assert_eq!(line_diff(&"x\n".repeat(10), "y", 5).len(), 1);
    }

    #[test]
    fn flag_values_are_not_words() {
        let a: Vec<String> = ["7-orbit-ledger", "--to", "10.0.0.2:5", "--qr"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(words(&a), vec!["7-orbit-ledger"]);
        assert_eq!(opt(&a, "--to"), Some("10.0.0.2:5"));
        assert!(flag(&a, "--qr"));
    }

    #[test]
    fn a_qr_code_renders() {
        let ui = Ui::new();
        let q = qr(&ui, "nus sync pair 7-orbit-ledger --to 192.168.1.20:51807").unwrap();
        assert!(q.lines().count() > 10);
    }
}
