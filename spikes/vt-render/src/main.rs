//! Spike 3: real PTY → nus-vt → glyph atlas → wgpu. See docs/SPIKES.md.

mod font;
mod gpu;

use std::sync::Arc;
use std::time::{Duration, Instant};

use nus_vt::input::{self, Key, KeyAction, Mods};
use nus_vt::{Cell, Color, CursorShape, Flags, Modes, Term};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use font::Font;
use gpu::{Gpu, Instance};

const FONT_PT: f32 = 13.0;
const SCROLLBACK: usize = 10_000;

#[derive(Debug)]
enum UserEvent {
    PtyOutput,
}

struct State {
    window: Arc<Window>,
    gpu: Gpu,
    font: Font,
    hb: rustybuzz::Face<'static>,
    term: Term,
    pty: nus_pty::Pty,
    cols: usize,
    rows: usize,
    mods: ModifiersState,
    // Instrumentation.
    last_key: Option<Instant>,
    frames: u64,
    frame_cpu: Duration,
    stress: bool,
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    state: Option<State>,
}

impl State {
    fn new(window: Arc<Window>, proxy: EventLoopProxy<UserEvent>) -> anyhow::Result<State> {
        let gpu = Gpu::new(window.clone())?;
        let scale = window.scale_factor() as f32;
        let mut font = Font::load(FONT_PT * scale * 96.0 / 72.0)?;
        let hb = font.hb_face();
        // Warm the atlas with ASCII so the first frame doesn't stall.
        let ascii: String = (32u8..127).map(|b| b as char).collect();
        for (id, ..) in font.shape(&hb, &ascii) {
            font.glyph(id);
        }
        let (cols, rows) = grid_size(&gpu, &font);
        let mut term = Term::new(cols, rows, SCROLLBACK);
        term.cell_px = (font.metrics.cell_w as u16, font.metrics.cell_h as u16);
        let profile = nus_pty::Profile::default_shell();
        let pty = nus_pty::Pty::spawn(&profile, cols as u16, rows as u16, move || {
            let _ = proxy.send_event(UserEvent::PtyOutput);
        })?;
        window.set_title(&format!("nus spike 3 — {} — {}", profile.name, font.name));
        Ok(State {
            window,
            gpu,
            font,
            hb,
            term,
            pty,
            cols,
            rows,
            mods: ModifiersState::empty(),
            last_key: None,
            frames: 0,
            frame_cpu: Duration::ZERO,
            stress: false,
        })
    }

    fn resize(&mut self, w: u32, h: u32) {
        self.gpu.resize(w, h);
        let (cols, rows) = grid_size(&self.gpu, &self.font);
        if (cols, rows) != (self.cols, self.rows) {
            self.cols = cols;
            self.rows = rows;
            self.term.resize(cols, rows);
            let px = (self.font.metrics.cell_w as u16, self.font.metrics.cell_h as u16);
            if let Err(e) = self.pty.resize(cols as u16, rows as u16, px) {
                tracing::warn!("pty resize: {e}");
            }
        }
    }

    /// Pull PTY output through the terminal. Returns true if anything changed.
    fn pump(&mut self) -> bool {
        let out = self.pty.take_output();
        let mut changed = false;
        if !out.is_empty() {
            self.term.advance(&out);
            changed = true;
        }
        self.term.tick();
        let responses = self.term.take_responses();
        if !responses.is_empty() {
            let _ = self.pty.write(&responses);
        }
        for ev in self.term.take_events() {
            match ev {
                nus_vt::Event::Title(t) => self.window.set_title(&format!("{t} — nus spike 3")),
                nus_vt::Event::Bell => tracing::info!("bell"),
                other => tracing::debug!("{other:?}"),
            }
        }
        changed
    }

