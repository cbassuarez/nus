//! Replay: the session has a timeline.
//!
//! Every shell's bytes go to a cast (asciinema v2, one per tab) under
//! `profile/replay/<session>/`, and every block boundary is a checkpoint —
//! an `m` marker event carrying the command, its exit, the folder, and the
//! page beside as a still (`blobs/<tab>-<n>.png`, the pane's own pixels).
//! TERMINAL · REPLAY: KEEP 7 DAYS · 1 DAY · OFF; old sessions are pruned.
//!
//! The timeline (Ctrl+Shift+H): the tab shows a moment instead of now —
//! the shell by replaying its cast into a scratch `Term` up to that
//! checkpoint (our core, so the picture is exact), the page as its still,
//! greyed *then*. ←/→ walk the checkpoints, B cycles after · before · diff
//! for the page (changed pixels in signal), Esc returns to now.
//!
//! Share (`nus share`, the palette): the tab's cast, its stills and the
//! site's wasm renderer bundled into one HTML file that replays anywhere,
//! with the real cells; open it, gist it, push it.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use serde_json::{json, Value};

use crate::app::{fade, App, Pane, TermPane};
use nus_render::theme::metric as m;

const WASM_JS: &str = include_str!("../assets/replay/nus_vt_wasm.js");
const WASM: &[u8] = include_bytes!("../assets/replay/nus_vt_wasm_bg.wasm");
const PLAYER_JS: &str = include_str!("../assets/replay/terminal.js");

pub fn dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("replay")
}

/// One session's casts, written as the shells produce bytes.
pub struct Recorder {
    pub dir: PathBuf,
    t0: Instant,
    casts: HashMap<u64, Cast>,
    stills: u32,
}

struct Cast {
    path: PathBuf,
    file: std::io::BufWriter<std::fs::File>,
    marks: usize,
}

fn now_secs() -> u64 {
    crate::journal::now()
}

impl Recorder {
    /// A new session directory; sessions older than `keep_days` go.
    pub fn new(keep_days: u32) -> Option<Recorder> {
        let root = dir();
        std::fs::create_dir_all(&root).ok()?;
        let cutoff = now_secs().saturating_sub(keep_days as u64 * 86_400);
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.parse::<u64>().is_ok_and(|t| t < cutoff) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let dir = root.join(now_secs().to_string());
        std::fs::create_dir_all(dir.join("blobs")).ok()?;
        Some(Recorder { dir, t0: Instant::now(), casts: HashMap::new(), stills: 0 })
    }

