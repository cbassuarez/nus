//! Git on the command line: what `git` can take next, as the ghost after
//! the caret, and which words are git's, for the colours. Subcommands and
//! your aliases, then what each takes: branches and tags, remotes, changed
//! files, and its common flags. The repository is read in the background
//! (for-each-ref, remote, status, config) and cached per folder for a few
//! seconds, so drawing never waits on git.
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Most used first: a prefix picks the first match (`pu` is push).
const SUBCOMMANDS: &[&str] = &[
    "status", "add", "commit", "push", "pull", "checkout", "switch", "branch", "log", "diff", "fetch", "merge",
    "rebase", "stash", "reset", "restore", "clone", "init", "remote", "tag", "show", "cherry-pick", "revert",
    "rm", "mv", "blame", "bisect", "clean", "config", "grep", "reflog", "worktree", "submodule", "am", "apply",
    "archive", "describe", "format-patch", "gc", "help", "ls-files", "notes", "range-diff", "shortlog", "sparse-checkout",
];

/// What a subcommand's plain arguments are.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Takes {
    Refs,
    /// A remote, then refs.
    RemoteThenRefs,
    Files,
    Remotes,
    Nothing,
}

fn takes(sub: &str) -> Takes {
    match sub {
        "checkout" | "switch" | "merge" | "rebase" | "branch" | "cherry-pick" | "log" | "show" | "reset" | "revert" | "tag" | "describe" | "range-diff" | "shortlog" | "format-patch" => Takes::Refs,
        "push" | "pull" | "fetch" => Takes::RemoteThenRefs,
        "add" | "restore" | "rm" | "mv" | "diff" | "blame" | "clean" | "grep" | "ls-files" => Takes::Files,
        "remote" => Takes::Remotes,
        _ => Takes::Nothing,
    }
}

fn flags(sub: &str) -> &'static [&'static str] {
    match sub {
        "commit" => &["--amend", "--message", "--all", "--no-edit", "--fixup", "--signoff", "--verbose"],
        "push" => &["--force-with-lease", "--set-upstream", "--tags", "--delete", "--dry-run", "--force"],
        "pull" => &["--rebase", "--ff-only", "--no-rebase", "--autostash"],
        "fetch" => &["--prune", "--all", "--tags", "--depth"],
        "log" => &["--oneline", "--graph", "--all", "--decorate", "--stat", "--patch", "--author", "--since"],
        "diff" => &["--staged", "--cached", "--stat", "--name-only", "--word-diff"],
        "add" => &["--patch", "--all", "--update", "--intent-to-add"],
        "checkout" => &["-b", "--track", "--detach", "--"],
        "switch" => &["--create", "--detach", "--track", "-c"],
        "branch" => &["--delete", "--move", "--all", "--remotes", "--show-current", "--set-upstream-to"],
        "rebase" => &["--interactive", "--continue", "--abort", "--skip", "--onto", "--autosquash"],
        "merge" => &["--no-ff", "--ff-only", "--squash", "--abort", "--continue"],
        "stash" => &["push", "pop", "apply", "list", "drop", "show", "--include-untracked"],
        "reset" => &["--soft", "--mixed", "--hard", "--keep"],
        "restore" => &["--staged", "--source", "--worktree"],
        "status" => &["--short", "--branch", "--porcelain"],
        "clone" => &["--depth", "--branch", "--recurse-submodules", "--filter"],
        "remote" => &["add", "remove", "rename", "set-url", "-v"],
        "worktree" => &["add", "list", "remove", "prune"],
        "tag" => &["--annotate", "--delete", "--list", "--message"],
        _ => &[],
    }
}

/// What nus knows about the repository in one folder.
#[derive(Clone, Default, Debug)]
pub struct Repo {
    pub refs: Vec<String>,
    pub remotes: Vec<String>,
    pub files: Vec<String>,
    pub aliases: Vec<String>,
}

/// Each folder's repository, when it was last read.
type Cache = HashMap<String, (Instant, Option<Repo>)>;
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const FRESH: Duration = Duration::from_secs(4);

