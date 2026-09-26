//! Where a folder's repository stands: the branch, how far it is from
//! its upstream, what's changed, and whether a merge or rebase is under
//! way. Read with `git status --porcelain=v2 --branch` on a thread, never
//! with the index lock (GIT_OPTIONAL_LOCKS=0), cached per folder for a few
//! seconds. The tab list and the crumb ask every frame; they get the last
//! answer and a fresh read starts when it's old.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The repository's top folder.
    pub root: String,
    /// `main`, or `HEAD 4f2c1a9` when detached.
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    /// Changed in the working tree, not staged.
    pub changed: u32,
    pub untracked: u32,
    pub conflicts: u32,
    /// MERGING, REBASING, CHERRY-PICKING, REVERTING, BISECTING.
    pub op: Option<&'static str>,
}

impl State {
    /// Everything that differs from HEAD, as a count.
    pub fn dirty(&self) -> u32 {
        self.staged + self.changed + self.untracked + self.conflicts
    }

    /// `main ↑2 ↓1 ●3`: the short word for a tab row.
    pub fn short(&self) -> String {
        let mut s = self.branch.clone();
        if self.ahead > 0 {
            s.push_str(&format!(" \u{2191}{}", self.ahead));
        }
        if self.behind > 0 {
            s.push_str(&format!(" \u{2193}{}", self.behind));
        }
        if self.conflicts > 0 {
            s.push_str(&format!(" !{}", self.conflicts));
        } else if self.dirty() > 0 {
            s.push_str(&format!(" \u{25cf}{}", self.dirty()));
        }
        s
    }
}

/// Parse `git status --porcelain=v2 --branch` (without -z).
pub fn parse(out: &str) -> State {
    let mut s = State::default();
    let mut oid = String::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            s.branch = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.oid ") {
            oid = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for w in rest.split_whitespace() {
                if let Some(n) = w.strip_prefix('+') {
                    s.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = w.strip_prefix('-') {
                    s.behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            let xy = line.as_bytes().get(2..4).unwrap_or(b"..");
            if xy[0] != b'.' {
                s.staged += 1;
            }
            if xy[1] != b'.' {
                s.changed += 1;
            }
        } else if line.starts_with("u ") {
            s.conflicts += 1;
        } else if line.starts_with("? ") {
            s.untracked += 1;
        }
    }
    if s.branch == "(detached)" || s.branch.is_empty() {
        s.branch = if oid.is_empty() || oid == "(initial)" { "HEAD".into() } else { format!("HEAD {}", oid.get(..7).unwrap_or(&oid)) };
    }
    s
}

/// What the repository is in the middle of, from its git dir.
pub fn op_in(git_dir: &std::path::Path) -> Option<&'static str> {
    let has = |p: &str| git_dir.join(p).exists();
    if has("rebase-merge") || has("rebase-apply") {
        Some("REBASING")
    } else if has("MERGE_HEAD") {
        Some("MERGING")
    } else if has("CHERRY_PICK_HEAD") {
        Some("CHERRY-PICKING")
    } else if has("REVERT_HEAD") {
        Some("REVERTING")
    } else if has("BISECT_LOG") {
        Some("BISECTING")
    } else {
        None
    }
}

/// git, quietly: no prompts, no lock on the index, no console window.
pub fn git(cwd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new("git");
    c.args(args)
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn read(cwd: &str) -> Option<State> {
    let dirs = git(cwd, &["rev-parse", "--show-toplevel", "--absolute-git-dir"])?;
    let mut lines = dirs.lines();
    let root = lines.next()?.trim().to_string();
    let git_dir = lines.next().map(|l| std::path::PathBuf::from(l.trim()));
    let out = git(cwd, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"])?;
    let mut s = parse(&out);
    s.root = root;
    s.op = git_dir.as_deref().and_then(op_in);
    Some(s)
}

type Cache = HashMap<String, (Instant, Option<State>)>;
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const FRESH: Duration = Duration::from_secs(3);

/// The repository at `cwd`, as last read (None: not a repository, or not
/// read yet); a fresh read starts in the background when it's old.
pub fn get(cwd: &str) -> Option<State> {
    if cwd.is_empty() {
        return None;
    }
    let mut cache = CACHE.lock().ok()?;
    let stale = cache.get(cwd).is_none_or(|(at, _)| at.elapsed() > FRESH);
    if stale {
        let prev = cache.get(cwd).and_then(|(_, s)| s.clone());
        cache.insert(cwd.to_string(), (Instant::now(), prev.clone()));
        if cache.len() > 64 {
            cache.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(120));
        }
        let dir = cwd.to_string();
        std::thread::Builder::new()
            .name("git-state".into())
            .spawn(move || {
                let s = read(&dir);
                let changed = s != prev;
                if let Ok(mut c) = CACHE.lock() {
                    c.insert(dir, (Instant::now(), s));
                }
                if changed {
                    crate::browser_runtime::wake();
                }
            })
            .ok();
    }
    cache.get(cwd).and_then(|(_, s)| s.clone())
}

/// Read again at the next ask: after a command finishes, a commit, a switch.
pub fn touch(cwd: &str) {
    if let Ok(mut c) = CACHE.lock() {
        if let Some((at, _)) = c.get_mut(cwd) {
            *at = Instant::now() - FRESH - Duration::from_millis(1);
        }
    }
}

impl crate::app::TermPane {
    /// This shell's repository, when it's on this machine and in one.
    pub(crate) fn git(&self) -> Option<State> {
        if self.tunnel().is_some() {
            return None;
        }
        get(self.cwd.as_deref()?)
    }
}

impl crate::app::App {
    /// The repository of what's focused: a shell's folder, or the file in the editor.
    pub(crate) fn active_git(&self) -> Option<State> {
        let tab = self.tabs.get(self.active)?;
        match tab.focused_ref() {
            crate::app::Pane::Term(t) => t.git(),
            crate::app::Pane::Editor(e) => {
                let dir = e.buffers.get(e.active)?.path.as_ref()?.parent()?.to_string_lossy().into_owned();
                get(&dir)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_reads() {
        let out = "# branch.oid 4f2c1a9e0000\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n\
                   1 .M N... 100644 100644 100644 a b src/server.ts\n1 M. N... 100644 100644 100644 a b src/retry.ts\n\
                   1 MM N... 100644 100644 100644 a b both.ts\n2 R. N... 100644 100644 100644 a b R100 new.ts\told.ts\n\
                   u UU N... 100644 100644 100644 100644 a b c conflict.ts\n? notes.md\n";
        let s = parse(out);
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!((s.staged, s.changed, s.untracked, s.conflicts), (3, 2, 1, 1));
        assert_eq!(s.short(), "main \u{2191}2 \u{2193}1 !1");
    }

    #[test]
    fn detached_and_clean() {
        let s = parse("# branch.oid 4f2c1a9e0000aaaa\n# branch.head (detached)\n");
        assert_eq!(s.branch, "HEAD 4f2c1a9");
        assert_eq!(s.short(), "HEAD 4f2c1a9");
        let s = parse("# branch.oid abc\n# branch.head feat/x\n");
        assert_eq!(s.short(), "feat/x");
    }
}
