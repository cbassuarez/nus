//! Replay: the session has a timeline.
//!
//! Every shell's bytes go to a cast (asciinema v2, one per tab) under
//! `profile/replay/<session>/`, and every block boundary is a checkpoint —
//! an `m` marker event carrying the command, its exit, the folder, and the
//! page beside as a still (`blobs/<tab>-<n>.png`, the pane's own pixels).
//! TERMINAL · REPLAY: KEEP 7 DAYS · 1 DAY · OFF; old sessions are pruned.
//!
//! Ctrl+Shift+H opens a read-only history document with a session map.
//! Commands are detents in that map; the document and overview share line
//! positions. Search, copy, and keyboard navigation stay inside the history.
//! A terminal snapshot, page comparison, and portable playback are optional.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use nus_render::{Rect, Scene};
use serde_json::{json, Value};

use crate::app::{App, Pane};
#[path="replay_history.rs"] mod history;
pub(crate) use history::HistoryHit;

const WASM_JS: &str = include_str!("../assets/replay/nus_vt_wasm.js");
const WASM: &[u8] = include_bytes!("../assets/replay/nus_vt_wasm_bg.wasm");
const PLAYER_JS: &str = include_str!("../assets/replay/terminal.js");

pub fn dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("replay")
}

/// Rolling recent history. Each pane retains two segments, and all generated
/// casts/stills in this window share a budget. Explicitly exported replays stay.
pub struct Recorder {
    pub dir: PathBuf,
    t0: Instant,
    casts: HashMap<u64, Cast>,
    stills: u32,
    pending_bytes: u64,
}

struct Cast {
    path: PathBuf,
    file: Option<nus_vault::StreamWriter>,
    header: String,
    bytes: u64,
    utf8: Vec<u8>,
}

impl Cast {
    fn write(&mut self, event: &Value, limit: u64) {
        let line = format!("{event}\n");
        let stored=line.len() as u64+52;
        if stored > limit / 2 { return; }
        if self.bytes + stored > limit {
            // Close before renaming, including on Windows. Failure stops this
            // write rather than allowing the current file to grow indefinitely.
            if let Some(mut file) = self.file.take() { let _ = file.flush(); }
            let previous = self.path.with_extension("previous.cast");
            if previous.exists() && std::fs::remove_file(&previous).is_err() { return; }
            if std::fs::rename(&self.path, &previous).is_err() { return; }
            let Ok(mut file) = nus_vault::StreamWriter::create(&self.path) else { return; };
            if file.write_all(self.header.as_bytes()).is_err() { return; }
            self.bytes = self.header.len() as u64+132;
            self.file = Some(file);
        }
        if let Some(file) = self.file.as_mut() {
            if file.write_all(line.as_bytes()).is_ok() { self.bytes += stored; }
        }
    }
}

fn now_secs() -> u64 { crate::journal::now() }

