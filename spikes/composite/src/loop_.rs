//! The loop: click-to-source on localhost pages.
//!
//! Alt+Shift+click an element on a page a dev server serves and the editor
//! pane opens at its source. The element is asked, in order: a framework's
//! debug marker on the instance (React's `_debugSource`, Vue's `__file`,
//! Svelte's `__svelte_meta`), then the served file under the project — the
//! folder of the shell that started the server (the ports board knows), or
//! the focused shell's. LOCALHOST · CLICK TO SOURCE. The way back for
//! console → shell.

use serde_json::Value;

use crate::app::{App, Pane};

/// The probe run in the page at a point: `{file, line, col, how}` or a
/// `{pathname}` for the served-file fallback.
pub const PROBE_JS: &str = r#"(function(x, y){
  const el = document.elementFromPoint(x, y);
  if (!el) return null;
  // React: the fiber's nearest _debugSource.
  for (let n = el; n; n = n.parentElement) {
    const k = Object.keys(n).find(k => k.startsWith('__reactFiber$'));
    if (k) {
      let f = n[k];
      for (let i = 0; f && i < 64; i++, f = f.return) {
        const s = f._debugSource;
        if (s && s.fileName) return { file: s.fileName, line: s.lineNumber || 1, col: s.columnNumber || 1, how: 'react' };
      }
      break;
    }
  }
  // Svelte (dev): the element's loc.
  for (let n = el; n; n = n.parentElement) {
    const m = n.__svelte_meta;
    if (m && m.loc && m.loc.file) return { file: m.loc.file, line: (m.loc.line || 0) + 1, col: (m.loc.column || 0) + 1, how: 'svelte' };
  }
  // Vue (dev): the component's file.
  for (let n = el; n; n = n.parentElement) {
    const c = n.__vueParentComponent || n.__vue__;
    const file = c && ((c.type && c.type.__file) || (c.$options && c.$options.__file));
    if (file) return { file, line: 1, col: 1, how: 'vue' };
  }
  // Vite's inspector attribute, when a plugin left one.
  for (let n = el; n; n = n.parentElement) {
    const a = n.getAttribute && (n.getAttribute('data-inspector-relative-path') || n.getAttribute('data-source') || n.getAttribute('data-loc'));
    if (a) { const m = a.match(/^(.*?):(\d+)(?::(\d+))?$/); if (m) return { file: m[1], line: +m[2], col: +(m[3] || 1), how: 'attr' }; return { file: a, line: 1, col: 1, how: 'attr' }; }
  }
  return { pathname: location.pathname, tag: el.tagName.toLowerCase(), id: el.id || '', how: 'served' };
})"#;

