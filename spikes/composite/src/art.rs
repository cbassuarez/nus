//! Art: what plays behind the prompt. One Luau file per art — the four
//! that ship (the pond, memphis, space, the brain) are Luau too, so they
//! are worked examples — under profile/art/, hot-reloaded when they
//! change. A file defines `draw(c)` and gets called every frame with a
//! canvas: the pane's size, the line's box to keep clear of, the tokens,
//! the pointer, what is typed, the time, the place, the machine's
//! processes, and a few primitives — rect, circle, line, quad, poly, blob,
//! text. The script draws into a command list; the app draws the list.
//! Errors show at the foot of the pane, never crash the app.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Instant, SystemTime};

use nus_render::text::Style;
use nus_render::{Color, Instance, Rect, Scene};

use crate::app::{fade, App};

/// Where the files live.
pub fn dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("art")
}

/// Built-ins, by persisted key. Native artwork still enters through Luau.
pub const BUILTIN: [(&str, &str, &str); 5] = [
    ("pond", "the pond", include_str!("../assets/art/pond.luau")),
    ("memphis", "memphis", include_str!("../assets/art/memphis.luau")),
    ("space", "space", include_str!("../assets/art/space.luau")),
    ("sky", "the sky", include_str!("../assets/art/sky.luau")),
    ("brain", "the brain", include_str!("../assets/art/brain.luau")),
];

/// A blank to start from (ADD YOUR OWN, and the prompt the assistant gets).
pub const TEMPLATE: &str = include_str!("../assets/art/template.luau");
/// The canvas, as the assistant is told it.
pub const API: &str = include_str!("../assets/art/API.md");

#[derive(Clone, Debug)]
pub struct Info {
    /// The key: a built-in's name, or a file's stem.
    pub key: String,
    /// What the file calls itself (`-- name:`), else the key.
    pub name: String,
    /// One line it says about itself (`-- says:`).
    pub says: String,
    pub path: Option<PathBuf>,
}

/// Every art there is: the built-ins, then profile/art/*.luau by name.
pub fn list() -> Vec<Info> {
    let mut out: Vec<Info> = BUILTIN.iter().map(|(k, n, src)| Info { key: k.to_string(), name: n.to_string(), says: header(src, "says"), path: None }).collect();
    if let Ok(rd) = std::fs::read_dir(dir()) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "luau" || e == "lua")).collect();
        files.sort();
        for p in files {
            let key = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if key.is_empty() || BUILTIN.iter().any(|(k, _, _)| *k == key) {
                continue;
            }
            let src = std::fs::read_to_string(&p).unwrap_or_default();
            let name = { let n = header(&src, "name"); if n.is_empty() { key.clone() } else { n } };
            out.push(Info { key, name, says: header(&src, "says"), path: Some(p) });
        }
    }
    out
}

/// `-- name: …` style headers in the first lines.
pub fn header(src: &str, key: &str) -> String {
    src.lines().take(12).filter_map(|l| l.trim().strip_prefix("--")).map(str::trim).find_map(|l| l.strip_prefix(key).and_then(|r| r.trim_start().strip_prefix(':')).map(|v| v.trim().to_string())).unwrap_or_default()
}

/// A file name for a new art from what it is called.
pub fn slug(name: &str) -> String {
    let s: String = name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    let mut out = String::new();
    for part in s.split('-').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(part);
    }
    if out.is_empty() { "art".into() } else { out }
}

/// What the script draws, in the pane's own pixels.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum Cmd {
    Rect(Rect, Color, f32),
    Quad([[f32; 2]; 4], Color),
    /// A filled polygon, any shape, one instance.
    Poly(Points, Color),
    /// (x1, y1, x2, y2, width)
    Line(f32, f32, f32, f32, f32, Color),
    /// x, y (baseline), text, px, colour, font (0 mono · 1 serif · 2 strong), align (0 left · 1 centre · 2 right), tracked
    Text(f32, f32, String, f32, Color, u8, u8, bool),
    /// A sky over the rect: (az -1..1, sin alt, cover, wind, seed).
    Sky(Rect, f32, f32, f32, f32, [f32; 2], [f32;4]),
    /// A cached native volume: rect, conditions, stable view, command ordinal, and whether
    /// the art asked for the real sky (the app then fills in stars, planets and eclipses).
    Atmosphere(Rect, nus_render::sky::SkyParams, u64, usize, bool),
    /// Recovered Limb / Darkroom artwork; no CEF/WebGL or local star chart.
    Space(Rect, nus_render::space::SpaceParams, u64, usize),
}

/// What the canvas knows this frame.
#[derive(Clone, Default)]
pub struct Env {
    /// Identity belongs to the embedding pane/card, never its pixel geometry.
    pub view_id: u64,
    pub w: f32,
    pub h: f32,
    /// The line's box (x, y, w, h) and how far the rows beneath it reach.
    pub line: [f32; 4],
    pub rows: f32,
    pub pointer: Option<(f32, f32)>,
    pub typed: String,
    pub taps: Vec<(f32, f32)>,
    /// Individual existing artwork pieces positioned by an embedding page.
    pub pieces: Vec<[f32; 4]>,
    pub face: String,
    pub paper: Color,
    pub ink: Color,
    pub signal: Color,
    pub signals: Option<[Color; 6]>,
    pub dim: Color,
    pub tint: Color,
    pub place: Option<(f32, f32)>,
    /// The sky's time in unix ms when the app keeps one (the clock a viewer may have
    /// turned); otherwise `c:now()` reads the machine's.
    pub now_ms: Option<f64>,
    pub weather: Option<crate::weather::WeatherSnapshot>,
    pub procs: Option<crate::procs::Shared>,
    /// Logical px per… the pane's scale, so an art can size hairlines.
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    Theme,
    Light,
    Dark,
}

impl Backdrop {
    pub fn foreground(self, mode: nus_render::Mode, ink: Color, paper: Color) -> Color {
        match (self, mode) {
            (Self::Dark, nus_render::Mode::Paper) | (Self::Light, nus_render::Mode::Ink) => paper,
            _ => ink,
        }
    }

    /// Sky has its own exposure. A saturated chrome surface is not necessarily
    /// dark enough for daylight, or light enough for the night cloud field.
    pub fn sky_foreground(self, mode: nus_render::Mode, ink: Color, paper: Color) -> Color {
        let color = self.foreground(mode, ink, paper);
        let linear = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        let light = linear(color[0]) * 0.2126 + linear(color[1]) * 0.7152 + linear(color[2]) * 0.0722;
        match self {
            Self::Light if light > 0.04 => [0.035, 0.045, 0.06, 1.0],
            Self::Dark if light < 0.60 => [0.94, 0.95, 0.97, 1.0],
            _ => color,
        }
    }
}

// Scratch polygons return here when commands are drawn or discarded, including
// script errors. Bound retained storage independently of the drawing budget.
#[derive(Default)]
struct PointPool { buffers: Vec<Vec<[f32; 2]>>, bytes: usize }
type SharedPoints = Rc<RefCell<PointPool>>;
pub struct Points { data: Vec<[f32; 2]>, pool: SharedPoints }
#[cfg(test)]
impl std::fmt::Debug for Points {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.data.fmt(f) }
}
#[cfg(test)]
impl PartialEq for Points {
    fn eq(&self, other: &Self) -> bool { self.data == other.data }
}
impl Points {
    fn new(pool: &SharedPoints, capacity: usize) -> Self {
        let mut p = pool.borrow_mut();
        let mut data = p.buffers.iter().position(|v| v.capacity() >= capacity)
            .map(|i| p.buffers.swap_remove(i)).unwrap_or_default();
        p.bytes -= data.capacity() * std::mem::size_of::<[f32; 2]>();
        data.reserve(capacity);
        Self { data, pool: pool.clone() }
    }
}
impl std::ops::Deref for Points {
    type Target = Vec<[f32; 2]>;
    fn deref(&self) -> &Self::Target { &self.data }
}
impl std::ops::DerefMut for Points {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.data }
}
impl Drop for Points {
    fn drop(&mut self) {
        let bytes = self.data.capacity() * std::mem::size_of::<[f32; 2]>();
        let mut p = self.pool.borrow_mut();
        if bytes > 0 && p.buffers.len() < 128 && p.bytes + bytes <= 256 * 1024 {
            self.data.clear();
            p.bytes += bytes;
            p.buffers.push(std::mem::take(&mut self.data));
        }
    }
}

struct State {
    env: Env,
    cmds: Vec<Cmd>,
    command_bytes: usize,
    points: SharedPoints,
    colors: std::collections::HashMap<usize, (mlua::String, Color)>,
    signals: Option<([Color; 6], mlua::Table)>,
    t: f32,
    dt: f32,
    /// An artwork can request readable text independently of the app theme.
    backdrop: Backdrop,
}

impl State {
    fn color(&mut self, value: &mlua::Value, alpha: Option<f32>) -> Option<Color> {
        let mlua::Value::String(text) = value else { return color_of(value, alpha) };
        // Luau strings are immutable. Keep the string reference alongside its
        // identity so GC cannot recycle the pointer while this entry is cached.
        // Tables stay live: scripts may mutate their channels between commands.
        let key = text.to_pointer() as usize;
        let mut color = if let Some((_, color)) = self.colors.get(&key) { *color } else {
            let color = color_of(value, None)?;
            if text.as_bytes().len() <= 64 {
                if self.colors.len() == 64 { self.colors.clear(); }
                self.colors.insert(key, (text.clone(), color));
            }
            color
        };
        if let Some(alpha) = alpha { color[3] *= alpha.clamp(0.0, 1.0); }
        Some(color)
    }

