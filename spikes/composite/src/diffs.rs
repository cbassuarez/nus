//! A block whose output is a unified diff is a diff, not a stream: each
//! hunk's `@@` line carries chips at the right — for `git diff`, STAGE
//! and REVERT; for `git diff --cached`, UNSTAGE; for any other diff
//! (a patch an agent printed, `cat x.patch`), APPLY — and the chip does
//! it to the file, through `git apply` with a patch of that one hunk.
//! The output is never touched: the next `git diff` says what happened.

use std::path::{Path, PathBuf};

/// One hunk, with enough of its file header to stand alone as a patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// `a/src/main.rs` as the diff names it, and `b/…`.
    pub old: String,
    pub new: String,
    /// The `@@ … @@` line, as written.
    pub header: String,
    /// The hunk's lines, prefixes and all.
    pub lines: Vec<String>,
    /// The line of the output the `@@` sits on, from 0.
    pub at: usize,
    /// Lines the whole file section had before this hunk (`diff --git`,
    /// `index`, `---`, `+++`), for the patch.
    pub head: Vec<String>,
}

impl Hunk {
    /// The patch for this hunk alone.
    pub fn patch(&self) -> String {
        let mut s = String::new();
        for l in &self.head {
            s.push_str(l);
            s.push('\n');
        }
        s.push_str(&self.header);
        s.push('\n');
        for l in &self.lines {
            s.push_str(l);
            s.push('\n');
        }
        s
    }

    /// The file's path, as the diff's `+++` names it, `b/` dropped.
    pub fn file(&self) -> String {
        let n = self.new.trim();
        let n = n.strip_prefix("b/").unwrap_or(n);
        if n == "/dev/null" {
            let o = self.old.trim();
            return o.strip_prefix("a/").unwrap_or(o).to_string();
        }
        n.to_string()
    }

    /// A short word for the chip's tooltip: `+3 −1`.
    pub fn counts(&self) -> String {
        let plus = self.lines.iter().filter(|l| l.starts_with('+')).count();
        let minus = self.lines.iter().filter(|l| l.starts_with('-')).count();
        format!("+{plus} −{minus}")
    }
}

/// Every hunk in a unified diff, with the line each sits on.
pub fn parse(text: &str) -> Vec<Hunk> {
    let mut out = Vec::new();
    let mut head: Vec<String> = Vec::new();
    let (mut old, mut new) = (String::new(), String::new());
    let mut cur: Option<Hunk> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with("diff --git ") || line.starts_with("diff -") {
            if let Some(h) = cur.take() {
                out.push(h);
            }
            head = vec![line.to_string()];
            old.clear();
            new.clear();
            continue;
        }
        if line.starts_with("--- ") && cur.is_none() {
            old = line[4..].to_string();
            head.push(line.to_string());
            continue;
        }
        if line.starts_with("+++ ") && cur.is_none() {
            new = line[4..].to_string();
            head.push(line.to_string());
            continue;
        }
        if line.starts_with("index ") || line.starts_with("new file mode") || line.starts_with("deleted file mode") || line.starts_with("similarity index") || line.starts_with("rename ") || line.starts_with("old mode") || line.starts_with("new mode") {
            if cur.is_none() {
                head.push(line.to_string());
            }
            continue;
        }
        if line.starts_with("@@") {
            if let Some(h) = cur.take() {
                out.push(h);
            }
            if old.is_empty() && new.is_empty() {
                continue; // an @@ with no file: not ours
            }
            // The file header, once per hunk: the same lines each time.
            let hh: Vec<String> = head.iter().filter(|l| l.starts_with("diff ") || l.starts_with("--- ") || l.starts_with("+++ ") || l.starts_with("new file") || l.starts_with("deleted file")).cloned().collect();
            cur = Some(Hunk { old: old.clone(), new: new.clone(), header: line.to_string(), lines: Vec::new(), at: i, head: hh });
            continue;
        }
        if let Some(h) = cur.as_mut() {
            if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') || line.starts_with('\\') || line.is_empty() {
                h.lines.push(line.to_string());
            } else {
                // Something else: the hunk ended (a shell prompt, a blank message).
                out.push(cur.take().unwrap());
            }
        }
    }
    if let Some(h) = cur.take() {
        out.push(h);
    }
    // Trailing empty lines belong to the shell, not the hunk.
    for h in out.iter_mut() {
        while h.lines.last().is_some_and(|l| l.is_empty()) {
            h.lines.pop();
        }
    }
    out.retain(|h| !h.lines.is_empty());
    out
}

/// What the chips do, by the command that made the diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `git diff`: the working tree against the index — STAGE, REVERT.
    Worktree,
    /// `git diff --cached`/`--staged`: the index — UNSTAGE.
    Staged,
    /// Anything else that printed a diff — APPLY.
    Patch,
}

