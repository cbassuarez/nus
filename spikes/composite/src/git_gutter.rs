//! The editor's gutter against HEAD: a bar beside every line added or
//! changed since the last commit, a notch where lines were removed. The
//! file's HEAD text comes from `git show HEAD:./name` on a thread, cached
//! a few seconds (a commit refreshes it); the marks are a line diff
//! (Myers, with the common head and tail trimmed first), redone only when
//! the buffer's revision moves.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Added,
    Changed,
    /// Lines were removed just above this one.
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Equal,
    Insert,
    Delete,
}

fn hash(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.trim_end_matches(['\r', '\n']).hash(&mut h);
    h.finish()
}

/// Myers' shortest edit script; None past `limit` differences.
fn myers(a: &[u64], b: &[u64], limit: usize) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    if max == 0 {
        return Some(Vec::new());
    }
    let off = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    for d in 0..=(max.min(limit) as isize) {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let i = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[i - 1] < v[i + 1]) { v[i + 1] } else { v[i - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x >= n && y >= m {
                found = Some(d);
                break;
            }
            k += 2;
        }
        if found.is_some() {
            break;
        }
    }
    let d_end = found?;
    // Walk back through the trace.
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..=d_end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        let i = (k + off) as usize;
        let prev_k = if k == -d || (k != d && v[i - 1] < v[i + 1]) { k + 1 } else { k - 1 };
        let px = v[(prev_k + off) as usize];
        let py = px - prev_k;
        while x > px && y > py {
            ops.push(Op::Equal);
            x -= 1;
            y -= 1;
        }
        ops.push(if x == px { Op::Insert } else { Op::Delete });
        x = px;
        y = py;
    }
    while x > 0 && y > 0 {
        ops.push(Op::Equal);
        x -= 1;
        y -= 1;
    }
    ops.reverse();
    Some(ops)
}

/// A mark per line of `new` (plus one past the end, for lines removed at
/// the very bottom), comparing against `old`.
pub fn marks(old: &str, new: &str) -> Vec<Option<Mark>> {
    let a: Vec<u64> = old.lines().map(hash).collect();
    let b: Vec<u64> = new.lines().map(hash).collect();
    let mut out = vec![None; b.len() + 1];
    // Trim what's the same at both ends: most edits are small.
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let Some(ops) = myers(am, bm, 4000) else {
        for m in out.iter_mut().skip(pre).take(bm.len()) {
            *m = Some(Mark::Changed);
        }
        return out;
    };
    // Hunks: runs of inserts and deletes between equal lines.
    let mut j = pre; // index in new
    let mut k = 0;
    while k < ops.len() {
        if ops[k] == Op::Equal {
            j += 1;
            k += 1;
            continue;
        }
        let (mut ins, mut del) = (0usize, 0usize);
        while k < ops.len() && ops[k] != Op::Equal {
            match ops[k] {
                Op::Insert => ins += 1,
                Op::Delete => del += 1,
                Op::Equal => {}
            }
            k += 1;
        }
        if ins == 0 {
            out[j] = Some(Mark::Removed);
        } else {
            let mark = if del > 0 { Mark::Changed } else { Mark::Added };
            for m in out.iter_mut().skip(j).take(ins) {
                *m = Some(mark);
            }
        }
        j += ins;
    }
    out
}

struct Entry {
    /// The HEAD text: None not read yet, Some(None) not in a repository,
    /// Some(Some("")) new to the repository.
    head: Option<Option<String>>,
    read_at: Instant,
    reading: bool,
    rev: u64,
    marks: Arc<Vec<Option<Mark>>>,
}

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Entry>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const FRESH: Duration = Duration::from_secs(5);

fn read_head(path: &Path) -> Option<String> {
    let dir = path.parent()?.to_string_lossy().into_owned();
    let name = path.file_name()?.to_string_lossy().into_owned();
    crate::git_state::git(&dir, &["rev-parse", "--git-dir"])?;
    // In the repository: HEAD's copy, or nothing when the file is new to it.
    Some(crate::git_state::git(&dir, &["show", &format!("HEAD:./{name}")]).unwrap_or_default())
}

/// The marks for a buffer at `rev`, or None while HEAD is being read or
/// when the file isn't in a repository.
pub fn for_buffer(path: &Path, rev: u64, text: &ropey::Rope) -> Option<Arc<Vec<Option<Mark>>>> {
    let mut cache = CACHE.lock().ok()?;
    let e = cache.entry(path.to_path_buf()).or_insert(Entry { head: None, read_at: Instant::now() - FRESH * 2, reading: false, rev: u64::MAX, marks: Arc::new(Vec::new()) });
    if !e.reading && e.read_at.elapsed() > FRESH {
        e.reading = true;
        let p = path.to_path_buf();
        std::thread::Builder::new()
            .name("git-gutter".into())
            .spawn(move || {
                let head = read_head(&p);
                if let Ok(mut c) = CACHE.lock() {
                    if let Some(e) = c.get_mut(&p) {
                        if e.head.as_ref() != Some(&head) {
                            e.rev = u64::MAX;
                        }
                        e.head = Some(head);
                        e.read_at = Instant::now();
                        e.reading = false;
                    }
                }
                crate::browser_runtime::wake();
            })
            .ok();
    }
    let head = e.head.as_ref()?.as_ref()?.clone();
    if e.rev != rev {
        e.marks = Arc::new(marks(&head, &text.to_string()));
        e.rev = rev;
    }
    if cache.len() > 64 {
        cache.retain(|_, e| e.read_at.elapsed() < Duration::from_secs(600));
    }
    cache.get(path).map(|e| e.marks.clone())
}

/// Read HEAD again at the next draw: after a save, a commit, a switch.
pub fn touch(path: &Path) {
    if let Ok(mut c) = CACHE.lock() {
        if let Some(e) = c.get_mut(path) {
            e.read_at = Instant::now() - FRESH * 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_changed_removed() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nB\nc\nx\ny\nd\n";
        let m = marks(old, new);
        assert_eq!(m[0], None);
        assert_eq!(m[1], Some(Mark::Changed)); // b → B
        assert_eq!(m[2], None);
        assert_eq!(m[3], Some(Mark::Added)); // x
        assert_eq!(m[4], Some(Mark::Added)); // y
        assert_eq!(m[5], None); // d
        assert_eq!(m[6], Some(Mark::Removed)); // e went, at the end
    }

    #[test]
    fn same_and_new() {
        assert!(marks("a\nb\n", "a\nb\n").iter().all(|m| m.is_none()));
        let m = marks("", "one\ntwo\n");
        assert_eq!(&m[..2], &[Some(Mark::Added), Some(Mark::Added)]);
        let m = marks("a\nb\nc\n", "a\nc\n");
        assert_eq!(m[1], Some(Mark::Removed));
    }

    #[test]
    fn myers_is_minimal_on_a_classic() {
        let a: Vec<u64> = "ABCABBA".chars().map(|c| c as u64).collect();
        let b: Vec<u64> = "CBABAC".chars().map(|c| c as u64).collect();
        let ops = myers(&a, &b, 100).unwrap();
        assert_eq!(ops.iter().filter(|o| **o != Op::Equal).count(), 5);
    }
}
