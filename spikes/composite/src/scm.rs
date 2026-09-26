//! Source Control: the repository of what's focused, as a sheet over the
//! window (⌘⇧G, or the crumb's repository chip). Changes grouped as git
//! groups them, the diff of the one you're on, a message and COMMIT, the
//! branch and its upstream, the last few commits. Every action is the
//! real `git` on a thread: hooks, signing, credential helpers and config
//! behave exactly as they do in the shell, and a failure shows git's own
//! words. Nothing here is destructive without a second press.

use std::sync::{Arc, Mutex};

use nus_render::text::Style;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};
use winit::event::{ElementState, MouseButton};
use winit::keyboard::{Key as WKey, NamedKey};

use crate::app::App;
use crate::git_state::{git, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Conflict,
    Staged,
    Changed,
    Untracked,
}

impl Group {
    fn word(self) -> &'static str {
        match self {
            Group::Conflict => "CONFLICTS",
            Group::Staged => "STAGED",
            Group::Changed => "CHANGES",
            Group::Untracked => "NEW FILES",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub path: String,
    /// The letter git uses: M A D R C U ?.
    pub mark: char,
    pub group: Group,
}

#[derive(Clone, Debug, Default)]
pub struct Snap {
    pub state: State,
    pub files: Vec<FileRow>,
    /// `hash\tsubject\tauthor\twhen`, newest first.
    pub log: Vec<[String; 4]>,
    pub branches: Vec<String>,
    pub stashes: usize,
}

/// `git status --porcelain=v1` lines into rows: a file changed both in the
/// index and the tree is in both groups, as git shows it.
pub fn parse_files(out: &str) -> Vec<FileRow> {
    let mut v = Vec::new();
    for line in out.lines() {
        if line.len() < 4 {
            continue;
        }
        let (x, y) = (line.as_bytes()[0] as char, line.as_bytes()[1] as char);
        let path = line[3..].rsplit(" -> ").next().unwrap_or(&line[3..]).trim_matches('"').to_string();
        let conflict = x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D');
        if conflict {
            v.push(FileRow { path, mark: 'U', group: Group::Conflict });
        } else if x == '?' {
            v.push(FileRow { path, mark: '?', group: Group::Untracked });
        } else {
            if x != ' ' {
                v.push(FileRow { path: path.clone(), mark: x, group: Group::Staged });
            }
            if y != ' ' {
                v.push(FileRow { path, mark: y, group: Group::Changed });
            }
        }
    }
    let order = |g: Group| match g {
        Group::Conflict => 0,
        Group::Staged => 1,
        Group::Changed => 2,
        Group::Untracked => 3,
    };
    v.sort_by(|a, b| order(a.group).cmp(&order(b.group)).then(a.path.cmp(&b.path)));
    v
}

fn read_snap(cwd: &str) -> Option<Snap> {
    let state = {
        let out = git(cwd, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"])?;
        let mut s = crate::git_state::parse(&out);
        if let Some(d) = git(cwd, &["rev-parse", "--show-toplevel", "--absolute-git-dir"]) {
            let mut l = d.lines();
            s.root = l.next().unwrap_or_default().trim().to_string();
            s.op = l.next().map(|g| std::path::PathBuf::from(g.trim())).as_deref().and_then(crate::git_state::op_in);
        }
        s
    };
    let files = parse_files(&git(cwd, &["status", "--porcelain=v1", "--untracked-files=all"]).unwrap_or_default());
    let log = git(cwd, &["log", "-8", "--format=%h%x09%s%x09%an%x09%ar"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.splitn(4, '\t').collect();
            (p.len() == 4).then(|| [p[0].to_string(), p[1].to_string(), p[2].to_string(), p[3].to_string()])
        })
        .collect();
    let branches = git(cwd, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .take(40)
        .collect();
    let stashes = git(cwd, &["stash", "list"]).map(|s| s.lines().count()).unwrap_or(0);
    Some(Snap { state, files, log, branches, stashes })
}

/// The diff for one row: staged against HEAD, changed against the index,
/// a new file as all additions.
fn read_diff(cwd: &str, row: &FileRow) -> Vec<String> {
    let out = match row.group {
        Group::Staged => git(cwd, &["diff", "--cached", "--no-color", "--", &row.path]),
        Group::Changed | Group::Conflict => git(cwd, &["diff", "--no-color", "--", &row.path]),
        Group::Untracked => {
            let root = git(cwd, &["rev-parse", "--show-toplevel"]).unwrap_or_default();
            let p = std::path::Path::new(root.trim()).join(&row.path);
            std::fs::read_to_string(p).ok().map(|t| t.lines().take(400).map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n"))
        }
    };
    out.unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("diff --git") && !l.starts_with("index ") && !l.starts_with("--- ") && !l.starts_with("+++ "))
        .take(2000)
        .map(str::to_string)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    Row(usize),
    Toggle(usize),
    Discard(usize),
    StageAll,
    UnstageAll,
    Message,
    Commit,
    CommitPush,
    Push,
    Pull,
    Fetch,
    Branches,
    Switch(usize),
    Stash,
    StashPop,
    UndoCommit,
    OpenFile(usize),
    /// A conflicted file: take our side, theirs, or mark it resolved.
    Ours(usize),
    Theirs(usize),
    Resolved(usize),
    /// The merge, rebase or pick under way: go on, or give up (asks twice).
    Continue,
    Abort,
}

pub struct Scm {
    pub open: bool,
    pub rise: crate::anim::Anim,
    pub cwd: String,
    pub snap: Arc<Mutex<Option<Snap>>>,
    pub diff: Arc<Mutex<(Option<FileRow>, Vec<String>)>>,
    /// The action running, and the last one's outcome (ok, git's words).
    pub busy: Arc<Mutex<Option<String>>>,
    pub note: Arc<Mutex<Option<(bool, String)>>>,
    pub sel: usize,
    pub message: String,
    pub typing: bool,
    pub branches_open: bool,
    /// A destructive hit waiting for its second press.
    pub confirm: Option<Hit>,
    pub scroll: f32,
    pub reach: f32,
    pub diff_rect: Rect,
    pub hits: Vec<(Rect, Hit)>,
    pub rect: Rect,
}

impl Default for Scm {
    fn default() -> Self {
        Scm {
            open: false,
            rise: crate::anim::Anim::at(0.0),
            cwd: String::new(),
            snap: Arc::new(Mutex::new(None)),
            diff: Arc::new(Mutex::new((None, Vec::new()))),
            busy: Arc::new(Mutex::new(None)),
            note: Arc::new(Mutex::new(None)),
            sel: 0,
            message: String::new(),
            typing: false,
            branches_open: false,
            confirm: None,
            scroll: 0.0,
            reach: 0.0,
            diff_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            hits: Vec::new(),
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }
}

impl Scm {
    fn snapshot(&self) -> Option<Snap> {
        self.snap.lock().ok().and_then(|s| s.clone())
    }

    /// Read the repository again, on a thread.
    fn refresh(&self) {
        let (cwd, snap) = (self.cwd.clone(), self.snap.clone());
        std::thread::Builder::new()
            .name("scm".into())
            .spawn(move || {
                let s = read_snap(&cwd);
                if let Ok(mut g) = snap.lock() {
                    *g = s;
                }
                crate::git_state::touch(&cwd);
                crate::browser_runtime::wake();
            })
            .ok();
    }

    /// Load the selected row's diff, when it isn't the one shown.
    fn want_diff(&mut self, row: Option<FileRow>) {
        let have = self.diff.lock().ok().and_then(|d| d.0.clone());
        if have == row {
            return;
        }
        self.scroll = 0.0;
        if let Ok(mut d) = self.diff.lock() {
            *d = (row.clone(), Vec::new());
        }
        let Some(row) = row else { return };
        let (cwd, diff) = (self.cwd.clone(), self.diff.clone());
        std::thread::Builder::new()
            .name("scm-diff".into())
            .spawn(move || {
                let lines = read_diff(&cwd, &row);
                if let Ok(mut d) = diff.lock() {
                    if d.0.as_ref() == Some(&row) {
                        d.1 = lines;
                    }
                }
                crate::browser_runtime::wake();
            })
            .ok();
    }

    /// Run git with `args` (one or more commands in turn) on a thread;
    /// say how it went, then read the repository again.
    fn run(&self, what: &str, cmds: Vec<Vec<String>>) {
        if self.busy.lock().map(|b| b.is_some()).unwrap_or(true) {
            return;
        }
        if let Ok(mut b) = self.busy.lock() {
            *b = Some(what.to_string());
        }
        let (cwd, busy, note, snap, what) = (self.cwd.clone(), self.busy.clone(), self.note.clone(), self.snap.clone(), what.to_string());
        std::thread::Builder::new()
            .name("scm-run".into())
            .spawn(move || {
                let mut result = (true, format!("{what} · done"));
                for args in cmds {
                    let mut c = std::process::Command::new("git");
                    c.args(&args).current_dir(&cwd).env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null());
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        c.creation_flags(0x0800_0000);
                    }
                    match c.output() {
                        Ok(o) if o.status.success() => {}
                        Ok(o) => {
                            let err = String::from_utf8_lossy(&o.stderr);
                            let out = String::from_utf8_lossy(&o.stdout);
                            let words = err.lines().chain(out.lines()).map(str::trim).find(|l| !l.is_empty()).unwrap_or("git said no").to_string();
                            result = (false, words);
                            break;
                        }
                        Err(e) => {
                            result = (false, format!("git didn't start: {e}"));
                            break;
                        }
                    }
                }
                crate::git_gutter::touch_all();
                if let Ok(mut n) = note.lock() {
                    *n = Some(result);
                }
                if let Ok(mut b) = busy.lock() {
                    *b = None;
                }
                let s = read_snap(&cwd);
                if let Ok(mut g) = snap.lock() {
                    *g = s;
                }
                crate::git_state::touch(&cwd);
                crate::browser_runtime::wake();
            })
            .ok();
    }
}

fn a(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

impl App {
    /// Where Source Control looks: the focused shell's folder, or the
    /// folder of the file in the editor.
    fn scm_folder(&self) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        match tab.focused_ref() {
            crate::app::Pane::Term(t) if t.tunnel().is_none() => t.cwd.clone(),
            crate::app::Pane::Editor(e) => Some(e.buffers.get(e.active)?.path.as_ref()?.parent()?.to_string_lossy().into_owned()),
            _ => None,
        }
    }

    pub(crate) fn open_scm(&mut self) {
        let Some(cwd) = self.scm_folder() else {
            self.toast(nus_render::text::icons::WARNING, "No Repository Here", "focus a shell or a file inside one", None);
            return;
        };
        if self.active_git().is_none() && git(&cwd, &["rev-parse", "--git-dir"]).is_none() {
            self.toast(nus_render::text::icons::WARNING, "No Repository Here", "this folder isn't in a git repository", None);
            return;
        }
        if self.scm.cwd != cwd {
            self.scm.message.clear();
            self.scm.sel = 0;
            if let Ok(mut s) = self.scm.snap.lock() {
                *s = None;
            }
            if let Ok(mut d) = self.scm.diff.lock() {
                *d = (None, Vec::new());
            }
        }
        self.scm.cwd = cwd;
        self.scm.open = true;
        self.scm.typing = false;
        self.scm.branches_open = false;
        self.scm.confirm = None;
        if let Ok(mut n) = self.scm.note.lock() {
            *n = None;
        }
        let d = self.motion.dur(crate::anim::base::PALETTE);
        self.scm.rise.replay(0.0, 1.0, d);
        self.scm.refresh();
        self.dirty = true;
    }

    pub(crate) fn close_scm(&mut self) {
        self.scm.open = false;
        self.scm.typing = false;
        self.scm.branches_open = false;
        self.dirty = true;
    }

    fn scm_act(&mut self, hit: Hit) {
        let snap = self.scm.snapshot();
        let files = snap.as_ref().map(|s| s.files.clone()).unwrap_or_default();
        let row = |i: usize| files.get(i).cloned();
        // A destructive action needs its second press.
        let destructive = matches!(hit, Hit::Discard(_) | Hit::UndoCommit | Hit::Abort | Hit::Ours(_) | Hit::Theirs(_));
        if destructive && self.scm.confirm != Some(hit) {
            self.scm.confirm = Some(hit);
            self.dirty = true;
            return;
        }
        self.scm.confirm = None;
        if hit != Hit::Message {
            self.scm.typing = false;
        }
        match hit {
            Hit::Close => self.close_scm(),
            Hit::Row(i) => self.scm.sel = i,
            Hit::Message => self.scm.typing = true,
            Hit::Toggle(i) => {
                if let Some(r) = row(i) {
                    self.scm.sel = i;
                    let cmd = match r.group {
                        Group::Staged => a(&["restore", "--staged", "--", &r.path]),
                        _ => a(&["add", "--", &r.path]),
                    };
                    let what = if r.group == Group::Staged { "unstage" } else { "stage" };
                    self.scm.run(what, vec![cmd]);
                }
            }
            Hit::Discard(i) => {
                if let Some(r) = row(i) {
                    let cmd = match r.group {
                        Group::Untracked => a(&["clean", "-f", "--", &r.path]),
                        Group::Staged => a(&["restore", "--staged", "--worktree", "--source=HEAD", "--", &r.path]),
                        _ => a(&["restore", "--", &r.path]),
                    };
                    self.scm.run("discard", vec![cmd]);
                }
            }
            Hit::StageAll => self.scm.run("stage all", vec![a(&["add", "--all"])]),
            Hit::UnstageAll => self.scm.run("unstage all", vec![a(&["restore", "--staged", "--", "."])]),
            Hit::Commit | Hit::CommitPush => {
                let msg = self.scm.message.trim().to_string();
                if msg.is_empty() {
                    self.scm.typing = true;
                    if let Ok(mut n) = self.scm.note.lock() {
                        *n = Some((false, "write a message first".into()));
                    }
                } else if !files.iter().any(|f| f.group == Group::Staged) {
                    if let Ok(mut n) = self.scm.note.lock() {
                        *n = Some((false, "nothing staged · stage a file, or STAGE ALL".into()));
                    }
                } else {
                    let mut cmds = vec![a(&["commit", "-m", &msg])];
                    if hit == Hit::CommitPush {
                        cmds.push(if snap.as_ref().is_some_and(|s| s.state.upstream.is_some()) { a(&["push"]) } else { a(&["push", "-u", "origin", "HEAD"]) });
                    }
                    self.scm.run(if hit == Hit::Commit { "commit" } else { "commit and push" }, cmds);
                    self.scm.message.clear();
                }
            }
            Hit::Push => {
                let up = snap.as_ref().is_some_and(|s| s.state.upstream.is_some());
                self.scm.run("push", vec![if up { a(&["push"]) } else { a(&["push", "-u", "origin", "HEAD"]) }]);
            }
            Hit::Pull => self.scm.run("pull", vec![a(&["pull", "--ff-only"])]),
            Hit::Fetch => self.scm.run("fetch", vec![a(&["fetch", "--prune"])]),
            Hit::Branches => self.scm.branches_open = !self.scm.branches_open,
            Hit::Switch(i) => {
                self.scm.branches_open = false;
                if let Some(b) = snap.as_ref().and_then(|s| s.branches.get(i).cloned()) {
                    self.scm.run(&format!("switch to {b}"), vec![a(&["switch", &b])]);
                }
            }
            Hit::Stash => self.scm.run("stash", vec![a(&["stash", "push", "--include-untracked"])]),
            Hit::StashPop => self.scm.run("pop the stash", vec![a(&["stash", "pop"])]),
            Hit::UndoCommit => self.scm.run("undo the last commit (changes kept)", vec![a(&["reset", "--soft", "HEAD~1"])]),
            Hit::Ours(i) | Hit::Theirs(i) => {
                if let Some(r) = row(i) {
                    let side = if matches!(hit, Hit::Ours(_)) { "--ours" } else { "--theirs" };
                    self.scm.run(&format!("take {} for {}", &side[2..], r.path), vec![a(&["checkout", side, "--", &r.path]), a(&["add", "--", &r.path])]);
                }
            }
            Hit::Resolved(i) => {
                if let Some(r) = row(i) {
                    self.scm.run(&format!("mark {} resolved", r.path), vec![a(&["add", "--", &r.path])]);
                }
            }
            Hit::Continue | Hit::Abort => {
                let op = snap.as_ref().and_then(|s| s.state.op).unwrap_or("");
                let verb = match op {
                    "REBASING" => "rebase",
                    "CHERRY-PICKING" => "cherry-pick",
                    "REVERTING" => "revert",
                    "MERGING" => "merge",
                    _ => "",
                };
                if !verb.is_empty() {
                    let cmd = match (hit, verb) {
                        (Hit::Abort, v) => a(&[v, "--abort"]),
                        (_, "merge") => a(&["commit", "--no-edit"]),
                        (_, v) => a(&["-c", "core.editor=true", v, "--continue"]),
                    };
                    self.scm.run(&format!("{} the {verb}", if hit == Hit::Abort { "abort" } else { "continue" }), vec![cmd]);
                }
            }
            Hit::OpenFile(i) => {
                if let (Some(r), Some(s)) = (row(i), snap.as_ref()) {
                    let p = std::path::Path::new(&s.state.root).join(&r.path);
                    self.close_scm();
                    self.open_file(&p, false);
                }
            }
        }
        self.dirty = true;
    }

    pub(crate) fn scm_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        if !self.scm.open {
            return false;
        }
        if ev.state != ElementState::Pressed {
            return true;
        }
        let mods = self.mods;
        let primary = if cfg!(target_os = "macos") { mods.super_key() } else { mods.control_key() };
        if self.scm.typing {
            if crate::field::edit(&mut self.scm.message, ev, mods, 500).taken() {
                self.dirty = true;
                return true;
            }
            match &ev.logical_key {
                WKey::Named(NamedKey::Escape) => self.scm.typing = false,
                WKey::Named(NamedKey::Enter) if primary => self.scm_act(if mods.shift_key() { Hit::CommitPush } else { Hit::Commit }),
                _ => {}
            }
            self.dirty = true;
            return true;
        }
        let n = self.scm.snapshot().map(|s| s.files.len()).unwrap_or(0);
        match &ev.logical_key {
            WKey::Named(NamedKey::Escape) => {
                if self.scm.branches_open || self.scm.confirm.is_some() {
                    self.scm.branches_open = false;
                    self.scm.confirm = None;
                } else {
                    self.close_scm();
                }
            }
            WKey::Named(NamedKey::ArrowDown) if n > 0 => self.scm.sel = (self.scm.sel + 1).min(n - 1),
            WKey::Named(NamedKey::ArrowUp) => self.scm.sel = self.scm.sel.saturating_sub(1),
            WKey::Named(NamedKey::Space) if n > 0 => self.scm_act(Hit::Toggle(self.scm.sel)),
            WKey::Named(NamedKey::Enter) if primary => self.scm_act(Hit::Commit),
            WKey::Named(NamedKey::Enter) if n > 0 => self.scm_act(Hit::OpenFile(self.scm.sel)),
            WKey::Character(c) if !primary && (c == "c" || c == "m") => self.scm.typing = true,
            _ => {}
        }
        self.dirty = true;
        true
    }

    pub(crate) fn scm_mouse(&mut self, button: MouseButton, state: ElementState, x: f32, y: f32) -> bool {
        if !self.scm.open {
            return false;
        }
        if button != MouseButton::Left || state != ElementState::Pressed {
            return true;
        }
        if let Some(&(_, hit)) = self.scm.hits.iter().rev().find(|(r, _)| r.contains(x, y)) {
            self.scm_act(hit);
        } else if !self.scm.rect.contains(x, y) {
            self.close_scm();
        } else {
            self.scm.typing = false;
            self.scm.branches_open = false;
            self.scm.confirm = None;
            self.dirty = true;
        }
        true
    }

    pub(crate) fn scm_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !self.scm.open {
            return false;
        }
        if self.scm.diff_rect.contains(x, y) {
            self.scm.scroll = (self.scm.scroll - dy).clamp(0.0, self.scm.reach.max(0.0));
            self.dirty = true;
        }
        true
    }

