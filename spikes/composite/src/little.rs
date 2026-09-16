//! Little nus: a link from another app opens here, in a small floating
//! window with our band and one page — keep it (Ctrl+Shift+O) and it
//! becomes a tab in the main window; Esc and it is gone. Also the single-
//! instance handoff that makes that possible, and default-browser
//! registration (Windows; the others are noted).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Instant;

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style, Target, Theme};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, PhysicalKey};
use winit::window::Window;

use crate::app::{App, WebPane};

// ── Single instance ──────────────────────────────────────────────────────
// The first instance listens on a loopback port written to
// profile/instance; a second one hands its URLs over and exits. v1 moves
// this to a named pipe / unix socket; the protocol is one URL per line.

pub enum Claim {
    /// We are the instance: URLs from later launches arrive here.
    Primary(Receiver<String>),
    /// Another instance took the URLs; exit.
    HandedOff,
}

fn instance_file() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("instance")
}

/// Claim the instance, handing `urls` to a running one if there is one.
pub fn claim(urls: &[String]) -> Claim {
    if let Ok(port) = std::fs::read_to_string(instance_file()).map(|s| s.trim().to_string()) {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap_or(0))) {
            let mut ok = true;
            for u in urls {
                ok &= writeln!(s, "{u}").is_ok();
            }
            if urls.is_empty() {
                ok &= writeln!(s, "raise").is_ok();
            }
            if ok {
                return Claim::HandedOff;
            }
        }
    }
    let (tx, rx) = channel();
    for u in urls {
        let _ = tx.send(u.clone());
    }
    match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => {
            let port = l.local_addr().map(|a| a.port()).unwrap_or(0);
            let _ = std::fs::create_dir_all(instance_file().parent().unwrap());
            let _ = std::fs::write(instance_file(), port.to_string());
            std::thread::spawn(move || {
                for conn in l.incoming().flatten() {
                    let r = BufReader::new(conn);
                    for line in r.lines().map_while(Result::ok) {
                        let _ = tx.send(line);
                    }
                }
            });
        }
        Err(e) => tracing::warn!("single instance: {e}"),
    }
    Claim::Primary(rx)
}

/// URLs on the command line (anything that parses as http(s) or a bare host).
pub fn urls_from_args() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .filter_map(|a| crate::app::strict_url(&a).or_else(|| if a.starts_with("http") { Some(a.clone()) } else { None }))
        .collect()
}

// ── The little window ────────────────────────────────────────────────────

pub struct Little {
    pub window: Arc<Window>,
    pub target: Target,
    pub scene: Scene,
    pub pane: WebPane,
    pub focused: bool,
    pub last_frame: Instant,
    pub mods: winit::keyboard::ModifiersState,
    /// Header hit rects: keep, close.
    pub keep: Rect,
    pub close: Rect,
}

pub const LITTLE_W: f64 = 560.0;
pub const LITTLE_H: f64 = 720.0;

impl App {
    /// A URL handed in from outside: open it little, or raise the window.
    pub fn open_little(&mut self, url: &str) {
        if url == "raise" {
            self.window.focus_window();
            return;
        }
        // Reuse the little window if it is up; else ask the host for one.
        if let Some(l) = self.little.as_mut() {
            l.pane.tab.load(url);
            l.window.focus_window();
            return;
        }
        self.little_request = Some(url.to_string());
    }

    pub fn attach_little(&mut self, window: Arc<Window>, url: &str) {
        let Ok(target) = self.gpu.target(window.clone()) else { return };
        let Some(pane) = self.new_web_pane(url) else { return };
        let scale = window.scale_factor() as f32;
        {
            let mut s = pane.tab.shared.borrow_mut();
            s.scale = scale;
        }
        let mut l = Little {
            window,
            target,
            scene: Scene::new(),
            pane,
            focused: true,
            last_frame: Instant::now(),
            mods: Default::default(),
            keep: Rect::new(0.0, 0.0, 0.0, 0.0),
            close: Rect::new(0.0, 0.0, 0.0, 0.0),
        };
        l.window.set_visible(true);
        l.window.focus_window();
        self.little_layout(&mut l);
        self.little = Some(l);
    }

