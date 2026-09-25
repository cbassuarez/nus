//! Little nus: a link from another app opens here, in a small floating
//! window with our band and one page — keep it (Ctrl+Shift+O) and it
//! becomes a tab in the main window; Esc and it is gone. Also the single-
//! instance handoff that makes that possible, and default-browser
//! registration (Windows; the others are noted).

use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use nus_render::text::icons;
use nus_render::theme::metric as m;
use nus_render::{Rect, Scene, Style, Target, Theme};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key as WKey, KeyCode, NamedKey, PhysicalKey};
use winit::window::Window;

use crate::app::{Caps, App, WebPane};

// ── Single instance ──────────────────────────────────────────────────────
// The first instance listens on a loopback port written to
// profile/instance; a second one hands its URLs over and exits. v1 moves
// this to a named pipe / unix socket; all requests use authenticated JSON lines.

pub enum Claim {
    /// We are the instance: URLs from later launches arrive here; the
    /// port other processes reach us on.
    Primary(Receiver<Inbound>, u16, Sender<Inbound>),
}

fn instance_file() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_default().join("profile").join("instance")
}

/// What the instance port delivers: a URL to open (or "raise"), or a
/// remote-control request with its reply channel.
pub enum Inbound {
    Url(String),
    Request(crate::remote::Request),
}

/// Claim the instance, handing `urls` to a running one if there is one.
pub fn handoff(urls: &[String]) -> bool {
    if let Ok(instance) = std::fs::read_to_string(instance_file()) {
        let mut lines = instance.lines();
        let port = lines.next().unwrap_or("").parse::<u16>().unwrap_or(0);
        let token = lines.next().unwrap_or("");
        if !token.is_empty() {
            let addr = std::net::SocketAddr::from(([127,0,0,1],port));
            if let Ok(mut s) = TcpStream::connect_timeout(&addr, crate::security::IO_TIMEOUT) {
                let _ = s.set_read_timeout(Some(crate::security::IO_TIMEOUT));
                let _ = s.set_write_timeout(Some(crate::security::IO_TIMEOUT));
                let request = serde_json::json!({"token":token,"cmd":"__handoff","args":urls,"protocol":nus_compat::CLI_PROTOCOL});
                if writeln!(s, "{request}").is_ok() {
                    if let Ok(Some(reply)) = crate::security::line(&mut BufReader::new(s), 4096, Instant::now()+crate::security::IO_TIMEOUT) {
                        if serde_json::from_str::<serde_json::Value>(&reply).ok().is_some_and(|v| v["ok"] == true) { return true; }
                    }
                }
            }
        }
    }
    false
}

/// Called only while holding the profile lifetime lock.
pub fn claim(urls: &[String]) -> Claim {
    let token = crate::remote::new_token();
    let (rx, port, tx) = listen(urls, token.clone());
    if port != 0 {
        let _ = std::fs::create_dir_all(instance_file().parent().unwrap());
        // The port on the first line, the token on the second; the CLI reads both.
        if let Err(e) = crate::security::write_secret(&instance_file(), format!("{port}\n{token}\n").as_bytes()) {
            tracing::error!("Could not publish owner-only instance credentials: {e}");
        }
    }
    Claim::Primary(rx, port, tx)
}