    /// A word button; returns its width. Primary: filled.
    fn scm_button(&mut self, scene: &mut Scene, x: f32, y: f32, word: &str, primary: bool, hit: Hit) -> f32 {
        let t = self.theme.clone();
        let st = self.label_strong();
        let h = self.px(26.0);
        let w = self.fonts.measure(st, word) + self.px(18.0);
        let r = Rect::new(x, y, w, h);
        let hot = r.contains(self.mouse.0, self.mouse.1);
        if primary {
            scene.rect(r, t.ink);
        } else if hot {
            scene.rect(r, t.tint);
        }
        scene.outline(r, self.px(m::HAIRLINE), t.ink);
        let color = if primary { self.on_fill(t.ink) } else { t.ink };
        self.fonts.draw(scene, Style { color, ..st }, x + self.px(9.0), y + h * 0.5 + st.px * 0.36, word);
        self.scm.hits.push((r, hit));
        w
    }

    pub(crate) fn draw_scm(&mut self, scene: &mut Scene, w: f32, h: f32) {
        if !self.scm.open {
            return;
        }
        self.scm.hits.clear();
        let t = self.theme.clone();
        let ink = t.ink;
        let label = self.label();
        let strong = self.label_strong();
        let dim = Style { color: t.dim, ..label };
        let mono = Style { font: self.f.term, px: self.px(12.5), color: ink, tracking: 0.0 };
        let rise = self.scm.rise.value();
        if self.scm.rise.active() {
            self.dirty = true;
        }
        scene.layer(None);
        scene.rect(Rect::new(0.0, 0.0, w, h), nus_render::theme::Theme::with_alpha(t.scrim, t.scrim[3] * rise));
        let bw = (w * 0.78).max(self.px(720.0)).min(w - 2.0 * self.px(16.0));
        let bh = (h * 0.8).min(h - self.px(60.0));
        let bx = ((w - bw) / 2.0).round();
        let by = ((h - bh) / 2.0 * 0.8).round() + (1.0 - rise) * self.px(10.0);
        let r = Rect::new(bx, by, bw, bh);
        self.scm.rect = r;
        scene.rect(Rect::new(r.x + self.px(8.0), r.y + self.px(8.0), r.w, r.h), ink);
        scene.rect(r, t.paper);
        scene.outline(r, self.px(m::STRUCTURE), ink);
        scene.layer(Some(r));

        let snap = self.scm.snapshot();
        let pad = self.px(16.0);
        let busy = self.scm.busy.lock().ok().and_then(|b| b.clone());
        let note = self.scm.note.lock().ok().and_then(|n| n.clone());
        if busy.is_some() {
            self.dirty = true;
        }

        // Header: the title, the repository, the branch; fetch, pull, push, close.
        let hy = r.y + self.px(14.0);
        let title = Style { px: self.px(16.0), ..strong };
        let mut x = r.x + pad;
        x += self.fonts.draw(scene, title, x, hy + self.px(15.0), "SOURCE CONTROL") + self.px(14.0);
        if let Some(s) = &snap {
            let repo = std::path::Path::new(&s.state.root).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            x += self.fonts.draw(scene, dim, x, hy + self.px(14.0), &repo) + self.px(12.0);
            let word = format!("{} \u{25be}", s.state.branch);
            x += self.scm_button(scene, x, hy, &word, false, Hit::Branches) + self.px(8.0);
            if let Some(up) = &s.state.upstream {
                self.fonts.draw(scene, dim, x, hy + self.px(14.0), &format!("\u{2192} {up}"));
            }
            let mut rx = r.right() - pad;
            let close_w = self.fonts.measure(strong, "CLOSE") + self.px(18.0);
            rx -= close_w;
            self.scm_button(scene, rx, hy, "CLOSE", false, Hit::Close);
            for (word, hit, primary) in [
                (if s.state.ahead > 0 { format!("PUSH \u{2191}{}", s.state.ahead) } else { "PUSH".into() }, Hit::Push, s.state.ahead > 0),
                (if s.state.behind > 0 { format!("PULL \u{2193}{}", s.state.behind) } else { "PULL".into() }, Hit::Pull, s.state.behind > 0),
                ("FETCH".into(), Hit::Fetch, false),
            ] {
                let ww = self.fonts.measure(strong, &word) + self.px(18.0);
                rx -= ww + self.px(8.0);
                self.scm_button(scene, rx, hy, &word, primary, hit);
            }
        }
        let top = hy + self.px(26.0) + self.px(12.0);
        scene.hline(r.x, top, r.w, self.px(m::STRUCTURE), ink);

        let Some(s) = snap else {
            self.fonts.draw(scene, dim, r.x + pad, top + self.px(30.0), "READING THE REPOSITORY\u{2026}");
            self.dirty = true;
            scene.layer(None);
            return;
        };

        // A merge or rebase under way: say so, across the top.
        let mut body_top = top;
        if let Some(op) = s.state.op {
            let band = Rect::new(r.x, top + self.px(2.0), r.w, self.px(28.0));
            scene.rect(band, self.surface.signal);
            let on = self.on_fill(self.surface.signal);
            let words = if s.state.conflicts > 0 { format!("{op} · {} CONFLICT{} · RESOLVE, STAGE, THEN COMMIT", s.state.conflicts, if s.state.conflicts == 1 { "" } else { "S" }) } else { format!("{op} · STAGE, THEN COMMIT TO FINISH") };
            self.fonts.draw(scene, Style { color: on, ..strong }, band.x + pad, band.y + self.px(18.0), &words);
            // CONTINUE and ABORT (twice), in the band's own colours.
            let mut bx3 = band.right() - pad;
            let abort = if self.scm.confirm == Some(Hit::Abort) { "ABORT? AGAIN" } else { "ABORT" };
            for (word, hit) in [(abort, Hit::Abort), ("CONTINUE", Hit::Continue)] {
                let ww = self.fonts.measure(strong, word) + self.px(16.0);
                bx3 -= ww;
                let br = Rect::new(bx3, band.y + self.px(4.0), ww, band.h - self.px(8.0));
                scene.outline(br, self.px(m::HAIRLINE), on);
                self.fonts.draw(scene, Style { color: on, ..strong }, br.x + self.px(8.0), band.y + self.px(18.0), word);
                self.scm.hits.push((br, hit));
                bx3 -= self.px(8.0);
            }
            body_top = band.bottom();
        }

        // Left: the message, COMMIT, then the files by group.
        let lw = (r.w * 0.40).max(self.px(300.0));
        let left = Rect::new(r.x, body_top, lw, r.bottom() - body_top - self.px(40.0));
        scene.vline(left.right(), body_top, left.h, self.px(m::HAIRLINE), ink);
        let mut y = left.y + self.px(14.0);
        self.fonts.draw(scene, dim, left.x + pad, y + self.px(8.0), "MESSAGE");
        y += self.px(16.0);
        let field = Rect::new(left.x + pad, y, left.w - 2.0 * pad, self.px(58.0));
        scene.outline(field, self.px(if self.scm.typing { m::STRUCTURE } else { m::HAIRLINE }), ink);
        self.scm.hits.push((field, Hit::Message));
        let msg_style = Style { font: self.f.ui, ..label };
        let shown = if self.scm.message.is_empty() && !self.scm.typing { "what this commit does (C to write)".to_string() } else { self.scm.message.clone() };
        let lines = crate::reader::wrap(&self.fonts, msg_style, &shown, field.w - self.px(16.0));
        let mut ly = field.y + self.px(18.0);
        for (k, l) in lines.iter().take(3).enumerate() {
            let st = if self.scm.message.is_empty() { dim } else { msg_style };
            let lw2 = self.fonts.draw(scene, st, field.x + self.px(8.0), ly, l);
            if self.scm.typing && k + 1 == lines.len().min(3) && !self.scm.message.is_empty() {
                scene.rect(Rect::new(field.x + self.px(9.0) + lw2, ly - self.px(11.0), self.px(1.5), self.px(14.0)), ink);
            }
            ly += self.px(16.0);
        }
        if self.scm.typing && self.scm.message.is_empty() {
            scene.rect(Rect::new(field.x + self.px(8.0), field.y + self.px(7.0), self.px(1.5), self.px(14.0)), ink);
        }
        y = field.bottom() + self.px(10.0);
        let staged = s.files.iter().filter(|f| f.group == Group::Staged).count();
        let mut bx2 = left.x + pad;
        bx2 += self.scm_button(scene, bx2, y, &format!("COMMIT {staged}"), true, Hit::Commit) + self.px(8.0);
        self.scm_button(scene, bx2, y, "COMMIT & PUSH", false, Hit::CommitPush);
        y += self.px(26.0) + self.px(6.0);
        self.fonts.draw(scene, dim, left.x + pad, y + self.px(10.0), if cfg!(target_os = "macos") { "\u{2318}\u{21a9} COMMIT · \u{21e7}\u{2318}\u{21a9} AND PUSH · HOOKS AND SIGNING AS IN THE SHELL" } else { "CTRL+ENTER COMMIT · CTRL+SHIFT+ENTER AND PUSH" });
        y += self.px(22.0);
        scene.hline(left.x, y, left.w, self.px(m::HAIRLINE), ink);
        y += self.px(6.0);

        let row_h = self.px(24.0);
        let list = Rect::new(left.x, y, left.w, left.bottom() - y);
        scene.layer(Some(list));
        let sel = self.scm.sel.min(s.files.len().saturating_sub(1));
        self.scm.sel = sel;
        let mut group: Option<Group> = None;
        if s.files.is_empty() {
            self.fonts.draw(scene, dim, left.x + pad, y + self.px(18.0), "NOTHING CHANGED · THE TREE MATCHES HEAD");
        }
        for (i, f) in s.files.iter().enumerate() {
            if y > list.bottom() {
                break;
            }
            if group != Some(f.group) {
                group = Some(f.group);
                let n = s.files.iter().filter(|g| g.group == f.group).count();
                self.fonts.draw(scene, dim, left.x + pad, y + self.px(16.0), &format!("{} · {n}", f.group.word()));
                let (word, hit) = if f.group == Group::Staged { ("UNSTAGE ALL", Hit::UnstageAll) } else { ("STAGE ALL", Hit::StageAll) };
                if f.group != Group::Conflict {
                    let ww = self.fonts.measure(label, word);
                    let wr = Rect::new(left.right() - pad - ww, y + self.px(4.0), ww, self.px(16.0));
                    self.fonts.draw(scene, label, wr.x, y + self.px(16.0), word);
                    self.scm.hits.push((wr, hit));
                }
                y += self.px(24.0);
            }
            let rr = Rect::new(left.x, y, left.w, row_h);
            if i == sel {
                scene.rect(rr, t.tint);
                scene.rect(Rect::new(rr.x, rr.y, self.px(2.0), rr.h), self.surface.signal);
            }
            self.scm.hits.push((rr, Hit::Row(i)));
            let mark_color = match f.mark {
                'A' | '?' => crate::theme_edit::from_rgb(t.ansi[2]),
                'D' => crate::theme_edit::from_rgb(t.ansi[1]),
                'U' => self.surface.signal,
                _ => crate::theme_edit::from_rgb(t.ansi[4]),
            };
            let base = y + row_h * 0.5 + self.px(4.0);
            self.fonts.draw(scene, Style { color: mark_color, ..strong }, left.x + pad, base, &f.mark.to_string());
            // Actions at the right: STAGE / UNSTAGE, and DISCARD (twice).
            let mut ax = left.right() - pad;
            let toggle = if f.group == Group::Staged { "UNSTAGE" } else { "STAGE" };
            let discard_armed = self.scm.confirm == Some(Hit::Discard(i));
            let discard = if discard_armed { "DISCARD? AGAIN" } else { "DISCARD" };
            let armed = |h: Hit| self.scm.confirm == Some(h);
            let acts: Vec<(&str, Hit, nus_render::Color)> = if f.group == Group::Conflict {
                vec![
                    ("RESOLVED", Hit::Resolved(i), ink),
                    (if armed(Hit::Theirs(i)) { "THEIRS? AGAIN" } else { "THEIRS" }, Hit::Theirs(i), if armed(Hit::Theirs(i)) { self.surface.signal } else { t.dim }),
                    (if armed(Hit::Ours(i)) { "OURS? AGAIN" } else { "OURS" }, Hit::Ours(i), if armed(Hit::Ours(i)) { self.surface.signal } else { t.dim }),
                ]
            } else {
                vec![(toggle, Hit::Toggle(i), ink), (discard, Hit::Discard(i), if discard_armed { self.surface.signal } else { t.dim })]
            };
            for (word, hit, color) in acts {
                let ww = self.fonts.measure(label, word);
                ax -= ww;
                self.fonts.draw(scene, Style { color, ..label }, ax, base, word);
                self.scm.hits.push((Rect::new(ax - self.px(4.0), y, ww + self.px(8.0), row_h), hit));
                ax -= self.px(12.0);
            }
            let name = self.fit(label, &f.path, ax - (left.x + pad + self.px(18.0)) - self.px(6.0));
            self.fonts.draw(scene, label, left.x + pad + self.px(18.0), base, &name);
            y += row_h;
        }
        scene.layer(Some(r));

        // Right: the selected file's diff, then the history.
        let right = Rect::new(left.right(), body_top, r.right() - left.right(), left.h);
        let hist_h = self.px(24.0) + self.px(18.0) * s.log.len().min(6) as f32 + self.px(10.0);
        let diff_r = Rect::new(right.x, right.y, right.w, right.h - hist_h);
        self.scm.diff_rect = diff_r;
        let row = s.files.get(sel).cloned();
        self.scm.want_diff(row.clone());
        let (_, diff) = self.scm.diff.lock().map(|d| d.clone()).unwrap_or_default();
        if let Some(rw) = &row {
            let head = format!("{} · {}", rw.path, rw.group.word().to_lowercase());
            let head = self.fit(strong, &head, diff_r.w - 2.0 * pad);
            self.fonts.draw(scene, strong, diff_r.x + pad, diff_r.y + self.px(22.0), &head);
            let open_w = self.fonts.measure(label, "OPEN");
            let orx = diff_r.right() - pad - open_w;
            self.fonts.draw(scene, label, orx, diff_r.y + self.px(22.0), "OPEN");
            self.scm.hits.push((Rect::new(orx - self.px(4.0), diff_r.y + self.px(8.0), open_w + self.px(8.0), self.px(20.0)), Hit::OpenFile(sel)));
        }
        let lh = self.px(17.0);
        let body = Rect::new(diff_r.x, diff_r.y + self.px(34.0), diff_r.w, diff_r.h - self.px(34.0));
        self.scm.reach = (diff.len() as f32 * lh - body.h + self.px(10.0)).max(0.0);
        self.scm.scroll = self.scm.scroll.min(self.scm.reach);
        scene.layer(Some(body));
        let add_bg = nus_render::theme::Theme::with_alpha(crate::theme_edit::from_rgb(t.ansi[2]), 0.14);
        let del_bg = nus_render::theme::Theme::with_alpha(crate::theme_edit::from_rgb(t.ansi[1]), 0.14);
        let first = (self.scm.scroll / lh).floor() as usize;
        let mut dy = body.y - (self.scm.scroll - first as f32 * lh);
        for l in diff.iter().skip(first) {
            if dy > body.bottom() {
                break;
            }
            let (bg, color) = if l.starts_with('+') {
                (Some(add_bg), ink)
            } else if l.starts_with('-') {
                (Some(del_bg), ink)
            } else if l.starts_with("@@") {
                (Some(t.tint), t.dim)
            } else {
                (None, ink)
            };
            if let Some(bg) = bg {
                scene.rect(Rect::new(body.x, dy, body.w, lh), bg);
            }
            let text: String = l.chars().take(400).collect::<String>().replace('\t', "    ");
            self.fonts.draw(scene, Style { color, ..mono }, body.x + pad, dy + lh * 0.72, &text);
            dy += lh;
        }
        if row.is_some() && diff.is_empty() {
            self.fonts.draw(scene, dim, body.x + pad, body.y + self.px(14.0), "READING THE DIFF\u{2026}");
        }
        scene.layer(Some(r));
        let hy2 = diff_r.bottom();
        scene.hline(right.x, hy2, right.w, self.px(m::HAIRLINE), ink);
        self.fonts.draw(scene, dim, right.x + pad, hy2 + self.px(18.0), "HISTORY");
        let mut yy = hy2 + self.px(24.0);
        for c in s.log.iter().take(6) {
            let base = yy + self.px(12.0);
            let hw = self.fonts.draw(scene, Style { color: t.dim, ..mono }, right.x + pad, base, &c[0]);
            let tail = format!("{} · {}", c[2], c[3]);
            let tw = self.fonts.measure(dim, &tail);
            self.fonts.draw(scene, dim, right.right() - pad - tw, base, &tail);
            let subj = self.fit(label, &c[1], right.w - 2.0 * pad - hw - tw - self.px(24.0));
            self.fonts.draw(scene, label, right.x + pad + hw + self.px(12.0), base, &subj);
            yy += self.px(18.0);
        }

        // Footer: stash, undo, and what just happened.
        let fy = r.bottom() - self.px(40.0);
        scene.hline(r.x, fy, r.w, self.px(m::STRUCTURE), ink);
        let mut fx = r.x + pad;
        let by2 = fy + self.px(7.0);
        fx += self.scm_button(scene, fx, by2, "STASH", false, Hit::Stash) + self.px(8.0);
        if s.stashes > 0 {
            fx += self.scm_button(scene, fx, by2, &format!("POP STASH ({})", s.stashes), false, Hit::StashPop) + self.px(8.0);
        }
        let undo = if self.scm.confirm == Some(Hit::UndoCommit) { "UNDO LAST COMMIT? AGAIN" } else { "UNDO LAST COMMIT" };
        fx += self.scm_button(scene, fx, by2, undo, false, Hit::UndoCommit) + self.px(16.0);
        let status = match (&busy, &note) {
            (Some(b), _) => (t.dim, format!("{b}\u{2026}")),
            (None, Some((ok, words))) => (if *ok { t.dim } else { self.surface.signal }, words.clone()),
            _ => (t.dim, "\u{2191}\u{2193} MOVE · SPACE STAGES · \u{21a9} OPENS · C WRITES · ESC CLOSES".into()),
        };
        let words = self.fit(label, &status.1, r.right() - pad - fx);
        self.fonts.draw(scene, Style { color: status.0, ..label }, fx, fy + self.px(24.0), &words);

        // The branch list, over everything.
        if self.scm.branches_open {
            let bw2 = self.px(280.0);
            let n = s.branches.len().min(14);
            let list = Rect::new(r.x + pad + self.px(150.0), hy + self.px(30.0), bw2, self.px(26.0) * n as f32 + self.px(12.0));
            scene.rect(Rect::new(list.x + self.px(5.0), list.y + self.px(5.0), list.w, list.h), ink);
            scene.rect(list, t.paper);
            scene.outline(list, self.px(m::STRUCTURE), ink);
            for (k, b) in s.branches.iter().take(n).enumerate() {
                let rr = Rect::new(list.x, list.y + self.px(6.0) + k as f32 * self.px(26.0), list.w, self.px(26.0));
                let current = *b == s.state.branch;
                if rr.contains(self.mouse.0, self.mouse.1) {
                    scene.rect(rr, t.tint);
                }
                let st = if current { strong } else { label };
                let word = if current { format!("{b} · here") } else { b.clone() };
                self.fonts.draw(scene, st, rr.x + self.px(12.0), rr.y + self.px(17.0), &word);
                if !current {
                    self.scm.hits.push((rr, Hit::Switch(k)));
                }
            }
        }
        scene.layer(None);
    }
}