impl Recorder {
    pub fn new(_keep_days: u32) -> Option<Recorder> {
        if crate::private::enabled() { return None; }
        let root = dir();
        crate::protected_state::ready(&nus_vault::profile_for(&root)).ok()?;
        std::fs::create_dir_all(&root).ok()?;
        let mut stamp = now_secs();
        loop {
            let dir = root.join(stamp.to_string());
            match std::fs::create_dir(&dir) {
                Ok(()) => {
                    std::fs::create_dir(dir.join("blobs")).ok()?;
                    return Some(Recorder { dir, t0: crate::clock::now(), casts: HashMap::new(), stills: 0, pending_bytes: 0 });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => stamp += 1,
                Err(_) => return None,
            }
        }
    }

    fn t(&self) -> f64 { crate::clock::since(self.t0).as_secs_f64() }

    fn cast(&mut self, tab: u64, cols: usize, rows: usize) -> Option<&mut Cast> {
        if !self.casts.contains_key(&tab) {
            let path = self.dir.join(format!("tab-{tab}.cast"));
            let existing=if path.is_file(){Some(nus_vault::read(&path).ok()?)}else{None};
            let mut file = nus_vault::StreamWriter::create(&path).ok()?;
            let header = format!("{}\n", json!({ "version": 2, "width": cols, "height": rows, "timestamp": now_secs(), "env": { "TERM": "xterm-256color", "SHELL": "nus" } }));
            let initial=existing.as_deref().unwrap_or(header.as_bytes());
            file.write_all(initial).ok()?;let bytes=initial.len() as u64+132;
            self.casts.insert(tab, Cast { path, file: Some(file), header, bytes, utf8: Vec::new() });
        }
        self.casts.get_mut(&tab)
    }

    pub fn output(&mut self, tab: u64, cols: usize, rows: usize, bytes: &[u8]) {
        let t = self.t();
        for chunk in bytes.chunks(16 * 1024) {
            if let Some(c) = self.cast(tab, cols, rows) {
                let text = decode_chunk(&mut c.utf8, chunk);
                if !text.is_empty() { c.write(&json!([t, "o", text]), crate::storage::REPLAY_SEGMENT); }
            }
            self.pending_bytes += chunk.len() as u64 * 6; // JSON escaping upper bound.
            if self.pending_bytes >= 64 * 1024 { self.enforce_budget(Some(tab), crate::storage::REPLAY_WINDOW); }
        }
    }

    pub fn resize(&mut self, tab: u64, cols: usize, rows: usize) {
        let t = self.t();
        if let Some(c) = self.cast(tab, cols, rows) { c.write(&json!([t, "r", format!("{cols}x{rows}")]), crate::storage::REPLAY_SEGMENT); }
        self.pending_bytes += 128;
        if self.pending_bytes >= 64 * 1024 { self.enforce_budget(Some(tab), crate::storage::REPLAY_WINDOW); }
    }

    pub fn mark(&mut self, tab: u64, cols: usize, rows: usize, payload: &Value) {
        let t = self.t();
        let mut payload = payload.clone();
        // Keep command boundaries even when a command produced enormous output.
        if let Some(output) = payload.get_mut("output") {
            if let Some(text) = output.as_str().filter(|s| s.len() > 128 * 1024) {
                let mut start = text.len() - 128 * 1024;
                while !text.is_char_boundary(start) { start += 1; }
                *output = json!(format!("[Earlier output omitted from recent history]\n{}", &text[start..]));
            }
        }
        if let Some(c) = self.cast(tab, cols, rows) {
            c.write(&json!([t, "m", payload.to_string()]), crate::storage::REPLAY_SEGMENT);
        }
        self.enforce_budget(Some(tab), crate::storage::REPLAY_WINDOW);
    }

    pub fn flush(&mut self) {
        for c in self.casts.values_mut() { if let Some(f) = c.file.as_mut() { let _ = f.flush(); } }
    }

    /// Closing a pane releases its file descriptor immediately at maintenance.
    pub fn retain(&mut self, streams: &std::collections::HashSet<u64>) {
        self.casts.retain(|id, _| streams.contains(id));
    }

    fn enforce_budget(&mut self, current: Option<u64>, budget: u64) {
        self.flush(); self.pending_bytes = 0;
        let mut entries = crate::storage::files(&self.dir, true);
        let protected = current.map(|id| self.dir.join(format!("tab-{id}.cast")));
        entries.sort_by_key(|e| (protected.as_ref() == Some(&e.path), e.modified));
        let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
        for e in entries {
            if total <= budget { break; }
            // Close buffered handles before deletion, on every platform.
            self.casts.retain(|_, c| c.path != e.path);
            if std::fs::remove_file(&e.path).is_ok() { total = total.saturating_sub(e.bytes); }
        }
    }

    pub fn cast_path(&self, tab: u64) -> Option<PathBuf> {
        let path = self.dir.join(format!("tab-{tab}.cast"));
        path.exists().then_some(path)
    }

    pub fn still_path(&mut self, tab: u64) -> PathBuf {
        self.stills = self.stills.wrapping_add(1);
        self.dir.join("blobs").join(format!("{tab}-{}.png", self.stills))
    }
}

fn read_cast(path: &Path) -> std::io::Result<String> {
    let mut text = crate::storage::tail(&path.with_extension("previous.cast"), crate::storage::REPLAY_SEGMENT).unwrap_or_default();
    text.push_str(&crate::storage::tail(path, crate::storage::REPLAY_SEGMENT)?);
    Ok(text)
}

/// A checkpoint waiting on the next draw for its page still.
pub struct Pending {
    pub tab_index: usize,
    pub tab_id: u64,
    pub right: bool,
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
    let cols = dimension(header.get("width").and_then(Value::as_u64).unwrap_or(80));
    let rows = dimension(header.get("height").and_then(Value::as_u64).unwrap_or(24));
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
                    evs.push(Ev::Resize(t, dimension(c as u64), dimension(r as u64)));
                }
            }
            "m" => evs.push(Ev::Mark(t, serde_json::from_str(data).unwrap_or(Value::Null))),
            _ => {}
        }
    }
    (cols, rows, evs)
}


