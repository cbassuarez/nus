//! Layouts as files. A `.nus.luau` returns a table describing a window —
//! tabs, panes, the hatch — and can compute (it's Luau, sandboxed like
//! rules). Open one with `nus open layout.nus.luau`, the palette, or the
//! atlas; a `.nus.luau` in a shell's cwd offers itself; SAVE LAYOUT in
//! the palette writes the current window as one.
//!
//! ```luau
//! return {
//!   space = "nus",
//!   tabs = {
//!     { shell = "pwsh", cwd = env.HOME .. "/nus", run = "cargo watch -x check" },
//!     { page = "http://localhost:5173/", beside = 1 },
//!     { edit = "src/main.rs" },
//!     { page = "https://docs.rs/wgpu", name = "wgpu docs", pinned = true },
//!   },
//!   hatch = { shell = "pwsh" },
//! }
//! ```

use std::path::{Path, PathBuf};

use crate::app::{App, Pane};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutTab {
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub run: Option<String>,
    pub page: Option<String>,
    pub edit: Option<String>,
    /// 1-based index of the tab this one sits beside (as its split).
    pub beside: Option<usize>,
    pub name: Option<String>,
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub space: Option<String>,
    pub tabs: Vec<LayoutTab>,
    pub hatch: Option<LayoutTab>,
}

/// Evaluate a layout file in a sandbox with `env` (HOME, USER, the file's
/// own folder as `here`) available.
pub fn load(path: &Path) -> Result<Layout, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lua = mlua::Lua::new();
    lua.sandbox(true).ok();
    let env = lua.create_table().map_err(|e| e.to_string())?;
    if let Some(h) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let _ = env.set("HOME", h.to_string_lossy().to_string());
    }
    if let Ok(u) = std::env::var("USERNAME").or_else(|_| std::env::var("USER")) {
        let _ = env.set("USER", u);
    }
    if let Some(d) = path.parent() {
        let _ = env.set("here", d.display().to_string());
    }
    let _ = lua.globals().set("env", env);
    let v: mlua::Value = lua.load(&src).set_name(path.display().to_string()).eval().map_err(|e| crate::surface::first_line(&e.to_string()))?;
    let mlua::Value::Table(t) = v else { return Err("a layout file must return a table".into()) };
    let tab_of = |v: mlua::Table| -> LayoutTab {
        LayoutTab {
            shell: v.get("shell").ok(),
            cwd: v.get("cwd").ok(),
            run: v.get("run").ok(),
            page: v.get("page").ok(),
            edit: v.get("edit").ok(),
            beside: v.get::<usize>("beside").ok(),
            name: v.get("name").ok(),
            pinned: v.get("pinned").unwrap_or(false),
        }
    };
    let tabs: Vec<LayoutTab> = t
        .get::<mlua::Table>("tabs")
        .map(|list| list.sequence_values::<mlua::Table>().filter_map(|r| r.ok()).map(tab_of).collect())
        .unwrap_or_default();
    let hatch = t.get::<mlua::Table>("hatch").ok().map(tab_of);
    Ok(Layout { space: t.get("space").ok(), tabs, hatch })
}