// ── git status, in the shell ───────────────────────────────────────────────

/// What a chip on a `git status` block does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockAct {
    Stage(String),
    Unstage(String),
    StageAll,
    Open,
}

/// Is this command a `git status`, and in the short form?
pub fn status_kind(cmd: &str) -> Option<bool> {
    let w: Vec<&str> = cmd.split_whitespace().collect();
    let i = w.iter().position(|x| *x == "git")?;
    let rest = &w[i + 1..];
    // Global options first: `-C dir` and `-c k=v` take a word each.
    let mut sub = 0;
    while sub < rest.len() && rest[sub].starts_with('-') {
        sub += if matches!(rest[sub], "-C" | "-c") { 2 } else { 1 };
    }
    if sub >= rest.len() {
        return None;
    }
    (rest[sub] == "status").then(|| rest.iter().any(|x| matches!(*x, "-s" | "--short" | "--porcelain" | "-sb" | "-bs") || x.starts_with("--porcelain")))
}

/// The files in a `git status` block's output, by output line: the path
/// and whether it's staged. Long form reads its sections; short form its
/// XY columns (a file with both takes the unstaged side).
pub fn status_lines(text: &str, short: bool) -> Vec<(usize, String, bool)> {
    let mut v = Vec::new();
    if short {
        for (i, l) in text.lines().enumerate() {
            if l.len() < 4 || l.starts_with("##") {
                continue;
            }
            let (x, y) = (l.as_bytes()[0] as char, l.as_bytes()[1] as char);
            if !(" MADRCU?!".contains(x) && " MADRCU?!".contains(y)) || l.as_bytes()[2] != b' ' {
                continue;
            }
            let path = l[3..].rsplit(" -> ").next().unwrap_or(&l[3..]).trim_matches('"').to_string();
            let staged = x != ' ' && x != '?' && y == ' ';
            v.push((i, path, staged));
        }
        return v;
    }
    let mut section: Option<bool> = None;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim();
        if t.starts_with("Changes to be committed") {
            section = Some(true);
        } else if t.starts_with("Changes not staged") || t.starts_with("Untracked files") || t.starts_with("Unmerged paths") {
            section = Some(false);
        } else if t.is_empty() || !l.starts_with(char::is_whitespace) {
            if !t.is_empty() {
                section = None;
            }
        } else if let Some(staged) = section {
            if t.starts_with('(') {
                continue;
            }
            let path = match t.split_once(':') {
                Some((kind, p)) if kind.chars().all(|c| c.is_ascii_lowercase() || c == ' ') => p.trim(),
                _ => t,
            };
            let path = path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"').to_string();
            if !path.is_empty() {
                v.push((i, path, staged));
            }
        }
    }
    v
}