    fn t(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    fn cast(&mut self, tab: u64, cols: usize, rows: usize) -> Option<&mut Cast> {
        if !self.casts.contains_key(&tab) {
            let path = self.dir.join(format!("tab-{tab}.cast"));
            let mut file = std::io::BufWriter::new(std::fs::File::create(&path).ok()?);
            let header = json!({ "version": 2, "width": cols, "height": rows, "timestamp": now_secs(), "env": { "TERM": "xterm-256color", "SHELL": "nus" } });
            let _ = writeln!(file, "{header}");
            self.casts.insert(tab, Cast { path, file, marks: 0 });
        }
        self.casts.get_mut(&tab)
    }

    /// Bytes the shell produced.
    pub fn output(&mut self, tab: u64, cols: usize, rows: usize, bytes: &[u8]) {
        let t = self.t();
        if let Some(c) = self.cast(tab, cols, rows) {
            let ev = json!([t, "o", String::from_utf8_lossy(bytes)]);
            let _ = writeln!(c.file, "{ev}");
        }
    }

    /// The shell's size changed.
    pub fn resize(&mut self, tab: u64, cols: usize, rows: usize) {
        let t = self.t();
        if let Some(c) = self.cast(tab, cols, rows) {
            let ev = json!([t, "r", format!("{cols}x{rows}")]);
            let _ = writeln!(c.file, "{ev}");
        }
    }

    /// A checkpoint: the marker event, flushed so the timeline can read it.
    pub fn mark(&mut self, tab: u64, cols: usize, rows: usize, payload: &Value) {
        let t = self.t();
        if let Some(c) = self.cast(tab, cols, rows) {
            c.marks += 1;
            let ev = json!([t, "m", payload.to_string()]);
            let _ = writeln!(c.file, "{ev}");
            let _ = c.file.flush();
        }
    }

    pub fn flush(&mut self) {
        for c in self.casts.values_mut() {
            let _ = c.file.flush();
        }
    }

    pub fn cast_path(&self, tab: u64) -> Option<PathBuf> {
        self.casts.get(&tab).map(|c| c.path.clone())
    }

    /// Where the next still goes: `blobs/<tab>-<n>.png`.
    pub fn still_path(&mut self, tab: u64) -> PathBuf {
        self.stills += 1;
        self.dir.join("blobs").join(format!("{tab}-{}.png", self.stills))
    }
}

/// A checkpoint waiting on the next draw for its page still.
pub struct Pending {
    pub tab_index: usize,
    pub tab_id: u64,
    pub payload: Value,
    /// The page pane beside: right side?
    pub page: Option<bool>,
}

/// One event of a cast, parsed.
enum Ev {
    Out(f64, Vec<u8>),
    Resize(f64, usize, usize),
    Mark(f64, Value),
}

fn parse_cast(text: &str) -> (usize, usize, Vec<Ev>) {
    let mut lines = text.lines();
    let header: Value = lines.next().and_then(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null);
    let cols = header.get("width").and_then(Value::as_u64).unwrap_or(80) as usize;
    let rows = header.get("height").and_then(Value::as_u64).unwrap_or(24) as usize;
    let mut evs = Vec::new();
    for l in lines {
        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
        let t = v.get(0).and_then(Value::as_f64).unwrap_or(0.0);
        let kind = v.get(1).and_then(Value::as_str).unwrap_or("");
        let data = v.get(2).and_then(Value::as_str).unwrap_or("");
        match kind {
            "o" => evs.push(Ev::Out(t, data.as_bytes().to_vec())),
            "r" => {
                let mut it = data.split('x').filter_map(|n| n.parse::<usize>().ok());
                if let (Some(c), Some(r)) = (it.next(), it.next()) {
                    evs.push(Ev::Resize(t, c, r));
                }
            }
            "m" => evs.push(Ev::Mark(t, serde_json::from_str(data).unwrap_or(Value::Null))),
            _ => {}
        }
    }
    (cols, rows, evs)
}

/// The timeline over one tab.
pub struct Timeline {
    pub tab_index: usize,
    pub tab_id: u64,
    cols: usize,
    rows: usize,
    events: Vec<Ev>,
    /// Indices into `events` of the marks.
    marks: Vec<usize>,
    pub at: usize,
    pub term: nus_vt::Term,
    pub mode: Compare,
    pub hits: Vec<(Rect, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    After,
    Before,
    Diff,
}

impl Timeline {
    pub fn count(&self) -> usize {
        self.marks.len()
    }

    /// The checkpoint's payload at `i`.
    pub fn payload(&self, i: usize) -> Option<&Value> {
        self.marks.get(i).and_then(|&k| match &self.events[k] {
            Ev::Mark(_, v) => Some(v),
            _ => None,
        })
    }

    pub fn time(&self, i: usize) -> f64 {
        self.marks.get(i).map(|&k| match &self.events[k] {
            Ev::Out(t, _) | Ev::Resize(t, ..) | Ev::Mark(t, _) => *t,
        }).unwrap_or(0.0)
    }

