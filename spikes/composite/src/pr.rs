//! The pull request for the branch you're on, and its checks, for the
//! crumb: GitHub only, with the sign-in the profile already has (sync's
//! forge). Asked on a thread, at most once a minute per branch and commit;
//! nothing is asked without a sign-in, and no other token is borrowed.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pr {
    pub number: u64,
    pub url: String,
    pub draft: bool,
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
}

impl Pr {
    /// `PR #412 ✓ 6/6`, `PR #412 ✕ 1 FAILING`, `PR #412 ◌ 2 RUNNING`.
    pub fn word(&self) -> String {
        let total = self.passed + self.failed + self.running;
        let draft = if self.draft { " DRAFT" } else { "" };
        if total == 0 {
            format!("PR #{}{draft}", self.number)
        } else if self.failed > 0 {
            format!("PR #{}{draft} \u{2715} {} FAILING", self.number, self.failed)
        } else if self.running > 0 {
            format!("PR #{}{draft} \u{25cc} {} RUNNING", self.number, self.running)
        } else {
            format!("PR #{}{draft} \u{2713} {}/{}", self.number, self.passed, total)
        }
    }
}

/// `git@github.com:o/r.git`, `https://github.com/o/r(.git)`, `ssh://git@github.com/o/r`
/// → (owner, repo), for github.com only.
pub fn github_repo(url: &str) -> Option<(String, String)> {
    let u = url.trim();
    let rest = u
        .strip_prefix("git@github.com:")
        .or_else(|| u.split_once("github.com/").map(|(_, r)| r))
        .or_else(|| u.split_once("github.com:").map(|(_, r)| r))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut it = rest.split('/');
    let (o, r) = (it.next()?.to_string(), it.next()?.to_string());
    (!o.is_empty() && !r.is_empty()).then_some((o, r))
}

/// Pass, fail and running counts from a check-runs answer.
pub fn tally(v: &serde_json::Value) -> (u32, u32, u32) {
    let (mut p, mut f, mut r) = (0, 0, 0);
    for run in v["check_runs"].as_array().into_iter().flatten() {
        if run["status"].as_str() != Some("completed") {
            r += 1;
            continue;
        }
        match run["conclusion"].as_str() {
            Some("success" | "neutral" | "skipped") => p += 1,
            Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure") => f += 1,
            _ => {}
        }
    }
    (p, f, r)
}

fn look(root: &str, branch: &str) -> Option<Pr> {
    let f = crate::forge::load().filter(|f| f.kind == crate::forge::Kind::GitHub)?;
    let token = crate::forge::token()?;
    let origin = crate::git_state::git(root, &["remote", "get-url", "origin"])?;
    let (owner, repo) = github_repo(&origin)?;
    let sha = crate::git_state::git(root, &["rev-parse", "HEAD"])?.trim().to_string();
    let h = crate::forge::api_headers(f.kind, &token);
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls?state=open&head={owner}:{branch}&per_page=1");
    let (code, body) = crate::forge::http("GET", &url, &h, None).ok()?;
    if code != 200 {
        return None;
    }
    let v = crate::forge::json(&body);
    let p = v.as_array()?.first()?;
    let mut pr = Pr {
        number: p["number"].as_u64()?,
        url: p["html_url"].as_str()?.to_string(),
        draft: p["draft"].as_bool().unwrap_or(false),
        ..Default::default()
    };
    let checks = format!("https://api.github.com/repos/{owner}/{repo}/commits/{sha}/check-runs?per_page=100");
    if let Ok((200, body)) = crate::forge::http("GET", &checks, &h, None) {
        (pr.passed, pr.failed, pr.running) = tally(&crate::forge::json(&body));
    }
    Some(pr)
}

type Cache = HashMap<(String, String), (Instant, Option<Pr>)>;
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The open PR for `branch` in the repository at `root`, as last asked.
pub fn get(root: &str, branch: &str) -> Option<Pr> {
    if root.is_empty() || branch.is_empty() || branch.starts_with("HEAD") {
        return None;
    }
    let key = (root.to_string(), branch.to_string());
    let mut cache = CACHE.lock().ok()?;
    let entry = cache.get(&key);
    // Faster while checks run, so they land in the crumb as they finish.
    let fresh = match entry {
        Some((at, Some(p))) if p.running > 0 => at.elapsed() < Duration::from_secs(20),
        Some((at, _)) => at.elapsed() < Duration::from_secs(60),
        None => false,
    };
    if !fresh && crate::forge::load().is_some_and(|f| f.kind == crate::forge::Kind::GitHub) {
        let prev = entry.and_then(|(_, p)| p.clone());
        cache.insert(key.clone(), (Instant::now(), prev));
        std::thread::Builder::new()
            .name("pr".into())
            .spawn(move || {
                let pr = look(&key.0, &key.1);
                if let Ok(mut c) = CACHE.lock() {
                    c.insert(key, (Instant::now(), pr));
                }
                crate::browser_runtime::wake();
            })
            .ok();
    }
    cache.get(&(root.to_string(), branch.to_string())).and_then(|(_, p)| p.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_and_checks() {
        assert_eq!(github_repo("git@github.com:cbassuarez/nus.git"), Some(("cbassuarez".into(), "nus".into())));
        assert_eq!(github_repo("https://github.com/o/r"), Some(("o".into(), "r".into())));
        assert_eq!(github_repo("ssh://git@github.com/o/r.git"), Some(("o".into(), "r".into())));
        assert_eq!(github_repo("https://gitlab.com/o/r"), None);
        let v: serde_json::Value = serde_json::from_str(r#"{"check_runs":[{"status":"completed","conclusion":"success"},{"status":"completed","conclusion":"failure"},{"status":"in_progress","conclusion":null},{"status":"completed","conclusion":"skipped"}]}"#).unwrap();
        assert_eq!(tally(&v), (2, 1, 1));
        let pr = Pr { number: 412, passed: 6, ..Default::default() };
        assert_eq!(pr.word(), "PR #412 \u{2713} 6/6");
        let pr = Pr { number: 412, passed: 5, failed: 1, ..Default::default() };
        assert_eq!(pr.word(), "PR #412 \u{2715} 1 FAILING");
    }
}