    fn key(&mut self, event: &KeyEvent) {
        let action = match (event.state, event.repeat) {
            (ElementState::Released, _) => KeyAction::Release,
            (ElementState::Pressed, true) => KeyAction::Repeat,
            (ElementState::Pressed, false) => KeyAction::Press,
        };
        let mut mods = Mods::empty();
        mods.set(Mods::SHIFT, self.mods.shift_key());
        mods.set(Mods::CTRL, self.mods.control_key());
        mods.set(Mods::ALT, self.mods.alt_key());
        mods.set(Mods::SUPER, self.mods.super_key());

        // Spike-local chords.
        if action == KeyAction::Press && mods.contains(Mods::CTRL | Mods::SHIFT) {
            if let WKey::Character(c) = &event.logical_key {
                match c.as_str() {
                    "S" | "s" => {
                        self.stress = !self.stress;
                        tracing::info!("stress redraw: {}", self.stress);
                        self.window.request_redraw();
                        return;
                    }
                    "V" | "v" => return, // paste: not in this spike
                    _ => {}
                }
            }
        }

        let key = match &event.logical_key {
            WKey::Named(n) => match n {
                NamedKey::Enter => Key::Enter,
                NamedKey::Tab => Key::Tab,
                NamedKey::Backspace => Key::Backspace,
                NamedKey::Escape => Key::Escape,
                NamedKey::ArrowUp => Key::Up,
                NamedKey::ArrowDown => Key::Down,
                NamedKey::ArrowLeft => Key::Left,
                NamedKey::ArrowRight => Key::Right,
                NamedKey::Home => Key::Home,
                NamedKey::End => Key::End,
                NamedKey::PageUp => Key::PageUp,
                NamedKey::PageDown => Key::PageDown,
                NamedKey::Insert => Key::Insert,
                NamedKey::Delete => Key::Delete,
                NamedKey::Space => Key::Char(' '),
                NamedKey::F1 => Key::F(1),
                NamedKey::F2 => Key::F(2),
                NamedKey::F3 => Key::F(3),
                NamedKey::F4 => Key::F(4),
                NamedKey::F5 => Key::F(5),
                NamedKey::F6 => Key::F(6),
                NamedKey::F7 => Key::F(7),
                NamedKey::F8 => Key::F(8),
                NamedKey::F9 => Key::F(9),
                NamedKey::F10 => Key::F(10),
                NamedKey::F11 => Key::F(11),
                NamedKey::F12 => Key::F(12),
                _ => return,
            },
            WKey::Character(s) => match s.chars().next() {
                Some(c) => Key::Char(c),
                None => return,
            },
            _ => return,
        };
        let bytes = input::encode(key, mods, action, self.term.modes(), self.term.keyboard_mode());
        if bytes.is_empty() {
            return;
        }
        if self.term.grid().display_offset != 0 {
            self.term.grid_mut().scroll_display(-(SCROLLBACK as isize));
        }
        self.last_key = Some(Instant::now());
        if let Err(e) = self.pty.write(&bytes) {
            tracing::warn!("pty write: {e}");
        }
    }

    fn build_instances(&mut self) -> (Vec<Instance>, [f32; 4]) {
        let (cw, ch, baseline) = (self.font.metrics.cell_w, self.font.metrics.cell_h, self.font.metrics.baseline);
        let palette = &self.term.palette;
        let clear = to_f32(palette.get(nus_vt::palette::BG));
        let mut bg = Vec::with_capacity(self.cols * self.rows / 4);
        let mut fg = Vec::with_capacity(self.cols * self.rows);
        let grid = self.term.grid();
        let cursor = *self.term.cursor();
        let modes = self.term.modes();
        let show_cursor = modes.contains(Modes::SHOW_CURSOR) && grid.display_offset == 0;
        let cursor_shape = self.term.cursor_style().shape;
        let cursor_rgb = to_f32(palette.get(nus_vt::palette::CURSOR));

        let mut text = String::new();
        let mut col_of = Vec::new(); // byte offset -> column
        for r in 0..self.rows {
            let row = grid.visible_row(r);
            let y = r as f32 * ch;
            text.clear();
            col_of.clear();
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.flags.contains(Flags::WIDE_SPACER) {
                    continue;
                }
                let is_cursor = show_cursor && r == cursor.row && c == cursor.col;
                let (cfg, cbg) = resolve(cell, palette, is_cursor && cursor_shape == CursorShape::Block, cursor_rgb);
                if let Some(bgc) = cbg {
                    let w = if cell.flags.contains(Flags::WIDE) { 2.0 * cw } else { cw };
                    bg.push(Instance::rect(c as f32 * cw, y, w, ch, bgc));
                }
                if is_cursor && cursor_shape != CursorShape::Block {
                    let (cy, chh) = match cursor_shape {
                        CursorShape::Underline => (y + ch - 2.0, 2.0),
                        _ => (y, ch), // beam handled as width below
                    };
                    if cursor_shape == CursorShape::Beam {
                        bg.push(Instance::rect(c as f32 * cw, y, 2.0, ch, cursor_rgb));
                    } else {
                        bg.push(Instance::rect(c as f32 * cw, cy, cw, chh, cursor_rgb));
                    }
                }
                if cell.flags.intersects(Flags::ANY_UNDERLINE) && !cell.flags.contains(Flags::HIDDEN) {
                    let ulc = cell.ul.map(|u| to_f32(palette.resolve(u, true))).unwrap_or(cfg);
                    bg.push(Instance::rect(c as f32 * cw, y + baseline + 2.0, cw, 1.0, ulc));
                }
                if cell.flags.contains(Flags::STRIKE) {
                    bg.push(Instance::rect(c as f32 * cw, y + ch * 0.5, cw, 1.0, cfg));
                }
                if cell.ch != ' ' && !cell.flags.contains(Flags::HIDDEN) {
                    let start = text.len();
                    text.push(cell.ch);
                    for _ in start..text.len() {
                        col_of.push(c);
                    }
                } else {
                    // Keep byte→column mapping dense: a space is one byte.
                    text.push(' ');
                    col_of.push(c);
                }
            }
            if text.trim().is_empty() {
                continue;
            }
            for (id, cluster, xo, yo) in self.font.shape(&self.hb, &text) {
                let Some(g) = self.font.glyph(id) else { continue };
                let c = col_of[cluster as usize];
                let cell = &row.cells[c];
                let is_cursor = show_cursor && r == cursor.row && c == cursor.col;
                let (cfg, _) = resolve(cell, palette, is_cursor && cursor_shape == CursorShape::Block, cursor_rgb);
                let x = c as f32 * cw + xo + g.left as f32;
                let gy = y + baseline - yo - g.top as f32;
                fg.push(Instance::glyph(x, gy, g.width as f32, g.height as f32, g.uv, cfg));
            }
        }
        for (x, y, w, h, data) in self.font.uploads.drain(..) {
            self.gpu.upload_glyph(x, y, w, h, &data);
        }
        bg.extend(fg);
        (bg, clear)
    }

    fn redraw(&mut self) {
        let changed = self.pump();
        let damaged = self.term.grid_mut().is_damaged();
        let _ = self.term.grid_mut().take_damage();
        if !(changed || damaged || self.stress || self.frames == 0) {
            return;
        }
        let t0 = Instant::now();
        let (instances, clear) = self.build_instances();
        let built = t0.elapsed();
        self.window.pre_present_notify();
        let presented = self.gpu.render(&instances, clear);
        let cpu = t0.elapsed();
        self.frames += 1;
        self.frame_cpu += cpu;
        if let (Some(k), true) = (self.last_key.take(), presented && (changed || damaged)) {
            tracing::info!(
                "key→present {:.2} ms (build {:.2} ms, {} instances)",
                k.elapsed().as_secs_f64() * 1e3,
                built.as_secs_f64() * 1e3,
                instances.len()
            );
        }
        if self.frames % 240 == 0 {
            tracing::info!(
                "{} frames, avg cpu {:.2} ms/frame",
                self.frames,
                self.frame_cpu.as_secs_f64() * 1e3 / 240.0
            );
            self.frame_cpu = Duration::ZERO;
        }
        if self.stress {
            self.term.grid_mut().damage_all();
            self.window.request_redraw();
        }
    }
}

