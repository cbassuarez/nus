//! Git in the sidebar's FILES page: what each file in the tree is to the
//! repository (a letter at the row's end, a dot on folders holding
//! changes), a branch line under the head, and the page's second and
//! third views, CHANGES and HISTORY. Read on a thread per repository,
//! at most every few seconds, never with the index lock.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use nus_render::text::{icons, Style};
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene};

use crate::app::{App, SideHit};
use crate::git_state::{git, State};
use crate::scm::{FileRow, Group};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub when: String,
    /// `HEAD -> main, origin/main, tag: v2`.
    pub refs: String,
}

#[derive(Clone, Debug, Default)]
pub struct Side {
    pub root: PathBuf,
    pub state: State,
    /// Where the asked folder sits in the repository (`git rev-parse
    /// --show-prefix`): tree paths are matched through it, so symlinks and
    /// /private on macOS can't split them from git's.
    pub prefix: PathBuf,
    /// Every changed file by its path in the repository: its letter (M A D R ? U).
    pub marks: HashMap<PathBuf, char>,
    /// Folders (in the repository) holding a change somewhere under them.
    pub dirs: HashSet<PathBuf>,
    pub changes: Vec<FileRow>,
    pub log: Vec<Commit>,
}

impl Side {
    fn key(&self, folder: &Path, path: &Path) -> Option<PathBuf> {
        Some(self.prefix.join(path.strip_prefix(folder).ok()?))
    }

    /// A tree row's letter, the tree rooted at `folder`.
    pub fn mark_for(&self, folder: &Path, path: &Path) -> Option<char> {
        self.marks.get(&self.key(folder, path)?).copied()
    }

    /// Does a folder in the tree hold a change?
    pub fn holds_change(&self, folder: &Path, path: &Path) -> bool {
        self.key(folder, path).is_some_and(|k| self.dirs.contains(&k))
    }
}

/// `%h\t%s\t%an\t%ar\t%D` lines into commits.
pub fn parse_log(out: &str) -> Vec<Commit> {
    out.lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.splitn(5, '\t').collect();
            (p.len() >= 4).then(|| Commit {
                hash: p[0].into(),
                subject: p[1].into(),
                author: p[2].into(),
                when: p[3].into(),
                refs: p.get(4).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        })
        .collect()
}

/// Letters and folders from the changed files, by path in the repository.
pub fn marks_of(files: &[FileRow]) -> (HashMap<PathBuf, char>, HashSet<PathBuf>) {
    let mut marks = HashMap::new();
    let mut dirs = HashSet::new();
    for f in files {
        let abs = PathBuf::from(&f.path);
        // A file both staged and changed shows its working-tree letter;
        // a conflict wins over everything.
        let keep = match marks.get(&abs) {
            Some('U') => false,
            Some(_) => f.group != Group::Staged,
            None => true,
        };
        if keep {
            marks.insert(abs.clone(), f.mark);
        }
        let mut d = abs.parent();
        while let Some(p) = d {
            if p.as_os_str().is_empty() || !dirs.insert(p.to_path_buf()) {
                break;
            }
            d = p.parent();
        }
    }
    (marks, dirs)
}