pub fn kind_of(cmd: &str) -> Kind {
    // The segment that runs git, past any cd and the like: `cd x; git diff`.
    let c = cmd.split([';', '|']).flat_map(|s| s.split("&&")).map(str::trim).find(|s| s.starts_with("git ")).unwrap_or(cmd.trim());
    let words: Vec<&str> = c.split_whitespace().filter(|w| !w.starts_with('-')).collect();
    let is_git_diff = words.first() == Some(&"git") && words.get(1) == Some(&"diff");
    if is_git_diff && (c.contains("--cached") || c.contains("--staged")) {
        Kind::Staged
    } else if is_git_diff {
        Kind::Worktree
    } else {
        Kind::Patch
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do {
    Stage,
    Revert,
    Unstage,
    Apply,
}

impl Do {
    /// What the toast says once it is done.
    pub fn done(self) -> &'static str {
        match self {
            Do::Stage => "Staged",
            Do::Revert => "Reverted",
            Do::Unstage => "Unstaged",
            Do::Apply => "Applied",
        }
    }
    /// What takes it back: the index and the working tree each have
    /// their pair, so an undo is the same `git apply`, the other way.
    pub fn undo(self) -> Do {
        match self {
            Do::Stage => Do::Unstage,
            Do::Unstage => Do::Stage,
            Do::Revert => Do::Apply,
            Do::Apply => Do::Revert,
        }
    }
    /// The verb, for "Could Not …".
    pub fn verb(self) -> &'static str {
        match self {
            Do::Stage => "Stage",
            Do::Revert => "Revert",
            Do::Unstage => "Unstage",
            Do::Apply => "Apply",
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Do::Stage => "STAGE",
            Do::Revert => "REVERT",
            Do::Unstage => "UNSTAGE",
            Do::Apply => "APPLY",
        }
    }
    pub fn for_kind(kind: Kind) -> &'static [Do] {
        match kind {
            Kind::Worktree => &[Do::Stage, Do::Revert],
            Kind::Staged => &[Do::Unstage],
            Kind::Patch => &[Do::Apply],
        }
    }
}

/// Run it: the hunk as a patch through `git apply`, in `cwd`.
pub fn run(hunk: &Hunk, what: Do, cwd: &Path) -> Result<String, String> {
    let patch = hunk.patch();
    let tmp: PathBuf = std::env::temp_dir().join(format!("nus-hunk-{}.patch", std::process::id()));
    std::fs::write(&tmp, &patch).map_err(|e| e.to_string())?;
    let mut args: Vec<&str> = vec!["apply", "--whitespace=nowarn"];
    match what {
        Do::Stage => args.push("--cached"),
        Do::Revert => args.push("-R"),
        Do::Unstage => {
            args.push("--cached");
            args.push("-R");
        }
        Do::Apply => {}
    }
    let tmp_s = tmp.to_string_lossy().to_string();
    args.push(&tmp_s);
    let mut c = std::process::Command::new("git");
    c.args(&args).current_dir(cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().map_err(|e| format!("git: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    if out.status.success() {
        Ok(format!("{} · {} · {}", what.word().to_lowercase(), hunk.file(), hunk.counts()))
    } else {
        Err(crate::surface::first_line(String::from_utf8_lossy(&out.stderr).trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1111111..2222222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    old();\n+    new();\n }\n@@ -10,2 +10,3 @@\n x\n+y\n z\ndiff --git a/b.txt b/b.txt\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-a\n+b\n";

    #[test]
    fn hunks_stand_alone_as_patches() {
        let h = parse(DIFF);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].file(), "src/a.rs");
        assert_eq!(h[0].at, 4);
        assert_eq!(h[0].counts(), "+1 −1");
        assert_eq!(h[1].header, "@@ -10,2 +10,3 @@");
        assert_eq!(h[2].file(), "b.txt");
        let p = h[1].patch();
        assert!(p.starts_with("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,2 +10,3 @@\n"));
        assert!(p.ends_with(" z\n"));
    }

    #[test]
    fn the_command_says_what_the_chips_do() {
        assert_eq!(kind_of("git diff"), Kind::Worktree);
        assert_eq!(kind_of("git diff --cached src"), Kind::Staged);
        assert_eq!(kind_of("cat fix.patch"), Kind::Patch);
        assert_eq!(kind_of("cd x; git --no-pager diff -- a.rs"), Kind::Worktree);
        assert_eq!(Do::for_kind(Kind::Worktree), &[Do::Stage, Do::Revert]);
    }

    #[test]
    fn every_chip_has_its_undo() {
        for d in [Do::Stage, Do::Unstage, Do::Revert, Do::Apply] {
            assert_eq!(d.undo().undo(), d);
        }
    }

    /// Stage then undo leaves the index as it was; revert then undo
    /// leaves the file as it was. Real git, in a scratch repository.
    #[test]
    fn undo_takes_a_hunk_back() {
        let git = |dir: &Path, args: &[&str]| std::process::Command::new("git").args(args).current_dir(dir).output();
        let dir = std::env::temp_dir().join(format!("nus-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if git(&dir, &["init", "-q"]).is_err() {
            return; // no git here
        }
        // The machine's git config must not decide the bytes: Windows
        // runners check out with core.autocrlf=true (a\r\n), and a
        // signing setup would stop the commit.
        git(&dir, &["config", "core.autocrlf", "false"]).unwrap();
        git(&dir, &["config", "commit.gpgsign", "false"]).unwrap();
        std::fs::write(dir.join("f.txt"), "a\n").unwrap();
        git(&dir, &["add", "f.txt"]).unwrap();
        git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "a"]).unwrap();
        std::fs::write(dir.join("f.txt"), "b\n").unwrap();
        let diff = String::from_utf8(git(&dir, &["diff"]).unwrap().stdout).unwrap();
        let h = parse(&diff).remove(0);
        let staged = |dir: &Path| !git(dir, &["diff", "--cached", "--quiet"]).unwrap().status.success();

        run(&h, Do::Stage, &dir).unwrap();
        assert!(staged(&dir));
        run(&h, Do::Stage.undo(), &dir).unwrap();
        assert!(!staged(&dir));

        run(&h, Do::Revert, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "a\n");
        run(&h, Do::Revert.undo(), &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "b\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_prompt_after_the_diff_ends_the_hunk() {
        let h = parse("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n\nPS C:\\> ");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].lines, vec!["-a", "+b"]);
    }
}
