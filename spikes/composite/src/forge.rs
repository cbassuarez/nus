//! The third way for the profile: a private repo on a forge you already
//! have. GitHub (sign in from the card, or paste a token), Forgejo, Gitea
//! or GitLab (a token). nus finds or makes `nus-profile`, private, and the
//! git carrier does the rest — sealed files only, as ever. The token stays
//! in profile/sync/forge.token: never in a URL, never in git's config,
//! never on the carrier; git gets it as a header per command.
//!
//! The web calls go through `curl` on a worker; the card reads the phase
//! each frame.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The repo nus makes, on every forge.
pub const REPO: &str = "nus-profile";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Kind {
    #[default]
    GitHub,
    Forgejo,
    Gitea,
    GitLab,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::GitHub, Kind::Forgejo, Kind::Gitea, Kind::GitLab];

    pub fn name(self) -> &'static str {
        match self {
            Kind::GitHub => "GitHub",
            Kind::Forgejo => "Forgejo",
            Kind::Gitea => "Gitea",
            Kind::GitLab => "GitLab",
        }
    }

    /// Where it usually lives; Forgejo's home is Codeberg.
    pub fn default_host(self) -> &'static str {
        match self {
            Kind::GitHub => "https://github.com",
            Kind::Forgejo => "https://codeberg.org",
            Kind::Gitea => "https://gitea.com",
            Kind::GitLab => "https://gitlab.com",
        }
    }

    /// Where a token is made, for the card's hint.
    pub fn token_hint(self) -> &'static str {
        match self {
            Kind::GitHub => "settings › developer settings › personal access tokens · repo scope",
            Kind::Forgejo | Kind::Gitea => "settings › applications › generate token · repository read and write",
            Kind::GitLab => "preferences › access tokens · api scope",
        }
    }

    /// The username git speaks the token with, over Basic auth.
    pub(crate) fn git_user(self, login: &str) -> String {
        match self {
            Kind::GitHub => "x-access-token".into(),
            Kind::GitLab => "oauth2".into(),
            Kind::Forgejo | Kind::Gitea => login.to_string(),
        }
    }
}

/// What the profile remembers about its forge (the token is a file apart).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forge {
    pub kind: Kind,
    /// Scheme and host, no trailing slash.
    pub host: String,
    pub user: String,
    pub repo: String,
    pub clone_url: String,
}

impl Forge {
    /// `github · seb/nus-profile`
    pub fn word(&self) -> String {
        format!("{} · {}/{}", self.kind.name().to_lowercase(), self.user, self.repo)
    }
}

fn sync_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("sync")
}

fn forge_path() -> PathBuf {
    sync_dir().join("forge.json")
}

fn token_path() -> PathBuf {
    sync_dir().join("forge.token")
}

