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

/// The four that ship, by their file names.
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
pub enum Cmd {
    Rect(Rect, Color, f32),
    Quad([[f32; 2]; 4], Color),
    /// A filled polygon, any shape, one instance.
    Poly(Vec<[f32; 2]>, Color),
    /// (x1, y1, x2, y2, width)
    Line(f32, f32, f32, f32, f32, Color),
    /// x, y (baseline), text, px, colour, font (0 mono · 1 serif · 2 strong), align (0 left · 1 centre · 2 right), tracked
    Text(f32, f32, String, f32, Color, u8, u8, bool),
    /// A sky over the rect: (az -1..1, sin alt, cover, wind, seed).
    Sky(Rect, f32, f32, f32, f32, [f32; 2]),
}

/// What the canvas knows this frame.
#[derive(Clone, Default)]
pub struct Env {
    pub w: f32,
    pub h: f32,
    /// The line's box (x, y, w, h) and how far the rows beneath it reach.
    pub line: [f32; 4],
    pub rows: f32,
    pub pointer: Option<(f32, f32)>,
    pub typed: String,
    pub taps: Vec<(f32, f32)>,
    pub face: String,
    pub paper: Color,
    pub ink: Color,
    pub signal: Color,
    pub dim: Color,
    pub tint: Color,
    pub place: (f32, f32),
    pub procs: Option<crate::procs::Shared>,
    /// Logical px per… the pane's scale, so an art can size hairlines.
    pub scale: f32,
}

struct State {
    env: Env,
    cmds: Vec<Cmd>,
    t: f32,
    dt: f32,
    /// The art says its backdrop is dark: the line goes paper over it.
    dark: bool,
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

fn points_of(t: &mlua::Table) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for v in t.sequence_values::<mlua::Value>().flatten() {
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
        let flat: Vec<f32> = out.iter().map(|p| p[0]).collect();
        return flat.chunks(2).filter(|c| c.len() == 2).map(|c| [c[0], c[1]]).collect();
    }
    out
}