/// Listen on loopback for authenticated launch handoff and remote-control
/// requests (JSON lines carrying the token); `urls` are queued first.
pub fn listen(urls: &[String], token: String) -> (Receiver<Inbound>, u16, Sender<Inbound>) {
    let (tx, rx) = channel();
    let tx_out = tx.clone();
    for u in urls {
        let _ = tx.send(Inbound::Url(u.clone()));
    }
    let mut port = 0;
    match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => {
            port = l.local_addr().map(|a| a.port()).unwrap_or(0);
            std::thread::spawn(move || {
                for conn in l.incoming().flatten() {
                    let Some(permit) = crate::security::Connection::acquire() else { continue };
                    let tx = tx.clone();
                    let token = token.clone();
                    std::thread::spawn(move || {
                        let _permit = permit;
                        let _ = conn.set_read_timeout(Some(crate::security::IO_TIMEOUT));
                        let _ = conn.set_write_timeout(Some(crate::security::IO_TIMEOUT));
                        use std::io::Write as _;
                        let mut w = match conn.try_clone() {
                            Ok(w) => w,
                            Err(_) => return,
                        };
                        let mut r = BufReader::new(conn);
                        for _ in 0..256 {
                            let Ok(Some(line)) = crate::security::line(&mut r, crate::security::MAX_REQUEST, Instant::now()+crate::security::IO_TIMEOUT) else { break };
                            if line.starts_with('{') {
                                let v: serde_json::Value = match serde_json::from_str(&line) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        let _ = writeln!(w, "{}", serde_json::json!({ "ok": false, "error": format!("bad json: {e}") }));
                                        continue;
                                    }
                                };
                                if !crate::security::token_matches(&token, v.get("token").and_then(|t| t.as_str()).unwrap_or("")) {
                                    let _ = writeln!(w, "{}", serde_json::json!({ "ok": false, "error": "bad token" }));
                                    break;
                                }
                                if !nus_compat::protocol_matches(v.get("protocol"), nus_compat::CLI_PROTOCOL) {
                                    let _ = writeln!(w, "{}", serde_json::json!({"ok":false,"error":"CLI_PROTOCOL_MISMATCH: use the CLI bundled with this nus", "protocol":nus_compat::CLI_PROTOCOL}));
                                    break;
                                }
                                let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("").to_string();
                                // A hand may wait on the user; the rest answers within seconds.
                                let patience = if cmd == "hands" { 180 } else { 10 };
                                let args = v.get("args").cloned().unwrap_or(serde_json::Value::Null);
                                if cmd == "__handoff" {
                                    let Some(urls) = args.as_array().filter(|a| a.len() <= 128) else { break };
                                    if urls.iter().any(|u| !u.as_str().is_some_and(|u| u.starts_with("file://") || crate::app::strict_url(u).is_some())) { break; }
                                    if urls.is_empty() { let _ = tx.send(Inbound::Url("raise".into())); }
                                    for u in urls { let _ = tx.send(Inbound::Url(u.as_str().unwrap().into())); }
                                    let _ = writeln!(w, "{{\"ok\":true}}");
                                    break;
                                }
                                let (reply_tx, reply_rx) = channel();
                                if tx.send(Inbound::Request(crate::remote::Request { cmd, args, reply: reply_tx, origin: crate::remote::Origin::Cli })).is_err() {
                                    break;
                                }
                                // A hand may wait on the user; the rest answers within seconds.
                                match reply_rx.recv_timeout(std::time::Duration::from_secs(patience)) {
                                    Ok(answer) => {
                                        let _ = writeln!(w, "{answer}");
                                    }
                                    Err(_) => {
                                        let _ = writeln!(w, "{}", serde_json::json!({ "ok": false, "error": "no answer" }));
                                    }
                                }
                                continue;
                            }
                            // Launch handoff uses authenticated JSON too.
                            break;
                        }
                    });
                }
            });
        }
        Err(e) => tracing::warn!("single instance: {e}"),
    }
    (rx, port, tx_out)
}