    fn little_layout(&self, l: &mut Little) {
        let scale = l.window.scale_factor() as f32;
        let (w, h) = (l.target.size.0 as f32, l.target.size.1 as f32);
        let band = (4.0 * scale).round();
        let head = (34.0 * scale).round();
        l.pane.rect = Rect::new(0.0, band + head, w, h - band - head);
        l.pane.page = l.pane.rect;
        {
            let mut s = l.pane.tab.shared.borrow_mut();
            s.origin = (0.0, band + head);
            s.scale = scale;
            if let Ok(p) = l.window.outer_position() {
                s.window_pos = (p.x, p.y);
            }
        }
        l.pane.tab.resized((w / scale).floor(), ((h - band - head) / scale).floor());
        let isz = (16.0 * scale).round();
        l.close = Rect::new(w - 14.0 * scale - isz, band, isz + 14.0 * scale, head);
        l.keep = Rect::new(l.close.x - 14.0 * scale - isz - 8.0 * scale, band, isz + 22.0 * scale, head);
    }

    /// Draw the little window: band, header (title · keep · close), page.
    pub fn little_frame(&mut self) {
        let Some(mut l) = self.little.take() else { return };
        if l.last_frame.elapsed().as_millis() < 16 {
            self.little = Some(l);
            return;
        }
        l.last_frame = Instant::now();
        l.pane.tab.begin_frame();
        let scale = l.window.scale_factor() as f32;
        let (w, h) = (l.target.size.0 as f32, l.target.size.1 as f32);
        let t: Theme = self.theme.clone();
        let ink = t.ink;
        let band = (4.0 * scale).round();
        let head = (34.0 * scale).round();
        l.scene.clear();
        l.scene.layer(None);
        l.scene.rect(Rect::new(0.0, 0.0, w, band), self.surface.signal);
        if let (Some(kind), true) = (self.surface.texture_kind.shader_kind(), self.surface.texture > 0.0) {
            l.scene.push(nus_render::Instance::texture_kind(Rect::new(0.0, 0.0, w, band), kind, [1.0, 1.0, 1.0, self.surface.texture], self.surface.texture_scale * scale, 0.0));
        }
        let (title, url, bind) = {
            let s = l.pane.tab.shared.borrow();
            (s.title.clone(), s.url.clone(), s.bind.clone())
        };
        let label = Style { font: self.f.ui, px: (m::LABEL_PX * scale).round(), color: ink, tracking: m::LABEL_PX * scale * m::LABEL_TRACKING };
        let dim = Style { color: t.dim, ..label };
        let base = band + head / 2.0 + (m::LABEL_PX * scale) / 2.0 - 2.0 * scale;
        let isz = (16.0 * scale).round();
        let mut x = 14.0 * scale;
        self.fonts.draw_icon(&mut l.scene, icons::GLOBE, isz, x, base - isz + 2.0 * scale, ink);
        x += isz + 8.0 * scale;
        let shown = if title.is_empty() { url.clone() } else { title.clone() };
        let maxw = l.keep.x - 14.0 * scale - x;
        let mut s: String = shown.to_uppercase();
        while !s.is_empty() && self.fonts.measure(label, &format!("{s}…")) > maxw {
            s.pop();
        }
        let text = if s.len() < shown.len() { format!("{s}…") } else { s };
        self.fonts.draw(&mut l.scene, label, x, base, &text);
        // Keep: arrow-square-in. Close: x.
        self.fonts.draw_icon(&mut l.scene, icons::OPEN_EXTERNAL, isz, l.keep.x + 8.0 * scale, base - isz + 2.0 * scale, ink);
        self.fonts.draw_icon(&mut l.scene, icons::CLOSE, isz, l.close.x + 7.0 * scale, base - isz + 2.0 * scale, ink);
        l.scene.hline(0.0, band + head - scale, w, scale, ink);
        let _ = dim;
        // Page.
        l.scene.rect(l.pane.page, t.page);
        if let Some(b) = bind {
            l.scene.texture(l.pane.page, b, Some(l.pane.page));
            l.scene.layer(None);
        }
        if l.focused {
            let th = (m::FLOATING * scale).round();
            l.scene.push(nus_render::Instance::stroke(Rect::new(0.0, 0.0, w, h), 0.0, th, ink, None, 0.0));
        }
        l.scene.finish();
        for (x, y, w, h, data) in self.fonts.uploads.drain(..) {
            self.gpu.upload_glyph(x, y, w, h, &data);
        }
        let clear = self.paper();
        self.gpu.render(&mut l.target, &l.scene, clear);
        self.little = Some(l);
    }