fn git(cwd: &str, args: &[&str]) -> Vec<String> {
    let mut c = std::process::Command::new("git");
    c.args(args).current_dir(cwd).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c.output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

/// The repository at `cwd`, as last read; starts a fresh read in the
/// background when that's old. None until the first read lands.
pub fn repo(cwd: &str) -> Option<Repo> {
    let mut cache = CACHE.lock().ok()?;
    let stale = cache.get(cwd).is_none_or(|(at, _)| at.elapsed() > FRESH);
    if stale {
        let prev = cache.get(cwd).and_then(|(_, r)| r.clone());
        // Marked fresh now, so only one read runs at a time.
        cache.insert(cwd.to_string(), (Instant::now(), prev));
        if cache.len() > 64 {
            cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(120));
        }
        let dir = cwd.to_string();
        std::thread::spawn(move || {
            let refs = git(&dir, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads", "refs/remotes", "refs/tags"]);
            let found = !refs.is_empty() || !git(&dir, &["rev-parse", "--git-dir"]).is_empty();
            let repo = found.then(|| Repo {
                refs: refs.into_iter().filter(|r| !r.ends_with("/HEAD")).take(400).collect(),
                remotes: git(&dir, &["remote"]),
                files: git(&dir, &["status", "--porcelain", "--untracked-files=normal"]).into_iter().filter_map(|l| l.get(3..).map(|p| p.rsplit(" -> ").next().unwrap_or(p).trim_matches('"').to_string())).take(400).collect(),
                aliases: git(&dir, &["config", "--get-regexp", r"^alias\."]).into_iter().filter_map(|l| l.strip_prefix("alias.").and_then(|a| a.split_whitespace().next()).map(str::to_string)).collect(),
            });
            if let Ok(mut c) = CACHE.lock() {
                c.insert(dir, (Instant::now(), repo));
            }
            crate::browser_runtime::wake();
        });
    }
    cache.get(cwd).and_then(|(_, r)| r.clone())
}

/// The words of the git command the caret is in, and whether the last
/// word is still being typed (no space after it). None: not in a git command.
fn git_words(line: &str) -> Option<(Vec<String>, bool)> {
    // Only the last command of a pipeline or list.
    let seg = line.rsplit(['|', '&', ';']).next().unwrap_or(line);
    let open = !seg.ends_with(char::is_whitespace);
    let words: Vec<String> = seg.split_whitespace().map(str::to_string).collect();
    if words.first().map(String::as_str) != Some("git") || (words.len() == 1 && open) {
        return None;
    }
    Some((words, open))
}

/// Candidates for the word at the caret (`prefix`), best first.
fn candidates(words: &[String], open: bool, repo: Option<&Repo>) -> (String, Vec<String>) {
    let prefix = if open { words.last().cloned().unwrap_or_default() } else { String::new() };
    // Words before the caret's word, after `git`.
    let before: Vec<&str> = words[1..words.len() - usize::from(open)].iter().map(String::as_str).collect();
    // Global options (`-C dir`, `-c k=v`, `--no-pager`) come before the subcommand.
    let mut i = 0;
    while i < before.len() && before[i].starts_with('-') {
        i += if matches!(before[i], "-C" | "-c") { 2 } else { 1 };
    }
    let Some(sub) = before.get(i).copied() else {
        let mut out: Vec<String> = SUBCOMMANDS.iter().map(|s| s.to_string()).collect();
        if let Some(r) = repo {
            out.extend(r.aliases.iter().cloned());
        }
        return (prefix, out);
    };
    let args: Vec<&str> = before[i + 1..].iter().copied().filter(|a| !a.starts_with('-')).collect();
    if prefix.starts_with('-') {
        return (prefix, flags(sub).iter().filter(|f| f.starts_with('-')).map(|s| s.to_string()).collect());
    }
    let Some(repo) = repo else {
        return (prefix, flags(sub).iter().filter(|f| !f.starts_with('-')).map(|s| s.to_string()).collect());
    };
    let refs = || repo.refs.clone();
    let out = match takes(sub) {
        Takes::Refs => refs(),
        Takes::RemoteThenRefs if args.is_empty() => repo.remotes.clone(),
        Takes::RemoteThenRefs => {
            // After a remote, its branches without the remote's prefix too.
            let remote = args[0];
            let mut v: Vec<String> = repo.refs.iter().filter(|r| !r.contains('/')).cloned().collect();
            v.extend(repo.refs.iter().filter_map(|r| r.strip_prefix(&format!("{remote}/")).map(str::to_string)));
            v
        }
        Takes::Files => repo.files.clone(),
        Takes::Remotes => {
            let mut v: Vec<String> = flags(sub).iter().filter(|f| !f.starts_with('-')).map(|s| s.to_string()).collect();
            v.extend(repo.remotes.iter().cloned());
            v
        }
        Takes::Nothing => flags(sub).iter().filter(|f| !f.starts_with('-')).map(|s| s.to_string()).collect(),
    };
    (prefix, out)
}

/// The rest of the word at the caret, for a line in `cwd`: None when this
/// isn't git, nothing matches, or the word is already whole.
pub fn ghost(line: &str, cwd: Option<&str>) -> Option<String> {
    let (words, open) = git_words(line)?;
    let repo = cwd.and_then(repo);
    let (prefix, list) = candidates(&words, open, repo.as_ref());
    if prefix.is_empty() {
        return None;
    }
    let hit = list.iter().find(|c| c.len() > prefix.len() && c.starts_with(&prefix))?;
    Some(hit[prefix.len()..].to_string())
}

/// Whether the caret is in a git command (so other completers stay out).
pub fn is_git(line: &str) -> bool {
    let seg = line.rsplit(['|', '&', ';']).next().unwrap_or(line);
    seg.split_whitespace().next() == Some("git") && seg.contains(char::is_whitespace)
}

/// What a word of a git line is, for its colour: the subcommand, a ref, a
/// remote, a changed file.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Sub,
    Ref,
    Remote,
    File,
}