fn read(folder: &Path) -> Option<Side> {
    let dir = folder.to_string_lossy().into_owned();
    let top = git(&dir, &["rev-parse", "--show-toplevel", "--absolute-git-dir"])?;
    let mut lines = top.lines();
    let root = PathBuf::from(lines.next()?.trim());
    let git_dir = lines.next().map(|l| PathBuf::from(l.trim()));
    let r = root.to_string_lossy().into_owned();
    let mut state = crate::git_state::parse(&git(&r, &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"]).unwrap_or_default());
    state.root = r.clone();
    state.op = git_dir.as_deref().and_then(crate::git_state::op_in);
    let changes = crate::scm::parse_files(&git(&r, &["status", "--porcelain=v1", "--untracked-files=all"]).unwrap_or_default());
    let (marks, dirs) = marks_of(&changes);
    let prefix = PathBuf::from(git(&dir, &["rev-parse", "--show-prefix"]).unwrap_or_default().trim());
    let log = parse_log(&git(&r, &["log", "-40", "--format=%h%x09%s%x09%an%x09%ar%x09%D"]).unwrap_or_default());
    Some(Side { root, state, prefix, marks, dirs, changes, log })
}

type Cache = HashMap<PathBuf, (Instant, Option<Arc<Side>>)>;
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The repository around `folder`, as last read; a read starts when it's old.
pub fn get(folder: &Path) -> Option<Arc<Side>> {
    let mut c = CACHE.lock().ok()?;
    let stale = c.get(folder).is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(3));
    if stale {
        let prev = c.get(folder).and_then(|(_, s)| s.clone());
        c.insert(folder.to_path_buf(), (Instant::now(), prev));
        if c.len() > 16 {
            c.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        }
        let f = folder.to_path_buf();
        std::thread::Builder::new()
            .name("git-side".into())
            .spawn(move || {
                let s = read(&f).map(Arc::new);
                let changed = {
                    let old = CACHE.lock().ok().and_then(|c| c.get(&f).and_then(|(_, s)| s.clone()));
                    old.map(|o| (o.state.clone(), o.changes.len(), o.log.first().cloned())) != s.as_ref().map(|n| (n.state.clone(), n.changes.len(), n.log.first().cloned()))
                };
                if let Ok(mut c) = CACHE.lock() {
                    c.insert(f, (Instant::now(), s));
                }
                if changed {
                    crate::browser_runtime::wake();
                }
            })
            .ok();
    }
    c.get(folder).and_then(|(_, s)| s.clone())
}

/// The colour a letter wears: added green, removed red, conflict signal,
/// changed blue.
pub(crate) fn mark_color(app: &App, mark: char) -> nus_render::Color {
    let t = &app.theme;
    match mark {
        'A' | '?' => crate::theme_edit::from_rgb(t.ansi[2]),
        'D' => crate::theme_edit::from_rgb(t.ansi[1]),
        'U' => app.surface.signal,
        _ => crate::theme_edit::from_rgb(t.ansi[4]),
    }
}

impl App {
    /// The branch line and the FILES · CHANGES · HISTORY switch under the
    /// tree's head. Returns how tall they were (0 outside a repository).
    pub(crate) fn draw_git_side_head(&mut self, scene: &mut Scene, sb: Rect, top: f32, side: &Side) -> f32 {
        let t = self.theme.clone();
        let label = self.label();
        let dim = Style { color: t.dim, ..label };
        let row_h = self.px(m::ROW_H);
        let pad = self.px(m::ROW_PAD_X);
        let isz = self.px(12.0);
        // The branch, where it flows, and what's waiting: a click opens Source Control.
        let line = Rect::new(sb.x, top, sb.w, row_h);
        let hot = line.contains(self.mouse.0, self.mouse.1);
        if hot {
            scene.rect(line, crate::app::fade(t.tint, 0.5));
        }
        let s = &side.state;
        let warn = s.conflicts > 0 || s.op.is_some();
        let bc = if warn { self.surface.signal } else { t.ink };
        let base = top + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
        let gi = if s.op == Some("MERGING") { icons::GIT_MERGE } else { icons::GIT_BRANCH };
        self.fonts.draw_icon(scene, gi, isz, sb.x + pad, top + (row_h - isz) / 2.0, bc);
        let mut x = sb.x + pad + isz + self.px(7.0);
        let head = match s.op {
            Some(op) => format!("{op} \u{b7} {}", s.branch),
            None => s.branch.clone(),
        };
        x += self.fonts.draw(scene, Style { color: bc, ..label }, x, base, &self.fit(label, &head, sb.w * 0.45));
        let mut tail = String::new();
        if let Some(up) = &s.upstream {
            tail.push_str(&format!(" \u{2192} {up}"));
        }
        if s.ahead > 0 {
            tail.push_str(&format!(" \u{2191}{}", s.ahead));
        }
        if s.behind > 0 {
            tail.push_str(&format!(" \u{2193}{}", s.behind));
        }
        let tail = self.fit(dim, &tail, sb.right() - x - pad);
        self.fonts.draw(scene, dim, x, base, &tail);
        self.side_hits.push((line, SideHit::GitScm));
        // The switch.
        let sw = Rect::new(sb.x, top + row_h, sb.w, row_h);
        let n = side.changes.len();
        let views = [
            (icons::FILES, "FILES".to_string()),
            (icons::GIT_DIFF, if n > 0 { format!("CHANGES {n}") } else { "CHANGES".into() }),
            (icons::HISTORY, "HISTORY".to_string()),
        ];
        let cell = sw.w / 3.0;
        let current = self.tree.view as usize;
        for (k, (icon, word)) in views.iter().enumerate() {
            let r = Rect::new(sw.x + cell * k as f32, sw.y, cell, sw.h);
            let on = k == current;
            if on {
                scene.rect(Rect::new(r.x, r.bottom() - self.px(2.0), r.w, self.px(2.0)), self.surface.signal);
            } else if r.contains(self.mouse.0, self.mouse.1) {
                scene.rect(r, crate::app::fade(t.tint, 0.5));
            }
            let col = if on { t.ink } else { t.dim };
            let st = Style { color: col, ..label };
            let word = self.fit(st, word, r.w - isz - self.px(14.0));
            let w = isz + self.px(5.0) + self.fonts.measure(st, &word);
            let x0 = r.x + ((r.w - w) / 2.0).max(self.px(4.0));
            self.fonts.draw_icon(scene, *icon, isz, x0, r.y + (row_h - isz) / 2.0, col);
            self.fonts.draw(scene, st, x0 + isz + self.px(5.0), r.y + (row_h + self.px(m::LABEL_PX)) / 2.0 - self.px(1.0), &word);
            self.side_hits.push((r, SideHit::FilesView(k as u8)));
        }
        scene.hline(sb.x, sw.bottom() - self.px(m::HAIRLINE), sb.w, self.px(m::HAIRLINE), t.tint);
        row_h * 2.0
    }

    /// CHANGES: every changed file, by group; a click opens it beside the shell.
    pub(crate) fn draw_git_changes(&mut self, scene: &mut Scene, list: Rect, side: &Side) {
        let t = self.theme.clone();
        let label = self.label();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        let row_h = self.px(m::ROW_H);
        let pad = self.px(m::ROW_PAD_X);
        let total = side.changes.len() as f32 * row_h;
        self.tree.scroll = self.tree.scroll.clamp(0.0, (total - list.h).max(0.0));
        let mut y = list.y - self.tree.scroll;
        if side.changes.is_empty() {
            self.fonts.draw(scene, dim, list.x + pad, list.y + self.px(22.0), "NOTHING CHANGED SINCE THE LAST COMMIT");
        }
        for (k, f) in side.changes.iter().enumerate() {
            if y > list.bottom() {
                break;
            }
            if y + row_h >= list.y {
                let rr = Rect::new(list.x, y, list.w, row_h);
                if rr.contains(self.mouse.0, self.mouse.1) {
                    scene.rect(rr, crate::app::fade(t.tint, 0.5));
                }
                let base = y + (row_h + self.px(m::UI_PX)) / 2.0 - self.px(2.0);
                let mc = mark_color(self, f.mark);
                let letter = if f.group == Group::Staged { format!("{}\u{2022}", f.mark) } else { f.mark.to_string() };
                self.fonts.draw(scene, Style { color: mc, ..self.label_strong() }, list.x + pad, base, &letter);
                let p = Path::new(&f.path);
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.path.clone());
                let folder = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                let x = list.x + pad + self.px(26.0);
                let nw = self.fonts.draw(scene, ui, x, base, &self.fit(ui, &name, list.right() - x - pad));
                if !folder.is_empty() {
                    let fx = x + nw + self.px(8.0);
                    if fx < list.right() - pad - self.px(30.0) {
                        self.fonts.draw(scene, dim, fx, base, &self.fit(dim, &folder, list.right() - fx - pad));
                    }
                }
                self.side_hits.push((rr, SideHit::ChangeRow(k)));
            }
            y += row_h;
        }
    }

    /// HISTORY: the last commits as a line of nodes; a click shows one in a tab.
    pub(crate) fn draw_git_history(&mut self, scene: &mut Scene, list: Rect, side: &Side) {
        let t = self.theme.clone();
        let label = self.label();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        let row_h = self.px(m::ROW_H) + self.px(14.0);
        let pad = self.px(m::ROW_PAD_X);
        let isz = self.px(13.0);
        let total = side.log.len() as f32 * row_h;
        self.tree.scroll = self.tree.scroll.clamp(0.0, (total - list.h).max(0.0));
        let mut y = list.y - self.tree.scroll;
        let gx = list.x + pad;
        for (k, c) in side.log.iter().enumerate() {
            if y > list.bottom() {
                break;
            }
            if y + row_h >= list.y {
                let rr = Rect::new(list.x, y, list.w, row_h);
                if rr.contains(self.mouse.0, self.mouse.1) {
                    scene.rect(rr, crate::app::fade(t.tint, 0.5));
                }
                // The graph: a rule through the nodes, the newest in ink.
                if k + 1 < side.log.len() {
                    scene.rect(Rect::new(gx + isz * 0.5 - self.px(0.75), y + self.px(8.0) + isz * 0.5, self.px(1.5), row_h), t.dim);
                }
                self.fonts.draw_icon(scene, icons::GIT_COMMIT, isz, gx, y + self.px(8.0), if k == 0 { t.ink } else { t.dim });
                let x = gx + isz + self.px(8.0);
                let b1 = y + self.px(8.0) + self.px(m::UI_PX) - self.px(1.0);
                let subj = self.fit(ui, &c.subject, list.right() - x - pad);
                self.fonts.draw(scene, ui, x, b1, &subj);
                let b2 = b1 + self.px(15.0);
                let mut meta = format!("{} \u{b7} {} \u{b7} {}", c.hash, c.author, c.when);
                // Branches and tags pointing here, shortened.
                let refs: Vec<String> = c.refs.split(", ").filter(|r| !r.is_empty()).map(|r| r.trim_start_matches("HEAD -> ").trim_start_matches("tag: ").to_string()).collect();
                if !refs.is_empty() {
                    meta = format!("{} \u{b7} {meta}", refs.join(" "));
                }
                let st = if refs.is_empty() { dim } else { Style { color: self.surface.signal, ..dim } };
                self.fonts.draw(scene, st, x, b2, &self.fit(dim, &meta, list.right() - x - pad));
                self.side_hits.push((rr, SideHit::CommitRow(k)));
            }
            y += row_h;
        }
        if side.log.is_empty() {
            self.fonts.draw(scene, dim, list.x + pad, list.y + self.px(22.0), "NO COMMITS YET");
        }
    }

    /// A commit from HISTORY: `git show` in a new tab at the repository, so
    /// its diff is a block with chips like any other.
    pub(crate) fn show_commit(&mut self, k: usize) {
        let Some(root) = self.tree.root.clone() else { return };
        let Some(side) = get(&root) else { return };
        let Some(c) = side.log.get(k) else { return };
        let dir = side.root.to_string_lossy().into_owned();
        let profile = self.behavior.default_profile;
        if let Ok(mut t) = self.new_term_pane_at(false, profile, Some(dir)) {
            t.type_at_prompt = Some(format!("git --no-pager show --stat --patch {}\r", c.hash));
            let tab = self.make_tab(crate::app::Pane::Term(t), None);
            self.tabs.push(tab);
            self.activate(self.tabs.len() - 1);
        }
    }

    /// A file from CHANGES: open it beside the shell, like the tree does.
    pub(crate) fn open_change(&mut self, k: usize) {
        let Some(root) = self.tree.root.clone() else { return };
        let Some(side) = get(&root) else { return };
        let Some(f) = side.changes.get(k) else { return };
        let p = side.root.join(&f.path);
        if p.is_file() {
            self.open_from_tree(&p, false);
        } else {
            // Deleted: nothing to open; Source Control can restore it.
            self.open_scm();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real repository, the tree rooted in a subfolder.
    #[test]
    fn reads_from_a_subfolder() {
        if git(".", &["--version"]).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("nus-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/new")).unwrap();
        let run = |args: &[&str]| std::process::Command::new("git").args(args).current_dir(&dir)
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
            .output().unwrap().status.success();
        assert!(run(&["init", "-q", "-b", "main"]));
        std::fs::write(dir.join("docs/a.md"), "a\n").unwrap();
        assert!(run(&["add", "."]) && run(&["-c", "commit.gpgsign=false", "commit", "-q", "-m", "docs"]));
        std::fs::write(dir.join("docs/a.md"), "b\n").unwrap();
        std::fs::write(dir.join("docs/new/x.md"), "x\n").unwrap();
        let docs = dir.join("docs");
        let side = read(&docs).expect("a repository");
        assert_eq!(side.state.branch, "main");
        assert_eq!(side.log.first().map(|c| c.subject.as_str()), Some("docs"));
        assert_eq!(side.mark_for(&docs, &docs.join("a.md")), Some('M'));
        assert_eq!(side.mark_for(&docs, &docs.join("new/x.md")), Some('?'));
        assert!(side.holds_change(&docs, &docs.join("new")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_and_marks() {
        let log = parse_log("4f2c1a9\tqueue: retry\tseb\t14 minutes ago\tHEAD -> main, origin/main\nc1d09e2\ttests\tana\t1 hour ago\t\n");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].refs, "HEAD -> main, origin/main");
        assert_eq!(log[1].author, "ana");
        let files = crate::scm::parse_files("M  src/a.rs\n M src/a.rs\n?? docs/new/x.md\nUU c.rs\n");
        let (marks, dirs) = marks_of(&files);
        assert_eq!(marks.get(Path::new("src/a.rs")), Some(&'M'));
        assert_eq!(marks.get(Path::new("docs/new/x.md")), Some(&'?'));
        assert_eq!(marks.get(Path::new("c.rs")), Some(&'U'));
        assert!(dirs.contains(Path::new("src")) && dirs.contains(Path::new("docs")) && dirs.contains(Path::new("docs/new")));
        assert_eq!(dirs.len(), 3);
        // A tree rooted at /home/me/repo/docs, which git calls docs/.
        let side = Side { prefix: PathBuf::from("docs/"), marks, dirs, ..Default::default() };
        let tree = Path::new("/home/me/repo/docs");
        assert_eq!(side.mark_for(tree, Path::new("/home/me/repo/docs/new/x.md")), Some('?'));
        assert!(side.holds_change(tree, Path::new("/home/me/repo/docs/new")));
        assert!(!side.holds_change(tree, Path::new("/home/me/repo/docs/old")));
    }
}