    /// Keep: the page becomes a tab in the main window.
    pub fn keep_little(&mut self) {
        let Some(l) = self.little.take() else { return };
        let url = l.pane.tab.shared.borrow().url.clone();
        drop(l);
        if !url.is_empty() {
            self.open_url(&url, true);
        }
        self.window.focus_window();
    }

    pub fn close_little(&mut self) {
        self.little = None;
    }

    // Events from the little window.

    pub fn little_resized(&mut self, w: u32, h: u32) {
        let Some(mut l) = self.little.take() else { return };
        l.target.resize(&self.gpu.device, w, h);
        self.little_layout(&mut l);
        self.little = Some(l);
    }

    pub fn little_moved(&mut self, x: i32, y: i32) {
        if let Some(l) = self.little.as_mut() {
            l.pane.tab.shared.borrow_mut().window_pos = (x, y);
        }
    }

    pub fn little_focus(&mut self, f: bool) {
        if let Some(l) = self.little.as_mut() {
            l.focused = f;
            l.pane.tab.focus(f);
        }
    }

    pub fn little_modifiers(&mut self, m: winit::keyboard::ModifiersState) {
        if let Some(l) = self.little.as_mut() {
            l.mods = m;
        }
    }

    pub fn little_key(&mut self, ev: &KeyEvent) {
        let Some(l) = self.little.as_ref() else { return };
        let pressed = ev.state == ElementState::Pressed;
        let (ctrl, shift, sup) = (l.mods.control_key(), l.mods.shift_key(), l.mods.super_key());
        let app = if cfg!(target_os = "macos") { sup } else { ctrl && shift };
        if pressed {
            if ev.logical_key == WKey::Named(NamedKey::Escape) {
                return self.close_little();
            }
            if app && ev.physical_key == PhysicalKey::Code(KeyCode::KeyO) {
                return self.keep_little();
            }
        }
        crate::app::forward_key(&l.pane.tab, ev, l.mods);
    }

    pub fn little_mouse(&mut self, button: MouseButton, state: ElementState, pos: (f32, f32)) {
        let Some(l) = self.little.as_ref() else { return };
        let pressed = state == ElementState::Pressed;
        let (x, y) = pos;
        if pressed && button == MouseButton::Left && y < l.pane.rect.y {
            if l.keep.contains(x, y) {
                return self.keep_little();
            }
            if l.close.contains(x, y) {
                return self.close_little();
            }
            let _ = l.window.drag_window();
            return;
        }
        if l.pane.page.contains(x, y) || !pressed {
            let scale = l.window.scale_factor() as f32;
            let (lx, ly) = ((x - l.pane.page.x) / scale, (y - l.pane.page.y) / scale);
            let b = match button {
                MouseButton::Left => cef::MouseButtonType::LEFT,
                MouseButton::Right => cef::MouseButtonType::RIGHT,
                MouseButton::Middle => cef::MouseButtonType::MIDDLE,
                _ => return,
            };
            l.pane.tab.mouse_click(lx as i32, ly as i32, crate::app::cef_mods(l.mods), b, !pressed, 1);
        }
    }

    pub fn little_cursor(&mut self, pos: (f32, f32)) {
        let Some(l) = self.little.as_ref() else { return };
        let scale = l.window.scale_factor() as f32;
        let (lx, ly) = ((pos.0 - l.pane.page.x) / scale, (pos.1 - l.pane.page.y) / scale);
        l.pane.tab.mouse_move(lx as i32, ly as i32, crate::app::cef_mods(l.mods), false);
    }