/// URLs on the command line (anything that parses as http(s) or a bare host).
/// URLs and files from the command line: `composite <url>` opens a little
/// window, `composite <file>` opens the editor (as `file://…`).
pub fn urls_from_args() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .filter_map(|a| {
            let p = std::path::Path::new(&a);
            if p.is_file() {
                let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_string();
                return Some(format!("file://{abs}"));
            }
            crate::app::strict_url(&a).or_else(|| if a.starts_with("http") { Some(a.clone()) } else { None })
        })
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
        if let Some(p) = url.strip_prefix("file://") {
            self.open_file(std::path::Path::new(p), false);
            self.window.focus_window();
            return;
        }
        if self.behavior.outside == crate::settings::Outside::NewTab {
            self.open_url_by_other(url);
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
        pane.tab.set_scale(scale);
        let mut l = Little {
            window,
            target,
            scene: Scene::new(),
            pane,
            focused: true,
            last_frame: crate::clock::now(),
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
            if let Ok(p) = l.window.outer_position() {
                s.window_pos = (p.x, p.y);
            }
        }
        l.pane.tab.set_scale(scale);
        l.pane.tab.resized((w / scale).floor(), ((h - band - head) / scale).floor());
        let isz = (16.0 * scale).round();
        l.close = Rect::new(w - 14.0 * scale - isz, band, isz + 14.0 * scale, head);
        l.keep = Rect::new(l.close.x - 14.0 * scale - isz - 8.0 * scale, band, isz + 22.0 * scale, head);
    }

    /// Draw the little window: band, header (title · keep · close), page.
    pub fn little_frame(&mut self) {
        let Some(mut l) = self.little.take() else { return };
        if crate::clock::since(l.last_frame).as_millis() < 16 {
            self.little = Some(l);
            return;
        }
        l.last_frame = crate::clock::now();
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
        let mut s: String = shown.caps();
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
        crate::app::forward_key(&l.pane.tab, &ev.into(), l.mods);
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

// ── Login item ───────────────────────────────────────────────────────────

fn startup_link() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(std::path::PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup\nus.lnk"))
}

/// macOS: a per-user LaunchAgent that opens the bundle at login.
fn launch_agent() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::PathBuf::from(home).join("Library/LaunchAgents/dev.nus.app.login.plist"))
}

/// Linux: an XDG autostart entry.
fn autostart_entry() -> Option<std::path::PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart/nus.desktop"))
}

/// Where this platform keeps the login entry.
fn login_entry() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "windows") { startup_link() } else if cfg!(target_os = "macos") { launch_agent() } else { autostart_entry() }
}

/// The LaunchAgent for `exe`. It opens the bundle, as Finder would, so the
/// app settles into its own profile; a bare binary (a development build)
/// runs as itself.
fn launch_agent_plist(exe: &str) -> String {
    let args: Vec<String> = match exe.find(".app/Contents/MacOS/") {
        Some(i) => vec!["/usr/bin/open".into(), "-a".into(), exe[..i + 4].to_string()],
        None => vec![exe.to_string()],
    };
    let args: String = args.iter().map(|a| format!("\n        <string>{}</string>", xml(a))).collect();
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.nus.app.login</string>
    <key>ProgramArguments</key>
    <array>{args}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#)
}

#[cfg(all(test, target_os = "macos"))]
mod login_tests {
    #[test]
    fn launch_agent_opens_the_bundle_and_is_a_valid_plist() {
        let plist = super::launch_agent_plist("/Applications/nus & co.app/Contents/MacOS/nus");
        assert!(plist.contains("<string>/Applications/nus &amp; co.app</string>"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.plist");
        std::fs::write(&path, &plist).unwrap();
        assert!(std::process::Command::new("plutil").arg("-lint").arg(&path).status().unwrap().success());
    }
}

pub fn login_item_registered() -> bool {
    login_entry().is_some_and(|p| p.exists())
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Start nus when you log in: a Startup shortcut (Windows), a LaunchAgent
/// (macOS) or an autostart entry (Linux). Off removes it; nothing else changes.
pub fn login_item(on: bool) -> Result<(), String> {
    let Some(entry) = login_entry() else { return Err("Couldn't find your user folder.".into()) };
    if !on {
        if cfg!(target_os = "macos") {
            let _ = std::process::Command::new("launchctl").arg("unload").arg(&entry).output();
        }
        return match std::fs::remove_file(&entry) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(dir) = entry.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
    if cfg!(target_os = "macos") {
        let plist = launch_agent_plist(&exe.to_string_lossy());
        return std::fs::write(&entry, plist).map_err(|e| e.to_string());
    }
    if !cfg!(target_os = "windows") {
        let desktop = format!("[Desktop Entry]\nType=Application\nName=nus\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n", exe.display());
        return std::fs::write(&entry, desktop).map_err(|e| e.to_string());
    }
    let link = entry;
    let script = format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; $s.WorkingDirectory = '{}'; $s.Save()",
        link.display(),
        exe.display(),
        exe.parent().map(|p| p.display().to_string()).unwrap_or_default()
    );
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
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
