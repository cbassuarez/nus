//! StatusNotifierItem transport. D-Bus work stays off the window event loop.
//!
//! While nus starts, the icon plays the launch the macOS Dock tile plays:
//! the letter cycles through its six faces on the same beat, and once the
//! window is up it finishes the pass and settles on the usual face. A
//! Linux dock draws the desktop entry's icon and has no way to animate a
//! running app's tile, so the tray is where it can be seen. The cycle runs
//! on this thread, so it keeps time while the main thread builds the window.
use super::super::menu_drawer::{Signal, SignalStyle};
use std::sync::mpsc;
use winit::event_loop::EventLoopProxy;

#[derive(Clone, Copy)]
pub struct State {
    pub signal: nus_render::Color,
    pub work: Signal,
    pub style: SignalStyle,
    pub enabled: bool,
}

enum Msg {
    State(State),
    /// Start the launch cycle.
    Launch,
    /// The window is up: finish the pass and settle.
    Ready,
}

/// The launch cycle, as the macOS Dock tile's (dock_launch.rs).
struct Cycle {
    index: usize,
    next: std::time::Instant,
    ready: bool,
}

pub struct Tray(mpsc::Sender<Msg>);
impl Tray {
    pub fn new(proxy: EventLoopProxy<crate::UserEvent>, initial: State) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            use ksni::blocking::TrayMethods;
            use nus_render::dock_icon::{Face, STEP_SECONDS};
            let step = std::time::Duration::from_secs_f32(STEP_SECONDS);
            let mut state = initial;
            let mut cycle: Option<Cycle> = None;
            let mut handle: Option<ksni::blocking::Handle<Item>> = None;
            let mut warned = false;
            loop {
                let face = cycle.as_ref().map(|c| c.index);
                if !state.enabled {
                    if let Some(h) = handle.take() { h.shutdown().wait(); }
                } else if let Some(h) = &handle {
                    if h.update(|item| { item.state = state; item.face = face; }).is_none() { handle = None; }
                } else {
                    match (Item { proxy: proxy.clone(), state, face }).assume_sni_available(true).spawn() {
                        Ok(h) => { handle = Some(h); warned = false; }
                        Err(e) if !warned => { tracing::warn!("nus tray unavailable: {e}; use the footer drawer"); warned = true; }
                        Err(_) => {}
                    }
                }
                // While launching, wake on the beat; otherwise every few
                // seconds, to retry a missing session bus without delaying
                // the app or requiring a restart.
                let wait = match &cycle {
                    Some(c) => c.next.saturating_duration_since(std::time::Instant::now()),
                    None => std::time::Duration::from_secs(5),
                };
                match rx.recv_timeout(wait) {
                    Ok(first) => {
                        for msg in std::iter::once(first).chain(rx.try_iter()) {
                            match msg {
                                Msg::State(s) => state = s,
                                Msg::Launch => cycle = Some(Cycle { index: 0, next: std::time::Instant::now() + step, ready: false }),
                                Msg::Ready => {
                                    if let Some(c) = cycle.as_mut() { c.ready = true; }
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(c) = cycle.as_mut() {
                            if std::time::Instant::now() >= c.next {
                                if c.ready && c.index == Face::ALL.len() - 1 {
                                    cycle = None;
                                } else {
                                    c.index = (c.index + 1) % Face::ALL.len();
                                    c.next = std::time::Instant::now() + step;
                                }
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            if let Some(h) = handle { h.shutdown().wait(); }
        });
        Self(tx)
    }
    pub fn update(&self, state: State) { let _ = self.0.send(Msg::State(state)); }
    pub fn launch(&self) { let _ = self.0.send(Msg::Launch); }
    pub fn ready(&self) { let _ = self.0.send(Msg::Ready); }
}

struct Item { proxy: EventLoopProxy<crate::UserEvent>, state: State, face: Option<usize> }
impl Item {
    fn send(&self, event: crate::UserEvent) { let _ = self.proxy.send_event(event); }
}
fn icon(state: State, size: u32, face: Option<usize>) -> ksni::Icon {
    let badge = super::badge(state.work, state.style, false);
    let mut data = match face {
        // A launch frame: the face alone, as the Dock tile shows it.
        Some(i) => nus_render::dock_icon::render(size, state.signal, nus_render::dock_icon::Face::ALL[i % nus_render::dock_icon::Face::ALL.len()]),
        None => nus_render::dock_icon::tray(size, state.signal, badge.as_deref()),
    };
    for pixel in data.chunks_exact_mut(4) { pixel.rotate_right(1); }
    ksni::Icon { width: size as i32, height: size as i32, data }
}
impl ksni::Tray for Item {
    fn id(&self) -> String { "dev.nus.desktop".into() }
    fn title(&self) -> String { format!("nus · {}", self.state.work.short()) }
    fn status(&self) -> ksni::Status { if self.state.work.attention > 0 { ksni::Status::NeedsAttention } else { ksni::Status::Active } }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> { [24, 36, 48].into_iter().map(|size| icon(self.state, size, self.face)).collect() }
    fn tool_tip(&self) -> ksni::ToolTip { ksni::ToolTip { title: "nus".into(), description: self.state.work.text(), ..Default::default() } }
    fn activate(&mut self, _x: i32, _y: i32) {
        // Hosts disagree on the coordinate space; the drawer uses a known display anchor.
        self.send(crate::UserEvent::MenuDrawer(None));
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool { true }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem { label: format!("Open nus drawer · {}", self.state.work.short()), activate: Box::new(|s: &mut Self| s.send(crate::UserEvent::MenuDrawer(None))), ..Default::default() }.into(),
            StandardItem { label: "Show / hide Hatch".into(), activate: Box::new(|s: &mut Self| s.send(crate::UserEvent::Hatch)), ..Default::default() }.into(),
            StandardItem { label: "Open nus window".into(), activate: Box::new(|s: &mut Self| s.send(crate::UserEvent::HatchMain)), ..Default::default() }.into(),
            StandardItem { label: "Quit nus".into(), activate: Box::new(|s: &mut Self| s.send(crate::UserEvent::HatchQuit)), ..Default::default() }.into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixmap_preserves_theme_and_converts_rgba_to_argb() {
        let state = State { signal: [0.2, 0.7, 0.8, 1.0], work: Signal::default(), style: SignalStyle::Dot, enabled: true };
        let expected = nus_render::dock_icon::tray(24, state.signal, None);
        let actual = icon(state, 24, None);
        assert_eq!(actual.data.len(), 24 * 24 * 4);
        for (rgba, argb) in expected.chunks_exact(4).zip(actual.data.chunks_exact(4)) {
            assert_eq!(argb, [rgba[3], rgba[0], rgba[1], rgba[2]]);
        }
    }
}