pub fn load() -> Option<Forge> {
    let s = std::fs::read_to_string(forge_path()).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn token() -> Option<String> {
    crate::protected_state::read_text(&token_path()).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

fn write_private(path: &PathBuf, text: &str) {
    let _ = std::fs::create_dir_all(sync_dir());
    let _ = if *path == token_path() {crate::protected_state::write(path,text.as_bytes())} else {std::fs::write(path,text)};
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn save(f: &Forge, token: &str) {
    if let Ok(s) = serde_json::to_string_pretty(f) {
        write_private(&forge_path(), &s);
    }
    write_private(&token_path(), token);
}

pub fn forget() {
    let _ = std::fs::remove_file(forge_path());
    let _ = std::fs::remove_file(token_path());
}

/// The `Authorization` value the git carrier sends, when a forge is set up.
pub fn auth_header() -> Option<String> {
    let f = load()?;
    let t = token()?;
    Some(format!("Basic {}", base64(format!("{}:{}", f.kind.git_user(&f.user), t).as_bytes())))
}

/// Plain base64, for one header.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ── The web, through curl ────────────────────────────────────────────────

/// One request: the status and the body.
pub(crate) fn http(method: &str, url: &str, headers: &[String], body: Option<&str>) -> Result<(u16, String), String> {
    let mut c = std::process::Command::new("curl");
    c.args(["-sS", "-L", "-X", method, "-w", "\n%{http_code}", "-A", "nus", "--max-time", "30"]);
    for h in headers {
        c.args(["-H", h]);
    }
    if let Some(b) = body {
        c.args(["-H", "Content-Type: application/json", "--data-binary", b]);
    }
    c.arg(url);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = c.output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (body, code) = text.rsplit_once('\n').unwrap_or((&text, "0"));
    Ok((code.trim().parse().unwrap_or(0), body.to_string()))
}

pub(crate) fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

pub(crate) fn api_headers(kind: Kind, token: &str) -> Vec<String> {
    match kind {
        Kind::GitHub => vec![format!("Authorization: Bearer {token}"), "Accept: application/vnd.github+json".into(), "X-GitHub-Api-Version: 2022-11-28".into()],
        Kind::Forgejo | Kind::Gitea => vec![format!("Authorization: token {token}"), "Accept: application/json".into()],
        Kind::GitLab => vec![format!("PRIVATE-TOKEN: {token}"), "Accept: application/json".into()],
    }
}

/// Who the token is: the login.
pub fn whoami(kind: Kind, host: &str, token: &str) -> Result<String, String> {
    let url = match kind {
        Kind::GitHub => "https://api.github.com/user".to_string(),
        Kind::Forgejo | Kind::Gitea => format!("{host}/api/v1/user"),
        Kind::GitLab => format!("{host}/api/v4/user"),
    };
    let (code, body) = http("GET", &url, &api_headers(kind, token), None)?;
    let v = json(&body);
    let login = match kind {
        Kind::GitLab => v["username"].as_str(),
        _ => v["login"].as_str(),
    };
    match (code, login) {
        (200, Some(l)) => Ok(l.to_string()),
        (401 | 403, _) => Err("the token was refused".into()),
        (c, _) => Err(format!("{} said {c}", kind.name())),
    }
}

/// The private repo, found or made: its https clone URL.
pub fn ensure_repo(kind: Kind, host: &str, token: &str, user: &str) -> Result<String, String> {
    let h = api_headers(kind, token);
    let (get_url, post_url, post_body, url_key) = match kind {
        Kind::GitHub => (
            format!("https://api.github.com/repos/{user}/{REPO}"),
            "https://api.github.com/user/repos".to_string(),
            format!(r#"{{"name":"{REPO}","private":true,"description":"nus profile, sealed","auto_init":false}}"#),
            "clone_url",
        ),
        Kind::Forgejo | Kind::Gitea => (
            format!("{host}/api/v1/repos/{user}/{REPO}"),
            format!("{host}/api/v1/user/repos"),
            format!(r#"{{"name":"{REPO}","private":true,"description":"nus profile, sealed","auto_init":false}}"#),
            "clone_url",
        ),
        Kind::GitLab => (
            format!("{host}/api/v4/projects/{user}%2F{REPO}"),
            format!("{host}/api/v4/projects"),
            format!(r#"{{"name":"{REPO}","visibility":"private","description":"nus profile, sealed","initialize_with_readme":false}}"#),
            "http_url_to_repo",
        ),
    };
    let (code, body) = http("GET", &get_url, &h, None)?;
    if code == 200 {
        let v = json(&body);
        let private = match kind {
            Kind::GitLab => v["visibility"].as_str() == Some("private"),
            _ => v["private"].as_bool().unwrap_or(false),
        };
        if !private {
            return Err(format!("{REPO} is there but not private · make it private, or remove it"));
        }
        return v[url_key].as_str().map(str::to_string).ok_or_else(|| "the repo has no clone url".into());
    }
    let (code, body) = http("POST", &post_url, &h, Some(&post_body))?;
    let v = json(&body);
    match code {
        200 | 201 => v[url_key].as_str().map(str::to_string).ok_or_else(|| "made, but no clone url came back".into()),
        401 | 403 => Err("the token may not make repos".into()),
        c => Err(format!("{} would not make {REPO} ({c}): {}", kind.name(), v["message"].as_str().unwrap_or("").chars().take(80).collect::<String>())),
    }
}

// ── GitHub's device flow ────────────────────────────────────────────────

/// nus's GitHub app, for signing in from the card: built in, or the
/// environment's. Without one, a token is the way.
pub fn client_id() -> Option<String> {
    std::env::var("NUS_GITHUB_CLIENT_ID").ok().filter(|s| !s.is_empty()).or_else(|| option_env!("NUS_GITHUB_CLIENT_ID").map(str::to_string)).filter(|s| !s.is_empty())
}

pub struct Device {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub fn device_start(client_id: &str) -> Result<Device, String> {
    let (code, body) = http(
        "POST",
        "https://github.com/login/device/code",
        &["Accept: application/json".into()],
        Some(&format!(r#"{{"client_id":"{client_id}","scope":"repo"}}"#)),
    )?;
    let v = json(&body);
    if code != 200 {
        return Err(format!("github said {code}: {}", v["error_description"].as_str().unwrap_or("")));
    }
    Ok(Device {
        device_code: v["device_code"].as_str().unwrap_or("").into(),
        user_code: v["user_code"].as_str().unwrap_or("").into(),
        verification_uri: v["verification_uri"].as_str().unwrap_or("https://github.com/login/device").into(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

pub enum Poll {
    Pending,
    SlowDown,
    Token(String),
    Denied,
    Expired,
    Failed(String),
}

pub fn device_poll(client_id: &str, device_code: &str) -> Poll {
    let body = format!(r#"{{"client_id":"{client_id}","device_code":"{device_code}","grant_type":"urn:ietf:params:oauth:grant-type:device_code"}}"#);
    match http("POST", "https://github.com/login/oauth/access_token", &["Accept: application/json".into()], Some(&body)) {
        Err(e) => Poll::Failed(e),
        Ok((_, b)) => {
            let v = json(&b);
            if let Some(t) = v["access_token"].as_str() {
                return Poll::Token(t.to_string());
            }
            match v["error"].as_str() {
                Some("authorization_pending") => Poll::Pending,
                Some("slow_down") => Poll::SlowDown,
                Some("access_denied") => Poll::Denied,
                Some("expired_token") => Poll::Expired,
                Some(e) => Poll::Failed(e.to_string()),
                None => Poll::Failed("no answer".into()),
            }
        }
    }
}

// ── Sign-ins already on this machine ──────────────────────────────────────

/// A sign-in found here: the GitHub CLI's, or one git's credential helper
/// keeps (the macOS keychain, Git Credential Manager, libsecret…).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where it came from, as the card says it: "gh", "git's keychain".
    pub source: &'static str,
    pub user: String,
    pub token: String,
}

/// `https://github.com/` → `github.com`.
pub fn host_name(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    let h = h.split_once("://").map(|(_, r)| r).unwrap_or(h);
    h.split('/').next().unwrap_or(h).to_string()
}

/// Run a program with `input` on stdin and nothing that could ask a person
/// anything; its stdout, when it exits 0 within `secs`.
fn quiet(program: &str, args: &[&str], input: &str, secs: u64) -> Option<String> {
    use std::io::Write;
    let mut c = std::process::Command::new(program);
    c.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_ASKPASS", "true")
        .env("SSH_ASKPASS", "true")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = c.spawn().ok()?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(input.as_bytes());
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let out = child.wait_with_output().ok()?;
                return st.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned());
            }
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// `gh auth status` names the account: "Logged in to github.com account seb
/// (keyring)", or in older versions "Logged in to github.com as seb".
fn gh_login(status: &str) -> Option<String> {
    for line in status.lines() {
        for key in [" account ", " as "] {
            if let Some((_, rest)) = line.split_once(key) {
                if line.contains("Logged in") {
                    let w: String = rest.trim().chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
                    if !w.is_empty() {
                        return Some(w);
                    }
                }
            }
        }
    }
    None
}

/// What `git credential fill` answered: `username=` and `password=` lines.
fn parse_credential(out: &str) -> Option<(String, String)> {
    let mut user = String::new();
    let mut pass = String::new();
    for line in out.lines() {
        if let Some(v) = line.strip_prefix("username=") {
            user = v.to_string();
        } else if let Some(v) = line.strip_prefix("password=") {
            pass = v.to_string();
        }
    }
    (!pass.is_empty()).then_some((user, pass))
}

fn from_gh(host: &str) -> Option<Found> {
    let token = quiet("gh", &["auth", "token", "--hostname", host], "", 5)?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    // gh prints its status on stderr in some versions; either is fine.
    let status = std::process::Command::new("gh")
        .args(["auth", "status", "--hostname", host])
        .env("GH_PROMPT_DISABLED", "1")
        .output()
        .ok()
        .map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
        .unwrap_or_default();
    Some(Found { source: "gh", user: gh_login(&status).unwrap_or_default(), token })
}

fn from_git(host: &str) -> Option<Found> {
    let input = format!("protocol=https\nhost={host}\n\n");
    let out = quiet("git", &["-c", "credential.interactive=false", "credential", "fill"], &input, 6)?;
    let (user, token) = parse_credential(&out)?;
    // x-access-token, oauth2, PersonalAccessToken: not a login.
    let user = if matches!(user.as_str(), "x-access-token" | "oauth2" | "PersonalAccessToken" | "token") { String::new() } else { user };
    Some(Found { source: "git's keychain", user, token })
}

/// Every sign-in for `host` this machine already has, each token once.
pub fn found_here(kind: Kind, host: &str) -> Vec<Found> {
    let h = host_name(host);
    let mut v = Vec::new();
    if kind == Kind::GitHub {
        v.extend(from_gh(&h));
    }
    if let Some(g) = from_git(&h) {
        if !v.iter().any(|f: &Found| f.token == g.token) {
            v.push(g);
        }
    }
    // Put a name to the nameless: ask the forge who the token is.
    for f in v.iter_mut().filter(|f| f.user.is_empty()) {
        if let Ok(u) = whoami(kind, host, &f.token) {
            f.user = u;
        }
    }
    v.retain(|f| !f.user.is_empty());
    v
}

/// The look, on a worker; None until it's done.
pub struct Probe {
    pub host: String,
    pub found: Arc<Mutex<Option<Vec<Found>>>>,
}

impl Probe {
    pub fn start(kind: Kind, host: &str) -> Probe {
        let found = Arc::new(Mutex::new(None));
        let (f, h) = (found.clone(), host.to_string());
        std::thread::Builder::new()
            .name("forge-look".into())
            .spawn(move || {
                let v = found_here(kind, &h);
                if let Ok(mut g) = f.lock() {
                    *g = Some(v);
                }
            })
            .ok();
        Probe { host: host.to_string(), found }
    }

    pub fn result(&self) -> Option<Vec<Found>> {
        self.found.lock().ok().and_then(|g| g.clone())
    }
}

// ── git's credential helper ─────────────────────────────────────────────

/// What `nus credential get` hands git for `host` (`github.com`): the
/// signed-in forge's user and token, when that's where git is going.
pub fn credential_for(host: &str) -> Option<(String, String)> {
    let f = load()?;
    if !host_name(&f.host).eq_ignore_ascii_case(host.trim()) {
        return None;
    }
    Some((f.kind.git_user(&f.user), token()?))
}

/// The helper line git runs: the nus command beside the app, pointed at
/// this instance. None when the command isn't there.
pub fn helper_line() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let nus = exe.parent()?.join(if cfg!(windows) { "nus.exe" } else { "nus" });
    if !nus.is_file() {
        return None;
    }
    let inst = std::env::current_dir().ok()?.join("profile").join("instance");
    let q = |p: &std::path::Path| p.to_string_lossy().replace('\\', "/").replace('\'', "'\\''");
    Some(format!("!NUS_INSTANCE='{}' '{}' credential", q(&inst), q(&nus)))
}

fn helper_key(host: &str) -> String {
    format!("credential.{}.helper", host.trim_end_matches('/'))
}

/// Let git in every shell use this sign-in (on), or stop (off): nus's
/// line added to, or taken out of, the global config for the forge's host.
pub fn set_git_uses(host: &str, on: bool) -> Result<(), String> {
    let key = helper_key(host);
    // Out first, so turning it on twice doesn't add it twice.
    let _ = quiet("git", &["config", "--global", "--unset-all", &key, "nus.*credential"], "", 5);
    if !on {
        return Ok(());
    }
    let line = helper_line().ok_or("the nus command isn't beside the app")?;
    quiet("git", &["config", "--global", "--add", &key, &line], "", 5).map(|_| ()).ok_or_else(|| "git config didn't take it".to_string())
}

// ── The flow, on a worker ─────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Asking GitHub for a code.
    Starting,
    /// Enter this code on that page; nus is waiting.
    Code { user_code: String, uri: String },
    /// The token is in; who is it, is the repo there.
    Verifying,
    Making,
    Done(Forge),
    Failed(String),
}

pub struct Flow {
    pub phase: Arc<Mutex<Phase>>,
    pub cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl Flow {
    pub fn phase(&self) -> Phase {
        self.phase.lock().map(|p| p.clone()).unwrap_or(Phase::Failed("lost".into()))
    }
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn set(phase: &Arc<Mutex<Phase>>, p: Phase) {
    if let Ok(mut g) = phase.lock() {
        *g = p;
    }
}

/// From a token: who, the repo, saved.
fn finish(phase: &Arc<Mutex<Phase>>, kind: Kind, host: &str, token: &str) {
    set(phase, Phase::Verifying);
    let user = match whoami(kind, host, token) {
        Ok(u) => u,
        Err(e) => return set(phase, Phase::Failed(e)),
    };
    set(phase, Phase::Making);
    match ensure_repo(kind, host, token, &user) {
        Ok(clone_url) => {
            let f = Forge { kind, host: host.to_string(), user, repo: REPO.into(), clone_url };
            save(&f, token);
            set(phase, Phase::Done(f));
        }
        Err(e) => set(phase, Phase::Failed(e)),
    }
}

/// A token you pasted.
pub fn start_token(kind: Kind, host: &str, token: &str) -> Flow {
    let phase = Arc::new(Mutex::new(Phase::Verifying));
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (p, host, token) = (phase.clone(), host.trim_end_matches('/').to_string(), token.trim().to_string());
    std::thread::Builder::new().name("forge".into()).spawn(move || finish(&p, kind, &host, &token)).ok();
    Flow { phase, cancel }
}

/// GitHub, signed into from the card: a code to enter on github.com.
pub fn start_device(client_id: &str) -> Flow {
    let phase = Arc::new(Mutex::new(Phase::Starting));
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (p, c, id) = (phase.clone(), cancel.clone(), client_id.to_string());
    std::thread::Builder::new()
        .name("forge".into())
        .spawn(move || {
            let d = match device_start(&id) {
                Ok(d) => d,
                Err(e) => return set(&p, Phase::Failed(e)),
            };
            set(&p, Phase::Code { user_code: d.user_code.clone(), uri: d.verification_uri.clone() });
            let mut wait = d.interval.max(5);
            let deadline = std::time::Instant::now() + Duration::from_secs(d.expires_in);
            loop {
                if c.load(std::sync::atomic::Ordering::Relaxed) {
                    return set(&p, Phase::Failed("cancelled".into()));
                }
                if std::time::Instant::now() > deadline {
                    return set(&p, Phase::Failed("the code expired · try again".into()));
                }
                std::thread::sleep(Duration::from_secs(wait));
                match device_poll(&id, &d.device_code) {
                    Poll::Pending => {}
                    Poll::SlowDown => wait += 5,
                    Poll::Token(t) => return finish(&p, Kind::GitHub, Kind::GitHub.default_host(), &t),
                    Poll::Denied => return set(&p, Phase::Failed("you said no on github".into())),
                    Poll::Expired => return set(&p, Phase::Failed("the code expired · try again".into())),
                    Poll::Failed(e) => return set(&p, Phase::Failed(e)),
                }
            }
        })
        .ok();
    Flow { phase, cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_the_usual() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"x-access-token:ghp_abc"), "eC1hY2Nlc3MtdG9rZW46Z2hwX2FiYw==");
    }

    #[test]
    fn found_signins_parse() {
        assert_eq!(host_name("https://github.com/"), "github.com");
        assert_eq!(host_name("code.example.dev/x"), "code.example.dev");
        assert_eq!(gh_login("github.com\n  ✓ Logged in to github.com account seb (keyring)\n").as_deref(), Some("seb"));
        assert_eq!(gh_login("✓ Logged in to github.com as ana-b (oauth_token)").as_deref(), Some("ana-b"));
        assert_eq!(gh_login("You are not logged into any GitHub hosts."), None);
        assert_eq!(parse_credential("protocol=https\nhost=github.com\nusername=seb\npassword=gho_x\n"), Some(("seb".into(), "gho_x".into())));
        assert_eq!(parse_credential("protocol=https\nhost=github.com\n"), None);
    }

    #[test]
    fn hosts_and_words() {
        assert_eq!(Kind::Forgejo.default_host(), "https://codeberg.org");
        let f = Forge { kind: Kind::GitHub, host: "https://github.com".into(), user: "seb".into(), repo: REPO.into(), clone_url: "https://github.com/seb/nus-profile.git".into() };
        assert_eq!(f.word(), "github · seb/nus-profile");
        assert_eq!(Kind::GitLab.git_user("seb"), "oauth2");
        assert_eq!(Kind::Gitea.git_user("seb"), "seb");
    }
}