    fn push(&mut self, cmd: Cmd) -> mlua::Result<()> {
        let bytes = std::mem::size_of::<Cmd>() + match &cmd {
            Cmd::Poly(p, _) => p.len() * std::mem::size_of::<[f32; 2]>(),
            Cmd::Text(_, _, s, ..) => s.len(),
            _ => 0,
        };
        if self.cmds.len() >= 8192 || bytes > (4 * 1024 * 1024usize).saturating_sub(self.command_bytes) {
            return Err(mlua::Error::runtime("artwork drawing budget exceeded"));
        }
        self.command_bytes += bytes;
        self.cmds.push(cmd);
        Ok(())
    }
}

fn execution_budget(lua: &mlua::Lua, millis: u64) {
    let deadline = Instant::now() + std::time::Duration::from_millis(millis);
    lua.set_interrupt(move |_| {
        if Instant::now() > deadline { Err(mlua::Error::runtime("artwork execution budget exceeded")) }
        else { Ok(mlua::VmState::Continue) }
    });
}

#[derive(Clone)]
struct Canvas(Rc<RefCell<State>>);

fn color_of(v: &mlua::Value, alpha: Option<f32>) -> Option<Color> {
    let mut c = match v {
        mlua::Value::String(s) => {
            let s = s.to_str().ok()?;
            let s = s.trim().trim_start_matches('#');
            match s.len() {
                6 => crate::surface::parse_hex(s)?,
                8 => {
                    let v = u32::from_str_radix(s, 16).ok()?;
                    [((v >> 24) & 255) as f32 / 255.0, ((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
                }
                _ => return None,
            }
        }
        mlua::Value::Table(t) => {
            let g = |k: &str, i: i64| -> f32 { t.get::<f32>(k).ok().or_else(|| t.get::<f32>(i).ok()).unwrap_or(0.0) };
            let a = t.get::<f32>("a").ok().or_else(|| t.get::<f32>(4).ok()).unwrap_or(1.0);
            [g("r", 1), g("g", 2), g("b", 3), a]
        }
        _ => return None,
    };
    if let Some(a) = alpha {
        c[3] *= a.clamp(0.0, 1.0);
    }
    Some(c)
}

fn table_color(lua: &mlua::Lua, c: Color) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    t.set("r", c[0])?;
    t.set("g", c[1])?;
    t.set("b", c[2])?;
    t.set("a", c[3])?;
    Ok(t)
}

fn points_of(t: &mlua::Table, pool: &SharedPoints) -> mlua::Result<Points> {
    let mut out = Points::new(pool, t.raw_len().min(2048));
    for (i, v) in t.sequence_values::<mlua::Value>().flatten().enumerate() {
        if i >= 2048 { return Err(mlua::Error::runtime("artwork point budget exceeded")); }
        match v {
            mlua::Value::Table(p) => {
                let x = p.get::<f32>(1).ok().or_else(|| p.get::<f32>("x").ok());
                let y = p.get::<f32>(2).ok().or_else(|| p.get::<f32>("y").ok());
                if let (Some(x), Some(y)) = (x, y) {
                    out.push([x, y]);
                }
            }
            mlua::Value::Number(n) => out.push([n as f32, f32::NAN]),
            mlua::Value::Integer(n) => out.push([n as f32, f32::NAN]),
            _ => {}
        }
    }
    // A flat list of numbers: pairs.
    if out.iter().all(|p| p[1].is_nan()) && out.len() >= 4 {
        let pairs = out.len() / 2;
        for i in 0..pairs { out[i] = [out[i * 2][0], out[i * 2 + 1][0]]; }
        out.truncate(pairs);
    }
    Ok(out)
}

/// A smooth closed curve through the points (quadratics through the
/// midpoints), as a polygon.
fn smooth_closed(pts: &[[f32; 2]], per: usize, pool: &SharedPoints) -> Points {
    let n = pts.len();
    let mut out = Points::new(pool, n * per);
    if n < 3 { out.extend_from_slice(pts); return out; }
    for i in 0..n {
        let p0 = pts[(i + n - 1) % n];
        let p1 = pts[i];
        let p2 = pts[(i + 1) % n];
        let a = [(p0[0] + p1[0]) / 2.0, (p0[1] + p1[1]) / 2.0];
        let b = [(p1[0] + p2[0]) / 2.0, (p1[1] + p2[1]) / 2.0];
        for k in 0..per {
            let t = k as f32 / per as f32;
            let u = 1.0 - t;
            out.push([u * u * a[0] + 2.0 * u * t * p1[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * p1[1] + t * t * b[1]]);
        }
    }
    out
}

/// A smooth open curve through the points, as a polyline.
fn smooth_open(pts: &[[f32; 2]], per: usize, pool: &SharedPoints) -> Points {
    let n = pts.len();
    let mut out = Points::new(pool, n * per);
    if n < 3 { out.extend_from_slice(pts); return out; }
    out.push(pts[0]);
    for i in 1..n - 1 {
        let p1 = pts[i];
        let a = if i == 1 { pts[0] } else { [(pts[i - 1][0] + p1[0]) / 2.0, (pts[i - 1][1] + p1[1]) / 2.0] };
        let b = if i == n - 2 { pts[n - 1] } else { [(p1[0] + pts[i + 1][0]) / 2.0, (p1[1] + pts[i + 1][1]) / 2.0] };
        for k in 1..=per {
            let t = k as f32 / per as f32;
            let u = 1.0 - t;
            out.push([u * u * a[0] + 2.0 * u * t * p1[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * p1[1] + t * t * b[1]]);
        }
    }
    out
}


impl mlua::UserData for Canvas {
    fn add_fields<F: mlua::UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("w", |_, c| Ok(c.0.borrow().env.w));
        f.add_field_method_get("h", |_, c| Ok(c.0.borrow().env.h));
        f.add_field_method_get("t", |_, c| Ok(c.0.borrow().t));
        f.add_field_method_get("dt", |_, c| Ok(c.0.borrow().dt));
        f.add_field_method_get("face", |_, c| Ok(c.0.borrow().env.face.clone()));
        f.add_field_method_get("typed", |_, c| Ok(c.0.borrow().env.typed.clone()));
        f.add_field_method_get("rows", |_, c| Ok(c.0.borrow().env.rows));
        f.add_field_method_get("scale", |_, c| Ok(c.0.borrow().env.scale));
        f.add_field_method_get("paper", |_, c| Ok(crate::surface::hex(c.0.borrow().env.paper)));
        f.add_field_method_get("ink", |_, c| Ok(crate::surface::hex(c.0.borrow().env.ink)));
        f.add_field_method_get("signal", |_, c| Ok(crate::surface::hex(c.0.borrow().env.signal)));
        f.add_field_method_get("dim", |_, c| Ok(crate::surface::hex(c.0.borrow().env.dim)));
        f.add_field_method_get("tint", |_, c| Ok(crate::surface::hex(c.0.borrow().env.tint)));
        f.add_field_method_get("prompt", |lua, c| {
            let l = c.0.borrow().env.line;
            let t = lua.create_table()?;
            t.set("x", l[0])?;
            t.set("y", l[1])?;
            t.set("w", l[2])?;
            t.set("h", l[3])?;
            Ok(t)
        });
        f.add_field_method_get("pointer", |lua, c| {
            let p = c.0.borrow().env.pointer;
            match p {
                Some((x, y)) => {
                    let t = lua.create_table()?;
                    t.set("x", x)?;
                    t.set("y", y)?;
                    Ok(mlua::Value::Table(t))
                }
                None => Ok(mlua::Value::Nil),
            }
        });
        f.add_field_method_get("signals", |lua, c| {
            let mut state = c.0.borrow_mut();
            let colors = state.env.signals.unwrap_or(nus_render::theme::signal::ALL);
            if let Some((previous, table)) = &state.signals {
                if *previous == colors { return Ok(table.clone()); }
            }
            let table = match &state.signals { Some((_, t)) => t.clone(), None => lua.create_table()? };
            for (key, color) in ["red", "blue", "gold", "green", "violet", "teal"].into_iter().zip(colors) {
                table.set(key, crate::surface::hex(color))?;
            }
            state.signals = Some((colors, table.clone()));
            Ok(table)
        });
    }

    fn add_methods<M: mlua::UserDataMethods<Self>>(m: &mut M) {
        m.add_method("rect", |_, c, (x, y, w, h, col, a, r): (f32, f32, f32, f32, mlua::Value, Option<f32>, Option<f32>)| {
            let color = c.0.borrow_mut().color(&col, a);
            if let Some(col) = color {
                c.0.borrow_mut().push(Cmd::Rect(Rect::new(x, y, w, h), col, r.unwrap_or(0.0)))?;
            }
            Ok(())
        });
        m.add_method("circle", |_, c, (cx, cy, r, col, a): (f32, f32, f32, mlua::Value, Option<f32>)| {
            let color = c.0.borrow_mut().color(&col, a);
            if let Some(col) = color {
                c.0.borrow_mut().push(Cmd::Rect(Rect::new(cx - r, cy - r, r * 2.0, r * 2.0), col, r))?;
            }
            Ok(())
        });
        m.add_method("oval", |_, c, (cx, cy, rx, ry, col, a): (f32, f32, f32, f32, mlua::Value, Option<f32>)| {
            let color = c.0.borrow_mut().color(&col, a);
            if let Some(col) = color {
                let n = ((rx.max(ry) * 0.8) as usize).clamp(24, 96);
                let mut pts = Points::new(&c.0.borrow().points, n);
                pts.extend((0..n).map(|i| { let th = i as f32 / n as f32 * std::f32::consts::TAU; [cx + rx * th.cos(), cy + ry * th.sin()] }));
                c.0.borrow_mut().push(Cmd::Poly(pts, col))?;
            }
            Ok(())
        });
        m.add_method("line", |_, c, (x1, y1, x2, y2, w, col, a): (f32, f32, f32, f32, f32, mlua::Value, Option<f32>)| {
            let color = c.0.borrow_mut().color(&col, a);
            if let Some(col) = color {
                c.0.borrow_mut().push(Cmd::Line(x1, y1, x2, y2, w, col))?;
            }
            Ok(())
        });
        m.add_method("quad", |_, c, (pts, col, a): (mlua::Table, mlua::Value, Option<f32>)| {
            let p = points_of(&pts, &c.0.borrow().points)?;
            let color = c.0.borrow_mut().color(&col, a);
            if let (Some(col), true) = (color, p.len() >= 4) {
                c.0.borrow_mut().push(Cmd::Quad([p[0], p[1], p[2], p[3]], col))?;
            }
            Ok(())
        });
        m.add_method("poly", |_, c, (pts, col, a): (mlua::Table, mlua::Value, Option<f32>)| {
            let p = points_of(&pts, &c.0.borrow().points)?;
            let color = c.0.borrow_mut().color(&col, a);
            if let (Some(col), true) = (color, p.len() >= 3) {
                c.0.borrow_mut().push(Cmd::Poly(p, col))?;
            }
            Ok(())
        });
        m.add_method("blob", |_, c, (pts, col, a): (mlua::Table, mlua::Value, Option<f32>)| {
            let p = smooth_closed(&points_of(&pts, &c.0.borrow().points)?, 5, &c.0.borrow().points);
            let color = c.0.borrow_mut().color(&col, a);
            if let (Some(col), true) = (color, p.len() >= 3) {
                c.0.borrow_mut().push(Cmd::Poly(p, col))?;
            }
            Ok(())
        });
        m.add_method("curve", |_, c, (pts, w, col, a): (mlua::Table, f32, mlua::Value, Option<f32>)| {
            let p = smooth_open(&points_of(&pts, &c.0.borrow().points)?, 4, &c.0.borrow().points);
            let color = c.0.borrow_mut().color(&col, a);
            if let Some(col) = color {
                let mut s = c.0.borrow_mut();
                for pair in p.windows(2) {
                    s.push(Cmd::Line(pair[0][0], pair[0][1], pair[1][0], pair[1][1], w, col))?;
                }
            }
            Ok(())
        });
        m.add_method("text", |_, c, (x, y, text, px, col, a, opts): (f32, f32, String, f32, mlua::Value, Option<f32>, Option<mlua::Table>)| {
            let color = c.0.borrow_mut().color(&col, a);
            if let Some(col) = color {
                let (mut font, mut align, mut tracked) = (0u8, 0u8, false);
                if let Some(o) = opts {
                    font = match o.get::<String>("font").unwrap_or_default().as_str() { "serif" => 1, "strong" => 2, _ => 0 };
                    align = match o.get::<String>("align").unwrap_or_default().as_str() { "center" | "centre" => 1, "right" => 2, _ => 0 };
                    tracked = o.get::<bool>("caps").unwrap_or(false);
                }
                c.0.borrow_mut().push(Cmd::Text(x, y, text, px, col, font, align, tracked))?;
            }
            Ok(())
        });
        // A near-enough width for laying text out: mono is 0.6 em a glyph.
        m.add_method("measure", |_, _, (text, px, font): (String, f32, Option<String>)| {
            let per = if font.as_deref() == Some("serif") { 0.5 } else { 0.6 };
            Ok(text.chars().count() as f32 * px * per)
        });
        m.add_method("mix", |_, _, (a, b, t): (mlua::Value, mlua::Value, f32)| {
            match (color_of(&a, None), color_of(&b, None)) {
                (Some(a), Some(b)) => Ok(crate::surface::hex(crate::surface::mix(a, b, t))),
                _ => Ok(String::new()),
            }
        });
        m.add_method("rgba", |lua, _, (r, g, b, a): (f32, f32, f32, Option<f32>)| table_color(lua, [r, g, b, a.unwrap_or(1.0)]));
        // The sky: a table of az (-1 east … 1 west), alt (sin of the sun's
        // altitude), cover (0..1), wind, seed {x, y}; x, y, w, h default to the pane.
        m.add_method("sky", |_, c, o: mlua::Table| {
            let mut s = c.0.borrow_mut();
            let (w, h) = (s.env.w, s.env.h);
            let g = |k: &str, d: f32| o.get::<f32>(k).unwrap_or(d);
            let r = Rect::new(g("x", 0.0), g("y", 0.0), g("w", w), g("h", h));
            let seed = o.get::<mlua::Table>("seed").ok().map(|t| [t.get::<f32>(1).unwrap_or(0.0), t.get::<f32>(2).unwrap_or(0.0)]).unwrap_or([3.7, 1.3]);
            s.push(Cmd::Sky(r, g("az", 0.0).clamp(-1.0, 1.0), g("alt", 0.5).clamp(-1.0, 1.0), g("cover", 0.4).clamp(0.0, 1.0), g("wind", 1.0), seed, [g("moon_az",0.0),g("moon_alt",-1.0),g("moon_light",0.0),g("moon_waxing",1.0)]))?;
            Ok(())
        });
        // New atmosphere primitive; keep c:sky and its packed legacy shader
        // intact for saved user artwork. All celestial vectors are East/Up/North.
        m.add_method("atmosphere", |_, c, o: mlua::Table| {
            let mut s = c.0.borrow_mut();
            let g = |key: &str, default: f32| o.get::<f32>(key).ok().filter(|x| x.is_finite()).unwrap_or(default);
            let r = Rect::new(g("x", 0.0), g("y", 0.0), g("w", s.env.w).max(1.0), g("h", s.env.h).max(1.0));
            let vector = |key: &str, defaults: [f32; 3]| {
                let table = o.get::<mlua::Table>(key).ok();
                std::array::from_fn(|i| table.as_ref().and_then(|t| t.get::<f32>(i+1).ok()).filter(|x| x.is_finite()).unwrap_or(defaults[i]))
            };
            let wind = |key: &str, defaults: [f32; 2]| {
                let table = o.get::<mlua::Table>(key).ok();
                std::array::from_fn(|i| table.as_ref().and_then(|t| t.get::<f32>(i+1).ok()).filter(|x| x.is_finite()).unwrap_or(defaults[i]).clamp(-150.0,150.0))
            };
            let d = nus_render::sky::SkyParams::default();
            let p = nus_render::sky::SkyParams {
                sun_direction: vector("sun", d.sun_direction), moon_direction: vector("moon", d.moon_direction),
                moon_illumination: g("moon_light", d.moon_illumination).clamp(0.0, 1.0),
                moon_waxing: o.get::<bool>("moon_waxing").unwrap_or(true),
                low_cover: g("low", d.low_cover).clamp(0.0, 1.0),
                mid_cover: g("middle", d.mid_cover).clamp(0.0, 1.0),
                high_cover: g("high", d.high_cover).clamp(0.0, 1.0),
                stratus: g("stratus", d.stratus).clamp(0.0, 1.0),
                precipitation_mm_h: g("precipitation", 0.0).clamp(0.0, 100.0),
                cloud_base_km: g("base", d.cloud_base_km).clamp(0.2, 8.0),
                haze: g("haze", d.haze).clamp(0.0, 1.0),
                wind_low: wind("wind_low", d.wind_low), wind_mid: wind("wind_middle", d.wind_mid), wind_high: wind("wind_high", d.wind_high),
                seed: g("seed", 42.0).clamp(0.0, 1_000_000.0) as u32,
                view_azimuth: g("bearing", d.view_azimuth),
                view_elevation: g("elevation", d.view_elevation).clamp(0.1, 1.4),
                fov_y: g("fov", d.fov_y).clamp(0.3, 1.5),
                reading_rect: [(s.env.line[0]-r.x)/r.w, (s.env.line[1]-r.y)/r.h, s.env.line[2]/r.w, (s.env.line[3]+s.env.rows)/r.h],
                reading_strength: g("prompt_light", 0.14).clamp(0.0, 0.35),
                ..d
            };
            let (view_id, ordinal) = (s.env.view_id, s.cmds.len());
            let astro = o.get::<bool>("astro").unwrap_or(false) && s.env.place.is_some();
            s.push(Cmd::Atmosphere(r, p, view_id, ordinal, astro))?;
            Ok(())
        });
        // The recovered Space scene. The native Home motion controller supplies
        // camera time; a preview uses the exact settled opening pose.
        m.add_method("orbital", |_, c, o: mlua::Table| {
            let mut s = c.0.borrow_mut();
            let g = |key: &str, default: f32| o.get::<f32>(key).ok().filter(|x| x.is_finite()).unwrap_or(default);
            let r = Rect::new(g("x",0.0),g("y",0.0),g("w",s.env.w).max(1.0),g("h",s.env.h).max(1.0));
            let d = nus_render::space::SpaceParams::default();
            let p = nus_render::space::SpaceParams {
                phase:g("phase",d.phase),blend:g("blend",d.blend),time:g("time",0.0),
                exposure:g("exposure",d.exposure),seed:g("seed",42.0).clamp(0.0,1_000_000.0) as u32,
                lines:o.get::<bool>("lines").unwrap_or(true),
                reading_rect:[(s.env.line[0]-r.x)/r.w,(s.env.line[1]-r.y)/r.h,s.env.line[2]/r.w,(s.env.line[3]+s.env.rows)/r.h],
            }.clean();
            let (view,ordinal) = (s.env.view_id,s.cmds.len());
            // Prevent user scripts from issuing a large number of expensive
            // orbital passes in a single canvas. Normal Space needs exactly one.
            if s.cmds.iter().filter(|c| matches!(c,Cmd::Space(..))).count() >= 2 {
                return Err(mlua::Error::runtime("at most two orbital views per art canvas"));
            }
            s.push(Cmd::Space(r,p,view,ordinal))?;
            Ok(())
        });
        // Explicit artwork brightness wins over the application theme.
        // "paper" remains the theme-following default for existing scripts.
        m.add_method("backdrop", |_, c, which: String| {
            c.0.borrow_mut().backdrop = match which.as_str() { "dark" => Backdrop::Dark, "light" => Backdrop::Light, _ => Backdrop::Theme };
            Ok(())
        });
        m.add_method("celestial", |lua, _, (ms,lat,lon):(f64,f64,f64)| {
            let sky=crate::celestial::sky(ms,lat,lon);let t=lua.create_table()?;
            for (name,b) in [("sun",sky.sun),("moon",sky.moon)] {
                let v=lua.create_table()?;v.set("ra",b.ra)?;v.set("dec",b.dec)?;v.set("alt",b.alt)?;v.set("az",b.az)?;t.set(name,v)?;
            }
            t.set("illumination",sky.illumination)?;t.set("waxing",sky.waxing)?;Ok(t)
        });
        // The clock, in unix milliseconds; NUS_CLOCK pins it (for photographs of a night sky at noon).
        m.add_method("now", |_, c, ()| {
            if let Some(ms) = c.0.borrow().env.now_ms {
                return Ok(ms);
            }
            if let Some(ms) = std::env::var("NUS_CLOCK").ok().and_then(|v| v.parse::<f64>().ok()) {
                return Ok(ms);
            }
            Ok(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0))
        });
        m.add_method("place", |lua, c, ()| {
            let Some((lat, lon)) = c.0.borrow().env.place else { return Ok(None); };
            let t = lua.create_table()?;
            t.set("lat", lat)?;
            t.set("lon", lon)?;
            Ok(Some(t))
        });
        m.add_method("weather", |lua, c, ()| {
            let Some(w) = c.0.borrow().env.weather else { return Ok(None); };
            let t = lua.create_table()?;
            for (key, value) in [("cover", w.cloud_cover), ("low", w.cloud_low), ("middle", w.cloud_mid),
                ("high", w.cloud_high), ("humidity", w.humidity), ("fog", w.fog),
                ("visibility", w.visibility_m), ("precipitation", w.precipitation_mm)] { t.set(key, value)?; }
            // Surface wind drives modeled low-cloud drift; it is never
            // presented as a measured upper-air wind. Missing data stays nil.
            for (key, value) in [("wind_low", w.wind_10m), ("wind_middle", w.wind_500hpa), ("wind_high", w.wind_250hpa)] {
                if value.speed_mps.is_some() && value.direction_deg.is_some() {
                    let wind = lua.create_table()?;
                    wind.set("speed", value.speed_mps)?; wind.set("direction", value.direction_deg)?;
                    wind.set("height", value.height_m)?; t.set(key, wind)?;
                }
            }
            t.set("source", crate::weather::WeatherSnapshot::SOURCE)?;
            t.set("valid_at", w.source_time)?; t.set("fetched_at", w.fetched_at)?;
            t.set("stale", w.stale)?; t.set("offline", w.offline)?;
            t.set("code", w.weather_code.as_ref().map(|v| v.as_str()))?;
            Ok(Some(t))
        });
        m.add_method("pieces", |lua, c, ()| {
            let t=lua.create_table()?;
            for (i,p) in c.0.borrow().env.pieces.iter().enumerate() {
                let item=lua.create_table()?;
                item.set("index",p[0])?;item.set("x",p[1])?;item.set("y",p[2])?;item.set("scale",p[3])?;
                t.set(i+1,item)?;
            }
            Ok(t)
        });
        m.add_method("taps", |lua, c, ()| {
            let taps = std::mem::take(&mut c.0.borrow_mut().env.taps);
            let t = lua.create_table()?;
            for (i, (x, y)) in taps.iter().enumerate() {
                let p = lua.create_table()?;
                p.set("x", *x)?;
                p.set("y", *y)?;
                t.set(i + 1, p)?;
            }
            Ok(t)
        });
        m.add_method("processes", |lua, c, ()| {
            let t = lua.create_table()?;
            let shared = c.0.borrow().env.procs.clone();
            let Some(shared) = shared else { return Ok(t) };
            let s = shared.lock().map(|s| s.clone()).unwrap_or_default();
            let list = lua.create_table()?;
            for (i, p) in s.procs.iter().enumerate() {
                let row = lua.create_table()?;
                row.set("pid", p.pid)?;
                row.set("ppid", p.ppid)?;
                row.set("name", p.name.clone())?;
                row.set("cpu", p.cpu)?;
                row.set("mem", p.mem)?;
                row.set("threads", p.threads)?;
                list.set(i + 1, row)?;
            }
            t.set("list", list)?;
            t.set("ctx", s.ctx_per_s)?;
            t.set("syscalls", s.syscalls_per_s)?;
            t.set("threads", s.threads)?;
            t.set("handles", s.handles)?;
            t.set("ready", s.taken.is_some())?;
            Ok(t)
        });
    }
}

pub struct Art {
    pub key: String,
    pub name: String,
    pub path: Option<PathBuf>,
    mtime: Option<SystemTime>,
    checked: Instant,
    lua: mlua::Lua,
    state: Rc<RefCell<State>>,
    /// The last error, if the script broke; it draws at the foot.
    pub status: Option<String>,
    /// Set by the last frame: text contrast for this artwork.
    pub backdrop: Backdrop,
    started: Instant,
    last: Instant,
    /// Keep the koi and the stars where they were across a reload.
    pub reloads: u32,
}

/// Recognize only the exact retired stock file. Edited user artwork is kept.
/// No file is deleted: an obsolete stock override remains available on disk.
fn retired_space(source: &str) -> bool {
    use sha2::{Digest, Sha256};
    const RETIRED:[u8;32]=[0x9c,0x0f,0x3e,0x51,0x20,0x9b,0x7d,0x69,0x8f,0x67,0xc7,0xec,0x63,0x86,0x14,0x28,0xc7,0x91,0x8c,0x5c,0xe9,0x31,0x28,0x19,0x80,0x69,0x41,0x66,0x7d,0x66,0x0f,0x95];
    let digest:[u8;32]=Sha256::digest(source.as_bytes()).into();
    digest==RETIRED
}

impl Art {
    /// By key: a built-in, or profile/art/<key>.luau.
    pub fn open(key: &str) -> Art {
        let path = dir().join(format!("{key}.luau"));
        let (name, src, path) = match BUILTIN.iter().find(|(k, _, _)| *k == key) {
            Some((_, n, src)) if !path.is_file() => (n.to_string(), src.to_string(), None),
            _ => {
                let src = std::fs::read_to_string(&path).unwrap_or_default();
                let n = header(&src, "name");
                (if n.is_empty() { key.to_string() } else { n }, src, Some(path))
            }
        };
        let mut art = Art {
            key: key.to_string(),
            name,
            mtime: path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()),
            path,
            checked: crate::clock::now(),
            lua: mlua::Lua::new(),
            state: Rc::new(RefCell::new(State { env: Env::default(), cmds: Vec::new(), command_bytes: 0, points: Default::default(), colors: Default::default(), signals: None, t: 0.0, dt: 0.0, backdrop: Backdrop::Theme })),
            backdrop: Backdrop::Theme,
            status: None,
            started: crate::clock::now(),
            last: crate::clock::now(),
            reloads: 0,
        };
        art.load(&src);
        art
    }

    fn load(&mut self, src: &str) {
        let src = if self.key=="space" && retired_space(src) {
            tracing::info!("Using redesigned Space; recognized retired stock override preserved on disk");
            include_str!("../assets/art/space.luau")
        } else {src};
        // String identities are local to one VM; release them while it is alive.
        self.state.borrow_mut().colors.clear();
        self.state.borrow_mut().signals = None;
        let lua = mlua::Lua::new();
        lua.sandbox(true).ok();
        self.status = None;
        if let Err(e) = lua.set_memory_limit(32 * 1024 * 1024) {
            self.status = Some(short_error(&e));
            self.lua = lua;
            return;
        }
        execution_budget(&lua, 250);
        match lua.load(src).set_name(&self.key).exec() {
            Ok(()) => {}
            Err(e) => self.status = Some(short_error(&e)),
        }
        self.lua = lua;
    }

    /// The file changed: load it again, keeping the clock.
    pub fn tend(&mut self) {
        if crate::clock::since(self.checked).as_millis() < 700 {
            return;
        }
        self.checked = crate::clock::now();
        let Some(p) = self.path.clone() else { return };
        let m = std::fs::metadata(&p).ok().and_then(|m| m.modified().ok());
        if m != self.mtime {
            self.mtime = m;
            let src = std::fs::read_to_string(&p).unwrap_or_default();
            let n = header(&src, "name");
            if !n.is_empty() {
                self.name = n;
            }
            self.load(&src);
            self.reloads += 1;
        }
    }

    /// One frame: the script's `draw(c)`, then what it drew.
    pub fn frame(&mut self, env: Env) -> Vec<Cmd> {
        let time = crate::clock::since(self.started).as_secs_f32();
        self.frame_at(env, time)
    }

    /// The renderer drained these commands. Retain their backing allocation
    /// for the next frame instead of growing a fresh command list each time.
    pub fn recycle_commands(&mut self, mut commands: Vec<Cmd>) {
        commands.clear();
        self.state.borrow_mut().cmds = commands;
    }

    pub(crate) fn trim_scratch(&mut self) {
        if crate::clock::since(self.last).as_secs() >= 10 {
            self.reclaim_scratch();
        }
    }

    pub(crate) fn reclaim_scratch(&mut self) {
        let mut state = self.state.borrow_mut();
        state.cmds = Vec::new();
        state.colors.clear();
        *state.points.borrow_mut() = PointPool::default();
    }

    /// Sample existing artwork at a fixed time for previews and reduced motion.
    pub fn frame_at(&mut self, env: Env, time: f32) -> Vec<Cmd> {
        let _timing = crate::perf::scope("art_script");
        let now = crate::clock::now();
        let dt = now.duration_since(self.last).as_secs_f32().clamp(1.0 / 240.0, 0.05);
        self.last = now;
        {
            let mut s = self.state.borrow_mut();
            if let Some((previous, table)) = &mut s.signals {
                let colors = env.signals.unwrap_or(nus_render::theme::signal::ALL);
                if *previous != colors {
                    for (key, color) in ["red", "blue", "gold", "green", "violet", "teal"].into_iter().zip(colors) {
                        let _ = table.set(key, crate::surface::hex(color));
                    }
                    *previous = colors;
                }
            }
            s.env = env;
            s.cmds.clear();
            s.command_bytes = 0;
            s.backdrop = Backdrop::Theme;
            s.t = time;
            s.dt = dt;
        }
        if self.status.is_some() {
            return Vec::new();
        }
        execution_budget(&self.lua, 100);
        let canvas = Canvas(self.state.clone());
        let g = self.lua.globals();
        let result: mlua::Result<()> = match g.get::<mlua::Function>("draw") {
            Ok(f) => f.call::<()>(canvas),
            Err(_) => Err(mlua::Error::runtime("no draw(c) in this art")),
        };
        if let Err(e) = result {
            self.status = Some(short_error(&e));
        }
        self.backdrop = self.state.borrow().backdrop;
        std::mem::take(&mut self.state.borrow_mut().cmds)
    }
}

fn short_error(e: &mlua::Error) -> String {
    let s = e.to_string();
    let first = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let first = first.strip_prefix("runtime error: ").unwrap_or(first);
    crate::app::fit_cmd(first, 110)
}

impl App {
    /// Draw a frame's commands into `r`, clipped to it.
    pub(crate) fn draw_art_cmds(&mut self, scene: &mut Scene, r: Rect, cmds: Vec<Cmd>) -> Vec<Cmd> {
        self.draw_art_cmds_scaled(scene, r, cmds, 1.0)
    }

    /// The same, the canvas shrunk by `sc` into `r`: a card's preview of
    /// an art that ran at the pane's size. Type too small to read is
    /// greeked — a bar where the words would be.
    pub(crate) fn draw_art_cmds_scaled(&mut self, scene: &mut Scene, r: Rect, mut cmds: Vec<Cmd>, sc: f32) -> Vec<Cmd> {
        let _timing = crate::perf::scope("art_commands");
        // Clip to the card within whatever clips the page, and give the page
        // its clip back after: a bare layer(None) here let every card and row
        // drawn afterwards spill over the settings header as the page scrolled.
        let outer = scene.clip();
        scene.layer(Some(outer.map_or(r, |o| r.intersect(&o))));
        let (ox, oy) = (r.x, r.y);
        let at = |x: f32, y: f32| -> [f32; 2] { [x * sc + ox, y * sc + oy] };
        for cmd in cmds.drain(..) {
            match cmd {
                Cmd::Rect(rr, c, radius) => {
                    let rr = Rect::new(rr.x * sc + ox, rr.y * sc + oy, rr.w * sc, rr.h * sc);
                    if radius > 0.0 {
                        scene.push(Instance::rounded(rr, (radius * sc).min(rr.w / 2.0).min(rr.h / 2.0), c));
                    } else {
                        scene.rect(rr, c);
                    }
                }
                Cmd::Quad(p, c) => {
                    let p = p.map(|q| at(q[0], q[1]));
                    scene.poly(&p, c);
                }
                Cmd::Poly(mut p, c) => {
                    for q in p.iter_mut() { *q = at(q[0], q[1]); }
                    scene.poly(&p, c);
                }
                Cmd::Line(x1, y1, x2, y2, w, c) => {
                    let (dx, dy) = (x2 - x1, y2 - y1);
                    let l = (dx * dx + dy * dy).sqrt().max(1e-3);
                    let w = (w * sc).max(0.6);
                    let (nx, ny) = (-dy / l * w / 2.0, dx / l * w / 2.0);
                    let a = at(x1, y1);
                    let b = at(x2, y2);
                    scene.push(Instance::quad([[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]], c));
                }
                Cmd::Sky(rr, az, alt, cover, wind, seed, moon) => {
                    let rr = Rect::new(rr.x * sc + ox, rr.y * sc + oy, rr.w * sc, rr.h * sc);
                    let time = if self.motion.reduced() { 8.0 } else { crate::clock::since(self.started).as_secs_f32() };
                    scene.sky(rr, az, alt, cover, wind, time, seed, moon);
                }
                Cmd::Atmosphere(rr, params, view_id, ordinal, _) => {
                    let rr = Rect::new(rr.x * sc + ox, rr.y * sc + oy, rr.w * sc, rr.h * sc);
                    let reduced = self.motion.reduced();
                    let moving = !reduced && self.art_budget() != crate::power::Budget::Still
                        && !self.prompt_composing && crate::clock::since(self.last_key) >= crate::sky::TYPING_HOLD;
                    let bind = self.skies.draw(&self.gpu, (view_id, ordinal), rr, params, moving, reduced);
                    let clip = scene.clip();
                    scene.texture(rr, bind, clip);
                    scene.layer(clip);
                    // Names and quiet lines belong to the sky's own layer, under the prompt.
                    self.draw_sky_overlay(scene, rr, view_id);
                }
                Cmd::Space(rr, params, view, ordinal) => {
                    let rr = Rect::new(rr.x*sc+ox,rr.y*sc+oy,rr.w*sc,rr.h*sc);
                    self.draw_space_cmd(scene,rr,params,(view,ordinal));
                }
                Cmd::Text(x, y, text, px, c, font, align, tracked) => {
                    let size = self.px(px) * sc;
                    let p = at(x, y);
                    if size < 5.0 {
                        let w = text.chars().count() as f32 * size * 0.6;
                        let x = match align { 1 => p[0] - w / 2.0, 2 => p[0] - w, _ => p[0] };
                        scene.rect(Rect::new(x, p[1] - size * 0.7, w, (size * 0.45).max(1.0)), fade(c, 0.6));
                        continue;
                    }
                    let font = match font { 1 => self.f.wordmark, 2 => self.f.strong, _ => self.f.ui };
                    let st = Style { font, px: size, color: c, tracking: if tracked { size * 0.08 } else { 0.0 } };
                    let w = self.fonts.measure(st, &text);
                    let x = match align { 1 => p[0] - w / 2.0, 2 => p[0] - w, _ => p[0] };
                    self.fonts.draw(scene, st, x, p[1], &text);
                }
            }
        }
        scene.layer(outer);
        cmds
    }

    /// The sampler, started the first time an art asks.
    pub(crate) fn procs_shared(&mut self) -> crate::procs::Shared {
        if self.procs.is_none() {
            self.procs = Some(crate::procs::start());
        }
        self.procs.clone().unwrap()
    }

    /// Only an explicitly chosen location is exposed to artwork.
    pub(crate) fn place(&self) -> Option<(f32, f32)> {
        self.behavior.place.map(|[lat, lon]| (lat, lon))
    }

    /// New art from the blank, opened in the editor.
    pub(crate) fn add_art(&mut self) {
        let d = dir();
        let _ = std::fs::create_dir_all(&d);
        let mut n = 1;
        let mut path = d.join("mine.luau");
        while path.exists() {
            n += 1;
            path = d.join(format!("mine-{n}.luau"));
        }
        let _ = std::fs::write(&path, TEMPLATE);
        let key = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        self.behavior.home_look = crate::settings::HomeLook::Art;
        self.behavior.home_art = key;
        self.save_prefs();
        self.open_file(&path, false);
        self.toast(nus_render::text::icons::PALETTE, "Your Art", "in the picker; save and it redraws", None);
    }

    /// The assistant writes one: the panel opens on a shell with the
    /// canvas described and a name asked for; the reply's ```luau block
    /// lands in profile/art/.
    pub(crate) fn ask_for_art(&mut self) {
        if self.ask_term().is_none() {
            let profile = self.behavior.default_profile;
            self.new_tab(profile);
        }
        let task = format!(
            "Write an art for nus's prompt: a Luau file that draws behind the line, quiet and beautiful, on paper and on ink. Reply with ONE ```luau block and nothing else, starting with `-- name: <a short name>` and `-- says: <one line>`. Keep it under 200 lines, draw nothing over c.prompt, and read this first:\n\n{}\n\nA blank to start from:\n\n```luau\n{}\n```",
            API,
            TEMPLATE
        );
        self.ask_send_task("an art for the prompt · the canvas is described, a luau file comes back", &task);
        if let Some(ask) = self.ask_term().and_then(|t| t.ask.as_mut()) {
            ask.art = true;
        }
        self.layout();
        self.toast(nus_render::text::icons::ASSISTANT, "Asking For Art", "it lands in the picker", None);
    }

    /// An answer with a ```luau block, from an ask that wanted an art.
    pub(crate) fn art_from_answer(&mut self, md: &str) -> Option<String> {
        let start = md.find("```luau").or_else(|| md.find("```lua"))?;
        let body_start = md[start..].find('\n')? + start + 1;
        let end = md[body_start..].find("```")? + body_start;
        let src = md[body_start..end].trim_end().to_string();
        let name = { let n = header(&src, "name"); if n.is_empty() { "from the assistant".to_string() } else { n } };
        let d = dir();
        let _ = std::fs::create_dir_all(&d);
        let mut key = slug(&name);
        let mut path = d.join(format!("{key}.luau"));
        let mut n = 1;
        while path.exists() {
            n += 1;
            key = format!("{}-{n}", slug(&name));
            path = d.join(format!("{key}.luau"));
        }
        std::fs::write(&path, format!("{src}\n")).ok()?;
        self.behavior.home_look = crate::settings::HomeLook::Art;
        self.behavior.home_art = key.clone();
        self.save_prefs();
        Some(name)
    }
}

/// The art's own folder, for the picker's OPEN THE FOLDER.
pub fn open_dir() {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = nus_compat::command(if cfg!(windows) { "explorer" } else { "open" }).arg(&d).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_view_identity_and_pose_survive_resize() {
        let mut art=Art::open("space");art.load(include_str!("../assets/art/space.luau"));
        for (w,h) in [(1280.0,800.0),(540.0,760.0)] {
            let commands=art.frame_at(Env{view_id:91,w,h,..Default::default()},125.0);
            assert!(matches!(commands.as_slice(),[Cmd::Space(_,p,91,0)] if p.time==0.0 && p.blend==0.0));
        }
        assert!(!retired_space(include_str!("../assets/art/space.luau")));
        assert!(!retired_space("-- name: space\nfunction draw(c) c:rect(0,0,10,10,'#ffffff') end"));
    }
    #[test]
    fn orbital_pass_count_is_bounded() {
        let mut art=Art::open("space");
        art.load("function draw(c) for i=1,100 do c:orbital({}) end end");
        let commands=art.frame_at(Env::default(),0.0);
        assert_eq!(commands.len(),2);assert!(art.status.as_deref().unwrap().contains("two orbital"));
    }
    #[test]
    fn customized_space_is_not_replaced() {
        let mut art=Art::open("space");
        art.load("function draw(c) c:rect(0,0,10,10,'#abcdef') end");
        assert!(matches!(art.frame_at(Env::default(),0.0).as_slice(),[Cmd::Rect(..)]));
    }

    #[test]
    fn cached_artwork_palette_tracks_theme_without_restarting_script() {
        let mut art = Art::open("memphis");
        art.load("local colors; local count=0; function draw(c) colors=colors or c.signals; count+=1; c:rect(count,0,10,10,colors.red) end");
        let env = Env { signals: Some(nus_render::theme::signal::ALL), ..Default::default() };
        let first = art.frame_at(env.clone(), 1.0);
        let mut next = env;
        next.signals.as_mut().unwrap()[0] = nus_render::theme::hex(0x83d7ff);
        let second = art.frame_at(next.clone(), 2.0);
        assert!(matches!(&first[0], Cmd::Rect(r, _, _) if r.x == 1.0));
        assert!(matches!(&second[0], Cmd::Rect(r, c, _) if r.x == 2.0 && *c == next.signals.unwrap()[0]));
        art.reclaim_scratch();
        let third = art.frame_at(next, 3.0);
        assert!(matches!(&third[0], Cmd::Rect(r, _, _) if r.x == 3.0));
        assert!(art.status.is_none());
    }

    #[test]
    fn cached_colors_preserve_alpha_mutable_tables_and_reload() {
        let mut art = Art::open("memphis");
        art.load("function draw(c) local a={r=1,g=0,b=0,a=1}; c:rect(0,0,1,1,a); a.r=0;a.b=1;c:rect(0,0,1,1,a); c:rect(0,0,1,1,'#ff000080',0.5); c:rect(0,0,1,1,'#ff000080',1) end");
        let commands = art.frame_at(Env::default(), 1.0);
        let colors: Vec<_> = commands.iter().filter_map(|c| if let Cmd::Rect(_, c, _) = c { Some(*c) } else { None }).collect();
        assert_eq!(colors[0], [1.0,0.0,0.0,1.0]);
        assert_eq!(colors[1], [0.0,0.0,1.0,1.0]);
        assert_eq!(colors[2][3], 128.0/255.0*0.5);
        assert_eq!(colors[3][3], 128.0/255.0);
        assert_eq!(art.state.borrow().colors.len(), 1);
        art.load("function draw(c) c:rect(0,0,1,1,'#0000ff') end");
        assert!(art.state.borrow().colors.is_empty());
        let commands = art.frame_at(Env::default(), 2.0);
        assert!(matches!(commands[0], Cmd::Rect(_, [0.0,0.0,1.0,1.0], _)));
        art.reclaim_scratch();
        assert!(art.state.borrow().colors.is_empty());
        assert_eq!(art.state.borrow().points.borrow().bytes, 0);
    }

    #[test]
    #[ignore = "diagnostic allocation stack samples"]
    fn memphis_allocation_stacks() {
        let mut art = Art::open("memphis");
        for i in 0..30 { let c=art.frame_at(Env {w:1280.0,h:800.0,..Default::default()},8.0+i as f32/30.0); art.recycle_commands(c); }
        for nth in [1, 10, 30, 100, 200, 400, 500] {
            let (_, counts, stack) = crate::count_alloc::sample(nth, || {
                let c=art.frame_at(Env {w:1280.0,h:800.0,..Default::default()},10.0); art.recycle_commands(c);
            });
            eprintln!("ALLOCATION {nth}: {counts:?}\n{}", stack.map(|v|v.to_string()).unwrap_or_default());
        }
    }

    #[test]
    fn polygon_pool_reuses_storage_bounds_retention_and_clears_flat_pairs() {
        let pool = SharedPoints::default();
        let mut points = Points::new(&pool, 16);
        points.push([1.0, 2.0]);
        let ptr = points.as_ptr();
        drop(points);
        let points = Points::new(&pool, 16);
        assert_eq!(points.as_ptr(), ptr);
        assert!(points.is_empty());
        drop(points);
        let many: Vec<_> = (0..200).map(|_| Points::new(&pool, 2048)).collect();
        drop(many);
        assert!(pool.borrow().bytes <= 256 * 1024);
        assert!(pool.borrow().buffers.len() <= 128);
        let lua = mlua::Lua::new();
        let table = lua.create_sequence_from([1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert_eq!(points_of(&table, &pool).unwrap().as_slice(), &[[1.0, 2.0], [3.0, 4.0]]);
    }

    // Run with NUS_ART_REFERENCE pointing to the prior Memphis source. Keeps
    // the reference external instead of shipping a duplicate built-in artwork.
    #[test]
    #[ignore = "requires NUS_ART_REFERENCE; records allocation comparison"]
    fn memphis_reference_and_allocations() {
        let source = std::fs::read_to_string(std::env::var("NUS_ART_REFERENCE").unwrap()).unwrap();
        let mut before = Art::open("memphis"); before.load(&source);
        let mut after = Art::open("memphis");
        // Exact command equality covers entry, idle, pointer, typing and taps,
        // plus welcome's custom placements and repeated frames with reused tables.
        for pieces in [vec![], vec![[2.0,100.0,100.0,0.3],[6.0,650.0,480.0,0.4]]] {
            for time in [0.0, 0.3, 0.7, 1.0, 1.8, 3.0, 8.0, 8.2, 9.0] {
                let env = Env { w:1280.0, h:800.0, pieces:pieces.clone(), pointer:Some((700.0,250.0)),
                    typed:if time > 8.0 {"hello".into()} else {String::new()},
                    taps:if time == 8.2 {vec![(80.0,640.0)]} else {vec![]}, ..Default::default() };
                let a = before.frame_at(env.clone(), time); let b = after.frame_at(env, time);
                assert!(before.status.is_none() && after.status.is_none(), "{:?} {:?}", before.status, after.status);
                assert_eq!(a, b, "geometry at {time}");
                before.recycle_commands(a); after.recycle_commands(b);
            }
        }
        fn sample(art: &mut Art) -> crate::count_alloc::Counts {
            for i in 0..30 { let c = art.frame_at(Env {w:1280.0,h:800.0,..Default::default()},10.0+i as f32/30.0); art.recycle_commands(c); }
            crate::count_alloc::count(|| {
                for i in 0..300 {
                    let c = art.frame_at(Env {w:1280.0,h:800.0,..Default::default()},12.0+i as f32/30.0);
                    art.recycle_commands(c);
                }
            }).1
        }
        let a = sample(&mut before); let b = sample(&mut after);
        eprintln!("MEMPHIS_ALLOC frames=300 before_calls={} before_bytes={} after_calls={} after_bytes={}",a.calls,a.bytes,b.calls,b.bytes);
        assert!(b.calls < a.calls && b.bytes < a.bytes);
    }

    #[test]
    fn rendered_commands_reuse_capacity_and_do_not_leak_into_the_next_frame() {
        let mut art = Art::open("memphis");
        art.load("function draw(c) c:rect(1,2,3,4,'#ffffff') end");
        let mut commands = art.frame_at(Env::default(), 1.0);
        let storage = commands.as_ptr();
        let capacity = commands.capacity();
        commands.clear();
        art.recycle_commands(commands);
        let commands = art.frame_at(Env::default(), 2.0);
        assert_eq!(commands.as_ptr(), storage);
        assert_eq!(commands.capacity(), capacity);
        assert_eq!(commands.len(), 1);
    }

    #[test]
    fn runaway_artwork_stops_without_unbounded_host_or_lua_memory() {
        let mut art = Art::open("memphis");
        art.load("function draw(c) for i=1,10000 do c:rect(0,0,1,1,'#ffffff') end end");
        assert!(art.frame(Env::default()).len() <= 8192);
        assert!(art.status.as_deref().unwrap().contains("budget"));
        art.load("function draw(c) while true do end end");
        art.frame(Env::default());
        assert!(art.status.as_deref().unwrap().contains("execution budget"));
        art.load("local huge = string.rep('x', 64*1024*1024)");
        assert!(art.status.is_some());
        art.load("function draw(c) c:text(0,0,'Recovered',12,'#ffffff') end");
        assert_eq!(art.frame(Env::default()).len(), 1);
        assert!(art.status.is_none());
    }

    #[test]
    fn headers_and_slugs() {
        assert_eq!(header("-- name: the pond\n-- says: koi, quiet\nlocal x = 1", "name"), "the pond");
        assert_eq!(header("-- name: the pond\n-- says: koi, quiet\n", "says"), "koi, quiet");
        assert_eq!(slug("The Pond!  at night"), "the-pond-at-night");
        assert_eq!(slug(""), "art");
    }


    #[test]
    fn the_builtins_draw() {
        for (key, _, _) in BUILTIN {
            let mut art = Art::open(key);
            assert!(art.status.is_none(), "{key}: {:?}", art.status);
            let env = Env { w: 1280.0, h: 800.0, line: [250.0, 300.0, 780.0, 40.0], face: "paper".into(), paper: [0.96, 0.95, 0.92, 1.0], ink: [0.08, 0.08, 0.08, 1.0], signal: [0.78, 0.06, 0.18, 1.0], dim: [0.42, 0.41, 0.38, 1.0], tint: [0.9, 0.9, 0.88, 1.0], place: Some((35.2, -106.6)), scale: 1.0, ..Default::default() };
            // Memphis drops in on the beat: give it a beat.
            std::thread::sleep(std::time::Duration::from_millis(320));
            let mut total = 0;
            for _ in 0..3 {
                let cmds = art.frame(env.clone());
                assert!(art.status.is_none(), "{key}: {:?}", art.status);
                total += cmds.len();
            }
            assert!(total > 0, "{key} drew nothing");
        }
    }

    fn assert_art_reading_area_clear(commands: &[Cmd], line: [f32; 4], rows: f32, size: [f32; 2]) {
        let protected = Rect::new(line[0], line[1] - 20.0, line[2], line[3] + rows + 20.0);
        for command in commands {
            let bounds = match command {
                Cmd::Rect(r, _, _) if r.x == 0.0 && r.y == 0.0 && r.w == size[0] && r.h == size[1] => continue,
                Cmd::Rect(r, ..) => *r,
                Cmd::Line(x0, y0, x1, y1, w, _) => Rect::new(x0.min(*x1) - w / 2.0, y0.min(*y1) - w / 2.0, (x1 - x0).abs() + w, (y1 - y0).abs() + w),
                Cmd::Text(x, y, text, px, _, _, align, tracked) => {
                    let w = text.chars().count() as f32 * px * if *tracked { 0.68 } else { 0.6 };
                    let left = match align { 1 => x - w / 2.0, 2 => x - w, _ => *x };
                    Rect::new(left, y - px, w, px * 1.2)
                }
                _ => continue,
            };
            let overlaps = bounds.x < protected.x + protected.w && bounds.x + bounds.w > protected.x
                && bounds.y < protected.y + protected.h && bounds.y + bounds.h > protected.y;
            assert!(!overlaps, "artwork crosses reading area: {command:?}");
        }
    }

    #[test]
    fn pond_ripples_respect_new_result_rows_without_restarting_the_rings() {
        let mut art = Art::open("pond");
        art.load(include_str!("../assets/art/pond.luau"));
        let env = Env { w: 1200.0, h: 800.0, line: [300.0, 250.0, 600.0, 40.0],
            ink: [1.0, 0.0, 1.0, 1.0], face: "paper".into(), ..Default::default() };
        art.frame_at(Env { taps: vec![(500.0, 440.0), (100.0, 440.0)], ..env.clone() }, 1.0);
        let ripple_lines = |commands: Vec<Cmd>| commands.into_iter().filter(|c|
            matches!(c, Cmd::Line(_, _, _, _, _, col) if col[0] == 1.0 && col[1] == 0.0 && col[2] == 1.0)).collect::<Vec<_>>();
        let open = ripple_lines(art.frame_at(env.clone(), 2.0));
        let expanded = ripple_lines(art.frame_at(Env { rows: 250.0, ..env.clone() }, 2.0));
        assert!(art.status.is_none(), "{:?}", art.status);
        assert!(open.len() > expanded.len() && !expanded.is_empty());
        assert_art_reading_area_clear(&expanded, env.line, 250.0, [env.w, env.h]);
        let outside = |commands: Vec<Cmd>| commands.into_iter().filter(|c|
            matches!(c, Cmd::Line(x0, _, x1, _, _, _) if *x0 < 250.0 && *x1 < 250.0)).collect::<Vec<_>>();
        assert_eq!(outside(open), outside(expanded), "unobstructed ripples keep their geometry and brightness");
    }

    #[test]
    fn space_native_scene_receives_current_prompt_rows_at_narrow_widths() {
        let mut art = Art::open("space");
        art.load(include_str!("../assets/art/space.luau"));
        for (w, h, line, rows) in [
            (1280.0, 800.0, [260.0, 260.0, 760.0, 52.0], 220.0),
            (540.0, 780.0, [24.0, 220.0, 492.0, 68.0], 280.0),
            (720.0, 500.0, [34.0, 140.0, 652.0, 52.0], 210.0),
        ] {
            let commands = art.frame_at(Env { w, h, line, rows, pointer: Some((w / 2.0, 190.0)), typed: "a".into(), ..Default::default() }, 8.0);
            assert!(art.status.is_none(), "{:?}", art.status);
            assert_eq!(commands.len(),1,"Space must not retain the old chart");
            let Cmd::Space(r,p,_,_) = &commands[0] else {panic!("Space lacks the native orbital pass")};
            assert_eq!((r.w,r.h),(w,h));
            assert_eq!(p.reading_rect,[line[0]/w,line[1]/h,line[2]/w,(line[3]+rows)/h]);
            assert_eq!(p.blend,0.0);assert_eq!(p.time,0.0);
            assert_eq!(art.backdrop,Backdrop::Dark);
        }
    }

    #[test]
    fn brain_deep_process_trees_stay_outside_current_prompt_and_rows() {
        let sample = crate::procs::Sample {
            procs: (1..=90).map(|pid| crate::procs::Proc { pid, ppid: pid - 1,
                name: if pid == 1 { "nus".into() } else { format!("worker-{pid}") },
                cpu: 84.0, mem: 140.0, ..Default::default() }).collect(),
            taken: Some(std::time::Instant::now()), ..Default::default()
        };
        let shared = std::sync::Arc::new(std::sync::Mutex::new(sample));
        let mut art = Art::open("brain");
        art.load(include_str!("../assets/art/brain.luau"));
        for (w, h, line, rows) in [
            (1280.0, 900.0, [260.0, 300.0, 760.0, 52.0], 0.0),
            (1280.0, 900.0, [260.0, 300.0, 760.0, 52.0], 260.0),
            (520.0, 840.0, [24.0, 260.0, 472.0, 70.0], 300.0),
        ] {
            let commands = art.frame_at(Env { w, h, line, rows, procs: Some(shared.clone()),
                ink: [0.1, 0.1, 0.1, 1.0], typed: "worker".into(), ..Default::default() }, 8.0);
            assert!(art.status.is_none(), "{:?}", art.status);
            assert!(commands.iter().any(|c| matches!(c, Cmd::Text(_, _, text, ..) if text.contains('%'))));
            assert_art_reading_area_clear(&commands, line, rows, [w, h]);
        }
    }

    #[test]
    fn sky_view_identity_survives_resize_and_separates_commands() {
        let mut art = Art::open("sky");
        art.load("function draw(c) c:atmosphere({}) c:atmosphere({}) end");
        let collect = |commands: Vec<Cmd>| commands.into_iter().filter_map(|cmd| match cmd {
            Cmd::Atmosphere(rect, _, view, ordinal, _) => Some((rect.w, rect.h, view, ordinal)), _ => None,
        }).collect::<Vec<_>>();
        let env = Env { view_id: 41, w: 800.0, h: 600.0, ..Default::default() };
        let initial = collect(art.frame_at(env.clone(), 8.0));
        let resized = collect(art.frame_at(Env { w: 600.0, h: 900.0, ..env.clone() }, 8.0));
        let another = collect(art.frame_at(Env { view_id: 42, ..env }, 8.0));
        assert!(art.status.is_none(), "{:?}", art.status);
        assert_eq!(initial, vec![(800.0, 600.0, 41, 0), (800.0, 600.0, 41, 1)]);
        assert_eq!(resized, vec![(600.0, 900.0, 41, 0), (600.0, 900.0, 41, 1)]);
        assert_eq!(another, vec![(800.0, 600.0, 42, 0), (800.0, 600.0, 42, 1)]);
    }

    #[test]
    fn sky_taps_do_not_change_cloud_position_parameters() {
        let mut art = Art::open("sky");
        let env = Env { w: 800.0, h: 600.0, ..Default::default() };
        let params = |commands: Vec<Cmd>| commands.into_iter().find_map(|c| match c {
            Cmd::Atmosphere(_, p, ..) => Some((p.wind_low, p.wind_mid, p.wind_high, p.seed)), _ => None,
        }).unwrap();
        let before = params(art.frame_at(env.clone(), 100.0));
        let after = params(art.frame_at(Env { taps: vec![(300.0, 200.0)], ..env }, 100.0));
        assert_eq!(before, after, "tapping must not jump the cloud field");
    }

    #[test]
    fn sky_uses_clear_weather_and_converts_wind_without_inventing_upper_measurements() {
        let mut art = Art::open("sky");
        art.load(BUILTIN.iter().find(|(key, _, _)| *key == "sky").unwrap().2);
        let w = crate::weather::WeatherSnapshot {
            cloud_cover: Some(0.0), cloud_low: Some(0.0), cloud_mid: Some(0.0), cloud_high: Some(0.0),
            wind_10m: crate::weather::Wind { speed_mps: Some(10.0), direction_deg: Some(270.0), height_m: Some(10.0) },
            ..Default::default()
        };
        let commands = art.frame_at(Env { w:800.0, h:600.0, weather:Some(w), ..Default::default() }, 8.0);
        assert!(art.status.is_none(), "{:?}", art.status);
        let p = commands.into_iter().find_map(|c| if let Cmd::Atmosphere(_,p,..)=c {Some(p)} else {None}).unwrap();
        assert_eq!((p.low_cover,p.mid_cover,p.high_cover), (0.0,0.0,0.0));
        assert!((p.wind_low[0]-10.0).abs()<0.0001 && p.wind_low[1].abs()<0.0001);
        assert!((p.wind_mid[0]-14.0).abs()<0.0001, "upper drift is a deterministic fallback");
    }

    #[test]
    fn atmosphere_preserves_front_back_directions_and_legacy_sky() {
        let mut art = Art::open("sky");
        art.load("function draw(c) c:atmosphere({sun={0,0,-1},moon={0,0,1}}) c:sky({az=0,alt=.5}) end");
        let commands = art.frame_at(Env { w:800.0, h:600.0, ..Default::default() }, 8.0);
        assert!(art.status.is_none(), "{:?}", art.status);
        assert!(commands.iter().any(|c| matches!(c, Cmd::Sky(..))));
        let p = commands.into_iter().find_map(|c| if let Cmd::Atmosphere(_,p,..)=c {Some(p)} else {None}).unwrap();
        assert_eq!(p.sun_direction, [0.0,0.0,-1.0]);
        assert_eq!(p.moon_direction, [0.0,0.0,1.0]);
    }

    #[test]
    fn artwork_contrast_is_independent_of_the_app_theme() {
        use nus_render::Mode;
        let black = [0.0, 0.0, 0.0, 1.0];
        let white = [1.0; 4];
        for (mode, ink, paper) in [(Mode::Paper, black, white), (Mode::Ink, white, black)] {
            assert_eq!(Backdrop::Light.foreground(mode, ink, paper), black);
            assert_eq!(Backdrop::Dark.foreground(mode, ink, paper), white);
            assert_eq!(Backdrop::Theme.foreground(mode, ink, paper), ink);
        }
    }

    #[test]
    fn sky_text_remains_legible_with_a_saturated_chrome_theme() {
        let blue = [0.13, 0.37, 0.79, 1.0];
        let white = [1.0; 4];
        let day = Backdrop::Light.sky_foreground(nus_render::Mode::Ink, white, blue);
        assert!(day[..3].iter().all(|c| *c < 0.1));
        let night = Backdrop::Dark.sky_foreground(nus_render::Mode::Paper, white, blue);
        assert!(night[..3].iter().all(|c| *c > 0.9));
        assert_eq!(Backdrop::Theme.sky_foreground(nus_render::Mode::Ink, blue, white), blue);
    }

    #[test]
    fn sky_art_draws_without_location_and_with_an_explicit_place() {
        for (key, place) in ["sky", "space"].into_iter().flat_map(|key| [None, Some((35.2, -106.6)), Some((-33.9, 151.2))].map(|place| (key, place))) {
            let mut art=Art::open(key);
            let commands=art.frame_at(Env {w:800.0,h:600.0,place,..Default::default()},8.0);
            assert!(art.status.is_none(),"{key}: {:?}",art.status);
            if key == "sky" {
                assert!(commands.iter().any(|c|matches!(c,Cmd::Atmosphere(..))), "sky has no atmosphere layer");
                assert_ne!(art.backdrop, Backdrop::Theme);
            } else {
                assert!(matches!(commands.as_slice(),[Cmd::Space(..)]), "Space must use Limb/Darkroom, not the retired local chart");
            }
            assert_eq!(art.state.borrow().env.place, place, "art must not invent a user location");
            if place.is_none() {
                assert!(!commands.iter().any(|c| matches!(c, Cmd::Text(_,_,text,..) if text.contains('°') || text.starts_with("LST "))), "sample chart must not display a fabricated user location or local time");
            }
        }
    }

    #[test]
    fn welcome_peek_points_at_the_nearer_edge() {
        for (x,chevron) in [(0.0,"«"),(800.0,"»")] {
            let mut art=Art::open("memphis");
            let commands=art.frame_at(Env {w:800.0,h:600.0,pieces:vec![[11.0,x,300.0,0.5]],face:"paper".into(),paper:[1.0;4],ink:[0.0,0.0,0.0,1.0],..Default::default()},3.0);
            assert!(art.status.is_none(),"{:?}",art.status);
            assert!(commands.iter().any(|c|matches!(c,Cmd::Text(_,_,t,..) if t==chevron)),"{chevron} at x={x}");
            // The dome reaches into the pane from its edge, never past it.
            let polys:Vec<_>=commands.iter().filter_map(|c|if let Cmd::Poly(points,_)=c {Some(points)} else {None}).collect();
            assert!(!polys.is_empty());
            assert!(polys.iter().any(|p|p.iter().all(|xy|(xy[0]-x).abs()<=50.0)));
        }
        // Behind the prompt it stays out of the composition.
        let mut art=Art::open("memphis");
        let commands=art.frame_at(Env {w:1280.0,h:800.0,..Default::default()},3.0);
        // The sticker's » is the only text there.
        assert_eq!(commands.iter().filter(|c|matches!(c,Cmd::Text(..))).count(),1);
    }

    #[test]
    fn welcome_places_existing_memphis_vectors_individually() {
        let mut art=Art::open("memphis");
        let commands=art.frame_at(Env {w:800.0,h:600.0,pieces:vec![[2.0,100.0,100.0,0.3],[6.0,650.0,480.0,0.4]],face:"paper".into(),paper:[1.0;4],ink:[0.0,0.0,0.0,1.0],..Default::default()},3.0);
        assert!(art.status.is_none(),"{:?}",art.status);
        let polys:Vec<_>=commands.iter().filter_map(|c|if let Cmd::Poly(points,_)=c {Some(points)} else {None}).collect();
        assert!(polys.iter().any(|p|p.iter().all(|xy|xy[0]<180.0 && xy[1]<180.0)));
        assert!(polys.iter().any(|p|p.iter().all(|xy|xy[0]>550.0 && xy[1]>450.0)));
    }

    #[test]
    fn a_script_draws() {
        let mut art = Art { key: "t".into(), name: "t".into(), path: None, mtime: None, checked: crate::clock::now(), lua: mlua::Lua::new(), state: Rc::new(RefCell::new(State { env: Env::default(), cmds: Vec::new(), command_bytes: 0, points: Default::default(), colors: Default::default(), signals: None, t: 0.0, dt: 0.0, backdrop: Backdrop::Theme })), status: None, backdrop: Backdrop::Theme, started: crate::clock::now(), last: crate::clock::now(), reloads: 0 };
        art.load("function draw(c) c:rect(1, 2, 3, 4, c.ink) c:circle(5, 5, 2, '#c8102e', 0.5) c:text(0, 10, 'hi', 11, c.dim, 1, { caps = true }) c:blob({ {0,0}, {10,0}, {10,10}, {0,10} }, c.signal) end");
        let cmds = art.frame(Env { w: 100.0, h: 100.0, ..Default::default() });
        assert!(cmds.len() >= 4, "{}", cmds.len());
        assert!(art.status.is_none());
        art.load("function draw(c) error('boom') end");
        let _ = art.frame(Env::default());
        assert!(art.status.as_deref().unwrap_or("").contains("boom"));
    }
}