    pub fn little_wheel(&mut self, delta: MouseScrollDelta, pos: (f32, f32)) {
        let Some(l) = self.little.as_ref() else { return };
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => ((x * 40.0) as i32, (y * 40.0) as i32),
            MouseScrollDelta::PixelDelta(p) => (p.x as i32, p.y as i32),
        };
        let scale = l.window.scale_factor() as f32;
        let (lx, ly) = ((pos.0 - l.pane.page.x) / scale, (pos.1 - l.pane.page.y) / scale);
        l.pane.tab.wheel(lx as i32, ly as i32, crate::app::cef_mods(l.mods), dx, dy);
    }
}

// ── Default browser ──────────────────────────────────────────────────────

/// Whether nus is registered as a browser choice (Windows: HKCU keys).
pub fn registered() -> bool {
    if !cfg!(target_os = "windows") {
        return false;
    }
    std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\RegisteredApplications", "/v", "nus"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Register nus with Windows as a browser (user scope, reversible), then
/// open Settings → Default apps so the user can pick it. Other OSes need
/// an app bundle / .desktop file (v1).
pub fn register() -> Result<(), String> {
    if !cfg!(target_os = "windows") {
        return Err("registration needs an app bundle (macOS) or .desktop file (Linux) — v1".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().to_string();
    let cmd = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    let steps: Vec<Vec<String>> = vec![
        vec![r"HKCU\Software\Classes\nusURL".into(), "/ve".into(), "/d".into(), "nus URL".into()],
        vec![r"HKCU\Software\Classes\nusURL".into(), "/v".into(), "URL Protocol".into(), "/d".into(), "".into()],
        vec![r"HKCU\Software\Classes\nusURL\DefaultIcon".into(), "/ve".into(), "/d".into(), icon.clone()],
        vec![r"HKCU\Software\Classes\nusURL\shell\open\command".into(), "/ve".into(), "/d".into(), cmd.clone()],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus".into(), "/ve".into(), "/d".into(), "nus".into()],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\DefaultIcon".into(), "/ve".into(), "/d".into(), icon],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\shell\open\command".into(), "/ve".into(), "/d".into(), format!("\"{exe}\"")],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\Capabilities".into(), "/v".into(), "ApplicationName".into(), "/d".into(), "nus".into()],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\Capabilities".into(), "/v".into(), "ApplicationDescription".into(), "/d".into(), "terminal and browser".into()],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\Capabilities\URLAssociations".into(), "/v".into(), "http".into(), "/d".into(), "nusURL".into()],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\Capabilities\URLAssociations".into(), "/v".into(), "https".into(), "/d".into(), "nusURL".into()],
        vec![r"HKCU\Software\Clients\StartMenuInternet\nus\Capabilities\FileAssociations".into(), "/v".into(), ".html".into(), "/d".into(), "nusURL".into()],
        vec![r"HKCU\Software\RegisteredApplications".into(), "/v".into(), "nus".into(), "/d".into(), r"Software\Clients\StartMenuInternet\nus\Capabilities".into()],
    ];
    for s in steps {
        let mut args = vec!["add".to_string(), s[0].clone()];
        args.extend(s[1..].iter().cloned());
        args.push("/f".into());
        let out = std::process::Command::new("reg").args(&args).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    let _ = std::process::Command::new("cmd").args(["/c", "start", "", "ms-settings:defaultapps"]).spawn();
    Ok(())
}

pub fn unregister() -> Result<(), String> {
    if !cfg!(target_os = "windows") {
        return Ok(());
    }
    for (key, value) in [
        (r"HKCU\Software\RegisteredApplications", Some("nus")),
        (r"HKCU\Software\Clients\StartMenuInternet\nus", None),
        (r"HKCU\Software\Classes\nusURL", None),
    ] {
        let mut args = vec!["delete", key];
        if let Some(v) = value {
            args.extend(["/v", v]);
        }
        args.push("/f");
        let _ = std::process::Command::new("reg").args(&args).output();
    }
    Ok(())
}