fn grid_size(gpu: &Gpu, font: &Font) -> (usize, usize) {
    let cols = (gpu.size.0 as f32 / font.metrics.cell_w).floor().max(2.0) as usize;
    let rows = (gpu.size.1 as f32 / font.metrics.cell_h).floor().max(1.0) as usize;
    (cols, rows)
}

fn to_f32(c: nus_vt::Rgb) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
}

/// Resolve a cell's fg and optional bg (None when it's the default bg, which
/// the clear color already painted).
fn resolve(cell: &Cell, palette: &nus_vt::Palette, cursor: bool, cursor_rgb: [f32; 4]) -> ([f32; 4], Option<[f32; 4]>) {
    let mut fg = cell.fg;
    let mut bg = cell.bg;
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
        if fg == Color::Default {
            fg = Color::Indexed(0);
        }
    }
    let mut fgc = to_f32(palette.resolve(fg, true));
    if cell.flags.contains(Flags::BOLD) {
        if let Color::Indexed(i @ 0..=7) = fg {
            fgc = to_f32(palette.get(i as usize + 8));
        }
    }
    if cell.flags.contains(Flags::DIM) {
        fgc = [fgc[0] * 0.6, fgc[1] * 0.6, fgc[2] * 0.6, 1.0];
    }
    if cursor {
        let bgc = to_f32(palette.resolve(Color::Default, false));
        return (bgc, Some(cursor_rgb));
    }
    let bgc = if bg == Color::Default && !cell.flags.contains(Flags::INVERSE) {
        None
    } else {
        Some(to_f32(palette.resolve(bg, false)))
    };
    (fgc, bgc)
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("nus spike 3")
            .with_inner_size(winit::dpi::LogicalSize::new(1000.0, 640.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        match State::new(window, self.proxy.clone()) {
            Ok(s) => self.state = Some(s),
            Err(e) => {
                tracing::error!("init failed: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        if let Some(s) = &self.state {
            s.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(s) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                s.resize(size.width, size.height);
                s.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = s.window.inner_size();
                s.resize(size.width, size.height);
            }
            WindowEvent::ModifiersChanged(m) => s.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => s.key(&event),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as isize,
                    MouseScrollDelta::PixelDelta(p) => (p.y / s.font.metrics.cell_h as f64) as isize,
                };
                s.term.grid_mut().scroll_display(lines);
                s.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                s.redraw();
                if s.pty.exit_code().is_some() {
                    tracing::info!("shell exited");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = App { proxy, state: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}
