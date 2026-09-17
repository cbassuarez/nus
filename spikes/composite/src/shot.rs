//! The app photographs itself.
//!
//! `NUS_SHOT=<script>` runs a step list on the app's own event loop — the
//! same functions the chords and clicks call, not synthetic input — and at
//! each `shot` step draws the scene into an offscreen texture and writes it
//! as a PNG. Nothing here touches the OS's input or focus, so it can run
//! while the machine is in use. `NUS_MODE` picks the face; the file is
//! `<name>-<face>.png` in `NUS_SHOT_OUT` (default `docs/media`).
//!
//! The script is one step per line, `verb rest`; `#` starts a comment:
//!
//!   wait 800                   milliseconds
//!   url http://localhost:8000/ open in the split, as the URL rule does
//!   tab https://…              open as a new tab
//!   shell git status           type into the shell at its prompt and run
//!   line cargo te              type into the shell without running
//!   palette go cargo te        the palette (go | new | url) with a query
//!   ask why did that fail?     the ask panel, with the question sent
//!   theme nord                 a stock theme by name
//!   board | compact | atlas | settings | devtools | reader | split | sidebar
//!   hover 40 200               the pointer at logical px from the top-left
//!   click 900 500              a left click there
//!   altclick 900 500           with Alt held (a peek)
//!   shot window                capture the whole window
//!   shot hero 0.5 0.06 0.5 0.94   capture a fraction [x y w h] of it
//!   focus shell | page         which half of the split has the focus
//!   cancel                     Ctrl+C to the shell: a clean prompt again
//!   close                      the palette, ask, board and atlas, whichever is up
//!   quit                       (implicit at the end)

use std::path::PathBuf;
use std::time::{Duration, Instant};

use winit::event::{ElementState, MouseButton};
use winit::keyboard::ModifiersState;

use crate::app::{App, PaletteMode, Pane};

pub struct Shot {
    steps: Vec<String>,
    next: usize,
    until: Option<Instant>,
    out: PathBuf,
    face: &'static str,
    /// A capture asked for by the last step, taken after the next draw.
    pub pending: Option<(String, Option<[f32; 4]>)>,
    pub done: bool,
}

impl Shot {
    /// From `NUS_SHOT`, if set.
    pub fn from_env() -> Option<Shot> {
        let path = std::env::var_os("NUS_SHOT")?;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("NUS_SHOT: {}: {e}", path.to_string_lossy());
                return None;
            }
        };
        let steps = text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        let out = std::env::var_os("NUS_SHOT_OUT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("docs/media"));
        let face = if std::env::var("NUS_MODE").ok().as_deref() == Some("paper") { "paper" } else { "ink" };
        Some(Shot { steps, next: 0, until: None, out, face, pending: None, done: false })
    }
}