    /// Replay the cast into a fresh term up to the checkpoint at `at`.
    fn rebuild(&mut self, palette_from: &nus_render::theme::Theme) {
        let mut term = nus_vt::Term::new(self.cols, self.rows, 4000);
        palette_from.apply(&mut term.palette);
        let end = self.marks.get(self.at).copied().unwrap_or(self.events.len());
        for ev in &self.events[..=end.min(self.events.len().saturating_sub(1))] {
            match ev {
                Ev::Out(_, bytes) => term.advance(bytes),
                Ev::Resize(_, c, r) => term.resize(*c, *r),
                Ev::Mark(..) => {}
            }
        }
        let _ = term.take_responses();
        let _ = term.take_events();
        self.term = term;
    }
}

/// A PNG's pixels as RGBA, with its size.
fn read_png(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

impl App {
    /// A texture from RGBA pixels, bound for the quad pipeline.
    pub(crate) fn bind_rgba(&self, rgba: &[u8], w: u32, h: u32) -> Arc<wgpu::BindGroup> {
        let bgra: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("still"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bgra,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        (self.bind_texture)(&tex)
    }

    /// A block finished: queue its checkpoint (the still is taken on the next draw).
    pub(crate) fn checkpoint(&mut self, tab_index: usize, payload: Value) {
        if self.recorder.is_none() {
            return;
        }
        let Some(tab) = self.tabs.get(tab_index) else { return };
        let page = if matches!(tab.right, Some(Pane::Web(_))) {
            Some(true)
        } else if matches!(tab.left, Pane::Web(_)) {
            Some(false)
        } else {
            None
        };
        self.checkpoints.push(Pending { tab_index, tab_id: tab.id, payload, page });
        self.dirty = true;
    }

    /// After a draw: stills for the pending checkpoints, then their marks.
    pub(crate) fn checkpoint_draw(&mut self, clear: [f32; 4]) {
        if self.checkpoints.is_empty() {
            return;
        }
        for mut p in std::mem::take(&mut self.checkpoints) {
            let (cols, rows) = self.tabs.get(p.tab_index).and_then(|t| match &t.left {
                Pane::Term(tp) => Some((tp.term.cols(), tp.term.rows())),
                _ => None,
            }).unwrap_or((80, 24));
            if let Some(right) = p.page {
                let info = self.tabs.get(p.tab_index).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }).and_then(|pane| match pane {
                    Pane::Web(w) => {
                        let s = w.tab.shared.borrow();
                        Some((w.page, s.url.clone(), s.title.clone()))
                    }
                    _ => None,
                });
                if let Some((r, url, title)) = info {
                    let path = self.recorder.as_mut().map(|rec| rec.still_path(p.tab_id));
                    if let Some(path) = path {
                        let crop = (r.x.max(0.0) as u32, r.y.max(0.0) as u32, r.w.max(1.0) as u32, r.h.max(1.0) as u32);
                        if self.snapshot_png(clear, Some(crop), &path).is_ok() {
                            let rel = format!("blobs/{}", path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
                            p.payload["page"] = json!({ "url": url, "title": title, "png": rel });
                        }
                    }
                }
            }
            if let Some(rec) = self.recorder.as_mut() {
                rec.mark(p.tab_id, cols, rows, &p.payload);
            }
        }
    }

    /// Ctrl+Shift+H: the timeline for the active tab, at its last checkpoint.
    pub(crate) fn toggle_timeline(&mut self) {
        if self.timeline.is_some() {
            self.close_timeline();
            return;
        }
        let i = self.active;
        let Some(tab) = self.tabs.get(i) else { return };
        let id = tab.id;
        if let Some(rec) = self.recorder.as_mut() {
            rec.flush();
        }
        let Some(path) = self.recorder.as_ref().and_then(|r| r.cast_path(id)) else {
            self.notice("nothing recorded for this tab yet");
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let (cols, rows, events) = parse_cast(&text);
        let marks: Vec<usize> = events.iter().enumerate().filter(|(_, e)| matches!(e, Ev::Mark(..))).map(|(k, _)| k).collect();
        if marks.is_empty() {
            self.notice("no checkpoints yet · run a command first");
            return;
        }
        let at = marks.len() - 1;
        let mut tl = Timeline { tab_index: i, tab_id: id, cols, rows, events, marks, at, term: nus_vt::Term::new(cols, rows, 10), mode: Compare::After, hits: Vec::new() };
        tl.rebuild(&self.theme);
        self.timeline = Some(tl);
        self.timeline_apply();
        self.play_event("toggle");
        self.dirty = true;
    }

    pub(crate) fn close_timeline(&mut self) {
        let Some(tl) = self.timeline.take() else { return };
        if let Some(tab) = self.tabs.get_mut(tl.tab_index) {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                match p {
                    Pane::Term(t) => {
                        t.replay = false;
                        t.view_key = None;
                    }
                    Pane::Web(w) => w.still = None,
                    _ => {}
                }
            }
        }
        self.layout();
        self.dirty = true;
    }

    /// Set the panes to show the checkpoint at `at`: the scratch term and the still.
    fn timeline_apply(&mut self) {
        let Some(tl) = self.timeline.as_ref() else { return };
        let (idx, at, mode) = (tl.tab_index, tl.at, tl.mode);
        let dir = self.recorder.as_ref().map(|r| r.dir.clone()).unwrap_or_default();
        let png_of = |p: Option<&Value>| p.and_then(|v| v.pointer("/page/png")).and_then(Value::as_str).map(|s| dir.join(s));
        let after = png_of(tl.payload(at));
        let before = if at > 0 { png_of(tl.payload(at - 1)) } else { None };
        let still = match mode {
            Compare::After => after.and_then(|p| read_png(&p)),
            Compare::Before => before.and_then(|p| read_png(&p)),
            Compare::Diff => match (before.and_then(|p| read_png(&p)), after.and_then(|p| read_png(&p))) {
                (Some((b, bw, bh)), Some((mut a, aw, ah))) if (bw, bh) == (aw, ah) => {
                    let sig = self.surface.signal;
                    let s = [(sig[0] * 255.0) as u8, (sig[1] * 255.0) as u8, (sig[2] * 255.0) as u8];
                    for (pa, pb) in a.chunks_mut(4).zip(b.chunks(4)) {
                        let d = (pa[0] as i32 - pb[0] as i32).abs() + (pa[1] as i32 - pb[1] as i32).abs() + (pa[2] as i32 - pb[2] as i32).abs();
                        if d > 40 {
                            pa[0] = s[0];
                            pa[1] = s[1];
                            pa[2] = s[2];
                        } else {
                            pa[0] = (pa[0] / 2) + 64;
                            pa[1] = (pa[1] / 2) + 64;
                            pa[2] = (pa[2] / 2) + 64;
                        }
                    }
                    Some((a, aw, ah))
                }
                (_, after) => after,
            },
        };
        let bind = still.map(|(rgba, w, h)| self.bind_rgba(&rgba, w, h));
        if let Some(tab) = self.tabs.get_mut(idx) {
            for p in std::iter::once(&mut tab.left).chain(tab.right.as_mut()) {
                match p {
                    Pane::Term(t) => {
                        t.replay = true;
                        t.view_key = None;
                    }
                    Pane::Web(w) => w.still = bind.clone(),
                    _ => {}
                }
            }
        }
    }

    /// Keys while the timeline is up. Returns true when it took the key.
    pub(crate) fn timeline_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key as K, NamedKey};
        let Some(tl) = self.timeline.as_mut() else { return false };
        match key {
            K::Named(NamedKey::Escape) => {
                self.close_timeline();
                return true;
            }
            K::Named(NamedKey::ArrowLeft) => tl.at = tl.at.saturating_sub(1),
            K::Named(NamedKey::ArrowRight) => tl.at = (tl.at + 1).min(tl.count().saturating_sub(1)),
            K::Named(NamedKey::Home) => tl.at = 0,
            K::Named(NamedKey::End) => tl.at = tl.count().saturating_sub(1),
            K::Character(c) if c.eq_ignore_ascii_case("b") => {
                tl.mode = match tl.mode {
                    Compare::After => Compare::Before,
                    Compare::Before => Compare::Diff,
                    Compare::Diff => Compare::After,
                };
            }
            _ => return true,
        }
        let theme = self.theme.clone();
        if let Some(tl) = self.timeline.as_mut() {
            tl.rebuild(&theme);
        }
        self.timeline_apply();
        self.play_event("toggle");
        self.dirty = true;
        true
    }