/// Separate pane streams; old left-pane cast names remain valid.
pub(crate) fn stream_id(tab:u64,right:bool)->u64 {tab | if right {1<<63}else{0}}
fn dimension(n:u64)->usize {n.clamp(1,1000)as usize}
fn decode_chunk(pending:&mut Vec<u8>,bytes:&[u8])->String {
    pending.extend_from_slice(bytes);let mut out=String::new();let mut used=0;
    while used<pending.len() {
        match std::str::from_utf8(&pending[used..]) {
            Ok(s)=>{out.push_str(s);used=pending.len();},
            Err(e)=>{let end=used+e.valid_up_to();out.push_str(std::str::from_utf8(&pending[used..end]).unwrap());used=end;
                if let Some(n)=e.error_len(){out.push('\u{fffd}');used+=n;}else{break;}}
        }
    }
    pending.drain(..used);out
}
fn records(cols:usize,rows:usize,events:&[Ev])->Vec<Value> {
    let mut term=nus_vt::Term::new(cols,rows,4000);let mut result=Vec::new();let mut last=0.0;
    for ev in events {
        match ev {
            Ev::Out(_,bytes)=>term.advance(bytes),Ev::Resize(_,c,r)=>term.resize(*c,*r),
            Ev::Mark(t,payload)=>{
                let mut v=if payload.is_object(){payload.clone()}else{json!({})};
                v["t"]=json!(t);let ms=v["ms"].as_f64().unwrap_or(0.0);
                v["start"]=json!((t-ms/1000.0).max(last).min(*t));last=*t;
                if v.get("output").is_none(){
                    let output=term.marks.iter().rev().find(|m|m.kind==nus_vt::MarkKind::OutputStart).map(|m|term.output_text(m)).unwrap_or_else(||term.grid().text());
                    v["output"]=json!(output);v["output_source"]=json!("reconstructed");
                }
                result.push(v);
            }
        }
        let _=term.take_responses();let _=term.take_events();
    }
    if result.is_empty() && !events.is_empty(){result.push(json!({"cmd":"Session output","output":term.grid().text(),"t":0,"start":0}));}
    result
}

/// The timeline over one tab.
pub struct Timeline {
    pub tab_id: u64,
    cols: usize,
    rows: usize,
    events: Vec<Ev>,
    /// Indices into `events` of the marks.
    marks: Vec<usize>,
    pub at: usize,
    pub term: nus_vt::Term,
    pub mode: Compare,
    pub hits: Vec<(Rect, HistoryHit)>,
    pub right: bool,
    pub records: Vec<Value>,
    pub query: String,
    pub snapshot: bool,
    pub focus: Option<HistoryHit>,
    pub area: Rect,
    pub list_area: Rect,
    pub detail_area: Rect,
    pub detail_scroll: f32,
    pub detail_max: f32,
    pub reveal: bool,
    pub ranges: Vec<(usize,f32,f32)>,
    pub map_drag: Option<f32>,
    pub follow_scroll: bool,
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