impl App {
    /// One step per call, when the last wait is over and no capture is
    /// outstanding. Called from the main loop before the frame.
    pub fn shot_tick(&mut self) {
        let Some(s) = self.shot.as_mut() else { return };
        if s.done || s.pending.is_some() {
            return;
        }
        if s.until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        s.until = None;
        let Some(step) = s.steps.get(s.next).cloned() else {
            s.done = true;
            return;
        };
        s.next += 1;
        let (verb, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
        let rest = rest.trim();
        eprintln!("shot: {step}");
        match verb {
            "wait" => {
                let ms: u64 = rest.parse().unwrap_or(500);
                if let Some(s) = self.shot.as_mut() {
                    s.until = Some(Instant::now() + Duration::from_millis(ms));
                }
            }
            "url" => self.open_url(rest, false),
            "tab" => self.open_url(rest, true),
            "shell" => self.shot_type(&format!("{rest}\r")),
            "line" => self.shot_type(rest),
            "cancel" => self.shot_type(""),
            "focus" => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.focus_right = rest == "page" && t.right.is_some();
                }
                self.layout();
            }
            "palette" => {
                let (mode, query) = rest.split_once(' ').unwrap_or((rest, ""));
                let mode = match mode {
                    "new" => PaletteMode::New,
                    "url" => PaletteMode::Url,
                    _ => PaletteMode::Go,
                };
                self.open_palette(mode);
                if let Some((_, q)) = self.palette.as_mut() {
                    q.push_str(query);
                }
                self.palette_sel = 0;
            }
            "ask" => {
                // Ask sits beside a shell: the shell must hold the focus.
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.focus_right = false;
                }
                if self.ask_term().map_or(true, |t| t.ask.is_none()) {
                    self.toggle_ask();
                }
                if let Some(ask) = self.ask_term().and_then(|t| t.ask.as_mut()) {
                    ask.input = rest.to_string();
                }
                self.ask_send();
            }
            "theme" => {
                let want = rest.to_lowercase();
                if let Some(t) = crate::themes::all().into_iter().find(|t| t.name.to_lowercase() == want) {
                    self.apply_theme(&t);
                } else {
                    eprintln!("shot: no theme named {rest}");
                }
            }
            "board" => self.open_board(),
            "compact" => self.toggle_compact(),
            "atlas" => self.open_start(),
            "settings" => self.open_settings(),
            "devtools" => self.toggle_devtools(),
            "reader" => self.toggle_reader(),
            "split" => self.divide(),
            "sidebar" => self.run(crate::app::Action::ToggleSidebar),
            "close" => {
                self.palette = None;
                self.start = None;
                if self.board.open {
                    self.close_board();
                }
                if let Some(t) = self.ask_term() {
                    t.ask = None;
                }
                self.layout();
            }
            "hover" | "click" | "altclick" => {
                let mut it = rest.split_whitespace().filter_map(|n| n.parse::<f32>().ok());
                let (x, y) = (it.next().unwrap_or(0.0) * self.scale, it.next().unwrap_or(0.0) * self.scale);
                if verb == "altclick" {
                    self.modifiers(ModifiersState::ALT);
                }
                self.mouse_moved(x, y);
                if verb != "hover" {
                    self.mouse_button(MouseButton::Left, ElementState::Pressed);
                    self.mouse_button(MouseButton::Left, ElementState::Released);
                }
                if verb == "altclick" {
                    self.modifiers(ModifiersState::empty());
                }
            }
            "shot" => {
                let mut it = rest.split_whitespace();
                let name = it.next().unwrap_or("shot").to_string();
                let f: Vec<f32> = it.filter_map(|n| n.parse().ok()).collect();
                let crop = (f.len() == 4).then(|| [f[0], f[1], f[2], f[3]]);
                if let Some(s) = self.shot.as_mut() {
                    s.pending = Some((name, crop));
                }
            }
            "quit" => {
                if let Some(s) = self.shot.as_mut() {
                    s.done = true;
                }
            }
            other => eprintln!("shot: unknown step `{other}`"),
        }
        self.dirty = true;
    }

    /// Text into the shell of the active tab, or the first shell.
    fn shot_type(&mut self, text: &str) {
        let i = match self.tabs.get(self.active).map(|t| &t.left) {
            Some(Pane::Term(_)) => Some(self.active),
            _ => self.tabs.iter().position(|t| matches!(t.left, Pane::Term(_))),
        };
        if let Some(Pane::Term(t)) = i.and_then(|i| self.tabs.get_mut(i)).map(|t| &mut t.left) {
            let _ = t.pty.write(text.as_bytes());
        }
    }

    /// After a draw: the pending capture, from the scene just drawn.
    pub fn shot_capture(&mut self, clear: [f32; 4]) {
        let Some((name, crop)) = self.shot.as_mut().and_then(|s| s.pending.take()) else { return };
        let (w, h) = self.target.size;
        let rgba = self.gpu.snapshot((w, h), &self.scene, clear);
        let (x0, y0, cw, ch) = match crop {
            Some([x, y, cw, ch]) => (
                (x * w as f32) as u32,
                (y * h as f32) as u32,
                ((cw * w as f32) as u32).max(1).min(w),
                ((ch * h as f32) as u32).max(1).min(h),
            ),
            None => (0, 0, w, h),
        };
        let mut px = Vec::with_capacity((cw * ch * 4) as usize);
        for row in y0..(y0 + ch).min(h) {
            let start = ((row * w + x0) * 4) as usize;
            px.extend_from_slice(&rgba[start..start + (cw.min(w - x0) * 4) as usize]);
        }
        let Some(s) = self.shot.as_ref() else { return };
        let _ = std::fs::create_dir_all(&s.out);
        let path = s.out.join(format!("{name}-{}.png", s.face));
        let write = || -> Result<(), Box<dyn std::error::Error>> {
            let file = std::fs::File::create(&path)?;
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), cw, ch);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&px)?;
            Ok(())
        };
        match write() {
            Ok(()) => eprintln!("shot: wrote {} ({cw}×{ch} px at {}×)", path.display(), self.scale),
            Err(e) => eprintln!("shot: {}: {e}", path.display()),
        }
    }
}