    /// A click on the ruler's ticks.
    pub(crate) fn timeline_click(&mut self, x: f32, y: f32) -> bool {
        let Some(tl) = self.timeline.as_mut() else { return false };
        let Some(&(_, i)) = tl.hits.iter().find(|(r, _)| r.contains(x, y)) else { return false };
        tl.at = i;
        let theme = self.theme.clone();
        if let Some(tl) = self.timeline.as_mut() {
            tl.rebuild(&theme);
        }
        self.timeline_apply();
        self.dirty = true;
        true
    }

    /// The ruler along the bottom of the shell pane: a tick per checkpoint,
    /// the current one in signal, the words in the tooltip.
    pub(crate) fn draw_timeline_ruler(&mut self, scene: &mut Scene, p: &TermPane, r: Rect) {
        let Some(tl) = self.timeline.as_ref() else { return };
        if !p.replay {
            return;
        }
        let t = self.theme.clone();
        let ink = t.ink;
        let n = tl.count();
        let at = tl.at;
        let mode = tl.mode;
        let payloads: Vec<(f64, String, Option<i64>)> = (0..n)
            .map(|i| {
                let v = tl.payload(i);
                (tl.time(i), v.and_then(|v| v.get("cmd")).and_then(Value::as_str).unwrap_or("").to_string(), v.and_then(|v| v.get("exit")).and_then(Value::as_i64))
            })
            .collect();
        let h = self.px(22.0);
        let bar = Rect::new(r.x, r.bottom() - h, r.w, h);
        scene.rect(bar, fade(self.paper(), 0.94));
        scene.hline(bar.x, bar.y, bar.w, self.px(m::HAIRLINE), fade(ink, 0.35));
        let label = Style { color: t.dim, ..self.label() };
        let isz = self.px(12.0);
        let mut x = bar.x + self.px(10.0);
        self.fonts.draw_icon(scene, nus_render::text::icons::HISTORY, isz, x, bar.y + (bar.h - isz) / 2.0, self.surface.signal);
        x += isz + self.px(10.0);
        let words = match mode {
            Compare::After => "then",
            Compare::Before => "before",
            Compare::Diff => "diff",
        };
        x += self.fonts.draw(scene, label, x, bar.y + bar.h / 2.0 + self.px(4.0), words) + self.px(14.0);
        let span = (bar.right() - self.px(12.0) - x).max(self.px(40.0));
        let step = if n > 1 { span / (n - 1) as f32 } else { 0.0 };
        let (mx, my) = self.mouse;
        let mut hits = Vec::new();
        scene.hline(x, bar.y + bar.h / 2.0, span, self.px(m::HAIRLINE), fade(ink, 0.4));
        for (i, (time, cmd, exit)) in payloads.iter().enumerate() {
            let tx = x + step * i as f32;
            let d = if i == at { self.px(8.0) } else { self.px(5.0) };
            let tick = Rect::new(tx - d / 2.0, bar.y + (bar.h - d) / 2.0, d, d);
            let color = if i == at { self.surface.signal } else { match exit { Some(0) => fade(ink, 0.6), Some(_) => fade(self.surface.signal, 0.5), None => fade(ink, 0.3) } };
            scene.rect(tick, color);
            let hit = Rect::new(tx - self.px(8.0), bar.y, self.px(16.0), bar.h);
            if hit.contains(mx, my) {
                self.tip_words(hit, &format!("{} · {:.0}s in · {}", if cmd.is_empty() { "start" } else { cmd.as_str() }, time, match exit { Some(0) => "ok".to_string(), Some(c) => format!("exit {c}"), None => String::new() }));
            }
            hits.push((hit, i));
        }
        if let Some(tl) = self.timeline.as_mut() {
            tl.hits = hits;
        }
    }