/// Spans (char start, len, kind) of git's words in `line`.
pub fn spans(line: &str, cwd: Option<&str>) -> Vec<(usize, usize, Kind)> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let repo = cwd.and_then(|c| CACHE.lock().ok()?.get(c).and_then(|(_, r)| r.clone()));
    let mut i = 0;
    let mut seg_words: Vec<(usize, String)> = Vec::new();
    let flush = |words: &mut Vec<(usize, String)>, out: &mut Vec<(usize, usize, Kind)>| {
        if words.first().map(|w| w.1.as_str()) == Some("git") {
            let mut sub_seen = false;
            for (start, w) in words.iter().skip(1) {
                if w.starts_with('-') {
                    continue;
                }
                let kind = if !sub_seen {
                    sub_seen = true;
                    (SUBCOMMANDS.contains(&w.as_str()) || repo.as_ref().is_some_and(|r| r.aliases.contains(w))).then_some(Kind::Sub)
                } else if let Some(r) = &repo {
                    if r.remotes.contains(w) {
                        Some(Kind::Remote)
                    } else if r.refs.contains(w) {
                        Some(Kind::Ref)
                    } else if r.files.contains(w) {
                        Some(Kind::File)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(k) = kind {
                    out.push((*start, w.chars().count(), k));
                }
            }
        }
        words.clear();
    };
    while i < chars.len() {
        let c = chars[i];
        if "|&;".contains(c) {
            flush(&mut seg_words, &mut out);
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && !"|&;".contains(chars[i]) {
            i += 1;
        }
        seg_words.push((start, chars[start..i].iter().collect()));
    }
    flush(&mut seg_words, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repo {
        Repo { refs: vec!["main".into(), "feature/x".into(), "origin/main".into(), "origin/release".into()], remotes: vec!["origin".into()], files: vec!["src/app.rs".into()], aliases: vec!["co".into()] }
    }

    fn ghost_with(line: &str) -> Option<String> {
        let (w, open) = git_words(line)?;
        let (prefix, list) = candidates(&w, open, Some(&repo()));
        if prefix.is_empty() {
            return None;
        }
        list.iter().find(|c| c.len() > prefix.len() && c.starts_with(&prefix)).map(|c| c[prefix.len()..].to_string())
    }

    #[test]
    fn push_is_never_pushd() {
        assert_eq!(ghost_with("git pu"), Some("sh".into()));
        assert_eq!(ghost_with("git push"), None);
        assert_eq!(ghost_with("git push o"), Some("rigin".into()));
        assert_eq!(ghost_with("git push origin rel"), Some("ease".into()));
    }

    #[test]
    fn refs_files_flags_and_aliases() {
        assert_eq!(ghost_with("git checkout fea"), Some("ture/x".into()));
        assert_eq!(ghost_with("git add src/a"), Some("pp.rs".into()));
        assert_eq!(ghost_with("git commit --am"), Some("end".into()));
        assert_eq!(ghost_with("git c"), Some("ommit".into()));
        assert_eq!(ghost_with("ls | git sta"), Some("tus".into()));
        assert_eq!(ghost_with("gitx pu"), None);
        assert!(is_git("git push "));
        assert!(!is_git("git"));
    }
}