    /// Replay the cast into a fresh term up to the checkpoint at `at`.
    fn rebuild(&mut self, palette_from: &nus_render::theme::Theme) {
        let mut term = nus_vt::Term::new(self.cols, self.rows, 4000);
        palette_from.apply(&mut term.palette);
        let end = self.marks.get(self.at).copied().unwrap_or(self.events.len());
        for ev in self.events.iter().take(end.saturating_add(1)) {
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
    let bytes=nus_vault::read(path).ok()?;
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
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
        let right=payload.get("right").and_then(Value::as_bool).unwrap_or(false);
        self.checkpoints.push(Pending { tab_index, tab_id: tab.id, right, payload, page });
        self.dirty = true;
    }

    /// After a draw: stills for the pending checkpoints, then their marks.
    pub(crate) fn checkpoint_draw(&mut self, clear: [f32; 4]) {
        if self.checkpoints.is_empty() {
            return;
        }
        for mut p in std::mem::take(&mut self.checkpoints) {
            let Some(index)=self.tabs.iter().position(|t|t.id==p.tab_id) else {continue;};p.tab_index=index;
            let (cols, rows) = self.tabs.get(p.tab_index).and_then(|t|if p.right {t.right.as_ref()}else{Some(&t.left)}).and_then(|pane| match pane {
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
                rec.mark(stream_id(p.tab_id,p.right), cols, rows, &p.payload);
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
        let right=tab.focus_right && matches!(tab.right,Some(Pane::Term(_)));
        if let Some(rec) = self.recorder.as_mut() {
            rec.flush();
        }
        let Some(path) = self.recorder.as_ref().and_then(|r| r.cast_path(stream_id(id,right))) else {
            self.notice(nus_render::text::icons::HISTORY, "Nothing Recorded Yet", "for this tab");
            return;
        };
        let Ok(text) = read_cast(&path) else { return };
        let (cols, rows, events) = parse_cast(&text);
        let marks: Vec<usize> = events.iter().enumerate().filter(|(_, e)| matches!(e, Ev::Mark(..))).map(|(k, _)| k).collect();
        if marks.is_empty() {
            self.notice(nus_render::text::icons::HISTORY, "No Checkpoints Yet", "run a command first");
            return;
        }
        let at = marks.len() - 1;
        let records=records(cols,rows,&events);
        let mut tl = Timeline { tab_id: id, cols, rows, events, marks, at, term: nus_vt::Term::new(cols, rows, 10), mode: Compare::After, hits: Vec::new(),right,records,query:String::new(),snapshot:false,focus:Some(HistoryHit::Search),area:Rect::new(0.0,0.0,0.0,0.0),list_area:Rect::new(0.0,0.0,0.0,0.0),detail_area:Rect::new(0.0,0.0,0.0,0.0),detail_scroll:0.0,detail_max:0.0,reveal:true,ranges:Vec::new(),map_drag:None,follow_scroll:false };
        tl.rebuild(&self.theme);
        self.timeline = Some(tl);
        self.timeline_apply();
        self.play_event("toggle");
        self.dirty = true;
    }

    pub(crate) fn close_timeline(&mut self) {
        let Some(tl) = self.timeline.take() else { return };
        if let Some(tab) = self.tabs.iter_mut().find(|t|t.id==tl.tab_id) {
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
        let Some(idx)=self.tabs.iter().position(|t|t.id==tl.tab_id) else {self.close_timeline();return;};
        let (at, mode, right) = (tl.at, tl.mode,tl.right);
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
            for (pane_right,p) in std::iter::once((false,&mut tab.left)).chain(tab.right.as_mut().map(|p|(true,p))) {
                match p {
                    Pane::Term(t) => {
                        t.replay = pane_right==right;
                        t.view_key = None;
                    }
                    Pane::Web(w) => w.still = bind.clone(),
                    _ => {}
                }
            }
        }
    }

    /// Share: one HTML file that replays the tab's cast with the real
    /// renderer, its stills beside. Returns the path.
    pub(crate) fn share_replay(&mut self, tab_index: usize) -> Result<PathBuf, String> {
        let Some(tab) = self.tabs.get(tab_index) else { return Err("no such tab".into()) };
        let id = tab.id;
        let title = tab.title();
        let right=tab.focus_right && matches!(tab.right,Some(Pane::Term(_)));
        let Some(rec) = self.recorder.as_mut() else { return Err("replay is off".into()) };
        rec.flush();
        let cast_path = rec.cast_path(stream_id(id,right)).ok_or("nothing recorded for this tab")?;
        let session = rec.dir.clone();
        let cast = read_cast(&cast_path).map_err(|e| e.to_string())?;
        let (cols, rows, events) = parse_cast(&cast);
        // The stills, inlined as data URLs.
        let mut stills=records(cols,rows,&events);
        for record in &mut stills {
            let png=record.pointer("/page/png").and_then(Value::as_str).and_then(|rel| {
                let path=session.join(rel);let canonical=path.canonicalize().ok()?;
                if !canonical.starts_with(session.canonicalize().ok()?){return None;}nus_vault::read(&canonical).ok()
            }).map(|b|b64(&b));
            record["png"]=json!(png);
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
fn share_html(title: &str, cast: &str, stills: &[Value], _theme: &nus_render::theme::Theme, signal: nus_render::Color) -> String {
    let glue = WASM_JS
        .replace("export class Shell", "class Shell")
        .replace("export function source_rev", "function source_rev")
        .replace("export { initSync, __wbg_init as default };", "const init = __wbg_init;");
    let player = PLAYER_JS
        .replace("import init, { Shell } from '../wasm/nus_vt_wasm.js';", "")
        .replace("export async function mountHeroTerminal", "async function mountHeroTerminal")
        .replace("export function", "function")
        .replace("export class", "class");
    let esc=|s:&str|s.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;");
    // Base64 data cannot terminate a script element, even when recorded output
    // contains HTML, Unicode separators, or a literal closing script tag.
    include_str!("../assets/replay/history.html")
        .replace("@@GLUE@@",&glue).replace("@@PLAYER@@",&player)
        .replace("@@CAST@@",&b64(cast.as_bytes()))
        .replace("@@RECORDS@@",&b64(serde_json::to_string(stills).unwrap_or_default().as_bytes()))
        .replace("@@WASM@@",&b64(WASM)).replace("@@SIGNAL@@",&hex(signal))
        .replace("@@TITLE@@",&esc(title))

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noisy_replay_rotates_and_keeps_recent_command_boundaries() {
        let root = tempfile::tempdir().unwrap();
        nus_vault::install_test_key(root.path()).unwrap();
        let mut rec = Recorder { dir: root.path().to_path_buf(), t0: Instant::now(), casts: HashMap::new(), stills: 0, pending_bytes: 0 };
        for i in 0..100 {
            let cast = rec.cast(1, 80, 24).unwrap();
            cast.write(&json!([i, "m", json!({"cmd":format!("command-{i}")}).to_string()]), 512);
        }
        rec.flush();
        let path = rec.cast_path(1).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() <= 512);
        assert!(std::fs::metadata(path.with_extension("previous.cast")).unwrap().len() <= 512);
        let (_, _, events) = parse_cast(&read_cast(&path).unwrap());
        assert!(events.len() > 1 && events.len() < 100);
        assert!(matches!(events.last(), Some(Ev::Mark(_, p)) if p["cmd"] == "command-99"));
        rec.retain(&Default::default());
        assert!(rec.casts.is_empty());
        let before = std::fs::metadata(&path).unwrap().len();
        rec.mark(1, 80, 24, &json!({"cmd":"reopened"}));
        rec.flush();
        assert!(std::fs::metadata(path).unwrap().len() > before, "reopening must append, not truncate");
    }

    #[test]
    fn session_budget_counts_stills_and_closes_evicted_cast_handles() {
        let root = tempfile::tempdir().unwrap();
        nus_vault::install_test_key(root.path()).unwrap();
        std::fs::create_dir(root.path().join("blobs")).unwrap();
        let mut rec = Recorder { dir: root.path().to_path_buf(), t0: Instant::now(), casts: HashMap::new(), stills: 0, pending_bytes: 0 };
        for id in 1..15 {
            rec.mark(id, 80, 24, &json!({"cmd":"test"}));
            std::fs::write(rec.still_path(id), vec![0; 500]).unwrap();
        }
        rec.enforce_budget(Some(14), 1500);
        assert!(crate::storage::files(&rec.dir,true).iter().map(|e|e.bytes).sum::<u64>() <= 1500);
        assert!(rec.cast_path(14).is_some());
        assert!(rec.casts.values().all(|c|c.path.exists()));
        rec.output(1, 80, 24, b"after eviction"); rec.flush();
        assert!(read_cast(&rec.cast_path(1).unwrap()).unwrap().contains("after eviction"));
    }

    #[test]
    fn utf8_split_across_every_byte_is_lossless() {
        let source="hello 🌙 café \u{1b}[31m赤\u{1b}[0m";
        let mut pending=Vec::new();let mut out=String::new();
        for byte in source.as_bytes(){out.push_str(&decode_chunk(&mut pending,&[*byte]));}
        assert_eq!(out,source);assert!(pending.is_empty());
        assert_eq!(decode_chunk(&mut pending,&[0xff,b'x']),"�x");
    }
    #[test]
    fn independent_pane_streams_and_bounded_sizes() {
        assert_eq!(stream_id(42,false),42);assert_ne!(stream_id(42,true),stream_id(42,false));
        let(c,r,events)=parse_cast("{\"width\":0,\"height\":999999999}\n[1,\"r\",\"0x9999999\"]");
        assert_eq!((c,r),(1,1000));assert!(matches!(events[0],Ev::Resize(_,1,1000)));
    }
    #[test]
    fn historical_output_is_searchable_and_scripts_stay_data() {
        let payload=json!({"cmd":"printf '<script>'","output":"</script><script>window.pwned=true</script> café","exit":1,"cwd":"/tmp/project","ms":250});
        let events=vec![Ev::Out(0.0,b"hello".to_vec()),Ev::Mark(1.0,payload.clone())];
        let entries=records(80,24,&events);assert_eq!(entries[0]["output"],payload["output"]);assert_eq!(entries[0]["start"],0.75);
        let html=share_html("</title><script>alert(1)</script>","</script>",&entries,&nus_render::theme::Theme::paper(),[0.8,0.1,0.2,1.0]);
        assert!(!html.contains("<script>alert(1)"));assert!(!html.contains("<script>window.pwned"));assert!(!html.contains("@@"));assert!(html.contains("session-map"));assert!(html.contains("autoplay:false,loop:false"));
        if let Some(path)=std::env::var_os("NUS_REPLAY_FIXTURE") {
            let mut cast=String::from("{\"version\":2,\"width\":80,\"height\":24}\n");let mut ev=Vec::new();
            for i in 0..18 {let output=(0..(i*3+4)).map(|n|format!("  test {:03}  {}",n,if i==6&&n==8{"FAILED: expected 200, received 503"}else{"passed"})).collect::<Vec<_>>().join("\n");let p=json!({"cmd":format!("cargo test --package workspace-{}",i),"output":output,"cwd":"~/work/nus","exit":if i==6{1}else{0},"ms":(i+1)*123});let start=i as f64*3.0;let bytes=format!("$ cargo test --package workspace-{i}\r\n{}\r\n",output.replace('\n',"\r\n"));cast.push_str(&format!("{}\n{}\n",json!([start,"o",bytes]),json!([start+2.0,"m",p.to_string()])));ev.push(Ev::Out(start,bytes.into_bytes()));ev.push(Ev::Mark(start+2.0,p));}
            let html=share_html("nus workspace · verification fixture",&cast,&records(80,24,&ev),&nus_render::theme::Theme::paper(),[0.8,0.1,0.2,1.0]);std::fs::write(path,html).unwrap();
        }
    }

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