/// Write a layout as Luau.
pub fn render(l: &Layout) -> String {
    fn q(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
    let mut out = String::from("-- a nus layout · open it with `nus open <this file>`\nreturn {\n");
    if let Some(s) = &l.space {
        out.push_str(&format!("  space = {},\n", q(s)));
    }
    out.push_str("  tabs = {\n");
    for t in &l.tabs {
        let mut f: Vec<String> = Vec::new();
        if let Some(s) = &t.shell {
            f.push(format!("shell = {}", q(s)));
        }
        if let Some(s) = &t.cwd {
            f.push(format!("cwd = {}", q(s)));
        }
        if let Some(s) = &t.run {
            f.push(format!("run = {}", q(s)));
        }
        if let Some(s) = &t.page {
            f.push(format!("page = {}", q(s)));
        }
        if let Some(s) = &t.edit {
            f.push(format!("edit = {}", q(s)));
        }
        if let Some(b) = t.beside {
            f.push(format!("beside = {b}"));
        }
        if let Some(s) = &t.name {
            f.push(format!("name = {}", q(s)));
        }
        if t.pinned {
            f.push("pinned = true".into());
        }
        out.push_str(&format!("    {{ {} }},\n", f.join(", ")));
    }
    out.push_str("  },\n");
    if let Some(h) = &l.hatch {
        let mut f: Vec<String> = Vec::new();
        if let Some(s) = &h.shell {
            f.push(format!("shell = {}", q(s)));
        }
        if let Some(s) = &h.cwd {
            f.push(format!("cwd = {}", q(s)));
        }
        out.push_str(&format!("  hatch = {{ {} }},\n", f.join(", ")));
    }
    out.push_str("}\n");
    out
}

fn layouts_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("layouts")
}

/// The saved layouts under profile/layouts, by name.
pub fn saved() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(layouts_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".nus.luau")))
        .map(|p| (p.file_name().unwrap().to_string_lossy().trim_end_matches(".nus.luau").to_string(), p))
        .collect();
    v.sort();
    v
}

impl App {
    /// Build the window from a layout: every tab, its split, the hatch.
    pub(crate) fn open_layout(&mut self, path: &Path) {
        let l = match load(path) {
            Ok(l) => l,
            Err(e) => {
                self.notice(&format!("layout · {e}"));
                return;
            }
        };
        let l = self.rules.on_open_layout(l);
        if let Some(s) = &l.space {
            self.space_name = s.clone();
        }
        let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut made: Vec<Option<usize>> = Vec::new();
        for t in &l.tabs {
            let pane = self.layout_pane(t, &base);
            let Some(pane) = pane else {
                made.push(None);
                continue;
            };
            // Beside an earlier tab: its split.
            if let Some(b) = t.beside.and_then(|b| made.get(b.wrapping_sub(1)).copied().flatten()) {
                if let Some(tab) = self.tabs.get_mut(b) {
                    if tab.right.is_none() {
                        tab.right = Some(pane);
                        made.push(Some(b));
                        continue;
                    }
                }
            }
            let mut tab = self.make_tab(pane, None);
            tab.name = t.name.clone();
            tab.pinned = t.pinned;
            self.tabs.push(tab);
            made.push(Some(self.tabs.len() - 1));
        }
        if let Some(h) = &l.hatch {
            if self.hatch_tab().is_none() {
                if let Some(pane) = self.layout_pane(h, &base) {
                    let mut tab = self.make_tab(pane, None);
                    tab.hatch = true;
                    self.tabs.push(tab);
                }
            }
        }
        if let Some(first) = made.iter().flatten().next() {
            self.activate(*first);
        }
        self.remember(crate::start::Saved::Layout { path: path.display().to_string() });
        self.apply_term_resizes(false);
        self.layout();
        self.save_session();
        self.dirty = true;
    }

    fn layout_pane(&mut self, t: &crate::layout_file::LayoutTab, base: &Path) -> Option<Pane> {
        let resolve = |p: &str| -> PathBuf {
            let pb = PathBuf::from(p);
            if pb.is_absolute() {
                pb
            } else {
                base.join(pb)
            }
        };
        if let Some(url) = &t.page {
            return self.new_web_pane(url).map(Pane::Web);
        }
        if let Some(f) = &t.edit {
            let mut e = crate::editor::EditorPane::new(nus_render::Rect::new(0.0, 0.0, 1.0, 1.0));
            return e.open(&resolve(f)).ok().map(|_| Pane::Editor(e));
        }
        let profile = t.shell.as_deref().and_then(|n| self.profiles.iter().position(|p| p.name.eq_ignore_ascii_case(n))).unwrap_or(self.behavior.default_profile);
        let mut term = self.new_term_pane(false, profile).ok()?;
        if let Some(c) = &t.cwd {
            let _ = term.pty.write(format!("cd \"{}\"\r", resolve(c).display()).as_bytes());
        }
        if let Some(r) = &t.run {
            let _ = term.pty.write(format!("{r}\r").as_bytes());
        }
        Some(Pane::Term(term))
    }