/// A served path under a folder: `/docs/architecture/` → `docs/architecture/index.html`.
pub fn served_file(root: &std::path::Path, pathname: &str) -> Option<std::path::PathBuf> {
    let rel = pathname.trim_start_matches('/');
    let cands = [rel.to_string(), format!("{}/index.html", rel.trim_end_matches('/')), format!("{}.html", rel.trim_end_matches('/'))];
    for c in cands {
        if c.is_empty() {
            continue;
        }
        let p = root.join(c.replace('/', std::path::MAIN_SEPARATOR_STR));
        if p.is_file() {
            return Some(p);
        }
    }
    if rel.is_empty() || rel == "/" {
        let p = root.join("index.html");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

impl App {
    /// Alt+Shift+click on a local page: ask the element, then open its source.
    /// Returns true when the click was taken.
    pub(crate) fn loop_click(&mut self, tab: usize, right: bool, lx: f32, ly: f32) -> bool {
        if !self.behavior.click_to_source {
            return false;
        }
        let Some(w) = self.web_pane_ref(tab, right) else { return false };
        let url = w.tab.shared.borrow().url.clone();
        if !crate::app::is_local_url(&url) {
            return false;
        }
        let id = w.tab.eval_reply(&format!("({PROBE_JS})({lx}, {ly})"));
        self.loop_probe = Some((tab, right, id, crate::clock::now()));
        self.notice(nus_render::text::icons::CURSOR, "Finding The Source…", "");
        true
    }

    /// The probe answered: resolve and open.
    pub(crate) fn poll_loop(&mut self) {
        let Some((tab, right, id, since)) = self.loop_probe else { return };
        let reply = self.web_pane_ref(tab, right).and_then(|w| w.tab.take_reply(id));
        let Some(v) = reply else {
            if crate::clock::since(since).as_secs() > 5 {
                self.loop_probe = None;
                self.notice(nus_render::text::icons::CURSOR, "No Answer", "the page did not answer");
            }
            return;
        };
        self.loop_probe = None;
        let v = v.pointer("/result/value").cloned().unwrap_or(Value::Null);
        if v.is_null() {
            self.notice(nus_render::text::icons::CURSOR, "Nothing Under The Pointer", "");
            return;
        }
        let root = self.loop_root(tab);
        let (path, line) = if let Some(file) = v.get("file").and_then(Value::as_str) {
            let line = v.get("line").and_then(Value::as_u64).unwrap_or(1) as usize;
            let p = std::path::Path::new(file);
            let p = if p.is_absolute() { p.to_path_buf() } else { root.clone().map(|r| r.join(file)).unwrap_or_else(|| p.to_path_buf()) };
            (p, line)
        } else {
            let pathname = v.get("pathname").and_then(Value::as_str).unwrap_or("/");
            let Some(root) = root else {
                self.notice(nus_render::text::icons::FOLDER, "No Project Folder", "start the server from a shell here");
                return;
            };
            match served_file(&root, pathname) {
                Some(p) => (p, 1),
                None => {
                    self.notice(nus_render::text::icons::FOLDER, "No Source File", format!("under {} for {pathname}", root.display()));
                    return;
                }
            }
        };
        if !path.is_file() {
            self.notice(nus_render::text::icons::FOLDER, "Not Here", path.display().to_string());
            return;
        }
        let how = v.get("how").and_then(Value::as_str).unwrap_or("").to_string();
        self.open_file(&path, true);
        self.layout();
        if line > 1 {
            if let Some(e) = self.focused_editor() {
                if let Some(b) = e.buf_mut() {
                    b.cursor = b.at(line.saturating_sub(1), 0);
                    b.anchor = None;
                    b.scroll = line.saturating_sub(8);
                }
            }
        }
        self.notice(nus_render::text::icons::CODE, "Found", format!("{} · line {line} · {how}", path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()));
        self.dirty = true;
    }

    /// The folder the page's server was started from: the ports board's
    /// owner for its port, else the shell beside it, else the focused cwd.
    fn loop_root(&self, tab: usize) -> Option<std::path::PathBuf> {
        let url = self.web_pane_ref(tab, false).or_else(|| self.web_pane_ref(tab, true)).map(|w| w.tab.shared.borrow().url.clone())?;
        let port: Option<u16> = url.split("//").nth(1).and_then(|h| h.split('/').next()).and_then(|h| h.rsplit(':').next()).and_then(|p| p.parse().ok());
        if let Some(port) = port {
            if let Some(cwd) = self.board.rows.iter().find(|r| r.port == port).and_then(|r| r.cwd.clone()) {
                return Some(std::path::PathBuf::from(cwd));
            }
        }
        let beside = self.tabs.get(tab).and_then(|t| match (&t.left, t.right.as_ref()) {
            (Pane::Term(s), _) | (_, Some(Pane::Term(s))) => s.term.cwd.clone(),
            _ => None,
        });
        beside.or_else(|| self.focused_cwd()).map(std::path::PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn served_paths() {
        let dir = std::env::temp_dir().join(format!("nus-loop-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("docs").join("architecture"));
        std::fs::write(dir.join("index.html"), "x").unwrap();
        std::fs::write(dir.join("docs").join("architecture").join("index.html"), "x").unwrap();
        assert!(served_file(&dir, "/").unwrap().ends_with("index.html"));
        assert!(served_file(&dir, "/docs/architecture/").unwrap().ends_with("index.html"));
        assert!(served_file(&dir, "/nope/").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