/// A smooth closed curve through the points (quadratics through the
/// midpoints), as a polygon.
fn smooth_closed(pts: &[[f32; 2]], per: usize) -> Vec<[f32; 2]> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut out = Vec::with_capacity(n * per);
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
fn smooth_open(pts: &[[f32; 2]], per: usize) -> Vec<[f32; 2]> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
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
        f.add_field_method_get("signals", |lua, _| {
            let t = lua.create_table()?;
            for (k, v) in [("red", "#c8102e"), ("blue", "#1f5fbf"), ("gold", "#d9a400"), ("green", "#2e7d32"), ("violet", "#6b3fa0"), ("teal", "#1a7f8a")] {
                t.set(k, v)?;
            }
            Ok(t)
        });
    }

    fn add_methods<M: mlua::UserDataMethods<Self>>(m: &mut M) {
        m.add_method("rect", |_, c, (x, y, w, h, col, a, r): (f32, f32, f32, f32, mlua::Value, Option<f32>, Option<f32>)| {
            if let Some(col) = color_of(&col, a) {
                c.0.borrow_mut().cmds.push(Cmd::Rect(Rect::new(x, y, w, h), col, r.unwrap_or(0.0)));
            }
            Ok(())
        });
        m.add_method("circle", |_, c, (cx, cy, r, col, a): (f32, f32, f32, mlua::Value, Option<f32>)| {
            if let Some(col) = color_of(&col, a) {
                c.0.borrow_mut().cmds.push(Cmd::Rect(Rect::new(cx - r, cy - r, r * 2.0, r * 2.0), col, r));
            }
            Ok(())
        });
        m.add_method("oval", |_, c, (cx, cy, rx, ry, col, a): (f32, f32, f32, f32, mlua::Value, Option<f32>)| {
            if let Some(col) = color_of(&col, a) {
                let n = ((rx.max(ry) * 0.8) as usize).clamp(24, 96);
                let pts: Vec<[f32; 2]> = (0..n).map(|i| { let th = i as f32 / n as f32 * std::f32::consts::TAU; [cx + rx * th.cos(), cy + ry * th.sin()] }).collect();
                c.0.borrow_mut().cmds.push(Cmd::Poly(pts, col));
            }
            Ok(())
        });
        m.add_method("line", |_, c, (x1, y1, x2, y2, w, col, a): (f32, f32, f32, f32, f32, mlua::Value, Option<f32>)| {
            if let Some(col) = color_of(&col, a) {
                c.0.borrow_mut().cmds.push(Cmd::Line(x1, y1, x2, y2, w, col));
            }
            Ok(())
        });
        m.add_method("quad", |_, c, (pts, col, a): (mlua::Table, mlua::Value, Option<f32>)| {
            let p = points_of(&pts);
            if let (Some(col), true) = (color_of(&col, a), p.len() >= 4) {
                c.0.borrow_mut().cmds.push(Cmd::Quad([p[0], p[1], p[2], p[3]], col));
            }
            Ok(())
        });
        m.add_method("poly", |_, c, (pts, col, a): (mlua::Table, mlua::Value, Option<f32>)| {
            let p = points_of(&pts);
            if let (Some(col), true) = (color_of(&col, a), p.len() >= 3) {
                c.0.borrow_mut().cmds.push(Cmd::Poly(p, col));
            }
            Ok(())
        });
        m.add_method("blob", |_, c, (pts, col, a): (mlua::Table, mlua::Value, Option<f32>)| {
            let p = smooth_closed(&points_of(&pts), 5);
            if let (Some(col), true) = (color_of(&col, a), p.len() >= 3) {
                c.0.borrow_mut().cmds.push(Cmd::Poly(p, col));
            }
            Ok(())
        });
        m.add_method("curve", |_, c, (pts, w, col, a): (mlua::Table, f32, mlua::Value, Option<f32>)| {
            let p = smooth_open(&points_of(&pts), 4);
            if let Some(col) = color_of(&col, a) {
                let mut s = c.0.borrow_mut();
                for pair in p.windows(2) {
                    s.cmds.push(Cmd::Line(pair[0][0], pair[0][1], pair[1][0], pair[1][1], w, col));
                }
            }
            Ok(())
        });
        m.add_method("text", |_, c, (x, y, text, px, col, a, opts): (f32, f32, String, f32, mlua::Value, Option<f32>, Option<mlua::Table>)| {
            if let Some(col) = color_of(&col, a) {
                let (mut font, mut align, mut tracked) = (0u8, 0u8, false);
                if let Some(o) = opts {
                    font = match o.get::<String>("font").unwrap_or_default().as_str() { "serif" => 1, "strong" => 2, _ => 0 };
                    align = match o.get::<String>("align").unwrap_or_default().as_str() { "center" | "centre" => 1, "right" => 2, _ => 0 };
                    tracked = o.get::<bool>("caps").unwrap_or(false);
                }
                c.0.borrow_mut().cmds.push(Cmd::Text(x, y, text, px, col, font, align, tracked));
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
            s.cmds.push(Cmd::Sky(r, g("az", 0.0).clamp(-1.0, 1.0), g("alt", 0.5).clamp(-1.0, 1.0), g("cover", 0.4).clamp(0.0, 1.0), g("wind", 1.0), seed));
            Ok(())
        });
        // What the art is drawn on: "dark" puts the line in paper with a
        // shadow; "paper" (the default) keeps it in ink.
        m.add_method("backdrop", |_, c, which: String| {
            c.0.borrow_mut().dark = which == "dark";
            Ok(())
        });
        // The clock, in unix milliseconds; NUS_CLOCK pins it (for photographs of a night sky at noon).
        m.add_method("now", |_, _, ()| {
            if let Some(ms) = std::env::var("NUS_CLOCK").ok().and_then(|v| v.parse::<f64>().ok()) {
                return Ok(ms);
            }
            Ok(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0))
        });
        m.add_method("place", |lua, c, ()| {
            let (lat, lon) = c.0.borrow().env.place;
            let t = lua.create_table()?;
            t.set("lat", lat)?;
            t.set("lon", lon)?;
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
    /// Set by the last frame: the line goes paper over this art.
    pub dark: bool,
    started: Instant,
    last: Instant,
    /// Keep the koi and the stars where they were across a reload.
    pub reloads: u32,
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
            checked: Instant::now(),
            lua: mlua::Lua::new(),
            state: Rc::new(RefCell::new(State { env: Env::default(), cmds: Vec::new(), t: 0.0, dt: 0.0, dark: false })),
            dark: false,
            status: None,
            started: Instant::now(),
            last: Instant::now(),
            reloads: 0,
        };
        art.load(&src);
        art
    }

    fn load(&mut self, src: &str) {
        let lua = mlua::Lua::new();
        lua.sandbox(true).ok();
        self.status = None;
        match lua.load(src).set_name(&self.key).exec() {
            Ok(()) => {}
            Err(e) => self.status = Some(short_error(&e)),
        }
        self.lua = lua;
    }

    /// The file changed: load it again, keeping the clock.
    pub fn tend(&mut self) {
        if self.checked.elapsed().as_millis() < 700 {
            return;
        }
        self.checked = Instant::now();
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
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().clamp(1.0 / 240.0, 0.05);
        self.last = now;
        {
            let mut s = self.state.borrow_mut();
            s.env = env;
            s.cmds.clear();
            s.dark = false;
            s.t = self.started.elapsed().as_secs_f32();
            s.dt = dt;
        }
        if self.status.is_some() {
            return Vec::new();
        }
        let canvas = Canvas(self.state.clone());
        let g = self.lua.globals();
        let result: mlua::Result<()> = match g.get::<mlua::Function>("draw") {
            Ok(f) => f.call::<()>(canvas),
            Err(_) => Err(mlua::Error::runtime("no draw(c) in this art")),
        };
        if let Err(e) = result {
            self.status = Some(short_error(&e));
        }
        self.dark = self.state.borrow().dark;
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
    pub(crate) fn draw_art_cmds(&mut self, scene: &mut Scene, r: Rect, cmds: Vec<Cmd>) {
        self.draw_art_cmds_scaled(scene, r, cmds, 1.0);
    }

    /// The same, the canvas shrunk by `sc` into `r`: a card's preview of
    /// an art that ran at the pane's size. Type too small to read is
    /// greeked — a bar where the words would be.
    pub(crate) fn draw_art_cmds_scaled(&mut self, scene: &mut Scene, r: Rect, cmds: Vec<Cmd>, sc: f32) {
        scene.layer(Some(r));
        let (ox, oy) = (r.x, r.y);
        let at = |x: f32, y: f32| -> [f32; 2] { [x * sc + ox, y * sc + oy] };
        for cmd in cmds {
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
                    let p: Vec<[f32; 2]> = p.iter().map(|q| at(q[0], q[1])).collect();
                    scene.poly(&p, c);
                }
                Cmd::Poly(p, c) => {
                    let p: Vec<[f32; 2]> = p.iter().map(|q| at(q[0], q[1])).collect();
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
                Cmd::Sky(rr, az, alt, cover, wind, seed) => {
                    let rr = Rect::new(rr.x * sc + ox, rr.y * sc + oy, rr.w * sc, rr.h * sc);
                    scene.sky(rr, az, alt, cover, wind, self.started.elapsed().as_secs_f32(), seed);
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
        scene.layer(None);
    }

    /// The sampler, started the first time an art asks.
    pub(crate) fn procs_shared(&mut self) -> crate::procs::Shared {
        if self.procs.is_none() {
            self.procs = Some(crate::procs::start());
        }
        self.procs.clone().unwrap()
    }

    /// A rough place for the sky: the setting, else the clock's zone.
    pub(crate) fn place(&self) -> (f32, f32) {
        if let Some([lat, lon]) = self.behavior.place {
            return (lat, lon);
        }
        // Local offset from UTC, in hours, puts the longitude within a zone.
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let offset_h = local_offset_hours(now);
        (40.0, (offset_h * 15.0).clamp(-180.0, 180.0))
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
        self.toast("YOURS · IN THE PICKER · SAVE AND IT REDRAWS", None);
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
        self.toast("ASKING FOR AN ART · IT LANDS IN THE PICKER", None);
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

/// Hours east of UTC for the local zone at `unix`, from the C library's
/// idea of local time.
fn local_offset_hours(unix: i64) -> f32 {
    // Windows: _get_timezone gives seconds west of UTC; DST from _get_daylight.
    #[cfg(windows)]
    {
        extern "C" {
            fn _get_timezone(seconds: *mut std::os::raw::c_long) -> std::os::raw::c_int;
            fn _get_daylight(hours: *mut std::os::raw::c_int) -> std::os::raw::c_int;
        }
        let mut tz: std::os::raw::c_long = 0;
        let mut dl: std::os::raw::c_int = 0;
        unsafe {
            let _ = _get_timezone(&mut tz);
            let _ = _get_daylight(&mut dl);
        }
        let _ = unix;
        return -(tz as f32) / 3600.0 + if dl != 0 { 1.0 } else { 0.0 };
    }
    #[cfg(not(windows))]
    {
        let _ = unix;
        0.0
    }
}

/// The art's own folder, for the picker's OPEN THE FOLDER.
pub fn open_dir() {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = std::process::Command::new(if cfg!(windows) { "explorer" } else { "open" }).arg(&d).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_slugs() {
        assert_eq!(header("-- name: the pond\n-- says: koi, quiet\nlocal x = 1", "name"), "the pond");
        assert_eq!(header("-- name: the pond\n-- says: koi, quiet\n", "says"), "koi, quiet");
        assert_eq!(slug("The Pond!  at night"), "the-pond-at-night");
        assert_eq!(slug(""), "art");
    }


    #[test]
    fn the_four_that_ship_draw() {
        for (key, _, _) in BUILTIN {
            let mut art = Art::open(key);
            assert!(art.status.is_none(), "{key}: {:?}", art.status);
            let env = Env { w: 1280.0, h: 800.0, line: [250.0, 300.0, 780.0, 40.0], face: "paper".into(), paper: [0.96, 0.95, 0.92, 1.0], ink: [0.08, 0.08, 0.08, 1.0], signal: [0.78, 0.06, 0.18, 1.0], dim: [0.42, 0.41, 0.38, 1.0], tint: [0.9, 0.9, 0.88, 1.0], place: (35.2, -106.6), scale: 1.0, ..Default::default() };
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

    #[test]
    fn a_script_draws() {
        let mut art = Art { key: "t".into(), name: "t".into(), path: None, mtime: None, checked: Instant::now(), lua: mlua::Lua::new(), state: Rc::new(RefCell::new(State { env: Env::default(), cmds: Vec::new(), t: 0.0, dt: 0.0, dark: false })), status: None, dark: false, started: Instant::now(), last: Instant::now(), reloads: 0 };
        art.load("function draw(c) c:rect(1, 2, 3, 4, c.ink) c:circle(5, 5, 2, '#c8102e', 0.5) c:text(0, 10, 'hi', 11, c.dim, 1, { caps = true }) c:blob({ {0,0}, {10,0}, {10,10}, {0,10} }, c.signal) end");
        let cmds = art.frame(Env { w: 100.0, h: 100.0, ..Default::default() });
        assert!(cmds.len() >= 4, "{}", cmds.len());
        assert!(art.status.is_none());
        art.load("function draw(c) error('boom') end");
        let _ = art.frame(Env::default());
        assert!(art.status.as_deref().unwrap_or("").contains("boom"));
    }
}