    /// The window as a layout.
    pub(crate) fn current_layout(&self) -> Layout {
        let mut tabs = Vec::new();
        let mut hatch = None;
        let pane_of = |p: &Pane| -> LayoutTab {
            match p {
                Pane::Term(t) => LayoutTab { shell: self.profiles.get(t.profile).map(|p| p.name.clone()), cwd: t.term.cwd.clone(), ..Default::default() },
                Pane::Web(w) => LayoutTab { page: Some(w.tab.shared.borrow().url.clone()), ..Default::default() },
                Pane::Editor(e) => LayoutTab { edit: e.buf().and_then(|b| b.path.as_ref()).map(|p| p.display().to_string()), ..Default::default() },
                _ => LayoutTab::default(),
            }
        };
        for t in self.tabs.iter().filter(|t| t.peek.is_none()) {
            if t.hatch {
                hatch = Some(pane_of(&t.left));
                continue;
            }
            let mut l = pane_of(&t.left);
            if l == LayoutTab::default() {
                continue;
            }
            l.name = t.name.clone();
            l.pinned = t.pinned;
            tabs.push(l);
            if let Some(r) = &t.right {
                let mut rt = pane_of(r);
                if rt != LayoutTab::default() {
                    rt.beside = Some(tabs.len());
                    tabs.push(rt);
                }
            }
        }
        Layout { space: Some(self.space_name.clone()), tabs, hatch }
    }

    /// SAVE LAYOUT: write the window under profile/layouts/<name>.nus.luau.
    pub(crate) fn save_layout(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let dir = layouts_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{name}.nus.luau"));
        match std::fs::write(&path, render(&self.current_layout())) {
            Ok(()) => self.notice(&format!("layout saved · {}", path.display())),
            Err(e) => self.notice(&format!("layout · {e}")),
        }
    }

    /// A shell's cwd has a layout file: offer it once per folder.
    pub(crate) fn offer_layout_here(&mut self) {
        let Some(cwd) = self.tabs.get(self.active).and_then(|t| match &t.left {
            Pane::Term(term) => term.term.cwd.clone(),
            _ => None,
        }) else {
            return;
        };
        let dir = PathBuf::from(&cwd);
        if self.layout_offered.contains(&dir) {
            return;
        }
        let Some(file) = std::fs::read_dir(&dir).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".nus.luau")))) else {
            self.layout_offered.insert(dir);
            return;
        };
        self.layout_offered.insert(dir);
        self.layout_offer = Some((file, std::time::Instant::now()));
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let l = Layout {
            space: Some("nus".into()),
            tabs: vec![
                LayoutTab { shell: Some("pwsh".into()), cwd: Some("C:\\x".into()), run: Some("cargo watch".into()), ..Default::default() },
                LayoutTab { page: Some("http://localhost:5173/".into()), beside: Some(1), ..Default::default() },
                LayoutTab { edit: Some("src/main.rs".into()), name: Some("main".into()), pinned: true, ..Default::default() },
            ],
            hatch: Some(LayoutTab { shell: Some("pwsh".into()), ..Default::default() }),
        };
        let src = render(&l);
        let dir = std::env::temp_dir().join(format!("nus-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.nus.luau");
        std::fs::write(&p, &src).unwrap();
        let back = load(&p).unwrap();
        assert_eq!(back, l);
        // A computed layout.
        std::fs::write(&p, "return { tabs = { { shell = \"bash\", cwd = env.here .. \"/x\" } } }").unwrap();
        let c = load(&p).unwrap();
        assert!(c.tabs[0].cwd.as_deref().unwrap().ends_with("/x"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