impl App {
    /// A chip on a `git status` block: the real git, in the shell's folder.
    pub(crate) fn status_block_act(&mut self, act: BlockAct, cwd: String) {
        let (args, what): (Vec<String>, String) = match &act {
            BlockAct::Stage(p) => (a(&["add", "--", p]), format!("staged {p}")),
            BlockAct::Unstage(p) => (a(&["restore", "--staged", "--", p]), format!("unstaged {p}")),
            BlockAct::StageAll => (a(&["add", "--all"]), "staged everything".into()),
            BlockAct::Open => return self.open_scm(),
        };
        let mut c = std::process::Command::new("git");
        c.args(&args).current_dir(&cwd).env("GIT_TERMINAL_PROMPT", "0").stdin(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        match c.output() {
            Ok(o) if o.status.success() => {
                crate::git_state::touch(&cwd);
                self.play_event("success");
                self.toast(nus_render::text::icons::CHECK, "Done", format!("{what} · git status again to see it"), None);
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("git said no").to_string();
                self.toast_problem("Git Said No", err, None);
            }
            Err(e) => self.toast_problem("Git Didn't Start", e.to_string(), None),
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real git, on a throwaway repository: status, groups, branch, log.
    #[test]
    fn reads_a_real_repository() {
        if git(".", &["--version"]).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("nus-scm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().to_string();
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git").args(args).current_dir(&dir)
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
                .output().unwrap().status.success();
            assert!(ok, "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("b.txt"), "new\n").unwrap();
        let s = read_snap(&d).expect("a repository");
        assert_eq!(s.state.branch, "main");
        assert_eq!(s.log.len(), 1);
        assert_eq!(s.log[0][1], "first");
        assert!(s.files.iter().any(|f| f.path == "a.txt" && f.group == Group::Changed));
        assert!(s.files.iter().any(|f| f.path == "b.txt" && f.group == Group::Untracked));
        let diff = read_diff(&d, &FileRow { path: "a.txt".into(), mark: 'M', group: Group::Changed });
        assert!(diff.iter().any(|l| l == "+two"), "{diff:?}");
        assert_eq!(crate::git_state::get(&d), None, "first ask starts a read");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A real merge conflict: the row is a conflict, the repository says MERGING.
    #[test]
    fn sees_a_real_conflict() {
        if git(".", &["--version"]).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("nus-scm-conflict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = dir.to_string_lossy().to_string();
        let run = |args: &[&str]| {
            std::process::Command::new("git").args(args).current_dir(&dir)
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
                .output().unwrap().status.success()
        };
        assert!(run(&["init", "-q", "-b", "main"]));
        std::fs::write(dir.join("c.txt"), "base\n").unwrap();
        assert!(run(&["add", "."]) && run(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "base"]));
        assert!(run(&["switch", "-q", "-c", "other"]));
        std::fs::write(dir.join("c.txt"), "theirs\n").unwrap();
        assert!(run(&["-c", "commit.gpgsign=false", "commit", "-q", "-am", "theirs"]));
        assert!(run(&["switch", "-q", "main"]));
        std::fs::write(dir.join("c.txt"), "ours\n").unwrap();
        assert!(run(&["-c", "commit.gpgsign=false", "commit", "-q", "-am", "ours"]));
        assert!(!run(&["merge", "-q", "other"]), "the merge should conflict");
        let s = read_snap(&d).expect("a repository");
        assert_eq!(s.state.op, Some("MERGING"));
        assert_eq!(s.state.conflicts, 1);
        assert!(s.files.iter().any(|f| f.path == "c.txt" && f.group == Group::Conflict));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_blocks_read_both_forms() {
        assert_eq!(status_kind("git status"), Some(false));
        assert_eq!(status_kind("git -C x status -sb"), Some(true));
        assert_eq!(status_kind("git status --porcelain=v1"), Some(true));
        assert_eq!(status_kind("git stash"), None);
        let long = "On branch main\nChanges to be committed:\n  (use \"git restore --staged <file>...\" to unstage)\n        modified:   src/a.rs\n        renamed:    old.rs -> new.rs\n\nChanges not staged for commit:\n        modified:   src/b.rs\n\nUntracked files:\n  (use \"git add <file>...\")\n        notes.md\n\nno changes added\n";
        let v = status_lines(long, false);
        assert_eq!(v, vec![(3, "src/a.rs".into(), true), (4, "new.rs".into(), true), (7, "src/b.rs".into(), false), (11, "notes.md".into(), false)]);
        let short = "## main...origin/main\nM  staged.rs\n M changed.rs\nMM both.rs\n?? new.md\n";
        let v = status_lines(short, true);
        assert_eq!(v, vec![(1, "staged.rs".into(), true), (2, "changed.rs".into(), false), (3, "both.rs".into(), false), (4, "new.md".into(), false)]);
    }

    #[test]
    fn files_group_like_git() {
        let out = " M src/server.ts\nM  src/retry.ts\nMM both.ts\nA  new.ts\nR  old.ts -> renamed.ts\nUU conflict.ts\n?? notes.md\n D gone.ts\n";
        let f = parse_files(out);
        let g = |p: &str, grp: Group| f.iter().find(|r| r.path == p && r.group == grp).map(|r| r.mark);
        assert_eq!(g("src/server.ts", Group::Changed), Some('M'));
        assert_eq!(g("src/retry.ts", Group::Staged), Some('M'));
        assert_eq!(g("both.ts", Group::Staged), Some('M'));
        assert_eq!(g("both.ts", Group::Changed), Some('M'));
        assert_eq!(g("renamed.ts", Group::Staged), Some('R'));
        assert_eq!(g("conflict.ts", Group::Conflict), Some('U'));
        assert_eq!(g("notes.md", Group::Untracked), Some('?'));
        assert_eq!(g("gone.ts", Group::Changed), Some('D'));
        // Conflicts first, then staged, changed, new.
        assert_eq!(f[0].group, Group::Conflict);
        assert_eq!(f.last().unwrap().group, Group::Untracked);
    }
}