    /// Share: one HTML file that replays the tab's cast with the real
    /// renderer, its stills beside. Returns the path.
    pub(crate) fn share_replay(&mut self, tab_index: usize) -> Result<PathBuf, String> {
        let Some(tab) = self.tabs.get(tab_index) else { return Err("no such tab".into()) };
        let id = tab.id;
        let title = tab.title();
        let Some(rec) = self.recorder.as_mut() else { return Err("replay is off".into()) };
        rec.flush();
        let cast_path = rec.cast_path(id).ok_or("nothing recorded for this tab")?;
        let session = rec.dir.clone();
        let cast = std::fs::read_to_string(&cast_path).map_err(|e| e.to_string())?;
        let (_, _, events) = parse_cast(&cast);
        // The stills, inlined as data URLs.
        let mut stills = Vec::new();
        for ev in &events {
            if let Ev::Mark(t, v) = ev {
                let cmd = v.get("cmd").and_then(Value::as_str).unwrap_or("").to_string();
                let png = v.pointer("/page/png").and_then(Value::as_str).and_then(|rel| std::fs::read(session.join(rel)).ok()).map(|b| crate::replay::b64(&b));
                let url = v.pointer("/page/url").and_then(Value::as_str).unwrap_or("").to_string();
                stills.push(json!({ "t": t, "cmd": cmd, "exit": v.get("exit"), "url": url, "png": png }));
            }
        }
        let out_dir = std::env::current_dir().unwrap_or_default().join("profile").join("shares");
        std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
        let path = out_dir.join(format!("replay-{}.html", now_secs()));
        let html = share_html(&title, &cast, &stills, &self.theme, self.surface.signal);
        std::fs::write(&path, html).map_err(|e| e.to_string())?;
        Ok(path)
    }
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() / 3 * 4 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn hex(c: nus_render::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

/// The player, bundled: the wasm glue and the site's terminal.js with their
/// module syntax removed, the wasm and the cast inline, so it runs from a
/// file:// page and travels as one file.
fn share_html(title: &str, cast: &str, stills: &[Value], theme: &nus_render::theme::Theme, signal: nus_render::Color) -> String {
    let glue = WASM_JS
        .replace("export class Shell", "class Shell")
        .replace("export function source_rev", "function source_rev")
        .replace("export { initSync, __wbg_init as default };", "const init = __wbg_init;");
    let player = PLAYER_JS
        .replace("import init, { Shell } from '../wasm/nus_vt_wasm.js';", "")
        .replace("export async function mountHeroTerminal", "async function mountHeroTerminal")
        .replace("export function", "function")
        .replace("export class", "class");
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let stills_html: String = stills
        .iter()
        .map(|s| {
            let cmd = esc(s.get("cmd").and_then(Value::as_str).unwrap_or(""));
            let t = s.get("t").and_then(Value::as_f64).unwrap_or(0.0);
            let png = s.get("png").and_then(Value::as_str);
            let url = esc(s.get("url").and_then(Value::as_str).unwrap_or(""));
            let img = png.map(|p| format!("<img src=\"data:image/png;base64,{p}\" alt=\"the page beside\">")).unwrap_or_default();
            format!("<figure data-t=\"{t}\"><figcaption><code>$ {cmd}</code> <span>{t:.0}s · {url}</span></figcaption>{img}</figure>")
        })
        .collect();
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><title>{title} · a replay from nus</title>
<style>
:root {{ --paper:{paper}; --ink:{ink}; --dim:{dim}; --signal:{sig}; }}
html,body {{ margin:0; background:var(--paper); color:var(--ink); font-family:"IBM Plex Mono","Cascadia Mono",Consolas,monospace; font-size:13px; }}
.band {{ position:fixed; left:0; top:0; right:0; height:6px; background:var(--signal); }}
main {{ display:grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap:24px; padding:34px; }}
h1 {{ font-family:"Newsreader","Times New Roman",serif; font-style:italic; font-weight:400; font-size:28px; margin:0 0 12px; grid-column:1/-1; }}
.term {{ border:1px solid var(--ink); background:#141414; position:relative; }}
.term canvas {{ display:block; width:100%; }}
.bar {{ display:flex; gap:12px; align-items:center; padding:8px 10px; border-top:1px solid var(--ink); font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--dim); }}
.bar input {{ flex:1; }}
.bar button {{ font:inherit; background:none; border:1px solid var(--ink); color:var(--ink); padding:2px 8px; cursor:pointer; }}
aside figure {{ margin:0 0 18px; }}
aside img {{ width:100%; border:1px solid var(--ink); display:block; }}
aside figcaption {{ font-size:12px; margin-bottom:6px; }}
aside figcaption span {{ color:var(--dim); margin-left:8px; }}
aside figure.now img {{ outline:2px solid var(--signal); }}
.foot {{ grid-column:1/-1; border-top:1px solid var(--ink); padding-top:8px; font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--dim); }}
</style></head><body>
<div class="band"></div>
<main>
<h1>{title}</h1>
<section class="term" data-hero-term>
  <div data-term-stage><canvas></canvas></div>
  <div class="bar"><button data-term-play>pause</button><input type="range" data-term-scrub min="0" max="1000" value="0"><span data-term-status></span><span data-term-size></span></div>
</section>
<aside>{stills}</aside>
<div class="foot">a replay from nus · the cells are the app's own renderer, in wasm · the stills are the page beside, at each command</div>
</main>
<script type="module">
{glue}
{player}
const CAST = {cast_json};
const WASM_B64 = "{wasm}";
const bytes = Uint8Array.from(atob(WASM_B64), c => c.charCodeAt(0));
const root = document.querySelector('[data-hero-term]');
mountHeroTerminal(root, {{ wasm: bytes, castText: CAST }}).then((term) => {{
  // Light the still of the command the replay is inside.
  const figs = [...document.querySelectorAll('aside figure')];
  setInterval(() => {{
    const t = term.t ?? term.time ?? 0;
    let cur = null;
    for (const f of figs) if (parseFloat(f.dataset.t) <= t) cur = f;
    figs.forEach(f => f.classList.toggle('now', f === cur));
  }}, 250);
}}).catch((e) => {{ document.querySelector('[data-term-status]').textContent = 'replay failed: ' + e; }});
</script>
</body></html>"##,
        title = esc(title),
        paper = hex(theme.paper),
        ink = hex(theme.ink),
        dim = hex(theme.dim),
        sig = hex(signal),
        stills = stills_html,
        glue = glue,
        player = player,
        cast_json = serde_json::to_string(cast).unwrap_or_default(),
        wasm = b64(WASM),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_parses_marks() {
        let text = "{\"version\":2,\"width\":80,\"height\":24}\n[0.1,\"o\",\"hi\"]\n[0.2,\"r\",\"100x30\"]\n[0.3,\"m\",\"{\\\"cmd\\\":\\\"ls\\\"}\"]\n";
        let (c, r, evs) = parse_cast(text);
        assert_eq!((c, r), (80, 24));
        assert_eq!(evs.len(), 3);
        assert!(matches!(evs[1], Ev::Resize(_, 100, 30)));
        assert!(matches!(&evs[2], Ev::Mark(_, v) if v["cmd"] == "ls"));
    }

    #[test]
    fn share_bundles_without_module_syntax() {
        let html = share_html("t", "{\"version\":2}\n", &[], &nus_render::theme::Theme::ink(), [1.0, 0.0, 0.0, 1.0]);
        assert!(!html.contains("export class Shell"));
        assert!(!html.contains("import init"));
        assert!(html.contains("const init = __wbg_init;"));
        assert!(html.contains("WASM_B64"));
    }
}
