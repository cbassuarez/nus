//! StatusNotifierItem transport. D-Bus work stays off the window event loop.
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
pub struct Tray(mpsc::Sender<State>);
impl Tray {
    pub fn new(proxy: EventLoopProxy<crate::UserEvent>, initial: State) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            use ksni::blocking::TrayMethods;
            let mut state = initial;
            let mut handle: Option<ksni::blocking::Handle<Item>> = None;
            let mut warned = false;
            loop {
                if !state.enabled {
                    if let Some(h) = handle.take() { h.shutdown().wait(); }
                } else if let Some(h) = &handle {
                    if h.update(|item| item.state = state).is_none() { handle = None; }
                } else {
                    match (Item { proxy: proxy.clone(), state }).assume_sni_available(true).spawn() {
                        Ok(h) => { handle = Some(h); warned = false; }
                        Err(e) if !warned => { tracing::warn!("nus tray unavailable: {e}; use the footer drawer"); warned = true; }
                        Err(_) => {}
                    }
                }
                // Retry a missing session bus without delaying the app or requiring a restart.
                match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(next) => { state = next; while let Ok(next) = rx.try_recv() { state = next; } }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            if let Some(h) = handle { h.shutdown().wait(); }
        });
        Self(tx)
    }
    pub fn update(&self, state: State) { let _ = self.0.send(state); }
}

struct Item { proxy: EventLoopProxy<crate::UserEvent>, state: State }
impl Item {
    fn send(&self, event: crate::UserEvent) { let _ = self.proxy.send_event(event); }
}
fn icon(state: State, size: u32) -> ksni::Icon {
    let badge = super::badge(state.work, state.style, false);
    let mut data = nus_render::dock_icon::tray(size, state.signal, badge.as_deref());
    for pixel in data.chunks_exact_mut(4) { pixel.rotate_right(1); }
    ksni::Icon { width: size as i32, height: size as i32, data }
}
impl ksni::Tray for Item {
    fn id(&self) -> String { "dev.nus.desktop".into() }
    fn title(&self) -> String { format!("nus · {}", self.state.work.short()) }
    fn status(&self) -> ksni::Status { if self.state.work.attention > 0 { ksni::Status::NeedsAttention } else { ksni::Status::Active } }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> { [24, 36, 48].into_iter().map(|size| icon(self.state, size)).collect() }
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
        let actual = icon(state, 24);
        assert_eq!(actual.data.len(), 24 * 24 * 4);
        for (rgba, argb) in expected.chunks_exact(4).zip(actual.data.chunks_exact(4)) {
            assert_eq!(argb, [rgba[3], rgba[0], rgba[1], rgba[2]]);
        }
    }
}
